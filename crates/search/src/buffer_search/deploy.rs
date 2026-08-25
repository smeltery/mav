use super::*;

impl BufferSearchBar {
    pub fn deploy(
        &mut self,
        deploy: &Deploy,
        seed_query_override: Option<SeedQuerySetting>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let filtered_search_range = if deploy.selection_search_enabled {
            Some(FilteredSearchRange::Default)
        } else {
            None
        };
        if self.show(window, cx) {
            if let Some(active_item) = self.active_searchable_item.as_mut() {
                active_item.toggle_filtered_search_ranges(filtered_search_range, window, cx);
            }
            self.search_suggested(seed_query_override, window, cx);
            self.smartcase(window, cx);
            self.sync_select_next_case_sensitivity(cx);
            self.replace_enabled |= deploy.replace_enabled;
            self.selection_search_enabled =
                self.selection_search_enabled
                    .or(if deploy.selection_search_enabled {
                        Some(FilteredSearchRange::Default)
                    } else {
                        None
                    });
            if deploy.focus {
                let mut handle = self.query_editor.focus_handle(cx);
                let mut select_query = true;

                let has_seed_text = self
                    .query_suggestion(seed_query_override, window, cx)
                    .is_some();
                if deploy.replace_enabled && has_seed_text {
                    handle = self.replacement_editor.focus_handle(cx);
                    select_query = false;
                };

                if select_query {
                    self.select_query(window, cx);
                }

                window.focus(&handle, cx);
            }
            return true;
        }

        cx.propagate();
        false
    }

    pub fn toggle(&mut self, action: &Deploy, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_dismissed() {
            self.deploy(action, None, window, cx);
        } else {
            self.dismiss(&Dismiss, window, cx);
        }
    }

    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(handle) = self.active_searchable_item.as_ref() else {
            return false;
        };

        let configured_options =
            SearchOptions::from_settings(&EditorSettings::get_global(cx).search);
        let settings_changed = configured_options != self.configured_options;

        if self.dismissed && settings_changed {
            // Only update configuration options when search bar is dismissed,
            // so we don't miss updates even after calling show twice
            self.configured_options = configured_options;
            self.search_options = configured_options;
            self.default_options = configured_options;
        }

        // This isn't a normal setting; it's only applicable to vim search.
        self.search_options.remove(SearchOptions::BACKWARDS);

        self.dismissed = false;
        self.adjust_query_regex_language(cx);
        handle.search_bar_visibility_changed(true, window, cx);
        cx.notify();
        cx.emit(Event::UpdateLocation);
        cx.emit(ToolbarItemEvent::ChangeLocation(
            if self.needs_expand_collapse_option(cx) {
                ToolbarItemLocation::PrimaryLeft
            } else {
                ToolbarItemLocation::Secondary
            },
        ));
        true
    }

    pub(super) fn supported_options(&self, cx: &mut Context<Self>) -> workspace::searchable::SearchOptions {
        self.active_searchable_item
            .as_ref()
            .map(|item| item.supported_options(cx))
            .unwrap_or_default()
    }

    // We provide an expand/collapse button if we are in a multibuffer
    // and not doing a project search.
    pub(super) fn needs_expand_collapse_option(&self, cx: &App) -> bool {
        if let Some(item) = &self.active_searchable_item {
            let buffer_kind = item.buffer_kind(cx);

            if buffer_kind == ItemBufferKind::Singleton {
                return false;
            }

            let workspace::searchable::SearchOptions {
                find_in_results, ..
            } = item.supported_options(cx);
            !find_in_results
        } else {
            false
        }
    }

    pub(super) fn toggle_fold_all(&mut self, _: &ToggleFoldAll, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_fold_all_in_item(window, cx);
    }

    pub(super) fn toggle_fold_all_in_item(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = &self.active_searchable_item {
            if let Some(item) = item.act_as_type(TypeId::of::<Editor>(), cx) {
                let editor = item.downcast::<Editor>().expect("Is an editor");
                editor.update(cx, |editor, cx| {
                    let is_collapsed = editor.has_any_buffer_folded(cx);
                    if is_collapsed {
                        editor.unfold_all(&UnfoldAll, window, cx);
                    } else {
                        editor.fold_all(&FoldAll, window, cx);
                    }
                })
            }
        }
    }

    pub fn search_suggested(
        &mut self,
        seed_query_override: Option<SeedQuerySetting>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let search = self
            .query_suggestion(seed_query_override, window, cx)
            .map(|suggestion| {
                self.search(&suggestion, Some(self.default_options), true, window, cx)
            });

        #[cfg(target_os = "macos")]
        let search = search.or_else(|| {
            self.pending_external_query
                .take()
                .map(|(query, options)| self.search(&query, Some(options), true, window, cx))
        });

        if let Some(search) = search {
            cx.spawn_in(window, async move |this, cx| {
                if search.await.is_ok() {
                    this.update_in(cx, |this, window, cx| {
                        if !this.dismissed {
                            this.activate_current_match(window, cx)
                        }
                    })
                } else {
                    Ok(())
                }
            })
            .detach_and_log_err(cx);
        }
    }

    pub fn activate_current_match(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(match_ix) = self.active_match_index
            && let Some(active_searchable_item) = self.active_searchable_item.as_ref()
            && let Some((matches, token)) = self
                .searchable_items_with_matches
                .get(&active_searchable_item.downgrade())
        {
            active_searchable_item.activate_match(match_ix, matches, *token, window, cx)
        }
    }

    pub fn select_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query_editor.update(cx, |query_editor, cx| {
            query_editor.select_all(&Default::default(), window, cx);
        });
    }

    pub fn query(&self, cx: &App) -> String {
        self.query_editor.read(cx).text(cx)
    }

    pub fn replacement(&self, cx: &mut App) -> String {
        self.replacement_editor.read(cx).text(cx)
    }

    pub fn query_suggestion(
        &mut self,
        seed_query_override: Option<SeedQuerySetting>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        self.active_searchable_item
            .as_ref()
            .map(|searchable_item| {
                searchable_item.query_suggestion(seed_query_override, window, cx)
            })
            .filter(|suggestion| !suggestion.is_empty())
    }

    pub fn set_replacement(&mut self, replacement: Option<&str>, cx: &mut Context<Self>) {
        if replacement.is_none() {
            self.replace_enabled = false;
            return;
        }
        self.replace_enabled = true;
        self.replacement_editor
            .update(cx, |replacement_editor, cx| {
                replacement_editor
                    .buffer()
                    .update(cx, |replacement_buffer, cx| {
                        let len = replacement_buffer.len(cx);
                        replacement_buffer.edit(
                            [(MultiBufferOffset(0)..len, replacement.unwrap())],
                            None,
                            cx,
                        );
                    });
            });
    }

    pub fn focus_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus(&self.replacement_editor.focus_handle(cx), window, cx);
        cx.notify();
    }
}
