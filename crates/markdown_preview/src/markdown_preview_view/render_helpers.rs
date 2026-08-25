use super::*;

impl MarkdownPreviewView {
    /// Returns the theme chosen in `markdown_preview_theme`, or `None` if the
    /// user hasn't set one or it can't be resolved.
    pub(super) fn resolve_preview_theme(&self, cx: &App) -> Option<Arc<Theme>> {
        let theme_settings = ThemeSettings::get_global(cx);
        let theme_selection = theme_settings.markdown_preview_theme.as_ref()?;
        let theme_name = theme_selection.name(SystemAppearance::global(cx).0);
        ThemeRegistry::global(cx).get(&theme_name.0).ok()
    }

    pub(super) fn render_markdown_element(
        &self,
        preview_theme: &Option<Arc<Theme>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> MarkdownElement {
        let active_editor = self
            .active_editor
            .as_ref()
            .map(|state| state.editor.clone());

        let mut workspace_directory = None;
        if let Some(workspace_entity) = self.workspace.upgrade() {
            let project = workspace_entity.read(cx).project();
            if let Some(tree) = project.read(cx).worktrees(cx).next() {
                workspace_directory = Some(tree.read(cx).abs_path().to_path_buf());
            }
        }

        let markdown_style = if let Some(theme) = preview_theme {
            MarkdownStyle::themed_with_overrides(
                MarkdownFont::Preview,
                theme.colors(),
                theme.syntax(),
                window,
                cx,
            )
        } else {
            MarkdownStyle::themed(MarkdownFont::Preview, window, cx)
        };

        let mut markdown_element = MarkdownElement::new(self.markdown.clone(), markdown_style)
            .code_block_renderer(CodeBlockRenderer::Default {
                copy_button_visibility: CopyButtonVisibility::VisibleOnHover,
                wrap_button_visibility: markdown::WrapButtonVisibility::Hidden,
                border: false,
            })
            .scroll_handle(self.scroll_handle.clone())
            .show_root_block_markers()
            .image_resolver({
                let base_directory = self.base_directory.clone();
                move |dest_url| {
                    resolve_preview_image(
                        dest_url,
                        base_directory.as_deref(),
                        workspace_directory.as_deref(),
                    )
                }
            })
            .on_url_click({
                let view_handle = cx.entity().downgrade();
                let workspace = self.workspace.clone();
                let base_directory = self.base_directory.clone();
                move |url, window, cx| {
                    handle_url_click(
                        url,
                        &view_handle,
                        base_directory.clone(),
                        &workspace,
                        window,
                        cx,
                    );
                }
            });

        if let Some(active_editor) = active_editor {
            let editor_for_checkbox = active_editor.clone();
            let view_handle = cx.entity().downgrade();
            markdown_element = markdown_element
                .on_source_click(move |source_index, click_count, window, cx| {
                    if click_count == 2 {
                        Self::move_cursor_to_source_index(&active_editor, source_index, window, cx);
                        true
                    } else {
                        false
                    }
                })
                .on_checkbox_toggle(move |source_range, new_checked, window, cx| {
                    Self::apply_checkbox_toggle_to_editor(
                        &editor_for_checkbox,
                        source_range,
                        new_checked,
                        cx,
                    );
                    Self::refresh_preview(view_handle.clone(), window, cx);
                });
        }

        markdown_element
    }

    pub(super) fn apply_checkbox_toggle_to_editor(
        editor: &Entity<Editor>,
        source_range: std::ops::Range<usize>,
        new_checked: bool,
        cx: &mut App,
    ) {
        let task_marker = if new_checked { "[x]" } else { "[ ]" };
        let expected_existing_marker = if new_checked { "[ ]" } else { "[x]" };

        editor.update(cx, |editor, cx| {
            let existing_marker: String = editor
                .buffer()
                .read(cx)
                .snapshot(cx)
                .text_for_range(
                    MultiBufferOffset(source_range.start)..MultiBufferOffset(source_range.end),
                )
                .collect();

            debug_assert_eq!(existing_marker, expected_existing_marker);

            editor.edit(
                [(
                    MultiBufferOffset(source_range.start)..MultiBufferOffset(source_range.end),
                    task_marker,
                )],
                cx,
            );
        });
    }

    pub(super) fn refresh_preview(
        view_handle: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(view) = view_handle.upgrade() {
            let preview_is_focused = view.read(cx).focus_handle.contains_focused(window, cx);
            if !preview_is_focused {
                return;
            }

            cx.update_entity(&view, |this, cx| {
                this.update_markdown_from_active_editor(false, false, window, cx);
            });
        }
    }
}
