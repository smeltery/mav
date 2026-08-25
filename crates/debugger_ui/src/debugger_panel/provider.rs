use super::*;

pub(super) struct DebuggerProvider(Entity<DebugPanel>);

impl workspace::DebuggerProvider for DebuggerProvider {
    fn start_session(
        &self,
        definition: DebugScenario,
        context: SharedTaskContext,
        buffer: Option<Entity<Buffer>>,
        worktree_id: Option<WorktreeId>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.0.update(cx, |_, cx| {
            cx.defer_in(window, move |this, window, cx| {
                this.start_session(definition, context, buffer, worktree_id, window, cx);
            })
        })
    }

    fn spawn_task_or_modal(
        &self,
        workspace: &mut Workspace,
        action: &tasks_ui::Spawn,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        spawn_task_or_modal(workspace, action, window, cx);
    }

    fn debug_scenario_scheduled(&self, cx: &mut App) {
        self.0.update(cx, |this, _| {
            this.debug_scenario_scheduled_last = true;
        });
    }

    fn task_scheduled(&self, cx: &mut App) {
        self.0.update(cx, |this, _| {
            this.debug_scenario_scheduled_last = false;
        })
    }

    fn debug_scenario_scheduled_last(&self, cx: &App) -> bool {
        self.0.read(cx).debug_scenario_scheduled_last
    }

    fn active_thread_state(&self, cx: &App) -> Option<ThreadStatus> {
        let session = self.0.read(cx).active_session()?;
        let thread = session.read(cx).running_state().read(cx).thread_id()?;
        session.read(cx).session(cx).read(cx).thread_state(thread)
    }
}
