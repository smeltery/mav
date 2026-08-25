use super::*;

impl BreakpointEntry {
    pub(super) fn render(
        &mut self,
        strip_mode: Option<ActiveBreakpointStripMode>,
        props: SupportedBreakpointProperties,
        ix: usize,
        is_selected: bool,
        focus_handle: FocusHandle,
    ) -> ListItem {
        match &mut self.kind {
            BreakpointEntryKind::LineBreakpoint(line_breakpoint) => line_breakpoint.render(
                props,
                strip_mode,
                ix,
                is_selected,
                focus_handle,
                self.weak.clone(),
            ),
            BreakpointEntryKind::ExceptionBreakpoint(exception_breakpoint) => exception_breakpoint
                .render(
                    props.for_exception_breakpoints(),
                    strip_mode,
                    ix,
                    is_selected,
                    focus_handle,
                    self.weak.clone(),
                ),
            BreakpointEntryKind::DataBreakpoint(data_breakpoint) => data_breakpoint.render(
                props.for_data_breakpoints(),
                strip_mode,
                ix,
                is_selected,
                focus_handle,
                self.weak.clone(),
            ),
        }
    }

    pub(super) fn id(&self) -> SharedString {
        match &self.kind {
            BreakpointEntryKind::LineBreakpoint(line_breakpoint) => format!(
                "source-breakpoint-control-strip-{:?}:{}",
                line_breakpoint.breakpoint.path, line_breakpoint.breakpoint.row
            )
            .into(),
            BreakpointEntryKind::ExceptionBreakpoint(exception_breakpoint) => format!(
                "exception-breakpoint-control-strip--{}",
                exception_breakpoint.id
            )
            .into(),
            BreakpointEntryKind::DataBreakpoint(data_breakpoint) => format!(
                "data-breakpoint-control-strip--{}",
                data_breakpoint.0.dap.data_id
            )
            .into(),
        }
    }

    pub(super) fn has_log(&self) -> bool {
        match &self.kind {
            BreakpointEntryKind::LineBreakpoint(line_breakpoint) => {
                line_breakpoint.breakpoint.message.is_some()
            }
            _ => false,
        }
    }

    pub(super) fn has_condition(&self) -> bool {
        match &self.kind {
            BreakpointEntryKind::LineBreakpoint(line_breakpoint) => {
                line_breakpoint.breakpoint.condition.is_some()
            }
            // We don't support conditions on exception/data breakpoints
            _ => false,
        }
    }

    pub(super) fn has_hit_condition(&self) -> bool {
        match &self.kind {
            BreakpointEntryKind::LineBreakpoint(line_breakpoint) => {
                line_breakpoint.breakpoint.hit_condition.is_some()
            }
            _ => false,
        }
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy)]
    pub(super) struct SupportedBreakpointProperties: u32 {
        const LOG = 1 << 0;
        const CONDITION = 1 << 1;
        const HIT_CONDITION = 1 << 2;
        // Conditions for exceptions can be set only when exception filters are supported.
        const EXCEPTION_FILTER_OPTIONS = 1 << 3;
    }
}

impl From<&Capabilities> for SupportedBreakpointProperties {
    fn from(caps: &Capabilities) -> Self {
        let mut this = Self::empty();
        for (prop, offset) in [
            (caps.supports_log_points, Self::LOG),
            (caps.supports_conditional_breakpoints, Self::CONDITION),
            (
                caps.supports_hit_conditional_breakpoints,
                Self::HIT_CONDITION,
            ),
            (
                caps.supports_exception_options,
                Self::EXCEPTION_FILTER_OPTIONS,
            ),
        ] {
            if prop.unwrap_or_default() {
                this.insert(offset);
            }
        }
        this
    }
}

impl SupportedBreakpointProperties {
    pub(super) fn for_exception_breakpoints(self) -> Self {
        // TODO: we don't yet support conditions for exception breakpoints at the data layer, hence all props are disabled here.
        Self::empty()
    }
    pub(super) fn for_data_breakpoints(self) -> Self {
        // TODO: we don't yet support conditions for data breakpoints at the data layer, hence all props are disabled here.
        Self::empty()
    }
}
