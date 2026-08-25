use super::*;

impl MarkdownPreviewView {
    pub(super) fn line_scroll_amount(&self, cx: &App) -> Pixels {
        let settings = ThemeSettings::get_global(cx);
        settings.markdown_preview_font_size(cx) * settings.buffer_line_height.value()
    }

    pub(super) fn increase_font_size(
        &mut self,
        action: &IncreaseBufferFontSize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.adjust_font_size(action.persist, px(1.0), cx);
    }

    pub(super) fn decrease_font_size(
        &mut self,
        action: &DecreaseBufferFontSize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.adjust_font_size(action.persist, px(-1.0), cx);
    }

    pub(super) fn adjust_font_size(
        &mut self,
        persist: bool,
        delta: Pixels,
        cx: &mut Context<Self>,
    ) {
        if persist {
            let Ok(fs) = self
                .workspace
                .read_with(cx, |workspace, _| workspace.app_state().fs.clone())
            else {
                return;
            };
            update_settings_file(fs, cx, move |settings, cx| {
                let size = ThemeSettings::get_global(cx).markdown_preview_font_size(cx) + delta;
                settings.theme.markdown_preview_font_size =
                    Some(f32::from(theme_settings::clamp_font_size(size)).into());
            });
        } else {
            theme_settings::adjust_markdown_preview_font_size(cx, |size| size + delta);
        }
    }

    pub(super) fn reset_font_size(
        &mut self,
        action: &ResetBufferFontSize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action.persist {
            let Ok(fs) = self
                .workspace
                .read_with(cx, |workspace, _| workspace.app_state().fs.clone())
            else {
                return;
            };
            update_settings_file(fs, cx, move |settings, _| {
                settings.theme.markdown_preview_font_size = None;
            });
        } else {
            theme_settings::reset_markdown_preview_font_size(cx);
        }
    }

    pub(super) fn scroll_by_amount(&self, distance: Pixels) {
        let offset = self.scroll_handle.offset();
        self.scroll_handle
            .set_offset(point(offset.x, offset.y - distance));
    }

    pub(super) fn scroll_page_up(
        &mut self,
        _: &ScrollPageUp,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let viewport_height = self.scroll_handle.bounds().size.height;
        if viewport_height.is_zero() {
            return;
        }

        self.scroll_by_amount(-viewport_height);
        cx.notify();
    }

    pub(super) fn scroll_page_down(
        &mut self,
        _: &ScrollPageDown,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let viewport_height = self.scroll_handle.bounds().size.height;
        if viewport_height.is_zero() {
            return;
        }

        self.scroll_by_amount(viewport_height);
        cx.notify();
    }

    pub(super) fn scroll_up(&mut self, _: &ScrollUp, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(bounds) = self
            .scroll_handle
            .bounds_for_item(self.scroll_handle.top_item())
        {
            let item_height = bounds.size.height;
            // Scroll no more than the rough equivalent of a large headline
            let max_height = window.rem_size() * 2;
            let scroll_height = min(item_height, max_height);
            self.scroll_by_amount(-scroll_height);
        } else {
            let scroll_height = self.line_scroll_amount(cx);
            if !scroll_height.is_zero() {
                self.scroll_by_amount(-scroll_height);
            }
        }
        cx.notify();
    }

    pub(super) fn scroll_down(
        &mut self,
        _: &ScrollDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(bounds) = self
            .scroll_handle
            .bounds_for_item(self.scroll_handle.top_item())
        {
            let item_height = bounds.size.height;
            // Scroll no more than the rough equivalent of a large headline
            let max_height = window.rem_size() * 2;
            let scroll_height = min(item_height, max_height);
            self.scroll_by_amount(scroll_height);
        } else {
            let scroll_height = self.line_scroll_amount(cx);
            if !scroll_height.is_zero() {
                self.scroll_by_amount(scroll_height);
            }
        }
        cx.notify();
    }

    pub(super) fn scroll_up_by_item(
        &mut self,
        _: &ScrollUpByItem,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(bounds) = self
            .scroll_handle
            .bounds_for_item(self.scroll_handle.top_item())
        {
            self.scroll_by_amount(-bounds.size.height);
        }
        cx.notify();
    }

    pub(super) fn scroll_down_by_item(
        &mut self,
        _: &ScrollDownByItem,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(bounds) = self
            .scroll_handle
            .bounds_for_item(self.scroll_handle.top_item())
        {
            self.scroll_by_amount(bounds.size.height);
        }
        cx.notify();
    }

    pub(super) fn scroll_to_top(
        &mut self,
        _: &ScrollToTop,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scroll_handle.scroll_to_item(0);
        cx.notify();
    }

    pub(super) fn scroll_to_bottom(
        &mut self,
        _: &ScrollToBottom,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scroll_handle.scroll_to_bottom();
        cx.notify();
    }
}
