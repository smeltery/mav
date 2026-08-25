use anyhow::{Context as _, anyhow};
use x11rb::connection::RequestConnection;

use crate::linux::X11ClientStatePtr;
use gpui::{
    AnyWindowHandle, Bounds, Decorations, DevicePixels, ForegroundExecutor, GpuSpecs, Modifiers,
    Pixels, PlatformAtlas, PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformWindow,
    Point, PromptButton, PromptLevel, RequestFrameOptions, ResizeEdge, ScaledPixels, Scene, Size,
    Tiling, WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowControlArea,
    WindowDecorations, WindowKind, WindowParams, px,
};
use gpui_wgpu::{CompositorGpuHint, WgpuRenderer, WgpuSurfaceConfig};

use collections::FxHashSet;
use gpui_util::{ResultExt, maybe};
use raw_window_handle as rwh;
use x11rb::{
    connection::Connection,
    cookie::{Cookie, VoidCookie},
    errors::ConnectionError,
    properties::WmSizeHints,
    protocol::{
        sync,
        xinput::{self, ConnectionExt as _},
        xproto::{self, ClientMessageEvent, ConnectionExt, TranslateCoordinatesReply},
    },
    wrapper::ConnectionExt as _,
    xcb_ffi::XCBConnection,
};

use std::{
    cell::RefCell, ffi::c_void, fmt::Display, num::NonZeroU32, ptr::NonNull, rc::Rc, sync::Arc,
};

use super::{X11Display, XINPUT_ALL_DEVICE_GROUPS, XINPUT_ALL_DEVICES};

use window_impl::WmHintPropertyState;

mod accesskit;
mod decorations;
mod platform;
mod raw_handles;
mod state;
mod state_ptr;
mod visuals;
mod window_impl;

use self::visuals::{
    EdgeConstraints, find_visuals, query_render_extent, resize_edge_to_moveresize,
};

x11rb::atom_manager! {
    pub XcbAtoms: AtomsCookie {
        XA_ATOM,
        XdndAware,
        XdndStatus,
        XdndEnter,
        XdndLeave,
        XdndPosition,
        XdndSelection,
        XdndDrop,
        XdndFinished,
        XdndTypeList,
        XdndActionCopy,
        TextUriList: b"text/uri-list",
        UTF8_STRING,
        TEXT,
        STRING,
        TEXT_PLAIN_UTF8: b"text/plain;charset=utf-8",
        TEXT_PLAIN: b"text/plain",
        XDND_DATA,
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        WM_CHANGE_STATE,
        WM_TRANSIENT_FOR,
        _NET_WM_PID,
        _NET_WM_NAME,
        _NET_WM_ICON,
        _NET_WM_STATE,
        _NET_WM_STATE_MAXIMIMAV_VERT,
        _NET_WM_STATE_MAXIMIMAV_HORZ,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_STATE_FOCUSED,
        _NET_ACTIVE_WINDOW,
        _NET_WM_SYNC_REQUEST,
        _NET_WM_SYNC_REQUEST_COUNTER,
        _NET_WM_BYPASS_COMPOSITOR,
        _NET_WM_MOVERESIZE,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_NOTIFICATION,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_WM_STATE_MODAL,
        _NET_WM_SYNC,
        _NET_SUPPORTED,
        _MOTIF_WM_HINTS,
        _GTK_SHOW_WINDOW_MENU,
        _GTK_FRAME_EXTENTS,
        _GTK_EDGE_CONSTRAINTS,
        _NET_CLIENT_LIST_STACKING,
    }
}

#[derive(Debug, Clone, Copy)]
struct RawWindow {
    connection: *mut c_void,
    screen_id: usize,
    window_id: u32,
    visual_id: u32,
}

// Safety: The raw pointers in RawWindow point to X11 connection
// which is valid for the window's lifetime. These are used only for
// passing to wgpu which needs Send+Sync for surface creation.
unsafe impl Send for RawWindow {}
unsafe impl Sync for RawWindow {}

#[derive(Default)]
pub struct Callbacks {
    request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    input: Option<Box<dyn FnMut(PlatformInput) -> gpui::DispatchEventResult>>,
    active_status_change: Option<Box<dyn FnMut(bool)>>,
    hovered_status_change: Option<Box<dyn FnMut(bool)>>,
    resize: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    moved: Option<Box<dyn FnMut()>>,
    should_close: Option<Box<dyn FnMut() -> bool>>,
    close: Option<Box<dyn FnOnce()>>,
    appearance_changed: Option<Box<dyn FnMut()>>,
    button_layout_changed: Option<Box<dyn FnMut()>>,
}

pub struct X11WindowState {
    pub destroyed: bool,
    parent: Option<X11WindowStatePtr>,
    children: FxHashSet<xproto::Window>,
    client: X11ClientStatePtr,
    executor: ForegroundExecutor,
    atoms: XcbAtoms,
    x_root_window: xproto::Window,
    x_screen_index: usize,
    visual_id: u32,
    pub(crate) counter_id: sync::Counter,
    pub(crate) last_sync_counter: Option<sync::Int64>,
    bounds: Bounds<Pixels>,
    scale_factor: f32,
    renderer: WgpuRenderer,
    display: Rc<dyn PlatformDisplay>,
    input_handler: Option<PlatformInputHandler>,
    appearance: WindowAppearance,
    background_appearance: WindowBackgroundAppearance,
    maximized_vertical: bool,
    maximized_horizontal: bool,
    hidden: bool,
    active: bool,
    hovered: bool,
    pub(crate) force_render_after_recovery: bool,
    fullscreen: bool,
    client_side_decorations_supported: bool,
    decorations: WindowDecorations,
    edge_constraints: Option<EdgeConstraints>,
    pub handle: AnyWindowHandle,
    last_insets: [u32; 4],
    accesskit_adapter: Option<accesskit_unix::Adapter>,
}

impl X11WindowState {
    fn is_transparent(&self) -> bool {
        self.background_appearance != WindowBackgroundAppearance::Opaque
    }
}

#[derive(Clone)]
pub(crate) struct X11WindowStatePtr {
    pub state: Rc<RefCell<X11WindowState>>,
    pub(crate) callbacks: Rc<RefCell<Callbacks>>,
    xcb: Rc<XCBConnection>,
    pub(crate) x_window: xproto::Window,
}

pub(crate) fn xcb_flush(xcb: &XCBConnection) {
    xcb.flush()
        .map_err(handle_connection_error)
        .context("X11 flush failed")
        .log_err();
}

pub(crate) fn check_reply<E, F, C>(
    failure_context: F,
    result: Result<VoidCookie<'_, C>, ConnectionError>,
) -> anyhow::Result<()>
where
    E: Display + Send + Sync + 'static,
    F: FnOnce() -> E,
    C: RequestConnection,
{
    result
        .map_err(handle_connection_error)
        .and_then(|response| response.check().map_err(|reply_error| anyhow!(reply_error)))
        .with_context(failure_context)
}

pub(crate) fn get_reply<E, F, C, O>(
    failure_context: F,
    result: Result<Cookie<'_, C, O>, ConnectionError>,
) -> anyhow::Result<O>
where
    E: Display + Send + Sync + 'static,
    F: FnOnce() -> E,
    C: RequestConnection,
    O: x11rb::x11_utils::TryParse,
{
    result
        .map_err(handle_connection_error)
        .and_then(|response| response.reply().map_err(|reply_error| anyhow!(reply_error)))
        .with_context(failure_context)
}

/// Convert X11 connection errors to `anyhow::Error` and panic for unrecoverable errors.
pub(crate) fn handle_connection_error(err: ConnectionError) -> anyhow::Error {
    match err {
        ConnectionError::UnknownError => anyhow!("X11 connection: Unknown error"),
        ConnectionError::UnsupportedExtension => anyhow!("X11 connection: Unsupported extension"),
        ConnectionError::MaximumRequestLengthExceeded => {
            anyhow!("X11 connection: Maximum request length exceeded")
        }
        ConnectionError::FdPassingFailed => {
            panic!("X11 connection: File descriptor passing failed")
        }
        ConnectionError::ParseError(parse_error) => {
            anyhow!(parse_error).context("Parse error in X11 response")
        }
        ConnectionError::InsufficientMemory => panic!("X11 connection: Insufficient memory"),
        ConnectionError::IoError(err) => anyhow!(err).context("X11 connection: IOError"),
        _ => anyhow!(err),
    }
}

pub(crate) struct X11Window(pub X11WindowStatePtr);
