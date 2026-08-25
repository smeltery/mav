use super::*;

impl X11Client {
    pub(super) fn handle_xinput_event(&self, event: Event) -> Option<()> {
        match event {
            Event::XinputButtonPress(event) => {
                let window = self.get_window(event.event)?;
                let mut state = self.0.borrow_mut();

                let modifiers = modifiers_from_xinput_info(event.mods);
                state.modifiers = modifiers;

                let position = point(
                    px(event.event_x as f32 / u16::MAX as f32 / state.scale_factor),
                    px(event.event_y as f32 / u16::MAX as f32 / state.scale_factor),
                );

                if state.composing && state.ximc.is_some() {
                    drop(state);
                    self.reset_ime();
                    window.handle_ime_unmark();
                    state = self.0.borrow_mut();
                } else if let Some(text) = state.pre_edit_text.take() {
                    if let Some(compose_state) = state.compose_state.as_mut() {
                        compose_state.reset();
                    }
                    drop(state);
                    window.handle_ime_commit(text);
                    state = self.0.borrow_mut();
                }
                match button_or_scroll_from_event_detail(event.detail) {
                    Some(ButtonOrScroll::Button(button)) => {
                        let click_elapsed = state.last_click.elapsed();
                        if click_elapsed < DOUBLE_CLICK_INTERVAL
                            && state
                                .last_mouse_button
                                .is_some_and(|prev_button| prev_button == button)
                            && is_within_click_distance(state.last_location, position)
                        {
                            state.current_count += 1;
                        } else {
                            state.current_count = 1;
                        }

                        state.last_click = Instant::now();
                        state.last_mouse_button = Some(button);
                        state.last_location = position;
                        let current_count = state.current_count;

                        drop(state);
                        window.handle_input(PlatformInput::MouseDown(gpui::MouseDownEvent {
                            button,
                            position,
                            modifiers,
                            click_count: current_count,
                            first_mouse: false,
                        }));
                    }
                    Some(ButtonOrScroll::Scroll(direction)) => {
                        drop(state);
                        // Emulated scroll button presses are sent simultaneously with smooth scrolling XinputMotion events.
                        // Since handling those events does the scrolling, they are skipped here.
                        if !event
                            .flags
                            .contains(xinput::PointerEventFlags::POINTER_EMULATED)
                        {
                            let scroll_delta = match direction {
                                ScrollDirection::Up => Point::new(0.0, SCROLL_LINES),
                                ScrollDirection::Down => Point::new(0.0, -SCROLL_LINES),
                                ScrollDirection::Left => Point::new(SCROLL_LINES, 0.0),
                                ScrollDirection::Right => Point::new(-SCROLL_LINES, 0.0),
                            };
                            window.handle_input(PlatformInput::ScrollWheel(
                                make_scroll_wheel_event(position, scroll_delta, modifiers),
                            ));
                        }
                    }
                    None => {
                        log::error!("Unknown x11 button: {}", event.detail);
                    }
                }
            }
            Event::XinputButtonRelease(event) => {
                let window = self.get_window(event.event)?;
                let mut state = self.0.borrow_mut();
                let modifiers = modifiers_from_xinput_info(event.mods);
                state.modifiers = modifiers;

                let position = point(
                    px(event.event_x as f32 / u16::MAX as f32 / state.scale_factor),
                    px(event.event_y as f32 / u16::MAX as f32 / state.scale_factor),
                );
                match button_or_scroll_from_event_detail(event.detail) {
                    Some(ButtonOrScroll::Button(button)) => {
                        let click_count = state.current_count;
                        drop(state);
                        window.handle_input(PlatformInput::MouseUp(gpui::MouseUpEvent {
                            button,
                            position,
                            modifiers,
                            click_count,
                        }));
                    }
                    Some(ButtonOrScroll::Scroll(_)) => {}
                    None => {}
                }
            }
            Event::XinputMotion(event) => {
                let window = self.get_window(event.event)?;
                let mut state = self.0.borrow_mut();
                state.restore_cursor_after_hide();
                if window.is_blocked() {
                    // We want to set the cursor to the default arrow
                    // when the window is blocked
                    let style = CursorStyle::Arrow;

                    let current_style = state
                        .cursor_styles
                        .get(&window.x_window)
                        .unwrap_or(&CursorStyle::Arrow);
                    if *current_style != style
                        && let Some(cursor) = state.get_cursor_icon(style)
                    {
                        state.cursor_styles.insert(window.x_window, style);
                        check_reply(
                            || "Failed to set cursor style",
                            state.xcb_connection.change_window_attributes(
                                window.x_window,
                                &ChangeWindowAttributesAux {
                                    cursor: Some(cursor),
                                    ..Default::default()
                                },
                            ),
                        )
                        .log_err();
                        state.xcb_connection.flush().log_err();
                    };
                }
                let pressed_button = pressed_button_from_mask(event.button_mask[0]);
                let position = point(
                    px(event.event_x as f32 / u16::MAX as f32 / state.scale_factor),
                    px(event.event_y as f32 / u16::MAX as f32 / state.scale_factor),
                );
                let modifiers = modifiers_from_xinput_info(event.mods);
                state.modifiers = modifiers;
                drop(state);

                if event.valuator_mask[0] & 3 != 0 {
                    window.handle_input(PlatformInput::MouseMove(gpui::MouseMoveEvent {
                        position,
                        pressed_button,
                        modifiers,
                    }));
                }

                state = self.0.borrow_mut();
                if let Some(pointer) = state.pointer_device_states.get_mut(&event.sourceid) {
                    let scroll_delta = get_scroll_delta_and_update_state(pointer, &event);
                    drop(state);
                    if let Some(scroll_delta) = scroll_delta {
                        window.handle_input(PlatformInput::ScrollWheel(make_scroll_wheel_event(
                            position,
                            scroll_delta,
                            modifiers,
                        )));
                    }
                }
            }
            Event::XinputEnter(event) if event.mode == xinput::NotifyMode::NORMAL => {
                let window = self.get_window(event.event)?;
                window.set_hovered(true);
                let mut state = self.0.borrow_mut();
                state.mouse_focused_window = Some(event.event);
                state.restore_cursor_after_hide();
            }
            Event::XinputLeave(event) if event.mode == xinput::NotifyMode::NORMAL => {
                let mut state = self.0.borrow_mut();

                // Set last scroll values to `None` so that a large delta isn't created if scrolling is done outside the window (the valuator is global)
                reset_all_pointer_device_scroll_positions(&mut state.pointer_device_states);
                state.mouse_focused_window = None;
                let pressed_button = pressed_button_from_mask(event.buttons[0]);
                let position = point(
                    px(event.event_x as f32 / u16::MAX as f32 / state.scale_factor),
                    px(event.event_y as f32 / u16::MAX as f32 / state.scale_factor),
                );
                let modifiers = modifiers_from_xinput_info(event.mods);
                state.modifiers = modifiers;
                drop(state);

                let window = self.get_window(event.event)?;
                window.handle_input(PlatformInput::MouseExited(gpui::MouseExitEvent {
                    pressed_button,
                    position,
                    modifiers,
                }));
                window.set_hovered(false);
            }
            Event::XinputHierarchy(event) => {
                let mut state = self.0.borrow_mut();
                // Temporarily use `state.pointer_device_states` to only store pointers that still have valid scroll values.
                // Any change to a device invalidates its scroll values.
                for info in event.infos {
                    if is_pointer_device(info.type_) {
                        state.pointer_device_states.remove(&info.deviceid);
                    }
                }
                if let Some(pointer_device_states) = current_pointer_device_states(
                    &state.xcb_connection,
                    &state.pointer_device_states,
                ) {
                    state.pointer_device_states = pointer_device_states;
                }
            }
            Event::XinputDeviceChanged(event) => {
                let mut state = self.0.borrow_mut();
                if let Some(pointer) = state.pointer_device_states.get_mut(&event.sourceid) {
                    reset_pointer_device_scroll_positions(pointer);
                }
            }
            Event::XinputGesturePinchBegin(event) => {
                let window = self.get_window(event.event)?;
                let mut state = self.0.borrow_mut();
                state.pinch_scale = 1.0;
                let modifiers = modifiers_from_xinput_info(event.mods);
                state.modifiers = modifiers;
                let position = point(
                    px(event.event_x as f32 / u16::MAX as f32 / state.scale_factor),
                    px(event.event_y as f32 / u16::MAX as f32 / state.scale_factor),
                );
                drop(state);
                window.handle_input(PlatformInput::Pinch(gpui::PinchEvent {
                    position,
                    delta: 0.0,
                    modifiers,
                    phase: gpui::TouchPhase::Started,
                }));
            }
            Event::XinputGesturePinchUpdate(event) => {
                let window = self.get_window(event.event)?;
                let mut state = self.0.borrow_mut();
                let modifiers = modifiers_from_xinput_info(event.mods);
                state.modifiers = modifiers;
                let position = point(
                    px(event.event_x as f32 / u16::MAX as f32 / state.scale_factor),
                    px(event.event_y as f32 / u16::MAX as f32 / state.scale_factor),
                );
                // scale is in FP16.16 format: divide by 65536 to get the float value
                let new_absolute_scale = event.scale as f32 / 65536.0;
                let previous_scale = state.pinch_scale;
                let zoom_delta = new_absolute_scale - previous_scale;
                state.pinch_scale = new_absolute_scale;
                drop(state);
                window.handle_input(PlatformInput::Pinch(gpui::PinchEvent {
                    position,
                    delta: zoom_delta,
                    modifiers,
                    phase: gpui::TouchPhase::Moved,
                }));
            }
            Event::XinputGesturePinchEnd(event) => {
                let window = self.get_window(event.event)?;
                let mut state = self.0.borrow_mut();
                state.pinch_scale = 1.0;
                let modifiers = modifiers_from_xinput_info(event.mods);
                state.modifiers = modifiers;
                let position = point(
                    px(event.event_x as f32 / u16::MAX as f32 / state.scale_factor),
                    px(event.event_y as f32 / u16::MAX as f32 / state.scale_factor),
                );
                drop(state);
                window.handle_input(PlatformInput::Pinch(gpui::PinchEvent {
                    position,
                    delta: 0.0,
                    modifiers,
                    phase: gpui::TouchPhase::Ended,
                }));
            }
            _ => {}
            _ => {}
        };

        Some(())
    }
}
