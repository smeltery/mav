use super::*;

struct DmabufProbeState {
    device: Option<u64>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for DmabufProbeState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, ()> for DmabufProbeState {
    fn event(
        _: &mut Self,
        _: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        _: zwp_linux_dmabuf_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1, ()> for DmabufProbeState {
    fn event(
        state: &mut Self,
        _: &zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
        event: zwp_linux_dmabuf_feedback_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_linux_dmabuf_feedback_v1::Event::MainDevice { device } = event {
            if let Ok(bytes) = <[u8; 8]>::try_from(device.as_slice()) {
                state.device = Some(u64::from_ne_bytes(bytes));
            }
        }
    }
}

pub(super) fn detect_compositor_gpu() -> Option<CompositorGpuHint> {
    let connection = Connection::connect_to_env().ok()?;
    let (globals, mut event_queue) = registry_queue_init::<DmabufProbeState>(&connection).ok()?;
    let queue_handle = event_queue.handle();

    let dmabuf: zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1 =
        globals.bind(&queue_handle, 4..=4, ()).ok()?;
    let feedback = dmabuf.get_default_feedback(&queue_handle, ());

    let mut state = DmabufProbeState { device: None };

    event_queue.roundtrip(&mut state).ok()?;

    feedback.destroy();
    dmabuf.destroy();

    crate::linux::compositor_gpu_hint_from_dev_t(state.device?)
}
