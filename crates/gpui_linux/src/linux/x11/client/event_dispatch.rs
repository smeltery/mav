use super::*;

impl X11Client {
    pub(super) fn handle_event(&self, event: Event) -> Option<()> {
        match event {
            Event::UnmapNotify(event) => {
                let mut state = self.0.borrow_mut();
                if let Some(window_ref) = state.windows.get_mut(&event.window) {
                    window_ref.is_mapped = false;
                }
                state.update_refresh_loop(event.window);
            }
            Event::MapNotify(event) => {
                let mut state = self.0.borrow_mut();
                if let Some(window_ref) = state.windows.get_mut(&event.window) {
                    window_ref.is_mapped = true;
                }
                state.update_refresh_loop(event.window);
            }
            Event::VisibilityNotify(event) => {
                let mut state = self.0.borrow_mut();
                if let Some(window_ref) = state.windows.get_mut(&event.window) {
                    window_ref.last_visibility = event.state;
                }
                state.update_refresh_loop(event.window);
            }
            Event::ClientMessage(event) => {
                let window = self.get_window(event.window)?;
                let [atom, arg1, arg2, arg3, arg4] = event.data.as_data32();
                let mut state = self.0.borrow_mut();

                if atom == state.atoms.WM_DELETE_WINDOW && window.should_close() {
                    // window "x" button clicked by user
                    // Rest of the close logic is handled in drop_window()
                    drop(state);
                    window.close();
                    state = self.0.borrow_mut();
                } else if atom == state.atoms._NET_WM_SYNC_REQUEST {
                    window.state.borrow_mut().last_sync_counter =
                        Some(x11rb::protocol::sync::Int64 {
                            lo: arg2,
                            hi: arg3 as i32,
                        })
                }

                if event.type_ == state.atoms.XdndEnter {
                    state.xdnd_state.other_window = atom;
                    if (arg1 & 0x1) == 0x1 {
                        state.xdnd_state.drag_type = xdnd_get_supported_atom(
                            &state.xcb_connection,
                            &state.atoms,
                            state.xdnd_state.other_window,
                        );
                    } else {
                        if let Some(atom) = [arg2, arg3, arg4]
                            .into_iter()
                            .find(|atom| xdnd_is_atom_supported(*atom, &state.atoms))
                        {
                            state.xdnd_state.drag_type = atom;
                        }
                    }
                } else if event.type_ == state.atoms.XdndLeave {
                    let position = state.xdnd_state.position;
                    drop(state);
                    window
                        .handle_input(PlatformInput::FileDrop(FileDropEvent::Pending { position }));
                    window.handle_input(PlatformInput::FileDrop(FileDropEvent::Exited {}));
                    self.0.borrow_mut().xdnd_state = Xdnd::default();
                } else if event.type_ == state.atoms.XdndPosition {
                    if let Ok(pos) = get_reply(
                        || "Failed to query pointer position",
                        state.xcb_connection.query_pointer(event.window),
                    ) {
                        state.xdnd_state.position =
                            Point::new(px(pos.win_x as f32), px(pos.win_y as f32));
                    }
                    if !state.xdnd_state.retrieved {
                        check_reply(
                            || "Failed to convert selection for drag and drop",
                            state.xcb_connection.convert_selection(
                                event.window,
                                state.atoms.XdndSelection,
                                state.xdnd_state.drag_type,
                                state.atoms.XDND_DATA,
                                arg3,
                            ),
                        )
                        .log_err();
                    }
                    xdnd_send_status(
                        &state.xcb_connection,
                        &state.atoms,
                        event.window,
                        state.xdnd_state.other_window,
                        arg4,
                    );
                    let position = state.xdnd_state.position;
                    drop(state);
                    window
                        .handle_input(PlatformInput::FileDrop(FileDropEvent::Pending { position }));
                } else if event.type_ == state.atoms.XdndDrop {
                    xdnd_send_finished(
                        &state.xcb_connection,
                        &state.atoms,
                        event.window,
                        state.xdnd_state.other_window,
                    );
                    let position = state.xdnd_state.position;
                    drop(state);
                    window
                        .handle_input(PlatformInput::FileDrop(FileDropEvent::Submit { position }));
                    self.0.borrow_mut().xdnd_state = Xdnd::default();
                }
            }
            Event::SelectionNotify(event) => {
                let window = self.get_window(event.requestor)?;
                let state = self.0.borrow_mut();
                let reply = get_reply(
                    || "Failed to get XDND_DATA",
                    state.xcb_connection.get_property(
                        false,
                        event.requestor,
                        state.atoms.XDND_DATA,
                        AtomEnum::ANY,
                        0,
                        1024,
                    ),
                )
                .log_err();
                let Some(reply) = reply else {
                    return Some(());
                };
                if let Ok(file_list) = str::from_utf8(&reply.value) {
                    let paths: SmallVec<[_; 2]> = file_list
                        .lines()
                        .filter_map(|path| Url::parse(path).log_err())
                        .filter_map(|url| match url.to_file_path() {
                            Ok(url) => Some(url),
                            Err(()) => {
                                log::error!("Failed turn {url:?} into a file path");
                                None
                            }
                        })
                        .collect();
                    let input = PlatformInput::FileDrop(FileDropEvent::Entered {
                        position: state.xdnd_state.position,
                        paths: gpui::ExternalPaths(paths),
                    });
                    drop(state);
                    window.handle_input(input);
                    self.0.borrow_mut().xdnd_state.retrieved = true;
                }
            }
            Event::ConfigureNotify(event) => {
                let bounds = Bounds {
                    origin: Point {
                        x: event.x.into(),
                        y: event.y.into(),
                    },
                    size: Size {
                        width: event.width.into(),
                        height: event.height.into(),
                    },
                };
                let window = self.get_window(event.window)?;
                window
                    .set_bounds(bounds)
                    .context("X11: Failed to set window bounds")
                    .log_err();
            }
            Event::PropertyNotify(event) => {
                let window = self.get_window(event.window)?;
                window
                    .property_notify(event)
                    .context("X11: Failed to handle property notify")
                    .log_err();
            }
            Event::FocusIn(event) => {
                let window = self.get_window(event.event)?;
                window.set_active(true);
                let mut state = self.0.borrow_mut();
                state.keyboard_focused_window = Some(event.event);
                if let Some(handler) = state.xim_handler.as_mut() {
                    handler.window = event.event;
                }
                drop(state);
                self.enable_ime();
            }
            Event::FocusOut(event) => {
                let window = self.get_window(event.event)?;
                window.set_active(false);
                let mut state = self.0.borrow_mut();
                // Set last scroll values to `None` so that a large delta isn't created if scrolling is done outside the window (the valuator is global)
                reset_all_pointer_device_scroll_positions(&mut state.pointer_device_states);
                state.keyboard_focused_window = None;
                if let Some(compose_state) = state.compose_state.as_mut() {
                    compose_state.reset();
                }
                state.pre_edit_text.take();
                state.restore_cursor_after_hide();
                drop(state);
                self.reset_ime();
                window.handle_ime_delete();
            }
            Event::XkbNewKeyboardNotify(_) | Event::XkbMapNotify(_) => {
                let mut state = self.0.borrow_mut();
                let xkb_state = {
                    let xkb_keymap = xkbc::x11::keymap_new_from_device(
                        &state.xkb_context,
                        &state.xcb_connection,
                        state.xkb_device_id,
                        xkbc::KEYMAP_COMPILE_NO_FLAGS,
                    );
                    xkbc::x11::state_new_from_device(
                        &xkb_keymap,
                        &state.xcb_connection,
                        state.xkb_device_id,
                    )
                };
                state.xkb = xkb_state;
                drop(state);
                self.handle_keyboard_layout_change();
            }
            Event::XkbStateNotify(event) => {
                let mut state = self.0.borrow_mut();
                let old_layout = state.xkb.serialize_layout(STATE_LAYOUT_EFFECTIVE);
                let new_layout = u32::from(event.group);
                state.xkb.update_mask(
                    event.base_mods.into(),
                    event.latched_mods.into(),
                    event.locked_mods.into(),
                    event.base_group as u32,
                    event.latched_group as u32,
                    event.locked_group.into(),
                );
                let modifiers = modifiers_from_xkb(&state.xkb);
                let capslock = capslock_from_xkb(&state.xkb);
                if state.last_modifiers_changed_event == modifiers
                    && state.last_capslock_changed_event == capslock
                {
                    drop(state);
                } else {
                    let focused_window_id = state.keyboard_focused_window?;
                    state.modifiers = modifiers;
                    state.last_modifiers_changed_event = modifiers;
                    state.capslock = capslock;
                    state.last_capslock_changed_event = capslock;
                    drop(state);

                    let focused_window = self.get_window(focused_window_id)?;
                    focused_window.handle_input(PlatformInput::ModifiersChanged(
                        ModifiersChangedEvent {
                            modifiers,
                            capslock,
                        },
                    ));
                }

                if new_layout != old_layout {
                    self.handle_keyboard_layout_change();
                }
            }
            Event::KeyPress(event) => {
                let window = self.get_window(event.event)?;
                let mut state = self.0.borrow_mut();

                let modifiers = modifiers_from_state(event.state);
                state.modifiers = modifiers;
                state.pre_key_char_down.take();
                let key_event_state = xkb_state_for_key_event(&state.xkb, event.state);

                let keystroke = {
                    let code = event.detail.into();
                    let mut keystroke = keystroke_from_xkb(&key_event_state, modifiers, code);
                    let keysym = key_event_state.key_get_one_sym(code);

                    if keysym.is_modifier_key() {
                        return Some(());
                    }

                    if let Some(mut compose_state) = state.compose_state.take() {
                        compose_state.feed(keysym);
                        match compose_state.status() {
                            xkbc::Status::Composed => {
                                state.pre_edit_text.take();
                                keystroke.key_char = compose_state.utf8();
                                if let Some(keysym) = compose_state.keysym() {
                                    keystroke.key = xkbc::keysym_get_name(keysym);
                                }
                            }
                            xkbc::Status::Composing => {
                                keystroke.key_char = None;
                                state.pre_edit_text = compose_state
                                    .utf8()
                                    .or(keystroke_underlying_dead_key(keysym));
                                let pre_edit =
                                    state.pre_edit_text.clone().unwrap_or(String::default());
                                drop(state);
                                window.handle_ime_preedit(pre_edit);
                                state = self.0.borrow_mut();
                            }
                            xkbc::Status::Cancelled => {
                                let pre_edit = state.pre_edit_text.take();
                                drop(state);
                                if let Some(pre_edit) = pre_edit {
                                    window.handle_ime_commit(pre_edit);
                                }
                                if let Some(current_key) = keystroke_underlying_dead_key(keysym) {
                                    window.handle_ime_preedit(current_key);
                                }
                                state = self.0.borrow_mut();
                                compose_state.feed(keysym);
                            }
                            _ => {}
                        }
                        state.compose_state = Some(compose_state);
                    }
                    keystroke
                };
                drop(state);
                window.handle_input(PlatformInput::KeyDown(gpui::KeyDownEvent {
                    keystroke,
                    is_held: false,
                    prefer_character_input: false,
                }));
            }
            Event::KeyRelease(event) => {
                let window = self.get_window(event.event)?;
                let mut state = self.0.borrow_mut();

                let modifiers = modifiers_from_state(event.state);
                state.modifiers = modifiers;
                let key_event_state = xkb_state_for_key_event(&state.xkb, event.state);

                let keystroke = {
                    let code = event.detail.into();
                    let keystroke = keystroke_from_xkb(&key_event_state, modifiers, code);
                    let keysym = key_event_state.key_get_one_sym(code);

                    if keysym.is_modifier_key() {
                        return Some(());
                    }

                    keystroke
                };
                drop(state);
                window.handle_input(PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke }));
            }
            event @ (Event::XinputButtonPress(_)
            | Event::XinputButtonRelease(_)
            | Event::XinputMotion(_)
            | Event::XinputEnter(_)
            | Event::XinputLeave(_)
            | Event::XinputHierarchy(_)
            | Event::XinputDeviceChanged(_)
            | Event::XinputGesturePinchBegin(_)
            | Event::XinputGesturePinchUpdate(_)
            | Event::XinputGesturePinchEnd(_)) => {
                return self.handle_xinput_event(event);
            }
        };

        Some(())
    }
}
