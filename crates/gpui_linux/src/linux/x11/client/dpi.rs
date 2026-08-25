use super::*;

enum DpiMode {
    Randr,
    Scale(f32),
    NotSet,
}

pub(super) fn get_scale_factor(
    connection: &XCBConnection,
    resource_database: &Database,
    screen_index: usize,
) -> f32 {
    let env_dpi = std::env::var(GPUI_X11_SCALE_FACTOR_ENV)
        .ok()
        .map(|var| {
            if var.to_lowercase() == "randr" {
                DpiMode::Randr
            } else if let Ok(scale) = var.parse::<f32>() {
                if valid_scale_factor(scale) {
                    DpiMode::Scale(scale)
                } else {
                    panic!(
                        "`{}` must be a positive normal number or `randr`. Got `{}`",
                        GPUI_X11_SCALE_FACTOR_ENV, var
                    );
                }
            } else if var.is_empty() {
                DpiMode::NotSet
            } else {
                panic!(
                    "`{}` must be a positive number or `randr`. Got `{}`",
                    GPUI_X11_SCALE_FACTOR_ENV, var
                );
            }
        })
        .unwrap_or(DpiMode::NotSet);

    match env_dpi {
        DpiMode::Scale(scale) => {
            log::info!(
                "Using scale factor from {}: {}",
                GPUI_X11_SCALE_FACTOR_ENV,
                scale
            );
            return scale;
        }
        DpiMode::Randr => {
            if let Some(scale) = get_randr_scale_factor(connection, screen_index) {
                log::info!(
                    "Using RandR scale factor from {}=randr: {}",
                    GPUI_X11_SCALE_FACTOR_ENV,
                    scale
                );
                return scale;
            }
            log::warn!("Failed to calculate RandR scale factor, falling back to default");
            return 1.0;
        }
        DpiMode::NotSet => {}
    }

    // TODO: Use scale factor from XSettings here

    if let Some(dpi) = resource_database
        .get_value::<f32>("Xft.dpi", "Xft.dpi")
        .ok()
        .flatten()
    {
        let scale = dpi / 96.0; // base dpi
        log::info!("Using scale factor from Xft.dpi: {}", scale);
        return scale;
    }

    if let Some(scale) = get_randr_scale_factor(connection, screen_index) {
        log::info!("Using RandR scale factor: {}", scale);
        return scale;
    }

    log::info!("Using default scale factor: 1.0");
    1.0
}

fn get_randr_scale_factor(connection: &XCBConnection, screen_index: usize) -> Option<f32> {
    let root = connection.setup().roots.get(screen_index)?.root;

    let version_cookie = connection.randr_query_version(1, 6).ok()?;
    let version_reply = version_cookie.reply().ok()?;
    if version_reply.major_version < 1
        || (version_reply.major_version == 1 && version_reply.minor_version < 5)
    {
        return legacy_get_randr_scale_factor(connection, root); // for randr <1.5
    }

    let monitors_cookie = connection.randr_get_monitors(root, true).ok()?; // true for active only
    let monitors_reply = monitors_cookie.reply().ok()?;

    let mut fallback_scale: Option<f32> = None;
    for monitor in monitors_reply.monitors {
        if monitor.width_in_millimeters == 0 || monitor.height_in_millimeters == 0 {
            continue;
        }
        let scale_factor = get_dpi_factor(
            (monitor.width as u32, monitor.height as u32),
            (
                monitor.width_in_millimeters as u64,
                monitor.height_in_millimeters as u64,
            ),
        );
        if monitor.primary {
            return Some(scale_factor);
        } else if fallback_scale.is_none() {
            fallback_scale = Some(scale_factor);
        }
    }

    fallback_scale
}

fn legacy_get_randr_scale_factor(connection: &XCBConnection, root: u32) -> Option<f32> {
    let primary_cookie = connection.randr_get_output_primary(root).ok()?;
    let primary_reply = primary_cookie.reply().ok()?;
    let primary_output = primary_reply.output;

    let primary_output_cookie = connection
        .randr_get_output_info(primary_output, x11rb::CURRENT_TIME)
        .ok()?;
    let primary_output_info = primary_output_cookie.reply().ok()?;

    // try primary
    if primary_output_info.connection == randr::Connection::CONNECTED
        && primary_output_info.mm_width > 0
        && primary_output_info.mm_height > 0
        && primary_output_info.crtc != 0
    {
        let crtc_cookie = connection
            .randr_get_crtc_info(primary_output_info.crtc, x11rb::CURRENT_TIME)
            .ok()?;
        let crtc_info = crtc_cookie.reply().ok()?;

        if crtc_info.width > 0 && crtc_info.height > 0 {
            let scale_factor = get_dpi_factor(
                (crtc_info.width as u32, crtc_info.height as u32),
                (
                    primary_output_info.mm_width as u64,
                    primary_output_info.mm_height as u64,
                ),
            );
            return Some(scale_factor);
        }
    }

    // fallback: full scan
    let resources_cookie = connection.randr_get_screen_resources_current(root).ok()?;
    let screen_resources = resources_cookie.reply().ok()?;

    let mut crtc_cookies = Vec::with_capacity(screen_resources.crtcs.len());
    for &crtc in &screen_resources.crtcs {
        if let Ok(cookie) = connection.randr_get_crtc_info(crtc, x11rb::CURRENT_TIME) {
            crtc_cookies.push((crtc, cookie));
        }
    }

    let mut crtc_infos: HashMap<randr::Crtc, randr::GetCrtcInfoReply> = HashMap::default();
    let mut valid_outputs: HashSet<randr::Output> = HashSet::new();
    for (crtc, cookie) in crtc_cookies {
        if let Ok(reply) = cookie.reply()
            && reply.width > 0
            && reply.height > 0
            && !reply.outputs.is_empty()
        {
            crtc_infos.insert(crtc, reply.clone());
            valid_outputs.extend(&reply.outputs);
        }
    }

    if valid_outputs.is_empty() {
        return None;
    }

    let mut output_cookies = Vec::with_capacity(valid_outputs.len());
    for &output in &valid_outputs {
        if let Ok(cookie) = connection.randr_get_output_info(output, x11rb::CURRENT_TIME) {
            output_cookies.push((output, cookie));
        }
    }
    let mut output_infos: HashMap<randr::Output, randr::GetOutputInfoReply> = HashMap::default();
    for (output, cookie) in output_cookies {
        if let Ok(reply) = cookie.reply() {
            output_infos.insert(output, reply);
        }
    }

    let mut fallback_scale: Option<f32> = None;
    for crtc_info in crtc_infos.values() {
        for &output in &crtc_info.outputs {
            if let Some(output_info) = output_infos.get(&output) {
                if output_info.connection != randr::Connection::CONNECTED {
                    continue;
                }

                if output_info.mm_width == 0 || output_info.mm_height == 0 {
                    continue;
                }

                let scale_factor = get_dpi_factor(
                    (crtc_info.width as u32, crtc_info.height as u32),
                    (output_info.mm_width as u64, output_info.mm_height as u64),
                );

                if output != primary_output && fallback_scale.is_none() {
                    fallback_scale = Some(scale_factor);
                }
            }
        }
    }

    fallback_scale
}

fn get_dpi_factor((width_px, height_px): (u32, u32), (width_mm, height_mm): (u64, u64)) -> f32 {
    let ppmm = ((width_px as f64 * height_px as f64) / (width_mm as f64 * height_mm as f64)).sqrt(); // pixels per mm

    const MM_PER_INCH: f64 = 25.4;
    const BASE_DPI: f64 = 96.0;
    const QUANTIZE_STEP: f64 = 12.0; // e.g. 1.25 = 15/12, 1.5 = 18/12, 1.75 = 21/12, 2.0 = 24/12
    const MIN_SCALE: f64 = 1.0;
    const MAX_SCALE: f64 = 20.0;

    let dpi_factor =
        ((ppmm * (QUANTIZE_STEP * MM_PER_INCH / BASE_DPI)).round() / QUANTIZE_STEP).max(MIN_SCALE);

    let validated_factor = if dpi_factor <= MAX_SCALE {
        dpi_factor
    } else {
        MIN_SCALE
    };

    if valid_scale_factor(validated_factor as f32) {
        validated_factor as f32
    } else {
        log::warn!(
            "Calculated DPI factor {} is invalid, using 1.0",
            validated_factor
        );
        1.0
    }
}

#[inline]
fn valid_scale_factor(scale_factor: f32) -> bool {
    scale_factor.is_sign_positive() && scale_factor.is_normal()
}

#[inline]
pub(super) fn xkb_state_for_key_event(
    xkb: &xkbc::State,
    event_state: xproto::KeyButMask,
) -> xkbc::State {
    let keymap = xkb.get_keymap();
    let mut key_event_state = xkbc::State::new(&keymap);

    let latched_modifiers = xkb.serialize_mods(xkbc::STATE_MODS_LATCHED);
    let locked_modifiers = xkb.serialize_mods(xkbc::STATE_MODS_LOCKED);
    let active_modifier_mask: xkbc::ModMask = u16::from(
        event_state
            & (xproto::KeyButMask::SHIFT
                | xproto::KeyButMask::LOCK
                | xproto::KeyButMask::CONTROL
                | xproto::KeyButMask::MOD1
                | xproto::KeyButMask::MOD2
                | xproto::KeyButMask::MOD3
                | xproto::KeyButMask::MOD4
                | xproto::KeyButMask::MOD5),
    )
    .into();
    let depressed_modifiers = active_modifier_mask & !(latched_modifiers | locked_modifiers);

    key_event_state.update_mask(
        depressed_modifiers,
        latched_modifiers,
        locked_modifiers,
        xkb.serialize_layout(xkbc::STATE_LAYOUT_DEPRESSED),
        xkb.serialize_layout(xkbc::STATE_LAYOUT_LATCHED),
        xkb.serialize_layout(xkbc::STATE_LAYOUT_LOCKED),
    );

    key_event_state
}
