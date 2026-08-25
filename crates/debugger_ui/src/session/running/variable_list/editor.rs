use super::*;

impl VariableList {
    pub(super) fn create_variable_editor(
        default: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<::editor::Editor> {
        let editor = cx.new(|cx| {
            let mut editor = ::editor::Editor::single_line(window, cx);

            let refinement = TextStyleRefinement {
                font_size: Some(
                    TextSize::XSmall
                        .rems(cx)
                        .to_pixels(window.rem_size())
                        .into(),
                ),
                ..Default::default()
            };
            editor.set_text_style_refinement(refinement);
            editor.set_text(default, window, cx);
            editor.select_all(&::editor::actions::SelectAll, window, cx);
            editor
        });
        editor.focus_handle(cx).focus(window, cx);
        editor
    }
}
