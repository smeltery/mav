use super::*;

pub(crate) fn contains_uppercase(str: &str) -> bool {
    str.chars().any(|c| c.is_uppercase())
}

impl ProjectSearch {
    pub fn new(project: Entity<Project>, cx: &mut Context<Self>) -> Self {
        let capability = project.read(cx).capability();
        let excerpts = cx.new(|_| MultiBuffer::new(capability));
        let subscription = Self::subscribe_to_excerpts(&excerpts, cx);

        Self {
            project,
            excerpts,
            pending_search: Default::default(),
            match_ranges: Default::default(),
            active_query: None,
            last_search_query_text: None,
            search_id: 0,
            search_state: SearchState::Idle,
            search_history_cursor: Default::default(),
            search_included_history_cursor: Default::default(),
            search_excluded_history_cursor: Default::default(),
            project_search_turning_into_text_finder: Arc::new(AtomicBool::new(false)),
            _excerpts_subscription: subscription,
        }
    }

    pub(super) fn clone(&self, cx: &mut Context<Self>) -> Entity<Self> {
        cx.new(|cx| {
            let excerpts = self
                .excerpts
                .update(cx, |excerpts, cx| cx.new(|cx| excerpts.clone(cx)));
            let subscription = Self::subscribe_to_excerpts(&excerpts, cx);

            Self {
                project: self.project.clone(),
                excerpts,
                pending_search: Default::default(),
                match_ranges: self.match_ranges.clone(),
                active_query: self.active_query.clone(),
                last_search_query_text: self.last_search_query_text.clone(),
                search_id: self.search_id,
                search_state: if self.pending_search.is_some() {
                    SearchState::Idle
                } else {
                    self.search_state
                },
                search_history_cursor: self.search_history_cursor.clone(),
                search_included_history_cursor: self.search_included_history_cursor.clone(),
                search_excluded_history_cursor: self.search_excluded_history_cursor.clone(),
                project_search_turning_into_text_finder: Arc::new(AtomicBool::new(false)),
                _excerpts_subscription: subscription,
            }
        })
    }
    fn subscribe_to_excerpts(
        excerpts: &Entity<MultiBuffer>,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe(excerpts, |this, _, event, cx| {
            if matches!(event, multi_buffer::Event::FileHandleChanged) {
                this.remove_deleted_buffers(cx);
            }
        })
    }

    fn remove_deleted_buffers(&mut self, cx: &mut Context<Self>) {
        let deleted_buffer_ids = self
            .excerpts
            .read(cx)
            .all_buffers_iter()
            .filter(|buffer| {
                buffer
                    .read(cx)
                    .file()
                    .is_some_and(|file| file.disk_state().is_deleted())
            })
            .map(|buffer| buffer.read(cx).remote_id())
            .collect::<Vec<_>>();

        if deleted_buffer_ids.is_empty() {
            return;
        }

        let snapshot = self.excerpts.update(cx, |excerpts, cx| {
            for buffer_id in deleted_buffer_ids {
                excerpts.remove_excerpts_for_buffer(buffer_id, cx);
            }
            excerpts.snapshot(cx)
        });

        self.match_ranges
            .retain(|range| snapshot.anchor_to_buffer_anchor(range.start).is_some());

        cx.notify();
    }

    pub(super) fn cursor(&self, kind: SearchInputKind) -> &SearchHistoryCursor {
        match kind {
            SearchInputKind::Query => &self.search_history_cursor,
            SearchInputKind::Include => &self.search_included_history_cursor,
            SearchInputKind::Exclude => &self.search_excluded_history_cursor,
        }
    }
    pub(super) fn cursor_mut(&mut self, kind: SearchInputKind) -> &mut SearchHistoryCursor {
        match kind {
            SearchInputKind::Query => &mut self.search_history_cursor,
            SearchInputKind::Include => &mut self.search_included_history_cursor,
            SearchInputKind::Exclude => &mut self.search_excluded_history_cursor,
        }
    }

    pub(super) fn search(&mut self, query: SearchQuery, cx: &mut Context<Self>) {
        let project_search_turning_into_text_finder =
            Arc::clone(&self.project_search_turning_into_text_finder);
        let search = self.project.update(cx, |project, cx| {
            project
                .search_history_mut(SearchInputKind::Query)
                .add(&mut self.search_history_cursor, query.as_str().to_string());
            let included = query.as_inner().files_to_include().sources().join(",");
            if !included.is_empty() {
                project
                    .search_history_mut(SearchInputKind::Include)
                    .add(&mut self.search_included_history_cursor, included);
            }
            let excluded = query.as_inner().files_to_exclude().sources().join(",");
            if !excluded.is_empty() {
                project
                    .search_history_mut(SearchInputKind::Exclude)
                    .add(&mut self.search_excluded_history_cursor, excluded);
            }
            project.search(query.clone(), cx)
        });
        self.last_search_query_text = Some(query.as_str().to_string());
        self.search_id += 1;
        self.active_query = Some(query);
        self.match_ranges.clear();
        self.search_state = SearchState::Running(SearchActivity::Searching);
        self.pending_search = Some(cx.spawn(async move |project_search, cx| {
            project_search
                .update(cx, |project_search, cx| {
                    project_search.match_ranges.clear();
                    project_search
                        .excerpts
                        .update(cx, |excerpts, cx| excerpts.clear(cx));
                })
                .ok()?;

            consume_search_stream(
                project_search,
                search,
                project_search_turning_into_text_finder,
                cx,
            )
            .await
        }));
        cx.notify();
    }

    // At the point this is called the multibuffer has already been filled with
    // plundered results from the text finder
    pub(crate) fn hook_up_ongoing_search(
        &mut self,
        search_results: SearchResults<SearchResult>,
        cx: &mut Context<Self>,
    ) {
        let project_search_turning_into_text_finder =
            Arc::clone(&self.project_search_turning_into_text_finder);

        self.pending_search = Some(cx.spawn(async move |project_search, cx| {
            consume_search_stream(
                project_search,
                search_results,
                project_search_turning_into_text_finder,
                cx,
            )
            .await
        }));
        cx.notify();
    }
}

/// Drain a search result stream into the project search's multibuffer.
async fn consume_search_stream(
    project_search: WeakEntity<ProjectSearch>,
    search_results: SearchResults<SearchResult>,
    project_search_turning_into_text_finder: Arc<AtomicBool>,
    cx: &mut AsyncApp,
) -> Option<SearchResults<SearchResult>> {
    // Note: is cancel safe
    let mut matches = pin!(search_results.rx.clone().ready_chunks(1024));

    let mut limit_reached = false;
    while let Some(results) = matches.next().await {
        let (buffers_with_ranges, has_reached_limit, search_activity) = cx
            .background_executor()
            .spawn(async move {
                let mut limit_reached = false;
                let mut search_activity = None;
                let mut buffers_with_ranges = Vec::with_capacity(results.len());
                for result in results {
                    match result {
                        project::search::SearchResult::Buffer { buffer, ranges } => {
                            buffers_with_ranges.push((buffer, ranges));
                        }
                        project::search::SearchResult::LimitReached => {
                            limit_reached = true;
                        }
                        project::search::SearchResult::WaitingForScan => {
                            search_activity = Some(SearchActivity::WaitingForScan);
                        }
                        project::search::SearchResult::Searching => {
                            search_activity = Some(SearchActivity::Searching);
                        }
                    }
                }
                (buffers_with_ranges, limit_reached, search_activity)
            })
            .await;
        limit_reached |= has_reached_limit;
        if let Some(search_activity) = search_activity {
            project_search
                .update(cx, |project_search, cx| {
                    project_search.search_state = SearchState::Running(search_activity);
                    cx.notify();
                })
                .ok()?;
        }
        let mut new_ranges = project_search
            .update(cx, |project_search, cx| {
                project_search.excerpts.update(cx, |excerpts, cx| {
                    buffers_with_ranges
                        .into_iter()
                        .map(|(buffer, ranges)| {
                            excerpts.set_anchored_excerpts_for_path(
                                PathKey::for_buffer(&buffer, cx),
                                buffer,
                                ranges,
                                multibuffer_context_lines(cx),
                                cx,
                            )
                        })
                        .collect::<FuturesOrdered<_>>()
                })
            })
            .ok()?;
        while let Some(new_ranges) = new_ranges.next().await {
            // `new_ranges.next().await` likely never gets hit while still pending so `async_task`
            // will not reschedule, starving other front end tasks, insert a yield point for that here
            smol::future::yield_now().await;
            project_search
                .update(cx, |project_search, cx| {
                    project_search.match_ranges.extend(new_ranges);
                    cx.notify();
                })
                .ok()?;
        }

        // We do not want to end the task before all the results taken
        // from the mpsc rx are in
        if project_search_turning_into_text_finder.load(Ordering::Relaxed) {
            break;
        }
    }

    if project_search_turning_into_text_finder.load(Ordering::Relaxed) {
        project_search_turning_into_text_finder.store(false, Ordering::Relaxed); // reset
        return Some(search_results);
    }

    project_search
        .update(cx, |project_search, cx| {
            project_search.search_state = if project_search.match_ranges.is_empty() {
                SearchState::Completed(SearchCompletion::NoResults)
            } else {
                SearchState::Completed(SearchCompletion::Results { limit_reached })
            };
            project_search.pending_search.take();
            cx.notify();
        })
        .ok()?;

    None
}
