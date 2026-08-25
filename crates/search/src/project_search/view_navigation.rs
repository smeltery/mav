use super::*;

impl ProjectSearchView {
    pub(super) fn select_match(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.active_match_index {
            let match_ranges = self.entity.read(cx).match_ranges.clone();

            if !EditorSettings::get_global(cx).search_wrap
                && ((direction == Direction::Next && index + 1 >= match_ranges.len())
                    || (direction == Direction::Prev && index == 0))
            {
                crate::show_no_more_matches(window, cx);
                return;
            }

            let new_index = self.results_editor.update(cx, |editor, cx| {
                editor.match_index_for_direction(
                    &match_ranges,
                    index,
                    direction,
                    1,
                    SearchToken::default(),
                    window,
                    cx,
                )
            });

            let range_to_select = match_ranges[new_index].clone();
            self.results_editor.update(cx, |editor, cx| {
                let range_to_select = editor.range_for_match(&range_to_select);
                let autoscroll = if EditorSettings::get_global(cx).search.center_on_match {
                    Autoscroll::center()
                } else {
                    Autoscroll::fit()
                };
                editor.unfold_ranges(std::slice::from_ref(&range_to_select), false, true, cx);
                editor.change_selections(SelectionEffects::scroll(autoscroll), window, cx, |s| {
                    s.select_ranges([range_to_select])
                });
            });
            self.highlight_matches(&match_ranges, Some(new_index), cx);
        }
    }

    pub(super) fn focus_query_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query_editor.update(cx, |query_editor, cx| {
            query_editor.select_all(&SelectAll, window, cx);
        });
        let editor_handle = self.query_editor.focus_handle(cx);
        window.focus(&editor_handle, cx);
    }

    /// Apply some state (from the textfinder) to the project search UI
    pub(crate) fn adopt_text_finder_state(
        &mut self,
        search_options: SearchOptions,
        active_query: Option<SearchQuery>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.search_options = search_options;
        self.adjust_query_regex_language(cx);
        if let Some(query) = active_query {
            let query_text = query.as_str().to_string();
            self.entity.update(cx, |search, _| {
                search.active_query = Some(query.clone());
                search.last_search_query_text = Some(query_text.clone());
                // Force `entity_changed` to treat this as a new search so the
                // first match gets selected and scrolled into view. The text
                // finder ran its searches via `project.search` directly, so the
                // entity's `search_id` was never advanced.
                search.search_id += 1;
            });
            self.set_search_editor(SearchInputKind::Query, &query_text, window, cx);
            self.focus_results_editor(window, cx);
        } else {
            self.focus_query_editor(window, cx);
        }
        self.entity_changed(window, cx);
    }

    pub(super) fn set_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.set_search_editor(SearchInputKind::Query, query, window, cx);
        if EditorSettings::get_global(cx).use_smartcase_search
            && !query.is_empty()
            && self.search_options.contains(SearchOptions::CASE_SENSITIVE)
                != contains_uppercase(query)
        {
            self.toggle_search_option(SearchOptions::CASE_SENSITIVE, cx)
        }
    }

    pub(super) fn set_search_editor(
        &mut self,
        kind: SearchInputKind,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = match kind {
            SearchInputKind::Query => &self.query_editor,
            SearchInputKind::Include => &self.included_files_editor,

            SearchInputKind::Exclude => &self.excluded_files_editor,
        };
        editor.update(cx, |editor, cx| {
            editor.set_text(text, window, cx);
            editor.request_autoscroll(Autoscroll::fit(), cx);
        });
    }

    pub(super) fn focus_results_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query_editor.update(cx, |query_editor, cx| {
            let cursor = query_editor.selections.newest_anchor().head();
            query_editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                s.select_ranges([cursor..cursor])
            });
        });
        let results_handle = self.results_editor.focus_handle(cx);
        window.focus(&results_handle, cx);
    }

    pub(super) fn entity_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let match_ranges = self.entity.read(cx).match_ranges.clone();

        if match_ranges.is_empty() {
            self.active_match_index = None;
            self.results_editor.update(cx, |editor, cx| {
                editor.clear_background_highlights(HighlightKey::ProjectSearchView, cx);
            });
        } else {
            self.active_match_index = Some(0);
            self.update_match_index(cx);
            let prev_search_id = mem::replace(&mut self.search_id, self.entity.read(cx).search_id);
            let is_new_search = self.search_id != prev_search_id;
            self.results_editor.update(cx, |editor, cx| {
                if is_new_search {
                    let range_to_select = match_ranges
                        .first()
                        .map(|range| editor.range_for_match(range));
                    editor.change_selections(Default::default(), window, cx, |s| {
                        s.select_ranges(range_to_select)
                    });
                    editor.scroll(Point::default(), Some(Axis::Vertical), window, cx);
                }
            });
            if is_new_search && self.query_editor.focus_handle(cx).is_focused(window) {
                self.focus_results_editor(window, cx);
            }
        }

        cx.emit(ViewEvent::UpdateTab);
        cx.notify();

        if self.pending_replace_all && self.entity.read(cx).pending_search.is_none() {
            self.replace_all(&ReplaceAll, window, cx);
        }
    }

    pub(super) fn update_match_index(&mut self, cx: &mut Context<Self>) {
        let results_editor = self.results_editor.read(cx);
        let newest_anchor = results_editor.selections.newest_anchor().head();
        let buffer_snapshot = results_editor.buffer().read(cx).snapshot(cx);
        let new_index = self.entity.update(cx, |this, cx| {
            let new_index = active_match_index(
                Direction::Next,
                &this.match_ranges,
                &newest_anchor,
                &buffer_snapshot,
            );

            self.highlight_matches(&this.match_ranges, new_index, cx);
            new_index
        });

        if self.active_match_index != new_index {
            self.active_match_index = new_index;
            cx.notify();
        }
    }

    #[ztracing::instrument(skip_all)]
    pub(super) fn highlight_matches(
        &self,
        match_ranges: &[Range<Anchor>],
        active_index: Option<usize>,
        cx: &mut App,
    ) {
        self.results_editor.update(cx, |editor, cx| {
            editor.highlight_background(
                HighlightKey::ProjectSearchView,
                match_ranges,
                move |index, theme| {
                    if active_index == Some(*index) {
                        theme.colors().search_active_match_background
                    } else {
                        theme.colors().search_match_background
                    }
                },
                cx,
            );
        });
    }

    pub fn has_matches(&self) -> bool {
        self.active_match_index.is_some()
    }

    pub(super) fn landing_text_minor(&self, cx: &App) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();
        v_flex()
            .gap_1()
            .child(
                Label::new("Hit enter to search. For more options:")
                    .color(Color::Muted)
                    .mb_2(),
            )
            .child(
                Button::new("filter-paths", "Include/exclude specific paths")
                    .start_icon(Icon::new(IconName::Filter).size(IconSize::Small))
                    .key_binding(KeyBinding::for_action_in(&ToggleFilters, &focus_handle, cx))
                    .on_click(|_event, window, cx| {
                        window.dispatch_action(ToggleFilters.boxed_clone(), cx)
                    }),
            )
            .child(
                Button::new("find-replace", "Find and replace")
                    .start_icon(Icon::new(IconName::Replace).size(IconSize::Small))
                    .key_binding(KeyBinding::for_action_in(&ToggleReplace, &focus_handle, cx))
                    .on_click(|_event, window, cx| {
                        window.dispatch_action(ToggleReplace.boxed_clone(), cx)
                    }),
            )
            .child(
                Button::new("regex", "Match with regex")
                    .start_icon(Icon::new(IconName::Regex).size(IconSize::Small))
                    .key_binding(KeyBinding::for_action_in(&ToggleRegex, &focus_handle, cx))
                    .on_click(|_event, window, cx| {
                        window.dispatch_action(ToggleRegex.boxed_clone(), cx)
                    }),
            )
            .child(
                Button::new("match-case", "Match case")
                    .start_icon(Icon::new(IconName::CaseSensitive).size(IconSize::Small))
                    .key_binding(KeyBinding::for_action_in(
                        &ToggleCaseSensitive,
                        &focus_handle,
                        cx,
                    ))
                    .on_click(|_event, window, cx| {
                        window.dispatch_action(ToggleCaseSensitive.boxed_clone(), cx)
                    }),
            )
            .child(
                Button::new("match-whole-words", "Match whole words")
                    .start_icon(Icon::new(IconName::WholeWord).size(IconSize::Small))
                    .key_binding(KeyBinding::for_action_in(
                        &ToggleWholeWord,
                        &focus_handle,
                        cx,
                    ))
                    .on_click(|_event, window, cx| {
                        window.dispatch_action(ToggleWholeWord.boxed_clone(), cx)
                    }),
            )
    }

    pub(super) fn border_color_for(&self, panel: InputPanel, cx: &App) -> Hsla {
        if self.panels_with_errors.contains_key(&panel) {
            Color::Error.color(cx)
        } else {
            cx.theme().colors().border
        }
    }

    pub(super) fn move_focus_to_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.results_editor.focus_handle(cx).is_focused(window)
            && !self.entity.read(cx).match_ranges.is_empty()
        {
            cx.stop_propagation();
            self.focus_results_editor(window, cx)
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn results_editor(&self) -> &Entity<Editor> {
        &self.results_editor
    }

    pub(super) fn adjust_query_regex_language(&self, cx: &mut App) {
        let enable = self.search_options.contains(SearchOptions::REGEX);
        let query_buffer = self
            .query_editor
            .read(cx)
            .buffer()
            .read(cx)
            .as_singleton()
            .expect("query editor should be backed by a singleton buffer");
        if enable {
            if let Some(regex_language) = self.regex_language.clone() {
                query_buffer.update(cx, |query_buffer, cx| {
                    query_buffer.set_language(Some(regex_language), cx);
                })
            }
        } else {
            query_buffer.update(cx, |query_buffer, cx| {
                query_buffer.set_language(None, cx);
            })
        }
    }
}

pub(crate) fn buffer_search_query(
    workspace: &mut Workspace,
    item: &dyn ItemHandle,
    cx: &mut Context<Workspace>,
) -> Option<String> {
    let buffer_search_bar = workspace
        .pane_for(item)
        .and_then(|pane| {
            pane.read(cx)
                .toolbar()
                .read(cx)
                .item_of_type::<BufferSearchBar>()
        })?
        .read(cx);
    if buffer_search_bar.query_editor_focused() {
        let buffer_search_query = buffer_search_bar.query(cx);
        if !buffer_search_query.is_empty() {
            return Some(buffer_search_query);
        }
    }
    None
}
