use super::*;

impl LineBreakpoint {
    pub(super) fn render(
        &mut self,
        props: SupportedBreakpointProperties,
        strip_mode: Option<ActiveBreakpointStripMode>,
        ix: usize,
        is_selected: bool,
        focus_handle: FocusHandle,
        weak: WeakEntity<BreakpointList>,
    ) -> ListItem {
        let icon_name = if self.breakpoint.state.is_enabled() {
            IconName::DebugBreakpoint
        } else {
            IconName::DebugDisabledBreakpoint
        };
        let path = self.breakpoint.path.clone();
        let row = self.breakpoint.row;
        let is_enabled = self.breakpoint.state.is_enabled();

        let indicator = div()
            .id(SharedString::from(format!(
                "breakpoint-ui-toggle-{:?}/{}:{}",
                self.dir, self.name, self.line
            )))
            .child(
                Icon::new(icon_name)
                    .color(Color::Debugger)
                    .size(IconSize::XSmall),
            )
            .tooltip({
                let focus_handle = focus_handle.clone();
                move |_window, cx| {
                    Tooltip::for_action_in(
                        if is_enabled {
                            "Disable Breakpoint"
                        } else {
                            "Enable Breakpoint"
                        },
                        &ToggleEnableBreakpoint,
                        &focus_handle,
                        cx,
                    )
                }
            })
            .on_click({
                let weak = weak.clone();
                let path = path.clone();
                move |_, _, cx| {
                    weak.update(cx, |breakpoint_list, cx| {
                        breakpoint_list.edit_line_breakpoint(
                            path.clone(),
                            row,
                            BreakpointEditAction::InvertState,
                            cx,
                        );
                    })
                    .ok();
                }
            })
            .on_mouse_down(MouseButton::Left, move |_, _, _| {});

        ListItem::new(SharedString::from(format!(
            "breakpoint-ui-item-{:?}/{}:{}",
            self.dir, self.name, self.line
        )))
        .toggle_state(is_selected)
        .inset(true)
        .on_click({
            let weak = weak.clone();
            move |_, window, cx| {
                weak.update(cx, |breakpoint_list, cx| {
                    breakpoint_list.select_ix(Some(ix), window, cx);
                })
                .ok();
            }
        })
        .on_secondary_mouse_down(|_, _, cx| {
            cx.stop_propagation();
        })
        .start_slot(indicator)
        .child(
            h_flex()
                .id(SharedString::from(format!(
                    "breakpoint-ui-on-click-go-to-line-{:?}/{}:{}",
                    self.dir, self.name, self.line
                )))
                .w_full()
                .gap_1()
                .min_h(rems_from_px(26.))
                .justify_between()
                .on_click({
                    let weak = weak.clone();
                    move |_, window, cx| {
                        weak.update(cx, |breakpoint_list, cx| {
                            breakpoint_list.select_ix(Some(ix), window, cx);
                            breakpoint_list.go_to_line_breakpoint(path.clone(), row, window, cx);
                        })
                        .ok();
                    }
                })
                .child(
                    h_flex()
                        .id("label-container")
                        .gap_0p5()
                        .child(
                            Label::new(format!("{}:{}", self.name, self.line))
                                .size(LabelSize::Small)
                                .line_height_style(ui::LineHeightStyle::UiLabel),
                        )
                        .children(self.dir.as_ref().and_then(|dir| {
                            let path_without_root = Path::new(dir.as_ref())
                                .components()
                                .skip(1)
                                .collect::<PathBuf>();
                            path_without_root.components().next()?;
                            Some(
                                Label::new(path_without_root.to_string_lossy().into_owned())
                                    .color(Color::Muted)
                                    .size(LabelSize::Small)
                                    .line_height_style(ui::LineHeightStyle::UiLabel)
                                    .truncate(),
                            )
                        }))
                        .when_some(self.dir.as_ref(), |this, parent_dir| {
                            this.tooltip(Tooltip::text(format!(
                                "Worktree parent path: {parent_dir}"
                            )))
                        }),
                )
                .child(BreakpointOptionsStrip {
                    props,
                    breakpoint: BreakpointEntry {
                        kind: BreakpointEntryKind::LineBreakpoint(self.clone()),
                        weak,
                    },
                    is_selected,
                    focus_handle,
                    strip_mode,
                    index: ix,
                }),
        )
    }
}

impl DataBreakpoint {
    pub(super) fn render(
        &self,
        props: SupportedBreakpointProperties,
        strip_mode: Option<ActiveBreakpointStripMode>,
        ix: usize,
        is_selected: bool,
        focus_handle: FocusHandle,
        list: WeakEntity<BreakpointList>,
    ) -> ListItem {
        let color = if self.0.is_enabled {
            Color::Debugger
        } else {
            Color::Muted
        };
        let is_enabled = self.0.is_enabled;
        let id = self.0.dap.data_id.clone();

        ListItem::new(SharedString::from(format!(
            "data-breakpoint-ui-item-{}",
            self.0.dap.data_id
        )))
        .toggle_state(is_selected)
        .inset(true)
        .start_slot(
            div()
                .id(SharedString::from(format!(
                    "data-breakpoint-ui-item-{}-click-handler",
                    self.0.dap.data_id
                )))
                .child(
                    Icon::new(IconName::Binary)
                        .color(color)
                        .size(IconSize::Small),
                )
                .tooltip({
                    let focus_handle = focus_handle.clone();
                    move |_window, cx| {
                        Tooltip::for_action_in(
                            if is_enabled {
                                "Disable Data Breakpoint"
                            } else {
                                "Enable Data Breakpoint"
                            },
                            &ToggleEnableBreakpoint,
                            &focus_handle,
                            cx,
                        )
                    }
                })
                .on_click({
                    let list = list.clone();
                    move |_, _, cx| {
                        list.update(cx, |this, cx| {
                            this.toggle_data_breakpoint(&id, cx);
                        })
                        .ok();
                    }
                }),
        )
        .child(
            h_flex()
                .w_full()
                .gap_1()
                .min_h(rems_from_px(26.))
                .justify_between()
                .child(
                    v_flex()
                        .py_1()
                        .gap_1()
                        .justify_center()
                        .id(("data-breakpoint-label", ix))
                        .child(
                            Label::new(self.0.context.human_readable_label())
                                .size(LabelSize::Small)
                                .line_height_style(ui::LineHeightStyle::UiLabel),
                        ),
                )
                .child(BreakpointOptionsStrip {
                    props,
                    breakpoint: BreakpointEntry {
                        kind: BreakpointEntryKind::DataBreakpoint(self.clone()),
                        weak: list,
                    },
                    is_selected,
                    focus_handle,
                    strip_mode,
                    index: ix,
                }),
        )
    }
}

impl ExceptionBreakpoint {
    pub(super) fn render(
        &mut self,
        props: SupportedBreakpointProperties,
        strip_mode: Option<ActiveBreakpointStripMode>,
        ix: usize,
        is_selected: bool,
        focus_handle: FocusHandle,
        list: WeakEntity<BreakpointList>,
    ) -> ListItem {
        let color = if self.is_enabled {
            Color::Debugger
        } else {
            Color::Muted
        };
        let id = SharedString::from(&self.id);
        let is_enabled = self.is_enabled;
        let weak = list.clone();

        ListItem::new(SharedString::from(format!(
            "exception-breakpoint-ui-item-{}",
            self.id
        )))
        .toggle_state(is_selected)
        .inset(true)
        .on_click({
            let list = list.clone();
            move |_, window, cx| {
                list.update(cx, |list, cx| list.select_ix(Some(ix), window, cx))
                    .ok();
            }
        })
        .on_secondary_mouse_down(|_, _, cx| {
            cx.stop_propagation();
        })
        .start_slot(
            div()
                .id(SharedString::from(format!(
                    "exception-breakpoint-ui-item-{}-click-handler",
                    self.id
                )))
                .child(
                    Icon::new(IconName::Flame)
                        .color(color)
                        .size(IconSize::Small),
                )
                .tooltip({
                    let focus_handle = focus_handle.clone();
                    move |_window, cx| {
                        Tooltip::for_action_in(
                            if is_enabled {
                                "Disable Exception Breakpoint"
                            } else {
                                "Enable Exception Breakpoint"
                            },
                            &ToggleEnableBreakpoint,
                            &focus_handle,
                            cx,
                        )
                    }
                })
                .on_click({
                    move |_, _, cx| {
                        list.update(cx, |this, cx| {
                            this.toggle_exception_breakpoint(&id, cx);
                        })
                        .ok();
                    }
                }),
        )
        .child(
            h_flex()
                .w_full()
                .gap_1()
                .min_h(rems_from_px(26.))
                .justify_between()
                .child(
                    v_flex()
                        .py_1()
                        .gap_1()
                        .justify_center()
                        .id(("exception-breakpoint-label", ix))
                        .child(
                            Label::new(self.data.label.clone())
                                .size(LabelSize::Small)
                                .line_height_style(ui::LineHeightStyle::UiLabel),
                        )
                        .when_some(self.data.description.clone(), |el, description| {
                            el.tooltip(Tooltip::text(description))
                        }),
                )
                .child(BreakpointOptionsStrip {
                    props,
                    breakpoint: BreakpointEntry {
                        kind: BreakpointEntryKind::ExceptionBreakpoint(self.clone()),
                        weak,
                    },
                    is_selected,
                    focus_handle,
                    strip_mode,
                    index: ix,
                }),
        )
    }
}
