use super::*;

impl X11WindowState {
    pub fn new(
        handle: AnyWindowHandle,
        client: X11ClientStatePtr,
        executor: ForegroundExecutor,
        gpu_context: gpui_wgpu::GpuContext,
        compositor_gpu: Option<CompositorGpuHint>,
        params: WindowParams,
        xcb: &Rc<XCBConnection>,
        client_side_decorations_supported: bool,
        x_main_screen_index: usize,
        x_window: xproto::Window,
        atoms: &XcbAtoms,
        scale_factor: f32,
        appearance: WindowAppearance,
        parent_window: Option<X11WindowStatePtr>,
        supports_xinput_gestures: bool,
        is_bgr: bool,
    ) -> anyhow::Result<Self> {
        let x_screen_index = params
            .display_id
            .map_or(x_main_screen_index, |did| u64::from(did) as usize);

        let visual_set = find_visuals(xcb, x_screen_index);

        let visual = match visual_set.transparent {
            Some(visual) => visual,
            None => {
                log::warn!("Unable to find a transparent visual",);
                visual_set.inherit
            }
        };
        log::info!("Using {:?}", visual);

        let colormap = if visual.colormap != 0 {
            visual.colormap
        } else {
            let id = xcb.generate_id()?;
            log::info!("Creating colormap {}", id);
            check_reply(
                || format!("X11 CreateColormap failed. id: {}", id),
                xcb.create_colormap(xproto::ColormapAlloc::NONE, id, visual_set.root, visual.id),
            )?;
            id
        };

        let win_aux = xproto::CreateWindowAux::new()
            // https://stackoverflow.com/questions/43218127/x11-xlib-xcb-creating-a-window-requires-border-pixel-if-specifying-colormap-wh
            .border_pixel(visual_set.black_pixel)
            .colormap(colormap)
            .override_redirect((params.kind == WindowKind::PopUp) as u32)
            .event_mask(
                xproto::EventMask::EXPOSURE
                    | xproto::EventMask::STRUCTURE_NOTIFY
                    | xproto::EventMask::FOCUS_CHANGE
                    | xproto::EventMask::KEY_PRESS
                    | xproto::EventMask::KEY_RELEASE
                    | xproto::EventMask::PROPERTY_CHANGE
                    | xproto::EventMask::VISIBILITY_CHANGE,
            );

        let mut bounds = params.bounds.to_device_pixels(scale_factor);
        if bounds.size.width.0 == 0 || bounds.size.height.0 == 0 {
            log::warn!(
                "Window bounds contain a zero value. height={}, width={}. Falling back to defaults.",
                bounds.size.height.0,
                bounds.size.width.0
            );
            bounds.size.width = 800.into();
            bounds.size.height = 600.into();
        }

        check_reply(
            || {
                format!(
                    "X11 CreateWindow failed. depth: {}, x_window: {}, visual_set.root: {}, bounds.origin.x.0: {}, bounds.origin.y.0: {}, bounds.size.width.0: {}, bounds.size.height.0: {}",
                    visual.depth,
                    x_window,
                    visual_set.root,
                    bounds.origin.x.0 + 2,
                    bounds.origin.y.0,
                    bounds.size.width.0,
                    bounds.size.height.0
                )
            },
            xcb.create_window(
                visual.depth,
                x_window,
                visual_set.root,
                (bounds.origin.x.0 + 2) as i16,
                bounds.origin.y.0 as i16,
                bounds.size.width.0 as u16,
                bounds.size.height.0 as u16,
                0,
                xproto::WindowClass::INPUT_OUTPUT,
                visual.id,
                &win_aux,
            ),
        )?;

        // Collect errors during setup, so that window can be destroyed on failure.
        let setup_result = maybe!({
            let pid = std::process::id();
            check_reply(
                || "X11 ChangeProperty for _NET_WM_PID failed.",
                xcb.change_property32(
                    xproto::PropMode::REPLACE,
                    x_window,
                    atoms._NET_WM_PID,
                    xproto::AtomEnum::CARDINAL,
                    &[pid],
                ),
            )?;

            let reply = get_reply(|| "X11 GetGeometry failed.", xcb.get_geometry(x_window))?;
            if reply.x == 0 && reply.y == 0 {
                bounds.origin.x.0 += 2;
                // Work around a bug where our rendered content appears
                // outside the window bounds when opened at the default position
                // (14px, 49px on X + Gnome + Ubuntu 22).
                let x = bounds.origin.x.0;
                let y = bounds.origin.y.0;
                check_reply(
                    || format!("X11 ConfigureWindow failed. x: {}, y: {}", x, y),
                    xcb.configure_window(x_window, &xproto::ConfigureWindowAux::new().x(x).y(y)),
                )?;
            }
            if let Some(titlebar) = params.titlebar
                && let Some(title) = titlebar.title
            {
                check_reply(
                    || "X11 ChangeProperty8 on WM_NAME failed.",
                    xcb.change_property8(
                        xproto::PropMode::REPLACE,
                        x_window,
                        xproto::AtomEnum::WM_NAME,
                        xproto::AtomEnum::STRING,
                        title.as_bytes(),
                    ),
                )?;
                check_reply(
                    || "X11 ChangeProperty8 on _NET_WM_NAME failed.",
                    xcb.change_property8(
                        xproto::PropMode::REPLACE,
                        x_window,
                        atoms._NET_WM_NAME,
                        atoms.UTF8_STRING,
                        title.as_bytes(),
                    ),
                )?;
            }

            if params.kind == WindowKind::PopUp {
                check_reply(
                    || "X11 ChangeProperty32 setting window type for pop-up failed.",
                    xcb.change_property32(
                        xproto::PropMode::REPLACE,
                        x_window,
                        atoms._NET_WM_WINDOW_TYPE,
                        xproto::AtomEnum::ATOM,
                        &[atoms._NET_WM_WINDOW_TYPE_NOTIFICATION],
                    ),
                )?;
            }

            if params.kind == WindowKind::Floating || params.kind == WindowKind::Dialog {
                if let Some(parent_window) = parent_window.as_ref().map(|w| w.x_window) {
                    // WM_TRANSIENT_FOR hint indicating the main application window. For floating windows, we set
                    // a parent window (WM_TRANSIENT_FOR) such that the window manager knows where to
                    // place the floating window in relation to the main window.
                    // https://specifications.freedesktop.org/wm-spec/1.4/ar01s05.html
                    check_reply(
                        || "X11 ChangeProperty32 setting WM_TRANSIENT_FOR for floating window failed.",
                        xcb.change_property32(
                            xproto::PropMode::REPLACE,
                            x_window,
                            atoms.WM_TRANSIENT_FOR,
                            xproto::AtomEnum::WINDOW,
                            &[parent_window],
                        ),
                    )?;
                }
            }

            let parent = if params.kind == WindowKind::Dialog
                && let Some(parent) = parent_window
            {
                parent.add_child(x_window);

                Some(parent)
            } else {
                None
            };

            if params.kind == WindowKind::Dialog {
                // _NET_WM_WINDOW_TYPE_DIALOG indicates that this is a dialog (floating) window
                // https://specifications.freedesktop.org/wm-spec/1.4/ar01s05.html
                check_reply(
                    || "X11 ChangeProperty32 setting window type for dialog window failed.",
                    xcb.change_property32(
                        xproto::PropMode::REPLACE,
                        x_window,
                        atoms._NET_WM_WINDOW_TYPE,
                        xproto::AtomEnum::ATOM,
                        &[atoms._NET_WM_WINDOW_TYPE_DIALOG],
                    ),
                )?;

                // We set the modal state for dialog windows, so that the window manager
                // can handle it appropriately (e.g., prevent interaction with the parent window
                // while the dialog is open).
                check_reply(
                    || "X11 ChangeProperty32 setting modal state for dialog window failed.",
                    xcb.change_property32(
                        xproto::PropMode::REPLACE,
                        x_window,
                        atoms._NET_WM_STATE,
                        xproto::AtomEnum::ATOM,
                        &[atoms._NET_WM_STATE_MODAL],
                    ),
                )?;
            }

            check_reply(
                || "X11 ChangeProperty32 setting protocols failed.",
                xcb.change_property32(
                    xproto::PropMode::REPLACE,
                    x_window,
                    atoms.WM_PROTOCOLS,
                    xproto::AtomEnum::ATOM,
                    &[atoms.WM_DELETE_WINDOW, atoms._NET_WM_SYNC_REQUEST],
                ),
            )?;

            get_reply(
                || "X11 sync protocol initialize failed.",
                sync::initialize(xcb, 3, 1),
            )?;
            let sync_request_counter = xcb.generate_id()?;
            check_reply(
                || "X11 sync CreateCounter failed.",
                sync::create_counter(xcb, sync_request_counter, sync::Int64 { lo: 0, hi: 0 }),
            )?;

            check_reply(
                || "X11 ChangeProperty32 setting sync request counter failed.",
                xcb.change_property32(
                    xproto::PropMode::REPLACE,
                    x_window,
                    atoms._NET_WM_SYNC_REQUEST_COUNTER,
                    xproto::AtomEnum::CARDINAL,
                    &[sync_request_counter],
                ),
            )?;

            let mut xi_event_mask = xinput::XIEventMask::MOTION
                | xinput::XIEventMask::BUTTON_PRESS
                | xinput::XIEventMask::BUTTON_RELEASE
                | xinput::XIEventMask::ENTER
                | xinput::XIEventMask::LEAVE;
            if supports_xinput_gestures {
                // x11rb 0.13 doesn't define XIEventMask constants for gesture
                // events, so we construct them from the event opcodes (each
                // XInput event type N maps to mask bit N).
                xi_event_mask |=
                    xinput::XIEventMask::from(1u32 << xinput::GESTURE_PINCH_BEGIN_EVENT)
                        | xinput::XIEventMask::from(1u32 << xinput::GESTURE_PINCH_UPDATE_EVENT)
                        | xinput::XIEventMask::from(1u32 << xinput::GESTURE_PINCH_END_EVENT);
            }
            check_reply(
                || "X11 XiSelectEvents failed.",
                xcb.xinput_xi_select_events(
                    x_window,
                    &[xinput::EventMask {
                        deviceid: XINPUT_ALL_DEVICE_GROUPS,
                        mask: vec![xi_event_mask],
                    }],
                ),
            )?;

            check_reply(
                || "X11 XiSelectEvents for device changes failed.",
                xcb.xinput_xi_select_events(
                    x_window,
                    &[xinput::EventMask {
                        deviceid: XINPUT_ALL_DEVICES,
                        mask: vec![
                            xinput::XIEventMask::HIERARCHY | xinput::XIEventMask::DEVICE_CHANGED,
                        ],
                    }],
                ),
            )?;

            xcb_flush(xcb);

            let mut renderer = {
                let raw_window = RawWindow {
                    connection: as_raw_xcb_connection::AsRawXcbConnection::as_raw_xcb_connection(
                        xcb,
                    ) as *mut _,
                    screen_id: x_screen_index,
                    window_id: x_window,
                    visual_id: visual.id,
                };
                let config = WgpuSurfaceConfig {
                    // Note: this has to be done after the GPU init, or otherwise
                    // the sizes are immediately invalidated.
                    size: query_render_extent(xcb, x_window)?,
                    // We set it to transparent by default, even if we have client-side
                    // decorations, since those seem to work on X11 even without `true` here.
                    // If the window appearance changes, then the renderer will get updated
                    // too
                    transparent: false,
                    preferred_present_mode: None,
                };
                WgpuRenderer::new(gpu_context, &raw_window, config, compositor_gpu)?
            };

            renderer.set_subpixel_layout(is_bgr);

            // Set max window size hints based on the GPU's maximum texture dimension.
            // This prevents the window from being resized larger than what the GPU can render.
            let max_texture_size = renderer.max_texture_size();
            let mut size_hints = WmSizeHints::new();
            if let Some(size) = params.window_min_size {
                size_hints.min_size =
                    Some((f32::from(size.width) as i32, f32::from(size.height) as i32));
            }
            size_hints.max_size = Some((max_texture_size as i32, max_texture_size as i32));
            check_reply(
                || {
                    format!(
                        "X11 change of WM_SIZE_HINTS failed. max_size: {:?}",
                        max_texture_size
                    )
                },
                size_hints.set_normal_hints(xcb, x_window),
            )?;

            if let Some(image) = params.icon {
                // https://specifications.freedesktop.org/wm-spec/1.4/ar01s05.html#id-1.6.13
                let property_size = 2 + (image.width() * image.height()) as usize;
                let mut property_data: Vec<u32> = Vec::with_capacity(property_size);
                property_data.push(image.width());
                property_data.push(image.height());
                property_data.extend(image.pixels().map(|px| {
                    let [r, g, b, a]: [u8; 4] = px.0;
                    u32::from_le_bytes([b, g, r, a])
                }));

                check_reply(
                    || "X11 ChangeProperty32 for _NET_ICON_NAME failed.",
                    xcb.change_property32(
                        xproto::PropMode::REPLACE,
                        x_window,
                        atoms._NET_WM_ICON,
                        xproto::AtomEnum::CARDINAL,
                        &property_data,
                    ),
                )?;
            }

            let display = Rc::new(X11Display::new(xcb, scale_factor, x_screen_index)?);

            Ok(Self {
                parent,
                children: FxHashSet::default(),
                client,
                executor,
                display,
                x_root_window: visual_set.root,
                x_screen_index,
                visual_id: visual.id,
                bounds: bounds.to_pixels(scale_factor),
                scale_factor,
                renderer,
                atoms: *atoms,
                input_handler: None,
                active: false,
                hovered: false,
                force_render_after_recovery: false,
                fullscreen: false,
                maximized_vertical: false,
                maximized_horizontal: false,
                hidden: false,
                appearance,
                handle,
                background_appearance: WindowBackgroundAppearance::Opaque,
                destroyed: false,
                client_side_decorations_supported,
                decorations: WindowDecorations::Server,
                last_insets: [0, 0, 0, 0],
                edge_constraints: None,
                accesskit_adapter: None,
                counter_id: sync_request_counter,
                last_sync_counter: None,
            })
        });

        if setup_result.is_err() {
            check_reply(
                || "X11 DestroyWindow failed while cleaning it up after setup failure.",
                xcb.destroy_window(x_window),
            )?;
            xcb_flush(xcb);
        }

        setup_result
    }

    pub(super) fn content_size(&self) -> Size<Pixels> {
        self.bounds.size
    }
}
