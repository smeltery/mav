use super::*;

impl ProjectSearchView {
    pub fn new_search_in_directory(
        workspace: &mut Workspace,
        dir_path: &RelPath,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let filter_str = dir_path.display(workspace.path_style(cx));

        let weak_workspace = cx.entity().downgrade();

        let entity = cx.new(|cx| ProjectSearch::new(workspace.project().clone(), cx));
        let search = cx.new(|cx| ProjectSearchView::new(weak_workspace, entity, window, cx, None));
        workspace.add_item_to_active_pane(Box::new(search.clone()), None, true, window, cx);
        search.update(cx, |search, cx| {
            search
                .included_files_editor
                .update(cx, |editor, cx| editor.set_text(filter_str, window, cx));
            search.filters_enabled = true;
            search.focus_query_editor(window, cx)
        });
    }

    /// Re-activate the most recently activated search in this pane or the most recent if it has been closed.
    /// If no search exists in the workspace, create a new one.
    pub fn deploy_search(
        workspace: &mut Workspace,
        action: &workspace::DeploySearch,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let existing = workspace
            .active_pane()
            .read(cx)
            .items()
            .find_map(|item| item.downcast::<ProjectSearchView>());

        Self::existing_or_new_search(workspace, existing, action, window, cx);
    }

    pub fn search_in_new(
        workspace: &mut Workspace,
        _: &SearchInNew,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        if let Some(search_view) = workspace
            .active_item(cx)
            .and_then(|item| item.downcast::<ProjectSearchView>())
        {
            let new_query = search_view.update(cx, |search_view, cx| {
                let open_buffers = if search_view.included_opened_only {
                    Some(search_view.open_buffers(cx, workspace))
                } else {
                    None
                };
                let new_query = search_view.build_search_query(cx, open_buffers);
                if new_query.is_some()
                    && let Some(old_query) = search_view.entity.read(cx).active_query.clone()
                {
                    search_view.query_editor.update(cx, |editor, cx| {
                        editor.set_text(old_query.as_str(), window, cx);
                    });
                    search_view.search_options = SearchOptions::from_query(&old_query);
                    search_view.adjust_query_regex_language(cx);
                }
                new_query
            });
            if let Some(new_query) = new_query {
                let entity = cx.new(|cx| {
                    let mut entity = ProjectSearch::new(workspace.project().clone(), cx);
                    entity.search(new_query, cx);
                    entity
                });
                let weak_workspace = cx.entity().downgrade();
                workspace.add_item_to_active_pane(
                    Box::new(
                        cx.new(|cx| ProjectSearchView::new(weak_workspace, entity, window, cx, None)),
                    ),
                    None,
                    true,
                    window,
                    cx,
                );
            }
        }
    }

    // Add another search tab to the workspace.
    pub fn new_search(
        workspace: &mut Workspace,
        _: &workspace::NewSearch,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::existing_or_new_search(workspace, None, &DeploySearch::default(), window, cx)
    }

    pub(super) fn existing_or_new_search(
        workspace: &mut Workspace,
        existing: Option<Entity<ProjectSearchView>>,
        action: &workspace::DeploySearch,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let query = workspace.active_item(cx).and_then(|item| {
            if let Some(buffer_search_query) = buffer_search_query(workspace, item.as_ref(), cx) {
                return Some(buffer_search_query);
            }

            let editor = item.act_as::<Editor>(cx)?;
            let query = editor.query_suggestion(None, window, cx);
            if query.is_empty() { None } else { Some(query) }
        });

        let search = if let Some(existing) = existing {
            workspace.activate_item(&existing, true, true, window, cx);
            existing
        } else {
            let settings = cx
                .global::<ActiveSettings>()
                .0
                .get(&workspace.project().downgrade());

            let settings = settings.cloned();

            let weak_workspace = cx.entity().downgrade();

            let project_search = cx.new(|cx| ProjectSearch::new(workspace.project().clone(), cx));
            let project_search_view = cx
                .new(|cx| ProjectSearchView::new(weak_workspace, project_search, window, cx, settings));

            workspace.add_item_to_active_pane(
                Box::new(project_search_view.clone()),
                None,
                true,
                window,
                cx,
            );
            project_search_view
        };

        search.update(cx, |search, cx| {
            search.replace_enabled |= action.replace_enabled;
            if let Some(regex) = action.regex {
                search.set_search_option_enabled(SearchOptions::REGEX, regex, cx);
            }
            if let Some(case_sensitive) = action.case_sensitive {
                search.set_search_option_enabled(SearchOptions::CASE_SENSITIVE, case_sensitive, cx);
            }
            if let Some(whole_word) = action.whole_word {
                search.set_search_option_enabled(SearchOptions::WHOLE_WORD, whole_word, cx);
            }
            if let Some(include_ignored) = action.include_ignored {
                search.set_search_option_enabled(SearchOptions::INCLUDE_IGNORED, include_ignored, cx);
            }
            let query = action
                .query
                .as_deref()
                .filter(|q| !q.is_empty())
                .or(query.as_deref());
            if let Some(query) = query {
                search.set_query(query, window, cx);
            }
            if let Some(included_files) = action.included_files.as_deref() {
                search
                    .included_files_editor
                    .update(cx, |editor, cx| editor.set_text(included_files, window, cx));
                search.filters_enabled = true;
            }
            if let Some(excluded_files) = action.excluded_files.as_deref() {
                search
                    .excluded_files_editor
                    .update(cx, |editor, cx| editor.set_text(excluded_files, window, cx));
                search.filters_enabled = true;
            }
            search.focus_query_editor(window, cx)
        });
    }

    pub(super) fn prompt_to_save_if_dirty_then_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<()>> {
        let project = self.entity.read(cx).project.clone();

        let can_autosave = self.results_editor.can_autosave(cx);
        let autosave_setting = self.results_editor.workspace_settings(cx).autosave;

        let will_autosave = can_autosave && autosave_setting.should_save_on_close();

        let is_dirty = self.is_dirty(cx);

        cx.spawn_in(window, async move |this, cx| {
            let skip_save_on_close = this
                .read_with(cx, |this, cx| {
                    this.workspace.read_with(cx, |workspace, cx| {
                        workspace::Pane::skip_save_on_close(&this.results_editor, workspace, cx)
                    })
                })?
                .unwrap_or(false);

            let should_prompt_to_save = !skip_save_on_close && !will_autosave && is_dirty;

            let should_search = if should_prompt_to_save {
                let options = &["Save", "Don't Save", "Cancel"];
                let result_channel = this.update_in(cx, |_, window, cx| {
                    window.prompt(
                        gpui::PromptLevel::Warning,
                        "Project search buffer contains unsaved edits. Do you want to save it?",
                        None,
                        options,
                        cx,
                    )
                })?;
                let result = result_channel.await?;
                let should_save = result == 0;
                if should_save {
                    this.update_in(cx, |this, window, cx| {
                        this.save(
                            SaveOptions {
                                format: true,
                                force_format: false,
                                autosave: false,
                            },
                            project,
                            window,
                            cx,
                        )
                    })?
                    .await
                    .log_err();
                }

                result != 2
            } else {
                true
            };
            if should_search {
                this.update(cx, |this, cx| {
                    this.search(cx);
                })?;
            }
            anyhow::Ok(())
        })
    }
}
