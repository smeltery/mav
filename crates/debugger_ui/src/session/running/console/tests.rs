use super::*;
use crate::tests::init_test;
use editor::{MultiBufferOffset, test::editor_test_context::EditorTestContext};
use gpui::TestAppContext;
use language::Point;

#[track_caller]
fn assert_completion_range(
    input: &str,
    expect: &str,
    replacement: &str,
    cx: &mut EditorTestContext,
) {
    cx.set_state(input);

    let buffer_position = cx.editor(|editor, _, cx| {
        editor
            .selections
            .newest::<Point>(&editor.display_snapshot(cx))
            .start
    });

    let snapshot = &cx.buffer_snapshot();

    let replace_range = ConsoleQueryBarCompletionProvider::replace_range_for_completion(
        &cx.buffer_text(),
        snapshot.anchor_before(buffer_position),
        replacement.as_bytes(),
        snapshot,
    );

    cx.update_editor(|editor, _, cx| {
        editor.edit(
            vec![(
                MultiBufferOffset(snapshot.offset_for_anchor(&replace_range.start))
                    ..MultiBufferOffset(snapshot.offset_for_anchor(&replace_range.end)),
                replacement,
            )],
            cx,
        );
    });

    pretty_assertions::assert_eq!(expect, cx.display_text());
}

#[gpui::test]
fn test_background_color_fetcher_preserves_default_background(cx: &mut TestAppContext) {
    init_test(cx);

    cx.update(|cx| {
        let mut theme = theme::GlobalTheme::theme(cx).as_ref().clone();
        theme.styles.colors.terminal_background = gpui::red();
        theme.styles.colors.terminal_ansi_background = gpui::blue();

        let color = background_color_fetcher(terminal::Color::Named(
            terminal::NamedColor::Background,
        ))(&theme);

        assert_eq!(color, gpui::red());
    });
}

#[gpui::test]
async fn test_determine_completion_replace_range(cx: &mut TestAppContext) {
    init_test(cx);

    let mut cx = EditorTestContext::new(cx).await;

    assert_completion_range("resˇ", "result", "result", &mut cx);
    assert_completion_range("print(resˇ)", "print(result)", "result", &mut cx);
    assert_completion_range("$author->nˇ", "$author->name", "$author->name", &mut cx);
    assert_completion_range(
        "$author->books[ˇ",
        "$author->books[0]",
        "$author->books[0]",
        &mut cx,
    );
}
