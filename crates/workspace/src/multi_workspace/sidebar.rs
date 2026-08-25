use super::*;

pub struct SidebarRenderState {
    pub open: bool,
    pub side: SidebarSide,
}

impl Default for SidebarRenderState {
    fn default() -> Self {
        Self {
            open: false,
            side: SidebarSide::default(),
        }
    }
}

pub fn sidebar_side_context_menu(
    id: impl Into<ElementId>,
    cx: &App,
) -> ui::RightClickMenu<ContextMenu> {
    let current_position = SidebarSettings::get_global(cx).side;
    right_click_menu(id).menu(move |window, cx| {
        let fs = <dyn fs::Fs>::global(cx);
        ContextMenu::build(window, cx, move |mut menu, _, _cx| {
            let positions: [(SidebarDockPosition, &str); 2] = [
                (SidebarDockPosition::Left, "Left"),
                (SidebarDockPosition::Right, "Right"),
            ];
            for (position, label) in positions {
                let fs = fs.clone();
                menu = menu.toggleable_entry(
                    label,
                    position == current_position,
                    IconPosition::Start,
                    None,
                    move |_window, cx| {
                        let side = match position {
                            SidebarDockPosition::Left => "left",
                            SidebarDockPosition::Right => "right",
                        };
                        telemetry::event!("Sidebar Side Changed", side = side);
                        settings::update_settings_file(fs.clone(), cx, move |settings, _cx| {
                            settings.sidebar.get_or_insert_default().set_side(position);
                        });
                    },
                );
            }
            menu
        })
    })
}

pub fn render_sidebar_header_controls(
    multi_workspace: Entity<MultiWorkspace>,
    cx: &mut App,
) -> Option<AnyElement> {
    let (enabled, sidebar, active_workspace) =
        multi_workspace.read_with(cx, |multi_workspace, cx| {
            (
                multi_workspace.multi_workspace_enabled(cx),
                multi_workspace.sidebar_render_state(cx),
                multi_workspace.workspace().clone(),
            )
        });

    render_sidebar_header_controls_for_state(
        multi_workspace,
        enabled,
        sidebar,
        Some(active_workspace),
        None,
        cx,
    )
}

pub fn render_sidebar_header_controls_with_state(
    multi_workspace: Entity<MultiWorkspace>,
    sidebar: SidebarRenderState,
    cx: &mut App,
) -> Option<AnyElement> {
    let (enabled, active_workspace) = multi_workspace.read_with(cx, |multi_workspace, cx| {
        (
            multi_workspace.multi_workspace_enabled(cx),
            multi_workspace.workspace().clone(),
        )
    });

    render_sidebar_header_controls_for_state(
        multi_workspace,
        enabled,
        sidebar,
        Some(active_workspace),
        None,
        cx,
    )
}

pub fn render_sidebar_header_controls_with_project_pane_visibility(
    multi_workspace: Entity<MultiWorkspace>,
    sidebar: SidebarRenderState,
    project_pane_visible: bool,
    cx: &mut App,
) -> Option<AnyElement> {
    let enabled = multi_workspace.read_with(cx, |multi_workspace, cx| {
        multi_workspace.multi_workspace_enabled(cx)
    });

    render_sidebar_header_controls_for_state(
        multi_workspace,
        enabled,
        sidebar,
        None,
        Some(project_pane_visible),
        cx,
    )
}

fn render_sidebar_header_controls_for_state(
    multi_workspace: Entity<MultiWorkspace>,
    enabled: bool,
    sidebar: SidebarRenderState,
    active_workspace: Option<Entity<Workspace>>,
    project_pane_visible: Option<bool>,
    cx: &mut App,
) -> Option<AnyElement> {
    if !enabled {
        return None;
    }

    let sidebar_open = sidebar.open;
    let sidebar_side = sidebar.side;
    let sidebar_icon = match (sidebar_open, sidebar_side) {
        (true, SidebarSide::Left) => IconName::SidebarLeftOpen,
        (true, SidebarSide::Right) => IconName::SidebarRightOpen,
        (false, SidebarSide::Left) => IconName::SidebarLeftClosed,
        (false, SidebarSide::Right) => IconName::SidebarRightClosed,
    };
    let sidebar_label = if sidebar_open {
        "Hide Sidebar"
    } else {
        "Open Sidebar"
    };
    let on_right = sidebar_side == SidebarSide::Right;
    let sidebar_multi_workspace = multi_workspace.clone();

    let sidebar_toggle_button = sidebar_side_context_menu("sidebar-toggle-menu", cx)
        .anchor(if on_right {
            gpui::Anchor::BottomRight
        } else {
            gpui::Anchor::BottomLeft
        })
        .attach(if on_right {
            gpui::Anchor::TopRight
        } else {
            gpui::Anchor::TopLeft
        })
        .trigger(move |_is_active, _window, _cx| {
            IconButton::new("sidebar-toggle", sidebar_icon)
                .icon_size(IconSize::Small)
                .icon_color(Color::Muted)
                .tooltip(move |_, cx| Tooltip::for_action(sidebar_label, &ToggleSidebar, cx))
                .on_click(move |_, window, cx| {
                    sidebar_multi_workspace.update(cx, |multi_workspace, cx| {
                        if sidebar_open {
                            multi_workspace.close_sidebar(window, cx);
                        } else {
                            multi_workspace.toggle_sidebar(window, cx);
                        }
                    });
                })
        })
        .into_any_element();

    let project_pane_toggle_button = if SidebarSettings::get_global(cx).show_project_pane_button {
        let is_visible = project_pane_visible.or_else(|| {
            active_workspace
                .as_ref()
                .map(|workspace| workspace.read(cx).panel_pane_visible(PaneKind::Project, cx))
        })?;
        let label = if is_visible {
            "Hide Project Pane"
        } else {
            "Show Project Pane"
        };

        Some(
            IconButton::new("project-pane-toggle", IconName::Compass)
                .icon_size(IconSize::Small)
                .icon_color(if is_visible {
                    Color::Default
                } else {
                    Color::Muted
                })
                .toggle_state(is_visible)
                .tooltip(move |_, cx| Tooltip::for_action(label, &ToggleProjectPane, cx))
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(ToggleProjectPane), cx);
                })
                .into_any_element(),
        )
    } else {
        None
    };

    Some(
        h_flex()
            .h_full()
            .gap_1()
            .child(sidebar_toggle_button)
            .when_some(project_pane_toggle_button, |this, button| {
                this.child(button)
            })
            .into_any_element(),
    )
}

pub enum MultiWorkspaceEvent {
    ActiveWorkspaceChanged {
        source_workspace: Option<WeakEntity<Workspace>>,
    },
    WorkspaceAdded(Entity<Workspace>),
    WorkspaceRemoved(EntityId),
    ProjectGroupsChanged,
}

pub enum SidebarEvent {
    SerializeNeeded,
}

pub trait Sidebar: Focusable + Render + EventEmitter<SidebarEvent> + Sized {
    fn width(&self, cx: &App) -> Pixels;
    fn set_width(&mut self, width: Option<Pixels>, cx: &mut Context<Self>);
    fn has_notifications(&self, cx: &App) -> bool;
    fn side(&self, _cx: &App) -> SidebarSide;

    fn is_threads_list_view_active(&self) -> bool {
        true
    }
    /// Makes focus reset back to the search editor upon toggling the sidebar from outside
    fn prepare_for_focus(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
    /// Opens or cycles the thread switcher popup.
    fn toggle_thread_switcher(
        &mut self,
        _select_last: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// Activates the next or previous project.
    fn cycle_project(&mut self, _forward: bool, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// Activates the next or previous thread in sidebar order.
    fn cycle_thread(&mut self, _forward: bool, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// Toggles the sidebar's primary options menu, if it has one.
    fn toggle_options_menu(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// Simulates update states for chrome testing.
    fn simulate_update_available(&mut self, _cx: &mut Context<Self>) {}

    #[cfg(not(target_os = "macos"))]
    fn open_application_menu(
        &mut self,
        _menu_name: String,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    #[cfg(not(target_os = "macos"))]
    fn activate_application_menu(
        &mut self,
        _right: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// Return an opaque JSON blob of sidebar-specific state to persist.
    fn serialized_state(&self, _cx: &App) -> Option<String> {
        None
    }

    /// Restore sidebar state from a previously-serialized blob.
    fn restore_serialized_state(
        &mut self,
        _state: &str,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}

pub trait SidebarHandle: 'static + Send + Sync {
    fn width(&self, cx: &App) -> Pixels;
    fn set_width(&self, width: Option<Pixels>, cx: &mut App);
    fn focus_handle(&self, cx: &App) -> FocusHandle;
    fn focus(&self, window: &mut Window, cx: &mut App);
    fn prepare_for_focus(&self, window: &mut Window, cx: &mut App);
    fn has_notifications(&self, cx: &App) -> bool;
    fn to_any(&self) -> AnyView;
    fn entity_id(&self) -> EntityId;
    fn toggle_thread_switcher(&self, select_last: bool, window: &mut Window, cx: &mut App);
    fn cycle_project(&self, forward: bool, window: &mut Window, cx: &mut App);
    fn cycle_thread(&self, forward: bool, window: &mut Window, cx: &mut App);
    fn toggle_options_menu(&self, window: &mut Window, cx: &mut App);
    fn simulate_update_available(&self, cx: &mut App);
    #[cfg(not(target_os = "macos"))]
    fn open_application_menu(&self, menu_name: String, window: &mut Window, cx: &mut App);
    #[cfg(not(target_os = "macos"))]
    fn activate_application_menu(&self, right: bool, window: &mut Window, cx: &mut App);

    fn is_threads_list_view_active(&self, cx: &App) -> bool;

    fn side(&self, cx: &App) -> SidebarSide;
    fn serialized_state(&self, cx: &App) -> Option<String>;
    fn restore_serialized_state(&self, state: &str, window: &mut Window, cx: &mut App);
}

#[derive(Clone)]
pub struct DraggedSidebar;

impl Render for DraggedSidebar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

impl<T: Sidebar> SidebarHandle for Entity<T> {
    fn width(&self, cx: &App) -> Pixels {
        self.read(cx).width(cx)
    }

    fn set_width(&self, width: Option<Pixels>, cx: &mut App) {
        self.update(cx, |this, cx| this.set_width(width, cx))
    }

    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.read(cx).focus_handle(cx)
    }

    fn focus(&self, window: &mut Window, cx: &mut App) {
        let handle = self.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    fn prepare_for_focus(&self, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| this.prepare_for_focus(window, cx));
    }

    fn has_notifications(&self, cx: &App) -> bool {
        self.read(cx).has_notifications(cx)
    }

    fn to_any(&self) -> AnyView {
        self.clone().into()
    }

    fn entity_id(&self) -> EntityId {
        Entity::entity_id(self)
    }

    fn toggle_thread_switcher(&self, select_last: bool, window: &mut Window, cx: &mut App) {
        let entity = self.clone();
        window.defer(cx, move |window, cx| {
            entity.update(cx, |this, cx| {
                this.toggle_thread_switcher(select_last, window, cx);
            });
        });
    }

    fn cycle_project(&self, forward: bool, window: &mut Window, cx: &mut App) {
        let entity = self.clone();
        window.defer(cx, move |window, cx| {
            entity.update(cx, |this, cx| {
                this.cycle_project(forward, window, cx);
            });
        });
    }

    fn cycle_thread(&self, forward: bool, window: &mut Window, cx: &mut App) {
        let entity = self.clone();
        window.defer(cx, move |window, cx| {
            entity.update(cx, |this, cx| {
                this.cycle_thread(forward, window, cx);
            });
        });
    }

    fn toggle_options_menu(&self, window: &mut Window, cx: &mut App) {
        let entity = self.clone();
        window.defer(cx, move |window, cx| {
            entity.update(cx, |this, cx| {
                this.toggle_options_menu(window, cx);
            });
        });
    }

    fn simulate_update_available(&self, cx: &mut App) {
        self.update(cx, |this, cx| {
            this.simulate_update_available(cx);
        });
    }

    #[cfg(not(target_os = "macos"))]
    fn open_application_menu(&self, menu_name: String, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| {
            this.open_application_menu(menu_name, window, cx);
        });
    }

    #[cfg(not(target_os = "macos"))]
    fn activate_application_menu(&self, right: bool, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| {
            this.activate_application_menu(right, window, cx);
        });
    }

    fn is_threads_list_view_active(&self, cx: &App) -> bool {
        self.read(cx).is_threads_list_view_active()
    }

    fn side(&self, cx: &App) -> SidebarSide {
        self.read(cx).side(cx)
    }

    fn serialized_state(&self, cx: &App) -> Option<String> {
        self.read(cx).serialized_state(cx)
    }

    fn restore_serialized_state(&self, state: &str, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| {
            this.restore_serialized_state(state, window, cx)
        })
    }
}
