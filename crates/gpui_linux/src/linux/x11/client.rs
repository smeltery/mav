use anyhow::{Context as _, anyhow};
use ashpd::WindowIdentifier;
use calloop::{
    EventLoop, LoopHandle, RegistrationToken,
    generic::{FdWrapper, Generic},
};
use collections::HashMap;
use core::str;
use gpui::{Capslock, profiler};
use gpui_util::ResultExt as _;
use http_client::Url;
use log::Level;
use smallvec::SmallVec;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashSet},
    ops::Deref,
    path::PathBuf,
    rc::{Rc, Weak},
    time::{Duration, Instant},
};

use x11rb::{
    connection::{Connection, RequestConnection},
    cursor,
    errors::ConnectionError,
    protocol::randr::ConnectionExt as _,
    protocol::xinput::ConnectionExt,
    protocol::xkb::ConnectionExt as _,
    protocol::xproto::{
        AtomEnum, ChangeWindowAttributesAux, ClientMessageData, ClientMessageEvent,
        ConnectionExt as _, EventMask, Visibility,
    },
    protocol::{Event, dri3, randr, render, xinput, xkb, xproto},
    resource_manager::Database,
    wrapper::ConnectionExt as _,
    xcb_ffi::XCBConnection,
};
use xim::{AttributeName, Client, InputStyle, x11rb::X11rbClient};
use xkbc::x11::ffi::{XKB_X11_MIN_MAJOR_XKB_VERSION, XKB_X11_MIN_MINOR_XKB_VERSION};
use xkbcommon::xkb::{self as xkbc, STATE_LAYOUT_EFFECTIVE};

use super::{
    ButtonOrScroll, ScrollDirection, X11Display, X11WindowStatePtr, XcbAtoms, XimCallbackEvent,
    XimHandler, button_or_scroll_from_event_detail, check_reply,
    clipboard::{self, Clipboard},
    get_reply, get_valuator_axis_index, handle_connection_error, modifiers_from_state,
    pressed_button_from_mask, xcb_flush,
};

use crate::linux::{
    DEFAULT_CURSOR_ICON_NAME, LinuxClient, capslock_from_xkb, cursor_style_to_icon_names,
    get_xkb_compose_state, is_within_click_distance, keystroke_from_xkb,
    keystroke_underlying_dead_key, log_cursor_icon_warning, modifiers_from_xkb, open_uri_internal,
    platform::{DOUBLE_CLICK_INTERVAL, SCROLL_LINES},
    reveal_path_internal,
    xdg_desktop_portal::{Event as XDPEvent, XDPEventSource},
};
use crate::linux::{LinuxCommon, LinuxKeyboardLayout, X11Window, modifiers_from_xinput_info};

use gpui::{
    AnyWindowHandle, Bounds, ClipboardItem, CursorStyle, DisplayId, FileDropEvent, Keystroke,
    Modifiers, ModifiersChangedEvent, MouseButton, Pixels, PlatformDisplay, PlatformInput,
    PlatformKeyboardLayout, PlatformWindow, Point, RequestFrameOptions, ScrollDelta, Size,
    TouchPhase, WindowButtonLayout, WindowParams, point, px,
};
use gpui_wgpu::{CompositorGpuHint, GpuContext};

/// Value for DeviceId parameters which selects all devices.
pub(crate) const XINPUT_ALL_DEVICES: xinput::DeviceId = 0;

/// Value for DeviceId parameters which selects all device groups. Events that
/// occur within the group are emitted by the group itself.
///
/// In XInput 2's interface, these are referred to as "master devices", but that
/// terminology is both archaic and unclear.
pub(crate) const XINPUT_ALL_DEVICE_GROUPS: xinput::DeviceId = 1;

const GPUI_X11_SCALE_FACTOR_ENV: &str = "GPUI_X11_SCALE_FACTOR";

pub(crate) struct WindowRef {
    window: X11WindowStatePtr,
    refresh_state: Option<RefreshState>,
    expose_event_received: bool,
    last_visibility: Visibility,
    is_mapped: bool,
}
#[path = "client/client_core.rs"]
mod client_core;
#[path = "client/dpi.rs"]
mod dpi;
#[path = "client/event_dispatch.rs"]
mod event_dispatch;
#[path = "client/event_ime.rs"]
mod event_ime;
#[path = "client/event_xinput.rs"]
mod event_xinput;
#[path = "client/helpers.rs"]
mod helpers;
#[path = "client/linux_client.rs"]
mod linux_client;
#[path = "client/state_ptr.rs"]
mod state_ptr;
#[path = "client/state_refresh.rs"]
mod state_refresh;
#[path = "client/tests.rs"]
mod tests;

use dpi::{get_scale_factor, xkb_state_for_key_event};
use helpers::{
    check_compositor_present, check_gtk_frame_extents_supported, create_invisible_cursor,
    current_pointer_device_states, detect_compositor_gpu, get_scroll_delta_and_update_state,
    is_pointer_device, make_scroll_wheel_event, mode_refresh_rate,
    reset_all_pointer_device_scroll_positions, reset_pointer_device_scroll_positions,
    xdnd_get_supported_atom, xdnd_is_atom_supported, xdnd_send_finished, xdnd_send_status,
};
pub(crate) use state_ptr::X11Client;

impl WindowRef {
    pub fn handle(&self) -> AnyWindowHandle {
        self.window.state.borrow().handle
    }
}

impl Deref for WindowRef {
    type Target = X11WindowStatePtr;

    fn deref(&self) -> &Self::Target {
        &self.window
    }
}

enum RefreshState {
    Hidden {
        refresh_rate: Duration,
    },
    PeriodicRefresh {
        refresh_rate: Duration,
        event_loop_token: RegistrationToken,
    },
}

#[derive(Debug)]
#[non_exhaustive]
pub enum EventHandlerError {
    XCBConnectionError(ConnectionError),
    XIMClientError(xim::ClientError),
}

impl std::error::Error for EventHandlerError {}

impl std::fmt::Display for EventHandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventHandlerError::XCBConnectionError(err) => err.fmt(f),
            EventHandlerError::XIMClientError(err) => err.fmt(f),
        }
    }
}

impl From<ConnectionError> for EventHandlerError {
    fn from(err: ConnectionError) -> Self {
        EventHandlerError::XCBConnectionError(err)
    }
}

impl From<xim::ClientError> for EventHandlerError {
    fn from(err: xim::ClientError) -> Self {
        EventHandlerError::XIMClientError(err)
    }
}

#[derive(Debug, Default)]
pub struct Xdnd {
    other_window: xproto::Window,
    drag_type: u32,
    retrieved: bool,
    position: Point<Pixels>,
}

#[derive(Debug)]
struct PointerDeviceState {
    horizontal: ScrollAxisState,
    vertical: ScrollAxisState,
}

#[derive(Debug, Default)]
struct ScrollAxisState {
    /// Valuator number for looking up this axis's scroll value.
    valuator_number: Option<u16>,
    /// Conversion factor from scroll units to lines.
    multiplier: f32,
    /// Last scroll value for calculating scroll delta.
    ///
    /// This gets set to `None` whenever it might be invalid - when devices change or when window focus changes.
    /// The logic errs on the side of invalidating this, since the consequence is just skipping the delta of one scroll event.
    /// The consequence of not invalidating it can be large invalid deltas, which are much more user visible.
    scroll_value: Option<f32>,
}

pub struct X11ClientState {
    pub(crate) loop_handle: LoopHandle<'static, X11Client>,
    pub(crate) event_loop: Option<calloop::EventLoop<'static, X11Client>>,

    pub(crate) last_click: Instant,
    pub(crate) last_mouse_button: Option<MouseButton>,
    pub(crate) last_location: Point<Pixels>,
    pub(crate) current_count: usize,
    pub(crate) pinch_scale: f32,

    pub(crate) gpu_context: GpuContext,
    pub(crate) compositor_gpu: Option<CompositorGpuHint>,

    pub(crate) scale_factor: f32,

    xkb_context: xkbc::Context,
    pub(crate) xcb_connection: Rc<XCBConnection>,
    xkb_device_id: i32,
    client_side_decorations_supported: bool,
    pub(crate) x_root_index: usize,
    pub(crate) resource_database: Database,
    pub(crate) atoms: XcbAtoms,
    pub(crate) windows: HashMap<xproto::Window, WindowRef>,
    pub(crate) mouse_focused_window: Option<xproto::Window>,
    pub(crate) keyboard_focused_window: Option<xproto::Window>,
    pub(crate) xkb: xkbc::State,
    keyboard_layout: LinuxKeyboardLayout,
    pub(crate) ximc: Option<X11rbClient<Rc<XCBConnection>>>,
    pub(crate) xim_handler: Option<XimHandler>,
    pub modifiers: Modifiers,
    pub capslock: Capslock,
    // TODO: Can the other updates to `modifiers` be removed so that this is unnecessary?
    // capslock logic was done analog to modifiers
    pub last_modifiers_changed_event: Modifiers,
    pub last_capslock_changed_event: Capslock,

    pub(crate) compose_state: Option<xkbc::compose::State>,
    pub(crate) pre_edit_text: Option<String>,
    pub(crate) composing: bool,
    pub(crate) pre_key_char_down: Option<Keystroke>,
    pub(crate) cursor_handle: cursor::Handle,
    pub(crate) cursor_styles: HashMap<xproto::Window, CursorStyle>,
    pub(crate) cursor_cache: HashMap<CursorStyle, Option<xproto::Cursor>>,
    pub(crate) invisible_cursor_cache: Option<xproto::Cursor>,
    pub(crate) cursor_hidden_window: Option<xproto::Window>,

    pointer_device_states: BTreeMap<xinput::DeviceId, PointerDeviceState>,

    pub(crate) supports_xinput_gestures: bool,

    pub(crate) common: LinuxCommon,
    pub(crate) clipboard: Clipboard,
    pub(crate) clipboard_item: Option<ClipboardItem>,
    pub(crate) xdnd_state: Xdnd,
}

#[derive(Clone)]
pub struct X11ClientStatePtr(pub Weak<RefCell<X11ClientState>>);
