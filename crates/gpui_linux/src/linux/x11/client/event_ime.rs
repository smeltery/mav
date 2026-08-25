use super::*;

impl X11Client {
    pub(super) fn handle_xim_callback_event(&self, event: XimCallbackEvent) {
        match event {
            XimCallbackEvent::XimXEvent(event) => {
                self.handle_event(event);
            }
            XimCallbackEvent::XimCommitEvent(window, text) => {
                self.xim_handle_commit(window, text);
            }
            XimCallbackEvent::XimPreeditEvent(window, text) => {
                self.xim_handle_preedit(window, text);
            }
        };
    }

    pub(super) fn xim_handle_event(&self, event: Event) -> Option<()> {
        match event {
            Event::KeyPress(event) | Event::KeyRelease(event) => {
                let mut state = self.0.borrow_mut();
                state.pre_key_char_down = Some(keystroke_from_xkb(
                    &state.xkb,
                    state.modifiers,
                    event.detail.into(),
                ));
                let (mut ximc, mut xim_handler) = state.take_xim()?;
                drop(state);
                xim_handler.window = event.event;
                ximc.forward_event(
                    xim_handler.im_id,
                    xim_handler.ic_id,
                    xim::ForwardEventFlag::empty(),
                    &event,
                )
                .context("X11: Failed to forward XIM event")
                .log_err();
                let mut state = self.0.borrow_mut();
                state.restore_xim(ximc, xim_handler);
                drop(state);
            }
            event => {
                self.handle_event(event);
            }
        }
        Some(())
    }

    fn xim_handle_commit(&self, window: xproto::Window, text: String) -> Option<()> {
        let Some(window) = self.get_window(window) else {
            log::error!("bug: Failed to get window for XIM commit");
            return None;
        };
        let mut state = self.0.borrow_mut();
        state.composing = false;
        drop(state);
        window.handle_ime_commit(text);
        Some(())
    }

    fn xim_handle_preedit(&self, window: xproto::Window, text: String) -> Option<()> {
        let Some(window) = self.get_window(window) else {
            log::error!("bug: Failed to get window for XIM preedit");
            return None;
        };

        let mut state = self.0.borrow_mut();
        let (mut ximc, xim_handler) = state.take_xim()?;
        state.composing = !text.is_empty();
        drop(state);
        window.handle_ime_preedit(text);

        if let Some(scaled_area) = window.get_ime_area() {
            let ic_attributes = ximc
                .build_ic_attributes()
                .push(
                    xim::AttributeName::InputStyle,
                    xim::InputStyle::PREEDIT_CALLBACKS,
                )
                .push(xim::AttributeName::ClientWindow, xim_handler.window)
                .push(xim::AttributeName::FocusWindow, xim_handler.window)
                .nested_list(xim::AttributeName::PreeditAttributes, |b| {
                    b.push(
                        xim::AttributeName::SpotLocation,
                        xim::Point {
                            x: u32::from(scaled_area.origin.x + scaled_area.size.width) as i16,
                            y: u32::from(scaled_area.origin.y + scaled_area.size.height) as i16,
                        },
                    );
                })
                .build();
            ximc.set_ic_values(xim_handler.im_id, xim_handler.ic_id, ic_attributes)
                .ok();
        }
        let mut state = self.0.borrow_mut();
        state.restore_xim(ximc, xim_handler);
        drop(state);
        Some(())
    }

    pub(super) fn handle_keyboard_layout_change(&self) {
        let mut state = self.0.borrow_mut();
        let layout_idx = state.xkb.serialize_layout(STATE_LAYOUT_EFFECTIVE);
        let keymap = state.xkb.get_keymap();
        let layout_name = keymap.layout_get_name(layout_idx);
        if layout_name != state.keyboard_layout.name() {
            state.keyboard_layout = LinuxKeyboardLayout::new(layout_name.to_string().into());
            if let Some(mut callback) = state.common.callbacks.keyboard_layout_change.take() {
                drop(state);
                callback();
                state = self.0.borrow_mut();
                state.common.callbacks.keyboard_layout_change = Some(callback);
            }
        }
    }
}
