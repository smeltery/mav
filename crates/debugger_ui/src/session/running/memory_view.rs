use std::{
    cell::{LazyCell, RefCell, RefMut},
    fmt::Write,
    ops::RangeInclusive,
    rc::Rc,
    sync::{Arc, LazyLock},
    time::Duration,
};

use editor::{Editor, EditorElement, EditorStyle};
use gpui::{
    Action, Along, AppContext, Axis, DismissEvent, DragMoveEvent, Empty, Entity, FocusHandle,
    Focusable, ListHorizontalSizingBehavior, MouseButton, Point, ScrollStrategy, ScrollWheelEvent,
    Subscription, Task, TextStyle, UniformList, UniformListScrollHandle, WeakEntity, actions,
    anchored, deferred, uniform_list,
};
use notifications::status_toast::StatusToast;
use project::debugger::{MemoryCell, dap_command::DataBreakpointContext, session::Session};
use settings::Settings;
use theme_settings::ThemeSettings;
use ui::{
    ContextMenu, Divider, DropdownMenu, FluentBuilder, IntoElement, PopoverMenuHandle, Render,
    ScrollableHandle, StatefulInteractiveElement, Tooltip, WithScrollbar, prelude::*,
};
use workspace::Workspace;

use crate::{ToggleDataBreakpoint, session::running::stack_frame_list::StackFrameList};

actions!(debugger, [GoToSelectedAddress]);

mod actions;
mod constructor;
mod render;
mod root_render;
mod state;

pub(super) use state::{Drag, SelectedMemoryRange, ViewState, ViewStateHandle};
pub(super) use state::{HEX_BYTES_MEMOIMAV, UNKNOWN_BYTE};

pub(crate) struct MemoryView {
    workspace: WeakEntity<Workspace>,
    stack_frame_list: WeakEntity<StackFrameList>,
    focus_handle: FocusHandle,
    view_state_handle: ViewStateHandle,
    query_editor: Entity<Editor>,
    session: Entity<Session>,
    width_picker_handle: PopoverMenuHandle<ContextMenu>,
    is_writing_memory: bool,
    open_context_menu: Option<(Entity<ContextMenu>, Point<Pixels>, Subscription)>,
}

impl Focusable for MemoryView {
    fn focus_handle(&self, _: &ui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

pub(super) struct ViewWidth {
    width: u8,
    label: SharedString,
}

impl ViewWidth {
    const fn new(width: u8, label: &'static str) -> Self {
        Self {
            width,
            label: SharedString::new_static(label),
        }
    }
}

static WIDTHS: [ViewWidth; 7] = [
    ViewWidth::new(1, "1 byte"),
    ViewWidth::new(2, "2 bytes"),
    ViewWidth::new(4, "4 bytes"),
    ViewWidth::new(8, "8 bytes"),
    ViewWidth::new(16, "16 bytes"),
    ViewWidth::new(32, "32 bytes"),
    ViewWidth::new(64, "64 bytes"),
];
