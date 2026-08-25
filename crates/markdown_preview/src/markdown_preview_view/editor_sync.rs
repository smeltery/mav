use super::*;

impl MarkdownPreviewView {
    pub fn new(
        mode: MarkdownPreviewMode,
        active_editor: Entity<Editor>,
        workspace: WeakEntity<Workspace>,
        language_registry: Arc<LanguageRegistry>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            let markdown = cx.new(|cx| {
                Markdown::new_with_options(
                    SharedString::default(),
                    Some(language_registry),
                    None,
                    MarkdownOptions {
                        parse_html: true,
                        render_mermaid_diagrams: true,
                        parse_heading_slugs: true,
                        render_metadata_blocks: true,
                        ..Default::default()
                    },
                    cx,
                )
            });
            let mut this = Self {
                active_editor: None,
                focus_handle: cx.focus_handle(),
                workspace: workspace.clone(),
                _markdown_subscription: cx.observe(
                    &markdown,
                    |this: &mut Self, _: Entity<Markdown>, cx| {
                        this.sync_active_root_block(cx);
                    },
                ),
                markdown,
                active_source_index: None,
                scroll_handle: ScrollHandle::new(),
                image_cache: RetainAllImageCache::new(cx),
                base_directory: None,
                pending_update_task: None,
                mode,
            };

            this.set_editor(active_editor, window, cx);

            match mode {
                MarkdownPreviewMode::Follow => {
                    if let Some(workspace) = &workspace.upgrade() {
                        cx.observe_in(workspace, window, |this, workspace, window, cx| {
                            let item = workspace.read(cx).active_item(cx);
                            this.workspace_updated(item, window, cx);
                        })
                        .detach();
                    } else {
                        log::error!("Failed to listen to workspace updates");
                    }
                }
                MarkdownPreviewMode::Default => {
                    // After workspace restoration the bound editor may be an orphan that
                    // wraps the right buffer but isn't the canonical Editor instance in
                    // any pane. Re-binding to the workspace's editor for our buffer is
                    // what restores cursor-driven scroll sync — `SelectionsChanged` only
                    // fires from the editor the user actually interacts with.
                    //
                    // Subscribing to `workspace::Event` (rather than `observe`) keeps the
                    // rebind check off the cursor-move hot path; `observe` would fire on
                    // every workspace `cx.notify`.
                    if let Some(workspace) = &workspace.upgrade() {
                        cx.subscribe_in(workspace, window, Self::on_workspace_event)
                            .detach();
                    }
                }
            }

            this
        })
    }

    pub(super) fn workspace_updated(
        &mut self,
        active_item: Option<Box<dyn ItemHandle>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(item) = active_item
            && item.item_id() != cx.entity_id()
            && let Some(editor) = item.act_as::<Editor>(cx)
            && Self::is_markdown_file(&editor, cx)
        {
            self.set_editor(editor, window, cx);
        }
    }

    pub fn is_markdown_path(path: impl AsRef<Path>) -> bool {
        path.as_ref().extension().is_some_and(|ext| {
            ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
        })
    }

    pub fn open_for_project_path(
        project_path: ProjectPath,
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let open_buffer = workspace
            .project()
            .update(cx, |project, cx| project.open_buffer(project_path, cx));

        cx.spawn_in(window, async move |workspace, mut cx| {
            let Some(buffer) = open_buffer
                .await
                .notify_workspace_async_err(workspace.clone(), &mut cx)
            else {
                return;
            };
            workspace
                .update_in(cx, |workspace, window, cx| {
                    let project = workspace.project().clone();
                    let editor = cx.new(|cx| Editor::for_buffer(buffer, Some(project), window, cx));
                    let preview = Self::create_markdown_view(workspace, editor, window, cx);
                    workspace.active_pane().update(cx, |pane, cx| {
                        pane.add_item(Box::new(preview), true, true, None, window, cx);
                    });
                })
                .ok();
        })
        .detach();
    }

    pub fn is_markdown_file<V>(editor: &Entity<Editor>, cx: &mut Context<V>) -> bool {
        let buffer = editor.read(cx).buffer().read(cx);
        if let Some(buffer) = buffer.as_singleton()
            && let Some(language) = buffer.read(cx).language()
        {
            return language.name() == "Markdown";
        }
        false
    }

    pub(super) fn set_editor(
        &mut self,
        editor: Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(active) = &self.active_editor
            && active.editor == editor
        {
            return;
        }

        let had_active_editor = self.active_editor.is_some();
        let subscription = cx.subscribe_in(
            &editor,
            window,
            |this, editor, event: &EditorEvent, window, cx| {
                match event {
                    EditorEvent::Edited { .. }
                    | EditorEvent::BufferEdited { .. }
                    | EditorEvent::DirtyChanged
                    | EditorEvent::BuffersEdited { .. } => {
                        this.update_markdown_from_active_editor(true, false, window, cx);
                    }
                    EditorEvent::FileHandleChanged => {
                        this.base_directory =
                            Self::get_folder_for_active_editor(editor.read(cx), cx);
                        this.update_markdown_from_active_editor(false, false, window, cx);
                        cx.emit(MarkdownPreviewEvent::SourceFileHandleChanged);
                    }
                    EditorEvent::SelectionsChanged { .. } => {
                        let (selection_start, editor_is_focused) =
                            editor.update(cx, |editor, cx| {
                                let index = Self::selected_source_index(editor, cx);
                                let focused = editor.focus_handle(cx).is_focused(window);
                                (index, focused)
                            });
                        if let Some(selection_start) = selection_start {
                            this.sync_preview_to_source_index(
                                selection_start,
                                editor_is_focused,
                                cx,
                            );
                            cx.notify();
                        }
                    }
                    _ => {}
                };
            },
        );

        self.base_directory = Self::get_folder_for_active_editor(editor.read(cx), cx);
        self.active_editor = Some(EditorState {
            editor,
            _subscription: subscription,
        });
        self.update_markdown_from_active_editor(false, true, window, cx);
        if had_active_editor {
            cx.emit(MarkdownPreviewEvent::SourceEditorChanged);
        }
    }

    pub(super) fn on_workspace_event(
        &mut self,
        workspace: &Entity<Workspace>,
        event: &workspace::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(
            event,
            workspace::Event::ItemAdded { .. } | workspace::Event::ItemRemoved { .. }
        ) {
            return;
        }
        let candidate = self.find_canonical_editor(workspace.read(cx), cx);
        if let Some(editor) = candidate
            && self
                .active_editor
                .as_ref()
                .is_none_or(|s| s.editor != editor)
        {
            self.set_editor(editor, window, cx);
        }
    }

    pub(super) fn find_canonical_editor(
        &self,
        workspace: &Workspace,
        cx: &App,
    ) -> Option<Entity<Editor>> {
        let current = self.active_editor.as_ref()?.editor.clone();
        let our_buffer = current.read(cx).buffer().read(cx).as_singleton()?;
        let mut fallback = None;
        for editor in workspace.items_of_type::<Editor>(cx) {
            if editor.read(cx).buffer().read(cx).as_singleton().as_ref() != Some(&our_buffer) {
                continue;
            }
            if editor == current {
                return Some(current);
            }
            fallback.get_or_insert(editor);
        }
        fallback
    }

    pub(super) fn update_markdown_from_active_editor(
        &mut self,
        wait_for_debounce: bool,
        should_reveal: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = &self.active_editor {
            // if there is already a task to update the ui and the current task is also debounced (not high priority), do nothing
            if wait_for_debounce && self.pending_update_task.is_some() {
                return;
            }
            self.pending_update_task = Some(self.schedule_markdown_update(
                wait_for_debounce,
                should_reveal,
                state.editor.clone(),
                window,
                cx,
            ));
        }
    }

    pub(super) fn schedule_markdown_update(
        &mut self,
        wait_for_debounce: bool,
        should_reveal_selection: bool,
        editor: Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        cx.spawn_in(window, async move |view, cx| {
            if wait_for_debounce {
                // Wait for the user to stop typing
                cx.background_executor().timer(REPARSE_DEBOUNCE).await;
            }

            let update = view.update(cx, |view, cx| {
                let is_active_editor = view
                    .active_editor
                    .as_ref()
                    .is_some_and(|active_editor| active_editor.editor == editor);
                if !is_active_editor {
                    return None;
                }

                editor.update(cx, |editor, cx| {
                    let contents = editor
                        .buffer()
                        .read(cx)
                        .as_singleton()?
                        .read(cx)
                        .as_rope()
                        .to_string()
                        .into();
                    let selection_start = Self::selected_source_index(editor, cx)?;
                    Some((contents, selection_start))
                })
            })?;

            view.update(cx, move |view, cx| {
                if let Some((contents, selection_start)) = update {
                    view.markdown.update(cx, |markdown, cx| {
                        markdown.reset(contents, cx);
                    });
                    view.sync_preview_to_source_index(selection_start, should_reveal_selection, cx);
                    cx.emit(SearchEvent::MatchesInvalidated);
                }
                view.pending_update_task = None;
                cx.notify();
            })
        })
    }

    pub(super) fn selected_source_index(editor: &Editor, cx: &mut App) -> Option<usize> {
        let display_snapshot = editor.display_snapshot(cx);
        let source_offset = editor
            .selections
            .last::<MultiBufferOffset>(&display_snapshot)
            .range()
            .start;
        let buffer = editor.buffer().read(cx).as_singleton()?;
        let buffer_id = buffer.read(cx).remote_id();
        let (buffer_snapshot, buffer_offset) = display_snapshot
            .buffer_snapshot()
            .point_to_buffer_offset(source_offset)?;

        if buffer_snapshot.remote_id() == buffer_id {
            Some(buffer_offset.0)
        } else {
            None
        }
    }

    pub(super) fn sync_preview_to_source_index(
        &mut self,
        source_index: usize,
        reveal: bool,
        cx: &mut Context<Self>,
    ) {
        self.active_source_index = Some(source_index);
        self.sync_active_root_block(cx);
        self.markdown.update(cx, |markdown, cx| {
            if reveal {
                markdown.request_autoscroll_to_source_index(source_index, cx);
            }
        });
    }

    pub(super) fn sync_active_root_block(&mut self, cx: &mut Context<Self>) {
        self.markdown.update(cx, |markdown, cx| {
            markdown.set_active_root_for_source_index(self.active_source_index, cx);
        });
    }

    pub(super) fn move_cursor_to_source_index(
        editor: &Entity<Editor>,
        source_index: usize,
        window: &mut Window,
        cx: &mut App,
    ) {
        editor.update(cx, |editor, cx| {
            let selection = MultiBufferOffset(source_index)..MultiBufferOffset(source_index);
            editor.change_selections(
                SelectionEffects::scroll(Autoscroll::center()),
                window,
                cx,
                |selections| selections.select_ranges(vec![selection]),
            );
            window.focus(&editor.focus_handle(cx), cx);
        });
    }

    /// The absolute path of the file that is currently being previewed.
    pub(super) fn get_folder_for_active_editor(editor: &Editor, cx: &App) -> Option<PathBuf> {
        if let Some(file) = editor.file_at(MultiBufferOffset(0), cx) {
            if let Some(file) = file.as_local() {
                file.abs_path(cx).parent().map(|p| p.to_path_buf())
            } else {
                None
            }
        } else {
            None
        }
    }
}
