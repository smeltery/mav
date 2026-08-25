use super::*;

impl BufferSearchBar {
    pub(super) fn update_matches(
        &mut self,
        reuse_existing_query: bool,
        add_to_history: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<()> {
        let (done_tx, done_rx) = oneshot::channel();
        let query = self.query(cx);
        self.pending_search.take();
        #[cfg(target_os = "macos")]
        self.pending_external_query.take();

        if let Some(active_searchable_item) = self.active_searchable_item.as_ref() {
            self.query_error = None;
            if query.is_empty() {
                self.clear_active_searchable_item_matches(window, cx);
                let _ = done_tx.send(());
                cx.notify();
            } else {
                let query: Arc<_> = if let Some(search) =
                    self.active_search.take().filter(|_| reuse_existing_query)
                {
                    search
                } else {
                    // Value doesn't matter, we only construct empty matchers with it

                    if self.search_options.contains(SearchOptions::REGEX) {
                        match SearchQuery::regex(
                            query,
                            self.search_options.contains(SearchOptions::WHOLE_WORD),
                            self.search_options.contains(SearchOptions::CASE_SENSITIVE),
                            false,
                            self.search_options
                                .contains(SearchOptions::ONE_MATCH_PER_LINE),
                            PathMatcher::default(),
                            PathMatcher::default(),
                            false,
                            None,
                        ) {
                            Ok(query) => query.with_replacement(self.replacement(cx)),
                            Err(e) => {
                                self.query_error = Some(e.to_string());
                                self.clear_active_searchable_item_matches(window, cx);
                                cx.notify();
                                return done_rx;
                            }
                        }
                    } else {
                        match SearchQuery::text(
                            query,
                            self.search_options.contains(SearchOptions::WHOLE_WORD),
                            self.search_options.contains(SearchOptions::CASE_SENSITIVE),
                            false,
                            PathMatcher::default(),
                            PathMatcher::default(),
                            false,
                            None,
                        ) {
                            Ok(query) => query.with_replacement(self.replacement(cx)),
                            Err(e) => {
                                self.query_error = Some(e.to_string());
                                self.clear_active_searchable_item_matches(window, cx);
                                cx.notify();
                                return done_rx;
                            }
                        }
                    }
                    .into()
                };

                self.active_search = Some(query.clone());
                let query_text = query.as_str().to_string();

                let matches_with_token =
                    active_searchable_item.find_matches_with_token(query, window, cx);

                let active_searchable_item = active_searchable_item.downgrade();
                self.pending_search = Some(cx.spawn_in(window, async move |this, cx| {
                    let (matches, token) = matches_with_token.await;

                    this.update_in(cx, |this, window, cx| {
                        if let Some(active_searchable_item) =
                            WeakSearchableItemHandle::upgrade(active_searchable_item.as_ref(), cx)
                        {
                            this.searchable_items_with_matches
                                .insert(active_searchable_item.downgrade(), (matches, token));

                            this.update_match_index(window, cx);

                            if add_to_history {
                                this.search_history
                                    .add(&mut this.search_history_cursor, query_text);
                            }
                            if !this.dismissed {
                                let (matches, token) = this
                                    .searchable_items_with_matches
                                    .get(&active_searchable_item.downgrade())
                                    .unwrap();
                                if matches.is_empty() {
                                    active_searchable_item.clear_matches(window, cx);
                                } else {
                                    active_searchable_item.update_matches(
                                        matches,
                                        this.active_match_index,
                                        *token,
                                        window,
                                        cx,
                                    );
                                }
                            }
                            let _ = done_tx.send(());
                            cx.notify();
                        }
                    })
                    .log_err();
                }));
            }
        }
        done_rx
    }

    pub(super) fn reverse_direction_if_backwards(&self, direction: Direction) -> Direction {
        if self.search_options.contains(SearchOptions::BACKWARDS) {
            direction.opposite()
        } else {
            direction
        }
    }

    pub fn update_match_index(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let direction = self.reverse_direction_if_backwards(Direction::Next);
        let new_index = self
            .active_searchable_item
            .as_ref()
            .and_then(|searchable_item| {
                let (matches, token) = self
                    .searchable_items_with_matches
                    .get(&searchable_item.downgrade())?;
                searchable_item.active_match_index(direction, matches, *token, window, cx)
            });
        if new_index != self.active_match_index {
            self.active_match_index = new_index;
            if !self.dismissed {
                if let Some(searchable_item) = self.active_searchable_item.as_ref() {
                    if let Some((matches, token)) = self
                        .searchable_items_with_matches
                        .get(&searchable_item.downgrade())
                    {
                        if !matches.is_empty() {
                            searchable_item.update_matches(matches, new_index, *token, window, cx);
                        }
                    }
                }
            }
            cx.notify();
        }
    }
}
