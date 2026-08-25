use super::*;

impl Default for ProjectSearchBar {
    pub(super) fn default() -> Self {
        Self::new()
    }
}

impl ProjectSearchBar {
    pub fn new() -> Self {
        Self {
            active_project_search: None,
            subscription: None,
        }
    }

    pub(super) fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(search_view) = self.active_project_search.as_ref() {
            search_view.update(cx, |search_view, cx| {
                if !search_view
                    .replacement_editor
                    .focus_handle(cx)
                    .is_focused(window)
                {
                    cx.stop_propagation();
                    search_view
                        .prompt_to_save_if_dirty_then_search(window, cx)
                        .detach_and_log_err(cx);
                }
            });
        }
    }

    pub(super) fn tab(&mut self, _: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_field(Direction::Next, window, cx);
    }

    pub(super) fn backtab(&mut self, _: &Backtab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_field(Direction::Prev, window, cx);
    }

    pub(super) fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(search_view) = self.active_project_search.as_ref() {
            search_view.update(cx, |search_view, cx| {
                search_view.query_editor.focus_handle(cx).focus(window, cx);
            });
        }
    }

    pub(super) fn cycle_field(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let active_project_search = match &self.active_project_search {
            Some(active_project_search) => active_project_search,
            None => return,
        };

        active_project_search.update(cx, |project_view, cx| {
            let mut views = vec![project_view.query_editor.focus_handle(cx)];
            if project_view.replace_enabled {
                views.push(project_view.replacement_editor.focus_handle(cx));
            }
            if project_view.filters_enabled {
                views.extend([
                    project_view.included_files_editor.focus_handle(cx),
                    project_view.excluded_files_editor.focus_handle(cx),
                ]);
            }
            let current_index = match views.iter().position(|focus| focus.is_focused(window)) {
                Some(index) => index,
                None => return,
            };

            let new_index = match direction {
                Direction::Next => (current_index + 1) % views.len(),
                Direction::Prev if current_index == 0 => views.len() - 1,
                Direction::Prev => (current_index - 1) % views.len(),
            };
            let next_focus_handle = &views[new_index];
            window.focus(next_focus_handle, cx);
            cx.stop_propagation();
        });
    }

    pub(crate) fn toggle_search_option(
        &mut self,
        option: SearchOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.active_project_search.is_none() {
            return false;
        }

        cx.spawn_in(window, async move |this, cx| {
            let task = this.update_in(cx, |this, window, cx| {
                let search_view = this.active_project_search.as_ref()?;
                search_view.update(cx, |search_view, cx| {
                    search_view.toggle_search_option(option, cx);
                    search_view
                        .entity
                        .read(cx)
                        .active_query
                        .is_some()
                        .then(|| search_view.prompt_to_save_if_dirty_then_search(window, cx))
                })
            })?;
            if let Some(task) = task {
                task.await?;
            }
            this.update(cx, |_, cx| {
                cx.notify();
            })?;
            anyhow::Ok(())
        })
        .detach();
        true
    }

    pub(super) fn toggle_replace(
        &mut self,
        _: &ToggleReplace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(search) = &self.active_project_search {
            search.update(cx, |this, cx| {
                this.replace_enabled = !this.replace_enabled;
                let editor_to_focus = if this.replace_enabled {
                    this.replacement_editor.focus_handle(cx)
                } else {
                    this.query_editor.focus_handle(cx)
                };
                window.focus(&editor_to_focus, cx);
                cx.notify();
            });
        }
    }

    pub(super) fn toggle_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(search_view) = self.active_project_search.as_ref() {
            search_view.update(cx, |search_view, cx| {
                search_view.toggle_filters(cx);
                search_view
                    .included_files_editor
                    .update(cx, |_, cx| cx.notify());
                search_view
                    .excluded_files_editor
                    .update(cx, |_, cx| cx.notify());
                window.refresh();
                cx.notify();
            });
            cx.notify();
            true
        } else {
            false
        }
    }

    pub(super) fn toggle_opened_only(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.active_project_search.is_none() {
            return false;
        }

        cx.spawn_in(window, async move |this, cx| {
            let task = this.update_in(cx, |this, window, cx| {
                let search_view = this.active_project_search.as_ref()?;
                search_view.update(cx, |search_view, cx| {
                    search_view.toggle_opened_only(window, cx);
                    search_view
                        .entity
                        .read(cx)
                        .active_query
                        .is_some()
                        .then(|| search_view.prompt_to_save_if_dirty_then_search(window, cx))
                })
            })?;
            if let Some(task) = task {
                task.await?;
            }
            this.update(cx, |_, cx| {
                cx.notify();
            })?;
            anyhow::Ok(())
        })
        .detach();
        true
    }

    pub(super) fn is_opened_only_enabled(&self, cx: &App) -> bool {
        if let Some(search_view) = self.active_project_search.as_ref() {
            search_view.read(cx).included_opened_only
        } else {
            false
        }
    }

    pub(super) fn move_focus_to_results(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(search_view) = self.active_project_search.as_ref() {
            search_view.update(cx, |search_view, cx| {
                search_view.move_focus_to_results(window, cx);
            });
            cx.notify();
        }
    }

    pub(super) fn next_history_query(
        &mut self,
        _: &NextHistoryQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(search_view) = self.active_project_search.as_ref() {
            search_view.update(cx, |search_view, cx| {
                for (editor, kind) in [
                    (search_view.query_editor.clone(), SearchInputKind::Query),
                    (
                        search_view.included_files_editor.clone(),
                        SearchInputKind::Include,
                    ),
                    (
                        search_view.excluded_files_editor.clone(),
                        SearchInputKind::Exclude,
                    ),
                ] {
                    if editor.focus_handle(cx).is_focused(window) {
                        if !should_navigate_history(&editor, HistoryNavigationDirection::Next, cx) {
                            cx.propagate();
                            return;
                        }

                        let new_query = search_view.entity.update(cx, |model, cx| {
                            let project = model.project.clone();

                            if let Some(new_query) = project.update(cx, |project, _| {
                                project
                                    .search_history_mut(kind)
                                    .next(model.cursor_mut(kind))
                                    .map(str::to_string)
                            }) {
                                Some(new_query)
                            } else {
                                model.cursor_mut(kind).take_draft()
                            }
                        });
                        if let Some(new_query) = new_query {
                            search_view.set_search_editor(kind, &new_query, window, cx);
                        }
                    }
                }
            });
        }
    }

    pub(super) fn previous_history_query(
        &mut self,
        _: &PreviousHistoryQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(search_view) = self.active_project_search.as_ref() {
            search_view.update(cx, |search_view, cx| {
                for (editor, kind) in [
                    (search_view.query_editor.clone(), SearchInputKind::Query),
                    (
                        search_view.included_files_editor.clone(),
                        SearchInputKind::Include,
                    ),
                    (
                        search_view.excluded_files_editor.clone(),
                        SearchInputKind::Exclude,
                    ),
                ] {
                    if editor.focus_handle(cx).is_focused(window) {
                        if !should_navigate_history(
                            &editor,
                            HistoryNavigationDirection::Previous,
                            cx,
                        ) {
                            cx.propagate();
                            return;
                        }

                        if editor.read(cx).text(cx).is_empty()
                            && let Some(new_query) = search_view
                                .entity
                                .read(cx)
                                .project
                                .read(cx)
                                .search_history(kind)
                                .current(search_view.entity.read(cx).cursor(kind))
                                .map(str::to_string)
                        {
                            search_view.set_search_editor(kind, &new_query, window, cx);
                            return;
                        }

                        let current_query = editor.read(cx).text(cx);
                        if let Some(new_query) = search_view.entity.update(cx, |model, cx| {
                            let project = model.project.clone();
                            project.update(cx, |project, _| {
                                project
                                    .search_history_mut(kind)
                                    .previous(model.cursor_mut(kind), &current_query)
                                    .map(str::to_string)
                            })
                        }) {
                            search_view.set_search_editor(kind, &new_query, window, cx);
                        }
                    }
                }
            });
        }
    }

    pub(super) fn select_next_match(
        &mut self,
        _: &SelectNextMatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(search) = self.active_project_search.as_ref() {
            search.update(cx, |this, cx| {
                this.select_match(Direction::Next, window, cx);
            })
        }
    }

    pub(super) fn select_prev_match(
        &mut self,
        _: &SelectPreviousMatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(search) = self.active_project_search.as_ref() {
            search.update(cx, |this, cx| {
                this.select_match(Direction::Prev, window, cx);
            })
        }
    }

    pub(super) fn open_text_finder(
        &mut self,
        _: &OpenTextFinder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(search) = &self.active_project_search else {
            tracing::warn!("active_project_search was none");
            return;
        };

        TextFinder::open_from_project_search(Entity::clone(search), window, cx).detach();
    }
}
