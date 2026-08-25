use super::*;

impl Render for BufferSearchBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle(cx);

        let split_buttons = self.render_split_buttons(window, cx);
        let has_splittable_editor = self.splittable_editor.is_some();

        let collapse_expand_button = if self.needs_expand_collapse_option(cx) {
            let query_editor_focus = self.query_editor.focus_handle(cx);

            let is_collapsed = self
                .active_searchable_item
                .as_ref()
                .and_then(|item| item.act_as_type(TypeId::of::<Editor>(), cx))
                .and_then(|item| item.downcast::<Editor>().ok())
                .map(|editor: Entity<Editor>| editor.read(cx).has_any_buffer_folded(cx))
                .unwrap_or_default();
            let (icon, tooltip_label) = if is_collapsed {
                (IconName::ChevronUpDown, "Expand All Files")
            } else {
                (IconName::ChevronDownUp, "Collapse All Files")
            };

            let collapse_expand_icon_button = |id| {
                IconButton::new(id, icon)
                    .icon_size(IconSize::Small)
                    .tooltip(move |_, cx| {
                        Tooltip::for_action_in(
                            tooltip_label,
                            &ToggleFoldAll,
                            &query_editor_focus,
                            cx,
                        )
                    })
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.toggle_fold_all(&ToggleFoldAll, window, cx);
                    }))
            };

            if self.dismissed {
                return h_flex()
                    .pl_0p5()
                    .gap_1()
                    .child(collapse_expand_icon_button(
                        "multibuffer-collapse-expand-empty",
                    ))
                    .when(has_splittable_editor, |this| this.children(split_buttons))
                    .into_any_element();
            }

            Some(
                h_flex()
                    .gap_1()
                    .child(collapse_expand_icon_button("multibuffer-collapse-expand"))
                    .children(split_buttons)
                    .into_any_element(),
            )
        } else {
            None
        };

        let narrow_mode =
            self.scroll_handle.bounds().size.width / window.rem_size() < 340. / BASE_REM_SIZE_IN_PX;

        let workspace::searchable::SearchOptions {
            case,
            word,
            regex,
            replacement,
            selection,
            select_all,
            find_in_results,
        } = self.supported_options(cx);

        self.query_editor.update(cx, |query_editor, cx| {
            if query_editor.placeholder_text(cx).is_none() {
                query_editor.set_placeholder_text("Search…", window, cx);
            }
        });

        self.replacement_editor.update(cx, |editor, cx| {
            editor.set_placeholder_text("Replace with…", window, cx);
        });

        let mut color_override = None;
        let match_text = self
            .active_searchable_item
            .as_ref()
            .and_then(|searchable_item| {
                if self.query(cx).is_empty() {
                    return None;
                }
                let matches_count = self
                    .searchable_items_with_matches
                    .get(&searchable_item.downgrade())
                    .map(|(matches, _)| matches.len())
                    .unwrap_or(0);
                if let Some(match_ix) = self.active_match_index {
                    Some(format!("{}/{}", match_ix + 1, matches_count))
                } else {
                    color_override = Some(Color::Error); // No matches found
                    None
                }
            })
            .unwrap_or_else(|| "0/0".to_string());
        let should_show_replace_input = self.replace_enabled && replacement;
        let in_replace = self.replacement_editor.focus_handle(cx).is_focused(window);

        let theme_colors = cx.theme().colors();
        let query_border = if self.query_error.is_some() {
            Color::Error.color(cx)
        } else {
            theme_colors.border
        };
        let replacement_border = theme_colors.border;

        let container_width = window.viewport_size().width;
        let input_width = SearchInputWidth::calc_width(container_width);

        let input_base_styles =
            |border_color| input_base_styles(border_color, |div| div.w(input_width));

        let input_style = if find_in_results {
            filter_search_results_input(query_border, |div| div.w(input_width), cx)
        } else {
            input_base_styles(query_border)
        };

        let query_column = input_style
            .child(div().flex_1().min_w_0().py_1().child(render_text_input(
                &self.query_editor,
                color_override,
                cx,
            )))
            .child(
                h_flex()
                    .flex_none()
                    .gap_1()
                    .when(case, |div| {
                        div.child(SearchOption::CaseSensitive.as_button(
                            self.search_options,
                            SearchSource::Buffer,
                            focus_handle.clone(),
                        ))
                    })
                    .when(word, |div| {
                        div.child(SearchOption::WholeWord.as_button(
                            self.search_options,
                            SearchSource::Buffer,
                            focus_handle.clone(),
                        ))
                    })
                    .when(regex, |div| {
                        div.child(SearchOption::Regex.as_button(
                            self.search_options,
                            SearchSource::Buffer,
                            focus_handle.clone(),
                        ))
                    }),
            );

        let mode_column = h_flex()
            .gap_1()
            .min_w_64()
            .when(replacement, |this| {
                this.child(render_action_button(
                    "buffer-search-bar-toggle",
                    IconName::Replace,
                    self.replace_enabled.then_some(ActionButtonState::Toggled),
                    "Toggle Replace",
                    &ToggleReplace,
                    focus_handle.clone(),
                ))
            })
            .when(selection, |this| {
                this.child(
                    IconButton::new(
                        "buffer-search-bar-toggle-search-selection-button",
                        IconName::Quote,
                    )
                    .style(ButtonStyle::Subtle)
                    .shape(IconButtonShape::Square)
                    .when(self.selection_search_enabled.is_some(), |button| {
                        button.style(ButtonStyle::Filled)
                    })
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.toggle_selection(&ToggleSelection, window, cx);
                    }))
                    .toggle_state(self.selection_search_enabled.is_some())
                    .tooltip({
                        let focus_handle = focus_handle.clone();
                        move |_window, cx| {
                            Tooltip::for_action_in(
                                "Toggle Search Selection",
                                &ToggleSelection,
                                &focus_handle,
                                cx,
                            )
                        }
                    }),
                )
            })
            .when(!find_in_results, |el| {
                let query_focus = self.query_editor.focus_handle(cx);
                let matches_column = h_flex()
                    .pl_2()
                    .ml_2()
                    .border_l_1()
                    .border_color(theme_colors.border_variant)
                    .child(render_action_button(
                        "buffer-search-nav-button",
                        ui::IconName::ChevronLeft,
                        self.active_match_index
                            .is_none()
                            .then_some(ActionButtonState::Disabled),
                        "Select Previous Match",
                        &SelectPreviousMatch,
                        query_focus.clone(),
                    ))
                    .child(render_action_button(
                        "buffer-search-nav-button",
                        ui::IconName::ChevronRight,
                        self.active_match_index
                            .is_none()
                            .then_some(ActionButtonState::Disabled),
                        "Select Next Match",
                        &SelectNextMatch,
                        query_focus.clone(),
                    ))
                    .when(!narrow_mode, |this| {
                        this.child(div().ml_2().min_w(rems_from_px(40.)).child(
                            Label::new(match_text).size(LabelSize::Small).color(
                                if self.active_match_index.is_some() {
                                    Color::Default
                                } else {
                                    Color::Disabled
                                },
                            ),
                        ))
                    });

                el.when(select_all, |el| {
                    el.child(render_action_button(
                        "buffer-search-nav-button",
                        IconName::SelectAll,
                        Default::default(),
                        "Select All Matches",
                        &SelectAllMatches,
                        query_focus.clone(),
                    ))
                })
                .child(matches_column)
            })
            .when(find_in_results, |el| {
                el.child(render_action_button(
                    "buffer-search",
                    IconName::Close,
                    Default::default(),
                    "Close Search Bar",
                    &Dismiss,
                    focus_handle.clone(),
                ))
            });

        let has_collapse_button = collapse_expand_button.is_some();

        let search_line = h_flex()
            .w_full()
            .gap_2()
            .when(find_in_results, |el| el.child(alignment_element()))
            .when(!find_in_results && has_collapse_button, |el| {
                el.pl_0p5().child(collapse_expand_button.expect("button"))
            })
            .child(query_column)
            .child(mode_column);

        let replace_line = should_show_replace_input.then(|| {
            let replace_column = input_base_styles(replacement_border).child(
                div()
                    .flex_1()
                    .py_1()
                    .child(render_text_input(&self.replacement_editor, None, cx)),
            );
            let focus_handle = self.replacement_editor.read(cx).focus_handle(cx);

            let replace_actions = h_flex()
                .min_w_64()
                .gap_1()
                .child(render_action_button(
                    "buffer-search-replace-button",
                    IconName::ReplaceNext,
                    Default::default(),
                    "Replace Next Match",
                    &ReplaceNext,
                    focus_handle.clone(),
                ))
                .child(render_action_button(
                    "buffer-search-replace-button",
                    IconName::ReplaceAll,
                    Default::default(),
                    "Replace All Matches",
                    &ReplaceAll,
                    focus_handle,
                ));

            h_flex()
                .w_full()
                .gap_2()
                .when(has_collapse_button, |this| this.child(alignment_element()))
                .child(replace_column)
                .child(replace_actions)
        });

        let mut key_context = KeyContext::new_with_defaults();
        key_context.add("BufferSearchBar");
        if in_replace {
            key_context.add("in_replace");
        }

        let query_error_line = self.query_error.as_ref().map(|error| {
            Label::new(error)
                .size(LabelSize::Small)
                .color(Color::Error)
                .mt_neg_1()
                .ml_2()
        });

        let search_line =
            h_flex()
                .relative()
                .child(search_line)
                .when(!narrow_mode && !find_in_results, |this| {
                    this.child(
                        h_flex()
                            .absolute()
                            .right_0()
                            .when(has_collapse_button, |this| {
                                this.pr_2()
                                    .border_r_1()
                                    .border_color(cx.theme().colors().border_variant)
                            })
                            .child(render_action_button(
                                "buffer-search",
                                IconName::Close,
                                Default::default(),
                                "Close Search Bar",
                                &Dismiss,
                                focus_handle.clone(),
                            )),
                    )
                });

        v_flex()
            .id("buffer_search")
            .gap_2()
            .w_full()
            .track_scroll(&self.scroll_handle)
            .key_context(key_context)
            .capture_action(cx.listener(Self::tab))
            .capture_action(cx.listener(Self::backtab))
            .capture_action(cx.listener(Self::toggle_fold_all))
            .on_action(cx.listener(Self::previous_history_query))
            .on_action(cx.listener(Self::next_history_query))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::select_next_match))
            .on_action(cx.listener(Self::select_prev_match))
            .on_action(cx.listener(|this, _: &ToggleOutline, window, cx| {
                if let Some(active_searchable_item) = &mut this.active_searchable_item {
                    active_searchable_item.relay_action(Box::new(ToggleOutline), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CopyPath, window, cx| {
                if let Some(active_searchable_item) = &mut this.active_searchable_item {
                    active_searchable_item.relay_action(Box::new(CopyPath), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CopyRelativePath, window, cx| {
                if let Some(active_searchable_item) = &mut this.active_searchable_item {
                    active_searchable_item.relay_action(Box::new(CopyRelativePath), window, cx);
                }
            }))
            .when(replacement, |this| {
                this.on_action(cx.listener(Self::toggle_replace))
                    .on_action(cx.listener(Self::replace_next))
                    .on_action(cx.listener(Self::replace_all))
            })
            .when(case, |this| {
                this.on_action(cx.listener(Self::toggle_case_sensitive))
            })
            .when(word, |this| {
                this.on_action(cx.listener(Self::toggle_whole_word))
            })
            .when(regex, |this| {
                this.on_action(cx.listener(Self::toggle_regex))
            })
            .when(selection, |this| {
                this.on_action(cx.listener(Self::toggle_selection))
            })
            .child(search_line)
            .children(query_error_line)
            .children(replace_line)
            .into_any_element()
    }
}
