use super::*;

impl Render for ProjectSearchBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(search) = self.active_project_search.clone() else {
            return div().into_any_element();
        };
        let search = search.read(cx);
        let focus_handle = search.focus_handle(cx);

        let container_width = window.viewport_size().width;
        let input_width = SearchInputWidth::calc_width(container_width);

        let input_base_styles = |panel: InputPanel| {
            input_base_styles(search.border_color_for(panel, cx), |div| match panel {
                InputPanel::Query | InputPanel::Replacement => div.w(input_width),
                InputPanel::Include | InputPanel::Exclude => div.flex_grow_1(),
            })
        };
        let theme_colors = cx.theme().colors();
        let project_search = search.entity.read(cx);
        let limit_reached = project_search.search_state.limit_reached();
        let is_search_underway = project_search.pending_search.is_some();

        let color_override = match (
            project_search.search_state,
            &project_search.active_query,
            &project_search.last_search_query_text,
        ) {
            (
                SearchState::Completed(SearchCompletion::NoResults),
                Some(query),
                Some(previous_query),
            ) if query.as_str() == previous_query => Some(Color::Error),
            _ => None,
        };

        let match_text = search
            .active_match_index
            .and_then(|index| {
                let index = index + 1;
                let match_quantity = project_search.match_ranges.len();
                if match_quantity > 0 {
                    debug_assert!(match_quantity >= index);
                    if limit_reached {
                        Some(format!("{index}/{match_quantity}+"))
                    } else {
                        Some(format!("{index}/{match_quantity}"))
                    }
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "0/0".to_string());

        let query_focus = search.query_editor.focus_handle(cx);

        let query_column = input_base_styles(InputPanel::Query)
            .on_action(cx.listener(|this, action, window, cx| this.confirm(action, window, cx)))
            .on_action(cx.listener(|this, action, window, cx| {
                this.previous_history_query(action, window, cx)
            }))
            .on_action(
                cx.listener(|this, action, window, cx| this.next_history_query(action, window, cx)),
            )
            .child(div().flex_1().py_1().child(render_text_input(
                &search.query_editor,
                color_override,
                cx,
            )))
            .child(
                h_flex()
                    .gap_1()
                    .child(SearchOption::CaseSensitive.as_button(
                        search.search_options,
                        SearchSource::Project(cx),
                        focus_handle.clone(),
                    ))
                    .child(SearchOption::WholeWord.as_button(
                        search.search_options,
                        SearchSource::Project(cx),
                        focus_handle.clone(),
                    ))
                    .child(SearchOption::Regex.as_button(
                        search.search_options,
                        SearchSource::Project(cx),
                        focus_handle.clone(),
                    )),
            );

        let matches_column = h_flex()
            .ml_1()
            .pl_1p5()
            .border_l_1()
            .border_color(theme_colors.border_variant)
            .child(render_action_button(
                "project-search-nav-button",
                IconName::ChevronLeft,
                search
                    .active_match_index
                    .is_none()
                    .then_some(ActionButtonState::Disabled),
                "Select Previous Match",
                &SelectPreviousMatch,
                query_focus.clone(),
            ))
            .child(render_action_button(
                "project-search-nav-button",
                IconName::ChevronRight,
                search
                    .active_match_index
                    .is_none()
                    .then_some(ActionButtonState::Disabled),
                "Select Next Match",
                &SelectNextMatch,
                query_focus.clone(),
            ))
            .child(
                div()
                    .id("matches")
                    .ml_2()
                    .min_w(rems_from_px(40.))
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(
                                Label::new(match_text)
                                    .size(LabelSize::Small)
                                    .when(search.active_match_index.is_some(), |this| {
                                        this.color(Color::Disabled)
                                    }),
                            )
                            .when(is_search_underway, |this| {
                                this.child(
                                    Icon::new(IconName::ArrowCircle)
                                        .color(Color::Accent)
                                        .size(IconSize::Small)
                                        .with_rotate_animation(2)
                                        .into_any_element(),
                                )
                            }),
                    )
                    .when(limit_reached, |this| {
                        this.tooltip(Tooltip::text(
                            "Search Limits Reached\nTry narrowing your search",
                        ))
                    }),
            );

        let mode_column = h_flex()
            .gap_1()
            .min_w_64()
            .child(
                IconButton::new("project-search-filter-button", IconName::Filter)
                    .shape(IconButtonShape::Square)
                    .tooltip(|_window, cx| {
                        Tooltip::for_action("Toggle Filters", &ToggleFilters, cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_filters(window, cx);
                    }))
                    .toggle_state(
                        self.active_project_search
                            .as_ref()
                            .map(|search| search.read(cx).filters_enabled)
                            .unwrap_or_default(),
                    )
                    .tooltip({
                        let focus_handle = focus_handle.clone();
                        move |_window, cx| {
                            Tooltip::for_action_in(
                                "Toggle Filters",
                                &ToggleFilters,
                                &focus_handle,
                                cx,
                            )
                        }
                    }),
            )
            .child(render_action_button(
                "project-search",
                IconName::Replace,
                self.active_project_search
                    .as_ref()
                    .map(|search| search.read(cx).replace_enabled)
                    .and_then(|enabled| enabled.then_some(ActionButtonState::Toggled)),
                "Toggle Replace",
                &ToggleReplace,
                focus_handle.clone(),
            ))
            .child(matches_column);

        let is_collapsed = search.results_editor.read(cx).has_any_buffer_folded(cx);

        let (icon, tooltip_label) = if is_collapsed {
            (IconName::ChevronUpDown, "Expand All Search Results")
        } else {
            (IconName::ChevronDownUp, "Collapse All Search Results")
        };

        let expand_button = IconButton::new("project-search-collapse-expand", icon)
            .shape(IconButtonShape::Square)
            .tooltip(move |_, cx| {
                Tooltip::for_action_in(
                    tooltip_label,
                    &ToggleAllSearchResults,
                    &query_focus.clone(),
                    cx,
                )
            })
            .on_click(cx.listener(|this, _, window, cx| {
                if let Some(active_view) = &this.active_project_search {
                    active_view.update(cx, |active_view, cx| {
                        active_view.toggle_all_search_results(&ToggleAllSearchResults, window, cx);
                    })
                }
            }));

        let search_line = h_flex()
            .pl_0p5()
            .w_full()
            .gap_2()
            .child(expand_button)
            .child(query_column)
            .child(mode_column);

        let replace_line = search.replace_enabled.then(|| {
            let replace_column = input_base_styles(InputPanel::Replacement).child(
                div().flex_1().py_1().child(render_text_input(
                    &search.replacement_editor,
                    None,
                    cx,
                )),
            );

            let focus_handle = search.replacement_editor.read(cx).focus_handle(cx);
            let replace_actions = h_flex()
                .min_w_64()
                .gap_1()
                .child(render_action_button(
                    "project-search-replace-button",
                    IconName::ReplaceNext,
                    is_search_underway.then_some(ActionButtonState::Disabled),
                    "Replace Next Match",
                    &ReplaceNext,
                    focus_handle.clone(),
                ))
                .child(render_action_button(
                    "project-search-replace-button",
                    IconName::ReplaceAll,
                    Default::default(),
                    "Replace All Matches",
                    &ReplaceAll,
                    focus_handle,
                ));

            h_flex()
                .w_full()
                .gap_2()
                .child(alignment_element())
                .child(replace_column)
                .child(replace_actions)
        });

        let filter_line = search.filters_enabled.then(|| {
            let include = input_base_styles(InputPanel::Include)
                .on_action(cx.listener(|this, action, window, cx| {
                    this.previous_history_query(action, window, cx)
                }))
                .on_action(cx.listener(|this, action, window, cx| {
                    this.next_history_query(action, window, cx)
                }))
                .child(render_text_input(&search.included_files_editor, None, cx));
            let exclude = input_base_styles(InputPanel::Exclude)
                .on_action(cx.listener(|this, action, window, cx| {
                    this.previous_history_query(action, window, cx)
                }))
                .on_action(cx.listener(|this, action, window, cx| {
                    this.next_history_query(action, window, cx)
                }))
                .child(render_text_input(&search.excluded_files_editor, None, cx));
            let mode_column = h_flex()
                .gap_1()
                .min_w_64()
                .child(
                    IconButton::new("project-search-opened-only", IconName::FolderSearch)
                        .shape(IconButtonShape::Square)
                        .toggle_state(self.is_opened_only_enabled(cx))
                        .tooltip(Tooltip::text("Only Search Open Files"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_opened_only(window, cx);
                        })),
                )
                .child(SearchOption::IncludeIgnored.as_button(
                    search.search_options,
                    SearchSource::Project(cx),
                    focus_handle,
                ));

            h_flex()
                .w_full()
                .gap_2()
                .child(alignment_element())
                .child(
                    h_flex()
                        .w(input_width)
                        .gap_2()
                        .child(include)
                        .child(exclude),
                )
                .child(mode_column)
        });

        let mut key_context = KeyContext::default();
        key_context.add("ProjectSearchBar");
        if search
            .replacement_editor
            .focus_handle(cx)
            .is_focused(window)
        {
            key_context.add("in_replace");
        }

        let query_error_line = search
            .panels_with_errors
            .get(&InputPanel::Query)
            .map(|error| {
                Label::new(error)
                    .size(LabelSize::Small)
                    .color(Color::Error)
                    .mt_neg_1()
                    .ml_2()
            });

        let filter_error_line = search
            .panels_with_errors
            .get(&InputPanel::Include)
            .or_else(|| search.panels_with_errors.get(&InputPanel::Exclude))
            .map(|error| {
                Label::new(error)
                    .size(LabelSize::Small)
                    .color(Color::Error)
                    .mt_neg_1()
                    .ml_2()
            });

        v_flex()
            .gap_2()
            .w_full()
            .key_context(key_context)
            .on_action(cx.listener(|this, _: &ToggleFocus, window, cx| {
                this.move_focus_to_results(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleFilters, window, cx| {
                this.toggle_filters(window, cx);
            }))
            .capture_action(cx.listener(Self::tab))
            .capture_action(cx.listener(Self::backtab))
            .on_action(cx.listener(|this, action, window, cx| this.confirm(action, window, cx)))
            .on_action(cx.listener(|this, action, window, cx| {
                this.toggle_replace(action, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleWholeWord, window, cx| {
                this.toggle_search_option(SearchOptions::WHOLE_WORD, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleCaseSensitive, window, cx| {
                this.toggle_search_option(SearchOptions::CASE_SENSITIVE, window, cx);
            }))
            .on_action(cx.listener(|this, action, window, cx| {
                if let Some(search) = this.active_project_search.as_ref() {
                    search.update(cx, |this, cx| {
                        this.replace_next(action, window, cx);
                    })
                }
            }))
            .on_action(cx.listener(|this, action, window, cx| {
                if let Some(search) = this.active_project_search.as_ref() {
                    search.update(cx, |this, cx| {
                        this.replace_all(action, window, cx);
                    })
                }
            }))
            .when(search.filters_enabled, |this| {
                this.on_action(cx.listener(|this, _: &ToggleIncludeIgnored, window, cx| {
                    this.toggle_search_option(SearchOptions::INCLUDE_IGNORED, window, cx);
                }))
            })
            .on_action(cx.listener(Self::select_next_match))
            .on_action(cx.listener(Self::select_prev_match))
            .on_action(cx.listener(Self::open_text_finder))
            .child(search_line)
            .children(query_error_line)
            .children(replace_line)
            .children(filter_line)
            .children(filter_error_line)
            .into_any_element()
    }
}

impl EventEmitter<ToolbarItemEvent> for ProjectSearchBar {}

impl ToolbarItemView for ProjectSearchBar {
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn ItemHandle>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> ToolbarItemLocation {
        cx.notify();
        self.subscription = None;
        self.active_project_search = None;
        if let Some(search) = active_pane_item.and_then(|i| i.downcast::<ProjectSearchView>()) {
            self.subscription = Some(cx.observe(&search, |_, _, cx| cx.notify()));
            self.active_project_search = Some(search);
            ToolbarItemLocation::PrimaryLeft {}
        } else {
            ToolbarItemLocation::Hidden
        }
    }
}
