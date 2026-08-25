use super::*;

pub(crate) struct SubView {
    inner: AnyView,
    item_focus_handle: FocusHandle,
    pub(super) kind: DebuggerPaneItem,
    running_state: WeakEntity<RunningState>,
    host_pane: WeakEntity<Pane>,
    show_indicator: Box<dyn Fn(&App) -> bool>,
    pub(super) actions: Option<Box<dyn FnMut(&mut Window, &mut App) -> AnyElement>>,
    pub(super) hovered: bool,
}

impl SubView {
    pub(crate) fn new(
        item_focus_handle: FocusHandle,
        view: AnyView,
        kind: DebuggerPaneItem,
        running_state: WeakEntity<RunningState>,
        host_pane: WeakEntity<Pane>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|_| Self {
            kind,
            inner: view,
            item_focus_handle,
            running_state,
            host_pane,
            show_indicator: Box::new(|_| false),
            actions: None,
            hovered: false,
        })
    }

    pub(crate) fn stack_frame_list(
        stack_frame_list: Entity<StackFrameList>,
        running_state: WeakEntity<RunningState>,
        host_pane: WeakEntity<Pane>,
        cx: &mut App,
    ) -> Entity<Self> {
        let weak_list = stack_frame_list.downgrade();
        let this = Self::new(
            stack_frame_list.focus_handle(cx),
            stack_frame_list.into(),
            DebuggerPaneItem::Frames,
            running_state,
            host_pane,
            cx,
        );

        this.update(cx, |this, _| {
            this.with_actions(Box::new(move |_, cx| {
                weak_list
                    .update(cx, |this, _| this.render_control_strip())
                    .unwrap_or_else(|_| div().into_any_element())
            }));
        });

        this
    }

    pub(crate) fn console(
        console: Entity<Console>,
        running_state: WeakEntity<RunningState>,
        host_pane: WeakEntity<Pane>,
        cx: &mut App,
    ) -> Entity<Self> {
        let weak_console = console.downgrade();
        let this = Self::new(
            console.focus_handle(cx),
            console.into(),
            DebuggerPaneItem::Console,
            running_state,
            host_pane,
            cx,
        );
        this.update(cx, |this, _| {
            this.with_indicator(Box::new(move |cx| {
                weak_console
                    .read_with(cx, |console, cx| console.show_indicator(cx))
                    .unwrap_or_default()
            }))
        });
        this
    }

    pub(crate) fn breakpoint_list(
        list: Entity<BreakpointList>,
        running_state: WeakEntity<RunningState>,
        host_pane: WeakEntity<Pane>,
        cx: &mut App,
    ) -> Entity<Self> {
        let weak_list = list.downgrade();
        let focus_handle = list.focus_handle(cx);
        let this = Self::new(
            focus_handle,
            list.into(),
            DebuggerPaneItem::BreakpointList,
            running_state,
            host_pane,
            cx,
        );

        this.update(cx, |this, _| {
            this.with_actions(Box::new(move |_, cx| {
                weak_list
                    .update(cx, |this, _| this.render_control_strip())
                    .unwrap_or_else(|_| div().into_any_element())
            }));
        });
        this
    }

    pub(crate) fn view_kind(&self) -> DebuggerPaneItem {
        self.kind
    }
    pub(crate) fn with_indicator(&mut self, indicator: Box<dyn Fn(&App) -> bool>) {
        self.show_indicator = indicator;
    }
    pub(crate) fn with_actions(
        &mut self,
        actions: Box<dyn FnMut(&mut Window, &mut App) -> AnyElement>,
    ) {
        self.actions = Some(actions);
    }

    pub(super) fn set_host_pane(&mut self, host_pane: WeakEntity<Pane>) {
        self.host_pane = host_pane;
    }
}
impl Focusable for SubView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.item_focus_handle.clone()
    }
}
impl EventEmitter<()> for SubView {}
impl Item for SubView {
    type Event = ();

    /// This is used to serialize debugger pane layouts
    /// A SharedString gets converted to a enum and back during serialization/deserialization.
    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        self.kind.to_shared_string()
    }

    fn tab_tooltip_text(&self, _: &App) -> Option<SharedString> {
        Some(self.kind.tab_tooltip())
    }

    fn tab_content(
        &self,
        params: workspace::item::TabContentParams,
        _: &Window,
        cx: &App,
    ) -> AnyElement {
        let label = Label::new(self.kind.to_shared_string())
            .size(ui::LabelSize::Small)
            .color(params.text_color())
            .line_height_style(ui::LineHeightStyle::UiLabel);

        if !params.selected && self.show_indicator.as_ref()(cx) {
            return h_flex()
                .justify_between()
                .child(ui::Indicator::dot())
                .gap_2()
                .child(label)
                .into_any_element();
        }

        label.into_any_element()
    }

    fn handle_drop(
        &self,
        active_pane: &Pane,
        dropped: &dyn Any,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let Some(tab) = dropped.downcast_ref::<DraggedTab>() else {
            return true;
        };
        let Some(this_pane) = self.host_pane.upgrade() else {
            return true;
        };
        if tab.item.downcast::<SubView>().is_none() {
            return true;
        }
        let Some(split_direction) = active_pane.drag_split_direction() else {
            return false;
        };

        let source = tab.pane.clone();
        let item_id_to_move = tab.item.item_id();
        let weak_running = self.running_state.clone();

        // Source pane may be the one currently updated, so defer the move.
        window.defer(cx, move |window, cx| {
            let new_pane = weak_running.update(cx, |running, cx| {
                let Some(project) = running.project.upgrade() else {
                    return Err(anyhow!("Debugger project has been dropped"));
                };

                let new_pane = new_debugger_pane(running.workspace.clone(), project, window, cx);
                let _previous_subscription = running.pane_close_subscriptions.insert(
                    new_pane.entity_id(),
                    cx.subscribe_in(&new_pane, window, RunningState::handle_pane_event),
                );
                debug_assert!(_previous_subscription.is_none());
                running
                    .panes
                    .split(&this_pane, &new_pane, split_direction, cx);
                anyhow::Ok(new_pane)
            });

            match new_pane.and_then(|result| result) {
                Ok(new_pane) => {
                    move_item(
                        &source,
                        &new_pane,
                        item_id_to_move,
                        new_pane.read(cx).active_item_index(),
                        true,
                        window,
                        cx,
                    );
                }
                Err(err) => {
                    log::error!("{err:?}");
                }
            }
        });

        true
    }
}

impl Render for SubView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id(format!(
                "subview-container-{}",
                self.kind.to_shared_string()
            ))
            .on_hover(cx.listener(|this, hovered, _, cx| {
                this.hovered = *hovered;
                cx.notify();
            }))
            .size_full()
            // Add border unconditionally to prevent layout shifts on focus changes.
            .border_1()
            .when(self.item_focus_handle.contains_focused(window, cx), |el| {
                el.border_color(cx.theme().colors().pane_focused_border)
            })
            .child(self.inner.clone())
    }
}
