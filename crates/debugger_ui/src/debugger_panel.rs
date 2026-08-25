use crate::persistence::DebuggerPaneItem;
use crate::session::DebugSession;
use crate::session::running::RunningState;
use crate::session::running::breakpoint_list::BreakpointList;

use crate::{
    ClearAllBreakpoints, Continue, CopyDebugAdapterArguments, Detach, FocusBreakpointList,
    FocusConsole, FocusFrames, FocusLoadedSources, FocusModules, FocusTerminal, FocusVariables,
    NewProcessModal, NewProcessMode, Pause, RerunSession, StepInto, StepOut, StepOver, Stop,
    ToggleExpandItem, ToggleSessionPicker, ToggleThreadPicker, persistence, spawn_task_or_modal,
};
use anyhow::{Context as _, Result, anyhow};
use collections::IndexMap;
use dap::adapters::DebugAdapterName;
use dap::{DapRegistry, StartDebuggingRequestArguments};
use dap::{client::SessionId, debugger_settings::DebuggerSettings};
use editor::{Editor, MultiBufferOffset, ToPoint};
use feature_flags::{FeatureFlag, FeatureFlagAppExt as _, PresenceFlag, register_feature_flag};
use gpui::{
    Action, Anchor, App, AsyncWindowContext, ClipboardItem, Context, DismissEvent, Entity,
    EntityId, EventEmitter, FocusHandle, Focusable, MouseButton, MouseDownEvent, Point,
    Subscription, Task, TaskExt, WeakEntity, anchored, deferred,
};

use itertools::Itertools as _;
use language::Buffer;
use mav_actions::debug_panel::ToggleFocus;
use project::debugger::session::{Session, SessionQuirks, SessionState, SessionStateEvent};
use project::{DebugScenarioContext, Fs, ProjectPath, TaskSourceKind, WorktreeId};
use project::{Project, debugger::session::ThreadStatus};
use rpc::proto::{self};
use settings::Settings;
use std::sync::{Arc, LazyLock};
use task::{DebugScenario, SharedTaskContext};
use tree_sitter::{Query, StreamingIterator as _};
use ui::{
    ContextMenu, Divider, PopoverMenu, PopoverMenuHandle, SplitButton, Tab, Tooltip, prelude::*,
};
use util::redact::redact_command;
use util::rel_path::RelPath;
use util::{ResultExt, debug_panic, maybe};
use workspace::SplitDirection;
use workspace::item::{ItemEvent, SaveOptions};
use workspace::{
    Item, Pane, Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

pub struct DebuggerHistoryFeatureFlag;

impl FeatureFlag for DebuggerHistoryFeatureFlag {
    const NAME: &'static str = "debugger-history";
    type Value = PresenceFlag;
}
register_feature_flag!(DebuggerHistoryFeatureFlag);

const DEBUG_PANEL_KEY: &str = "DebugPanel";

pub struct DebugPanel {
    active_session: Option<Entity<DebugSession>>,
    project: Entity<Project>,
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    context_menu: Option<(Entity<ContextMenu>, Point<Pixels>, Subscription)>,
    debug_scenario_scheduled_last: bool,
    pub(crate) sessions_with_children:
        IndexMap<Entity<DebugSession>, Vec<WeakEntity<DebugSession>>>,
    pub(crate) thread_picker_menu_handle: PopoverMenuHandle<ContextMenu>,
    pub(crate) session_picker_menu_handle: PopoverMenuHandle<ContextMenu>,
    fs: Arc<dyn Fs>,
    is_zoomed: bool,
    _subscriptions: [Subscription; 1],
    breakpoint_list: Entity<BreakpointList>,
}

mod constructor;
mod context_menu;
mod controls;
mod navigation;
mod panel_traits;
mod pickers;
mod provider;
mod render;
mod scenarios;
mod session_lifecycle;
mod workspace_setup;

pub(super) use provider::DebuggerProvider;
