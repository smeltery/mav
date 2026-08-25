use super::*;

impl ProjectSearchView {
    pub fn get_matches(&self, cx: &App) -> Vec<Range<Anchor>> {
        self.entity.read(cx).match_ranges.clone()
    }

    pub(super) fn open_text_finder(
        &mut self,
        _: &OpenTextFinder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        TextFinder::open_from_project_search(cx.entity(), window, cx).detach();
    }

    pub(super) fn toggle_filters(&mut self, cx: &mut Context<Self>) {
        self.filters_enabled = !self.filters_enabled;
        ActiveSettings::update_global(cx, |settings, cx| {
            settings.0.insert(
                self.entity.read(cx).project.downgrade(),
                self.current_settings(),
            );
        });
    }

    pub(super) fn current_settings(&self) -> ProjectSearchSettings {
        ProjectSearchSettings {
            search_options: self.search_options,
            filters_enabled: self.filters_enabled,
        }
    }

    pub(super) fn set_search_option_enabled(
        &mut self,
        option: SearchOptions,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if self.search_options.contains(option) != enabled {
            self.toggle_search_option(option, cx);
        }
    }

    pub(super) fn toggle_search_option(&mut self, option: SearchOptions, cx: &mut Context<Self>) {
        self.search_options.toggle(option);
        ActiveSettings::update_global(cx, |settings, cx| {
            settings.0.insert(
                self.entity.read(cx).project.downgrade(),
                self.current_settings(),
            );
        });
        self.adjust_query_regex_language(cx);
    }

    pub(super) fn toggle_opened_only(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.included_opened_only = !self.included_opened_only;
    }

    pub fn replacement(&self, cx: &App) -> String {
        self.replacement_editor.read(cx).text(cx)
    }

    pub(super) fn replace_next(&mut self, _: &ReplaceNext, window: &mut Window, cx: &mut Context<Self>) {
        if self.entity.read(cx).pending_search.is_some() {
            return;
        }
        if let Some(last_search_query_text) = &self.entity.read(cx).last_search_query_text
            && self.query_editor.read(cx).text(cx) != *last_search_query_text
        {
            // search query has changed, restart search and bail
            self.search(cx);
            return;
        }
        if self.entity.read(cx).match_ranges.is_empty() {
            return;
        }
        let Some(active_index) = self.active_match_index else {
            return;
        };

        let query = self.entity.read(cx).active_query.clone();
        if let Some(query) = query {
            let query = query.with_replacement(self.replacement(cx));

            let mat = self.entity.read(cx).match_ranges.get(active_index).cloned();
            self.results_editor.update(cx, |editor, cx| {
                if let Some(mat) = mat.as_ref() {
                    editor.replace(mat, &query, SearchToken::default(), window, cx);
                }
            });
            self.select_match(Direction::Next, window, cx)
        }
    }

    pub(super) fn replace_all(&mut self, _: &ReplaceAll, window: &mut Window, cx: &mut Context<Self>) {
        if self.entity.read(cx).pending_search.is_some() {
            self.pending_replace_all = true;
            return;
        }
        let query_text = self.query_editor.read(cx).text(cx);
        let query_is_stale =
            self.entity.read(cx).last_search_query_text.as_deref() != Some(query_text.as_str());
        if query_is_stale {
            self.pending_replace_all = true;
            self.search(cx);
            if self.entity.read(cx).pending_search.is_none() {
                self.pending_replace_all = false;
            }
            return;
        }
        self.pending_replace_all = false;
        if self.active_match_index.is_none() {
            return;
        }
        let Some(query) = self.entity.read(cx).active_query.as_ref() else {
            return;
        };
        let query = query.clone().with_replacement(self.replacement(cx));

        let match_ranges = self
            .entity
            .update(cx, |model, _| mem::take(&mut model.match_ranges));
        if match_ranges.is_empty() {
            return;
        }

        self.results_editor.update(cx, |editor, cx| {
            editor.replace_all(
                &mut match_ranges.iter(),
                &query,
                SearchToken::default(),
                window,
                cx,
            );
        });

        self.entity.update(cx, |model, _cx| {
            model.match_ranges = match_ranges;
        });
    }

    pub(super) fn toggle_all_search_results(
        &mut self,
        _: &ToggleAllSearchResults,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_results_visibility(window, cx);
    }

    pub(super) fn update_results_visibility(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let has_any_folded = self.results_editor.read(cx).has_any_buffer_folded(cx);
        self.results_editor.update(cx, |editor, cx| {
            if has_any_folded {
                editor.unfold_all(&UnfoldAll, window, cx);
            } else {
                editor.fold_all(&FoldAll, window, cx);
            }
        });
        cx.notify();
    }

    pub fn new(
        workspace: WeakEntity<Workspace>,
        entity: Entity<ProjectSearch>,
        window: &mut Window,
        cx: &mut Context<Self>,
        settings: Option<ProjectSearchSettings>,
    ) -> Self {
        let project;
        let excerpts;
        let mut replacement_text = None;
        let mut query_text = String::new();
        let mut subscriptions = Vec::new();

        // Read in settings if available
        let (mut options, filters_enabled) = if let Some(settings) = settings {
            (settings.search_options, settings.filters_enabled)
        } else {
            let search_options =
                SearchOptions::from_settings(&EditorSettings::get_global(cx).search);
            (search_options, false)
        };

        {
            let entity = entity.read(cx);
            project = entity.project.clone();
            excerpts = entity.excerpts.clone();
            if let Some(active_query) = entity.active_query.as_ref() {
                query_text = active_query.as_str().to_string();
                replacement_text = active_query.replacement().map(ToOwned::to_owned);
                options = SearchOptions::from_query(active_query);
            }
        }
        subscriptions.push(cx.observe_in(&entity, window, |this, _, window, cx| {
            this.entity_changed(window, cx)
        }));

        let query_editor = cx.new(|cx| {
            let mut editor = Editor::auto_height(1, 4, window, cx);
            editor.set_placeholder_text("Search all files…", window, cx);
            editor.set_use_autoclose(false);
            editor.set_use_selection_highlight(false);
            editor.set_text(query_text, window, cx);
            editor
        });
        // Subscribe to query_editor in order to reraise editor events for workspace item activation purposes
        subscriptions.push(
            cx.subscribe(&query_editor, |this, _, event: &EditorEvent, cx| {
                if let EditorEvent::Edited { .. } = event
                    && EditorSettings::get_global(cx).use_smartcase_search
                {
                    let query = this.search_query_text(cx);
                    if !query.is_empty()
                        && this.search_options.contains(SearchOptions::CASE_SENSITIVE)
                            != contains_uppercase(&query)
                    {
                        this.toggle_search_option(SearchOptions::CASE_SENSITIVE, cx);
                    }
                }
                cx.emit(ViewEvent::EditorEvent(event.clone()))
            }),
        );
        let replacement_editor = cx.new(|cx| {
            let mut editor = Editor::auto_height(1, 4, window, cx);
            editor.set_placeholder_text(REPLACE_PLACEHOLDER, window, cx);
            if let Some(text) = replacement_text {
                editor.set_text(text, window, cx);
            }
            editor
        });
        let results_editor = cx.new(|cx| {
            let mut editor = Editor::for_multibuffer(excerpts, Some(project.clone()), window, cx);
            editor.set_searchable(false);
            editor.set_in_project_search(true);
            editor
        });
        subscriptions.push(cx.observe(&results_editor, |_, _, cx| cx.emit(ViewEvent::UpdateTab)));

        subscriptions.push(
            cx.subscribe(&results_editor, |this, _, event: &EditorEvent, cx| {
                if matches!(event, editor::EditorEvent::SelectionsChanged { .. }) {
                    this.update_match_index(cx);
                }
                // Reraise editor events for workspace item activation purposes
                cx.emit(ViewEvent::EditorEvent(event.clone()));
            }),
        );
        subscriptions.push(cx.subscribe(
            &results_editor,
            |_this, _editor, _event: &SearchEvent, cx| cx.notify(),
        ));

        let included_files_editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text(INCLUDE_PLACEHOLDER, window, cx);

            editor
        });
        // Subscribe to include_files_editor in order to reraise editor events for workspace item activation purposes
        subscriptions.push(
            cx.subscribe(&included_files_editor, |_, _, event: &EditorEvent, cx| {
                cx.emit(ViewEvent::EditorEvent(event.clone()))
            }),
        );

        let excluded_files_editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text(EXCLUDE_PLACEHOLDER, window, cx);

            editor
        });
        // Subscribe to excluded_files_editor in order to reraise editor events for workspace item activation purposes
        subscriptions.push(
            cx.subscribe(&excluded_files_editor, |_, _, event: &EditorEvent, cx| {
                cx.emit(ViewEvent::EditorEvent(event.clone()))
            }),
        );

        let focus_handle = cx.focus_handle();
        subscriptions.push(cx.on_focus(&focus_handle, window, |_, window, cx| {
            cx.on_next_frame(window, |this, window, cx| {
                if this.focus_handle.is_focused(window) {
                    if this.has_matches() {
                        this.results_editor.focus_handle(cx).focus(window, cx);
                    } else {
                        this.query_editor.focus_handle(cx).focus(window, cx);
                    }
                }
            });
        }));

        let languages = project.read(cx).languages().clone();
        cx.spawn(async move |project_search_view, cx| {
            let regex_language = languages
                .language_for_name("regex")
                .await
                .context("loading regex language")?;
            project_search_view
                .update(cx, |project_search_view, cx| {
                    project_search_view.regex_language = Some(regex_language);
                    project_search_view.adjust_query_regex_language(cx);
                })
                .ok();
            anyhow::Ok(())
        })
        .detach_and_log_err(cx);

        // Check if Worktrees have all been previously indexed
        let mut this = ProjectSearchView {
            workspace,
            focus_handle,
            replacement_editor,
            search_id: entity.read(cx).search_id,
            entity,
            query_editor,
            results_editor,
            search_options: options,
            panels_with_errors: HashMap::default(),
            active_match_index: None,
            included_files_editor,
            excluded_files_editor,
            filters_enabled,
            replace_enabled: false,
            pending_replace_all: false,
            included_opened_only: false,
            regex_language: None,
            _subscriptions: subscriptions,
        };

        this.entity_changed(window, cx);
        this
    }
}
