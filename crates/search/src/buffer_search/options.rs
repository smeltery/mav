use super::*;

impl BufferSearchBar {
    pub fn search(
        &mut self,
        query: &str,
        options: Option<SearchOptions>,
        add_to_history: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<()> {
        let options = options.unwrap_or(self.default_options);
        let updated = query != self.query(cx) || self.search_options != options;
        if updated {
            self.query_editor.update(cx, |query_editor, cx| {
                query_editor.buffer().update(cx, |query_buffer, cx| {
                    let len = query_buffer.len(cx);
                    query_buffer.edit([(MultiBufferOffset(0)..len, query)], None, cx);
                });
                query_editor.request_autoscroll(Autoscroll::fit(), cx);
            });
            self.set_search_options(options, cx);
            self.clear_matches(window, cx);
            #[cfg(target_os = "macos")]
            self.update_find_pasteboard(cx);
            cx.notify();
        }
        self.update_matches(!updated, add_to_history, window, cx)
    }

    #[cfg(target_os = "macos")]
    pub fn update_find_pasteboard(&mut self, cx: &mut App) {
        cx.write_to_find_pasteboard(gpui::ClipboardItem::new_string_with_metadata(
            self.query(cx),
            self.search_options.bits().to_string(),
        ));
    }

    pub fn use_selection_for_find(
        &mut self,
        _: &UseSelectionForFind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.deploy(
            &Deploy {
                focus: false,
                replace_enabled: false,
                selection_search_enabled: false,
            },
            Some(SeedQuerySetting::Always),
            window,
            cx,
        );
    }

    pub fn focus_editor(&mut self, _: &FocusEditor, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(active_editor) = self.active_searchable_item.as_ref() {
            let handle = active_editor.item_focus_handle(cx);
            window.focus(&handle, cx);
        }
    }

    pub fn toggle_search_option(
        &mut self,
        search_option: SearchOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.search_options.toggle(search_option);
        self.default_options = self.search_options;
        drop(self.update_matches(false, false, window, cx));
        self.adjust_query_regex_language(cx);
        self.sync_select_next_case_sensitivity(cx);
        cx.notify();
    }

    pub fn has_search_option(&mut self, search_option: SearchOptions) -> bool {
        self.search_options.contains(search_option)
    }

    pub fn enable_search_option(
        &mut self,
        search_option: SearchOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.search_options.contains(search_option) {
            self.toggle_search_option(search_option, window, cx)
        }
    }

    pub fn set_search_within_selection(
        &mut self,
        search_within_selection: Option<FilteredSearchRange>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        let active_item = self.active_searchable_item.as_mut()?;
        self.selection_search_enabled = search_within_selection;
        active_item.toggle_filtered_search_ranges(self.selection_search_enabled, window, cx);
        cx.notify();
        Some(self.update_matches(false, false, window, cx))
    }

    pub fn set_search_options(&mut self, search_options: SearchOptions, cx: &mut Context<Self>) {
        self.search_options = search_options;
        self.adjust_query_regex_language(cx);
        self.sync_select_next_case_sensitivity(cx);
        cx.notify();
    }

    pub fn clear_search_within_ranges(
        &mut self,
        search_options: SearchOptions,
        cx: &mut Context<Self>,
    ) {
        self.search_options = search_options;
        self.adjust_query_regex_language(cx);
        cx.notify();
    }

    pub(super) fn select_next_match(
        &mut self,
        _: &SelectNextMatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_match(Direction::Next, 1, window, cx);
    }

    pub(super) fn select_prev_match(
        &mut self,
        _: &SelectPreviousMatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_match(Direction::Prev, 1, window, cx);
    }

    pub fn select_all_matches(
        &mut self,
        _: &SelectAllMatches,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.dismissed
            && self.active_match_index.is_some()
            && let Some(searchable_item) = self.active_searchable_item.as_ref()
            && let Some((matches, token)) = self
                .searchable_items_with_matches
                .get(&searchable_item.downgrade())
        {
            searchable_item.select_matches(matches, *token, window, cx);
            self.focus_editor(&FocusEditor, window, cx);
        }
    }

    pub fn select_match(
        &mut self,
        direction: Direction,
        count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(target_os = "macos")]
        if let Some((query, options)) = self.pending_external_query.take() {
            let search_rx = self.search(&query, Some(options), true, window, cx);
            cx.spawn_in(window, async move |this, cx| {
                if search_rx.await.is_ok() {
                    this.update_in(cx, |this, window, cx| {
                        this.activate_current_match(window, cx);
                    })
                    .ok();
                }
            })
            .detach();

            return;
        }

        if let Some(index) = self.active_match_index
            && let Some(searchable_item) = self.active_searchable_item.as_ref()
            && let Some((matches, token)) = self
                .searchable_items_with_matches
                .get(&searchable_item.downgrade())
                .filter(|(matches, _)| !matches.is_empty())
        {
            // If 'wrapscan' is disabled, searches do not wrap around the end of the file.
            if !EditorSettings::get_global(cx).search_wrap
                && ((direction == Direction::Next && index + count >= matches.len())
                    || (direction == Direction::Prev && index < count))
            {
                crate::show_no_more_matches(window, cx);
                return;
            }
            let new_match_index = searchable_item
                .match_index_for_direction(matches, index, direction, count, *token, window, cx);
            self.active_match_index = Some(new_match_index);

            searchable_item.update_matches(matches, Some(new_match_index), *token, window, cx);
            searchable_item.activate_match(new_match_index, matches, *token, window, cx);
        }
    }

    pub fn select_first_match(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(searchable_item) = self.active_searchable_item.as_ref()
            && let Some((matches, token)) = self
                .searchable_items_with_matches
                .get(&searchable_item.downgrade())
        {
            if matches.is_empty() {
                return;
            }
            searchable_item.update_matches(matches, Some(0), *token, window, cx);
            searchable_item.activate_match(0, matches, *token, window, cx);
        }
    }

    pub fn select_last_match(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(searchable_item) = self.active_searchable_item.as_ref()
            && let Some((matches, token)) = self
                .searchable_items_with_matches
                .get(&searchable_item.downgrade())
        {
            if matches.is_empty() {
                return;
            }
            let new_match_index = matches.len() - 1;
            searchable_item.update_matches(matches, Some(new_match_index), *token, window, cx);
            searchable_item.activate_match(new_match_index, matches, *token, window, cx);
        }
    }
}
