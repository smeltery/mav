use super::*;

pub(super) fn query_render_extent(
    xcb: &Rc<XCBConnection>,
    x_window: xproto::Window,
) -> anyhow::Result<Size<DevicePixels>> {
    let reply = get_reply(|| "X11 GetGeometry failed.", xcb.get_geometry(x_window))?;
    Ok(Size {
        width: DevicePixels(reply.width as i32),
        height: DevicePixels(reply.height as i32),
    })
}

pub(super) fn resize_edge_to_moveresize(edge: ResizeEdge) -> u32 {
    match edge {
        ResizeEdge::TopLeft => 0,
        ResizeEdge::Top => 1,
        ResizeEdge::TopRight => 2,
        ResizeEdge::Right => 3,
        ResizeEdge::BottomRight => 4,
        ResizeEdge::Bottom => 5,
        ResizeEdge::BottomLeft => 6,
        ResizeEdge::Left => 7,
    }
}

#[derive(Debug)]
pub(super) struct EdgeConstraints {
    pub(super) top_tiled: bool,
    #[allow(dead_code)]
    top_resizable: bool,

    pub(super) right_tiled: bool,
    #[allow(dead_code)]
    right_resizable: bool,

    pub(super) bottom_tiled: bool,
    #[allow(dead_code)]
    bottom_resizable: bool,

    pub(super) left_tiled: bool,
    #[allow(dead_code)]
    left_resizable: bool,
}

impl EdgeConstraints {
    pub(super) fn from_atom(atom: u32) -> Self {
        EdgeConstraints {
            top_tiled: (atom & (1 << 0)) != 0,
            top_resizable: (atom & (1 << 1)) != 0,
            right_tiled: (atom & (1 << 2)) != 0,
            right_resizable: (atom & (1 << 3)) != 0,
            bottom_tiled: (atom & (1 << 4)) != 0,
            bottom_resizable: (atom & (1 << 5)) != 0,
            left_tiled: (atom & (1 << 6)) != 0,
            left_resizable: (atom & (1 << 7)) != 0,
        }
    }

    pub(super) fn to_tiling(&self) -> Tiling {
        Tiling {
            top: self.top_tiled,
            right: self.right_tiled,
            bottom: self.bottom_tiled,
            left: self.left_tiled,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(super) struct Visual {
    id: xproto::Visualid,
    colormap: u32,
    depth: u8,
}

pub(super) struct VisualSet {
    pub(super) inherit: Visual,
    opaque: Option<Visual>,
    pub(super) transparent: Option<Visual>,
    pub(super) root: u32,
    pub(super) black_pixel: u32,
}

pub(super) fn find_visuals(xcb: &XCBConnection, screen_index: usize) -> VisualSet {
    let screen = &xcb.setup().roots[screen_index];
    let mut set = VisualSet {
        inherit: Visual {
            id: screen.root_visual,
            colormap: screen.default_colormap,
            depth: screen.root_depth,
        },
        opaque: None,
        transparent: None,
        root: screen.root,
        black_pixel: screen.black_pixel,
    };

    for depth_info in screen.allowed_depths.iter() {
        for visual_type in depth_info.visuals.iter() {
            let visual = Visual {
                id: visual_type.visual_id,
                colormap: 0,
                depth: depth_info.depth,
            };
            log::debug!(
                "Visual id: {}, class: {:?}, depth: {}, bits_per_value: {}, masks: 0x{:x} 0x{:x} 0x{:x}",
                visual_type.visual_id,
                visual_type.class,
                depth_info.depth,
                visual_type.bits_per_rgb_value,
                visual_type.red_mask,
                visual_type.green_mask,
                visual_type.blue_mask,
            );

            if (
                visual_type.red_mask,
                visual_type.green_mask,
                visual_type.blue_mask,
            ) != (0xFF0000, 0xFF00, 0xFF)
            {
                continue;
            }
            let color_mask = visual_type.red_mask | visual_type.green_mask | visual_type.blue_mask;
            let alpha_mask = color_mask as usize ^ ((1usize << depth_info.depth) - 1);

            if alpha_mask == 0 {
                if set.opaque.is_none() {
                    set.opaque = Some(visual);
                }
            } else {
                if set.transparent.is_none() {
                    set.transparent = Some(visual);
                }
            }
        }
    }

    set
}
