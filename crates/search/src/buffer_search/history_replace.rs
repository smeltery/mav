use super::*;

impl BufferSearchBar {
    pub(super) fn tab(&mut self, _: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_field(Direction::Next, window, cx);
    }

    pub(super) fn backtab(&mut self, _: &Backtab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_field(Direction::Prev, window, cx);
    }
    pub(super) fn cycle_field(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut handles = vec![self.query_editor.focus_handle(cx)];
        if self.replace_enabled {
            handles.push(self.replacement_editor.focus_handle(cx));
        }
        if let Some(item) = self.active_searchable_item.as_ref() {
            handles.push(item.item_focus_handle(cx));
        }
        let current_index = match handles.iter().position(|focus| focus.is_focused(window)) {
            Some(index) => index,
            None => return,
        };

        let new_index = match direction {
            Direction::Next => (current_index + 1) % handles.len(),
            Direction::Prev if current_index == 0 => handles.len() - 1,
            Direction::Prev => (current_index - 1) % handles.len(),
        };
        let next_focus_handle = &handles[new_index];
        self.focus(next_focus_handle, window, cx);
        cx.stop_propagation();
    }

    pub(super) fn next_history_query(
        &mut self,
        _: &NextHistoryQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !should_navigate_history(&self.query_editor, HistoryNavigationDirection::Next, cx) {
            cx.propagate();
            return;
        }

        if let Some(new_query) = self
            .search_history
            .next(&mut self.search_history_cursor)
            .map(str::to_string)
        {
            drop(self.search(&new_query, Some(self.search_options), false, window, cx));
        } else if let Some(draft) = self.search_history_cursor.take_draft() {
            drop(self.search(&draft, Some(self.search_options), false, window, cx));
        }
    }

    pub(super) fn previous_history_query(
        &mut self,
        _: &PreviousHistoryQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !should_navigate_history(&self.query_editor, HistoryNavigationDirection::Previous, cx) {
            cx.propagate();
            return;
        }

        if self.query(cx).is_empty()
            && let Some(new_query) = self
                .search_history
                .current(&self.search_history_cursor)
                .map(str::to_string)
        {
            drop(self.search(&new_query, Some(self.search_options), false, window, cx));
            return;
        }

        let current_query = self.query(cx);
        if let Some(new_query) = self
            .search_history
            .previous(&mut self.search_history_cursor, &current_query)
            .map(str::to_string)
        {
            drop(self.search(&new_query, Some(self.search_options), false, window, cx));
        }
    }

    pub(super) fn focus(&self, handle: &gpui::FocusHandle, window: &mut Window, cx: &mut App) {
        window.invalidate_character_coordinates();
        window.focus(handle, cx);
    }

    pub(super) fn toggle_replace(
        &mut self,
        _: &ToggleReplace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_searchable_item.is_some() {
            self.replace_enabled = !self.replace_enabled;
            let handle = if self.replace_enabled {
                self.replacement_editor.focus_handle(cx)
            } else {
                self.query_editor.focus_handle(cx)
            };
            self.focus(&handle, window, cx);
            cx.notify();
        }
    }

    pub(super) fn replace_next(
        &mut self,
        _: &ReplaceNext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut should_propagate = true;
        if !self.dismissed
            && self.active_search.is_some()
            && let Some(searchable_item) = self.active_searchable_item.as_ref()
            && let Some(query) = self.active_search.as_ref()
            && let Some((matches, token)) = self
                .searchable_items_with_matches
                .get(&searchable_item.downgrade())
        {
            if let Some(active_index) = self.active_match_index {
                let query = query
                    .as_ref()
                    .clone()
                    .with_replacement(self.replacement(cx));
                searchable_item.replace(matches.at(active_index), &query, *token, window, cx);
                self.select_next_match(&SelectNextMatch, window, cx);
            }
            should_propagate = false;
        }
        if !should_propagate {
            cx.stop_propagation();
        }
    }

    pub fn replace_all(&mut self, _: &ReplaceAll, window: &mut Window, cx: &mut Context<Self>) {
        if !self.dismissed
            && self.active_search.is_some()
            && let Some(searchable_item) = self.active_searchable_item.as_ref()
            && let Some(query) = self.active_search.as_ref()
            && let Some((matches, token)) = self
                .searchable_items_with_matches
                .get(&searchable_item.downgrade())
        {
            let query = query
                .as_ref()
                .clone()
                .with_replacement(self.replacement(cx));
            searchable_item.replace_all(&mut matches.iter(), &query, *token, window, cx);
        }
    }

    pub fn match_exists(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.update_match_index(window, cx);
        self.active_match_index.is_some()
    }

    pub fn should_use_smartcase_search(&mut self, cx: &mut Context<Self>) -> bool {
        EditorSettings::get_global(cx).use_smartcase_search
    }

    pub fn is_contains_uppercase(&mut self, str: &String) -> bool {
        str.chars().any(|c| c.is_uppercase())
    }

    pub(super) fn smartcase(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.should_use_smartcase_search(cx) {
            let query = self.query(cx);
            if !query.is_empty() {
                let is_case = self.is_contains_uppercase(&query);
                if self.has_search_option(SearchOptions::CASE_SENSITIVE) != is_case {
                    self.toggle_search_option(SearchOptions::CASE_SENSITIVE, window, cx);
                }
            }
        }
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

    /// Updates the searchable item's case sensitivity option to match the
    /// search bar's current case sensitivity setting. This ensures that
    /// editor's `select_next`/ `select_previous` operations respect the buffer
    /// search bar's search options.
    ///
    /// Clears the case sensitivity when the search bar is dismissed so that
    /// only the editor's settings are respected.
    pub(super) fn sync_select_next_case_sensitivity(&self, cx: &mut Context<Self>) {
        let case_sensitive = match self.dismissed {
            true => None,
            false => Some(self.search_options.contains(SearchOptions::CASE_SENSITIVE)),
        };

        if let Some(active_searchable_item) = self.active_searchable_item.as_ref() {
            active_searchable_item.set_search_is_case_sensitive(case_sensitive, cx);
        }
    }
}
