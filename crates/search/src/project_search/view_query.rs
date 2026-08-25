use super::*;

impl ProjectSearchView {
    fn search(&mut self, cx: &mut Context<Self>) {
        let open_buffers = if self.included_opened_only {
            self.workspace
                .update(cx, |workspace, cx| self.open_buffers(cx, workspace))
                .ok()
        } else {
            None
        };
        if let Some(query) = self.build_search_query(cx, open_buffers) {
            self.entity.update(cx, |model, cx| model.search(query, cx));
        }
    }

    pub fn search_query_text(&self, cx: &App) -> String {
        self.query_editor.read(cx).text(cx)
    }

    fn build_search_query(
        &mut self,
        cx: &mut Context<Self>,
        open_buffers: Option<Vec<Entity<Buffer>>>,
    ) -> Option<SearchQuery> {
        // Do not bail early in this function, as we want to fill out `self.panels_with_errors`.

        let text = self.search_query_text(cx);
        let included_files = self
            .filters_enabled
            .then(
                || match self.parse_path_matches(self.included_files_editor.read(cx).text(cx), cx) {
                    Ok(included_files) => {
                        let should_unmark_error = self.panels_with_errors.remove(&InputPanel::Include);
                        if should_unmark_error.is_some() {
                            cx.notify();
                        }
                        included_files
                    }
                    Err(e) => {
                        let should_mark_error = self
                            .panels_with_errors
                            .insert(InputPanel::Include, e.to_string());
                        if should_mark_error.is_none() {
                            cx.notify();
                        }
                        PathMatcher::default()
                    }
                },
            )
            .unwrap_or(PathMatcher::default());
        let excluded_files = self
            .filters_enabled
            .then(
                || match self.parse_path_matches(self.excluded_files_editor.read(cx).text(cx), cx) {
                    Ok(excluded_files) => {
                        let should_unmark_error = self.panels_with_errors.remove(&InputPanel::Exclude);
                        if should_unmark_error.is_some() {
                            cx.notify();
                        }

                        excluded_files
                    }
                    Err(e) => {
                        let should_mark_error = self
                            .panels_with_errors
                            .insert(InputPanel::Exclude, e.to_string());
                        if should_mark_error.is_none() {
                            cx.notify();
                        }
                        PathMatcher::default()
                    }
                },
            )
            .unwrap_or(PathMatcher::default());

        // If the project contains multiple visible worktrees, we match the
        // include/exclude patterns against full paths to allow them to be
        // disambiguated. For single worktree projects we use worktree relative
        // paths for convenience.
        let match_full_paths = self
            .entity
            .read(cx)
            .project
            .read(cx)
            .visible_worktrees(cx)
            .count()
            > 1;

        let query = match self.search_options.build_query(
            text,
            included_files,
            excluded_files,
            match_full_paths,
            open_buffers,
        ) {
            Ok(query) => {
                let should_unmark_error = self.panels_with_errors.remove(&InputPanel::Query);
                if should_unmark_error.is_some() {
                    cx.notify();
                }

                Some(query)
            }
            Err(e) => {
                let should_mark_error = self
                    .panels_with_errors
                    .insert(InputPanel::Query, e.to_string());
                if should_mark_error.is_none() {
                    cx.notify();
                }

                None
            }
        };
        if !self.panels_with_errors.is_empty() {
            return None;
        }
        if query.as_ref().is_some_and(|query| query.is_empty()) {
            return None;
        }
        query
    }

    fn open_buffers(&self, cx: &App, workspace: &Workspace) -> Vec<Entity<Buffer>> {
        let mut buffers = Vec::new();
        for editor in workspace.items_of_type::<Editor>(cx) {
            if let Some(buffer) = editor.read(cx).buffer().read(cx).as_singleton() {
                buffers.push(buffer);
            }
        }
        buffers
    }

    /// The include/exclude path matchers currently configured on this view,
    /// honoring `filters_enabled`. Read-only (unlike `build_search_query` it does
    /// not record parse errors in `panels_with_errors`); invalid globs fall back
    /// to a default (match-all) matcher. Shared with the text finder, which is
    /// backed by the same view.
    pub(crate) fn file_path_filters(&self, cx: &App) -> (PathMatcher, PathMatcher) {
        if !self.filters_enabled {
            return (PathMatcher::default(), PathMatcher::default());
        }
        let included = self
            .parse_path_matches(self.included_files_editor.read(cx).text(cx), cx)
            .unwrap_or_default();
        let excluded = self
            .parse_path_matches(self.excluded_files_editor.read(cx).text(cx), cx)
            .unwrap_or_default();
        (included, excluded)
    }

    fn parse_path_matches(&self, text: String, cx: &App) -> anyhow::Result<PathMatcher> {
        let path_style = self.entity.read(cx).project.read(cx).path_style(cx);
        let queries = split_glob_patterns(&text)
            .into_iter()
            .map(str::trim)
            .filter(|maybe_glob_str| !maybe_glob_str.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        Ok(PathMatcher::new(&queries, path_style)?)
    }
}
