use super::*;

impl BufferSearchBar {
    pub(super) fn on_query_editor_event(
        &mut self,
        _editor: &Entity<Editor>,
        event: &editor::EditorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            editor::EditorEvent::Focused => self.query_editor_focused = true,
            editor::EditorEvent::Blurred => self.query_editor_focused = false,
            editor::EditorEvent::Edited { .. } => {
                self.smartcase(window, cx);
                self.clear_matches(window, cx);
                let search = self.update_matches(false, true, window, cx);

                cx.spawn_in(window, async move |this, cx| {
                    if search.await.is_ok() {
                        this.update_in(cx, |this, window, cx| {
                            this.activate_current_match(window, cx);
                            #[cfg(target_os = "macos")]
                            this.update_find_pasteboard(cx);
                        })?;
                    }
                    anyhow::Ok(())
                })
                .detach_and_log_err(cx);
            }
            _ => {}
        }
    }

    pub(super) fn on_replacement_editor_event(
        &mut self,
        _: Entity<Editor>,
        event: &editor::EditorEvent,
        _: &mut Context<Self>,
    ) {
        match event {
            editor::EditorEvent::Focused => self.replacement_editor_focused = true,
            editor::EditorEvent::Blurred => self.replacement_editor_focused = false,
            _ => {}
        }
    }

    pub(super) fn on_active_searchable_item_event(
        &mut self,
        event: &SearchEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SearchEvent::MatchesInvalidated => {
                drop(self.update_matches(false, false, window, cx));
            }
            SearchEvent::ActiveMatchChanged => self.update_match_index(window, cx),
        }
    }

    pub(super) fn toggle_case_sensitive(
        &mut self,
        _: &ToggleCaseSensitive,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_search_option(SearchOptions::CASE_SENSITIVE, window, cx)
    }

    pub(super) fn toggle_whole_word(
        &mut self,
        _: &ToggleWholeWord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_search_option(SearchOptions::WHOLE_WORD, window, cx)
    }

    pub(super) fn toggle_selection(
        &mut self,
        _: &ToggleSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_search_within_selection(
            if let Some(_) = self.selection_search_enabled {
                None
            } else {
                Some(FilteredSearchRange::Default)
            },
            window,
            cx,
        );
    }

    pub(super) fn toggle_regex(
        &mut self,
        _: &ToggleRegex,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_search_option(SearchOptions::REGEX, window, cx)
    }

    pub(super) fn clear_active_searchable_item_matches(
        &mut self,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(active_searchable_item) = self.active_searchable_item.as_ref() {
            self.active_match_index = None;
            self.searchable_items_with_matches
                .remove(&active_searchable_item.downgrade());
            active_searchable_item.clear_matches(window, cx);
        }
    }

    pub fn has_active_match(&self) -> bool {
        self.active_match_index.is_some()
    }

    pub(super) fn clear_matches(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut active_item_matches = None;
        for (searchable_item, matches) in self.searchable_items_with_matches.drain() {
            if let Some(searchable_item) =
                WeakSearchableItemHandle::upgrade(searchable_item.as_ref(), cx)
            {
                if Some(&searchable_item) == self.active_searchable_item.as_ref() {
                    active_item_matches = Some((searchable_item.downgrade(), matches));
                } else {
                    searchable_item.clear_matches(window, cx);
                }
            }
        }

        self.searchable_items_with_matches
            .extend(active_item_matches);
    }
}
