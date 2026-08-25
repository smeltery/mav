use super::*;

impl PlatformWindow for X11Window {
    fn bounds(&self) -> Bounds<Pixels> {
        self.0.state.borrow().bounds
    }

    fn is_maximized(&self) -> bool {
        let state = self.0.state.borrow();

        // A maximized window that gets minimized will still retain its maximized state.
        !state.hidden && state.maximized_vertical && state.maximized_horizontal
    }

    fn window_bounds(&self) -> WindowBounds {
        let state = self.0.state.borrow();
        if self.is_maximized() {
            WindowBounds::Maximized(state.bounds)
        } else {
            WindowBounds::Windowed(state.bounds)
        }
    }

    fn inner_window_bounds(&self) -> WindowBounds {
        let state = self.0.state.borrow();
        if self.is_maximized() {
            WindowBounds::Maximized(state.bounds)
        } else {
            let mut bounds = state.bounds;
            let [left, right, top, bottom] = state.last_insets;

            let [left, right, top, bottom] = [
                px((left as f32) / state.scale_factor),
                px((right as f32) / state.scale_factor),
                px((top as f32) / state.scale_factor),
                px((bottom as f32) / state.scale_factor),
            ];

            bounds.origin.x += left;
            bounds.origin.y += top;
            bounds.size.width -= left + right;
            bounds.size.height -= top + bottom;

            WindowBounds::Windowed(bounds)
        }
    }

    pub(super) fn content_size(&self) -> Size<Pixels> {
        // After the wgpu migration, X11WindowState::content_size() returns logical pixels
        // (bounds.size is already divided by scale_factor in set_bounds), so no further
        // division is needed here. This matches the Wayland implementation.
        self.0.state.borrow().content_size()
    }

    fn resize(&mut self, size: Size<Pixels>) {
        let state = self.0.state.borrow();
        let size = size.to_device_pixels(state.scale_factor);
        let width = size.width.0 as u32;
        let height = size.height.0 as u32;

        check_reply(
            || {
                format!(
                    "X11 ConfigureWindow failed. width: {}, height: {}",
                    width, height
                )
            },
            self.0.xcb.configure_window(
                self.0.x_window,
                &xproto::ConfigureWindowAux::new()
                    .width(width)
                    .height(height),
            ),
        )
        .log_err();
        xcb_flush(&self.0.xcb);
    }

    fn scale_factor(&self) -> f32 {
        self.0.state.borrow().scale_factor
    }

    fn appearance(&self) -> WindowAppearance {
        self.0.state.borrow().appearance
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(self.0.state.borrow().display.clone())
    }

    fn mouse_position(&self) -> Point<Pixels> {
        get_reply(
            || "X11 QueryPointer failed.",
            self.0.xcb.query_pointer(self.0.x_window),
        )
        .log_err()
        .map_or(Point::new(Pixels::ZERO, Pixels::ZERO), |reply| {
            Point::new((reply.root_x as u32).into(), (reply.root_y as u32).into())
        })
    }

    fn modifiers(&self) -> Modifiers {
        self.0
            .state
            .borrow()
            .client
            .0
            .upgrade()
            .map(|ref_cell| ref_cell.borrow().modifiers)
            .unwrap_or_default()
    }

    fn capslock(&self) -> gpui::Capslock {
        self.0
            .state
            .borrow()
            .client
            .0
            .upgrade()
            .map(|ref_cell| ref_cell.borrow().capslock)
            .unwrap_or_default()
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.0.state.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.state.borrow_mut().input_handler.take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        None
    }

    fn activate(&self) {
        let data = [1, xproto::Time::CURRENT_TIME.into(), 0, 0, 0];
        let message = xproto::ClientMessageEvent::new(
            32,
            self.0.x_window,
            self.0.state.borrow().atoms._NET_ACTIVE_WINDOW,
            data,
        );
        self.0
            .xcb
            .send_event(
                false,
                self.0.state.borrow().x_root_window,
                xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
                message,
            )
            .log_err();
        self.0
            .xcb
            .set_input_focus(
                xproto::InputFocus::POINTER_ROOT,
                self.0.x_window,
                xproto::Time::CURRENT_TIME,
            )
            .log_err();
        xcb_flush(&self.0.xcb);
    }

    fn is_active(&self) -> bool {
        self.0.state.borrow().active
    }

    fn is_hovered(&self) -> bool {
        self.0.state.borrow().hovered
    }

    fn set_title(&mut self, title: &str) {
        check_reply(
            || "X11 ChangeProperty8 on WM_NAME failed.",
            self.0.xcb.change_property8(
                xproto::PropMode::REPLACE,
                self.0.x_window,
                xproto::AtomEnum::WM_NAME,
                xproto::AtomEnum::STRING,
                title.as_bytes(),
            ),
        )
        .log_err();

        check_reply(
            || "X11 ChangeProperty8 on _NET_WM_NAME failed.",
            self.0.xcb.change_property8(
                xproto::PropMode::REPLACE,
                self.0.x_window,
                self.0.state.borrow().atoms._NET_WM_NAME,
                self.0.state.borrow().atoms.UTF8_STRING,
                title.as_bytes(),
            ),
        )
        .log_err();
        xcb_flush(&self.0.xcb);
    }

    fn set_app_id(&mut self, app_id: &str) {
        let mut data = Vec::with_capacity(app_id.len() * 2 + 1);
        data.extend(app_id.bytes()); // instance https://unix.stackexchange.com/a/494170
        data.push(b'\0');
        data.extend(app_id.bytes()); // class

        check_reply(
            || "X11 ChangeProperty8 for WM_CLASS failed.",
            self.0.xcb.change_property8(
                xproto::PropMode::REPLACE,
                self.0.x_window,
                xproto::AtomEnum::WM_CLASS,
                xproto::AtomEnum::STRING,
                &data,
            ),
        )
        .log_err();
    }

    fn map_window(&mut self) -> anyhow::Result<()> {
        check_reply(
            || "X11 MapWindow failed.",
            self.0.xcb.map_window(self.0.x_window),
        )?;
        Ok(())
    }

    fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) {
        let mut state = self.0.state.borrow_mut();
        state.background_appearance = background_appearance;
        let transparent = state.is_transparent();
        state.renderer.update_transparency(transparent);
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        self.0.state.borrow().background_appearance
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        self.0
            .state
            .borrow()
            .client
            .0
            .upgrade()
            .map(|ref_cell| {
                let state = ref_cell.borrow();
                state
                    .gpu_context
                    .borrow()
                    .as_ref()
                    .is_some_and(|ctx| ctx.supports_dual_source_blending())
            })
            .unwrap_or_default()
    }

    fn minimize(&self) {
        let state = self.0.state.borrow();
        const WINDOW_ICONIC_STATE: u32 = 3;
        let message = ClientMessageEvent::new(
            32,
            self.0.x_window,
            state.atoms.WM_CHANGE_STATE,
            [WINDOW_ICONIC_STATE, 0, 0, 0, 0],
        );
        check_reply(
            || "X11 SendEvent to minimize window failed.",
            self.0.xcb.send_event(
                false,
                state.x_root_window,
                xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
                message,
            ),
        )
        .log_err();
    }

    fn zoom(&self) {
        let state = self.0.state.borrow();
        self.set_wm_hints(
            || "X11 SendEvent to maximize a window failed.",
            WmHintPropertyState::Toggle,
            state.atoms._NET_WM_STATE_MAXIMIMAV_VERT,
            state.atoms._NET_WM_STATE_MAXIMIMAV_HORZ,
        )
        .log_err();
    }

    fn toggle_fullscreen(&self) {
        let state = self.0.state.borrow();
        self.set_wm_hints(
            || "X11 SendEvent to fullscreen a window failed.",
            WmHintPropertyState::Toggle,
            state.atoms._NET_WM_STATE_FULLSCREEN,
            xproto::AtomEnum::NONE.into(),
        )
        .log_err();
    }

    fn is_fullscreen(&self) -> bool {
        self.0.state.borrow().fullscreen
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.callbacks.borrow_mut().request_frame = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> gpui::DispatchEventResult>) {
        self.0.callbacks.borrow_mut().input = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.callbacks.borrow_mut().active_status_change = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.callbacks.borrow_mut().hovered_status_change = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.callbacks.borrow_mut().resize = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        self.0.callbacks.borrow_mut().moved = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.callbacks.borrow_mut().should_close = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.callbacks.borrow_mut().close = Some(callback);
    }

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.callbacks.borrow_mut().appearance_changed = Some(callback);
    }

    fn on_button_layout_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.callbacks.borrow_mut().button_layout_changed = Some(callback);
    }

    fn draw(&self, scene: &Scene) {
        let mut inner = self.0.state.borrow_mut();

        if inner.renderer.device_lost() {
            let raw_window = RawWindow {
                connection: as_raw_xcb_connection::AsRawXcbConnection::as_raw_xcb_connection(
                    &*self.0.xcb,
                ) as *mut _,
                screen_id: inner.x_screen_index,
                window_id: self.0.x_window,
                visual_id: inner.visual_id,
            };
            match inner.renderer.recover(&raw_window) {
                Ok(()) => {}
                Err(err) => {
                    log::warn!("GPU recovery failed, will retry on next frame: {err}");
                }
            }

            inner.force_render_after_recovery = true;
            return;
        }

        inner.renderer.draw(scene);

        if inner.renderer.needs_redraw() {
            inner.force_render_after_recovery = true;
        }
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        let inner = self.0.state.borrow();
        inner.renderer.sprite_atlas().clone()
    }

    fn show_window_menu(&self, position: Point<Pixels>) {
        let state = self.0.state.borrow();

        check_reply(
            || "X11 UngrabPointer failed.",
            self.0.xcb.ungrab_pointer(x11rb::CURRENT_TIME),
        )
        .log_err();

        let Some(coords) = self.get_root_position(position).log_err() else {
            return;
        };
        let message = ClientMessageEvent::new(
            32,
            self.0.x_window,
            state.atoms._GTK_SHOW_WINDOW_MENU,
            [
                XINPUT_ALL_DEVICE_GROUPS as u32,
                coords.dst_x as u32,
                coords.dst_y as u32,
                0,
                0,
            ],
        );
        check_reply(
            || "X11 SendEvent to show window menu failed.",
            self.0.xcb.send_event(
                false,
                state.x_root_window,
                xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
                message,
            ),
        )
        .log_err();
    }

    fn start_window_move(&self) {
        const MOVERESIZE_MOVE: u32 = 8;
        self.send_moveresize(MOVERESIZE_MOVE).log_err();
    }

    fn start_window_resize(&self, edge: ResizeEdge) {
        self.send_moveresize(resize_edge_to_moveresize(edge))
            .log_err();
    }

    fn window_decorations(&self) -> gpui::Decorations {
        self.window_decorations_impl()
    }

    fn set_client_inset(&self, inset: Pixels) {
        self.set_client_inset_impl(inset);
    }

    fn request_decorations(&self, decorations: gpui::WindowDecorations) {
        self.request_decorations_impl(decorations);
    }

    fn update_ime_position(&self, bounds: Bounds<Pixels>) {
        let state = self.0.state.borrow();
        let client = state.client.clone();
        drop(state);
        client.update_ime_position(bounds);
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.0.state.borrow().renderer.gpu_specs().into()
    }

    fn play_system_bell(&self) {
        // Volume 0% means don't increase or decrease from system volume
        let _ = self.0.xcb.bell(0);
    }

    fn a11y_init(&self, callbacks: gpui::A11yCallbacks) {
        self.a11y_init_impl(callbacks);
    }

    fn a11y_tree_update(&self, tree_update: ::accesskit::TreeUpdate) {
        self.a11y_tree_update_impl(tree_update);
    }

    fn a11y_update_window_bounds(&self) {
        self.a11y_update_window_bounds_impl();
    }
}
