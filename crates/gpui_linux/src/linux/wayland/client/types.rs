use super::*;

pub(super) fn take_startup_activation_token_from_environment() -> Option<String> {
    let startup_activation_token = std::env::var(XDG_ACTIVATION_TOKEN_ENV_VAR)
        .ok()
        .filter(|token| !token.is_empty());
    // The token must be removed from the environment so it isn't inherited by child
    // processes we spawn, per the xdg-activation spec: https://wayland.app/protocols/xdg-activation-v1
    // SAFETY: This runs during Wayland platform initialization before GPUI starts
    // concurrent environment access or spawning child processes.
    unsafe { std::env::remove_var(XDG_ACTIVATION_TOKEN_ENV_VAR) };
    startup_activation_token
}

#[derive(Clone)]
pub struct Globals {
    pub qh: QueueHandle<WaylandClientStatePtr>,
    pub activation: Option<xdg_activation_v1::XdgActivationV1>,
    pub compositor: wl_compositor::WlCompositor,
    pub cursor_shape_manager: Option<wp_cursor_shape_manager_v1::WpCursorShapeManagerV1>,
    pub data_device_manager: Option<wl_data_device_manager::WlDataDeviceManager>,
    pub primary_selection_manager:
        Option<zwp_primary_selection_device_manager_v1::ZwpPrimarySelectionDeviceManagerV1>,
    pub wm_base: xdg_wm_base::XdgWmBase,
    pub shm: wl_shm::WlShm,
    pub seat: wl_seat::WlSeat,
    pub viewporter: Option<wp_viewporter::WpViewporter>,
    pub fractional_scale_manager:
        Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    pub decoration_manager: Option<zxdg_decoration_manager_v1::ZxdgDecorationManagerV1>,
    pub layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    pub blur_manager: Option<org_kde_kwin_blur_manager::OrgKdeKwinBlurManager>,
    pub text_input_manager: Option<zwp_text_input_manager_v3::ZwpTextInputManagerV3>,
    pub gesture_manager: Option<zwp_pointer_gestures_v1::ZwpPointerGesturesV1>,
    pub dialog: Option<xdg_wm_dialog_v1::XdgWmDialogV1>,
    pub system_bell: Option<xdg_system_bell_v1::XdgSystemBellV1>,
    pub executor: ForegroundExecutor,
}

impl Globals {
    fn new(
        globals: GlobalList,
        executor: ForegroundExecutor,
        qh: QueueHandle<WaylandClientStatePtr>,
        seat: wl_seat::WlSeat,
    ) -> Self {
        let dialog_v = XdgWmDialogV1::interface().version;
        Globals {
            activation: globals.bind(&qh, 1..=1, ()).ok(),
            compositor: globals
                .bind(
                    &qh,
                    wl_surface::REQ_SET_BUFFER_SCALE_SINCE
                        ..=wl_surface::EVT_PREFERRED_BUFFER_SCALE_SINCE,
                    (),
                )
                .unwrap(),
            cursor_shape_manager: globals.bind(&qh, 1..=1, ()).ok(),
            data_device_manager: globals
                .bind(
                    &qh,
                    WL_DATA_DEVICE_MANAGER_VERSION..=WL_DATA_DEVICE_MANAGER_VERSION,
                    (),
                )
                .ok(),
            primary_selection_manager: globals.bind(&qh, 1..=1, ()).ok(),
            shm: globals.bind(&qh, 1..=1, ()).unwrap(),
            seat,
            wm_base: globals.bind(&qh, 1..=5, ()).unwrap(),
            viewporter: globals.bind(&qh, 1..=1, ()).ok(),
            fractional_scale_manager: globals.bind(&qh, 1..=1, ()).ok(),
            decoration_manager: globals.bind(&qh, 1..=1, ()).ok(),
            layer_shell: globals.bind(&qh, 1..=5, ()).ok(),
            blur_manager: globals.bind(&qh, 1..=1, ()).ok(),
            text_input_manager: globals.bind(&qh, 1..=1, ()).ok(),
            gesture_manager: globals.bind(&qh, 1..=3, ()).ok(),
            dialog: globals.bind(&qh, dialog_v..=dialog_v, ()).ok(),
            system_bell: globals.bind(&qh, 1..=1, ()).ok(),
            executor,
            qh,
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InProgressOutput {
    name: Option<String>,
    scale: Option<i32>,
    position: Option<Point<DevicePixels>>,
    size: Option<Size<DevicePixels>>,
    subpixel: Option<wl_output::Subpixel>,
}

impl InProgressOutput {
    fn complete(&self) -> Option<Output> {
        if let Some((position, size)) = self.position.zip(self.size) {
            let scale = self.scale.unwrap_or(1);
            Some(Output {
                name: self.name.clone(),
                scale,
                bounds: Bounds::new(position, size),
                subpixel: self.subpixel,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Output {
    pub name: Option<String>,
    pub scale: i32,
    pub bounds: Bounds<DevicePixels>,
    pub subpixel: Option<wl_output::Subpixel>,
}

pub(crate) struct WaylandClientState {
    pub(super) serial_tracker: SerialTracker,
    pub(super) globals: Globals,
    pub gpu_context: GpuContext,
    pub compositor_gpu: Option<CompositorGpuHint>,
    pub(super) wl_seat: wl_seat::WlSeat, // TODO: Multi seat support
    pub(super) wl_pointer: Option<wl_pointer::WlPointer>,
    pub(super) pinch_gesture: Option<zwp_pointer_gesture_pinch_v1::ZwpPointerGesturePinchV1>,
    pub(super) pinch_scale: f32,
    pub(super) wl_keyboard: Option<wl_keyboard::WlKeyboard>,
    pub(super) cursor_shape_device: Option<wp_cursor_shape_device_v1::WpCursorShapeDeviceV1>,
    pub(super) data_device: Option<wl_data_device::WlDataDevice>,
    pub(super) primary_selection:
        Option<zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1>,
    pub(super) text_input: Option<zwp_text_input_v3::ZwpTextInputV3>,
    pub(super) pre_edit_text: Option<String>,
    pub(super) ime_pre_edit: Option<String>,
    pub(super) composing: bool,
    // Surface to Window mapping
    pub(super) windows: HashMap<ObjectId, WaylandWindowStatePtr>,
    // Output to scale mapping
    pub(super) outputs: HashMap<ObjectId, Output>,
    pub(super) in_progress_outputs: HashMap<ObjectId, InProgressOutput>,
    pub(super) wl_outputs: HashMap<ObjectId, wl_output::WlOutput>,
    pub(super) keyboard_layout: LinuxKeyboardLayout,
    pub(super) keymap_state: Option<xkb::State>,
    pub(super) compose_state: Option<xkb::compose::State>,
    pub(super) drag: DragState,
    pub(super) click: ClickState,
    pub(super) repeat: KeyRepeat,
    pub modifiers: Modifiers,
    pub capslock: Capslock,
    pub(super) axis_source: AxisSource,
    pub mouse_location: Option<Point<Pixels>>,
    pub(super) continuous_scroll_delta: Option<Point<Pixels>>,
    pub(super) discrete_scroll_delta: Option<Point<f32>>,
    pub(super) vertical_modifier: f32,
    pub(super) horizontal_modifier: f32,
    pub(super) scroll_event_received: bool,
    pub(super) enter_token: Option<()>,
    pub(super) button_pressed: Option<MouseButton>,
    pub(super) mouse_focused_window: Option<WaylandWindowStatePtr>,
    pub(super) keyboard_focused_window: Option<WaylandWindowStatePtr>,
    pub(super) loop_handle: LoopHandle<'static, WaylandClientStatePtr>,
    pub(super) cursor_style: Option<CursorStyle>,
    pub(super) cursor_hidden_window: Option<WaylandWindowStatePtr>,
    pub(super) clipboard: Clipboard,
    pub(super) data_offers: Vec<DataOffer<WlDataOffer>>,
    pub(super) primary_data_offer: Option<DataOffer<ZwpPrimarySelectionOfferV1>>,
    pub(super) cursor: Cursor,
    pub(super) pending_activation: Option<PendingActivation>,
    pub(super) startup_activation_token: Option<String>,
    pub(super) event_loop: Option<EventLoop<'static, WaylandClientStatePtr>>,
    pub common: LinuxCommon,
    pub(super) ime_enabled: Option<bool>,
}

pub struct DragState {
    data_offer: Option<wl_data_offer::WlDataOffer>,
    window: Option<WaylandWindowStatePtr>,
    position: Point<Pixels>,
}

pub struct ClickState {
    last_mouse_button: Option<MouseButton>,
    last_click: Instant,
    last_location: Point<Pixels>,
    current_count: usize,
}

pub(crate) struct KeyRepeat {
    characters_per_second: u32,
    delay: Duration,
    current_id: u64,
    current_keycode: Option<xkb::Keycode>,
}

pub(crate) enum PendingActivation {
    /// URI to open in the web browser.
    Uri(String),
    /// Path to open in the file explorer.
    Path(PathBuf),
    /// A window from ourselves to raise.
    Window(ObjectId),
}

impl WaylandClientState {
    pub(super) fn consume_startup_activation_token(&mut self, surface: &wl_surface::WlSurface) {
        let Some(startup_activation_token) = self.startup_activation_token.take() else {
            return;
        };
        let Some(activation) = self.globals.activation.as_ref() else {
            return;
        };
        activation.activate(startup_activation_token, surface);
    }
}

/// This struct is required to conform to Rust's orphan rules, so we can dispatch on the state but hand the
/// window to GPUI.
#[derive(Clone)]
pub struct WaylandClientStatePtr(pub(super) Weak<RefCell<WaylandClientState>>);

impl WaylandClientStatePtr {
    pub fn get_client(&self) -> Rc<RefCell<WaylandClientState>> {
        self.0
            .upgrade()
            .expect("The pointer should always be valid when dispatching in wayland")
    }

    pub fn get_serial(&self, kind: SerialKind) -> u32 {
        self.0.upgrade().unwrap().borrow().serial_tracker.get(kind)
    }

    pub fn set_pending_activation(&self, window: ObjectId) {
        self.0.upgrade().unwrap().borrow_mut().pending_activation =
            Some(PendingActivation::Window(window));
    }

    pub fn enable_ime(&self) {
        let client = self.get_client();
        let mut state = client.borrow_mut();
        state.ime_enabled = Some(true);
        let Some(text_input) = state.text_input.take() else {
            return;
        };

        text_input.enable();
        text_input.set_content_type(ContentHint::None, ContentPurpose::Normal);
        if let Some(window) = state.keyboard_focused_window.clone() {
            drop(state);
            if let Some(area) = window.get_ime_area() {
                text_input.set_cursor_rectangle(
                    f32::from(area.origin.x) as i32,
                    f32::from(area.origin.y) as i32,
                    f32::from(area.size.width) as i32,
                    f32::from(area.size.height) as i32,
                );
            }
            state = client.borrow_mut();
        }
        text_input.commit();
        state.text_input = Some(text_input);
    }

    pub fn disable_ime(&self) {
        let client = self.get_client();
        let mut state = client.borrow_mut();
        state.ime_enabled = Some(false);
        state.composing = false;
        if let Some(text_input) = &state.text_input {
            text_input.disable();
            text_input.commit();
        }
    }

    pub fn ime_enabled(&self) -> Option<bool> {
        let client = self.get_client();
        client.borrow().ime_enabled
    }

    pub fn update_ime_position(&self, bounds: Bounds<Pixels>) {
        let client = self.get_client();
        let state = client.borrow_mut();
        if state.composing || state.text_input.is_none() || state.pre_edit_text.is_some() {
            return;
        }

        let text_input = state.text_input.as_ref().unwrap();
        text_input.set_cursor_rectangle(
            bounds.origin.x.as_f32() as i32,
            bounds.origin.y.as_f32() as i32,
            bounds.size.width.as_f32() as i32,
            bounds.size.height.as_f32() as i32,
        );
        text_input.commit();
    }

    pub fn handle_keyboard_layout_change(&self) {
        let client = self.get_client();
        let mut state = client.borrow_mut();
        let changed = if let Some(keymap_state) = &state.keymap_state {
            let layout_idx = keymap_state.serialize_layout(xkbcommon::xkb::STATE_LAYOUT_EFFECTIVE);
            let keymap = keymap_state.get_keymap();
            let layout_name = keymap.layout_get_name(layout_idx);
            let changed = layout_name != state.keyboard_layout.name();
            if changed {
                state.keyboard_layout = LinuxKeyboardLayout::new(layout_name.to_string().into());
            }
            changed
        } else {
            let changed = &UNKNOWN_KEYBOARD_LAYOUT_NAME != state.keyboard_layout.name();
            if changed {
                state.keyboard_layout = LinuxKeyboardLayout::new(UNKNOWN_KEYBOARD_LAYOUT_NAME);
            }
            changed
        };

        if changed && let Some(mut callback) = state.common.callbacks.keyboard_layout_change.take()
        {
            drop(state);
            callback();
            state = client.borrow_mut();
            state.common.callbacks.keyboard_layout_change = Some(callback);
        }
    }

    pub fn drop_window(&self, surface_id: &ObjectId) {
        let client = self.get_client();
        let mut state = client.borrow_mut();
        let closed_window = state.windows.remove(surface_id).unwrap();
        if let Some(window) = state.mouse_focused_window.take()
            && !window.ptr_eq(&closed_window)
        {
            state.mouse_focused_window = Some(window);
        }
        if let Some(window) = state.keyboard_focused_window.take()
            && !window.ptr_eq(&closed_window)
        {
            state.keyboard_focused_window = Some(window);
        }
        if let Some(window) = state.cursor_hidden_window.take()
            && !window.ptr_eq(&closed_window)
        {
            state.cursor_hidden_window = Some(window);
        }
    }
}

impl WaylandClientState {
    fn hide_cursor_until_mouse_moves(&mut self) {
        if self.cursor_hidden_window.is_some() {
            return;
        }
        let Some(focused_window) = self.mouse_focused_window.clone() else {
            // No surface to apply the hidden cursor to.
            return;
        };
        let Some(wl_pointer) = self.wl_pointer.clone() else {
            // Seat lost its pointer capability; nothing to hide.
            return;
        };
        let serial = self.serial_tracker.get(SerialKind::MouseEnter);
        wl_pointer.set_cursor(serial, None, 0, 0);
        self.cursor_hidden_window = Some(focused_window);
    }

    fn restore_cursor_after_hide(&mut self) {
        if self.cursor_hidden_window.take().is_none() {
            return;
        }
        let Some(style) = self.cursor_style else {
            return;
        };
        let serial = self.serial_tracker.get(SerialKind::MouseEnter);
        if let Some(cursor_shape_device) = &self.cursor_shape_device {
            cursor_shape_device.set_shape(serial, to_shape(style));
            return;
        }
        let Some(focused_window) = self.mouse_focused_window.clone() else {
            log::warn!(
                "wayland: no focused surface to restore cursor style {:?} after hide; cursor may stay invisible",
                style
            );
            return;
        };
        let Some(wl_pointer) = self.wl_pointer.clone() else {
            log::warn!(
                "wayland: no wl_pointer to restore cursor style {:?} after hide; cursor may stay invisible",
                style
            );
            return;
        };
        let scale = focused_window.primary_output_scale();
        self.cursor.set_icon(
            &wl_pointer,
            serial,
            cursor_style_to_icon_names(style),
            scale,
        );
    }
}

#[derive(Clone)]
pub struct WaylandClient(pub(super) Rc<RefCell<WaylandClientState>>);

impl Drop for WaylandClient {
    fn drop(&mut self) {
        let mut state = self.0.borrow_mut();
        state.windows.clear();

        if let Some(wl_pointer) = &state.wl_pointer {
            wl_pointer.release();
        }
        if let Some(cursor_shape_device) = &state.cursor_shape_device {
            cursor_shape_device.destroy();
        }
        if let Some(data_device) = &state.data_device {
            data_device.release();
        }
        if let Some(text_input) = &state.text_input {
            text_input.destroy();
        }
    }
}

const WL_DATA_DEVICE_MANAGER_VERSION: u32 = 3;

pub(super) fn wl_seat_version(version: u32) -> u32 {
    // We rely on the wl_pointer.frame event
    const WL_SEAT_MIN_VERSION: u32 = 5;
    const WL_SEAT_MAX_VERSION: u32 = 9;

    if version < WL_SEAT_MIN_VERSION {
        panic!(
            "wl_seat below required version: {} < {}",
            version, WL_SEAT_MIN_VERSION
        );
    }

    version.clamp(WL_SEAT_MIN_VERSION, WL_SEAT_MAX_VERSION)
}

pub(super) fn wl_output_version(version: u32) -> u32 {
    const WL_OUTPUT_MIN_VERSION: u32 = 2;
    const WL_OUTPUT_MAX_VERSION: u32 = 4;

    if version < WL_OUTPUT_MIN_VERSION {
        panic!(
            "wl_output below required version: {} < {}",
            version, WL_OUTPUT_MIN_VERSION
        );
    }

    version.clamp(WL_OUTPUT_MIN_VERSION, WL_OUTPUT_MAX_VERSION)
}
