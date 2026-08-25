use super::*;

// Adapted from:
// https://docs.rs/winit/0.29.11/src/winit/platform_impl/linux/x11/monitor.rs.html#103-111
pub fn mode_refresh_rate(mode: &randr::ModeInfo) -> Duration {
    if mode.dot_clock == 0 || mode.htotal == 0 || mode.vtotal == 0 {
        return Duration::from_millis(16);
    }

    let millihertz = mode.dot_clock as u64 * 1_000 / (mode.htotal as u64 * mode.vtotal as u64);
    let micros = 1_000_000_000 / millihertz;
    log::info!("Refreshing every {}ms", micros / 1_000);
    Duration::from_micros(micros)
}

fn fp3232_to_f32(value: xinput::Fp3232) -> f32 {
    value.integral as f32 + value.frac as f32 / u32::MAX as f32
}

pub(super) fn detect_compositor_gpu(
    xcb_connection: &XCBConnection,
    screen: &xproto::Screen,
) -> Option<CompositorGpuHint> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;

    xcb_connection
        .extension_information(dri3::X11_EXTENSION_NAME)
        .ok()??;

    let reply = dri3::open(xcb_connection, screen.root, 0)
        .ok()?
        .reply()
        .ok()?;
    let fd = reply.device_fd;

    let path = format!("/proc/self/fd/{}", fd.as_raw_fd());
    let metadata = std::fs::metadata(&path).ok()?;

    crate::linux::compositor_gpu_hint_from_dev_t(metadata.rdev())
}

pub(super) fn check_compositor_present(
    xcb_connection: &XCBConnection,
    root: xproto::Window,
) -> bool {
    // Method 1: Check for _NET_WM_CM_S{root}
    let atom_name = format!("_NET_WM_CM_S{}", root);
    let atom1 = get_reply(
        || format!("Failed to intern {atom_name}"),
        xcb_connection.intern_atom(false, atom_name.as_bytes()),
    );
    let method1 = match atom1.log_with_level(Level::Debug) {
        Some(reply) if reply.atom != x11rb::NONE => {
            let atom = reply.atom;
            get_reply(
                || format!("Failed to get {atom_name} owner"),
                xcb_connection.get_selection_owner(atom),
            )
            .map(|reply| reply.owner != 0)
            .log_with_level(Level::Debug)
            .unwrap_or(false)
        }
        _ => false,
    };

    // Method 2: Check for _NET_WM_CM_OWNER
    let atom_name = "_NET_WM_CM_OWNER";
    let atom2 = get_reply(
        || format!("Failed to intern {atom_name}"),
        xcb_connection.intern_atom(false, atom_name.as_bytes()),
    );
    let method2 = match atom2.log_with_level(Level::Debug) {
        Some(reply) if reply.atom != x11rb::NONE => {
            let atom = reply.atom;
            get_reply(
                || format!("Failed to get {atom_name}"),
                xcb_connection.get_property(false, root, atom, xproto::AtomEnum::WINDOW, 0, 1),
            )
            .map(|reply| reply.value_len > 0)
            .unwrap_or(false)
        }
        _ => return false,
    };

    // Method 3: Check for _NET_SUPPORTING_WM_CHECK
    let atom_name = "_NET_SUPPORTING_WM_CHECK";
    let atom3 = get_reply(
        || format!("Failed to intern {atom_name}"),
        xcb_connection.intern_atom(false, atom_name.as_bytes()),
    );
    let method3 = match atom3.log_with_level(Level::Debug) {
        Some(reply) if reply.atom != x11rb::NONE => {
            let atom = reply.atom;
            get_reply(
                || format!("Failed to get {atom_name}"),
                xcb_connection.get_property(false, root, atom, xproto::AtomEnum::WINDOW, 0, 1),
            )
            .map(|reply| reply.value_len > 0)
            .unwrap_or(false)
        }
        _ => return false,
    };

    log::debug!(
        "Compositor detection: _NET_WM_CM_S?={}, _NET_WM_CM_OWNER={}, _NET_SUPPORTING_WM_CHECK={}",
        method1,
        method2,
        method3
    );

    method1 || method2 || method3
}

pub(super) fn check_gtk_frame_extents_supported(
    xcb_connection: &XCBConnection,
    atoms: &XcbAtoms,
    root: xproto::Window,
) -> bool {
    let Some(supported_atoms) = get_reply(
        || "Failed to get _NET_SUPPORTED",
        xcb_connection.get_property(
            false,
            root,
            atoms._NET_SUPPORTED,
            xproto::AtomEnum::ATOM,
            0,
            1024,
        ),
    )
    .log_with_level(Level::Debug) else {
        return false;
    };

    let supported_atom_ids: Vec<u32> = supported_atoms
        .value
        .chunks_exact(4)
        .filter_map(|chunk| chunk.try_into().ok().map(u32::from_ne_bytes))
        .collect();

    supported_atom_ids.contains(&atoms._GTK_FRAME_EXTENTS)
}

pub(super) fn xdnd_is_atom_supported(atom: u32, atoms: &XcbAtoms) -> bool {
    atom == atoms.TEXT
        || atom == atoms.STRING
        || atom == atoms.UTF8_STRING
        || atom == atoms.TEXT_PLAIN
        || atom == atoms.TEXT_PLAIN_UTF8
        || atom == atoms.TextUriList
}

pub(super) fn xdnd_get_supported_atom(
    xcb_connection: &XCBConnection,
    supported_atoms: &XcbAtoms,
    target: xproto::Window,
) -> u32 {
    if let Some(reply) = get_reply(
        || "Failed to get XDnD supported atoms",
        xcb_connection.get_property(
            false,
            target,
            supported_atoms.XdndTypeList,
            AtomEnum::ANY,
            0,
            1024,
        ),
    )
    .log_with_level(Level::Warn)
        && let Some(atoms) = reply.value32()
    {
        for atom in atoms {
            if xdnd_is_atom_supported(atom, supported_atoms) {
                return atom;
            }
        }
    }
    0
}

pub(super) fn xdnd_send_finished(
    xcb_connection: &XCBConnection,
    atoms: &XcbAtoms,
    source: xproto::Window,
    target: xproto::Window,
) {
    let message = ClientMessageEvent {
        format: 32,
        window: target,
        type_: atoms.XdndFinished,
        data: ClientMessageData::from([source, 1, atoms.XdndActionCopy, 0, 0]),
        sequence: 0,
        response_type: xproto::CLIENT_MESSAGE_EVENT,
    };
    check_reply(
        || "Failed to send XDnD finished event",
        xcb_connection.send_event(false, target, EventMask::default(), message),
    )
    .log_err();
    xcb_connection.flush().log_err();
}

pub(super) fn xdnd_send_status(
    xcb_connection: &XCBConnection,
    atoms: &XcbAtoms,
    source: xproto::Window,
    target: xproto::Window,
    action: u32,
) {
    let message = ClientMessageEvent {
        format: 32,
        window: target,
        type_: atoms.XdndStatus,
        data: ClientMessageData::from([source, 1, 0, 0, action]),
        sequence: 0,
        response_type: xproto::CLIENT_MESSAGE_EVENT,
    };
    check_reply(
        || "Failed to send XDnD status event",
        xcb_connection.send_event(false, target, EventMask::default(), message),
    )
    .log_err();
    xcb_connection.flush().log_err();
}

/// Recomputes `pointer_device_states` by querying all pointer devices.
/// When a device is present in `scroll_values_to_preserve`, its value for `ScrollAxisState.scroll_value` is used.
pub(super) fn current_pointer_device_states(
    xcb_connection: &XCBConnection,
    scroll_values_to_preserve: &BTreeMap<xinput::DeviceId, PointerDeviceState>,
) -> Option<BTreeMap<xinput::DeviceId, PointerDeviceState>> {
    let devices_query_result = get_reply(
        || "Failed to query XInput devices",
        xcb_connection.xinput_xi_query_device(XINPUT_ALL_DEVICES),
    )
    .log_err()?;

    let mut pointer_device_states = BTreeMap::new();
    pointer_device_states.extend(
        devices_query_result
            .infos
            .iter()
            .filter(|info| is_pointer_device(info.type_))
            .filter_map(|info| {
                let scroll_data = info
                    .classes
                    .iter()
                    .filter_map(|class| class.data.as_scroll())
                    .copied()
                    .rev()
                    .collect::<Vec<_>>();
                let old_state = scroll_values_to_preserve.get(&info.deviceid);
                let old_horizontal = old_state.map(|state| &state.horizontal);
                let old_vertical = old_state.map(|state| &state.vertical);
                let horizontal = scroll_data
                    .iter()
                    .find(|data| data.scroll_type == xinput::ScrollType::HORIZONTAL)
                    .map(|data| scroll_data_to_axis_state(data, old_horizontal));
                let vertical = scroll_data
                    .iter()
                    .find(|data| data.scroll_type == xinput::ScrollType::VERTICAL)
                    .map(|data| scroll_data_to_axis_state(data, old_vertical));
                if horizontal.is_none() && vertical.is_none() {
                    None
                } else {
                    Some((
                        info.deviceid,
                        PointerDeviceState {
                            horizontal: horizontal.unwrap_or_else(Default::default),
                            vertical: vertical.unwrap_or_else(Default::default),
                        },
                    ))
                }
            }),
    );
    if pointer_device_states.is_empty() {
        log::error!("Found no xinput mouse pointers.");
    }
    Some(pointer_device_states)
}

/// Returns true if the device is a pointer device. Does not include pointer device groups.
pub(super) fn is_pointer_device(type_: xinput::DeviceType) -> bool {
    type_ == xinput::DeviceType::SLAVE_POINTER
}

fn scroll_data_to_axis_state(
    data: &xinput::DeviceClassDataScroll,
    old_axis_state_with_valid_scroll_value: Option<&ScrollAxisState>,
) -> ScrollAxisState {
    ScrollAxisState {
        valuator_number: Some(data.number),
        multiplier: SCROLL_LINES / fp3232_to_f32(data.increment),
        scroll_value: old_axis_state_with_valid_scroll_value.and_then(|state| state.scroll_value),
    }
}

pub(super) fn reset_all_pointer_device_scroll_positions(
    pointer_device_states: &mut BTreeMap<xinput::DeviceId, PointerDeviceState>,
) {
    pointer_device_states
        .iter_mut()
        .for_each(|(_, device_state)| reset_pointer_device_scroll_positions(device_state));
}

pub(super) fn reset_pointer_device_scroll_positions(pointer: &mut PointerDeviceState) {
    pointer.horizontal.scroll_value = None;
    pointer.vertical.scroll_value = None;
}

/// Returns the scroll delta for a smooth scrolling motion event, or `None` if no scroll data is present.
pub(super) fn get_scroll_delta_and_update_state(
    pointer: &mut PointerDeviceState,
    event: &xinput::MotionEvent,
) -> Option<Point<f32>> {
    let delta_x = get_axis_scroll_delta_and_update_state(event, &mut pointer.horizontal);
    let delta_y = get_axis_scroll_delta_and_update_state(event, &mut pointer.vertical);
    if delta_x.is_some() || delta_y.is_some() {
        Some(Point::new(delta_x.unwrap_or(0.0), delta_y.unwrap_or(0.0)))
    } else {
        None
    }
}

fn get_axis_scroll_delta_and_update_state(
    event: &xinput::MotionEvent,
    axis: &mut ScrollAxisState,
) -> Option<f32> {
    let axis_index = get_valuator_axis_index(&event.valuator_mask, axis.valuator_number?)?;
    if let Some(axis_value) = event.axisvalues.get(axis_index) {
        let new_scroll = fp3232_to_f32(*axis_value);
        let delta_scroll = axis
            .scroll_value
            .map(|old_scroll| (old_scroll - new_scroll) * axis.multiplier);
        axis.scroll_value = Some(new_scroll);
        delta_scroll
    } else {
        log::error!("Encountered invalid XInput valuator_mask, scrolling may not work properly.");
        None
    }
}

pub(super) fn make_scroll_wheel_event(
    position: Point<Pixels>,
    scroll_delta: Point<f32>,
    modifiers: Modifiers,
) -> gpui::ScrollWheelEvent {
    // When shift is held down, vertical scrolling turns into horizontal scrolling.
    let delta = if modifiers.shift {
        Point {
            x: scroll_delta.y,
            y: 0.0,
        }
    } else {
        scroll_delta
    };
    gpui::ScrollWheelEvent {
        position,
        delta: ScrollDelta::Lines(delta),
        modifiers,
        touch_phase: TouchPhase::default(),
    }
}

pub(super) fn create_invisible_cursor(
    connection: &XCBConnection,
) -> anyhow::Result<crate::linux::x11::client::xproto::Cursor> {
    let empty_pixmap = connection.generate_id()?;
    let root = connection.setup().roots[0].root;
    connection.create_pixmap(1, empty_pixmap, root, 1, 1)?;

    let cursor = connection.generate_id()?;
    connection.create_cursor(cursor, empty_pixmap, empty_pixmap, 0, 0, 0, 0, 0, 0, 0, 0)?;

    connection.free_pixmap(empty_pixmap)?;

    xcb_flush(connection);
    Ok(cursor)
}
