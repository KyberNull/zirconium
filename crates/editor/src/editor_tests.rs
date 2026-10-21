use super::*;
use crate::{
    JoinLines,
    code_context_menus::CodeContextMenu,
    edit_prediction_tests::FakeEditPredictionDelegate,
    element::{StickyHeader, header_jump_data},
    scroll::scroll_amount::ScrollAmount,
    test::{
        assert_text_with_selections, build_editor,
        editor_language_test_context::{EditorLanguageTestContext, git_commit_lang},
        editor_test_context::EditorTestContext,
        select_ranges,
    },
};
use buffer_diff::{BufferDiff, DiffHunkSecondaryStatus, DiffHunkStatus, DiffHunkStatusKind};
use collections::HashMap;
use fs::Fs as _;
use gpui::{
    BackgroundExecutor, DismissEvent, TaskExt, TestAppContext, UpdateGlobal, VisualTestContext,
    WindowBounds, WindowOptions, div,
};
use indoc::indoc;
use language::{
    BracketPair, BracketPairConfig,
    Capability::ReadWrite,
    IndentGuideSettings, LanguageConfig, LanguageMatcher, LanguageName, LanguageQueries,
    PLAIN_TEXT, Point,
    language_settings::{CompletionSettingsContent, FormatterList, LanguageSettingsContent},
    tree_sitter_python,
};
use language_settings::Formatter;
use languages::markdown_lang;
use languages::rust_lang;
use multi_buffer::{IndentGuide, MultiBuffer, MultiBufferOffset, MultiBufferOffsetUtf16, PathKey};
use parking_lot::Mutex;
use pretty_assertions::{assert_eq, assert_ne};
use project::{FakeFs, Project, bookmark_store::SerializedBookmark};
use serde_json::{self, json};
use settings::{
    AllLanguageSettingsContent, EditorSettingsContent, IndentGuideBackgroundColoring,
    IndentGuideColoring, ScrollBeyondLastLine, SearchSettingsContent, SettingsContent,
    SettingsStore,
};
use std::{borrow::Cow, cmp::Ordering, sync::Arc};
use std::{cell::RefCell, rc::Rc, time::Instant};
use test::build_editor_with_project;
use unindent::Unindent;
use util::{
    assert_set_eq, path,
    rel_path::rel_path,
    test::{marked_text_ranges, sample_text},
};
use workspace::{
    CloseAllItems, CloseOtherItems, MultiWorkspace, NavigationEntry, OpenOptions,
    ToolbarItemLocation, ViewId,
    item::{FollowEvent, FollowableItem, Item, ItemHandle, SaveOptions},
    register_project_item,
};

fn display_ranges(editor: &Editor, cx: &mut Context<'_, Editor>) -> Vec<Range<DisplayPoint>> {
    editor
        .selections
        .display_ranges(&editor.display_snapshot(cx))
}

#[cfg(any(test, feature = "test-support"))]
pub mod property_test;

#[gpui::test]
fn test_edit_events(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let buffer = cx.new(|cx| {
        let mut buffer = language::Buffer::local("123456", cx);
        buffer.set_group_interval(Duration::from_secs(1));
        buffer
    });

    let events = Rc::new(RefCell::new(Vec::new()));
    let editor1 = cx.add_window({
        let events = events.clone();
        |window, cx| {
            let entity = cx.entity();
            cx.subscribe_in(
                &entity,
                window,
                move |_, _, event: &EditorEvent, _, _| match event {
                    EditorEvent::Edited { .. } => events.borrow_mut().push(("editor1", "edited")),
                    EditorEvent::BufferEdited => {
                        events.borrow_mut().push(("editor1", "buffer edited"))
                    }
                    _ => {}
                },
            )
            .detach();
            Editor::for_buffer(buffer.clone(), None, window, cx)
        }
    });

    let editor2 = cx.add_window({
        let events = events.clone();
        |window, cx| {
            cx.subscribe_in(
                &cx.entity(),
                window,
                move |_, _, event: &EditorEvent, _, _| match event {
                    EditorEvent::Edited { .. } => events.borrow_mut().push(("editor2", "edited")),
                    EditorEvent::BufferEdited => {
                        events.borrow_mut().push(("editor2", "buffer edited"))
                    }
                    _ => {}
                },
            )
            .detach();
            Editor::for_buffer(buffer.clone(), None, window, cx)
        }
    });

    assert_eq!(mem::take(&mut *events.borrow_mut()), []);

    // Mutating editor 1 will emit an `Edited` event only for that editor.
    _ = editor1.update(cx, |editor, window, cx| editor.insert("X", window, cx));
    assert_eq!(
        mem::take(&mut *events.borrow_mut()),
        [
            ("editor1", "edited"),
            ("editor1", "buffer edited"),
            ("editor2", "buffer edited"),
        ]
    );

    // Mutating editor 2 will emit an `Edited` event only for that editor.
    _ = editor2.update(cx, |editor, window, cx| editor.delete(&Delete, window, cx));
    assert_eq!(
        mem::take(&mut *events.borrow_mut()),
        [
            ("editor2", "edited"),
            ("editor1", "buffer edited"),
            ("editor2", "buffer edited"),
        ]
    );

    // Undoing on editor 1 will emit an `Edited` event only for that editor.
    _ = editor1.update(cx, |editor, window, cx| editor.undo(&Undo, window, cx));
    assert_eq!(
        mem::take(&mut *events.borrow_mut()),
        [
            ("editor1", "edited"),
            ("editor1", "buffer edited"),
            ("editor2", "buffer edited"),
        ]
    );

    // Redoing on editor 1 will emit an `Edited` event only for that editor.
    _ = editor1.update(cx, |editor, window, cx| editor.redo(&Redo, window, cx));
    assert_eq!(
        mem::take(&mut *events.borrow_mut()),
        [
            ("editor1", "edited"),
            ("editor1", "buffer edited"),
            ("editor2", "buffer edited"),
        ]
    );

    // Undoing on editor 2 will emit an `Edited` event only for that editor.
    _ = editor2.update(cx, |editor, window, cx| editor.undo(&Undo, window, cx));
    assert_eq!(
        mem::take(&mut *events.borrow_mut()),
        [
            ("editor2", "edited"),
            ("editor1", "buffer edited"),
            ("editor2", "buffer edited"),
        ]
    );

    // Redoing on editor 2 will emit an `Edited` event only for that editor.
    _ = editor2.update(cx, |editor, window, cx| editor.redo(&Redo, window, cx));
    assert_eq!(
        mem::take(&mut *events.borrow_mut()),
        [
            ("editor2", "edited"),
            ("editor1", "buffer edited"),
            ("editor2", "buffer edited"),
        ]
    );

    // No event is emitted when the mutation is a no-op.
    _ = editor2.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(0)..MultiBufferOffset(0)])
        });

        editor.backspace(&Backspace, window, cx);
    });
    assert_eq!(mem::take(&mut *events.borrow_mut()), []);
}

#[gpui::test]
fn test_undo_redo_with_selection_restoration(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut now = Instant::now();
    let group_interval = Duration::from_millis(1);
    let buffer = cx.new(|cx| {
        let mut buf = language::Buffer::local("123456", cx);
        buf.set_group_interval(group_interval);
        buf
    });
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let editor = cx.add_window(|window, cx| build_editor(buffer.clone(), window, cx));

    _ = editor.update(cx, |editor, window, cx| {
        editor.start_transaction_at(now, window, cx);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(2)..MultiBufferOffset(4)])
        });

        editor.insert("cd", window, cx);
        editor.end_transaction_at(now, cx);
        assert_eq!(editor.text(cx), "12cd56");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            vec![MultiBufferOffset(4)..MultiBufferOffset(4)]
        );

        editor.start_transaction_at(now, window, cx);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(4)..MultiBufferOffset(5)])
        });
        editor.insert("e", window, cx);
        editor.end_transaction_at(now, cx);
        assert_eq!(editor.text(cx), "12cde6");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            vec![MultiBufferOffset(5)..MultiBufferOffset(5)]
        );

        now += group_interval + Duration::from_millis(1);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(2)..MultiBufferOffset(2)])
        });

        // Simulate an edit in another editor
        buffer.update(cx, |buffer, cx| {
            buffer.start_transaction_at(now, cx);
            buffer.edit(
                [(MultiBufferOffset(0)..MultiBufferOffset(1), "a")],
                None,
                cx,
            );
            buffer.edit(
                [(MultiBufferOffset(1)..MultiBufferOffset(1), "b")],
                None,
                cx,
            );
            buffer.end_transaction_at(now, cx);
        });

        assert_eq!(editor.text(cx), "ab2cde6");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            vec![MultiBufferOffset(3)..MultiBufferOffset(3)]
        );

        // Last transaction happened past the group interval in a different editor.
        // Undo it individually and don't restore selections.
        editor.undo(&Undo, window, cx);
        assert_eq!(editor.text(cx), "12cde6");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            vec![MultiBufferOffset(2)..MultiBufferOffset(2)]
        );

        // First two transactions happened within the group interval in this editor.
        // Undo them together and restore selections.
        editor.undo(&Undo, window, cx);
        editor.undo(&Undo, window, cx); // Undo stack is empty here, so this is a no-op.
        assert_eq!(editor.text(cx), "123456");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            vec![MultiBufferOffset(0)..MultiBufferOffset(0)]
        );

        // Redo the first two transactions together.
        editor.redo(&Redo, window, cx);
        assert_eq!(editor.text(cx), "12cde6");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            vec![MultiBufferOffset(5)..MultiBufferOffset(5)]
        );

        // Redo the last transaction on its own.
        editor.redo(&Redo, window, cx);
        assert_eq!(editor.text(cx), "ab2cde6");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            vec![MultiBufferOffset(6)..MultiBufferOffset(6)]
        );

        // Test empty transactions.
        editor.start_transaction_at(now, window, cx);
        editor.end_transaction_at(now, cx);
        editor.undo(&Undo, window, cx);
        assert_eq!(editor.text(cx), "12cde6");
    });
}

#[gpui::test]
fn test_undo_redo_with_empty_history_selections_does_not_panic(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let now = Instant::now();
    let buffer = cx.new(|cx| language::Buffer::local("123456", cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let editor = cx.add_window(|window, cx| build_editor(buffer, window, cx));

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(0)..MultiBufferOffset(0)])
        });
        editor.start_transaction_at(now, window, cx);
        editor.insert("a", window, cx);
        let transaction_id = editor
            .end_transaction_at(now, cx)
            .expect("transaction should be created");
        assert_eq!(editor.text(cx), "a123456");

        editor.modify_transaction_selection_history(transaction_id, |selections| {
            selections.undo = Vec::new().into();
            selections.redo = Some(Vec::new().into());
        });

        editor.undo(&Undo, window, cx);
        assert_eq!(editor.text(cx), "123456");

        editor.redo(&Redo, window, cx);
        assert_eq!(editor.text(cx), "a123456");
    });
}

#[gpui::test]
fn test_accessibility_keyboard_word_completion(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    // Simulates the macOS Accessibility Keyboard word completion panel, which calls
    // insertText:replacementRange: to commit a completion. macOS sends two calls per
    // completion: one with a non-empty range replacing the typed prefix, and one with
    // an empty replacement range (cursor..cursor) to append a trailing space.

    cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("ab", cx);
        let mut editor = build_editor(buffer, window, cx);

        // Cursor is after the 2-char prefix "ab" at offset 2.
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(2)..MultiBufferOffset(2)])
        });

        // macOS completes "about" by replacing the prefix via range 0..2.
        editor.replace_text_in_range(Some(0..2), "about", window, cx);
        assert_eq!(editor.text(cx), "about");

        // macOS sends a trailing space as an empty replacement range (cursor..cursor).
        // Must insert at the cursor position, not call backspace first (which would
        // delete the preceding character).
        editor.replace_text_in_range(Some(5..5), " ", window, cx);
        assert_eq!(editor.text(cx), "about ");

        editor
    });

    // Multi-cursor: the replacement must fan out to all cursors, and the trailing
    // space must land at each cursor's actual current position. After the first
    // completion, macOS's reported cursor offset is stale (it doesn't account for
    // the offset shift caused by the other cursor's insertion), so the empty
    // replacement range must be ignored and the space inserted at each real cursor.
    cx.add_window(|window, cx| {
        // Two cursors, each after a 2-char prefix "ab" at the end of each line:
        //   "ab\nab" — cursors at offsets 2 and 5.
        let buffer = MultiBuffer::build_simple("ab\nab", cx);
        let mut editor = build_editor(buffer, window, cx);

        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([
                MultiBufferOffset(2)..MultiBufferOffset(2),
                MultiBufferOffset(5)..MultiBufferOffset(5),
            ])
        });

        // macOS reports the newest cursor (offset 5) and sends range 3..5 to
        // replace its 2-char prefix. selection_replacement_ranges applies the same
        // delta to fan out to both cursors: 0..2 and 3..5.
        editor.replace_text_in_range(Some(3..5), "about", window, cx);
        assert_eq!(editor.text(cx), "about\nabout");

        // Trailing space via empty range. macOS thinks the cursor is at offset 10
        // (5 - 2 + 7 = 10), but the actual cursors are at 5 and 11. The stale
        // offset must be ignored and the space inserted at each real cursor position.
        editor.replace_text_in_range(Some(10..10), " ", window, cx);
        assert_eq!(editor.text(cx), "about \nabout ");

        editor
    });
}

#[gpui::test]
fn test_ime_composition(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let buffer = cx.new(|cx| {
        let mut buffer = language::Buffer::local("abcde", cx);
        // Ensure automatic grouping doesn't occur.
        buffer.set_group_interval(Duration::ZERO);
        buffer
    });

    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    cx.add_window(|window, cx| {
        let mut editor = build_editor(buffer.clone(), window, cx);

        // Start a new IME composition.
        editor.replace_and_mark_text_in_range(Some(0..1), "à", None, window, cx);
        editor.replace_and_mark_text_in_range(Some(0..1), "á", None, window, cx);
        editor.replace_and_mark_text_in_range(Some(0..1), "ä", None, window, cx);
        assert_eq!(editor.text(cx), "äbcde");
        assert_eq!(
            editor.marked_text_ranges(cx),
            Some(vec![
                MultiBufferOffsetUtf16(OffsetUtf16(0))..MultiBufferOffsetUtf16(OffsetUtf16(1))
            ])
        );

        // Finalize IME composition.
        editor.replace_text_in_range(None, "ā", window, cx);
        assert_eq!(editor.text(cx), "ābcde");
        assert_eq!(editor.marked_text_ranges(cx), None);

        // IME composition edits are grouped and are undone/redone at once.
        editor.undo(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "abcde");
        assert_eq!(editor.marked_text_ranges(cx), None);
        editor.redo(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "ābcde");
        assert_eq!(editor.marked_text_ranges(cx), None);

        // Start a new IME composition.
        editor.replace_and_mark_text_in_range(Some(0..1), "à", None, window, cx);
        assert_eq!(
            editor.marked_text_ranges(cx),
            Some(vec![
                MultiBufferOffsetUtf16(OffsetUtf16(0))..MultiBufferOffsetUtf16(OffsetUtf16(1))
            ])
        );

        // Undoing during an IME composition cancels it.
        editor.undo(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "ābcde");
        assert_eq!(editor.marked_text_ranges(cx), None);

        // Start a new IME composition with an invalid marked range, ensuring it gets clipped.
        editor.replace_and_mark_text_in_range(Some(4..999), "è", None, window, cx);
        assert_eq!(editor.text(cx), "ābcdè");
        assert_eq!(
            editor.marked_text_ranges(cx),
            Some(vec![
                MultiBufferOffsetUtf16(OffsetUtf16(4))..MultiBufferOffsetUtf16(OffsetUtf16(5))
            ])
        );

        // Finalize IME composition with an invalid replacement range, ensuring it gets clipped.
        editor.replace_text_in_range(Some(4..999), "ę", window, cx);
        assert_eq!(editor.text(cx), "ābcdę");
        assert_eq!(editor.marked_text_ranges(cx), None);

        // Start a new IME composition with multiple cursors.
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([
                MultiBufferOffsetUtf16(OffsetUtf16(1))..MultiBufferOffsetUtf16(OffsetUtf16(1)),
                MultiBufferOffsetUtf16(OffsetUtf16(3))..MultiBufferOffsetUtf16(OffsetUtf16(3)),
                MultiBufferOffsetUtf16(OffsetUtf16(5))..MultiBufferOffsetUtf16(OffsetUtf16(5)),
            ])
        });
        editor.replace_and_mark_text_in_range(Some(4..5), "XYZ", None, window, cx);
        assert_eq!(editor.text(cx), "XYZbXYZdXYZ");
        assert_eq!(
            editor.marked_text_ranges(cx),
            Some(vec![
                MultiBufferOffsetUtf16(OffsetUtf16(0))..MultiBufferOffsetUtf16(OffsetUtf16(3)),
                MultiBufferOffsetUtf16(OffsetUtf16(4))..MultiBufferOffsetUtf16(OffsetUtf16(7)),
                MultiBufferOffsetUtf16(OffsetUtf16(8))..MultiBufferOffsetUtf16(OffsetUtf16(11))
            ])
        );

        // Ensure the newly-marked range gets treated as relative to the previously-marked ranges.
        editor.replace_and_mark_text_in_range(Some(1..2), "1", None, window, cx);
        assert_eq!(editor.text(cx), "X1ZbX1ZdX1Z");
        assert_eq!(
            editor.marked_text_ranges(cx),
            Some(vec![
                MultiBufferOffsetUtf16(OffsetUtf16(1))..MultiBufferOffsetUtf16(OffsetUtf16(2)),
                MultiBufferOffsetUtf16(OffsetUtf16(5))..MultiBufferOffsetUtf16(OffsetUtf16(6)),
                MultiBufferOffsetUtf16(OffsetUtf16(9))..MultiBufferOffsetUtf16(OffsetUtf16(10))
            ])
        );

        // Finalize IME composition with multiple cursors.
        editor.replace_text_in_range(Some(9..10), "2", window, cx);
        assert_eq!(editor.text(cx), "X2ZbX2ZdX2Z");
        assert_eq!(editor.marked_text_ranges(cx), None);

        editor
    });
}

#[gpui::test]
fn test_selection_with_mouse(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaaaaa\nbbbbbb\ncccccc\nddddddd\n", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.begin_selection(DisplayPoint::new(DisplayRow(2), 2), false, 1, window, cx);
    });
    assert_eq!(
        editor
            .update(cx, |editor, _, cx| display_ranges(editor, cx))
            .unwrap(),
        [DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(2), 2)]
    );

    _ = editor.update(cx, |editor, window, cx| {
        editor.update_selection(
            DisplayPoint::new(DisplayRow(3), 3),
            0,
            gpui::Point::<f32>::default(),
            window,
            cx,
        );
    });

    assert_eq!(
        editor
            .update(cx, |editor, _, cx| display_ranges(editor, cx))
            .unwrap(),
        [DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(3), 3)]
    );

    _ = editor.update(cx, |editor, window, cx| {
        editor.update_selection(
            DisplayPoint::new(DisplayRow(1), 1),
            0,
            gpui::Point::<f32>::default(),
            window,
            cx,
        );
    });

    assert_eq!(
        editor
            .update(cx, |editor, _, cx| display_ranges(editor, cx))
            .unwrap(),
        [DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(1), 1)]
    );

    _ = editor.update(cx, |editor, window, cx| {
        editor.end_selection(window, cx);
        editor.update_selection(
            DisplayPoint::new(DisplayRow(3), 3),
            0,
            gpui::Point::<f32>::default(),
            window,
            cx,
        );
    });

    assert_eq!(
        editor
            .update(cx, |editor, _, cx| display_ranges(editor, cx))
            .unwrap(),
        [DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(1), 1)]
    );

    _ = editor.update(cx, |editor, window, cx| {
        editor.begin_selection(DisplayPoint::new(DisplayRow(3), 3), true, 1, window, cx);
        editor.update_selection(
            DisplayPoint::new(DisplayRow(0), 0),
            0,
            gpui::Point::<f32>::default(),
            window,
            cx,
        );
    });

    assert_eq!(
        editor
            .update(cx, |editor, _, cx| display_ranges(editor, cx))
            .unwrap(),
        [
            DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(1), 1),
            DisplayPoint::new(DisplayRow(3), 3)..DisplayPoint::new(DisplayRow(0), 0)
        ]
    );

    _ = editor.update(cx, |editor, window, cx| {
        editor.end_selection(window, cx);
    });

    assert_eq!(
        editor
            .update(cx, |editor, _, cx| display_ranges(editor, cx))
            .unwrap(),
        [DisplayPoint::new(DisplayRow(3), 3)..DisplayPoint::new(DisplayRow(0), 0)]
    );
}

#[gpui::test]
fn test_multiple_cursor_removal(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaaaaa\nbbbbbb\ncccccc\nddddddd\n", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.begin_selection(DisplayPoint::new(DisplayRow(2), 1), false, 1, window, cx);
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.end_selection(window, cx);
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.begin_selection(DisplayPoint::new(DisplayRow(3), 2), true, 1, window, cx);
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.end_selection(window, cx);
    });

    assert_eq!(
        editor
            .update(cx, |editor, _, cx| display_ranges(editor, cx))
            .unwrap(),
        [
            DisplayPoint::new(DisplayRow(2), 1)..DisplayPoint::new(DisplayRow(2), 1),
            DisplayPoint::new(DisplayRow(3), 2)..DisplayPoint::new(DisplayRow(3), 2)
        ]
    );

    _ = editor.update(cx, |editor, window, cx| {
        editor.begin_selection(DisplayPoint::new(DisplayRow(2), 1), true, 1, window, cx);
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.end_selection(window, cx);
    });

    assert_eq!(
        editor
            .update(cx, |editor, _, cx| display_ranges(editor, cx))
            .unwrap(),
        [DisplayPoint::new(DisplayRow(3), 2)..DisplayPoint::new(DisplayRow(3), 2)]
    );
}

#[gpui::test]
fn test_canceling_pending_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaaaaa\nbbbbbb\ncccccc\ndddddd\n", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.begin_selection(DisplayPoint::new(DisplayRow(2), 2), false, 1, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(2), 2)]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.update_selection(
            DisplayPoint::new(DisplayRow(3), 3),
            0,
            gpui::Point::<f32>::default(),
            window,
            cx,
        );
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(3), 3)]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.cancel(&Cancel, window, cx);
        editor.update_selection(
            DisplayPoint::new(DisplayRow(1), 1),
            0,
            gpui::Point::<f32>::default(),
            window,
            cx,
        );
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(3), 3)]
        );
    });
}

#[gpui::test]
fn test_movement_actions_with_pending_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaaaaa\nbbbbbb\ncccccc\ndddddd\n", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.begin_selection(DisplayPoint::new(DisplayRow(2), 2), false, 1, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(2), 2)]
        );

        editor.move_down(&Default::default(), window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(3), 2)..DisplayPoint::new(DisplayRow(3), 2)]
        );

        editor.begin_selection(DisplayPoint::new(DisplayRow(2), 2), false, 1, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(2), 2)]
        );

        editor.move_up(&Default::default(), window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(1), 2)..DisplayPoint::new(DisplayRow(1), 2)]
        );
    });
}

#[gpui::test]
fn test_extending_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaa bbb ccc ddd eee", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.begin_selection(DisplayPoint::new(DisplayRow(0), 5), false, 1, window, cx);
        editor.end_selection(window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 5)..DisplayPoint::new(DisplayRow(0), 5)]
        );

        editor.extend_selection(DisplayPoint::new(DisplayRow(0), 10), 1, window, cx);
        editor.end_selection(window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 5)..DisplayPoint::new(DisplayRow(0), 10)]
        );

        editor.extend_selection(DisplayPoint::new(DisplayRow(0), 10), 1, window, cx);
        editor.end_selection(window, cx);
        editor.extend_selection(DisplayPoint::new(DisplayRow(0), 10), 2, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 5)..DisplayPoint::new(DisplayRow(0), 11)]
        );

        editor.update_selection(
            DisplayPoint::new(DisplayRow(0), 1),
            0,
            gpui::Point::<f32>::default(),
            window,
            cx,
        );
        editor.end_selection(window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 5)..DisplayPoint::new(DisplayRow(0), 0)]
        );

        editor.begin_selection(DisplayPoint::new(DisplayRow(0), 5), true, 1, window, cx);
        editor.end_selection(window, cx);
        editor.begin_selection(DisplayPoint::new(DisplayRow(0), 5), true, 2, window, cx);
        editor.end_selection(window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 4)..DisplayPoint::new(DisplayRow(0), 7)]
        );

        editor.extend_selection(DisplayPoint::new(DisplayRow(0), 10), 1, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 4)..DisplayPoint::new(DisplayRow(0), 11)]
        );

        editor.update_selection(
            DisplayPoint::new(DisplayRow(0), 6),
            0,
            gpui::Point::<f32>::default(),
            window,
            cx,
        );
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 4)..DisplayPoint::new(DisplayRow(0), 7)]
        );

        editor.update_selection(
            DisplayPoint::new(DisplayRow(0), 1),
            0,
            gpui::Point::<f32>::default(),
            window,
            cx,
        );
        editor.end_selection(window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 7)..DisplayPoint::new(DisplayRow(0), 0)]
        );
    });
}

#[gpui::test]
fn test_extend_selection_after_collapsed_word_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaa bbb ccc ddd eee", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        // Double-click "bbb" to select it word-wise.
        editor.begin_selection(DisplayPoint::new(DisplayRow(0), 5), false, 2, window, cx);
        editor.end_selection(window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 4)..DisplayPoint::new(DisplayRow(0), 7)]
        );

        // Move the cursor with the keyboard, collapsing the selection.
        editor.move_right(&MoveRight, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 7)..DisplayPoint::new(DisplayRow(0), 7)]
        );

        // Shift+click further along the line should extend from the cursor,
        // not from the stale double-clicked word.
        editor.extend_selection(DisplayPoint::new(DisplayRow(0), 10), 1, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 7)..DisplayPoint::new(DisplayRow(0), 10)]
        );
    });
}

#[gpui::test]
fn test_extend_selection_after_collapsed_word_selection_moved_via_offsets(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaa bbb ccc ddd eee", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        // Double-click "bbb" to select it word-wise.
        editor.begin_selection(DisplayPoint::new(DisplayRow(0), 5), false, 2, window, cx);
        editor.end_selection(window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 4)..DisplayPoint::new(DisplayRow(0), 7)]
        );

        // Collapse the selection through `move_offsets_with`, the code path
        // used by non-keyboard-movement callers like
        // `move_to_enclosing_bracket` and `select_delimiters_impl`.
        editor.change_selections(SelectionEffects::default(), window, cx, |s| {
            s.move_offsets_with(&mut |_, selection| {
                selection.collapse_to(MultiBufferOffset(7), SelectionGoal::None);
            });
        });
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 7)..DisplayPoint::new(DisplayRow(0), 7)]
        );

        // Shift+click further along the line should extend from the cursor,
        // not from the stale double-clicked word.
        editor.extend_selection(DisplayPoint::new(DisplayRow(0), 10), 1, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(0), 7)..DisplayPoint::new(DisplayRow(0), 10)]
        );
    });
}

#[gpui::test]
fn test_extend_selection_from_empty_line_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaa\nbbb\n", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.begin_selection(DisplayPoint::new(DisplayRow(2), 0), false, 3, window, cx);
        editor.end_selection(window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(2), 0)..DisplayPoint::new(DisplayRow(2), 0)]
        );

        editor.extend_selection(DisplayPoint::new(DisplayRow(0), 1), 1, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(2), 0)..DisplayPoint::new(DisplayRow(0), 0)]
        );
    });
}

#[gpui::test]
fn test_clone(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let (text, selection_ranges) = marked_text_ranges(
        indoc! {"
            one
            two
            threeˇ
            four
            fiveˇ
        "},
        true,
    );

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple(&text, cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges(
                selection_ranges
                    .iter()
                    .map(|range| MultiBufferOffset(range.start)..MultiBufferOffset(range.end)),
            )
        });
        editor.fold_creases(
            vec![
                Crease::simple(Point::new(1, 0)..Point::new(2, 0), FoldPlaceholder::test()),
                Crease::simple(Point::new(3, 0)..Point::new(4, 0), FoldPlaceholder::test()),
            ],
            true,
            window,
            cx,
        );
    });

    let cloned_editor = editor
        .update(cx, |editor, _, cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| editor.clone(window, cx))
            })
        })
        .unwrap()
        .unwrap();

    let snapshot = editor
        .update(cx, |e, window, cx| e.snapshot(window, cx))
        .unwrap();
    let cloned_snapshot = cloned_editor
        .update(cx, |e, window, cx| e.snapshot(window, cx))
        .unwrap();

    assert_eq!(
        cloned_editor
            .update(cx, |e, _, cx| e.display_text(cx))
            .unwrap(),
        editor.update(cx, |e, _, cx| e.display_text(cx)).unwrap()
    );
    assert_eq!(
        cloned_snapshot
            .folds_in_range(MultiBufferOffset(0)..MultiBufferOffset(text.len()))
            .collect::<Vec<_>>(),
        snapshot
            .folds_in_range(MultiBufferOffset(0)..MultiBufferOffset(text.len()))
            .collect::<Vec<_>>(),
    );
    assert_set_eq!(
        cloned_editor
            .update(cx, |editor, _, cx| editor
                .selections
                .ranges::<Point>(&editor.display_snapshot(cx)))
            .unwrap(),
        editor
            .update(cx, |editor, _, cx| editor
                .selections
                .ranges(&editor.display_snapshot(cx)))
            .unwrap()
    );
    assert_set_eq!(
        cloned_editor
            .update(cx, |e, _window, cx| e
                .selections
                .display_ranges(&e.display_snapshot(cx)))
            .unwrap(),
        editor
            .update(cx, |e, _, cx| e
                .selections
                .display_ranges(&e.display_snapshot(cx)))
            .unwrap()
    );
}

#[gpui::test]
fn test_toggle_breadcrumb_does_not_change_settings(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    update_test_editor_settings(cx, &|settings| {
        settings.toolbar.get_or_insert_default().breadcrumbs = Some(true);
    });

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("hello", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        assert!(EditorSettings::get_global(cx).toolbar.breadcrumbs);
        assert_eq!(
            editor.breadcrumb_location(cx),
            ToolbarItemLocation::PrimaryLeft
        );

        editor.toggle_breadcrumb(&ToggleBreadcrumb, window, cx);
        assert!(EditorSettings::get_global(cx).toolbar.breadcrumbs);
        assert_eq!(editor.breadcrumb_location(cx), ToolbarItemLocation::Hidden);
    });

    // Changing unrelated settings should not affect breadcrumbs visibility.
    update_test_editor_settings(cx, &|settings| {
        settings.vertical_scroll_margin = Some(4.0);
    });
    cx.run_until_parked();

    _ = editor.update(cx, |editor, window, cx| {
        assert!(EditorSettings::get_global(cx).toolbar.breadcrumbs);
        assert_eq!(editor.breadcrumb_location(cx), ToolbarItemLocation::Hidden);

        editor.toggle_breadcrumb(&ToggleBreadcrumb, window, cx);
        assert!(EditorSettings::get_global(cx).toolbar.breadcrumbs);
        assert_eq!(
            editor.breadcrumb_location(cx),
            ToolbarItemLocation::PrimaryLeft
        );
    });
}

#[gpui::test]
async fn test_navigation_history(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    use workspace::item::Item;

    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = window
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let pane = workspace.read_with(cx, |workspace, _| workspace.active_pane().clone());

    _ = window.update(cx, |_mw, window, cx| {
        cx.new(|cx| {
            let buffer = MultiBuffer::build_simple(&sample_text(300, 5, 'a'), cx);
            let mut editor = build_editor(buffer, window, cx);
            let handle = cx.entity();
            editor.set_nav_history(Some(pane.read(cx).nav_history_for_item(&handle)));

            fn pop_history(editor: &mut Editor, cx: &mut App) -> Option<NavigationEntry> {
                editor.nav_history.as_mut().unwrap().pop_backward(cx)
            }

            // Move the cursor a small distance.
            // Nothing is added to the navigation history.
            editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                s.select_display_ranges([
                    DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0)
                ])
            });
            editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                s.select_display_ranges([
                    DisplayPoint::new(DisplayRow(3), 0)..DisplayPoint::new(DisplayRow(3), 0)
                ])
            });
            assert!(pop_history(&mut editor, cx).is_none());

            // Move the cursor a large distance.
            // The history can jump back to the previous position.
            editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                s.select_display_ranges([
                    DisplayPoint::new(DisplayRow(13), 0)..DisplayPoint::new(DisplayRow(13), 3)
                ])
            });
            let nav_entry = pop_history(&mut editor, cx).unwrap();
            editor.navigate(nav_entry.data.unwrap(), window, cx);
            assert_eq!(nav_entry.item.id(), cx.entity_id());
            assert_eq!(
                editor
                    .selections
                    .display_ranges(&editor.display_snapshot(cx)),
                &[DisplayPoint::new(DisplayRow(3), 0)..DisplayPoint::new(DisplayRow(3), 0)]
            );
            assert!(pop_history(&mut editor, cx).is_none());

            // Move the cursor a small distance via the mouse.
            // Nothing is added to the navigation history.
            editor.begin_selection(DisplayPoint::new(DisplayRow(5), 0), false, 1, window, cx);
            editor.end_selection(window, cx);
            assert_eq!(
                editor
                    .selections
                    .display_ranges(&editor.display_snapshot(cx)),
                &[DisplayPoint::new(DisplayRow(5), 0)..DisplayPoint::new(DisplayRow(5), 0)]
            );
            assert!(pop_history(&mut editor, cx).is_none());

            // Move the cursor a large distance via the mouse.
            // The history can jump back to the previous position.
            editor.begin_selection(DisplayPoint::new(DisplayRow(15), 0), false, 1, window, cx);
            editor.end_selection(window, cx);
            assert_eq!(
                editor
                    .selections
                    .display_ranges(&editor.display_snapshot(cx)),
                &[DisplayPoint::new(DisplayRow(15), 0)..DisplayPoint::new(DisplayRow(15), 0)]
            );
            let nav_entry = pop_history(&mut editor, cx).unwrap();
            editor.navigate(nav_entry.data.unwrap(), window, cx);
            assert_eq!(nav_entry.item.id(), cx.entity_id());
            assert_eq!(
                editor
                    .selections
                    .display_ranges(&editor.display_snapshot(cx)),
                &[DisplayPoint::new(DisplayRow(5), 0)..DisplayPoint::new(DisplayRow(5), 0)]
            );
            assert!(pop_history(&mut editor, cx).is_none());

            // Set scroll position to check later
            editor.set_scroll_position(gpui::Point::<f64>::new(5.5, 5.5), window, cx);
            let original_scroll_position = editor
                .scroll_manager
                .native_anchor(&editor.display_snapshot(cx), cx);

            // Jump to the end of the document and adjust scroll
            editor.move_to_end(&MoveToEnd, window, cx);
            editor.set_scroll_position(gpui::Point::<f64>::new(-2.5, -0.5), window, cx);
            assert_ne!(
                editor
                    .scroll_manager
                    .native_anchor(&editor.display_snapshot(cx), cx),
                original_scroll_position
            );

            let nav_entry = pop_history(&mut editor, cx).unwrap();
            editor.navigate(nav_entry.data.unwrap(), window, cx);
            assert_eq!(
                editor
                    .scroll_manager
                    .native_anchor(&editor.display_snapshot(cx), cx),
                original_scroll_position
            );

            let other_buffer =
                cx.new(|cx| MultiBuffer::singleton(cx.new(|cx| Buffer::local("test", cx)), cx));

            // Ensure we don't panic when navigation data contains invalid anchors *and* points.
            let invalid_anchor = other_buffer.update(cx, |buffer, cx| {
                buffer.snapshot(cx).anchor_after(MultiBufferOffset(3))
            });
            let invalid_point = Point::new(9999, 0);
            editor.navigate(
                Arc::new(NavigationData {
                    cursor_anchor: invalid_anchor,
                    cursor_position: invalid_point,
                    scroll_anchor: ScrollAnchor {
                        anchor: invalid_anchor,
                        offset: Default::default(),
                    },
                    scroll_top_row: invalid_point.row,
                }),
                window,
                cx,
            );
            assert_eq!(
                editor
                    .selections
                    .display_ranges(&editor.display_snapshot(cx)),
                &[editor.max_point(cx)..editor.max_point(cx)]
            );
            assert_eq!(
                editor.scroll_position(cx),
                gpui::Point::new(0., editor.max_point(cx).row().as_f64())
            );

            editor
        })
    });
}

#[gpui::test]
fn test_cancel(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaaaaa\nbbbbbb\ncccccc\ndddddd\n", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.begin_selection(DisplayPoint::new(DisplayRow(3), 4), false, 1, window, cx);
        editor.update_selection(
            DisplayPoint::new(DisplayRow(1), 1),
            0,
            gpui::Point::<f32>::default(),
            window,
            cx,
        );
        editor.end_selection(window, cx);

        editor.begin_selection(DisplayPoint::new(DisplayRow(0), 1), true, 1, window, cx);
        editor.update_selection(
            DisplayPoint::new(DisplayRow(0), 3),
            0,
            gpui::Point::<f32>::default(),
            window,
            cx,
        );
        editor.end_selection(window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(0), 3),
                DisplayPoint::new(DisplayRow(3), 4)..DisplayPoint::new(DisplayRow(1), 1),
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.cancel(&Cancel, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(3), 4)..DisplayPoint::new(DisplayRow(1), 1)]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.cancel(&Cancel, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            [DisplayPoint::new(DisplayRow(1), 1)..DisplayPoint::new(DisplayRow(1), 1)]
        );
    });
}

#[gpui::test]
async fn test_fold_action(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(rust_lang()), cx));
    cx.set_state(indoc! {"
        impl Foo {
            // Hello!

            fn a() {
                1
            }

            fn b() {
                2
            }

            fn c() {
                3
            }
        }ˇ
    "});

    cx.update_editor(|editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(7), 0)..DisplayPoint::new(DisplayRow(12), 0)
            ]);
        });
        editor.fold(&Fold, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                impl Foo {
                    // Hello!

                    fn a() {
                        1
                    }

                    fn b() {⋯}

                    fn c() {⋯}
                }
            "
            .unindent(),
        );

        editor.fold(&Fold, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                impl Foo {⋯}
            "
            .unindent(),
        );

        editor.unfold_lines(&UnfoldLines, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                impl Foo {
                    // Hello!

                    fn a() {
                        1
                    }

                    fn b() {⋯}

                    fn c() {⋯}
                }
            "
            .unindent(),
        );

        editor.unfold_lines(&UnfoldLines, window, cx);
        assert_eq!(
            editor.display_text(cx),
            editor.buffer.read(cx).read(cx).text()
        );
    });
}

#[gpui::test]
fn test_fold_action_without_language(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple(
            &"
                impl Foo {
                    // Hello!

                    fn a() {
                        1
                    }

                    fn b() {
                        2
                    }

                    fn c() {
                        3
                    }
                }
            "
            .unindent(),
            cx,
        );
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(7), 0)..DisplayPoint::new(DisplayRow(12), 0)
            ]);
        });
        editor.fold(&Fold, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                impl Foo {
                    // Hello!

                    fn a() {
                        1
                    }

                    fn b() {⋯
                    }

                    fn c() {⋯
                    }
                }
            "
            .unindent(),
        );

        editor.fold(&Fold, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                impl Foo {⋯
                }
            "
            .unindent(),
        );

        editor.unfold_lines(&UnfoldLines, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                impl Foo {
                    // Hello!

                    fn a() {
                        1
                    }

                    fn b() {⋯
                    }

                    fn c() {⋯
                    }
                }
            "
            .unindent(),
        );

        editor.unfold_lines(&UnfoldLines, window, cx);
        assert_eq!(
            editor.display_text(cx),
            editor.buffer.read(cx).read(cx).text()
        );
    });
}

#[gpui::test]
fn test_fold_action_whitespace_sensitive_language(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple(
            &"
                class Foo:
                    # Hello!

                    def a():
                        print(1)

                    def b():
                        print(2)

                    def c():
                        print(3)
            "
            .unindent(),
            cx,
        );
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(6), 0)..DisplayPoint::new(DisplayRow(10), 0)
            ]);
        });
        editor.fold(&Fold, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                class Foo:
                    # Hello!

                    def a():
                        print(1)

                    def b():⋯

                    def c():⋯
            "
            .unindent(),
        );

        editor.fold(&Fold, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                class Foo:⋯
            "
            .unindent(),
        );

        editor.unfold_lines(&UnfoldLines, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                class Foo:
                    # Hello!

                    def a():
                        print(1)

                    def b():⋯

                    def c():⋯
            "
            .unindent(),
        );

        editor.unfold_lines(&UnfoldLines, window, cx);
        assert_eq!(
            editor.display_text(cx),
            editor.buffer.read(cx).read(cx).text()
        );
    });
}

#[gpui::test]
fn test_fold_action_multiple_line_breaks(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple(
            &"
                class Foo:
                    # Hello!

                    def a():
                        print(1)

                    def b():
                        print(2)


                    def c():
                        print(3)


            "
            .unindent(),
            cx,
        );
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(6), 0)..DisplayPoint::new(DisplayRow(11), 0)
            ]);
        });
        editor.fold(&Fold, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                class Foo:
                    # Hello!

                    def a():
                        print(1)

                    def b():⋯


                    def c():⋯


            "
            .unindent(),
        );

        editor.fold(&Fold, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                class Foo:⋯


            "
            .unindent(),
        );

        editor.unfold_lines(&UnfoldLines, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                class Foo:
                    # Hello!

                    def a():
                        print(1)

                    def b():⋯


                    def c():⋯


            "
            .unindent(),
        );

        editor.unfold_lines(&UnfoldLines, window, cx);
        assert_eq!(
            editor.display_text(cx),
            editor.buffer.read(cx).read(cx).text()
        );
    });
}

#[gpui::test]
async fn test_fold_with_unindented_multiline_raw_string(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let language = Arc::new(
        Language::new(
            LanguageConfig::default(),
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_queries(LanguageQueries {
            overrides: Some(Cow::from(indoc! {"
                [
                  (string_literal)
                  (raw_string_literal)
                ] @string
                [
                  (line_comment)
                  (block_comment)
                ] @comment.inclusive
            "})),
            ..Default::default()
        })
        .expect("Could not parse queries"),
    );

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
    cx.set_state(indoc! {"
        fn main() {
            let s = r#\"
        a
        b
        c
        \"#;
        }ˇ
    "});

    cx.update_editor(|editor, window, cx| {
        editor.fold_at_level(&FoldAtLevel(1), window, cx);
        assert_eq!(
            editor.display_text(cx),
            indoc! {"
                fn main() {⋯
                }
            "},
        );
    });
}

#[gpui::test]
async fn test_fold_with_unindented_multiline_raw_string_includes_closing_bracket(
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(rust_lang()), cx));
    cx.set_state(indoc! {"
        ˇfn main() {
            let s = r#\"
        a
        b
        c
        \"#;
        }
    "});

    cx.update_editor(|editor, window, cx| {
        editor.fold_at_level(&FoldAtLevel(1), window, cx);
        assert_eq!(
            editor.display_text(cx),
            indoc! {"
                fn main() {⋯}
            "},
        );
    });
}

#[gpui::test]
async fn test_fold_with_unindented_multiline_block_comment(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let language = Arc::new(
        Language::new(
            LanguageConfig::default(),
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_queries(LanguageQueries {
            overrides: Some(Cow::from(indoc! {"
                [
                  (string_literal)
                  (raw_string_literal)
                ] @string
                [
                  (line_comment)
                  (block_comment)
                ] @comment.inclusive
            "})),
            ..Default::default()
        })
        .expect("Could not parse queries"),
    );

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
    cx.set_state(indoc! {"
        fn main() {
            let x = 1;
            /*
        unindented comment line
            */
        }ˇ
    "});

    cx.update_editor(|editor, window, cx| {
        editor.fold_at_level(&FoldAtLevel(1), window, cx);
        assert_eq!(
            editor.display_text(cx),
            indoc! {"
                fn main() {⋯
                }
            "},
        );
    });
}

#[gpui::test]
async fn test_fold_with_unindented_multiline_block_comment_includes_closing_bracket(
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(rust_lang()), cx));
    cx.set_state(indoc! {"
        ˇfn main() {
            let x = 1;
            /*
        unindented comment line
            */
        }
    "});

    cx.update_editor(|editor, window, cx| {
        editor.fold_at_level(&FoldAtLevel(1), window, cx);
        assert_eq!(
            editor.display_text(cx),
            indoc! {"
                fn main() {⋯}
            "},
        );
    });
}

#[gpui::test]
async fn test_fold_preserves_top_level_comments_between_python_classes(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let language = Arc::new(
        Language::new(
            LanguageConfig::default(),
            Some(tree_sitter_python::LANGUAGE.into()),
        )
        .with_queries(LanguageQueries {
            overrides: Some(Cow::from(indoc! {"
                (comment) @comment.inclusive
                (string) @string
            "})),
            ..Default::default()
        })
        .expect("Could not parse queries"),
    );

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
    cx.set_state(indoc! {"
        class Foo:
            def bar(self):
                pass


        # SECTION SEPARATOR

        class Baz:
            def qux(self):
                passˇ
    "});

    cx.update_editor(|editor, window, cx| {
        editor.fold_at_level(&FoldAtLevel(1), window, cx);
        assert_eq!(
            editor.display_text(cx),
            indoc! {"
                class Foo:⋯


                # SECTION SEPARATOR

                class Baz:
                    def qux(self):
                        pass
            "},
        );
    });
}

#[gpui::test]
async fn test_fold_preserves_top_level_comments_between_rust_functions(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let language = Arc::new(
        Language::new(
            LanguageConfig::default(),
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_queries(LanguageQueries {
            overrides: Some(Cow::from(indoc! {"
                [
                  (string_literal)
                  (raw_string_literal)
                ] @string
                [
                  (line_comment)
                  (block_comment)
                ] @comment.inclusive
            "})),
            ..Default::default()
        })
        .expect("Could not parse queries"),
    );

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
    cx.set_state(indoc! {"
        fn foo() {
            bar();
        }


        // SECTION SEPARATOR


        fn baz() {
            qux();ˇ
        }
    "});

    cx.update_editor(|editor, window, cx| {
        editor.fold_at_level(&FoldAtLevel(1), window, cx);
        assert_eq!(
            editor.display_text(cx),
            indoc! {"
                fn foo() {⋯
                }


                // SECTION SEPARATOR


                fn baz() {
                    qux();
                }
            "},
        );
    });
}

#[gpui::test]
async fn test_fold_terminates_at_top_level_multiline_string_between_python_classes(
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let language = Arc::new(
        Language::new(
            LanguageConfig::default(),
            Some(tree_sitter_python::LANGUAGE.into()),
        )
        .with_queries(LanguageQueries {
            overrides: Some(Cow::from(indoc! {"
                (comment) @comment.inclusive
                (string) @string
            "})),
            ..Default::default()
        })
        .expect("Could not parse queries"),
    );

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
    cx.set_state(indoc! {r#"
        class Foo:
            def bar(self):
                pass


        """
        top-level docstring at zero indent
        """


        class Baz:
            def qux(self):
                passˇ
    "#});

    cx.update_editor(|editor, window, cx| {
        editor.fold_at_level(&FoldAtLevel(1), window, cx);
        assert_eq!(
            editor.display_text(cx),
            indoc! {r#"
                class Foo:⋯


                """
                top-level docstring at zero indent
                """


                class Baz:
                    def qux(self):
                        pass
            "#},
        );
    });
}

#[gpui::test]
fn test_fold_at_level(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple(
            &"
                class Foo:
                    # Hello!

                    def a():
                        print(1)

                    def b():
                        print(2)


                class Bar:
                    # World!

                    def a():
                        print(1)

                    def b():
                        print(2)


            "
            .unindent(),
            cx,
        );
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.fold_at_level(&FoldAtLevel(2), window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                class Foo:
                    # Hello!

                    def a():⋯

                    def b():⋯


                class Bar:
                    # World!

                    def a():⋯

                    def b():⋯


            "
            .unindent(),
        );

        editor.fold_at_level(&FoldAtLevel(1), window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                class Foo:⋯


                class Bar:⋯


            "
            .unindent(),
        );

        editor.unfold_all(&UnfoldAll, window, cx);
        editor.fold_at_level(&FoldAtLevel(0), window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                class Foo:
                    # Hello!

                    def a():
                        print(1)

                    def b():
                        print(2)


                class Bar:
                    # World!

                    def a():
                        print(1)

                    def b():
                        print(2)


            "
            .unindent(),
        );

        assert_eq!(
            editor.display_text(cx),
            editor.buffer.read(cx).read(cx).text()
        );
        let (_, positions) = marked_text_ranges(
            &"
                       class Foo:
                           # Hello!

                           def a():
                              print(1)

                           def b():
                               p«riˇ»nt(2)


                       class Bar:
                           # World!

                           def a():
                               «ˇprint(1)

                           def b():
                               print(2)»


                   "
            .unindent(),
            true,
        );

        editor.change_selections(SelectionEffects::default(), window, cx, |s| {
            s.select_ranges(
                positions
                    .iter()
                    .map(|range| MultiBufferOffset(range.start)..MultiBufferOffset(range.end)),
            )
        });

        editor.fold_at_level(&FoldAtLevel(2), window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
                class Foo:
                    # Hello!

                    def a():⋯

                    def b():
                        print(2)


                class Bar:
                    # World!

                    def a():
                        print(1)

                    def b():
                        print(2)


            "
            .unindent(),
        );
    });
}

#[gpui::test]
fn test_move_cursor(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let buffer = cx.update(|cx| MultiBuffer::build_simple(&sample_text(6, 6, 'a'), cx));
    let editor = cx.add_window(|window, cx| build_editor(buffer.clone(), window, cx));

    buffer.update(cx, |buffer, cx| {
        buffer.edit(
            vec![
                (Point::new(1, 0)..Point::new(1, 0), "\t"),
                (Point::new(1, 1)..Point::new(1, 1), "\t"),
            ],
            None,
            cx,
        );
    });
    _ = editor.update(cx, |editor, window, cx| {
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0)]
        );

        editor.move_down(&MoveDown, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0)]
        );

        editor.move_right(&MoveRight, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(1), 4)..DisplayPoint::new(DisplayRow(1), 4)]
        );

        editor.move_left(&MoveLeft, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0)]
        );

        editor.move_up(&MoveUp, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0)]
        );

        editor.move_to_end(&MoveToEnd, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(5), 6)..DisplayPoint::new(DisplayRow(5), 6)]
        );

        editor.move_to_beginning(&MoveToBeginning, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0)]
        );

        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(0), 2)
            ]);
        });
        editor.select_to_beginning(&SelectToBeginning, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(0), 0)]
        );

        editor.select_to_end(&SelectToEnd, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(5), 6)]
        );
    });
}

#[gpui::test]
fn test_move_cursor_multibyte(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("🟥🟧🟨🟩🟦🟪\nabcde\nαβγδε", cx);
        build_editor(buffer, window, cx)
    });

    assert_eq!('🟥'.len_utf8(), 4);
    assert_eq!('α'.len_utf8(), 2);

    _ = editor.update(cx, |editor, window, cx| {
        editor.fold_creases(
            vec![
                Crease::simple(Point::new(0, 8)..Point::new(0, 16), FoldPlaceholder::test()),
                Crease::simple(Point::new(1, 2)..Point::new(1, 4), FoldPlaceholder::test()),
                Crease::simple(Point::new(2, 4)..Point::new(2, 8), FoldPlaceholder::test()),
            ],
            true,
            window,
            cx,
        );
        assert_eq!(editor.display_text(cx), "🟥🟧⋯🟦🟪\nab⋯e\nαβ⋯ε");

        editor.move_right(&MoveRight, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(0, "🟥".len())]);
        editor.move_right(&MoveRight, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(0, "🟥🟧".len())]);
        editor.move_right(&MoveRight, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(0, "🟥🟧⋯".len())]);

        editor.move_down(&MoveDown, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(1, "ab⋯e".len())]);
        editor.move_left(&MoveLeft, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(1, "ab⋯".len())]);
        editor.move_left(&MoveLeft, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(1, "ab".len())]);
        editor.move_left(&MoveLeft, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(1, "a".len())]);

        editor.move_down(&MoveDown, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(2, "α".len())]);
        editor.move_right(&MoveRight, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(2, "αβ".len())]);
        editor.move_right(&MoveRight, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(2, "αβ⋯".len())]);
        editor.move_right(&MoveRight, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(2, "αβ⋯ε".len())]);

        editor.move_up(&MoveUp, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(1, "ab⋯e".len())]);
        editor.move_down(&MoveDown, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(2, "αβ⋯ε".len())]);
        editor.move_up(&MoveUp, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(1, "ab⋯e".len())]);

        editor.move_up(&MoveUp, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(0, "🟥🟧".len())]);
        editor.move_left(&MoveLeft, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(0, "🟥".len())]);
        editor.move_left(&MoveLeft, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(0, "".len())]);
    });
}

#[gpui::test]
fn test_move_cursor_different_line_lengths(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("ⓐⓑⓒⓓⓔ\nabcd\nαβγ\nabcd\nⓐⓑⓒⓓⓔ\n", cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([empty_range(0, "ⓐⓑⓒⓓⓔ".len())]);
        });

        // moving above start of document should move selection to start of document,
        // but the next move down should still be at the original goal_x
        editor.move_up(&MoveUp, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(0, "".len())]);

        editor.move_down(&MoveDown, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(1, "abcd".len())]);

        editor.move_down(&MoveDown, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(2, "αβγ".len())]);

        editor.move_down(&MoveDown, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(3, "abcd".len())]);

        editor.move_down(&MoveDown, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(4, "ⓐⓑⓒⓓⓔ".len())]);

        // moving past end of document should not change goal_x
        editor.move_down(&MoveDown, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(5, "".len())]);

        editor.move_down(&MoveDown, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(5, "".len())]);

        editor.move_up(&MoveUp, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(4, "ⓐⓑⓒⓓⓔ".len())]);

        editor.move_up(&MoveUp, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(3, "abcd".len())]);

        editor.move_up(&MoveUp, window, cx);
        assert_eq!(display_ranges(editor, cx), &[empty_range(2, "αβγ".len())]);
    });
}

#[gpui::test]
fn test_beginning_end_of_line(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let move_to_beg = MoveToBeginningOfLine {
        stop_at_soft_wraps: true,
        stop_at_indent: true,
    };

    let delete_to_beg = DeleteToBeginningOfLine {
        stop_at_indent: false,
    };

    let move_to_end = MoveToEndOfLine {
        stop_at_soft_wraps: true,
    };

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("abc\n  def", cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(1), 4)..DisplayPoint::new(DisplayRow(1), 4),
            ]);
        });
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.move_to_beginning_of_line(&move_to_beg, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(1), 2)..DisplayPoint::new(DisplayRow(1), 2),
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.move_to_beginning_of_line(&move_to_beg, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0),
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.move_to_beginning_of_line(&move_to_beg, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(1), 2)..DisplayPoint::new(DisplayRow(1), 2),
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.move_to_end_of_line(&move_to_end, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 3)..DisplayPoint::new(DisplayRow(0), 3),
                DisplayPoint::new(DisplayRow(1), 5)..DisplayPoint::new(DisplayRow(1), 5),
            ]
        );
    });

    // Moving to the end of line again is a no-op.
    _ = editor.update(cx, |editor, window, cx| {
        editor.move_to_end_of_line(&move_to_end, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 3)..DisplayPoint::new(DisplayRow(0), 3),
                DisplayPoint::new(DisplayRow(1), 5)..DisplayPoint::new(DisplayRow(1), 5),
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.move_left(&MoveLeft, window, cx);
        editor.select_to_beginning_of_line(
            &SelectToBeginningOfLine {
                stop_at_soft_wraps: true,
                stop_at_indent: true,
            },
            window,
            cx,
        );
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(1), 4)..DisplayPoint::new(DisplayRow(1), 2),
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.select_to_beginning_of_line(
            &SelectToBeginningOfLine {
                stop_at_soft_wraps: true,
                stop_at_indent: true,
            },
            window,
            cx,
        );
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(1), 4)..DisplayPoint::new(DisplayRow(1), 0),
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.select_to_beginning_of_line(
            &SelectToBeginningOfLine {
                stop_at_soft_wraps: true,
                stop_at_indent: true,
            },
            window,
            cx,
        );
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(1), 4)..DisplayPoint::new(DisplayRow(1), 2),
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.select_to_end_of_line(
            &SelectToEndOfLine {
                stop_at_soft_wraps: true,
            },
            window,
            cx,
        );
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 3),
                DisplayPoint::new(DisplayRow(1), 4)..DisplayPoint::new(DisplayRow(1), 5),
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.delete_to_end_of_line(&DeleteToEndOfLine, window, cx);
        assert_eq!(editor.display_text(cx), "ab\n  de");
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 2),
                DisplayPoint::new(DisplayRow(1), 4)..DisplayPoint::new(DisplayRow(1), 4),
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.delete_to_beginning_of_line(&delete_to_beg, window, cx);
        assert_eq!(editor.display_text(cx), "\n");
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0),
            ]
        );
    });
}

#[gpui::test]
fn test_beginning_of_line_single_line_editor(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    _ = editor.update(cx, |editor, window, cx| {
        editor.set_text("  indented text", window, cx);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 10)..DisplayPoint::new(DisplayRow(0), 10)
            ]);
        });

        editor.move_to_beginning_of_line(
            &MoveToBeginningOfLine {
                stop_at_soft_wraps: true,
                stop_at_indent: true,
            },
            window,
            cx,
        );
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0)]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 10)..DisplayPoint::new(DisplayRow(0), 10)
            ]);
        });

        editor.select_to_beginning_of_line(
            &SelectToBeginningOfLine {
                stop_at_soft_wraps: true,
                stop_at_indent: true,
            },
            window,
            cx,
        );
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(0), 10)..DisplayPoint::new(DisplayRow(0), 0)]
        );
    });
}

#[gpui::test]
fn test_only_focused_editor_blinks_across_window_activation(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let window = cx.add_window(|window, cx| Editor::single_line(window, cx));
    let unfocused_editor = window
        .update(cx, |focused_editor, window, cx| {
            window.focus(&focused_editor.focus_handle(cx), cx);
            let unfocused_editor = cx.new(|cx| Editor::single_line(window, cx));
            window.activate_window();
            unfocused_editor
        })
        .unwrap();
    let focused_editor = window.root(cx).unwrap();
    let cx = &mut VisualTestContext::from_window(*window, cx);
    cx.run_until_parked();

    cx.update(|window, cx| {
        assert!(window.is_window_active());
        assert!(focused_editor.read(cx).blink_manager.read(cx).enabled());
        assert!(!unfocused_editor.read(cx).blink_manager.read(cx).enabled());
    });

    cx.deactivate_window();
    cx.update(|window, cx| {
        assert!(!window.is_window_active());
        assert!(!focused_editor.read(cx).blink_manager.read(cx).enabled());
        assert!(!unfocused_editor.read(cx).blink_manager.read(cx).enabled());
    });

    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.is_window_active());
        assert!(focused_editor.read(cx).blink_manager.read(cx).enabled());
        assert!(!unfocused_editor.read(cx).blink_manager.read(cx).enabled());
    });
}

#[gpui::test]
fn test_beginning_end_of_line_ignore_soft_wrap(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let move_to_beg = MoveToBeginningOfLine {
        stop_at_soft_wraps: false,
        stop_at_indent: false,
    };

    let move_to_end = MoveToEndOfLine {
        stop_at_soft_wraps: false,
    };

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("thequickbrownfox\njumpedoverthelazydogs", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.set_wrap_width(Some(140.0.into()), cx);

        // We expect the following lines after wrapping
        // ```
        // thequickbrownfox
        // jumpedoverthelazydo
        // gs
        // ```
        // The final `gs` was soft-wrapped onto a new line.
        assert_eq!(
            "thequickbrownfox\njumpedoverthelaz\nydogs",
            editor.display_text(cx),
        );

        // First, let's assert behavior on the first line, that was not soft-wrapped.
        // Start the cursor at the `k` on the first line
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 7)..DisplayPoint::new(DisplayRow(0), 7)
            ]);
        });

        // Moving to the beginning of the line should put us at the beginning of the line.
        editor.move_to_beginning_of_line(&move_to_beg, window, cx);
        assert_eq!(
            vec![DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0),],
            display_ranges(editor, cx)
        );

        // Moving to the end of the line should put us at the end of the line.
        editor.move_to_end_of_line(&move_to_end, window, cx);
        assert_eq!(
            vec![DisplayPoint::new(DisplayRow(0), 16)..DisplayPoint::new(DisplayRow(0), 16),],
            display_ranges(editor, cx)
        );

        // Now, let's assert behavior on the second line, that ended up being soft-wrapped.
        // Start the cursor at the last line (`y` that was wrapped to a new line)
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(2), 0)..DisplayPoint::new(DisplayRow(2), 0)
            ]);
        });

        // Moving to the beginning of the line should put us at the start of the second line of
        // display text, i.e., the `j`.
        editor.move_to_beginning_of_line(&move_to_beg, window, cx);
        assert_eq!(
            vec![DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0),],
            display_ranges(editor, cx)
        );

        // Moving to the beginning of the line again should be a no-op.
        editor.move_to_beginning_of_line(&move_to_beg, window, cx);
        assert_eq!(
            vec![DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0),],
            display_ranges(editor, cx)
        );

        // Moving to the end of the line should put us right after the `s` that was soft-wrapped to the
        // next display line.
        editor.move_to_end_of_line(&move_to_end, window, cx);
        assert_eq!(
            vec![DisplayPoint::new(DisplayRow(2), 5)..DisplayPoint::new(DisplayRow(2), 5),],
            display_ranges(editor, cx)
        );

        // Moving to the end of the line again should be a no-op.
        editor.move_to_end_of_line(&move_to_end, window, cx);
        assert_eq!(
            vec![DisplayPoint::new(DisplayRow(2), 5)..DisplayPoint::new(DisplayRow(2), 5),],
            display_ranges(editor, cx)
        );
    });
}

#[gpui::test]
fn test_beginning_of_line_stop_at_indent(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let move_to_beg = MoveToBeginningOfLine {
        stop_at_soft_wraps: true,
        stop_at_indent: true,
    };

    let select_to_beg = SelectToBeginningOfLine {
        stop_at_soft_wraps: true,
        stop_at_indent: true,
    };

    let delete_to_beg = DeleteToBeginningOfLine {
        stop_at_indent: true,
    };

    let move_to_end = MoveToEndOfLine {
        stop_at_soft_wraps: false,
    };

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("abc\n  def", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(1), 4)..DisplayPoint::new(DisplayRow(1), 4),
            ]);
        });

        // Moving to the beginning of the line should put the first cursor at the beginning of the line,
        // and the second cursor at the first non-whitespace character in the line.
        editor.move_to_beginning_of_line(&move_to_beg, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(1), 2)..DisplayPoint::new(DisplayRow(1), 2),
            ]
        );

        // Moving to the beginning of the line again should be a no-op for the first cursor,
        // and should move the second cursor to the beginning of the line.
        editor.move_to_beginning_of_line(&move_to_beg, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0),
            ]
        );

        // Moving to the beginning of the line again should still be a no-op for the first cursor,
        // and should move the second cursor back to the first non-whitespace character in the line.
        editor.move_to_beginning_of_line(&move_to_beg, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(1), 2)..DisplayPoint::new(DisplayRow(1), 2),
            ]
        );

        // Selecting to the beginning of the line should select to the beginning of the line for the first cursor,
        // and to the first non-whitespace character in the line for the second cursor.
        editor.move_to_end_of_line(&move_to_end, window, cx);
        editor.move_left(&MoveLeft, window, cx);
        editor.select_to_beginning_of_line(&select_to_beg, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(1), 4)..DisplayPoint::new(DisplayRow(1), 2),
            ]
        );

        // Selecting to the beginning of the line again should be a no-op for the first cursor,
        // and should select to the beginning of the line for the second cursor.
        editor.select_to_beginning_of_line(&select_to_beg, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(1), 4)..DisplayPoint::new(DisplayRow(1), 0),
            ]
        );

        // Deleting to the beginning of the line should delete to the beginning of the line for the first cursor,
        // and should delete to the first non-whitespace character in the line for the second cursor.
        editor.move_to_end_of_line(&move_to_end, window, cx);
        editor.move_left(&MoveLeft, window, cx);
        editor.delete_to_beginning_of_line(&delete_to_beg, window, cx);
        assert_eq!(editor.text(cx), "c\n  f");
    });
}

#[gpui::test]
fn test_beginning_of_line_with_cursor_between_line_start_and_indent(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let move_to_beg = MoveToBeginningOfLine {
        stop_at_soft_wraps: true,
        stop_at_indent: true,
    };

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("    hello\nworld", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        // test cursor between line_start and indent_start
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 3)..DisplayPoint::new(DisplayRow(0), 3)
            ]);
        });

        // cursor should move to line_start
        editor.move_to_beginning_of_line(&move_to_beg, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0)]
        );

        // cursor should move to indent_start
        editor.move_to_beginning_of_line(&move_to_beg, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(0), 4)..DisplayPoint::new(DisplayRow(0), 4)]
        );

        // cursor should move to back to line_start
        editor.move_to_beginning_of_line(&move_to_beg, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0)]
        );
    });
}

#[gpui::test]
fn test_prev_next_word_boundary(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("use std::str::{foo, bar}\n\n  {baz.qux()}", cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 11)..DisplayPoint::new(DisplayRow(0), 11),
                DisplayPoint::new(DisplayRow(2), 4)..DisplayPoint::new(DisplayRow(2), 4),
            ])
        });
        editor.move_to_previous_word_start(&MoveToPreviousWordStart, window, cx);
        assert_selection_ranges("use std::ˇstr::{foo, bar}\n\n  {ˇbaz.qux()}", editor, cx);

        editor.move_to_previous_word_start(&MoveToPreviousWordStart, window, cx);
        assert_selection_ranges("use stdˇ::str::{foo, bar}\n\n  ˇ{baz.qux()}", editor, cx);

        editor.move_to_previous_word_start(&MoveToPreviousWordStart, window, cx);
        assert_selection_ranges("use ˇstd::str::{foo, bar}\n\nˇ  {baz.qux()}", editor, cx);

        editor.move_to_previous_word_start(&MoveToPreviousWordStart, window, cx);
        assert_selection_ranges("ˇuse std::str::{foo, bar}\nˇ\n  {baz.qux()}", editor, cx);

        editor.move_to_previous_word_start(&MoveToPreviousWordStart, window, cx);
        assert_selection_ranges("ˇuse std::str::{foo, barˇ}\n\n  {baz.qux()}", editor, cx);

        editor.move_to_next_word_end(&MoveToNextWordEnd, window, cx);
        assert_selection_ranges("useˇ std::str::{foo, bar}ˇ\n\n  {baz.qux()}", editor, cx);

        editor.move_to_next_word_end(&MoveToNextWordEnd, window, cx);
        assert_selection_ranges("use stdˇ::str::{foo, bar}\nˇ\n  {baz.qux()}", editor, cx);

        editor.move_to_next_word_end(&MoveToNextWordEnd, window, cx);
        assert_selection_ranges("use std::ˇstr::{foo, bar}\n\n  {ˇbaz.qux()}", editor, cx);

        editor.move_right(&MoveRight, window, cx);
        editor.select_to_previous_word_start(&SelectToPreviousWordStart, window, cx);
        assert_selection_ranges(
            "use std::«ˇs»tr::{foo, bar}\n\n  {«ˇb»az.qux()}",
            editor,
            cx,
        );

        editor.select_to_previous_word_start(&SelectToPreviousWordStart, window, cx);
        assert_selection_ranges(
            "use std«ˇ::s»tr::{foo, bar}\n\n  «ˇ{b»az.qux()}",
            editor,
            cx,
        );

        editor.select_to_next_word_end(&SelectToNextWordEnd, window, cx);
        assert_selection_ranges(
            "use std::«ˇs»tr::{foo, bar}\n\n  {«ˇb»az.qux()}",
            editor,
            cx,
        );
    });
}

#[gpui::test]
fn test_prev_next_word_bounds_with_soft_wrap(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("use one::{\n    two::three::four::five\n};", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.set_wrap_width(Some(140.0.into()), cx);
        assert_eq!(
            editor.display_text(cx),
            "use one::{\n    two::three::\n    four::five\n};"
        );

        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(1), 7)..DisplayPoint::new(DisplayRow(1), 7)
            ]);
        });

        editor.move_to_next_word_end(&MoveToNextWordEnd, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(1), 9)..DisplayPoint::new(DisplayRow(1), 9)]
        );

        editor.move_to_next_word_end(&MoveToNextWordEnd, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(1), 14)..DisplayPoint::new(DisplayRow(1), 14)]
        );

        editor.move_to_next_word_end(&MoveToNextWordEnd, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(2), 4)..DisplayPoint::new(DisplayRow(2), 4)]
        );

        editor.move_to_next_word_end(&MoveToNextWordEnd, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(2), 8)..DisplayPoint::new(DisplayRow(2), 8)]
        );

        editor.move_to_previous_word_start(&MoveToPreviousWordStart, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(2), 4)..DisplayPoint::new(DisplayRow(2), 4)]
        );

        editor.move_to_previous_word_start(&MoveToPreviousWordStart, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(1), 14)..DisplayPoint::new(DisplayRow(1), 14)]
        );
    });
}

#[gpui::test]
async fn test_move_start_of_paragraph_end_of_paragraph(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let line_height = cx.update_editor(|editor, window, cx| {
        editor
            .style(cx)
            .text
            .line_height_in_pixels(window.rem_size())
    });
    cx.simulate_window_resize(cx.window, size(px(100.), 4. * line_height));

    // The third line only contains a single space so we can later assert that the
    // editor's paragraph movement considers a non-blank line as a paragraph
    // boundary.
    cx.set_state(&"ˇone\ntwo\n \nthree\nfourˇ\nfive\n\nsix");

    cx.update_editor(|editor, window, cx| {
        editor.move_to_end_of_paragraph(&MoveToEndOfParagraph, window, cx)
    });
    cx.assert_editor_state(&"one\ntwo\nˇ \nthree\nfour\nfive\nˇ\nsix");

    cx.update_editor(|editor, window, cx| {
        editor.move_to_end_of_paragraph(&MoveToEndOfParagraph, window, cx)
    });
    cx.assert_editor_state(&"one\ntwo\n \nthree\nfour\nfive\nˇ\nsixˇ");

    cx.update_editor(|editor, window, cx| {
        editor.move_to_end_of_paragraph(&MoveToEndOfParagraph, window, cx)
    });
    cx.assert_editor_state(&"one\ntwo\n \nthree\nfour\nfive\n\nsixˇ");

    cx.update_editor(|editor, window, cx| {
        editor.move_to_start_of_paragraph(&MoveToStartOfParagraph, window, cx)
    });
    cx.assert_editor_state(&"one\ntwo\n \nthree\nfour\nfive\nˇ\nsix");

    cx.update_editor(|editor, window, cx| {
        editor.move_to_start_of_paragraph(&MoveToStartOfParagraph, window, cx)
    });

    cx.assert_editor_state(&"one\ntwo\nˇ \nthree\nfour\nfive\n\nsix");

    cx.update_editor(|editor, window, cx| {
        editor.move_to_start_of_paragraph(&MoveToStartOfParagraph, window, cx)
    });
    cx.assert_editor_state(&"ˇone\ntwo\n \nthree\nfour\nfive\n\nsix");
}

#[gpui::test]
async fn test_move_to_next_and_previous_comment_paragraph(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(
        Language::new(
            LanguageConfig {
                line_comments: vec!["// ".into()],
                ..LanguageConfig::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_override_query("[(line_comment)(block_comment)] @comment.inclusive")
        .unwrap(),
    );

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    // A blank comment line (`//`) splits one comment block into two paragraphs;
    // a code line splits blocks; and a trailing comment preceded by code
    // (`let x = 1; // ...`) is not a comment line at all.
    cx.set_state(indoc! {"
        ˇ// first paragraph line one
        // first paragraph line two
        //
        // second paragraph
        fn code() {}
        // third paragraph
        let x = 1; // trailing comment, ignored
        // fourth paragraph
    "});
    cx.run_until_parked();

    let next = |cx: &mut EditorTestContext| {
        cx.update_editor(|editor, window, cx| {
            editor.move_to_next_comment_paragraph(&MoveToNextCommentParagraph, window, cx)
        });
    };
    let prev = |cx: &mut EditorTestContext| {
        cx.update_editor(|editor, window, cx| {
            editor.move_to_previous_comment_paragraph(&MoveToPreviousCommentParagraph, window, cx)
        });
    };

    // Forward: skip the second line of the first paragraph, the blank comment
    // line, the code line, and the trailing comment line.
    next(&mut cx);
    cx.assert_editor_state(indoc! {"
        // first paragraph line one
        // first paragraph line two
        //
        ˇ// second paragraph
        fn code() {}
        // third paragraph
        let x = 1; // trailing comment, ignored
        // fourth paragraph
    "});
    next(&mut cx);
    cx.assert_editor_state(indoc! {"
        // first paragraph line one
        // first paragraph line two
        //
        // second paragraph
        fn code() {}
        ˇ// third paragraph
        let x = 1; // trailing comment, ignored
        // fourth paragraph
    "});
    next(&mut cx);
    cx.assert_editor_state(indoc! {"
        // first paragraph line one
        // first paragraph line two
        //
        // second paragraph
        fn code() {}
        // third paragraph
        let x = 1; // trailing comment, ignored
        ˇ// fourth paragraph
    "});
    // No paragraph after the last one: the caret stays put.
    next(&mut cx);
    cx.assert_editor_state(indoc! {"
        // first paragraph line one
        // first paragraph line two
        //
        // second paragraph
        fn code() {}
        // third paragraph
        let x = 1; // trailing comment, ignored
        ˇ// fourth paragraph
    "});

    // Backward is the mirror image.
    prev(&mut cx);
    cx.assert_editor_state(indoc! {"
        // first paragraph line one
        // first paragraph line two
        //
        // second paragraph
        fn code() {}
        ˇ// third paragraph
        let x = 1; // trailing comment, ignored
        // fourth paragraph
    "});
    prev(&mut cx);
    cx.assert_editor_state(indoc! {"
        // first paragraph line one
        // first paragraph line two
        //
        ˇ// second paragraph
        fn code() {}
        // third paragraph
        let x = 1; // trailing comment, ignored
        // fourth paragraph
    "});
    prev(&mut cx);
    cx.assert_editor_state(indoc! {"
        ˇ// first paragraph line one
        // first paragraph line two
        //
        // second paragraph
        fn code() {}
        // third paragraph
        let x = 1; // trailing comment, ignored
        // fourth paragraph
    "});
    // No paragraph before the first one: the caret stays put.
    prev(&mut cx);
    cx.assert_editor_state(indoc! {"
        ˇ// first paragraph line one
        // first paragraph line two
        //
        // second paragraph
        fn code() {}
        // third paragraph
        let x = 1; // trailing comment, ignored
        // fourth paragraph
    "});
}

#[gpui::test]
async fn test_move_to_previous_comment_paragraph_skips_current_paragraph(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(
        Language::new(
            LanguageConfig {
                line_comments: vec!["// ".into()],
                ..LanguageConfig::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_override_query("[(line_comment)(block_comment)] @comment.inclusive")
        .unwrap(),
    );

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    let prev = |cx: &mut EditorTestContext| {
        cx.update_editor(|editor, window, cx| {
            editor.move_to_previous_comment_paragraph(&MoveToPreviousCommentParagraph, window, cx)
        });
    };

    // Caret in the middle of the last line of the second paragraph: moving to
    // the previous paragraph must skip the entire current paragraph and land on
    // the first paragraph's start, not on the current paragraph's own start.
    cx.set_state(indoc! {"
        // alpha one
        // alpha two
        // alpha three

        // beta one
        // beta ˇtwo
    "});
    cx.run_until_parked();
    prev(&mut cx);
    cx.assert_editor_state(indoc! {"
        ˇ// alpha one
        // alpha two
        // alpha three

        // beta one
        // beta two
    "});

    // Same when the caret is part-way through the *first* line of the second
    // paragraph.
    cx.set_state(indoc! {"
        // alpha one
        // alpha two
        // alpha three

        // beˇta one
        // beta two
    "});
    cx.run_until_parked();
    prev(&mut cx);
    cx.assert_editor_state(indoc! {"
        ˇ// alpha one
        // alpha two
        // alpha three

        // beta one
        // beta two
    "});

    // Inside the first paragraph there is no previous paragraph, so the caret
    // stays put rather than jumping to the current paragraph's own start.
    cx.set_state(indoc! {"
        // alpha one
        // alpha ˇtwo
        // alpha three

        // beta one
        // beta two
    "});
    cx.run_until_parked();
    prev(&mut cx);
    cx.assert_editor_state(indoc! {"
        // alpha one
        // alpha ˇtwo
        // alpha three

        // beta one
        // beta two
    "});
}

#[gpui::test]
async fn test_scroll_page_up_page_down(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;
    let line_height = cx.update_editor(|editor, window, cx| {
        editor
            .style(cx)
            .text
            .line_height_in_pixels(window.rem_size())
    });
    let window = cx.window;
    cx.simulate_window_resize(window, size(px(1000.), 4. * line_height + px(0.5)));

    cx.set_state(
        r#"ˇone
        two
        three
        four
        five
        six
        seven
        eight
        nine
        ten
        "#,
    );

    cx.update_editor(|editor, window, cx| {
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 0.)
        );
        editor.scroll_screen(&ScrollAmount::Page(1.), window, cx);
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 3.)
        );
        editor.scroll_screen(&ScrollAmount::Page(1.), window, cx);
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 6.)
        );
        editor.scroll_screen(&ScrollAmount::Page(-1.), window, cx);
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 3.)
        );

        editor.scroll_screen(&ScrollAmount::Page(-0.5), window, cx);
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 1.)
        );
        editor.scroll_screen(&ScrollAmount::Page(0.5), window, cx);
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 3.)
        );
    });
}

#[gpui::test]
async fn test_newest_selection_on_screen_with_multibyte_chars(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let multibyte_line = "ã".repeat(40);
    cx.set_state(&format!("{multibyte_line}ˇx\n"));
    cx.update_editor(|editor, window, cx| {
        editor.set_visible_line_count(50., window, cx);
        editor.set_visible_column_count(60.);
        assert_eq!(
            editor.newest_selection_on_screen(window, cx),
            Ordering::Equal
        );
        editor.set_visible_column_count(20.);
        assert_eq!(
            editor.newest_selection_on_screen(window, cx),
            Ordering::Greater
        );
    });

    let ascii_line = "a".repeat(40);
    cx.set_state(&format!("{ascii_line}ˇx\n"));
    cx.update_editor(|editor, window, cx| {
        editor.set_visible_line_count(50., window, cx);
        editor.set_visible_column_count(60.);
        assert_eq!(
            editor.newest_selection_on_screen(window, cx),
            Ordering::Equal
        );
        editor.set_visible_column_count(20.);
        assert_eq!(
            editor.newest_selection_on_screen(window, cx),
            Ordering::Greater
        );
    });
}

#[gpui::test]
async fn test_scroll_line_up_down_cursor_margin(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;
    let line_height = cx.update_editor(|editor, window, cx| {
        editor.set_vertical_scroll_margin(2, cx);
        editor
            .style(cx)
            .text
            .line_height_in_pixels(window.rem_size())
    });
    let window = cx.window;
    // 5 visible lines with margin=2: valid cursor rows are [top+2 .. top+2] (only the middle row)
    cx.simulate_window_resize(window, size(px(1000.), 5. * line_height));

    // Cursor at row 0 — autoscroll leaves viewport at y=0.
    cx.set_state(indoc! {"
        ˇone
        two
        three
        four
        five
        six
        seven
        eight
        nine
        ten
    "});

    cx.update_editor(|editor, window, cx| {
        assert_eq!(editor.snapshot(window, cx).scroll_position().y, 0.);

        // Scroll down 1: top=1, visible rows 1-5, margin=2 → min_row=3, max_row=3.
        // Cursor at row 0 < min_row=3 → clamped to row 3.
        editor.scroll_screen_with_cursor_margin(&ScrollAmount::Line(1.), window, cx);
        assert_eq!(editor.snapshot(window, cx).scroll_position().y, 1.);
        let snapshot = editor.display_snapshot(cx);
        assert_eq!(
            editor.selections.newest_display(&snapshot).head().row().0,
            3,
            "cursor clamped to min_row after scrolling down"
        );
    });

    cx.update_editor(|editor, window, cx| {
        // Scroll up 1: top=0, visible rows 0-4, margin=2 (top==0 special case) → min_row=0, max_row=2.
        // Cursor at row 3 > max_row=2 → clamped to row 2.
        editor.scroll_screen_with_cursor_margin(&ScrollAmount::Line(-1.), window, cx);
        assert_eq!(editor.snapshot(window, cx).scroll_position().y, 0.);
        let snapshot = editor.display_snapshot(cx);
        assert_eq!(
            editor.selections.newest_display(&snapshot).head().row().0,
            2,
            "cursor clamped to max_row after scrolling up"
        );
    });

    cx.update_editor(|editor, window, cx| {
        // Half page down (2 lines): top=2, visible rows 2-6, margin=2 → min_row=4, max_row=4.
        // Cursor at row 2 < min_row=4 → clamped to row 4.
        editor.scroll_screen_with_cursor_margin(&ScrollAmount::Page(0.5), window, cx);
        assert_eq!(editor.snapshot(window, cx).scroll_position().y, 2.);
        let snapshot = editor.display_snapshot(cx);
        assert_eq!(
            editor.selections.newest_display(&snapshot).head().row().0,
            4,
            "cursor clamped to min_row after scrolling half page down"
        );
    });

    cx.update_editor(|editor, window, cx| {
        // Half page up (2 lines): top=0, visible rows 0-4, margin=2 (top==0 special case) → min_row=0, max_row=2.
        // Cursor at row 4 > max_row=2 → clamped to row 2.
        editor.scroll_screen_with_cursor_margin(&ScrollAmount::Page(-0.5), window, cx);
        assert_eq!(editor.snapshot(window, cx).scroll_position().y, 0.);
        let snapshot = editor.display_snapshot(cx);
        assert_eq!(
            editor.selections.newest_display(&snapshot).head().row().0,
            2,
            "cursor clamped to max_row after scrolling half page up"
        );
    });
}

#[gpui::test]
async fn test_autoscroll(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let line_height = cx.update_editor(|editor, window, cx| {
        editor.set_vertical_scroll_margin(2, cx);
        editor
            .style(cx)
            .text
            .line_height_in_pixels(window.rem_size())
    });
    let window = cx.window;
    cx.simulate_window_resize(window, size(px(1000.), 6. * line_height));

    cx.set_state(
        r#"ˇone
            two
            three
            four
            five
            six
            seven
            eight
            nine
            ten
        "#,
    );
    cx.update_editor(|editor, window, cx| {
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 0.0)
        );
    });

    // Add a cursor below the visible area. Since both cursors cannot fit
    // on screen, the editor autoscrolls to reveal the newest cursor, and
    // allows the vertical scroll margin below that cursor.
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([
                Point::new(0, 0)..Point::new(0, 0),
                Point::new(6, 0)..Point::new(6, 0),
            ]);
        })
    });
    cx.update_editor(|editor, window, cx| {
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 3.0)
        );
    });

    // Move down. The editor cursor scrolls down to track the newest cursor.
    cx.update_editor(|editor, window, cx| {
        editor.move_down(&Default::default(), window, cx);
    });
    cx.update_editor(|editor, window, cx| {
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 4.0)
        );
    });

    // Add a cursor above the visible area. Since both cursors fit on screen,
    // the editor scrolls to show both.
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([
                Point::new(1, 0)..Point::new(1, 0),
                Point::new(6, 0)..Point::new(6, 0),
            ]);
        })
    });
    cx.update_editor(|editor, window, cx| {
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 1.0)
        );
    });
}

#[gpui::test]
async fn test_autoscroll_relative(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let line_height = cx.update_editor(|editor, window, cx| {
        editor.set_vertical_scroll_margin(0, cx);
        editor
            .style(cx)
            .text
            .line_height_in_pixels(window.rem_size())
    });
    let window = cx.window;

    // Resize the window such that only 6 lines of text fit on screen.
    cx.simulate_window_resize(window, size(px(1000.), 6. * line_height));

    cx.set_state(
        r#"ˇone
            two
            three
            four
            five
            six
            seven
            eight
            nine
            ten
            eleven
            twelve
            thirteen
            fourteen
            fifteen
        "#,
    );
    cx.update_editor(|editor, window, cx| {
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 0.0)
        );
    });

    // Placing the cursor at row 7 with a top-relative autoscroll of 2 display
    // rows, should land the scroll position's y coordinate at 5.0 (7 - 2).
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(
            SelectionEffects::scroll(Autoscroll::top_relative(2.0)),
            window,
            cx,
            |selections| selections.select_ranges([Point::new(7, 0)..Point::new(7, 0)]),
        );
    });
    cx.update_editor(|editor, window, cx| {
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 5.0)
        );
    });

    // Seeing as fractional offsets are supported, with the cursor at row 10 and
    // a top-relative autoscroll of 2.5 display rows, the scroll position's y
    // coordinate lands at 7.5 (10 - 2.5).
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(
            SelectionEffects::scroll(Autoscroll::top_relative(2.5)),
            window,
            cx,
            |selections| selections.select_ranges([Point::new(10, 0)..Point::new(10, 0)]),
        );
    });
    cx.update_editor(|editor, window, cx| {
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 7.5)
        );
    });

    // When the requested offset would scroll past the top of the buffer,
    // `scroll_position.y` is clamped to 0 rather than going negative.
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(
            SelectionEffects::scroll(Autoscroll::top_relative(4.0)),
            window,
            cx,
            |selections| selections.select_ranges([Point::new(1, 0)..Point::new(1, 0)]),
        );
    });

    cx.update_editor(|editor, window, cx| {
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 0.0)
        );
    });
}

#[gpui::test]
async fn test_autoscroll_horizontally_long_selection_tracks_cursor(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let window = cx.window;
    // Narrow viewport so a long line can't possibly fit on screen at once.
    cx.simulate_window_resize(window, size(px(200.), px(300.)));

    let long_line = "x".repeat(250);
    cx.set_state(&format!("ˇ{long_line}"));

    cx.update_editor(|editor, window, cx| {
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 0.0)
        );
    });

    // Simulate shift+end: extend the selection from column 0 to the end
    // of the long line. Before the fix, this selection's span was wider
    // than the viewport, which caused autoscroll_horizontally to bail
    // out entirely and leave scroll_position.x at 0.
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(0, 0)..Point::new(0, 250)]);
        });
    });
    cx.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        let scroll_x = editor.snapshot(window, cx).scroll_position().x;
        let visible_columns = editor.visible_column_count().unwrap();
        let right_edge = scroll_x + visible_columns;
        assert!(
            scroll_x <= 250.,
            "head must not be scrolled past, scroll_x = {scroll_x}"
        );
        assert!(
            right_edge > 249.,
            "head column must be at the right edge, visible columns end at {right_edge}"
        );
    });
}

#[gpui::test]
async fn test_autoscroll_horizontally_padded_span_boundary_still_scrolls(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;
    let window = cx.window;

    // Long line so scroll_width stays large and never clamps target_right,
    // isolating the case below. Viewport is tuned so a 5-char selection's
    // raw glyph span fits, but its padded span (+ gutter margin + em_advance,
    // the same padding the final scroll guard uses) does not.
    cx.simulate_window_resize(window, size(px(146.), px(300.)));

    let line = "x".repeat(300);
    cx.set_state(&format!("ˇ{line}"));

    cx.update_editor(|editor, window, cx| {
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 0.0)
        );
    });

    // Select a short span in the middle of the line (e.g. simulating a
    // search match). Its raw glyph span fits the viewport, but the padded
    // span does not, so the fix must still scroll to reveal it.
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(0, 100)..Point::new(0, 105)]);
        });
    });
    cx.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        let scroll_x = editor.snapshot(window, cx).scroll_position().x;
        let visible_columns = editor.visible_column_count().unwrap();
        let right_edge = scroll_x + visible_columns;
        assert!(
            scroll_x <= 105.,
            "selection head must be visible, scroll_x = {scroll_x}"
        );
        assert!(
            right_edge > 105.,
            "selection head must be visible, visible columns end at {right_edge}"
        );
    });
}

#[gpui::test]
async fn test_autoscroll_horizontally_fitting_selection_reveals_full_span(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;
    let window = cx.window;

    // Viewport wide enough to fit a short selection's full span at once.
    cx.simulate_window_resize(window, size(px(200.), px(300.)));

    let line = "x".repeat(300);
    cx.set_state(&format!("ˇ{line}"));

    // Scroll right first so the whole line is out of view.
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(0, 300)..Point::new(0, 300)]);
        });
    });
    cx.run_until_parked();

    let scrolled_right_x = cx.update_editor(|editor, window, cx| {
        let scroll_x = editor.snapshot(window, cx).scroll_position().x;
        assert!(
            scroll_x > 200.,
            "should be scrolled far right, scroll_x = {scroll_x}"
        );
        scroll_x
    });

    // Select a short span in the middle of the line (e.g. simulating a
    // search match). Since it fits within the viewport, both edges of
    // the selection should end up visible, not just the head.
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(0, 100)..Point::new(0, 105)]);
        });
    });
    cx.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        let scroll_x = editor.snapshot(window, cx).scroll_position().x;
        let visible_columns = editor.visible_column_count().unwrap();
        let right_edge = scroll_x + visible_columns;
        assert!(
            scroll_x < scrolled_right_x,
            "must scroll left towards the selection, scroll_x = {scroll_x}"
        );
        assert!(
            scroll_x <= 100.5,
            "selection start must be visible, scroll_x = {scroll_x}"
        );
        assert!(
            right_edge > 105.,
            "selection end must be visible, visible columns end at {right_edge}"
        );
    });
}

#[gpui::test]
async fn test_exclude_overscroll_margin_clamps_scroll_position(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    update_test_editor_settings(cx, &|settings| {
        settings.scroll_beyond_last_line = Some(ScrollBeyondLastLine::OnePage);
    });

    let mut cx = EditorTestContext::new(cx).await;

    let line_height = cx.update_editor(|editor, window, cx| {
        editor.set_mode(EditorMode::Full {
            scale_ui_elements_with_buffer_font_size: false,
            show_active_line_background: false,
            sizing_behavior: SizingBehavior::ExcludeOverscrollMargin,
        });
        editor
            .style(cx)
            .text
            .line_height_in_pixels(window.rem_size())
    });
    let window = cx.window;
    cx.simulate_window_resize(window, size(px(1000.), 6. * line_height));
    cx.set_state(
        &r#"
        ˇone
        two
        three
        four
        five
        six
        seven
        eight
        nine
        ten
        eleven
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        let snapshot = editor.snapshot(window, cx);
        let max_scroll_top =
            (snapshot.max_point().row().as_f64() - editor.visible_line_count().unwrap() + 1.)
                .max(0.);

        editor.set_scroll_position(gpui::Point::new(0., max_scroll_top + 10.), window, cx);

        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., max_scroll_top)
        );
    });
}

#[gpui::test]
async fn test_move_page_up_page_down(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let line_height = cx.update_editor(|editor, window, cx| {
        editor
            .style(cx)
            .text
            .line_height_in_pixels(window.rem_size())
    });
    let window = cx.window;
    cx.simulate_window_resize(window, size(px(100.), 4. * line_height));
    cx.set_state(
        &r#"
        ˇone
        two
        threeˇ
        four
        five
        six
        seven
        eight
        nine
        ten
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.move_page_down(&MovePageDown::default(), window, cx)
    });
    cx.assert_editor_state(
        &r#"
        one
        two
        three
        ˇfour
        five
        sixˇ
        seven
        eight
        nine
        ten
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.move_page_down(&MovePageDown::default(), window, cx)
    });
    cx.assert_editor_state(
        &r#"
        one
        two
        three
        four
        five
        six
        ˇseven
        eight
        nineˇ
        ten
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| editor.move_page_up(&MovePageUp::default(), window, cx));
    cx.assert_editor_state(
        &r#"
        one
        two
        three
        ˇfour
        five
        sixˇ
        seven
        eight
        nine
        ten
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| editor.move_page_up(&MovePageUp::default(), window, cx));
    cx.assert_editor_state(
        &r#"
        ˇone
        two
        threeˇ
        four
        five
        six
        seven
        eight
        nine
        ten
        "#
        .unindent(),
    );

    // Test select collapsing
    cx.update_editor(|editor, window, cx| {
        editor.move_page_down(&MovePageDown::default(), window, cx);
        editor.move_page_down(&MovePageDown::default(), window, cx);
        editor.move_page_down(&MovePageDown::default(), window, cx);
    });
    cx.assert_editor_state(
        &r#"
        one
        two
        three
        four
        five
        six
        seven
        eight
        nine
        ˇten
        ˇ"#
        .unindent(),
    );
}

#[gpui::test]
async fn test_delete_to_line_boundary(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    for selection in ["one «two threeˇ» four", "one «ˇtwo three» four"] {
        cx.set_state(selection);
        cx.update_editor(|editor, window, cx| {
            editor.delete_to_beginning_of_line(
                &DeleteToBeginningOfLine {
                    stop_at_indent: false,
                },
                window,
                cx,
            );
            assert_eq!(editor.text(cx), " four");
        });

        cx.set_state(selection);
        cx.update_editor(|editor, window, cx| {
            editor.delete_to_end_of_line(&DeleteToEndOfLine, window, cx);
            assert_eq!(editor.text(cx), "one ");
        });
    }
}

#[gpui::test]
async fn test_delete_to_word_boundary(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    // For an empty selection, the preceding word fragment is deleted.
    // For non-empty selections, only selected characters are deleted.
    cx.set_state("onˇe two t«hreˇ»e four");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: false,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state("ˇe two tˇe four");

    cx.set_state("e tˇwo te «fˇ»our");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_next_word_end(
            &DeleteToNextWordEnd {
                ignore_newlines: false,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state("e tˇ te ˇour");
}

#[gpui::test]
async fn test_delete_whitespaces(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("here is some text    ˇwith a space");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: false,
                ignore_brackets: true,
            },
            window,
            cx,
        );
    });
    // Continuous whitespace sequences are removed entirely, words behind them are not affected by the deletion action.
    cx.assert_editor_state("here is some textˇwith a space");

    cx.set_state("here is some text    ˇwith a space");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: false,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state("here is some textˇwith a space");

    cx.set_state("here is some textˇ    with a space");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_next_word_end(
            &DeleteToNextWordEnd {
                ignore_newlines: false,
                ignore_brackets: true,
            },
            window,
            cx,
        );
    });
    // Same happens in the other direction.
    cx.assert_editor_state("here is some textˇwith a space");

    cx.set_state("here is some textˇ    with a space");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_next_word_end(
            &DeleteToNextWordEnd {
                ignore_newlines: false,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state("here is some textˇwith a space");

    cx.set_state("here is some textˇ    with a space");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_next_word_end(
            &DeleteToNextWordEnd {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state("here is some textˇwith a space");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state("here is some ˇwith a space");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    // Single whitespaces are removed with the word behind them.
    cx.assert_editor_state("here is ˇwith a space");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state("here ˇwith a space");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state("ˇwith a space");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state("ˇwith a space");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_next_word_end(
            &DeleteToNextWordEnd {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    // Same happens in the other direction.
    cx.assert_editor_state("ˇ a space");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_next_word_end(
            &DeleteToNextWordEnd {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state("ˇ space");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_next_word_end(
            &DeleteToNextWordEnd {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state("ˇ");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_next_word_end(
            &DeleteToNextWordEnd {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state("ˇ");
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state("ˇ");
}

#[gpui::test]
async fn test_delete_to_bracket(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(
        Language::new(
            LanguageConfig {
                brackets: BracketPairConfig {
                    pairs: vec![
                        BracketPair {
                            start: "\"".to_string(),
                            end: "\"".to_string(),
                            close: true,
                            surround: true,
                            newline: false,
                        },
                        BracketPair {
                            start: "(".to_string(),
                            end: ")".to_string(),
                            close: true,
                            surround: true,
                            newline: true,
                        },
                    ],
                    ..BracketPairConfig::default()
                },
                ..LanguageConfig::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_brackets_query(
            r#"
                ("(" @open ")" @close)
                ("\"" @open "\"" @close)
            "#,
        )
        .unwrap(),
    );

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    cx.set_state(r#"macro!("// ˇCOMMENT");"#);
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    // Deletion stops before brackets if asked to not ignore them.
    cx.assert_editor_state(r#"macro!("ˇCOMMENT");"#);
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    // Deletion has to remove a single bracket and then stop again.
    cx.assert_editor_state(r#"macro!(ˇCOMMENT");"#);

    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state(r#"macro!ˇCOMMENT");"#);

    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state(r#"ˇCOMMENT");"#);

    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state(r#"ˇCOMMENT");"#);

    cx.update_editor(|editor, window, cx| {
        editor.delete_to_next_word_end(
            &DeleteToNextWordEnd {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    // Brackets on the right are not paired anymore, hence deletion does not stop at them
    cx.assert_editor_state(r#"ˇ");"#);

    cx.update_editor(|editor, window, cx| {
        editor.delete_to_next_word_end(
            &DeleteToNextWordEnd {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state(r#"ˇ"#);

    cx.update_editor(|editor, window, cx| {
        editor.delete_to_next_word_end(
            &DeleteToNextWordEnd {
                ignore_newlines: true,
                ignore_brackets: false,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state(r#"ˇ"#);

    cx.set_state(r#"macro!("// ˇCOMMENT");"#);
    cx.update_editor(|editor, window, cx| {
        editor.delete_to_previous_word_start(
            &DeleteToPreviousWordStart {
                ignore_newlines: true,
                ignore_brackets: true,
            },
            window,
            cx,
        );
    });
    cx.assert_editor_state(r#"macroˇCOMMENT");"#);
}

#[gpui::test]
fn test_delete_to_previous_word_start_or_newline(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("one\n2\nthree\n4", cx);
        build_editor(buffer, window, cx)
    });
    let del_to_prev_word_start = DeleteToPreviousWordStart {
        ignore_newlines: false,
        ignore_brackets: false,
    };
    let del_to_prev_word_start_ignore_newlines = DeleteToPreviousWordStart {
        ignore_newlines: true,
        ignore_brackets: false,
    };

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(3), 1)..DisplayPoint::new(DisplayRow(3), 1)
            ])
        });
        editor.delete_to_previous_word_start(&del_to_prev_word_start, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "one\n2\nthree\n");
        editor.delete_to_previous_word_start(&del_to_prev_word_start, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "one\n2\nthree");
        editor.delete_to_previous_word_start(&del_to_prev_word_start, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "one\n2\n");
        editor.delete_to_previous_word_start(&del_to_prev_word_start, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "one\n2");
        editor.delete_to_previous_word_start(&del_to_prev_word_start_ignore_newlines, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "one\n");
        editor.delete_to_previous_word_start(&del_to_prev_word_start_ignore_newlines, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "");
    });
}

#[gpui::test]
fn test_delete_to_previous_subword_start_or_newline(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("fooBar\n\nbazQux", cx);
        build_editor(buffer, window, cx)
    });
    let del_to_prev_sub_word_start = DeleteToPreviousSubwordStart {
        ignore_newlines: false,
        ignore_brackets: false,
    };
    let del_to_prev_sub_word_start_ignore_newlines = DeleteToPreviousSubwordStart {
        ignore_newlines: true,
        ignore_brackets: false,
    };

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(2), 6)..DisplayPoint::new(DisplayRow(2), 6)
            ])
        });
        editor.delete_to_previous_subword_start(&del_to_prev_sub_word_start, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "fooBar\n\nbaz");
        editor.delete_to_previous_subword_start(&del_to_prev_sub_word_start, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "fooBar\n\n");
        editor.delete_to_previous_subword_start(&del_to_prev_sub_word_start, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "fooBar\n");
        editor.delete_to_previous_subword_start(&del_to_prev_sub_word_start, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "fooBar");
        editor.delete_to_previous_subword_start(&del_to_prev_sub_word_start, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "foo");
        editor.delete_to_previous_subword_start(
            &del_to_prev_sub_word_start_ignore_newlines,
            window,
            cx,
        );
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "");
    });
}

#[gpui::test]
fn test_delete_to_next_word_end_or_newline(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("\none\n   two\nthree\n   four", cx);
        build_editor(buffer, window, cx)
    });
    let del_to_next_word_end = DeleteToNextWordEnd {
        ignore_newlines: false,
        ignore_brackets: false,
    };
    let del_to_next_word_end_ignore_newlines = DeleteToNextWordEnd {
        ignore_newlines: true,
        ignore_brackets: false,
    };

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0)
            ])
        });
        editor.delete_to_next_word_end(&del_to_next_word_end, window, cx);
        assert_eq!(
            editor.buffer.read(cx).read(cx).text(),
            "one\n   two\nthree\n   four"
        );
        editor.delete_to_next_word_end(&del_to_next_word_end, window, cx);
        assert_eq!(
            editor.buffer.read(cx).read(cx).text(),
            "\n   two\nthree\n   four"
        );
        editor.delete_to_next_word_end(&del_to_next_word_end, window, cx);
        assert_eq!(
            editor.buffer.read(cx).read(cx).text(),
            "two\nthree\n   four"
        );
        editor.delete_to_next_word_end(&del_to_next_word_end, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "\nthree\n   four");
        editor.delete_to_next_word_end(&del_to_next_word_end_ignore_newlines, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "\n   four");
        editor.delete_to_next_word_end(&del_to_next_word_end_ignore_newlines, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "four");
        editor.delete_to_next_word_end(&del_to_next_word_end_ignore_newlines, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "");
    });
}

#[gpui::test]
fn test_delete_to_next_subword_end_or_newline(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("\nfooBar\n   bazQux", cx);
        build_editor(buffer, window, cx)
    });
    let del_to_next_subword_end = DeleteToNextSubwordEnd {
        ignore_newlines: false,
        ignore_brackets: false,
    };
    let del_to_next_subword_end_ignore_newlines = DeleteToNextSubwordEnd {
        ignore_newlines: true,
        ignore_brackets: false,
    };

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0)
            ])
        });
        // Delete "\n" (empty line)
        editor.delete_to_next_subword_end(&del_to_next_subword_end, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "fooBar\n   bazQux");
        // Delete "foo" (subword boundary)
        editor.delete_to_next_subword_end(&del_to_next_subword_end, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "Bar\n   bazQux");
        // Delete "Bar"
        editor.delete_to_next_subword_end(&del_to_next_subword_end, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "\n   bazQux");
        // Delete "\n   " (newline + leading whitespace)
        editor.delete_to_next_subword_end(&del_to_next_subword_end, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "bazQux");
        // Delete "baz" (subword boundary)
        editor.delete_to_next_subword_end(&del_to_next_subword_end, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "Qux");
        // With ignore_newlines, delete "Qux"
        editor.delete_to_next_subword_end(&del_to_next_subword_end_ignore_newlines, window, cx);
        assert_eq!(editor.buffer.read(cx).read(cx).text(), "");
    });
}

#[gpui::test]
fn test_newline(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaaa\n    bbbb\n", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 2),
                DisplayPoint::new(DisplayRow(1), 2)..DisplayPoint::new(DisplayRow(1), 2),
                DisplayPoint::new(DisplayRow(1), 6)..DisplayPoint::new(DisplayRow(1), 6),
            ])
        });

        editor.newline(&Newline, window, cx);
        assert_eq!(editor.text(cx), "aa\naa\n  \n    bb\n    bb\n");
    });
}

#[gpui::test]
fn test_newline_trailing_whitespace(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.auto_indent = Some(settings::AutoIndentMode::PreserveIndent);
    });

    let buffer = cx.update(|cx| MultiBuffer::build_simple("    hello\n    world\n", cx));
    let editor = cx.add_window(|window, cx| build_editor(buffer.clone(), window, cx));

    editor
        .update(cx, |editor, window, cx| {
            editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                s.select_display_ranges([
                    DisplayPoint::new(DisplayRow(0), 9)..DisplayPoint::new(DisplayRow(0), 9)
                ])
            });

            editor.newline(&Newline, window, cx);
            assert_eq!(editor.text(cx), "    hello\n    \n    world\n");

            editor.newline(&Newline, window, cx);
            assert_eq!(editor.text(cx), "    hello\n\n    \n    world\n");
        })
        .unwrap();

    buffer.update(cx, |buffer, cx| {
        let start = MultiBufferOffset(0);
        let end = buffer.len(cx);
        buffer.edit([(start..end, "    hello\n    world\n")], None, cx);
    });

    editor
        .update(cx, |editor, window, cx| {
            editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                s.select_display_ranges([
                    DisplayPoint::new(DisplayRow(0), 7)..DisplayPoint::new(DisplayRow(0), 7)
                ])
            });

            editor.newline(&Newline, window, cx);
            assert_eq!(editor.text(cx), "    hel\n    lo\n    world\n");

            editor.newline(&Newline, window, cx);
            assert_eq!(editor.text(cx), "    hel\n\n    lo\n    world\n");
        })
        .unwrap();

    update_test_language_settings(cx, &|settings| {
        settings.defaults.tab_size = NonZeroU32::new(4);
        settings.defaults.hard_tabs = Some(true);
    });

    buffer.update(cx, |buffer, cx| {
        let start = MultiBufferOffset(0);
        let end = buffer.len(cx);
        buffer.edit([(start..end, "\thello\n\tworld\n")], None, cx);
    });

    editor
        .update(cx, |editor, window, cx| {
            editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                s.select_display_ranges([
                    DisplayPoint::new(DisplayRow(0), 9)..DisplayPoint::new(DisplayRow(0), 9)
                ])
            });

            editor.newline(&Newline, window, cx);
            assert_eq!(editor.text(cx), "\thello\n\t\n\tworld\n");

            editor.newline(&Newline, window, cx);
            assert_eq!(editor.text(cx), "\thello\n\n\t\n\tworld\n");
        })
        .unwrap();
}

#[gpui::test]
async fn test_newline_yaml(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    let yaml_language = languages::language("yaml", tree_sitter_yaml::LANGUAGE.into());
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(yaml_language), cx));

    // Object (between 2 fields)
    cx.set_state(indoc! {"
    test:ˇ
    hello: bye"});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.assert_editor_state(indoc! {"
    test:
        ˇ
    hello: bye"});

    // Object (first and single line)
    cx.set_state(indoc! {"
    test:ˇ"});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.assert_editor_state(indoc! {"
    test:
        ˇ"});

    // Array with objects (after first element)
    cx.set_state(indoc! {"
    test:
        - foo: barˇ"});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.assert_editor_state(indoc! {"
    test:
        - foo: bar
        ˇ"});

    // Array with objects and comment
    cx.set_state(indoc! {"
    test:
        - foo: bar
        - bar: # testˇ"});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.assert_editor_state(indoc! {"
    test:
        - foo: bar
        - bar: # test
            ˇ"});

    // Array with objects (after second element)
    cx.set_state(indoc! {"
    test:
        - foo: bar
        - bar: fooˇ"});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.assert_editor_state(indoc! {"
    test:
        - foo: bar
        - bar: foo
        ˇ"});

    // Array with strings (after first element)
    cx.set_state(indoc! {"
    test:
        - fooˇ"});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.assert_editor_state(indoc! {"
    test:
        - foo
        ˇ"});
}

#[gpui::test]
fn test_newline_with_old_selections(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple(
            "
                a
                b(
                    X
                )
                c(
                    X
                )
            "
            .unindent()
            .as_str(),
            cx,
        );
        let mut editor = build_editor(buffer, window, cx);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([
                Point::new(2, 4)..Point::new(2, 5),
                Point::new(5, 4)..Point::new(5, 5),
            ])
        });
        editor
    });

    _ = editor.update(cx, |editor, window, cx| {
        // Edit the buffer directly, deleting ranges surrounding the editor's selections
        editor.buffer.update(cx, |buffer, cx| {
            buffer.edit(
                [
                    (Point::new(1, 2)..Point::new(3, 0), ""),
                    (Point::new(4, 2)..Point::new(6, 0), ""),
                ],
                None,
                cx,
            );
            assert_eq!(
                buffer.read(cx).text(),
                "
                    a
                    b()
                    c()
                "
                .unindent()
            );
        });
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            &[
                Point::new(1, 2)..Point::new(1, 2),
                Point::new(2, 2)..Point::new(2, 2),
            ],
        );

        editor.newline(&Newline, window, cx);
        assert_eq!(
            editor.text(cx),
            "
                a
                b(
                )
                c(
                )
            "
            .unindent()
        );

        // The selections are moved after the inserted newlines
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            &[
                Point::new(2, 0)..Point::new(2, 0),
                Point::new(4, 0)..Point::new(4, 0),
            ],
        );
    });
}

#[gpui::test]
async fn test_newline_above(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(4)
    });

    let language = Arc::new(
        Language::new(
            LanguageConfig::default(),
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_indents_query(r#"(_ "(" ")" @end) @indent"#)
        .unwrap(),
    );

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
    cx.set_state(indoc! {"
        const a: ˇA = (
            (ˇ
                «const_functionˇ»(ˇ),
                so«mˇ»et«hˇ»ing_ˇelse,ˇ
            )ˇ
        ˇ);ˇ
    "});

    cx.update_editor(|e, window, cx| e.newline_above(&NewlineAbove, window, cx));
    cx.assert_editor_state(indoc! {"
        ˇ
        const a: A = (
            ˇ
            (
                ˇ
                ˇ
                const_function(),
                ˇ
                ˇ
                ˇ
                ˇ
                something_else,
                ˇ
            )
            ˇ
            ˇ
        );
    "});
}

#[gpui::test]
async fn test_newline_below(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(4)
    });

    let language = Arc::new(
        Language::new(
            LanguageConfig::default(),
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_indents_query(r#"(_ "(" ")" @end) @indent"#)
        .unwrap(),
    );

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
    cx.set_state(indoc! {"
        const a: ˇA = (
            (ˇ
                «const_functionˇ»(ˇ),
                so«mˇ»et«hˇ»ing_ˇelse,ˇ
            )ˇ
        ˇ);ˇ
    "});

    cx.update_editor(|e, window, cx| e.newline_below(&NewlineBelow, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: A = (
            ˇ
            (
                ˇ
                const_function(),
                ˇ
                ˇ
                something_else,
                ˇ
                ˇ
                ˇ
                ˇ
            )
            ˇ
        );
        ˇ
        ˇ
    "});
}

#[gpui::test]
fn test_newline_respects_read_only(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaaa\nbbbb\n", cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.set_read_only(true);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 2)
            ])
        });

        editor.newline(&Newline, window, cx);
        assert_eq!(
            editor.text(cx),
            "aaaa\nbbbb\n",
            "newline should not modify a read-only editor"
        );

        editor.newline_above(&NewlineAbove, window, cx);
        assert_eq!(
            editor.text(cx),
            "aaaa\nbbbb\n",
            "newline_above should not modify a read-only editor"
        );

        editor.newline_below(&NewlineBelow, window, cx);
        assert_eq!(
            editor.text(cx),
            "aaaa\nbbbb\n",
            "newline_below should not modify a read-only editor"
        );
    });
}

#[gpui::test]
async fn test_newline_below_with_cursor_on_deleted_hunk(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("aaa\nbbb\ncˇcc");
    cx.set_head_text("aaa\nXXX\nbbb\nccc");
    cx.run_until_parked();
    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&Default::default(), window, cx);
    });
    cx.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0),
                DisplayPoint::new(DisplayRow(3), 3)..DisplayPoint::new(DisplayRow(3), 3),
            ]);
        });
    });

    cx.update_editor(|editor, window, cx| {
        editor.newline_below(&NewlineBelow, window, cx);
    });
    cx.run_until_parked();

    assert_eq!(cx.buffer(|buffer, _| buffer.text()), "aaa\nbbb\nccc\n");

    let cursors = cx.update_editor(|editor, window, cx| {
        let display_snapshot = editor.snapshot(window, cx).display_snapshot;
        editor
            .selections
            .all_display(&display_snapshot)
            .iter()
            .map(|selection| selection.head())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        cursors,
        vec![
            DisplayPoint::new(DisplayRow(1), 0),
            DisplayPoint::new(DisplayRow(4), 0),
        ],
    );
}

#[gpui::test]
fn test_newline_below_multibuffer(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let buffer_1 = cx.new(|cx| Buffer::local("aaa\nbbb\nccc", cx));
    let buffer_2 = cx.new(|cx| Buffer::local("ddd\neee\nfff", cx));
    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1.clone(),
            [Point::new(0, 0)..Point::new(2, 3)],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(1),
            buffer_2.clone(),
            [Point::new(0, 0)..Point::new(2, 3)],
            0,
            cx,
        );
        multibuffer
    });

    cx.add_window(|window, cx| {
        let mut editor = build_editor(multibuffer, window, cx);

        assert_eq!(
            editor.text(cx),
            indoc! {"
                aaa
                bbb
                ccc
                ddd
                eee
                fff"}
        );

        // Cursor on the last line of the first excerpt.
        // The newline should be inserted within the first excerpt (buffer_1),
        // not in the second excerpt (buffer_2).
        select_ranges(
            &mut editor,
            indoc! {"
                aaa
                bbb
                cˇcc
                ddd
                eee
                fff"},
            window,
            cx,
        );
        editor.newline_below(&NewlineBelow, window, cx);
        assert_text_with_selections(
            &mut editor,
            indoc! {"
                aaa
                bbb
                ccc
                ˇ
                ddd
                eee
                fff"},
            cx,
        );
        buffer_1.read_with(cx, |buffer, _| {
            assert_eq!(buffer.text(), "aaa\nbbb\nccc\n");
        });
        buffer_2.read_with(cx, |buffer, _| {
            assert_eq!(buffer.text(), "ddd\neee\nfff");
        });

        editor
    });
}

#[gpui::test]
fn test_newline_below_multibuffer_middle_of_excerpt(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let buffer_1 = cx.new(|cx| Buffer::local("aaa\nbbb\nccc", cx));
    let buffer_2 = cx.new(|cx| Buffer::local("ddd\neee\nfff", cx));
    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1.clone(),
            [Point::new(0, 0)..Point::new(2, 3)],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(1),
            buffer_2.clone(),
            [Point::new(0, 0)..Point::new(2, 3)],
            0,
            cx,
        );
        multibuffer
    });

    cx.add_window(|window, cx| {
        let mut editor = build_editor(multibuffer, window, cx);

        // Cursor in the middle of the first excerpt.
        select_ranges(
            &mut editor,
            indoc! {"
                aˇaa
                bbb
                ccc
                ddd
                eee
                fff"},
            window,
            cx,
        );
        editor.newline_below(&NewlineBelow, window, cx);
        assert_text_with_selections(
            &mut editor,
            indoc! {"
                aaa
                ˇ
                bbb
                ccc
                ddd
                eee
                fff"},
            cx,
        );
        buffer_1.read_with(cx, |buffer, _| {
            assert_eq!(buffer.text(), "aaa\n\nbbb\nccc");
        });
        buffer_2.read_with(cx, |buffer, _| {
            assert_eq!(buffer.text(), "ddd\neee\nfff");
        });

        editor
    });
}

#[gpui::test]
fn test_newline_below_multibuffer_last_line_of_last_excerpt(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let buffer_1 = cx.new(|cx| Buffer::local("aaa\nbbb\nccc", cx));
    let buffer_2 = cx.new(|cx| Buffer::local("ddd\neee\nfff", cx));
    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1.clone(),
            [Point::new(0, 0)..Point::new(2, 3)],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(1),
            buffer_2.clone(),
            [Point::new(0, 0)..Point::new(2, 3)],
            0,
            cx,
        );
        multibuffer
    });

    cx.add_window(|window, cx| {
        let mut editor = build_editor(multibuffer, window, cx);

        // Cursor on the last line of the last excerpt.
        select_ranges(
            &mut editor,
            indoc! {"
                aaa
                bbb
                ccc
                ddd
                eee
                fˇff"},
            window,
            cx,
        );
        editor.newline_below(&NewlineBelow, window, cx);
        assert_text_with_selections(
            &mut editor,
            indoc! {"
                aaa
                bbb
                ccc
                ddd
                eee
                fff
                ˇ"},
            cx,
        );
        buffer_1.read_with(cx, |buffer, _| {
            assert_eq!(buffer.text(), "aaa\nbbb\nccc");
        });
        buffer_2.read_with(cx, |buffer, _| {
            assert_eq!(buffer.text(), "ddd\neee\nfff\n");
        });

        editor
    });
}

#[gpui::test]
fn test_newline_below_multibuffer_multiple_cursors(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let buffer_1 = cx.new(|cx| Buffer::local("aaa\nbbb\nccc", cx));
    let buffer_2 = cx.new(|cx| Buffer::local("ddd\neee\nfff", cx));
    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1.clone(),
            [Point::new(0, 0)..Point::new(2, 3)],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(1),
            buffer_2.clone(),
            [Point::new(0, 0)..Point::new(2, 3)],
            0,
            cx,
        );
        multibuffer
    });

    cx.add_window(|window, cx| {
        let mut editor = build_editor(multibuffer, window, cx);

        // Cursors on the last line of the first excerpt and the first line
        // of the second excerpt. Each newline should go into its respective buffer.
        select_ranges(
            &mut editor,
            indoc! {"
                aaa
                bbb
                cˇcc
                dˇdd
                eee
                fff"},
            window,
            cx,
        );
        editor.newline_below(&NewlineBelow, window, cx);
        assert_text_with_selections(
            &mut editor,
            indoc! {"
                aaa
                bbb
                ccc
                ˇ
                ddd
                ˇ
                eee
                fff"},
            cx,
        );
        buffer_1.read_with(cx, |buffer, _| {
            assert_eq!(buffer.text(), "aaa\nbbb\nccc\n");
        });
        buffer_2.read_with(cx, |buffer, _| {
            assert_eq!(buffer.text(), "ddd\n\neee\nfff");
        });

        editor
    });
}

#[gpui::test]
async fn test_newline_comments(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(4)
    });

    let language = Arc::new(Language::new(
        LanguageConfig {
            line_comments: vec!["// ".into()],
            ..LanguageConfig::default()
        },
        None,
    ));
    {
        let mut cx = EditorTestContext::new(cx).await;
        cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
        cx.set_state(indoc! {"
        // Fooˇ
    "});

        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        // Foo
        // ˇ
    "});
        // Ensure that we add comment prefix when existing line contains space
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(
            indoc! {"
        // Foo
        //s
        // ˇ
    "}
            .replace("s", " ") // s is used as space placeholder to prevent format on save
            .as_str(),
        );
        // Ensure that we add comment prefix when existing line does not contain space
        cx.set_state(indoc! {"
        // Foo
        //ˇ
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        // Foo
        //
        // ˇ
    "});
        // Ensure that if cursor is before the comment start, we do not actually insert a comment prefix.
        cx.set_state(indoc! {"
        ˇ// Foo
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"

        ˇ// Foo
    "});
    }
    // Ensure that comment continuations can be disabled.
    update_test_language_settings(cx, &|settings| {
        settings.defaults.extend_comment_on_newline = Some(false);
    });
    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(indoc! {"
        // Fooˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.assert_editor_state(indoc! {"
        // Foo
        ˇ
    "});
}

#[gpui::test]
async fn test_newline_comments_with_multiple_delimiters(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(4)
    });

    let language = Arc::new(Language::new(
        LanguageConfig {
            line_comments: vec!["// ".into(), "/// ".into()],
            ..LanguageConfig::default()
        },
        None,
    ));
    {
        let mut cx = EditorTestContext::new(cx).await;
        cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
        cx.set_state(indoc! {"
        //ˇ
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        //
        // ˇ
    "});

        cx.set_state(indoc! {"
        ///ˇ
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        ///
        /// ˇ
    "});
    }
}

#[gpui::test]
async fn test_newline_comments_with_brackets(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(4)
    });
    let language = Arc::new(Language::new(
        LanguageConfig {
            line_comments: vec!["// ".into()],
            brackets: BracketPairConfig {
                pairs: vec![BracketPair {
                    start: "(".to_string(),
                    end: ")".to_string(),
                    close: false,
                    surround: false,
                    newline: true,
                }],
                ..BracketPairConfig::default()
            },
            ..LanguageConfig::default()
        },
        None,
    ));

    {
        let mut cx = EditorTestContext::new(cx).await;
        cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
        cx.set_state(indoc! {"
        // (ˇ)
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        // (
        // ˇ)
    "})
    }
}

#[gpui::test]
async fn test_newline_comments_repl_separators(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(4)
    });
    let language = Arc::new(Language::new(
        LanguageConfig {
            line_comments: vec!["# ".into()],
            ..LanguageConfig::default()
        },
        None,
    ));

    {
        let mut cx = EditorTestContext::new(cx).await;
        cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
        cx.set_state(indoc! {"
        # %%ˇ
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        # %%
        ˇ
    "});

        cx.set_state(indoc! {"
            # %%%%%ˇ
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
            # %%%%%
            ˇ
    "});

        cx.set_state(indoc! {"
            # %ˇ%
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
            # %
            # ˇ%
    "});
    }
}

#[gpui::test]
async fn test_newline_documentation_comments(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(4)
    });

    let language = Arc::new(
        Language::new(
            LanguageConfig {
                documentation_comment: Some(language::BlockCommentConfig {
                    start: "/**".into(),
                    end: "*/".into(),
                    prefix: "* ".into(),
                    tab_size: 1,
                }),

                ..LanguageConfig::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_override_query("[(line_comment)(block_comment)] @comment.inclusive")
        .unwrap(),
    );

    {
        let mut cx = EditorTestContext::new(cx).await;
        cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
        cx.set_state(indoc! {"
        /**ˇ
    "});

        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        /**
         * ˇ
    "});
        // Ensure that if cursor is before the comment start,
        // we do not actually insert a comment prefix.
        cx.set_state(indoc! {"
        ˇ/**
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"

        ˇ/**
    "});
        // Ensure that if cursor is between it doesn't add comment prefix.
        cx.set_state(indoc! {"
        /*ˇ*
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        /*
        ˇ*
    "});
        // Ensure that if suffix exists on same line after cursor it adds new line.
        cx.set_state(indoc! {"
        /**ˇ*/
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        /**
         * ˇ
         */
    "});
        // Ensure that if suffix exists on same line after cursor with space it adds new line.
        cx.set_state(indoc! {"
        /**ˇ */
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        /**
         * ˇ
         */
    "});
        // Ensure that if suffix exists on same line after cursor with space it adds new line.
        cx.set_state(indoc! {"
        /** ˇ*/
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(
            indoc! {"
        /**s
         * ˇ
         */
    "}
            .replace("s", " ") // s is used as space placeholder to prevent format on save
            .as_str(),
        );
        // Ensure that delimiter space is preserved when newline on already
        // spaced delimiter.
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(
            indoc! {"
        /**s
         *s
         * ˇ
         */
    "}
            .replace("s", " ") // s is used as space placeholder to prevent format on save
            .as_str(),
        );
        // Ensure that delimiter space is preserved when space is not
        // on existing delimiter.
        cx.set_state(indoc! {"
        /**
         *ˇ
         */
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        /**
         *
         * ˇ
         */
    "});
        // Ensure that if suffix exists on same line after cursor it
        // doesn't add extra new line if prefix is not on same line.
        cx.set_state(indoc! {"
        /**
        ˇ*/
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        /**

        ˇ*/
    "});
        // Ensure that it detects suffix after existing prefix.
        cx.set_state(indoc! {"
        /**ˇ/
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        /**
        ˇ/
    "});
        // Ensure that if suffix exists on same line before
        // cursor it does not add comment prefix.
        cx.set_state(indoc! {"
        /** */ˇ
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        /** */
        ˇ
    "});
        // Ensure that if suffix exists on same line before
        // cursor it does not add comment prefix.
        cx.set_state(indoc! {"
        /**
         *
         */ˇ
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        /**
         *
         */
         ˇ
    "});

        // Ensure that inline comment followed by code
        // doesn't add comment prefix on newline
        cx.set_state(indoc! {"
        /** */ textˇ
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        /** */ text
        ˇ
    "});

        // Ensure that text after comment end tag
        // doesn't add comment prefix on newline
        cx.set_state(indoc! {"
        /**
         *
         */ˇtext
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        /**
         *
         */
         ˇtext
    "});

        // Ensure if not comment block it doesn't
        // add comment prefix on newline
        cx.set_state(indoc! {"
        * textˇ
    "});
        cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
        cx.assert_editor_state(indoc! {"
        * text
        ˇ
    "});
    }
    // Ensure that comment continuations can be disabled.
    update_test_language_settings(cx, &|settings| {
        settings.defaults.extend_comment_on_newline = Some(false);
    });
    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(indoc! {"
        /**ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.assert_editor_state(indoc! {"
        /**
        ˇ
    "});
}

#[gpui::test]
async fn test_newline_comments_with_block_comment(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(4)
    });

    let lua_language = Arc::new(Language::new(
        LanguageConfig {
            line_comments: vec!["--".into()],
            block_comment: Some(language::BlockCommentConfig {
                start: "--[[".into(),
                prefix: "".into(),
                end: "]]".into(),
                tab_size: 0,
            }),
            ..LanguageConfig::default()
        },
        None,
    ));

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(lua_language), cx));

    // Line with line comment should extend
    cx.set_state(indoc! {"
        --ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.assert_editor_state(indoc! {"
        --
        --ˇ
    "});

    // Line with block comment that matches line comment should not extend
    cx.set_state(indoc! {"
        --[[ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.assert_editor_state(indoc! {"
        --[[
        ˇ
    "});
}

#[gpui::test]
fn test_insert_with_old_selections(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("a( X ), b( Y ), c( Z )", cx);
        let mut editor = build_editor(buffer, window, cx);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([
                MultiBufferOffset(3)..MultiBufferOffset(4),
                MultiBufferOffset(11)..MultiBufferOffset(12),
                MultiBufferOffset(19)..MultiBufferOffset(20),
            ])
        });
        editor
    });

    _ = editor.update(cx, |editor, window, cx| {
        // Edit the buffer directly, deleting ranges surrounding the editor's selections
        editor.buffer.update(cx, |buffer, cx| {
            buffer.edit(
                [
                    (MultiBufferOffset(2)..MultiBufferOffset(5), ""),
                    (MultiBufferOffset(10)..MultiBufferOffset(13), ""),
                    (MultiBufferOffset(18)..MultiBufferOffset(21), ""),
                ],
                None,
                cx,
            );
            assert_eq!(buffer.read(cx).text(), "a(), b(), c()".unindent());
        });
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            &[
                MultiBufferOffset(2)..MultiBufferOffset(2),
                MultiBufferOffset(7)..MultiBufferOffset(7),
                MultiBufferOffset(12)..MultiBufferOffset(12)
            ],
        );

        editor.insert("Z", window, cx);
        assert_eq!(editor.text(cx), "a(Z), b(Z), c(Z)");

        // The selections are moved after the inserted characters
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            &[
                MultiBufferOffset(3)..MultiBufferOffset(3),
                MultiBufferOffset(9)..MultiBufferOffset(9),
                MultiBufferOffset(15)..MultiBufferOffset(15)
            ],
        );
    });
}

#[gpui::test]
async fn test_tab(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(3)
    });

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(indoc! {"
        ˇabˇc
        ˇ🏀ˇ🏀ˇefg
        dˇ
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
           ˇab ˇc
           ˇ🏀  ˇ🏀  ˇefg
        d  ˇ
    "});

    cx.set_state(indoc! {"
        a
        «🏀ˇ»🏀«🏀ˇ»🏀«🏀ˇ»
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        a
           «🏀ˇ»🏀«🏀ˇ»🏀«🏀ˇ»
    "});
}

#[gpui::test]
async fn test_tab_in_leading_whitespace_auto_indents_lines(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    let language = Arc::new(
        Language::new(
            LanguageConfig::default(),
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_indents_query(r#"(_ "(" ")" @end) @indent"#)
        .unwrap(),
    );
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    // test when all cursors are not at suggested indent
    // then simply move to their suggested indent location
    cx.set_state(indoc! {"
        const a: B = (
            c(
        ˇ
        ˇ    )
        );
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: B = (
            c(
                ˇ
            ˇ)
        );
    "});

    // test cursor already at suggested indent not moving when
    // other cursors are yet to reach their suggested indents
    cx.set_state(indoc! {"
        ˇ
        const a: B = (
            c(
                d(
        ˇ
                )
        ˇ
        ˇ    )
        );
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        ˇ
        const a: B = (
            c(
                d(
                    ˇ
                )
                ˇ
            ˇ)
        );
    "});
    // test when all cursors are at suggested indent then tab is inserted
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
            ˇ
        const a: B = (
            c(
                d(
                        ˇ
                )
                    ˇ
                ˇ)
        );
    "});

    // test when current indent is less than suggested indent,
    // we adjust line to match suggested indent and move cursor to it
    //
    // when no other cursor is at word boundary, all of them should move
    cx.set_state(indoc! {"
        const a: B = (
            c(
                d(
        ˇ
        ˇ   )
        ˇ   )
        );
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: B = (
            c(
                d(
                    ˇ
                ˇ)
            ˇ)
        );
    "});

    // test when current indent is less than suggested indent,
    // we adjust line to match suggested indent and move cursor to it
    //
    // when some other cursor is at word boundary, it should not move
    cx.set_state(indoc! {"
        const a: B = (
            c(
                d(
        ˇ
        ˇ   )
           ˇ)
        );
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: B = (
            c(
                d(
                    ˇ
                ˇ)
            ˇ)
        );
    "});

    // test when current indent is more than suggested indent,
    // we just move cursor to current indent instead of suggested indent
    //
    // when no other cursor is at word boundary, all of them should move
    cx.set_state(indoc! {"
        const a: B = (
            c(
                d(
        ˇ
        ˇ                )
        ˇ   )
        );
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: B = (
            c(
                d(
                    ˇ
                        ˇ)
            ˇ)
        );
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: B = (
            c(
                d(
                        ˇ
                            ˇ)
                ˇ)
        );
    "});

    // test when current indent is more than suggested indent,
    // we just move cursor to current indent instead of suggested indent
    //
    // when some other cursor is at word boundary, it doesn't move
    cx.set_state(indoc! {"
        const a: B = (
            c(
                d(
        ˇ
        ˇ                )
            ˇ)
        );
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: B = (
            c(
                d(
                    ˇ
                        ˇ)
            ˇ)
        );
    "});

    // handle auto-indent when there are multiple cursors on the same line
    cx.set_state(indoc! {"
        const a: B = (
            c(
        ˇ    ˇ
        ˇ    )
        );
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: B = (
            c(
                ˇ
            ˇ)
        );
    "});
}

#[gpui::test]
async fn test_tab_with_mixed_whitespace_txt(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(3)
    });

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(indoc! {"
         ˇ
        \t ˇ
        \t  ˇ
        \t   ˇ
         \t  \t\t \t      \t\t   \t\t    \t \t ˇ
    "});

    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
           ˇ
        \t   ˇ
        \t   ˇ
        \t      ˇ
         \t  \t\t \t      \t\t   \t\t    \t \t   ˇ
    "});
}

#[gpui::test]
async fn test_tab_with_mixed_whitespace_rust(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(4)
    });

    let language = Arc::new(
        Language::new(
            LanguageConfig::default(),
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_indents_query(r#"(_ "{" "}" @end) @indent"#)
        .unwrap(),
    );

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
    cx.set_state(indoc! {"
        fn a() {
            if b {
        \t ˇc
            }
        }
    "});

    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        fn a() {
            if b {
                ˇc
            }
        }
    "});
}

#[gpui::test]
async fn test_indent_outdent(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(4);
    });

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"
          «oneˇ» «twoˇ»
        three
         four
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
            «oneˇ» «twoˇ»
        three
         four
    "});

    cx.update_editor(|e, window, cx| e.backtab(&Backtab, window, cx));
    cx.assert_editor_state(indoc! {"
        «oneˇ» «twoˇ»
        three
         four
    "});

    // select across line ending
    cx.set_state(indoc! {"
        one two
        t«hree
        ˇ» four
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        one two
            t«hree
        ˇ» four
    "});

    cx.update_editor(|e, window, cx| e.backtab(&Backtab, window, cx));
    cx.assert_editor_state(indoc! {"
        one two
        t«hree
        ˇ» four
    "});

    // Ensure that indenting/outdenting works when the cursor is at column 0.
    cx.set_state(indoc! {"
        one two
        ˇthree
            four
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        one two
            ˇthree
            four
    "});

    cx.set_state(indoc! {"
        one two
        ˇ    three
            four
    "});
    cx.update_editor(|e, window, cx| e.backtab(&Backtab, window, cx));
    cx.assert_editor_state(indoc! {"
        one two
        ˇthree
            four
    "});
}

#[gpui::test]
async fn test_indent_yaml_comments_with_multiple_cursors(cx: &mut TestAppContext) {
    // This is a regression test for issue #33761
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    let yaml_language = languages::language("yaml", tree_sitter_yaml::LANGUAGE.into());
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(yaml_language), cx));

    cx.set_state(
        r#"ˇ#     ingress:
ˇ#         api:
ˇ#             enabled: false
ˇ#             pathType: Prefix
ˇ#           console:
ˇ#               enabled: false
ˇ#               pathType: Prefix
"#,
    );

    // Press tab to indent all lines
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));

    cx.assert_editor_state(
        r#"    ˇ#     ingress:
    ˇ#         api:
    ˇ#             enabled: false
    ˇ#             pathType: Prefix
    ˇ#           console:
    ˇ#               enabled: false
    ˇ#               pathType: Prefix
"#,
    );
}

#[gpui::test]
async fn test_indent_yaml_non_comments_with_multiple_cursors(cx: &mut TestAppContext) {
    // This is a test to make sure our fix for issue #33761 didn't break anything
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    let yaml_language = languages::language("yaml", tree_sitter_yaml::LANGUAGE.into());
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(yaml_language), cx));

    cx.set_state(
        r#"ˇingress:
ˇ  api:
ˇ    enabled: false
ˇ    pathType: Prefix
"#,
    );

    // Press tab to indent all lines
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));

    cx.assert_editor_state(
        r#"ˇingress:
    ˇapi:
        ˇenabled: false
        ˇpathType: Prefix
"#,
    );
}

#[gpui::test]
async fn test_indent_outdent_with_hard_tabs(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.hard_tabs = Some(true);
    });

    let mut cx = EditorTestContext::new(cx).await;

    // select two ranges on one line
    cx.set_state(indoc! {"
        «oneˇ» «twoˇ»
        three
        four
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        \t«oneˇ» «twoˇ»
        three
        four
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        \t\t«oneˇ» «twoˇ»
        three
        four
    "});
    cx.update_editor(|e, window, cx| e.backtab(&Backtab, window, cx));
    cx.assert_editor_state(indoc! {"
        \t«oneˇ» «twoˇ»
        three
        four
    "});
    cx.update_editor(|e, window, cx| e.backtab(&Backtab, window, cx));
    cx.assert_editor_state(indoc! {"
        «oneˇ» «twoˇ»
        three
        four
    "});

    // select across a line ending
    cx.set_state(indoc! {"
        one two
        t«hree
        ˇ»four
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        one two
        \tt«hree
        ˇ»four
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        one two
        \t\tt«hree
        ˇ»four
    "});
    cx.update_editor(|e, window, cx| e.backtab(&Backtab, window, cx));
    cx.assert_editor_state(indoc! {"
        one two
        \tt«hree
        ˇ»four
    "});
    cx.update_editor(|e, window, cx| e.backtab(&Backtab, window, cx));
    cx.assert_editor_state(indoc! {"
        one two
        t«hree
        ˇ»four
    "});

    // Ensure that indenting/outdenting works when the cursor is at column 0.
    cx.set_state(indoc! {"
        one two
        ˇthree
        four
    "});
    cx.update_editor(|e, window, cx| e.backtab(&Backtab, window, cx));
    cx.assert_editor_state(indoc! {"
        one two
        ˇthree
        four
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.assert_editor_state(indoc! {"
        one two
        \tˇthree
        four
    "});
    cx.update_editor(|e, window, cx| e.backtab(&Backtab, window, cx));
    cx.assert_editor_state(indoc! {"
        one two
        ˇthree
        four
    "});
}

#[gpui::test]
fn test_indent_outdent_with_excerpts(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.languages.0.extend([
            (
                "TOML".into(),
                LanguageSettingsContent {
                    tab_size: NonZeroU32::new(2),
                    ..Default::default()
                },
            ),
            (
                "Rust".into(),
                LanguageSettingsContent {
                    tab_size: NonZeroU32::new(4),
                    ..Default::default()
                },
            ),
        ]);
    });

    let toml_language = Arc::new(Language::new(
        LanguageConfig {
            name: "TOML".into(),
            ..Default::default()
        },
        None,
    ));
    let rust_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Rust".into(),
            ..Default::default()
        },
        None,
    ));

    let toml_buffer =
        cx.new(|cx| Buffer::local("a = 1\nb = 2\n", cx).with_language(toml_language, cx));
    let rust_buffer =
        cx.new(|cx| Buffer::local("const c: usize = 3;\n", cx).with_language(rust_language, cx));
    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            toml_buffer.clone(),
            [Point::new(0, 0)..Point::new(2, 0)],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(1),
            rust_buffer.clone(),
            [Point::new(0, 0)..Point::new(1, 0)],
            0,
            cx,
        );
        multibuffer
    });

    cx.add_window(|window, cx| {
        let mut editor = build_editor(multibuffer, window, cx);

        assert_eq!(
            editor.text(cx),
            indoc! {"
                a = 1
                b = 2

                const c: usize = 3;
            "}
        );

        select_ranges(
            &mut editor,
            indoc! {"
                «aˇ» = 1
                b = 2

                «const c:ˇ» usize = 3;
            "},
            window,
            cx,
        );

        editor.tab(&Tab, window, cx);
        assert_text_with_selections(
            &mut editor,
            indoc! {"
                  «aˇ» = 1
                b = 2

                    «const c:ˇ» usize = 3;
            "},
            cx,
        );
        editor.backtab(&Backtab, window, cx);
        assert_text_with_selections(
            &mut editor,
            indoc! {"
                «aˇ» = 1
                b = 2

                «const c:ˇ» usize = 3;
            "},
            cx,
        );

        editor
    });
}

#[gpui::test]
async fn test_backspace(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    // Basic backspace
    cx.set_state(indoc! {"
        onˇe two three
        fou«rˇ» five six
        seven «ˇeight nine
        »ten
    "});
    cx.update_editor(|e, window, cx| e.backspace(&Backspace, window, cx));
    cx.assert_editor_state(indoc! {"
        oˇe two three
        fouˇ five six
        seven ˇten
    "});

    // Test backspace inside and around indents
    cx.set_state(indoc! {"
        zero
            ˇone
                ˇtwo
            ˇ ˇ ˇ  three
        ˇ  ˇ  four
    "});
    cx.update_editor(|e, window, cx| e.backspace(&Backspace, window, cx));
    cx.assert_editor_state(indoc! {"
        zero
        ˇone
            ˇtwo
        ˇ  threeˇ  four
    "});
}

#[gpui::test]
async fn test_delete(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(indoc! {"
        onˇe two three
        fou«rˇ» five six
        seven «ˇeight nine
        »ten
    "});
    cx.update_editor(|e, window, cx| e.delete(&Delete, window, cx));
    cx.assert_editor_state(indoc! {"
        onˇ two three
        fouˇ five six
        seven ˇten
    "});
}

#[gpui::test]
fn test_delete_line(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("abc\ndef\nghi\n", cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(3), 0)..DisplayPoint::new(DisplayRow(3), 0),
            ])
        });
        editor.delete_line(&DeleteLine, window, cx);
        assert_eq!(editor.display_text(cx), "ghi");
        assert_eq!(
            display_ranges(editor, cx),
            vec![
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(0), 1),
            ]
        );
    });

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("abc\ndef\nghi\n", cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(2), 0)..DisplayPoint::new(DisplayRow(0), 1)
            ])
        });
        editor.delete_line(&DeleteLine, window, cx);
        assert_eq!(editor.display_text(cx), "ghi\n");
        assert_eq!(
            display_ranges(editor, cx),
            vec![DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(0), 1)]
        );
    });

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("abc\ndef\nghi\n\njkl\nmno", cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(2), 1)
            ])
        });
        editor.delete_line(&DeleteLine, window, cx);
        assert_eq!(editor.display_text(cx), "\njkl\nmno");
        assert_eq!(
            display_ranges(editor, cx),
            vec![DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0)]
        );
    });
}

#[gpui::test]
fn test_join_lines_with_single_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaa\nbbb\nccc\nddd\n\n", cx);
        let mut editor = build_editor(buffer.clone(), window, cx);
        let buffer = buffer.read(cx).as_singleton().unwrap();

        assert_eq!(
            editor
                .selections
                .ranges::<Point>(&editor.display_snapshot(cx)),
            &[Point::new(0, 0)..Point::new(0, 0)]
        );

        // When on single line, replace newline at end by space
        editor.join_lines(&JoinLines, window, cx);
        assert_eq!(buffer.read(cx).text(), "aaa bbb\nccc\nddd\n\n");
        assert_eq!(
            editor
                .selections
                .ranges::<Point>(&editor.display_snapshot(cx)),
            &[Point::new(0, 3)..Point::new(0, 3)]
        );

        editor.undo(&Undo, window, cx);
        assert_eq!(buffer.read(cx).text(), "aaa\nbbb\nccc\nddd\n\n");

        // Select a full line, i.e. start of the first line to the start of the second line
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(0, 0)..Point::new(1, 0)])
        });
        editor.join_lines(&JoinLines, window, cx);
        assert_eq!(buffer.read(cx).text(), "aaa bbb\nccc\nddd\n\n");

        editor.undo(&Undo, window, cx);
        assert_eq!(buffer.read(cx).text(), "aaa\nbbb\nccc\nddd\n\n");

        // Select two full lines
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(0, 0)..Point::new(2, 0)])
        });
        editor.join_lines(&JoinLines, window, cx);

        // Only the selected lines should be joined, not the third.
        assert_eq!(
            buffer.read(cx).text(),
            "aaa bbb\nccc\nddd\n\n",
            "only the two selected lines (a and b) should be joined"
        );

        // When multiple lines are selected, remove newlines that are spanned by the selection
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(0, 5)..Point::new(2, 2)])
        });
        editor.join_lines(&JoinLines, window, cx);
        assert_eq!(buffer.read(cx).text(), "aaa bbb ccc ddd\n\n");
        assert_eq!(
            editor
                .selections
                .ranges::<Point>(&editor.display_snapshot(cx)),
            &[Point::new(0, 11)..Point::new(0, 11)]
        );

        // Undo should be transactional
        editor.undo(&Undo, window, cx);
        assert_eq!(buffer.read(cx).text(), "aaa bbb\nccc\nddd\n\n");
        assert_eq!(
            editor
                .selections
                .ranges::<Point>(&editor.display_snapshot(cx)),
            &[Point::new(0, 5)..Point::new(2, 2)]
        );

        // When joining an empty line don't insert a space
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(2, 1)..Point::new(2, 2)])
        });
        editor.join_lines(&JoinLines, window, cx);
        assert_eq!(buffer.read(cx).text(), "aaa bbb\nccc\nddd\n");
        assert_eq!(
            editor
                .selections
                .ranges::<Point>(&editor.display_snapshot(cx)),
            [Point::new(2, 3)..Point::new(2, 3)]
        );

        // We can remove trailing newlines
        editor.join_lines(&JoinLines, window, cx);
        assert_eq!(buffer.read(cx).text(), "aaa bbb\nccc\nddd");
        assert_eq!(
            editor
                .selections
                .ranges::<Point>(&editor.display_snapshot(cx)),
            [Point::new(2, 3)..Point::new(2, 3)]
        );

        // We don't blow up on the last line
        editor.join_lines(&JoinLines, window, cx);
        assert_eq!(buffer.read(cx).text(), "aaa bbb\nccc\nddd");
        assert_eq!(
            editor
                .selections
                .ranges::<Point>(&editor.display_snapshot(cx)),
            [Point::new(2, 3)..Point::new(2, 3)]
        );

        // reset to test indentation
        editor.buffer.update(cx, |buffer, cx| {
            buffer.edit(
                [
                    (Point::new(1, 0)..Point::new(1, 2), "  "),
                    (Point::new(2, 0)..Point::new(2, 3), "  \n\td"),
                ],
                None,
                cx,
            )
        });

        // We remove any leading spaces
        assert_eq!(buffer.read(cx).text(), "aaa bbb\n  c\n  \n\td");
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(0, 1)..Point::new(0, 1)])
        });
        editor.join_lines(&JoinLines, window, cx);
        assert_eq!(buffer.read(cx).text(), "aaa bbb c\n  \n\td");

        // We don't insert a space for a line containing only spaces
        editor.join_lines(&JoinLines, window, cx);
        assert_eq!(buffer.read(cx).text(), "aaa bbb c\n\td");

        // We ignore any leading tabs
        editor.join_lines(&JoinLines, window, cx);
        assert_eq!(buffer.read(cx).text(), "aaa bbb c d");

        editor
    });
}

#[gpui::test]
fn test_join_lines_with_multi_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaa\nbbb\nccc\nddd\n\n", cx);
        let mut editor = build_editor(buffer.clone(), window, cx);
        let buffer = buffer.read(cx).as_singleton().unwrap();

        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([
                Point::new(0, 2)..Point::new(1, 1),
                Point::new(1, 2)..Point::new(1, 2),
                Point::new(3, 1)..Point::new(3, 2),
            ])
        });

        editor.join_lines(&JoinLines, window, cx);
        assert_eq!(buffer.read(cx).text(), "aaa bbb ccc\nddd\n");

        assert_eq!(
            editor
                .selections
                .ranges::<Point>(&editor.display_snapshot(cx)),
            [
                Point::new(0, 7)..Point::new(0, 7),
                Point::new(1, 3)..Point::new(1, 3)
            ]
        );
        editor
    });
}

#[gpui::test]
async fn test_join_lines_with_git_diff_base(executor: BackgroundExecutor, cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let diff_base = r#"
        Line 0
        Line 1
        Line 2
        Line 3
        "#
    .unindent();

    cx.set_state(
        &r#"
        ˇLine 0
        Line 1
        Line 2
        Line 3
        "#
        .unindent(),
    );

    cx.set_head_text(&diff_base);
    executor.run_until_parked();

    // Join lines
    cx.update_editor(|editor, window, cx| {
        editor.join_lines(&JoinLines, window, cx);
    });
    executor.run_until_parked();

    cx.assert_editor_state(
        &r#"
        Line 0ˇ Line 1
        Line 2
        Line 3
        "#
        .unindent(),
    );
    // Join again
    cx.update_editor(|editor, window, cx| {
        editor.join_lines(&JoinLines, window, cx);
    });
    executor.run_until_parked();

    cx.assert_editor_state(
        &r#"
        Line 0 Line 1ˇ Line 2
        Line 3
        "#
        .unindent(),
    );
}

#[gpui::test]
async fn test_join_lines_strips_comment_prefix(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    {
        let language = Arc::new(Language::new(
            LanguageConfig {
                line_comments: vec!["// ".into(), "/// ".into()],
                documentation_comment: Some(BlockCommentConfig {
                    start: "/*".into(),
                    end: "*/".into(),
                    prefix: "* ".into(),
                    tab_size: 1,
                }),
                ..LanguageConfig::default()
            },
            None,
        ));

        let mut cx = EditorTestContext::new(cx).await;
        cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

        // Strips the comment prefix (with trailing space) from the joined-in line.
        cx.set_state(indoc! {"
            // ˇfoo
            // bar
        "});
        cx.update_editor(|e, window, cx| e.join_lines(&JoinLines, window, cx));
        cx.assert_editor_state(indoc! {"
            // fooˇ bar
        "});

        // Strips the longer doc-comment prefix when both `//` and `///` match.
        cx.set_state(indoc! {"
            /// ˇfoo
            /// bar
        "});
        cx.update_editor(|e, window, cx| e.join_lines(&JoinLines, window, cx));
        cx.assert_editor_state(indoc! {"
            /// fooˇ bar
        "});

        // Does not strip when the second line is a regular line (no comment prefix).
        cx.set_state(indoc! {"
            // ˇfoo
            bar
        "});
        cx.update_editor(|e, window, cx| e.join_lines(&JoinLines, window, cx));
        cx.assert_editor_state(indoc! {"
            // fooˇ bar
        "});

        // No-whitespace join also strips the comment prefix.
        cx.set_state(indoc! {"
            // ˇfoo
            // bar
        "});
        cx.update_editor(|e, window, cx| e.join_lines_impl(false, window, cx));
        cx.assert_editor_state(indoc! {"
            // fooˇbar
        "});

        // Strips even when the joined-in line is just the bare prefix (no trailing space).
        cx.set_state(indoc! {"
            // ˇfoo
            //
        "});
        cx.update_editor(|e, window, cx| e.join_lines(&JoinLines, window, cx));
        cx.assert_editor_state(indoc! {"
            // fooˇ
        "});

        // Mixed line comment prefix types: the longer matching prefix is stripped.
        cx.set_state(indoc! {"
            // ˇfoo
            /// bar
        "});
        cx.update_editor(|e, window, cx| e.join_lines(&JoinLines, window, cx));
        cx.assert_editor_state(indoc! {"
            // fooˇ bar
        "});

        // Strips block comment body prefix (`* `) from the joined-in line.
        cx.set_state(indoc! {"
             * ˇfoo
             * bar
        "});
        cx.update_editor(|e, window, cx| e.join_lines(&JoinLines, window, cx));
        cx.assert_editor_state(indoc! {"
             * fooˇ bar
        "});

        // Strips bare block comment body prefix (`*` without trailing space).
        cx.set_state(indoc! {"
             * ˇfoo
             *
        "});
        cx.update_editor(|e, window, cx| e.join_lines(&JoinLines, window, cx));
        cx.assert_editor_state(indoc! {"
             * fooˇ
        "});
    }

    {
        let markdown_language = Arc::new(Language::new(
            LanguageConfig {
                unordered_list: vec!["- ".into(), "* ".into(), "+ ".into()],
                ..LanguageConfig::default()
            },
            None,
        ));

        let mut cx = EditorTestContext::new(cx).await;
        cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_language), cx));

        // Strips the `- ` list marker from the joined-in line.
        cx.set_state(indoc! {"
            - ˇfoo
            - bar
        "});
        cx.update_editor(|e, window, cx| e.join_lines(&JoinLines, window, cx));
        cx.assert_editor_state(indoc! {"
            - fooˇ bar
        "});

        // Strips the `* ` list marker from the joined-in line.
        cx.set_state(indoc! {"
            * ˇfoo
            * bar
        "});
        cx.update_editor(|e, window, cx| e.join_lines(&JoinLines, window, cx));
        cx.assert_editor_state(indoc! {"
            * fooˇ bar
        "});

        // Strips the `+ ` list marker from the joined-in line.
        cx.set_state(indoc! {"
            + ˇfoo
            + bar
        "});
        cx.update_editor(|e, window, cx| e.join_lines(&JoinLines, window, cx));
        cx.assert_editor_state(indoc! {"
            + fooˇ bar
        "});

        cx.set_state(indoc! {"
            fooˇ
            *bar*
        "});
        cx.update_editor(|e, window, cx| e.join_lines(&JoinLines, window, cx));
        cx.assert_editor_state(indoc! {"
            fooˇ *bar*
        "});

        cx.set_state(indoc! {"
            * ˇfoo
            *
        "});
        cx.update_editor(|e, window, cx| e.join_lines(&JoinLines, window, cx));
        cx.assert_editor_state(indoc! {"
            * fooˇ
        "});

        // No-whitespace join also strips the list marker.
        cx.set_state(indoc! {"
            - ˇfoo
            - bar
        "});
        cx.update_editor(|e, window, cx| e.join_lines_impl(false, window, cx));
        cx.assert_editor_state(indoc! {"
            - fooˇbar
        "});
    }
}

#[gpui::test]
async fn test_custom_newlines_cause_no_false_positive_diffs(
    executor: BackgroundExecutor,
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state("Line 0\r\nLine 1\rˇ\nLine 2\r\nLine 3");
    cx.set_head_text("Line 0\r\nLine 1\r\nLine 2\r\nLine 3");
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        let snapshot = editor.snapshot(window, cx);
        assert_eq!(
            snapshot
                .buffer_snapshot()
                .diff_hunks_in_range(MultiBufferOffset(0)..snapshot.buffer_snapshot().len())
                .collect::<Vec<_>>(),
            Vec::new(),
            "Should not have any diffs for files with custom newlines"
        );
    });
}

#[gpui::test]
async fn test_manipulate_immutable_lines_with_single_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    // Test sort_lines_case_insensitive()
    cx.set_state(indoc! {"
        «z
        y
        x
        Z
        Y
        Xˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.sort_lines_case_insensitive(&SortLinesCaseInsensitive, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «x
        X
        y
        Y
        z
        Zˇ»
    "});

    // Test sort_lines_by_length()
    //
    // Demonstrates:
    // - ∞ is 3 bytes UTF-8, but sorted by its char count (1)
    // - sort is stable
    cx.set_state(indoc! {"
        «123
        æ
        12
        ∞
        1
        æˇ»
    "});
    cx.update_editor(|e, window, cx| e.sort_lines_by_length(&SortLinesByLength, window, cx));
    cx.assert_editor_state(indoc! {"
        «æ
        ∞
        1
        æ
        12
        123ˇ»
    "});

    // Test reverse_lines()
    cx.set_state(indoc! {"
        «5
        4
        3
        2
        1ˇ»
    "});
    cx.update_editor(|e, window, cx| e.reverse_lines(&ReverseLines, window, cx));
    cx.assert_editor_state(indoc! {"
        «1
        2
        3
        4
        5ˇ»
    "});

    // Skip testing shuffle_line()

    // From here on out, test more complex cases of manipulate_immutable_lines() with a single driver method: sort_lines_case_sensitive()
    // Since all methods calling manipulate_immutable_lines() are doing the exact same general thing (reordering lines)

    // Don't manipulate when cursor is on single line, but expand the selection
    cx.set_state(indoc! {"
        ddˇdd
        ccc
        bb
        a
    "});
    cx.update_editor(|e, window, cx| {
        e.sort_lines_case_sensitive(&SortLinesCaseSensitive, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «ddddˇ»
        ccc
        bb
        a
    "});

    // Basic manipulate case
    // Start selection moves to column 0
    // End of selection shrinks to fit shorter line
    cx.set_state(indoc! {"
        dd«d
        ccc
        bb
        aaaaaˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.sort_lines_case_sensitive(&SortLinesCaseSensitive, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «aaaaa
        bb
        ccc
        dddˇ»
    "});

    // Manipulate case with newlines
    cx.set_state(indoc! {"
        dd«d
        ccc

        bb
        aaaaa

        ˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.sort_lines_case_sensitive(&SortLinesCaseSensitive, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «

        aaaaa
        bb
        ccc
        dddˇ»

    "});

    // Adding new line
    cx.set_state(indoc! {"
        aa«a
        bbˇ»b
    "});
    cx.update_editor(|e, window, cx| {
        e.manipulate_immutable_lines(window, cx, |lines| lines.push("added_line"))
    });
    cx.assert_editor_state(indoc! {"
        «aaa
        bbb
        added_lineˇ»
    "});

    // Removing line
    cx.set_state(indoc! {"
        aa«a
        bbbˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.manipulate_immutable_lines(window, cx, |lines| {
            lines.pop();
        })
    });
    cx.assert_editor_state(indoc! {"
        «aaaˇ»
    "});

    // Removing all lines
    cx.set_state(indoc! {"
        aa«a
        bbbˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.manipulate_immutable_lines(window, cx, |lines| {
            lines.drain(..);
        })
    });
    cx.assert_editor_state(indoc! {"
        ˇ
    "});
}

#[gpui::test]
async fn test_unique_lines_multi_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    // Consider continuous selection as single selection
    cx.set_state(indoc! {"
        Aaa«aa
        cˇ»c«c
        bb
        aaaˇ»aa
    "});
    cx.update_editor(|e, window, cx| {
        e.unique_lines_case_sensitive(&UniqueLinesCaseSensitive, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «Aaaaa
        ccc
        bb
        aaaaaˇ»
    "});

    cx.set_state(indoc! {"
        Aaa«aa
        cˇ»c«c
        bb
        aaaˇ»aa
    "});
    cx.update_editor(|e, window, cx| {
        e.unique_lines_case_insensitive(&UniqueLinesCaseInsensitive, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «Aaaaa
        ccc
        bbˇ»
    "});

    // Consider non continuous selection as distinct dedup operations
    cx.set_state(indoc! {"
        «aaaaa
        bb
        aaaaa
        aaaaaˇ»

        aaa«aaˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.unique_lines_case_sensitive(&UniqueLinesCaseSensitive, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «aaaaa
        bbˇ»

        «aaaaaˇ»
    "});
}

#[gpui::test]
async fn test_unique_lines_single_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"
        «Aaa
        aAa
        Aaaˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.unique_lines_case_sensitive(&UniqueLinesCaseSensitive, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «Aaa
        aAaˇ»
    "});

    cx.set_state(indoc! {"
        «Aaa
        aAa
        aaAˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.unique_lines_case_insensitive(&UniqueLinesCaseInsensitive, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «Aaaˇ»
    "});
}

#[gpui::test]
async fn test_wrap_in_tag_single_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let js_language = Arc::new(Language::new(
        LanguageConfig {
            name: "JavaScript".into(),
            wrap_characters: Some(language::WrapCharactersConfig {
                start_prefix: "<".into(),
                start_suffix: ">".into(),
                end_prefix: "</".into(),
                end_suffix: ">".into(),
            }),
            ..LanguageConfig::default()
        },
        None,
    ));

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(js_language), cx));

    cx.set_state(indoc! {"
        «testˇ»
    "});
    cx.update_editor(|e, window, cx| e.wrap_selections_in_tag(&WrapSelectionsInTag, window, cx));
    cx.assert_editor_state(indoc! {"
        <«ˇ»>test</«ˇ»>
    "});

    cx.set_state(indoc! {"
        «test
         testˇ»
    "});
    cx.update_editor(|e, window, cx| e.wrap_selections_in_tag(&WrapSelectionsInTag, window, cx));
    cx.assert_editor_state(indoc! {"
        <«ˇ»>test
         test</«ˇ»>
    "});

    cx.set_state(indoc! {"
        teˇst
    "});
    cx.update_editor(|e, window, cx| e.wrap_selections_in_tag(&WrapSelectionsInTag, window, cx));
    cx.assert_editor_state(indoc! {"
        te<«ˇ»></«ˇ»>st
    "});
}

#[gpui::test]
async fn test_wrap_in_tag_multi_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let js_language = Arc::new(Language::new(
        LanguageConfig {
            name: "JavaScript".into(),
            wrap_characters: Some(language::WrapCharactersConfig {
                start_prefix: "<".into(),
                start_suffix: ">".into(),
                end_prefix: "</".into(),
                end_suffix: ">".into(),
            }),
            ..LanguageConfig::default()
        },
        None,
    ));

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(js_language), cx));

    cx.set_state(indoc! {"
        «testˇ»
        «testˇ» «testˇ»
        «testˇ»
    "});
    cx.update_editor(|e, window, cx| e.wrap_selections_in_tag(&WrapSelectionsInTag, window, cx));
    cx.assert_editor_state(indoc! {"
        <«ˇ»>test</«ˇ»>
        <«ˇ»>test</«ˇ»> <«ˇ»>test</«ˇ»>
        <«ˇ»>test</«ˇ»>
    "});

    cx.set_state(indoc! {"
        «test
         testˇ»
        «test
         testˇ»
    "});
    cx.update_editor(|e, window, cx| e.wrap_selections_in_tag(&WrapSelectionsInTag, window, cx));
    cx.assert_editor_state(indoc! {"
        <«ˇ»>test
         test</«ˇ»>
        <«ˇ»>test
         test</«ˇ»>
    "});
}

#[gpui::test]
async fn test_wrap_in_tag_does_nothing_in_unsupported_languages(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let plaintext_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Plain Text".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(plaintext_language), cx));

    cx.set_state(indoc! {"
        «testˇ»
    "});
    cx.update_editor(|e, window, cx| e.wrap_selections_in_tag(&WrapSelectionsInTag, window, cx));
    cx.assert_editor_state(indoc! {"
      «testˇ»
    "});
}

#[gpui::test]
async fn test_manipulate_immutable_lines_with_multi_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    // Manipulate with multiple selections on a single line
    cx.set_state(indoc! {"
        dd«dd
        cˇ»c«c
        bb
        aaaˇ»aa
    "});
    cx.update_editor(|e, window, cx| {
        e.sort_lines_case_sensitive(&SortLinesCaseSensitive, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «aaaaa
        bb
        ccc
        ddddˇ»
    "});

    // Manipulate with multiple disjoin selections
    cx.set_state(indoc! {"
        5«
        4
        3
        2
        1ˇ»

        dd«dd
        ccc
        bb
        aaaˇ»aa
    "});
    cx.update_editor(|e, window, cx| {
        e.sort_lines_case_sensitive(&SortLinesCaseSensitive, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «1
        2
        3
        4
        5ˇ»

        «aaaaa
        bb
        ccc
        ddddˇ»
    "});

    // Adding lines on each selection
    cx.set_state(indoc! {"
        2«
        1ˇ»

        bb«bb
        aaaˇ»aa
    "});
    cx.update_editor(|e, window, cx| {
        e.manipulate_immutable_lines(window, cx, |lines| lines.push("added line"))
    });
    cx.assert_editor_state(indoc! {"
        «2
        1
        added lineˇ»

        «bbbb
        aaaaa
        added lineˇ»
    "});

    // Removing lines on each selection
    cx.set_state(indoc! {"
        2«
        1ˇ»

        bb«bb
        aaaˇ»aa
    "});
    cx.update_editor(|e, window, cx| {
        e.manipulate_immutable_lines(window, cx, |lines| {
            lines.pop();
        })
    });
    cx.assert_editor_state(indoc! {"
        «2ˇ»

        «bbbbˇ»
    "});
}

#[gpui::test]
async fn test_convert_indentation_to_spaces(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(3)
    });

    let mut cx = EditorTestContext::new(cx).await;

    // MULTI SELECTION
    // Ln.1 "«" tests empty lines
    // Ln.9 tests just leading whitespace
    cx.set_state(indoc! {"
        «
        abc                 // No indentationˇ»
        «\tabc              // 1 tabˇ»
        \t\tabc «      ˇ»   // 2 tabs
        \t ab«c             // Tab followed by space
         \tabc              // Space followed by tab (3 spaces should be the result)
        \t \t  \t   \tabc   // Mixed indentation (tab conversion depends on the column)
           abˇ»ˇc   ˇ    ˇ  // Already space indented«
        \t
        \tabc\tdef          // Only the leading tab is manipulatedˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.convert_indentation_to_spaces(&ConvertIndentationToSpaces, window, cx);
    });
    cx.assert_editor_state(
        indoc! {"
            «
            abc                 // No indentation
               abc              // 1 tab
                  abc          // 2 tabs
                abc             // Tab followed by space
               abc              // Space followed by tab (3 spaces should be the result)
                           abc   // Mixed indentation (tab conversion depends on the column)
               abc         // Already space indented
               ·
               abc\tdef          // Only the leading tab is manipulatedˇ»
        "}
        .replace("·", "")
        .as_str(), // · used as placeholder to prevent format-on-save from removing whitespace
    );

    // Test on just a few lines, the others should remain unchanged
    // Only lines (3, 5, 10, 11) should change
    cx.set_state(
        indoc! {"
            ·
            abc                 // No indentation
            \tabcˇ               // 1 tab
            \t\tabc             // 2 tabs
            \t abcˇ              // Tab followed by space
             \tabc              // Space followed by tab (3 spaces should be the result)
            \t \t  \t   \tabc   // Mixed indentation (tab conversion depends on the column)
               abc              // Already space indented
            «\t
            \tabc\tdef          // Only the leading tab is manipulatedˇ»
        "}
        .replace("·", "")
        .as_str(), // · used as placeholder to prevent format-on-save from removing whitespace
    );
    cx.update_editor(|e, window, cx| {
        e.convert_indentation_to_spaces(&ConvertIndentationToSpaces, window, cx);
    });
    cx.assert_editor_state(
        indoc! {"
            ·
            abc                 // No indentation
            «   abc               // 1 tabˇ»
            \t\tabc             // 2 tabs
            «    abc              // Tab followed by spaceˇ»
             \tabc              // Space followed by tab (3 spaces should be the result)
            \t \t  \t   \tabc   // Mixed indentation (tab conversion depends on the column)
               abc              // Already space indented
            «   ·
               abc\tdef          // Only the leading tab is manipulatedˇ»
        "}
        .replace("·", "")
        .as_str(), // · used as placeholder to prevent format-on-save from removing whitespace
    );

    // SINGLE SELECTION
    // Ln.1 "«" tests empty lines
    // Ln.9 tests just leading whitespace
    cx.set_state(indoc! {"
        «
        abc                 // No indentation
        \tabc               // 1 tab
        \t\tabc             // 2 tabs
        \t abc              // Tab followed by space
         \tabc              // Space followed by tab (3 spaces should be the result)
        \t \t  \t   \tabc   // Mixed indentation (tab conversion depends on the column)
           abc              // Already space indented
        \t
        \tabc\tdef          // Only the leading tab is manipulatedˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.convert_indentation_to_spaces(&ConvertIndentationToSpaces, window, cx);
    });
    cx.assert_editor_state(
        indoc! {"
            «
            abc                 // No indentation
               abc               // 1 tab
                  abc             // 2 tabs
                abc              // Tab followed by space
               abc              // Space followed by tab (3 spaces should be the result)
                           abc   // Mixed indentation (tab conversion depends on the column)
               abc              // Already space indented
               ·
               abc\tdef          // Only the leading tab is manipulatedˇ»
        "}
        .replace("·", "")
        .as_str(), // · used as placeholder to prevent format-on-save from removing whitespace
    );
}

#[gpui::test]
async fn test_convert_indentation_to_tabs(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = NonZeroU32::new(3)
    });

    let mut cx = EditorTestContext::new(cx).await;

    // MULTI SELECTION
    // Ln.1 "«" tests empty lines
    // Ln.11 tests just leading whitespace
    cx.set_state(indoc! {"
        «
        abˇ»ˇc                 // No indentation
         abc    ˇ        ˇ    // 1 space (< 3 so dont convert)
          abc  «             // 2 spaces (< 3 so dont convert)
           abc              // 3 spaces (convert)
             abc ˇ»           // 5 spaces (1 tab + 2 spaces)
        «\tˇ»\t«\tˇ»abc           // Already tab indented
        «\t abc              // Tab followed by space
         \tabc              // Space followed by tab (should be consumed due to tab)
        \t \t  \t   \tabc   // Mixed indentation (first 3 spaces are consumed, the others are converted)
           \tˇ»  «\t
           abcˇ»   \t ˇˇˇ        // Only the leading spaces should be converted
    "});
    cx.update_editor(|e, window, cx| {
        e.convert_indentation_to_tabs(&ConvertIndentationToTabs, window, cx);
    });
    cx.assert_editor_state(indoc! {"
        «
        abc                 // No indentation
         abc                // 1 space (< 3 so dont convert)
          abc               // 2 spaces (< 3 so dont convert)
        \tabc              // 3 spaces (convert)
        \t  abc            // 5 spaces (1 tab + 2 spaces)
        \t\t\tabc           // Already tab indented
        \t abc              // Tab followed by space
        \tabc              // Space followed by tab (should be consumed due to tab)
        \t\t\t\t\tabc   // Mixed indentation (first 3 spaces are consumed, the others are converted)
        \t\t\t
        \tabc   \t         // Only the leading spaces should be convertedˇ»
    "});

    // Test on just a few lines, the other should remain unchanged
    // Only lines (4, 8, 11, 12) should change
    cx.set_state(
        indoc! {"
            ·
            abc                 // No indentation
             abc                // 1 space (< 3 so dont convert)
              abc               // 2 spaces (< 3 so dont convert)
            «   abc              // 3 spaces (convert)ˇ»
                 abc            // 5 spaces (1 tab + 2 spaces)
            \t\t\tabc           // Already tab indented
            \t abc              // Tab followed by space
             \tabc      ˇ        // Space followed by tab (should be consumed due to tab)
               \t\t  \tabc      // Mixed indentation
            \t \t  \t   \tabc   // Mixed indentation
               \t  \tˇ
            «   abc   \t         // Only the leading spaces should be convertedˇ»
        "}
        .replace("·", "")
        .as_str(), // · used as placeholder to prevent format-on-save from removing whitespace
    );
    cx.update_editor(|e, window, cx| {
        e.convert_indentation_to_tabs(&ConvertIndentationToTabs, window, cx);
    });
    cx.assert_editor_state(
        indoc! {"
            ·
            abc                 // No indentation
             abc                // 1 space (< 3 so dont convert)
              abc               // 2 spaces (< 3 so dont convert)
            «\tabc              // 3 spaces (convert)ˇ»
                 abc            // 5 spaces (1 tab + 2 spaces)
            \t\t\tabc           // Already tab indented
            \t abc              // Tab followed by space
            «\tabc              // Space followed by tab (should be consumed due to tab)ˇ»
               \t\t  \tabc      // Mixed indentation
            \t \t  \t   \tabc   // Mixed indentation
            «\t\t\t
            \tabc   \t         // Only the leading spaces should be convertedˇ»
        "}
        .replace("·", "")
        .as_str(), // · used as placeholder to prevent format-on-save from removing whitespace
    );

    // SINGLE SELECTION
    // Ln.1 "«" tests empty lines
    // Ln.11 tests just leading whitespace
    cx.set_state(indoc! {"
        «
        abc                 // No indentation
         abc                // 1 space (< 3 so dont convert)
          abc               // 2 spaces (< 3 so dont convert)
           abc              // 3 spaces (convert)
             abc            // 5 spaces (1 tab + 2 spaces)
        \t\t\tabc           // Already tab indented
        \t abc              // Tab followed by space
         \tabc              // Space followed by tab (should be consumed due to tab)
        \t \t  \t   \tabc   // Mixed indentation (first 3 spaces are consumed, the others are converted)
           \t  \t
           abc   \t         // Only the leading spaces should be convertedˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.convert_indentation_to_tabs(&ConvertIndentationToTabs, window, cx);
    });
    cx.assert_editor_state(indoc! {"
        «
        abc                 // No indentation
         abc                // 1 space (< 3 so dont convert)
          abc               // 2 spaces (< 3 so dont convert)
        \tabc              // 3 spaces (convert)
        \t  abc            // 5 spaces (1 tab + 2 spaces)
        \t\t\tabc           // Already tab indented
        \t abc              // Tab followed by space
        \tabc              // Space followed by tab (should be consumed due to tab)
        \t\t\t\t\tabc   // Mixed indentation (first 3 spaces are consumed, the others are converted)
        \t\t\t
        \tabc   \t         // Only the leading spaces should be convertedˇ»
    "});
}

#[gpui::test]
async fn test_toggle_case(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    // If all lower case -> upper case
    cx.set_state(indoc! {"
        «hello worldˇ»
    "});
    cx.update_editor(|e, window, cx| e.toggle_case(&ToggleCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «HELLO WORLDˇ»
    "});

    // If all upper case -> lower case
    cx.set_state(indoc! {"
        «HELLO WORLDˇ»
    "});
    cx.update_editor(|e, window, cx| e.toggle_case(&ToggleCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «hello worldˇ»
    "});

    // If any upper case characters are identified -> lower case
    // This matches JetBrains IDEs
    cx.set_state(indoc! {"
        «hEllo worldˇ»
    "});
    cx.update_editor(|e, window, cx| e.toggle_case(&ToggleCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «hello worldˇ»
    "});
}

#[gpui::test]
async fn test_convert_to_sentence_case(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"
        «implement-windows-supportˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.convert_to_sentence_case(&ConvertToSentenceCase, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «Implement windows supportˇ»
    "});
}

#[gpui::test]
async fn test_convert_to_base64(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    // Encode a plain text selection
    cx.set_state(indoc! {"
        «helloˇ»
    "});
    cx.update_editor(|e, window, cx| e.convert_to_base64(&ConvertToBase64, window, cx));
    cx.assert_editor_state(indoc! {"
        «aGVsbG8=ˇ»
    "});

    // Decode a valid base64 selection
    cx.set_state(indoc! {"
        «aGVsbG8=ˇ»
    "});
    cx.update_editor(|e, window, cx| e.convert_from_base64(&ConvertFromBase64, window, cx));
    cx.assert_editor_state(indoc! {"
        «helloˇ»
    "});

    // Decode invalid base64 — should leave text unchanged
    cx.set_state(indoc! {"
        «not!!!ˇ»
    "});
    cx.update_editor(|e, window, cx| e.convert_from_base64(&ConvertFromBase64, window, cx));
    cx.assert_editor_state(indoc! {"
        «not!!!ˇ»
    "});
}

#[gpui::test]
fn test_manipulate_text_handles_cross_excerpt_edit_that_applies_differently(
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let buffer_1 = cx.new(|cx| {
        let mut buffer = Buffer::local("ab", cx);
        // The selected multibuffer range starts in this excerpt, but edits to
        // it are skipped because the underlying buffer is read-only.
        buffer.set_capability(language::Capability::ReadOnly, cx);
        buffer
    });
    let buffer_2 = cx.new(|cx| Buffer::local("cd", cx));
    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1.clone(),
            [Point::new(0, 0)..Point::new(0, 2)],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(1),
            buffer_2.clone(),
            [Point::new(0, 0)..Point::new(0, 2)],
            0,
            cx,
        );
        multibuffer
    });

    cx.add_window(|window, cx| {
        let mut editor = build_editor(multibuffer, window, cx);
        let len = editor.buffer().read(cx).len(cx);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
            selections.select_ranges([MultiBufferOffset(0)..len])
        });

        // No-op transformations should not be sent through `MultiBuffer::edit`.
        editor.manipulate_text(window, cx, |text| text.to_string());
        assert_eq!(buffer_1.read(cx).text(), "ab");
        assert_eq!(buffer_2.read(cx).text(), "cd");

        // A real replacement can apply differently than requested; selection
        // remapping should follow the actual edit instead of predicted offsets.
        editor.manipulate_text(window, cx, |_| "replacement".to_string());
        assert_eq!(buffer_1.read(cx).text(), "ab");
        assert_eq!(buffer_2.read(cx).text(), "");

        editor
    });
}

#[gpui::test]
async fn test_manipulate_text(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    // Test convert_to_upper_case()
    cx.set_state(indoc! {"
        «hello worldˇ»
    "});
    cx.update_editor(|e, window, cx| e.convert_to_upper_case(&ConvertToUpperCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «HELLO WORLDˇ»
    "});

    // Test convert_to_lower_case()
    cx.set_state(indoc! {"
        «HELLO WORLDˇ»
    "});
    cx.update_editor(|e, window, cx| e.convert_to_lower_case(&ConvertToLowerCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «hello worldˇ»
    "});

    // Test multiple line, single selection case
    cx.set_state(indoc! {"
        «The quick brown
        fox jumps over
        the lazy dogˇ»
    "});
    cx.update_editor(|e, window, cx| e.convert_to_title_case(&ConvertToTitleCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «The Quick Brown
        Fox Jumps Over
        The Lazy Dogˇ»
    "});

    // Test multiple line, single selection case
    cx.set_state(indoc! {"
        «The quick brown
        fox jumps over
        the lazy dogˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.convert_to_upper_camel_case(&ConvertToUpperCamelCase, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «TheQuickBrown
        FoxJumpsOver
        TheLazyDogˇ»
    "});

    // From here on out, test more complex cases of manipulate_text()

    // Test no selection case - should affect words cursors are in
    // Cursor at beginning, middle, and end of word
    cx.set_state(indoc! {"
        ˇhello big beauˇtiful worldˇ
    "});
    cx.update_editor(|e, window, cx| e.convert_to_upper_case(&ConvertToUpperCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «HELLOˇ» big «BEAUTIFULˇ» «WORLDˇ»
    "});

    // Test multiple selections on a single line and across multiple lines
    cx.set_state(indoc! {"
        «Theˇ» quick «brown
        foxˇ» jumps «overˇ»
        the «lazyˇ» dog
    "});
    cx.update_editor(|e, window, cx| e.convert_to_upper_case(&ConvertToUpperCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «THEˇ» quick «BROWN
        FOXˇ» jumps «OVERˇ»
        the «LAZYˇ» dog
    "});

    // Test case where text length grows
    cx.set_state(indoc! {"
        «tschüßˇ»
    "});
    cx.update_editor(|e, window, cx| e.convert_to_upper_case(&ConvertToUpperCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «TSCHÜSSˇ»
    "});

    // Test to make sure we don't crash when text shrinks
    cx.set_state(indoc! {"
        aaa_bbbˇ
    "});
    cx.update_editor(|e, window, cx| {
        e.convert_to_lower_camel_case(&ConvertToLowerCamelCase, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «aaaBbbˇ»
    "});

    // Test to make sure we all aware of the fact that each word can grow and shrink
    // Final selections should be aware of this fact
    cx.set_state(indoc! {"
        aaa_bˇbb bbˇb_ccc ˇccc_ddd
    "});
    cx.update_editor(|e, window, cx| {
        e.convert_to_lower_camel_case(&ConvertToLowerCamelCase, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «aaaBbbˇ» «bbbCccˇ» «cccDddˇ»
    "});

    cx.set_state(indoc! {"
        «hElLo, WoRld!ˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.convert_to_opposite_case(&ConvertToOppositeCase, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «HeLlO, wOrLD!ˇ»
    "});

    // Test that case conversions backed by `to_case` preserve leading/trailing whitespace.
    cx.set_state(indoc! {"
        «    hello worldˇ»
    "});
    cx.update_editor(|e, window, cx| e.convert_to_title_case(&ConvertToTitleCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «    Hello Worldˇ»
    "});

    cx.set_state(indoc! {"
        «    hello worldˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.convert_to_upper_camel_case(&ConvertToUpperCamelCase, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «    HelloWorldˇ»
    "});

    cx.set_state(indoc! {"
        «    hello worldˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.convert_to_lower_camel_case(&ConvertToLowerCamelCase, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «    helloWorldˇ»
    "});

    cx.set_state(indoc! {"
        «    hello worldˇ»
    "});
    cx.update_editor(|e, window, cx| e.convert_to_snake_case(&ConvertToSnakeCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «    hello_worldˇ»
    "});

    cx.set_state(indoc! {"
        «    hello worldˇ»
    "});
    cx.update_editor(|e, window, cx| e.convert_to_kebab_case(&ConvertToKebabCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «    hello-worldˇ»
    "});

    cx.set_state(indoc! {"
        «    hello worldˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.convert_to_sentence_case(&ConvertToSentenceCase, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        «    Hello worldˇ»
    "});

    cx.set_state(indoc! {"
        «    hello world\t\tˇ»
    "});
    cx.update_editor(|e, window, cx| e.convert_to_title_case(&ConvertToTitleCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «    Hello World\t\tˇ»
    "});

    cx.set_state(indoc! {"
        «    hello world\t\tˇ»
    "});
    cx.update_editor(|e, window, cx| e.convert_to_snake_case(&ConvertToSnakeCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «    hello_world\t\tˇ»
    "});

    // Test selections with `line_mode() = true`.
    cx.update_editor(|editor, _window, _cx| editor.selections.set_line_mode(true));
    cx.set_state(indoc! {"
        «The quick brown
        fox jumps over
        tˇ»he lazy dog
    "});
    cx.update_editor(|e, window, cx| e.convert_to_upper_case(&ConvertToUpperCase, window, cx));
    cx.assert_editor_state(indoc! {"
        «THE QUICK BROWN
        FOX JUMPS OVER
        THE LAZY DOGˇ»
    "});
}

#[gpui::test]
fn test_duplicate_line(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("abc\ndef\nghi\n", cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 2),
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0),
                DisplayPoint::new(DisplayRow(3), 0)..DisplayPoint::new(DisplayRow(3), 0),
            ])
        });
        editor.duplicate_line_down(&DuplicateLineDown, window, cx);
        assert_eq!(editor.display_text(cx), "abc\nabc\ndef\ndef\nghi\n\n");
        assert_eq!(
            display_ranges(editor, cx),
            vec![
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(1), 2)..DisplayPoint::new(DisplayRow(1), 2),
                DisplayPoint::new(DisplayRow(3), 0)..DisplayPoint::new(DisplayRow(3), 0),
                DisplayPoint::new(DisplayRow(6), 0)..DisplayPoint::new(DisplayRow(6), 0),
            ]
        );
    });

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("abc\ndef\nghi\n", cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(1), 2)..DisplayPoint::new(DisplayRow(2), 1),
            ])
        });
        editor.duplicate_line_down(&DuplicateLineDown, window, cx);
        assert_eq!(editor.display_text(cx), "abc\ndef\nghi\nabc\ndef\nghi\n");
        assert_eq!(
            display_ranges(editor, cx),
            vec![
                DisplayPoint::new(DisplayRow(3), 1)..DisplayPoint::new(DisplayRow(4), 1),
                DisplayPoint::new(DisplayRow(4), 2)..DisplayPoint::new(DisplayRow(5), 1),
            ]
        );
    });

    // With `duplicate_line_up` the selections move to the duplicated lines,
    // which are inserted above the original lines
    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("abc\ndef\nghi\n", cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 2),
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0),
                DisplayPoint::new(DisplayRow(3), 0)..DisplayPoint::new(DisplayRow(3), 0),
            ])
        });
        editor.duplicate_line_up(&DuplicateLineUp, window, cx);
        assert_eq!(editor.display_text(cx), "abc\nabc\ndef\ndef\nghi\n\n");
        assert_eq!(
            display_ranges(editor, cx),
            vec![
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 2),
                DisplayPoint::new(DisplayRow(2), 0)..DisplayPoint::new(DisplayRow(2), 0),
                DisplayPoint::new(DisplayRow(5), 0)..DisplayPoint::new(DisplayRow(5), 0),
            ]
        );
    });

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("abc\ndef\nghi\n", cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(1), 2)..DisplayPoint::new(DisplayRow(2), 1),
            ])
        });
        editor.duplicate_line_up(&DuplicateLineUp, window, cx);
        assert_eq!(editor.display_text(cx), "abc\ndef\nghi\nabc\ndef\nghi\n");
        assert_eq!(
            display_ranges(editor, cx),
            vec![
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(1), 2)..DisplayPoint::new(DisplayRow(2), 1),
            ]
        );
    });

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("abc\ndef\nghi\n", cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(1), 2)..DisplayPoint::new(DisplayRow(2), 1),
            ])
        });
        editor.duplicate_selection(&DuplicateSelection, window, cx);
        assert_eq!(editor.display_text(cx), "abc\ndbc\ndef\ngf\nghi\n");
        assert_eq!(
            display_ranges(editor, cx),
            vec![
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(3), 1),
            ]
        );
    });
}

#[gpui::test]
async fn test_rotate_selections(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    // Rotate text selections (horizontal)
    cx.set_state("x=«1ˇ», y=«2ˇ», z=«3ˇ»");
    cx.update_editor(|e, window, cx| {
        e.rotate_selections_forward(&RotateSelectionsForward, window, cx)
    });
    cx.assert_editor_state("x=«3ˇ», y=«1ˇ», z=«2ˇ»");
    cx.update_editor(|e, window, cx| {
        e.rotate_selections_backward(&RotateSelectionsBackward, window, cx)
    });
    cx.assert_editor_state("x=«1ˇ», y=«2ˇ», z=«3ˇ»");

    // Rotate text selections (vertical)
    cx.set_state(indoc! {"
        x=«1ˇ»
        y=«2ˇ»
        z=«3ˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.rotate_selections_forward(&RotateSelectionsForward, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        x=«3ˇ»
        y=«1ˇ»
        z=«2ˇ»
    "});
    cx.update_editor(|e, window, cx| {
        e.rotate_selections_backward(&RotateSelectionsBackward, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        x=«1ˇ»
        y=«2ˇ»
        z=«3ˇ»
    "});

    // Rotate text selections (vertical, different lengths)
    cx.set_state(indoc! {"
        x=\"«ˇ»\"
        y=\"«aˇ»\"
        z=\"«aaˇ»\"
    "});
    cx.update_editor(|e, window, cx| {
        e.rotate_selections_forward(&RotateSelectionsForward, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        x=\"«aaˇ»\"
        y=\"«ˇ»\"
        z=\"«aˇ»\"
    "});
    cx.update_editor(|e, window, cx| {
        e.rotate_selections_backward(&RotateSelectionsBackward, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        x=\"«ˇ»\"
        y=\"«aˇ»\"
        z=\"«aaˇ»\"
    "});

    // Rotate whole lines (cursor positions preserved)
    cx.set_state(indoc! {"
        ˇline123
        liˇne23
        line3ˇ
    "});
    cx.update_editor(|e, window, cx| {
        e.rotate_selections_forward(&RotateSelectionsForward, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        line3ˇ
        ˇline123
        liˇne23
    "});
    cx.update_editor(|e, window, cx| {
        e.rotate_selections_backward(&RotateSelectionsBackward, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        ˇline123
        liˇne23
        line3ˇ
    "});

    // Rotate whole lines, multiple cursors per line (positions preserved)
    cx.set_state(indoc! {"
        ˇliˇne123
        ˇline23
        ˇline3
    "});
    cx.update_editor(|e, window, cx| {
        e.rotate_selections_forward(&RotateSelectionsForward, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        ˇline3
        ˇliˇne123
        ˇline23
    "});
    cx.update_editor(|e, window, cx| {
        e.rotate_selections_backward(&RotateSelectionsBackward, window, cx)
    });
    cx.assert_editor_state(indoc! {"
        ˇliˇne123
        ˇline23
        ˇline3
    "});
}

#[gpui::test]
fn test_move_line_up_down(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple(&sample_text(10, 5, 'a'), cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.fold_creases(
            vec![
                Crease::simple(Point::new(0, 2)..Point::new(1, 2), FoldPlaceholder::test()),
                Crease::simple(Point::new(2, 3)..Point::new(4, 1), FoldPlaceholder::test()),
                Crease::simple(Point::new(7, 0)..Point::new(8, 4), FoldPlaceholder::test()),
            ],
            true,
            window,
            cx,
        );
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(3), 1)..DisplayPoint::new(DisplayRow(3), 1),
                DisplayPoint::new(DisplayRow(3), 2)..DisplayPoint::new(DisplayRow(4), 3),
                DisplayPoint::new(DisplayRow(5), 0)..DisplayPoint::new(DisplayRow(5), 2),
            ])
        });
        assert_eq!(
            editor.display_text(cx),
            "aa⋯bbb\nccc⋯eeee\nfffff\nggggg\n⋯i\njjjjj"
        );

        editor.move_line_up(&MoveLineUp, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "aa⋯bbb\nccc⋯eeee\nggggg\n⋯i\njjjjj\nfffff"
        );
        assert_eq!(
            display_ranges(editor, cx),
            vec![
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(2), 1)..DisplayPoint::new(DisplayRow(2), 1),
                DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(3), 3),
                DisplayPoint::new(DisplayRow(4), 0)..DisplayPoint::new(DisplayRow(4), 2)
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.move_line_down(&MoveLineDown, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "ccc⋯eeee\naa⋯bbb\nfffff\nggggg\n⋯i\njjjjj"
        );
        assert_eq!(
            display_ranges(editor, cx),
            vec![
                DisplayPoint::new(DisplayRow(1), 1)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(3), 1)..DisplayPoint::new(DisplayRow(3), 1),
                DisplayPoint::new(DisplayRow(3), 2)..DisplayPoint::new(DisplayRow(4), 3),
                DisplayPoint::new(DisplayRow(5), 0)..DisplayPoint::new(DisplayRow(5), 2)
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.move_line_down(&MoveLineDown, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "ccc⋯eeee\nfffff\naa⋯bbb\nggggg\n⋯i\njjjjj"
        );
        assert_eq!(
            display_ranges(editor, cx),
            vec![
                DisplayPoint::new(DisplayRow(2), 1)..DisplayPoint::new(DisplayRow(2), 1),
                DisplayPoint::new(DisplayRow(3), 1)..DisplayPoint::new(DisplayRow(3), 1),
                DisplayPoint::new(DisplayRow(3), 2)..DisplayPoint::new(DisplayRow(4), 3),
                DisplayPoint::new(DisplayRow(5), 0)..DisplayPoint::new(DisplayRow(5), 2)
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.move_line_up(&MoveLineUp, window, cx);
        assert_eq!(
            editor.display_text(cx),
            "ccc⋯eeee\naa⋯bbb\nggggg\n⋯i\njjjjj\nfffff"
        );
        assert_eq!(
            display_ranges(editor, cx),
            vec![
                DisplayPoint::new(DisplayRow(1), 1)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(2), 1)..DisplayPoint::new(DisplayRow(2), 1),
                DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(3), 3),
                DisplayPoint::new(DisplayRow(4), 0)..DisplayPoint::new(DisplayRow(4), 2)
            ]
        );
    });
}

#[gpui::test]
fn test_move_line_up_selection_at_end_of_fold(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("\n\n\n\n\n\naaaa\nbbbb\ncccc", cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.fold_creases(
            vec![Crease::simple(
                Point::new(6, 4)..Point::new(7, 4),
                FoldPlaceholder::test(),
            )],
            true,
            window,
            cx,
        );
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(7, 4)..Point::new(7, 4)])
        });
        assert_eq!(editor.display_text(cx), "\n\n\n\n\n\naaaa⋯\ncccc");
        editor.move_line_up(&MoveLineUp, window, cx);
        let buffer_text = editor.buffer.read(cx).snapshot(cx).text();
        assert_eq!(buffer_text, "\n\n\n\n\naaaa\nbbbb\n\ncccc");
    });
}

#[gpui::test]
fn test_move_line_up_down_with_blocks(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple(&sample_text(10, 5, 'a'), cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        let snapshot = editor.buffer.read(cx).snapshot(cx);
        editor.insert_blocks(
            [BlockProperties {
                style: BlockStyle::Fixed,
                placement: BlockPlacement::Below(snapshot.anchor_after(Point::new(2, 0))),
                height: Some(1),
                render: Arc::new(|_| div().into_any()),
                priority: 0,
            }],
            Some(Autoscroll::fit()),
            cx,
        );
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(2, 0)..Point::new(2, 0)])
        });
        editor.move_line_down(&MoveLineDown, window, cx);
    });
}

#[gpui::test]
async fn test_selections_and_replace_blocks(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(
        &"
            ˇzero
            one
            two
            three
            four
            five
        "
        .unindent(),
    );

    // Create a four-line block that replaces three lines of text.
    cx.update_editor(|editor, window, cx| {
        let snapshot = editor.snapshot(window, cx);
        let snapshot = &snapshot.buffer_snapshot();
        let placement = BlockPlacement::Replace(
            snapshot.anchor_after(Point::new(1, 0))..=snapshot.anchor_after(Point::new(3, 0)),
        );
        editor.insert_blocks(
            [BlockProperties {
                placement,
                height: Some(4),
                style: BlockStyle::Sticky,
                render: Arc::new(|_| gpui::div().into_any_element()),
                priority: 0,
            }],
            None,
            cx,
        );
    });

    // Move down so that the cursor touches the block.
    cx.update_editor(|editor, window, cx| {
        editor.move_down(&Default::default(), window, cx);
    });
    cx.assert_editor_state(
        &"
            zero
            «one
            two
            threeˇ»
            four
            five
        "
        .unindent(),
    );

    // Move down past the block.
    cx.update_editor(|editor, window, cx| {
        editor.move_down(&Default::default(), window, cx);
    });
    cx.assert_editor_state(
        &"
            zero
            one
            two
            three
            ˇfour
            five
        "
        .unindent(),
    );
}

#[gpui::test]
fn test_transpose(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    _ = cx.add_window(|window, cx| {
        let mut editor = build_editor(MultiBuffer::build_simple("abc", cx), window, cx);
        editor.set_style(EditorStyle::default(), window, cx);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(1)..MultiBufferOffset(1)])
        });
        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "bac");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [MultiBufferOffset(2)..MultiBufferOffset(2)]
        );

        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "bca");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [MultiBufferOffset(3)..MultiBufferOffset(3)]
        );

        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "bac");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [MultiBufferOffset(3)..MultiBufferOffset(3)]
        );

        editor
    });

    _ = cx.add_window(|window, cx| {
        let mut editor = build_editor(MultiBuffer::build_simple("abc\nde", cx), window, cx);
        editor.set_style(EditorStyle::default(), window, cx);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(3)..MultiBufferOffset(3)])
        });
        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "acb\nde");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [MultiBufferOffset(3)..MultiBufferOffset(3)]
        );

        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(4)..MultiBufferOffset(4)])
        });
        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "acbd\ne");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [MultiBufferOffset(5)..MultiBufferOffset(5)]
        );

        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "acbde\n");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [MultiBufferOffset(6)..MultiBufferOffset(6)]
        );

        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "acbd\ne");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [MultiBufferOffset(6)..MultiBufferOffset(6)]
        );

        editor
    });

    _ = cx.add_window(|window, cx| {
        let mut editor = build_editor(MultiBuffer::build_simple("abc\nde", cx), window, cx);
        editor.set_style(EditorStyle::default(), window, cx);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([
                MultiBufferOffset(1)..MultiBufferOffset(1),
                MultiBufferOffset(2)..MultiBufferOffset(2),
                MultiBufferOffset(4)..MultiBufferOffset(4),
            ])
        });
        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "bacd\ne");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [
                MultiBufferOffset(2)..MultiBufferOffset(2),
                MultiBufferOffset(3)..MultiBufferOffset(3),
                MultiBufferOffset(5)..MultiBufferOffset(5)
            ]
        );

        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "bcade\n");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [
                MultiBufferOffset(3)..MultiBufferOffset(3),
                MultiBufferOffset(4)..MultiBufferOffset(4),
                MultiBufferOffset(6)..MultiBufferOffset(6)
            ]
        );

        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "bcda\ne");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [
                MultiBufferOffset(4)..MultiBufferOffset(4),
                MultiBufferOffset(6)..MultiBufferOffset(6)
            ]
        );

        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "bcade\n");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [
                MultiBufferOffset(4)..MultiBufferOffset(4),
                MultiBufferOffset(6)..MultiBufferOffset(6)
            ]
        );

        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "bcaed\n");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [
                MultiBufferOffset(5)..MultiBufferOffset(5),
                MultiBufferOffset(6)..MultiBufferOffset(6)
            ]
        );

        editor
    });

    _ = cx.add_window(|window, cx| {
        let mut editor = build_editor(MultiBuffer::build_simple("🍐🏀✋", cx), window, cx);
        editor.set_style(EditorStyle::default(), window, cx);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(4)..MultiBufferOffset(4)])
        });
        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "🏀🍐✋");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [MultiBufferOffset(8)..MultiBufferOffset(8)]
        );

        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "🏀✋🍐");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [MultiBufferOffset(11)..MultiBufferOffset(11)]
        );

        editor.transpose(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "🏀🍐✋");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [MultiBufferOffset(11)..MultiBufferOffset(11)]
        );

        editor
    });
}

#[gpui::test]
async fn test_rewrap(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.languages.0.extend([
            (
                "Markdown".into(),
                LanguageSettingsContent {
                    allow_rewrap: Some(language_settings::RewrapBehavior::Anywhere),
                    preferred_line_length: Some(40),
                    ..Default::default()
                },
            ),
            (
                "Plain Text".into(),
                LanguageSettingsContent {
                    allow_rewrap: Some(language_settings::RewrapBehavior::Anywhere),
                    preferred_line_length: Some(40),
                    ..Default::default()
                },
            ),
            (
                "C++".into(),
                LanguageSettingsContent {
                    allow_rewrap: Some(language_settings::RewrapBehavior::InComments),
                    preferred_line_length: Some(40),
                    ..Default::default()
                },
            ),
            (
                "Python".into(),
                LanguageSettingsContent {
                    allow_rewrap: Some(language_settings::RewrapBehavior::InComments),
                    preferred_line_length: Some(40),
                    ..Default::default()
                },
            ),
            (
                "Rust".into(),
                LanguageSettingsContent {
                    allow_rewrap: Some(language_settings::RewrapBehavior::InComments),
                    preferred_line_length: Some(40),
                    ..Default::default()
                },
            ),
        ])
    });

    let mut cx = EditorTestContext::new(cx).await;

    let cpp_language = Arc::new(Language::new(
        LanguageConfig {
            name: "C++".into(),
            line_comments: vec!["// ".into()],
            ..LanguageConfig::default()
        },
        None,
    ));
    let python_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Python".into(),
            line_comments: vec!["# ".into()],
            ..LanguageConfig::default()
        },
        None,
    ));
    let markdown_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Markdown".into(),
            rewrap_prefixes: vec![
                regex::Regex::new("\\d+\\.\\s+").unwrap(),
                regex::Regex::new("[-*+]\\s+").unwrap(),
            ],
            ..LanguageConfig::default()
        },
        None,
    ));
    let rust_language = Arc::new(
        Language::new(
            LanguageConfig {
                name: "Rust".into(),
                line_comments: vec!["// ".into(), "/// ".into()],
                ..LanguageConfig::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_override_query("[(line_comment)(block_comment)] @comment.inclusive")
        .unwrap(),
    );

    let plaintext_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Plain Text".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    // Test basic rewrapping of a long line with a cursor
    assert_rewrap(
        indoc! {"
            // ˇThis is a long comment that needs to be wrapped.
        "},
        indoc! {"
            // ˇThis is a long comment that needs to
            // be wrapped.
        "},
        cpp_language.clone(),
        &mut cx,
    );

    // Test rewrapping a full selection
    assert_rewrap(
        indoc! {"
            «// This selected long comment needs to be wrapped.ˇ»"
        },
        indoc! {"
            «// This selected long comment needs to
            // be wrapped.ˇ»"
        },
        cpp_language.clone(),
        &mut cx,
    );

    // Test multiple cursors on different lines within the same paragraph are preserved after rewrapping
    assert_rewrap(
        indoc! {"
            // ˇThis is the first line.
            // Thisˇ is the second line.
            // This is the thirdˇ line, all part of one paragraph.
         "},
        indoc! {"
            // ˇThis is the first line. Thisˇ is the
            // second line. This is the thirdˇ line,
            // all part of one paragraph.
         "},
        cpp_language.clone(),
        &mut cx,
    );

    // Test multiple cursors in different paragraphs trigger separate rewraps
    assert_rewrap(
        indoc! {"
            // ˇThis is the first paragraph, first line.
            // ˇThis is the first paragraph, second line.

            // ˇThis is the second paragraph, first line.
            // ˇThis is the second paragraph, second line.
        "},
        indoc! {"
            // ˇThis is the first paragraph, first
            // line. ˇThis is the first paragraph,
            // second line.

            // ˇThis is the second paragraph, first
            // line. ˇThis is the second paragraph,
            // second line.
        "},
        cpp_language.clone(),
        &mut cx,
    );

    // Test that change in comment prefix (e.g., `//` to `///`) trigger separate rewraps
    assert_rewrap(
        indoc! {"
            «// A regular long long comment to be wrapped.
            /// A documentation long comment to be wrapped.ˇ»
          "},
        indoc! {"
            «// A regular long long comment to be
            // wrapped.
            /// A documentation long comment to be
            /// wrapped.ˇ»
          "},
        rust_language.clone(),
        &mut cx,
    );

    // Test that change in indentation level trigger separate rewraps
    assert_rewrap(
        indoc! {"
            fn foo() {
                «// This is a long comment at the base indent.
                    // This is a long comment at the next indent.ˇ»
            }
        "},
        indoc! {"
            fn foo() {
                «// This is a long comment at the
                // base indent.
                    // This is a long comment at the
                    // next indent.ˇ»
            }
        "},
        rust_language.clone(),
        &mut cx,
    );

    // Test that different comment prefix characters (e.g., '#') are handled correctly
    assert_rewrap(
        indoc! {"
            # ˇThis is a long comment using a pound sign.
        "},
        indoc! {"
            # ˇThis is a long comment using a pound
            # sign.
        "},
        python_language,
        &mut cx,
    );

    // Test rewrapping only affects comments, not code even when selected
    assert_rewrap(
        indoc! {"
            «/// This doc comment is long and should be wrapped.
            fn my_func(a: u32, b: u32, c: u32, d: u32, e: u32, f: u32) {}ˇ»
        "},
        indoc! {"
            «/// This doc comment is long and should
            /// be wrapped.
            fn my_func(a: u32, b: u32, c: u32, d: u32, e: u32, f: u32) {}ˇ»
        "},
        rust_language.clone(),
        &mut cx,
    );

    // Test that rewrapping works in Markdown documents where `allow_rewrap` is `Anywhere`
    assert_rewrap(
        indoc! {"
            # Header

            A long long long line of markdown text to wrap.ˇ
         "},
        indoc! {"
            # Header

            A long long long line of markdown text
            to wrap.ˇ
         "},
        markdown_language.clone(),
        &mut cx,
    );

    // Test that rewrapping boundary works and preserves relative indent for Markdown documents
    assert_rewrap(
        indoc! {"
            «1. This is a numbered list item that is very long and needs to be wrapped properly.
            2. This is a numbered list item that is very long and needs to be wrapped properly.
            - This is an unordered list item that is also very long and should not merge with the numbered item.ˇ»
        "},
        indoc! {"
            «1. This is a numbered list item that is
               very long and needs to be wrapped
               properly.
            2. This is a numbered list item that is
               very long and needs to be wrapped
               properly.
            - This is an unordered list item that is
              also very long and should not merge
              with the numbered item.ˇ»
        "},
        markdown_language.clone(),
        &mut cx,
    );

    // Test that rewrapping add indents for rewrapping boundary if not exists already.
    assert_rewrap(
        indoc! {"
            «1. This is a numbered list item that is
            very long and needs to be wrapped
            properly.
            2. This is a numbered list item that is
            very long and needs to be wrapped
            properly.
            - This is an unordered list item that is
            also very long and should not merge with
            the numbered item.ˇ»
        "},
        indoc! {"
            «1. This is a numbered list item that is
               very long and needs to be wrapped
               properly.
            2. This is a numbered list item that is
               very long and needs to be wrapped
               properly.
            - This is an unordered list item that is
              also very long and should not merge
              with the numbered item.ˇ»
        "},
        markdown_language.clone(),
        &mut cx,
    );

    // Test that rewrapping maintain indents even when they already exists.
    assert_rewrap(
        indoc! {"
            «1. This is a numbered list
               item that is very long and needs to be wrapped properly.
            2. This is a numbered list
               item that is very long and needs to be wrapped properly.
            - This is an unordered list item that is also very long and
              should not merge with the numbered item.ˇ»
        "},
        indoc! {"
            «1. This is a numbered list item that is
               very long and needs to be wrapped
               properly.
            2. This is a numbered list item that is
               very long and needs to be wrapped
               properly.
            - This is an unordered list item that is
              also very long and should not merge
              with the numbered item.ˇ»
        "},
        markdown_language.clone(),
        &mut cx,
    );

    // Test that empty selection rewrap on a numbered list item does not merge adjacent items
    assert_rewrap(
        indoc! {"
            1. This is the first numbered list item that is very long and needs to be wrapped properly.
            2. ˇThis is the second numbered list item that is also very long and needs to be wrapped.
            3. This is the third numbered list item, shorter.
        "},
        indoc! {"
            1. This is the first numbered list item
               that is very long and needs to be
               wrapped properly.
            2. ˇThis is the second numbered list item
               that is also very long and needs to
               be wrapped.
            3. This is the third numbered list item,
               shorter.
        "},
        markdown_language.clone(),
        &mut cx,
    );

    // Test that empty selection rewrap on a bullet list item does not merge adjacent items
    assert_rewrap(
        indoc! {"
            - This is the first bullet item that is very long and needs wrapping properly here.
            - ˇThis is the second bullet item that is also very long and needs to be wrapped.
            - This is the third bullet item, shorter.
        "},
        indoc! {"
            - This is the first bullet item that is
              very long and needs wrapping properly
              here.
            - ˇThis is the second bullet item that is
              also very long and needs to be
              wrapped.
            - This is the third bullet item,
              shorter.
        "},
        markdown_language,
        &mut cx,
    );

    // Test that rewrapping works in plain text where `allow_rewrap` is `Anywhere`
    assert_rewrap(
        indoc! {"
            ˇThis is a very long line of plain text that will be wrapped.
        "},
        indoc! {"
            ˇThis is a very long line of plain text
            that will be wrapped.
        "},
        plaintext_language.clone(),
        &mut cx,
    );

    // Test that non-commented code acts as a paragraph boundary within a selection
    assert_rewrap(
        indoc! {"
               «// This is the first long comment block to be wrapped.
               fn my_func(a: u32);
               // This is the second long comment block to be wrapped.ˇ»
           "},
        indoc! {"
               «// This is the first long comment block
               // to be wrapped.
               fn my_func(a: u32);
               // This is the second long comment block
               // to be wrapped.ˇ»
           "},
        rust_language,
        &mut cx,
    );

    // Test rewrapping multiple selections, including ones with blank lines or tabs
    assert_rewrap(
        indoc! {"
            «ˇThis is a very long line that will be wrapped.

            This is another paragraph in the same selection.»

            «\tThis is a very long indented line that will be wrapped.ˇ»
         "},
        indoc! {"
            «ˇThis is a very long line that will be
            wrapped.

            This is another paragraph in the same
            selection.»

            «\tThis is a very long indented line
            \tthat will be wrapped.ˇ»
         "},
        plaintext_language,
        &mut cx,
    );

    // Test that an empty comment line acts as a paragraph boundary
    assert_rewrap(
        indoc! {"
            // ˇThis is a long comment that will be wrapped.
            //
            // And this is another long comment that will also be wrapped.ˇ
         "},
        indoc! {"
            // ˇThis is a long comment that will be
            // wrapped.
            //
            // And this is another long comment that
            // will also be wrapped.ˇ
         "},
        cpp_language,
        &mut cx,
    );

    #[track_caller]
    fn assert_rewrap(
        unwrapped_text: &str,
        wrapped_text: &str,
        language: Arc<Language>,
        cx: &mut EditorTestContext,
    ) {
        cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
        cx.set_state(unwrapped_text);
        cx.update_editor(|e, _, cx| e.rewrap(RewrapOptions::default(), cx));
        cx.assert_editor_state(wrapped_text);
    }
}

#[gpui::test]
async fn test_rewrap_block_comments(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.languages.0.extend([(
            "Rust".into(),
            LanguageSettingsContent {
                allow_rewrap: Some(language_settings::RewrapBehavior::InComments),
                preferred_line_length: Some(40),
                ..Default::default()
            },
        )])
    });

    let mut cx = EditorTestContext::new(cx).await;

    let rust_lang = Arc::new(
        Language::new(
            LanguageConfig {
                name: "Rust".into(),
                line_comments: vec!["// ".into()],
                block_comment: Some(BlockCommentConfig {
                    start: "/*".into(),
                    end: "*/".into(),
                    prefix: "* ".into(),
                    tab_size: 1,
                }),
                documentation_comment: Some(BlockCommentConfig {
                    start: "/**".into(),
                    end: "*/".into(),
                    prefix: "* ".into(),
                    tab_size: 1,
                }),

                ..LanguageConfig::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_override_query("[(line_comment) (block_comment)] @comment.inclusive")
        .unwrap(),
    );

    // regular block comment
    assert_rewrap(
        indoc! {"
            /*
             *ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit.
             */
            /*ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit. */
        "},
        indoc! {"
            /*
             *ˇ Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */
            /*
             *ˇ Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */
        "},
        rust_lang.clone(),
        &mut cx,
    );

    // indent is respected
    assert_rewrap(
        indoc! {"
            {}
                /*ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit. */
        "},
        indoc! {"
            {}
                /*
                 *ˇ Lorem ipsum dolor sit amet,
                 * consectetur adipiscing elit.
                 */
        "},
        rust_lang.clone(),
        &mut cx,
    );

    // short block comments with inline delimiters
    assert_rewrap(
        indoc! {"
            /*ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit. */
            /*ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit.
             */
            /*
             *ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit. */
        "},
        indoc! {"
            /*
             *ˇ Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */
            /*
             *ˇ Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */
            /*
             *ˇ Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */
        "},
        rust_lang.clone(),
        &mut cx,
    );

    // multiline block comment with inline start/end delimiters
    assert_rewrap(
        indoc! {"
            /*ˇ Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit. */
        "},
        indoc! {"
            /*
             *ˇ Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */
        "},
        rust_lang.clone(),
        &mut cx,
    );

    // block comment rewrap still respects paragraph bounds
    assert_rewrap(
        indoc! {"
            /*
             *ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit.
             *
             * Lorem ipsum dolor sit amet, consectetur adipiscing elit.
             */
        "},
        indoc! {"
            /*
             *ˇ Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             *
             * Lorem ipsum dolor sit amet, consectetur adipiscing elit.
             */
        "},
        rust_lang.clone(),
        &mut cx,
    );

    // documentation comments
    assert_rewrap(
        indoc! {"
            /**ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit. */
            /**
             *ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit.
             */
        "},
        indoc! {"
            /**
             *ˇ Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */
            /**
             *ˇ Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */
        "},
        rust_lang.clone(),
        &mut cx,
    );

    // different, adjacent comments
    assert_rewrap(
        indoc! {"
            /**
             *ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit.
             */
            /*ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit. */
            //ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit.
        "},
        indoc! {"
            /**
             *ˇ Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */
            /*
             *ˇ Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */
            //ˇ Lorem ipsum dolor sit amet,
            // consectetur adipiscing elit.
        "},
        rust_lang.clone(),
        &mut cx,
    );

    // selection w/ single short block comment
    assert_rewrap(
        indoc! {"
            «/* Lorem ipsum dolor sit amet, consectetur adipiscing elit. */ˇ»
        "},
        indoc! {"
            «/*
             * Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */ˇ»
        "},
        rust_lang.clone(),
        &mut cx,
    );

    // rewrapping a single comment w/ abutting comments
    assert_rewrap(
        indoc! {"
            /* ˇLorem ipsum dolor sit amet, consectetur adipiscing elit. */
            /* Lorem ipsum dolor sit amet, consectetur adipiscing elit. */
        "},
        indoc! {"
            /*
             * ˇLorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */
            /* Lorem ipsum dolor sit amet, consectetur adipiscing elit. */
        "},
        rust_lang.clone(),
        &mut cx,
    );

    // selection w/ non-abutting short block comments
    assert_rewrap(
        indoc! {"
            «/* Lorem ipsum dolor sit amet, consectetur adipiscing elit. */

            /* Lorem ipsum dolor sit amet, consectetur adipiscing elit. */ˇ»
        "},
        indoc! {"
            «/*
             * Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */

            /*
             * Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */ˇ»
        "},
        rust_lang.clone(),
        &mut cx,
    );

    // selection of multiline block comments
    assert_rewrap(
        indoc! {"
            «/* Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit. */ˇ»
        "},
        indoc! {"
            «/*
             * Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */ˇ»
        "},
        rust_lang.clone(),
        &mut cx,
    );

    // partial selection of multiline block comments
    assert_rewrap(
        indoc! {"
            «/* Lorem ipsum dolor sit amet,ˇ»
             * consectetur adipiscing elit. */
            /* Lorem ipsum dolor sit amet,
             «* consectetur adipiscing elit. */ˇ»
        "},
        indoc! {"
            «/*
             * Lorem ipsum dolor sit amet,ˇ»
             * consectetur adipiscing elit. */
            /* Lorem ipsum dolor sit amet,
             «* consectetur adipiscing elit.
             */ˇ»
        "},
        rust_lang.clone(),
        &mut cx,
    );

    // selection w/ abutting short block comments
    // TODO: should not be combined; should rewrap as 2 comments
    assert_rewrap(
        indoc! {"
            «/* Lorem ipsum dolor sit amet, consectetur adipiscing elit. */
            /* Lorem ipsum dolor sit amet, consectetur adipiscing elit. */ˇ»
        "},
        // desired behavior:
        // indoc! {"
        //     «/*
        //      * Lorem ipsum dolor sit amet,
        //      * consectetur adipiscing elit.
        //      */
        //     /*
        //      * Lorem ipsum dolor sit amet,
        //      * consectetur adipiscing elit.
        //      */ˇ»
        // "},
        // actual behaviour:
        indoc! {"
            «/*
             * Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit. Lorem
             * ipsum dolor sit amet, consectetur
             * adipiscing elit.
             */ˇ»
        "},
        rust_lang.clone(),
        &mut cx,
    );

    // TODO: same as above, but with delimiters on separate line
    // assert_rewrap(
    //     indoc! {"
    //         «/* Lorem ipsum dolor sit amet, consectetur adipiscing elit.
    //          */
    //         /*
    //          * Lorem ipsum dolor sit amet, consectetur adipiscing elit. */ˇ»
    //     "},
    //     // desired:
    //     // indoc! {"
    //     //     «/*
    //     //      * Lorem ipsum dolor sit amet,
    //     //      * consectetur adipiscing elit.
    //     //      */
    //     //     /*
    //     //      * Lorem ipsum dolor sit amet,
    //     //      * consectetur adipiscing elit.
    //     //      */ˇ»
    //     // "},
    //     // actual: (but with trailing w/s on the empty lines)
    //     indoc! {"
    //         «/*
    //          * Lorem ipsum dolor sit amet,
    //          * consectetur adipiscing elit.
    //          *
    //          */
    //         /*
    //          *
    //          * Lorem ipsum dolor sit amet,
    //          * consectetur adipiscing elit.
    //          */ˇ»
    //     "},
    //     rust_lang.clone(),
    //     &mut cx,
    // );

    // TODO these are unhandled edge cases; not correct, just documenting known issues
    assert_rewrap(
        indoc! {"
            /*
             //ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit.
             */
            /*
             //ˇ Lorem ipsum dolor sit amet, consectetur adipiscing elit. */
            /*ˇ Lorem ipsum dolor sit amet */ /* consectetur adipiscing elit. */
        "},
        // desired:
        // indoc! {"
        //     /*
        //      *ˇ Lorem ipsum dolor sit amet,
        //      * consectetur adipiscing elit.
        //      */
        //     /*
        //      *ˇ Lorem ipsum dolor sit amet,
        //      * consectetur adipiscing elit.
        //      */
        //     /*
        //      *ˇ Lorem ipsum dolor sit amet
        //      */ /* consectetur adipiscing elit. */
        // "},
        // actual:
        indoc! {"
            /*
             //ˇ Lorem ipsum dolor sit amet,
             // consectetur adipiscing elit.
             */
            /*
             * //ˇ Lorem ipsum dolor sit amet,
             * consectetur adipiscing elit.
             */
            /*
             *ˇ Lorem ipsum dolor sit amet */ /*
             * consectetur adipiscing elit.
             */
        "},
        rust_lang,
        &mut cx,
    );

    #[track_caller]
    fn assert_rewrap(
        unwrapped_text: &str,
        wrapped_text: &str,
        language: Arc<Language>,
        cx: &mut EditorTestContext,
    ) {
        cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
        cx.set_state(unwrapped_text);
        cx.update_editor(|e, _, cx| e.rewrap(RewrapOptions::default(), cx));
        cx.assert_editor_state(wrapped_text);
    }
}

#[gpui::test]
async fn test_rewrap_line_comment_in_go(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.languages.0.extend([(
            "Go".into(),
            LanguageSettingsContent {
                allow_rewrap: Some(language_settings::RewrapBehavior::InComments),
                preferred_line_length: Some(40),
                ..Default::default()
            },
        )])
    });

    let mut cx = EditorTestContext::new(cx).await;

    let go_lang = languages::language("go", tree_sitter_go::LANGUAGE.into());

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(go_lang), cx));
    cx.set_state(indoc! {"
        // Lorem ipsum dolor sit amet, consectetur adipiscing elit.ˇ
    "});
    cx.update_editor(|e, _, cx| e.rewrap(RewrapOptions::default(), cx));
    cx.assert_editor_state(indoc! {"
        // Lorem ipsum dolor sit amet,
        // consectetur adipiscing elit.ˇ
    "});
}

#[gpui::test]
async fn test_rewrap_line_comment_in_c(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.languages.0.extend([(
            "C".into(),
            LanguageSettingsContent {
                allow_rewrap: Some(language_settings::RewrapBehavior::InComments),
                preferred_line_length: Some(40),
                ..Default::default()
            },
        )])
    });

    let mut cx = EditorTestContext::new(cx).await;

    let c_lang = languages::language("c", tree_sitter_c::LANGUAGE.into());

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(c_lang), cx));
    cx.set_state(indoc! {"
        // Lorem ipsum dolor sit amet, consectetur adipiscing elit.ˇ
    "});
    cx.update_editor(|e, _, cx| e.rewrap(RewrapOptions::default(), cx));
    cx.assert_editor_state(indoc! {"
        // Lorem ipsum dolor sit amet,
        // consectetur adipiscing elit.ˇ
    "});
}

#[gpui::test]
async fn test_hard_wrap(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(git_commit_lang()), cx));
    cx.update_editor(|editor, _, cx| {
        editor.set_hard_wrap(Some(14), cx);
    });

    cx.set_state(indoc!(
        "
        one two three ˇ
        "
    ));
    cx.simulate_input("four");
    cx.run_until_parked();

    cx.assert_editor_state(indoc!(
        "
        one two three
        fourˇ
        "
    ));

    cx.update_editor(|editor, window, cx| {
        editor.newline(&Default::default(), window, cx);
    });
    cx.run_until_parked();
    cx.assert_editor_state(indoc!(
        "
        one two three
        four
        ˇ
        "
    ));

    cx.simulate_input("five");
    cx.run_until_parked();
    cx.assert_editor_state(indoc!(
        "
        one two three
        four
        fiveˇ
        "
    ));

    cx.update_editor(|editor, window, cx| {
        editor.newline(&Default::default(), window, cx);
    });
    cx.run_until_parked();
    cx.simulate_input("# ");
    cx.run_until_parked();
    cx.assert_editor_state(indoc!(
        "
        one two three
        four
        five
        # ˇ
        "
    ));

    cx.update_editor(|editor, window, cx| {
        editor.newline(&Default::default(), window, cx);
    });
    cx.run_until_parked();
    cx.assert_editor_state(indoc!(
        "
        one two three
        four
        five
        #\x20
        #ˇ
        "
    ));

    cx.simulate_input(" 6");
    cx.run_until_parked();
    cx.assert_editor_state(indoc!(
        "
        one two three
        four
        five
        #
        # 6ˇ
        "
    ));
}

#[gpui::test]
async fn test_cut_line_ends(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"The quick brownˇ"});
    cx.update_editor(|e, window, cx| e.cut_to_end_of_line(&CutToEndOfLine::default(), window, cx));
    cx.assert_editor_state(indoc! {"The quick brownˇ"});

    cx.set_state(indoc! {"The emacs foxˇ"});
    cx.update_editor(|e, window, cx| e.kill_ring_cut(&KillRingCut, window, cx));
    cx.assert_editor_state(indoc! {"The emacs foxˇ"});

    cx.set_state(indoc! {"
        The quick« brownˇ»
        fox jumps overˇ
        the lazy dog"});
    cx.update_editor(|e, window, cx| e.cut(&Cut, window, cx));
    cx.assert_editor_state(indoc! {"
        The quickˇ
        ˇthe lazy dog"});

    cx.set_state(indoc! {"
        The quick« brownˇ»
        fox jumps overˇ
        the lazy dog"});
    cx.update_editor(|e, window, cx| e.cut_to_end_of_line(&CutToEndOfLine::default(), window, cx));
    cx.assert_editor_state(indoc! {"
        The quickˇ
        fox jumps overˇthe lazy dog"});

    cx.set_state(indoc! {"
        The quick« brownˇ»
        fox jumps overˇ
        the lazy dog"});
    cx.update_editor(|e, window, cx| {
        e.cut_to_end_of_line(
            &CutToEndOfLine {
                stop_at_newlines: true,
            },
            window,
            cx,
        )
    });
    cx.assert_editor_state(indoc! {"
        The quickˇ
        fox jumps overˇ
        the lazy dog"});

    cx.set_state(indoc! {"
        The quick« brownˇ»
        fox jumps overˇ
        the lazy dog"});
    cx.update_editor(|e, window, cx| e.kill_ring_cut(&KillRingCut, window, cx));
    cx.assert_editor_state(indoc! {"
        The quickˇ
        fox jumps overˇthe lazy dog"});

    for selection in ["The quick «brownˇ» fox", "The quick «ˇbrown» fox"] {
        cx.set_state(selection);
        cx.update_editor(|e, window, cx| {
            e.cut_to_end_of_line(&CutToEndOfLine::default(), window, cx)
        });
        cx.assert_editor_state("The quick ˇ");
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text().as_deref().map(str::to_string)),
            Some("brown fox".to_string())
        );
    }
}

#[gpui::test]
async fn test_kill_ring_cut_accumulates_multi_line_kills(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("ˇone\ntwo");

    cx.update_editor(|e, window, cx| {
        e.kill_ring_cut(&KillRingCut, window, cx);
        e.kill_ring_cut(&KillRingCut, window, cx);
    });

    cx.update_editor(|e, window, cx| e.kill_ring_yank(&KillRingYank, window, cx));
    cx.assert_editor_state("one\nˇtwo");
}

#[gpui::test]
async fn test_kill_ring_cut_matches_emacs_kill_line_sequence_at_end_of_buffer(
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("ˇa\nb\nc");

    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));
    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));
    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));

    cx.update_editor(|editor, _, cx| {
        assert_eq!(editor.text(cx), "\nc");
    });

    cx.update_editor(|editor, window, cx| editor.kill_ring_yank(&KillRingYank, window, cx));
    cx.update_editor(|editor, _, cx| {
        assert_eq!(editor.text(cx), "a\nb\nc");
    });
}

#[gpui::test]
async fn test_kill_ring_cut_accumulates_final_line_without_trailing_newline(
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("ˇa\nb\nc");

    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));
    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));
    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));
    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));
    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));

    cx.update_editor(|editor, _, cx| {
        assert_eq!(editor.text(cx), "");
    });

    cx.update_editor(|editor, window, cx| editor.kill_ring_yank(&KillRingYank, window, cx));
    cx.update_editor(|editor, _, cx| {
        assert_eq!(editor.text(cx), "a\nb\nc");
    });
}

#[gpui::test]
async fn test_kill_ring_yank_breaks_kill_ring_cut_accumulation(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("ˇa\nb\nc");

    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));
    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));
    cx.update_editor(|editor, window, cx| editor.kill_ring_yank(&KillRingYank, window, cx));
    cx.update_editor(|editor, window, cx| editor.move_to_beginning(&MoveToBeginning, window, cx));
    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));
    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));
    cx.update_editor(|editor, window, cx| editor.kill_ring_yank(&KillRingYank, window, cx));

    cx.update_editor(|editor, _, cx| {
        assert_eq!(editor.text(cx), "a\nb\nc");
    });
}

#[gpui::test]
async fn test_kill_ring_yank_pastes_accumulated_kill_at_each_cursor(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("ˇone\ntwo");

    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));
    cx.update_editor(|editor, window, cx| editor.kill_ring_cut(&KillRingCut, window, cx));

    cx.set_state("aˇ bˇ");
    cx.update_editor(|editor, window, cx| editor.kill_ring_yank(&KillRingYank, window, cx));
    cx.assert_editor_state("aone\nˇ bone\nˇ");
}

#[gpui::test]
async fn test_clipboard(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("«one✅ ˇ»two «three ˇ»four «five ˇ»six ");
    cx.update_editor(|e, window, cx| e.cut(&Cut, window, cx));
    cx.assert_editor_state("ˇtwo ˇfour ˇsix ");

    // Paste with three cursors. Each cursor pastes one slice of the clipboard text.
    cx.set_state("two ˇfour ˇsix ˇ");
    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state("two one✅ ˇfour three ˇsix five ˇ");

    // Paste again but with only two cursors. Since the number of cursors doesn't
    // match the number of slices in the clipboard, the entire clipboard text
    // is pasted at each cursor.
    cx.set_state("ˇtwo one✅ four three six five ˇ");
    cx.update_editor(|e, window, cx| {
        e.handle_input("( ", window, cx);
        e.paste(&Paste, window, cx);
        e.handle_input(") ", window, cx);
    });
    cx.assert_editor_state(
        &([
            "( one✅ ",
            "three ",
            "five ) ˇtwo one✅ four three six five ( one✅ ",
            "three ",
            "five ) ˇ",
        ]
        .join("\n")),
    );

    // Cut with three selections, one of which is full-line.
    cx.set_state(indoc! {"
        1«2ˇ»3
        4ˇ567
        «8ˇ»9"});
    cx.update_editor(|e, window, cx| e.cut(&Cut, window, cx));
    cx.assert_editor_state(indoc! {"
        1ˇ3
        ˇ9"});

    // Paste with three selections, noticing how the copied selection that was full-line
    // gets inserted before the second cursor.
    cx.set_state(indoc! {"
        1ˇ3
        9ˇ
        «oˇ»ne"});
    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state(indoc! {"
        12ˇ3
        4567
        9ˇ
        8ˇne"});

    // Copy with a single cursor only, which writes the whole line into the clipboard.
    cx.set_state(indoc! {"
        The quick brown
        fox juˇmps over
        the lazy dog"});
    cx.update_editor(|e, window, cx| e.copy(&Copy, window, cx));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text().as_deref().map(str::to_string)),
        Some("fox jumps over\n".to_string())
    );

    // Paste with three selections, noticing how the copied full-line selection is inserted
    // before the empty selections but replaces the selection that is non-empty.
    cx.set_state(indoc! {"
        Tˇhe quick brown
        «foˇ»x jumps over
        tˇhe lazy dog"});
    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state(indoc! {"
        fox jumps over
        Tˇhe quick brown
        fox jumps over
        ˇx jumps over
        fox jumps over
        tˇhe lazy dog"});
}

#[gpui::test]
async fn test_copy_and_paste_non_empty_selection_followed_by_empty_selection(
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(indoc! {"
        «fooˇ»
        barˇ
    "});

    cx.update_editor(|editor, window, cx| editor.copy(&Copy, window, cx));
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("foo\nbar\n".to_string())
    );

    cx.update_editor(|editor, window, cx| editor.paste(&Paste, window, cx));
    cx.assert_editor_state(indoc! {"
        fooˇ
        bar
        barˇ
    "});
}

#[gpui::test]
async fn test_copy_trim(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(
        r#"            «for selection in selections.iter() {
            let mut start = selection.start;
            let mut end = selection.end;
            let is_entire_line = selection.is_empty();
            if is_entire_line {
                start = Point::new(start.row, 0);ˇ»
                end = cmp::min(max_point, Point::new(end.row + 1, 0));
            }
        "#,
    );
    cx.update_editor(|e, window, cx| e.copy(&Copy, window, cx));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text().as_deref().map(str::to_string)),
        Some(
            "for selection in selections.iter() {
            let mut start = selection.start;
            let mut end = selection.end;
            let is_entire_line = selection.is_empty();
            if is_entire_line {
                start = Point::new(start.row, 0);"
                .to_string()
        ),
        "Regular copying preserves all indentation selected",
    );
    cx.update_editor(|e, window, cx| e.copy_and_trim(&CopyAndTrim, window, cx));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text().as_deref().map(str::to_string)),
        Some(
            "for selection in selections.iter() {
let mut start = selection.start;
let mut end = selection.end;
let is_entire_line = selection.is_empty();
if is_entire_line {
    start = Point::new(start.row, 0);"
                .to_string()
        ),
        "Copying with stripping should strip all leading whitespaces"
    );

    cx.set_state(
        r#"       «     for selection in selections.iter() {
            let mut start = selection.start;
            let mut end = selection.end;
            let is_entire_line = selection.is_empty();
            if is_entire_line {
                start = Point::new(start.row, 0);ˇ»
                end = cmp::min(max_point, Point::new(end.row + 1, 0));
            }
        "#,
    );
    cx.update_editor(|e, window, cx| e.copy(&Copy, window, cx));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text().as_deref().map(str::to_string)),
        Some(
            "     for selection in selections.iter() {
            let mut start = selection.start;
            let mut end = selection.end;
            let is_entire_line = selection.is_empty();
            if is_entire_line {
                start = Point::new(start.row, 0);"
                .to_string()
        ),
        "Regular copying preserves all indentation selected",
    );
    cx.update_editor(|e, window, cx| e.copy_and_trim(&CopyAndTrim, window, cx));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text().as_deref().map(str::to_string)),
        Some(
            "for selection in selections.iter() {
let mut start = selection.start;
let mut end = selection.end;
let is_entire_line = selection.is_empty();
if is_entire_line {
    start = Point::new(start.row, 0);"
                .to_string()
        ),
        "Copying with stripping should strip all leading whitespaces, even if some of it was selected"
    );

    cx.set_state(
        r#"       «ˇ     for selection in selections.iter() {
            let mut start = selection.start;
            let mut end = selection.end;
            let is_entire_line = selection.is_empty();
            if is_entire_line {
                start = Point::new(start.row, 0);»
                end = cmp::min(max_point, Point::new(end.row + 1, 0));
            }
        "#,
    );
    cx.update_editor(|e, window, cx| e.copy(&Copy, window, cx));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text().as_deref().map(str::to_string)),
        Some(
            "     for selection in selections.iter() {
            let mut start = selection.start;
            let mut end = selection.end;
            let is_entire_line = selection.is_empty();
            if is_entire_line {
                start = Point::new(start.row, 0);"
                .to_string()
        ),
        "Regular copying for reverse selection works the same",
    );
    cx.update_editor(|e, window, cx| e.copy_and_trim(&CopyAndTrim, window, cx));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text().as_deref().map(str::to_string)),
        Some(
            "for selection in selections.iter() {
let mut start = selection.start;
let mut end = selection.end;
let is_entire_line = selection.is_empty();
if is_entire_line {
    start = Point::new(start.row, 0);"
                .to_string()
        ),
        "Copying with stripping for reverse selection works the same"
    );

    cx.set_state(
        r#"            for selection «in selections.iter() {
            let mut start = selection.start;
            let mut end = selection.end;
            let is_entire_line = selection.is_empty();
            if is_entire_line {
                start = Point::new(start.row, 0);ˇ»
                end = cmp::min(max_point, Point::new(end.row + 1, 0));
            }
        "#,
    );
    cx.update_editor(|e, window, cx| e.copy(&Copy, window, cx));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text().as_deref().map(str::to_string)),
        Some(
            "in selections.iter() {
            let mut start = selection.start;
            let mut end = selection.end;
            let is_entire_line = selection.is_empty();
            if is_entire_line {
                start = Point::new(start.row, 0);"
                .to_string()
        ),
        "When selecting past the indent, the copying works as usual",
    );
    cx.update_editor(|e, window, cx| e.copy_and_trim(&CopyAndTrim, window, cx));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text().as_deref().map(str::to_string)),
        Some(
            "in selections.iter() {
            let mut start = selection.start;
            let mut end = selection.end;
            let is_entire_line = selection.is_empty();
            if is_entire_line {
                start = Point::new(start.row, 0);"
                .to_string()
        ),
        "When selecting past the indent, nothing is trimmed"
    );

    cx.set_state(
        r#"            «for selection in selections.iter() {
            let mut start = selection.start;

            let mut end = selection.end;
            let is_entire_line = selection.is_empty();
            if is_entire_line {
                start = Point::new(start.row, 0);
ˇ»                end = cmp::min(max_point, Point::new(end.row + 1, 0));
            }
        "#,
    );
    cx.update_editor(|e, window, cx| e.copy_and_trim(&CopyAndTrim, window, cx));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text().as_deref().map(str::to_string)),
        Some(
            "for selection in selections.iter() {
let mut start = selection.start;

let mut end = selection.end;
let is_entire_line = selection.is_empty();
if is_entire_line {
    start = Point::new(start.row, 0);
"
            .to_string()
        ),
        "Copying with stripping should ignore empty lines"
    );
}

#[gpui::test]
async fn test_copy_trim_line_mode(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"
        «    fn main() {
                1
            }ˇ»
    "});
    cx.update_editor(|editor, _window, _cx| editor.selections.set_line_mode(true));
    cx.update_editor(|editor, window, cx| editor.copy_and_trim(&CopyAndTrim, window, cx));

    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("fn main() {\n    1\n}\n".to_string())
    );

    let clipboard_selections: Vec<ClipboardSelection> = cx
        .read_from_clipboard()
        .and_then(|item| item.entries().first().cloned())
        .and_then(|entry| match entry {
            gpui::ClipboardEntry::String(text) => text.metadata_json(),
            _ => None,
        })
        .expect("should have clipboard selections");

    assert_eq!(clipboard_selections.len(), 1);
    assert!(clipboard_selections[0].is_entire_line);

    cx.set_state(indoc! {"
        «fn main() {
            1
        }ˇ»
    "});
    cx.update_editor(|editor, _window, _cx| editor.selections.set_line_mode(true));
    cx.update_editor(|editor, window, cx| editor.copy_and_trim(&CopyAndTrim, window, cx));

    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("fn main() {\n    1\n}\n".to_string())
    );

    let clipboard_selections: Vec<ClipboardSelection> = cx
        .read_from_clipboard()
        .and_then(|item| item.entries().first().cloned())
        .and_then(|entry| match entry {
            gpui::ClipboardEntry::String(text) => text.metadata_json(),
            _ => None,
        })
        .expect("should have clipboard selections");

    assert_eq!(clipboard_selections.len(), 1);
    assert!(clipboard_selections[0].is_entire_line);
}

#[gpui::test]
async fn test_clipboard_line_numbers_from_multibuffer(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    fs.insert_file(
        path!("/file.txt"),
        "first line\nsecond line\nthird line\nfourth line\nfifth line\n".into(),
    )
    .await;

    let project = Project::test(fs, [path!("/file.txt").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/file.txt"), cx)
        })
        .await
        .unwrap();

    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer.clone(),
            [Point::new(2, 0)..Point::new(5, 0)],
            0,
            cx,
        );
        multibuffer
    });

    let (editor, cx) = cx.add_window_view(|window, cx| {
        build_editor_with_project(project.clone(), multibuffer, window, cx)
    });

    editor.update_in(cx, |editor, window, cx| {
        assert_eq!(editor.text(cx), "third line\nfourth line\nfifth line\n");

        editor.select_all(&SelectAll, window, cx);
        editor.copy(&Copy, window, cx);
    });

    let clipboard_selections: Option<Vec<ClipboardSelection>> = cx
        .read_from_clipboard()
        .and_then(|item| item.entries().first().cloned())
        .and_then(|entry| match entry {
            gpui::ClipboardEntry::String(text) => text.metadata_json(),
            _ => None,
        });

    let selections = clipboard_selections.expect("should have clipboard selections");
    assert_eq!(selections.len(), 1);
    let selection = &selections[0];
    assert_eq!(
        selection.line_range,
        Some(2..=5),
        "line range should be from original file (rows 2-5), not multibuffer rows (0-2)"
    );
}

#[gpui::test]
async fn test_copy_file_location_from_multibuffer(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/root"),
        json!({
            "file.txt": "first line\nsecond line\nthird line\nfourth line\nfifth line\n",
        }),
    )
    .await;

    let project = Project::test(fs, [path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/root/file.txt"), cx)
        })
        .await
        .unwrap();

    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer.clone(),
            [Point::new(2, 0)..Point::new(5, 0)],
            0,
            cx,
        );
        multibuffer
    });

    let (editor, cx) = cx.add_window_view(|window, cx| {
        build_editor_with_project(project.clone(), multibuffer, window, cx)
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(0, 0)..Point::new(0, 0)]);
        });
        editor.copy_file_location(&CopyFileLocation, window, cx);
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("file.txt:3".to_string()),
        "cursor on the first excerpt row should report its line in the original file"
    );

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(1, 0)..Point::new(2, 0)]);
        });
        editor.copy_file_location(&CopyFileLocation, window, cx);
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("file.txt:4".to_string()),
        "a selection ending at the start of a row should not include that row"
    );

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(0, 0)..Point::new(2, 3)]);
        });
        editor.copy_file_location(&CopyFileLocation, window, cx);
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("file.txt:3-5".to_string()),
        "a multi-row selection should report the original file's line range"
    );
}

#[gpui::test]
async fn test_copy_file_location_across_buffers(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/root"),
        json!({
            "one.txt": "one\ntwo\nthree\n",
            "two.txt": "four\nfive\nsix\n",
        }),
    )
    .await;

    let project = Project::test(fs, [path!("/root").as_ref()], cx).await;

    let buffer_1 = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/root/one.txt"), cx)
        })
        .await
        .unwrap();
    let buffer_2 = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/root/two.txt"), cx)
        })
        .await
        .unwrap();

    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1,
            [Point::new(1, 0)..Point::new(3, 0)],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(1),
            buffer_2,
            [Point::new(1, 0)..Point::new(3, 0)],
            0,
            cx,
        );
        multibuffer
    });

    let (editor, cx) = cx.add_window_view(|window, cx| {
        build_editor_with_project(project.clone(), multibuffer, window, cx)
    });

    // The multi-buffer reads "two\nthree\n\nfive\nsix\n", so row 3 is the first
    // row of the second buffer's excerpt. Select from the first excerpt into it.
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(0, 0)..Point::new(3, 2)]);
        });
        editor.copy_file_location(&CopyFileLocation, window, cx);
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("two.txt:2".to_string()),
        "a selection spanning buffers should report the location of the cursor"
    );

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(3, 2)..Point::new(0, 0)]);
        });
        editor.copy_file_location(&CopyFileLocation, window, cx);
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("one.txt:2".to_string()),
        "a reversed selection spanning buffers should report the cursor's location in the first buffer"
    );
}

#[gpui::test]
async fn test_copy_file_location_with_deleted_hunk(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("ˇaaa\nbbb\nccc");
    cx.set_head_text("aaa\nXXX\nbbb\nccc");
    cx.run_until_parked();
    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&Default::default(), window, cx);
    });
    cx.run_until_parked();

    // Multi-buffer rows: 0 "aaa", 1 "XXX" (deleted hunk), 2 "bbb", 3 "ccc"
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(0, 0)..Point::new(2, 2)]);
        });
        editor.copy_file_location(&CopyFileLocation, window, cx);
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("file:2".to_string()),
        "a selection crossing a deleted hunk should report the location of the cursor"
    );

    cx.update_editor(|editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(0, 0)..Point::new(0, 0)]);
        });
        editor.copy_file_location(&CopyFileLocation, window, cx);
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("file:1".to_string())
    );

    cx.update_editor(|editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(1, 1)..Point::new(1, 1)]);
        });
        editor.copy_file_location(&CopyFileLocation, window, cx);
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("file:1".to_string()),
        "a cursor inside a deleted hunk has no file location, so the clipboard is unchanged"
    );
}

#[gpui::test]
async fn test_copy_file_location_in_singleton_buffer(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/root"),
        json!({
            "file.txt": "first line\nsecond line\nthird line\n",
        }),
    )
    .await;

    let project = Project::test(fs, [path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/root/file.txt"), cx)
        })
        .await
        .unwrap();
    let multibuffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));

    let (editor, cx) = cx.add_window_view(|window, cx| {
        build_editor_with_project(project.clone(), multibuffer, window, cx)
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(1, 0)..Point::new(1, 0)]);
        });
        editor.copy_file_location(&CopyFileLocation, window, cx);
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("file.txt:2".to_string())
    );

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(Default::default(), window, cx, |selections| {
            selections.select_ranges([Point::new(0, 0)..Point::new(2, 4)]);
        });
        editor.copy_file_location(&CopyFileLocation, window, cx);
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("file.txt:1-3".to_string())
    );
}

#[gpui::test]
async fn test_paste_multiline(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(rust_lang()), cx));

    // Cut an indented block, without the leading whitespace.
    cx.set_state(indoc! {"
        const a: B = (
            c(),
            «d(
                e,
                f
            )ˇ»
        );
    "});
    cx.update_editor(|e, window, cx| e.cut(&Cut, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: B = (
            c(),
            ˇ
        );
    "});

    // Paste it at the same position.
    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: B = (
            c(),
            d(
                e,
                f
            )ˇ
        );
    "});

    // Paste it at a line with a lower indent level.
    cx.set_state(indoc! {"
        ˇ
        const a: B = (
            c(),
        );
    "});
    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state(indoc! {"
        d(
            e,
            f
        )ˇ
        const a: B = (
            c(),
        );
    "});

    // Cut an indented block, with the leading whitespace.
    cx.set_state(indoc! {"
        const a: B = (
            c(),
        «    d(
                e,
                f
            )
        ˇ»);
    "});
    cx.update_editor(|e, window, cx| e.cut(&Cut, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: B = (
            c(),
        ˇ);
    "});

    // Paste it at the same position.
    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: B = (
            c(),
            d(
                e,
                f
            )
        ˇ);
    "});

    // Paste it at a line with a higher indent level.
    cx.set_state(indoc! {"
        const a: B = (
            c(),
            d(
                e,
                fˇ
            )
        );
    "});
    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: B = (
            c(),
            d(
                e,
                f    d(
                    e,
                    f
                )
        ˇ
            )
        );
    "});

    // Copy an indented block, starting mid-line
    cx.set_state(indoc! {"
        const a: B = (
            c(),
            somethin«g(
                e,
                f
            )ˇ»
        );
    "});
    cx.update_editor(|e, window, cx| e.copy(&Copy, window, cx));

    // Paste it on a line with a lower indent level
    cx.update_editor(|e, window, cx| e.move_to_end(&Default::default(), window, cx));
    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state(indoc! {"
        const a: B = (
            c(),
            something(
                e,
                f
            )
        );
        g(
            e,
            f
        )ˇ"});
}

#[gpui::test]
async fn test_paste_undo_does_not_include_preceding_edits(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.update_editor(|e, _, cx| {
        e.buffer().update(cx, |buffer, cx| {
            buffer.set_group_interval(Duration::from_secs(10), cx)
        })
    });
    // Type some text
    cx.set_state("ˇ");
    cx.update_editor(|e, window, cx| e.insert("hello", window, cx));
    // cx.assert_editor_state("helloˇ");

    // Paste some text immediately after typing
    cx.write_to_clipboard(ClipboardItem::new_string(" world".into()));
    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state("hello worldˇ");

    // Undo should only undo the paste, not the preceding typing
    cx.update_editor(|e, window, cx| e.undo(&Undo, window, cx));
    cx.assert_editor_state("helloˇ");

    // Undo again should undo the typing
    cx.update_editor(|e, window, cx| e.undo(&Undo, window, cx));
    cx.assert_editor_state("ˇ");
}

#[gpui::test]
async fn test_paste_content_from_other_app(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    cx.write_to_clipboard(ClipboardItem::new_string(
        "    d(\n        e\n    );\n".into(),
    ));

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(rust_lang()), cx));
    cx.run_until_parked();

    cx.set_state(indoc! {"
        fn a() {
            b();
            if c() {
                ˇ
            }
        }
    "});

    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state(indoc! {"
        fn a() {
            b();
            if c() {
                d(
                    e
                );
        ˇ
            }
        }
    "});

    cx.set_state(indoc! {"
        fn a() {
            b();
            ˇ
        }
    "});

    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state(indoc! {"
        fn a() {
            b();
            d(
                e
            );
        ˇ
        }
    "});
}

#[gpui::test]
async fn test_paste_multiline_from_other_app_into_matching_cursors(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    cx.write_to_clipboard(ClipboardItem::new_string("alpha\nbeta\ngamma".into()));

    let mut cx = EditorTestContext::new(cx).await;

    // Paste into 3 cursors: each cursor should receive one line.
    cx.set_state("ˇ one ˇ two ˇ three");
    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state("alphaˇ one betaˇ two gammaˇ three");

    // Paste into 2 cursors: line count doesn't match, so paste entire text at each cursor.
    cx.write_to_clipboard(ClipboardItem::new_string("alpha\nbeta\ngamma".into()));
    cx.set_state("ˇ one ˇ two");
    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state("alpha\nbeta\ngammaˇ one alpha\nbeta\ngammaˇ two");

    // Paste into a single cursor: should paste everything as-is.
    cx.write_to_clipboard(ClipboardItem::new_string("alpha\nbeta\ngamma".into()));
    cx.set_state("ˇ one");
    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state("alpha\nbeta\ngammaˇ one");

    // Paste with selections: each selection is replaced with its corresponding line.
    cx.write_to_clipboard(ClipboardItem::new_string("xx\nyy\nzz".into()));
    cx.set_state("«aˇ» one «bˇ» two «cˇ» three");
    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));
    cx.assert_editor_state("xxˇ one yyˇ two zzˇ three");
}

#[gpui::test]
fn test_select_all(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("abc\nde\nfgh", cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.select_all(&SelectAll, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(2), 3)]
        );
    });
}

#[gpui::test]
fn test_select_line(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple(&sample_text(6, 5, 'a'), cx);
        build_editor(buffer, window, cx)
    });
    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 2),
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0),
                DisplayPoint::new(DisplayRow(4), 2)..DisplayPoint::new(DisplayRow(4), 2),
            ])
        });
        editor.select_line(&SelectLine, window, cx);
        // Adjacent line selections should NOT merge (only overlapping ones do)
        assert_eq!(
            display_ranges(editor, cx),
            vec![
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(1), 0),
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(2), 0),
                DisplayPoint::new(DisplayRow(4), 0)..DisplayPoint::new(DisplayRow(5), 0),
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.select_line(&SelectLine, window, cx);
        assert_eq!(
            display_ranges(editor, cx),
            vec![
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(3), 0),
                DisplayPoint::new(DisplayRow(4), 0)..DisplayPoint::new(DisplayRow(5), 5),
            ]
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.select_line(&SelectLine, window, cx);
        // Adjacent but not overlapping, so they stay separate
        assert_eq!(
            display_ranges(editor, cx),
            vec![
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(4), 0),
                DisplayPoint::new(DisplayRow(4), 0)..DisplayPoint::new(DisplayRow(5), 5),
            ]
        );
    });
}

#[gpui::test]
async fn test_split_selection_into_lines(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    #[track_caller]
    fn test(cx: &mut EditorTestContext, initial_state: &'static str, expected_state: &'static str) {
        cx.set_state(initial_state);
        cx.update_editor(|e, window, cx| {
            e.split_selection_into_lines(&Default::default(), window, cx)
        });
        cx.assert_editor_state(expected_state);
    }

    // Selection starts and ends at the middle of lines, left-to-right
    test(
        &mut cx,
        "aa\nb«ˇb\ncc\ndd\ne»e\nff",
        "aa\nbbˇ\nccˇ\nddˇ\neˇe\nff",
    );
    // Same thing, right-to-left
    test(
        &mut cx,
        "aa\nb«b\ncc\ndd\neˇ»e\nff",
        "aa\nbbˇ\nccˇ\nddˇ\neˇe\nff",
    );

    // Whole buffer, left-to-right, last line *doesn't* end with newline
    test(
        &mut cx,
        "«ˇaa\nbb\ncc\ndd\nee\nff»",
        "aaˇ\nbbˇ\nccˇ\nddˇ\neeˇ\nffˇ",
    );
    // Same thing, right-to-left
    test(
        &mut cx,
        "«aa\nbb\ncc\ndd\nee\nffˇ»",
        "aaˇ\nbbˇ\nccˇ\nddˇ\neeˇ\nffˇ",
    );

    // Whole buffer, left-to-right, last line ends with newline
    test(
        &mut cx,
        "«ˇaa\nbb\ncc\ndd\nee\nff\n»",
        "aaˇ\nbbˇ\nccˇ\nddˇ\neeˇ\nffˇ\n",
    );
    // Same thing, right-to-left
    test(
        &mut cx,
        "«aa\nbb\ncc\ndd\nee\nff\nˇ»",
        "aaˇ\nbbˇ\nccˇ\nddˇ\neeˇ\nffˇ\n",
    );

    // Starts at the end of a line, ends at the start of another
    test(
        &mut cx,
        "aa\nbb«ˇ\ncc\ndd\nee\n»ff\n",
        "aa\nbbˇ\nccˇ\nddˇ\neeˇ\nff\n",
    );
}

#[gpui::test]
async fn test_split_selection_into_lines_does_not_scroll(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let large_body = "\nline".repeat(300);
    cx.set_state(&format!("«ˇstart{large_body}\nend»"));
    let initial_scroll_position = cx.update_editor(|editor, _, cx| editor.scroll_position(cx));

    cx.update_editor(|editor, window, cx| {
        editor.split_selection_into_lines(&Default::default(), window, cx);
    });

    let scroll_position_after_split = cx.update_editor(|editor, _, cx| editor.scroll_position(cx));
    assert_eq!(
        initial_scroll_position, scroll_position_after_split,
        "Scroll position should not change after splitting selection into lines"
    );
}

#[gpui::test]
async fn test_split_selection_into_lines_interacting_with_creases(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple(&sample_text(9, 5, 'a'), cx);
        build_editor(buffer, window, cx)
    });

    // setup
    _ = editor.update(cx, |editor, window, cx| {
        editor.fold_creases(
            vec![
                Crease::simple(Point::new(0, 2)..Point::new(1, 2), FoldPlaceholder::test()),
                Crease::simple(Point::new(2, 3)..Point::new(4, 1), FoldPlaceholder::test()),
                Crease::simple(Point::new(7, 0)..Point::new(8, 4), FoldPlaceholder::test()),
            ],
            true,
            window,
            cx,
        );
        assert_eq!(
            editor.display_text(cx),
            "aa⋯bbb\nccc⋯eeee\nfffff\nggggg\n⋯i"
        );
    });

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 2),
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0),
                DisplayPoint::new(DisplayRow(4), 4)..DisplayPoint::new(DisplayRow(4), 4),
            ])
        });
        editor.split_selection_into_lines(&Default::default(), window, cx);
        assert_eq!(
            editor.display_text(cx),
            "aaaaa\nbbbbb\nccc⋯eeee\nfffff\nggggg\n⋯i"
        );
    });
    EditorTestContext::for_editor(editor, cx)
        .await
        .assert_editor_state("aˇaˇaaa\nbbbbb\nˇccccc\nddddd\neeeee\nfffff\nggggg\nhhhhh\niiiiiˇ");

    _ = editor.update(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(5), 0)..DisplayPoint::new(DisplayRow(0), 1)
            ])
        });
        editor.split_selection_into_lines(&Default::default(), window, cx);
        assert_eq!(
            editor.display_text(cx),
            "aaaaa\nbbbbb\nccccc\nddddd\neeeee\nfffff\nggggg\nhhhhh\niiiii"
        );
        assert_eq!(
            display_ranges(editor, cx),
            [
                DisplayPoint::new(DisplayRow(0), 5)..DisplayPoint::new(DisplayRow(0), 5),
                DisplayPoint::new(DisplayRow(1), 5)..DisplayPoint::new(DisplayRow(1), 5),
                DisplayPoint::new(DisplayRow(2), 5)..DisplayPoint::new(DisplayRow(2), 5),
                DisplayPoint::new(DisplayRow(3), 5)..DisplayPoint::new(DisplayRow(3), 5),
                DisplayPoint::new(DisplayRow(4), 5)..DisplayPoint::new(DisplayRow(4), 5),
                DisplayPoint::new(DisplayRow(5), 5)..DisplayPoint::new(DisplayRow(5), 5),
                DisplayPoint::new(DisplayRow(6), 5)..DisplayPoint::new(DisplayRow(6), 5)
            ]
        );
    });
    EditorTestContext::for_editor(editor, cx)
        .await
        .assert_editor_state(
            "aaaaaˇ\nbbbbbˇ\ncccccˇ\ndddddˇ\neeeeeˇ\nfffffˇ\ngggggˇ\nhhhhh\niiiii",
        );
}

/// A different number of tabs can align the same column on each row, so a cursor has to
/// be placed by the column the tabs expand to. Counting a tab as a single column lands it
/// wherever that many characters happen to reach on the next row.
#[gpui::test]
async fn test_add_selection_below_with_tab_aligned_columns(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.hard_tabs = Some(true);
        settings.defaults.tab_size = Some(4.try_into().unwrap());
    });

    let mut cx = EditorTestContext::new(cx).await;

    // Both `=` render at column 48: `current.rgb.r` ends at 37 and is padded by three
    // tabs, `residuals[run].rgb.r` ends at 44 and is padded by one.
    cx.set_state(
        "\t\t\t\t\t\tcurrent.rgb.r\t\t\tˇ= p[0] >> 8;\n\t\t\t\t\t\tresiduals[run].rgb.r\t= p[0] & 0xff;\n",
    );

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(
            &AddSelectionBelow {
                skip_soft_wrap: true,
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(
        "\t\t\t\t\t\tcurrent.rgb.r\t\t\tˇ= p[0] >> 8;\n\t\t\t\t\t\tresiduals[run].rgb.r\tˇ= p[0] & 0xff;\n",
    );
}

#[gpui::test]
/// Regression test for a panic ("display point out of range"): with a multi-line fold,
/// buffer rows below the fold exceed the fold map's max row, so they must be converted
/// to tab map rows instead of being used directly.
async fn test_add_selection_above_below_with_fold(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc!(
        r#"fn foo() {
            aaaa
            bbbb
        }
        one
        twoˇ"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.fold_creases(
            vec![Crease::simple(
                Point::new(0, 10)..Point::new(3, 0),
                FoldPlaceholder::test(),
            )],
            true,
            window,
            cx,
        );
    });

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(
            &AddSelectionAbove {
                skip_soft_wrap: true,
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(indoc!(
        r#"fn foo() {
            aaaa
            bbbb
        }
        oneˇ
        twoˇ"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(
            &AddSelectionAbove {
                skip_soft_wrap: true,
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(indoc!(
        r#"fn ˇfoo() {
            aaaa
            bbbb
        }
        oneˇ
        twoˇ"#
    ));

    cx.set_state(indoc!(
        r#"fn fˇoo() {
            aaaa
            bbbb
        }
        one
        two"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.fold_creases(
            vec![Crease::simple(
                Point::new(0, 10)..Point::new(3, 0),
                FoldPlaceholder::test(),
            )],
            true,
            window,
            cx,
        );
    });

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(
            &AddSelectionBelow {
                skip_soft_wrap: true,
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(indoc!(
        r#"fn fˇoo() {
            aaaa
            bbbb
        }
        oneˇ
        two"#
    ));
}

#[gpui::test]
async fn test_add_selection_above_below(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc!(
        r#"abc
           defˇghi

           jk
           nlmo
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abcˇ
           defˇghi

           jk
           nlmo
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abcˇ
            defˇghi

            jk
            nlmo
            "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abc
           defˇghi

           jk
           nlmo
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.undo_selection(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abcˇ
           defˇghi

           jk
           nlmo
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.redo_selection(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abc
           defˇghi

           jk
           nlmo
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abc
           defˇghi
           ˇ
           jk
           nlmo
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abc
           defˇghi
           ˇ
           jkˇ
           nlmo
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abc
           defˇghi
           ˇ
           jkˇ
           nlmˇo
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abc
           defˇghi
           ˇ
           jkˇ
           nlmˇo
           ˇ"#
    ));

    // change selections
    cx.set_state(indoc!(
        r#"abc
           def«ˇg»hi

           jk
           nlmo
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abc
           def«ˇg»hi

           jk
           nlm«ˇo»
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abc
           def«ˇg»hi

           jk
           nlm«ˇo»
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abc
           def«ˇg»hi

           jk
           nlmo
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abc
           def«ˇg»hi

           jk
           nlmo
           "#
    ));

    // Change selections again
    cx.set_state(indoc!(
        r#"a«bc
           defgˇ»hi

           jk
           nlmo
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"a«bcˇ»
           d«efgˇ»hi

           j«kˇ»
           nlmo
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });
    cx.assert_editor_state(indoc!(
        r#"a«bcˇ»
           d«efgˇ»hi

           j«kˇ»
           n«lmoˇ»
           "#
    ));
    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"a«bcˇ»
           d«efgˇ»hi

           j«kˇ»
           nlmo
           "#
    ));

    // Change selections again
    cx.set_state(indoc!(
        r#"abc
           d«ˇefghi

           jk
           nlm»o
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"a«ˇbc»
           d«ˇef»ghi

           j«ˇk»
           n«ˇlm»o
           "#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"abc
           d«ˇef»ghi

           j«ˇk»
           n«ˇlm»o
           "#
    ));

    // Assert that the oldest selection's goal column is used when adding more
    // selections, not the most recently added selection's actual column.
    cx.set_state(indoc! {"
        foo bar bazˇ
        foo
        foo bar
    "});

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(
            &AddSelectionBelow {
                skip_soft_wrap: true,
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(indoc! {"
        foo bar bazˇ
        fooˇ
        foo bar
    "});

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(
            &AddSelectionBelow {
                skip_soft_wrap: true,
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(indoc! {"
        foo bar bazˇ
        fooˇ
        foo barˇ
    "});

    cx.set_state(indoc! {"
        foo bar baz
        foo
        foo barˇ
    "});

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(
            &AddSelectionAbove {
                skip_soft_wrap: true,
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(indoc! {"
        foo bar baz
        fooˇ
        foo barˇ
    "});

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(
            &AddSelectionAbove {
                skip_soft_wrap: true,
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(indoc! {"
        foo barˇ baz
        fooˇ
        foo barˇ
    "});
}

#[gpui::test]
async fn test_add_selection_above_below_multi_cursor(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc!(
        r#"line onˇe
           liˇne two
           line three
           line four"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    // test multiple cursors expand in the same direction
    cx.assert_editor_state(indoc!(
        r#"line onˇe
           liˇne twˇo
           liˇne three
           line four"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    // test multiple cursors expand below overflow
    cx.assert_editor_state(indoc!(
        r#"line onˇe
           liˇne twˇo
           liˇne thˇree
           liˇne foˇur"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
    });

    // test multiple cursors retrieves back correctly
    cx.assert_editor_state(indoc!(
        r#"line onˇe
           liˇne twˇo
           liˇne thˇree
           line four"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
    });

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
    });

    // test multiple cursor groups maintain independent direction - first expands up, second shrinks above
    cx.assert_editor_state(indoc!(
        r#"liˇne onˇe
           liˇne two
           line three
           line four"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.undo_selection(&Default::default(), window, cx);
    });

    // test undo
    cx.assert_editor_state(indoc!(
        r#"line onˇe
           liˇne twˇo
           line three
           line four"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.redo_selection(&Default::default(), window, cx);
    });

    // test redo
    cx.assert_editor_state(indoc!(
        r#"liˇne onˇe
           liˇne two
           line three
           line four"#
    ));

    cx.set_state(indoc!(
        r#"abcd
           ef«ghˇ»
           ijkl
           «mˇ»nop"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
    });

    // test multiple selections expand in the same direction
    cx.assert_editor_state(indoc!(
        r#"ab«cdˇ»
           ef«ghˇ»
           «iˇ»jkl
           «mˇ»nop"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
    });

    // test multiple selection upward overflow
    cx.assert_editor_state(indoc!(
        r#"ab«cdˇ»
           «eˇ»f«ghˇ»
           «iˇ»jkl
           «mˇ»nop"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    // test multiple selection retrieves back correctly
    cx.assert_editor_state(indoc!(
        r#"abcd
           ef«ghˇ»
           «iˇ»jkl
           «mˇ»nop"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    // test multiple cursor groups maintain independent direction - first shrinks down, second expands below
    cx.assert_editor_state(indoc!(
        r#"abcd
           ef«ghˇ»
           ij«klˇ»
           «mˇ»nop"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.undo_selection(&Default::default(), window, cx);
    });

    // test undo
    cx.assert_editor_state(indoc!(
        r#"abcd
           ef«ghˇ»
           «iˇ»jkl
           «mˇ»nop"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.redo_selection(&Default::default(), window, cx);
    });

    // test redo
    cx.assert_editor_state(indoc!(
        r#"abcd
           ef«ghˇ»
           ij«klˇ»
           «mˇ»nop"#
    ));
}

#[gpui::test]
async fn test_add_selection_above_below_multi_cursor_existing_state(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc!(
        r#"line onˇe
           liˇne two
           line three
           line four"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
        editor.add_selection_below(&Default::default(), window, cx);
        editor.add_selection_below(&Default::default(), window, cx);
    });

    // initial state with two multi cursor groups
    cx.assert_editor_state(indoc!(
        r#"line onˇe
           liˇne twˇo
           liˇne thˇree
           liˇne foˇur"#
    ));

    // add single cursor in middle - simulate opt click
    cx.update_editor(|editor, window, cx| {
        let new_cursor_point = DisplayPoint::new(DisplayRow(2), 4);
        editor.begin_selection(new_cursor_point, true, 1, window, cx);
        editor.end_selection(window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"line onˇe
           liˇne twˇo
           liˇneˇ thˇree
           liˇne foˇur"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
    });

    // test new added selection expands above and existing selection shrinks
    cx.assert_editor_state(indoc!(
        r#"line onˇe
           liˇneˇ twˇo
           liˇneˇ thˇree
           line four"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
    });

    // test new added selection expands above and existing selection shrinks
    cx.assert_editor_state(indoc!(
        r#"lineˇ onˇe
           liˇneˇ twˇo
           lineˇ three
           line four"#
    ));

    // intial state with two selection groups
    cx.set_state(indoc!(
        r#"abcd
           ef«ghˇ»
           ijkl
           «mˇ»nop"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_above(&Default::default(), window, cx);
        editor.add_selection_above(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"ab«cdˇ»
           «eˇ»f«ghˇ»
           «iˇ»jkl
           «mˇ»nop"#
    ));

    // add single selection in middle - simulate opt drag
    cx.update_editor(|editor, window, cx| {
        let new_cursor_point = DisplayPoint::new(DisplayRow(2), 3);
        editor.begin_selection(new_cursor_point, true, 1, window, cx);
        editor.update_selection(
            DisplayPoint::new(DisplayRow(2), 4),
            0,
            gpui::Point::<f32>::default(),
            window,
            cx,
        );
        editor.end_selection(window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"ab«cdˇ»
           «eˇ»f«ghˇ»
           «iˇ»jk«lˇ»
           «mˇ»nop"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    // test new added selection expands below, others shrinks from above
    cx.assert_editor_state(indoc!(
        r#"abcd
           ef«ghˇ»
           «iˇ»jk«lˇ»
           «mˇ»no«pˇ»"#
    ));
}

#[gpui::test]
async fn test_add_selection_above_below_multibyte(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    // Cursor after "Häl" (byte column 4, char column 3) should align to
    // char column 3 on the ASCII line below, not byte column 4.
    cx.set_state(indoc!(
        r#"Hälˇlö
           Hallo"#
    ));

    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        r#"Hälˇlö
           Halˇlo"#
    ));
}

#[gpui::test]
async fn test_select_next(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    // Enable case sensitive search.
    update_test_editor_settings(&mut cx, &|settings| {
        let mut search_settings = SearchSettingsContent::default();
        search_settings.case_sensitive = Some(true);
        settings.search = Some(search_settings);
    });

    cx.set_state("abc\nˇabc abc\ndefabc\nabc");

    cx.update_editor(|e, window, cx| e.select_next(&SelectNext::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("abc\n«abcˇ» abc\ndefabc\nabc");

    cx.update_editor(|e, window, cx| e.select_next(&SelectNext::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("abc\n«abcˇ» «abcˇ»\ndefabc\nabc");

    cx.update_editor(|editor, window, cx| editor.undo_selection(&UndoSelection, window, cx));
    cx.assert_editor_state("abc\n«abcˇ» abc\ndefabc\nabc");

    cx.update_editor(|editor, window, cx| editor.redo_selection(&RedoSelection, window, cx));
    cx.assert_editor_state("abc\n«abcˇ» «abcˇ»\ndefabc\nabc");

    cx.update_editor(|e, window, cx| e.select_next(&SelectNext::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("abc\n«abcˇ» «abcˇ»\ndefabc\n«abcˇ»");

    cx.update_editor(|e, window, cx| e.select_next(&SelectNext::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("«abcˇ»\n«abcˇ» «abcˇ»\ndefabc\n«abcˇ»");

    // Test selection direction should be preserved
    cx.set_state("abc\n«ˇabc» abc\ndefabc\nabc");

    cx.update_editor(|e, window, cx| e.select_next(&SelectNext::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("abc\n«ˇabc» «ˇabc»\ndefabc\nabc");

    // Test case sensitivity
    cx.set_state("«ˇfoo»\nFOO\nFoo\nfoo");
    cx.update_editor(|e, window, cx| {
        e.select_next(&SelectNext::default(), window, cx).unwrap();
    });
    cx.assert_editor_state("«ˇfoo»\nFOO\nFoo\n«ˇfoo»");

    // Disable case sensitive search.
    update_test_editor_settings(&mut cx, &|settings| {
        let mut search_settings = SearchSettingsContent::default();
        search_settings.case_sensitive = Some(false);
        settings.search = Some(search_settings);
    });

    cx.set_state("«ˇfoo»\nFOO\nFoo");
    cx.update_editor(|e, window, cx| {
        e.select_next(&SelectNext::default(), window, cx).unwrap();
        e.select_next(&SelectNext::default(), window, cx).unwrap();
    });
    cx.assert_editor_state("«ˇfoo»\n«ˇFOO»\n«ˇFoo»");
}

#[gpui::test]
async fn test_select_all_matches(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    // Enable case sensitive search.
    update_test_editor_settings(&mut cx, &|settings| {
        let mut search_settings = SearchSettingsContent::default();
        search_settings.case_sensitive = Some(true);
        settings.search = Some(search_settings);
    });

    // Test caret-only selections
    cx.set_state("abc\nˇabc abc\ndefabc\nabc");
    cx.update_editor(|e, window, cx| e.select_all_matches(&SelectAllMatches, window, cx))
        .unwrap();
    cx.assert_editor_state("«abcˇ»\n«abcˇ» «abcˇ»\ndefabc\n«abcˇ»");

    // Test left-to-right selections
    cx.set_state("abc\n«abcˇ»\nabc");
    cx.update_editor(|e, window, cx| e.select_all_matches(&SelectAllMatches, window, cx))
        .unwrap();
    cx.assert_editor_state("«abcˇ»\n«abcˇ»\n«abcˇ»");

    // Test right-to-left selections
    cx.set_state("abc\n«ˇabc»\nabc");
    cx.update_editor(|e, window, cx| e.select_all_matches(&SelectAllMatches, window, cx))
        .unwrap();
    cx.assert_editor_state("«ˇabc»\n«ˇabc»\n«ˇabc»");

    // Test selecting whitespace with caret selection
    cx.set_state("abc\nˇ   abc\nabc");
    cx.update_editor(|e, window, cx| e.select_all_matches(&SelectAllMatches, window, cx))
        .unwrap();
    cx.assert_editor_state("abc\n«   ˇ»abc\nabc");

    // Test selecting whitespace with left-to-right selection
    cx.set_state("abc\n«ˇ  »abc\nabc");
    cx.update_editor(|e, window, cx| e.select_all_matches(&SelectAllMatches, window, cx))
        .unwrap();
    cx.assert_editor_state("abc\n«ˇ  »abc\nabc");

    // Test no matches with right-to-left selection
    cx.set_state("abc\n«  ˇ»abc\nabc");
    cx.update_editor(|e, window, cx| e.select_all_matches(&SelectAllMatches, window, cx))
        .unwrap();
    cx.assert_editor_state("abc\n«  ˇ»abc\nabc");

    // Test with a single word and clip_at_line_ends=true (#29823)
    cx.set_state("aˇbc");
    cx.update_editor(|e, window, cx| {
        e.set_clip_at_line_ends(true, cx);
        e.select_all_matches(&SelectAllMatches, window, cx).unwrap();
        e.set_clip_at_line_ends(false, cx);
    });
    cx.assert_editor_state("«abcˇ»");

    // Test case sensitivity
    cx.set_state("fˇoo\nFOO\nFoo");
    cx.update_editor(|e, window, cx| {
        e.select_all_matches(&SelectAllMatches, window, cx).unwrap();
    });
    cx.assert_editor_state("«fooˇ»\nFOO\nFoo");

    // Disable case sensitive search.
    update_test_editor_settings(&mut cx, &|settings| {
        let mut search_settings = SearchSettingsContent::default();
        search_settings.case_sensitive = Some(false);
        settings.search = Some(search_settings);
    });

    cx.set_state("fˇoo\nFOO\nFoo");
    cx.update_editor(|e, window, cx| {
        e.select_all_matches(&SelectAllMatches, window, cx).unwrap();
    });
    cx.assert_editor_state("«fooˇ»\n«FOOˇ»\n«Fooˇ»");
}

#[gpui::test]
async fn test_select_all_matches_does_not_scroll(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    let large_body_1 = "\nd".repeat(200);
    let large_body_2 = "\ne".repeat(200);

    cx.set_state(&format!(
        "abc\nabc{large_body_1} «ˇa»bc{large_body_2}\nefabc\nabc"
    ));
    let initial_scroll_position = cx.update_editor(|editor, _, cx| {
        let scroll_position = editor.scroll_position(cx);
        assert!(scroll_position.y > 0.0, "Initial selection is between two large bodies and should have the editor scrolled to it");
        scroll_position
    });

    cx.update_editor(|editor, window, cx| editor.select_all_matches(&SelectAllMatches, window, cx))
        .unwrap();
    cx.assert_editor_state(&format!(
        "«ˇa»bc\n«ˇa»bc{large_body_1} «ˇa»bc{large_body_2}\nef«ˇa»bc\n«ˇa»bc"
    ));
    cx.update_editor(|editor, _, cx| {
        assert_eq!(
            editor.scroll_position(cx),
            initial_scroll_position,
            "Scroll position should not change after selecting all matches"
        )
    });

    // Simulate typing while the selections are active, as that is where the
    // editor would attempt to actually scroll to the newest selection, which
    // should have been set as the original selection to avoid scrolling to the
    // last match.
    cx.simulate_keystroke("x");
    cx.update_editor(|editor, _, cx| {
        assert_eq!(
            editor.scroll_position(cx),
            initial_scroll_position,
            "Scroll position should not change after editing all matches"
        )
    });

    cx.set_state(&format!(
        "abc\nabc{large_body_1} «aˇ»bc{large_body_2}\nefabc\nabc"
    ));
    let initial_scroll_position = cx.update_editor(|editor, _, cx| {
        let scroll_position = editor.scroll_position(cx);
        assert!(scroll_position.y > 0.0, "Initial selection is between two large bodies and should have the editor scrolled to it");
        scroll_position
    });

    cx.update_editor(|editor, window, cx| editor.select_all_matches(&SelectAllMatches, window, cx))
        .unwrap();
    cx.assert_editor_state(&format!(
        "«aˇ»bc\n«aˇ»bc{large_body_1} «aˇ»bc{large_body_2}\nef«aˇ»bc\n«aˇ»bc"
    ));
    cx.update_editor(|editor, _, cx| {
        assert_eq!(
            editor.scroll_position(cx),
            initial_scroll_position,
            "Scroll position should not change after selecting all matches"
        )
    });

    cx.simulate_keystroke("x");
    cx.update_editor(|editor, _, cx| {
        assert_eq!(
            editor.scroll_position(cx),
            initial_scroll_position,
            "Scroll position should not change after editing all matches"
        )
    });
}

#[gpui::test]
async fn test_undo_edit_prediction_scrolls_to_edit_pos(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let provider = cx.new(|_| FakeEditPredictionDelegate::default());
    cx.update_editor(|editor, window, cx| {
        editor.set_edit_prediction_provider(
            Some(provider.clone()),
            EditPredictionRequestTrigger::EditorCreated,
            window,
            cx,
        );
    });

    cx.set_state(indoc! {"
        line 1
        line 2
        linˇe 3
        line 4
        line 5
        line 6
        line 7
        line 8
        line 9
        line 10
    "});

    let snapshot = cx.buffer_snapshot();
    let edit_position = snapshot.anchor_after(Point::new(2, 4));

    cx.update(|_, cx| {
        provider.update(cx, |provider, _| {
            provider.set_edit_prediction(Some(edit_prediction_types::EditPrediction::Local {
                id: None,
                edits: vec![(edit_position..edit_position, "X".into())],
                cursor_position: None,
                edit_preview: None,
            }))
        })
    });

    cx.update_editor(|editor, window, cx| editor.update_visible_edit_prediction(window, cx));
    cx.update_editor(|editor, window, cx| {
        editor.accept_edit_prediction(&crate::AcceptEditPrediction, window, cx)
    });

    cx.assert_editor_state(indoc! {"
        line 1
        line 2
        lineXˇ 3
        line 4
        line 5
        line 6
        line 7
        line 8
        line 9
        line 10
    "});

    cx.update_editor(|editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(9, 2)..Point::new(9, 2)]);
        });
    });

    cx.assert_editor_state(indoc! {"
        line 1
        line 2
        lineX 3
        line 4
        line 5
        line 6
        line 7
        line 8
        line 9
        liˇne 10
    "});

    cx.update_editor(|editor, window, cx| {
        editor.undo(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc! {"
        line 1
        line 2
        lineˇ 3
        line 4
        line 5
        line 6
        line 7
        line 8
        line 9
        line 10
    "});
}

#[gpui::test]
async fn test_select_next_with_multiple_carets(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(
        r#"let foo = 2;
lˇet foo = 2;
let fooˇ = 2;
let foo = 2;
let foo = ˇ2;"#,
    );

    cx.update_editor(|e, window, cx| e.select_next(&SelectNext::default(), window, cx))
        .unwrap();
    cx.assert_editor_state(
        r#"let foo = 2;
«letˇ» foo = 2;
let «fooˇ» = 2;
let foo = 2;
let foo = «2ˇ»;"#,
    );

    // noop for multiple selections with different contents
    cx.update_editor(|e, window, cx| e.select_next(&SelectNext::default(), window, cx))
        .unwrap();
    cx.assert_editor_state(
        r#"let foo = 2;
«letˇ» foo = 2;
let «fooˇ» = 2;
let foo = 2;
let foo = «2ˇ»;"#,
    );

    // Test last selection direction should be preserved
    cx.set_state(
        r#"let foo = 2;
let foo = 2;
let «fooˇ» = 2;
let «ˇfoo» = 2;
let foo = 2;"#,
    );

    cx.update_editor(|e, window, cx| e.select_next(&SelectNext::default(), window, cx))
        .unwrap();
    cx.assert_editor_state(
        r#"let foo = 2;
let foo = 2;
let «fooˇ» = 2;
let «ˇfoo» = 2;
let «ˇfoo» = 2;"#,
    );
}

#[gpui::test]
async fn test_select_previous_multibuffer(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx =
        EditorTestContext::new_multibuffer(cx, ["aaa\n«bbb\nccc»\nddd", "aaa\n«bbb\nccc»\nddd"]);

    cx.assert_editor_state(indoc! {"
        ˇbbb
        ccc
        bbb
        ccc"});
    cx.dispatch_action(SelectPrevious::default());
    cx.assert_editor_state(indoc! {"
                «bbbˇ»
                ccc
                bbb
                ccc"});
    cx.dispatch_action(SelectPrevious::default());
    cx.assert_editor_state(indoc! {"
                «bbbˇ»
                ccc
                «bbbˇ»
                ccc"});
}

#[gpui::test]
async fn test_select_previous_with_single_caret(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state("abc\nˇabc abc\ndefabc\nabc");

    cx.update_editor(|e, window, cx| e.select_previous(&SelectPrevious::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("abc\n«abcˇ» abc\ndefabc\nabc");

    cx.update_editor(|e, window, cx| e.select_previous(&SelectPrevious::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("«abcˇ»\n«abcˇ» abc\ndefabc\nabc");

    cx.update_editor(|editor, window, cx| editor.undo_selection(&UndoSelection, window, cx));
    cx.assert_editor_state("abc\n«abcˇ» abc\ndefabc\nabc");

    cx.update_editor(|editor, window, cx| editor.redo_selection(&RedoSelection, window, cx));
    cx.assert_editor_state("«abcˇ»\n«abcˇ» abc\ndefabc\nabc");

    cx.update_editor(|e, window, cx| e.select_previous(&SelectPrevious::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("«abcˇ»\n«abcˇ» abc\ndefabc\n«abcˇ»");

    cx.update_editor(|e, window, cx| e.select_previous(&SelectPrevious::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("«abcˇ»\n«abcˇ» «abcˇ»\ndefabc\n«abcˇ»");
}

#[gpui::test]
async fn test_select_previous_empty_buffer(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state("aˇ");

    cx.update_editor(|e, window, cx| e.select_previous(&SelectPrevious::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("«aˇ»");
    cx.update_editor(|e, window, cx| e.select_previous(&SelectPrevious::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("«aˇ»");
}

#[gpui::test]
async fn test_select_previous_with_multiple_carets(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(
        r#"let foo = 2;
lˇet foo = 2;
let fooˇ = 2;
let foo = 2;
let foo = ˇ2;"#,
    );

    cx.update_editor(|e, window, cx| e.select_previous(&SelectPrevious::default(), window, cx))
        .unwrap();
    cx.assert_editor_state(
        r#"let foo = 2;
«letˇ» foo = 2;
let «fooˇ» = 2;
let foo = 2;
let foo = «2ˇ»;"#,
    );

    // noop for multiple selections with different contents
    cx.update_editor(|e, window, cx| e.select_previous(&SelectPrevious::default(), window, cx))
        .unwrap();
    cx.assert_editor_state(
        r#"let foo = 2;
«letˇ» foo = 2;
let «fooˇ» = 2;
let foo = 2;
let foo = «2ˇ»;"#,
    );
}

#[gpui::test]
async fn test_select_previous_with_single_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    // Enable case sensitive search.
    update_test_editor_settings(&mut cx, &|settings| {
        let mut search_settings = SearchSettingsContent::default();
        search_settings.case_sensitive = Some(true);
        settings.search = Some(search_settings);
    });

    cx.set_state("abc\n«ˇabc» abc\ndefabc\nabc");

    cx.update_editor(|e, window, cx| e.select_previous(&SelectPrevious::default(), window, cx))
        .unwrap();
    // selection direction is preserved
    cx.assert_editor_state("«ˇabc»\n«ˇabc» abc\ndefabc\nabc");

    cx.update_editor(|e, window, cx| e.select_previous(&SelectPrevious::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("«ˇabc»\n«ˇabc» abc\ndefabc\n«ˇabc»");

    cx.update_editor(|editor, window, cx| editor.undo_selection(&UndoSelection, window, cx));
    cx.assert_editor_state("«ˇabc»\n«ˇabc» abc\ndefabc\nabc");

    cx.update_editor(|editor, window, cx| editor.redo_selection(&RedoSelection, window, cx));
    cx.assert_editor_state("«ˇabc»\n«ˇabc» abc\ndefabc\n«ˇabc»");

    cx.update_editor(|e, window, cx| e.select_previous(&SelectPrevious::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("«ˇabc»\n«ˇabc» abc\ndef«ˇabc»\n«ˇabc»");

    cx.update_editor(|e, window, cx| e.select_previous(&SelectPrevious::default(), window, cx))
        .unwrap();
    cx.assert_editor_state("«ˇabc»\n«ˇabc» «ˇabc»\ndef«ˇabc»\n«ˇabc»");

    // Test case sensitivity
    cx.set_state("foo\nFOO\nFoo\n«ˇfoo»");
    cx.update_editor(|e, window, cx| {
        e.select_previous(&SelectPrevious::default(), window, cx)
            .unwrap();
        e.select_previous(&SelectPrevious::default(), window, cx)
            .unwrap();
    });
    cx.assert_editor_state("«ˇfoo»\nFOO\nFoo\n«ˇfoo»");

    // Disable case sensitive search.
    update_test_editor_settings(&mut cx, &|settings| {
        let mut search_settings = SearchSettingsContent::default();
        search_settings.case_sensitive = Some(false);
        settings.search = Some(search_settings);
    });

    cx.set_state("foo\nFOO\n«ˇFoo»");
    cx.update_editor(|e, window, cx| {
        e.select_previous(&SelectPrevious::default(), window, cx)
            .unwrap();
        e.select_previous(&SelectPrevious::default(), window, cx)
            .unwrap();
    });
    cx.assert_editor_state("«ˇfoo»\n«ˇFOO»\n«ˇFoo»");
}

#[gpui::test]
async fn test_select_larger_smaller_syntax_node(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(Language::new(
        LanguageConfig::default(),
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    let text = r#"
        use mod1::mod2::{mod3, mod4};

        fn fn_1(param1: bool, param2: &str) {
            let var1 = "text";
        }
    "#
    .unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 25)..DisplayPoint::new(DisplayRow(0), 25),
                DisplayPoint::new(DisplayRow(2), 24)..DisplayPoint::new(DisplayRow(2), 12),
                DisplayPoint::new(DisplayRow(3), 18)..DisplayPoint::new(DisplayRow(3), 18),
            ]);
        });
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, «mod4ˇ»};

                fn fn_1«ˇ(param1: bool, param2: &str)» {
                    let var1 = "«textˇ»";
                }
            "#},
            cx,
        );
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::«{mod3, mod4}ˇ»;

                «ˇfn fn_1(param1: bool, param2: &str) {
                    let var1 = "text";
                }»
            "#},
            cx,
        );
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    assert_eq!(
        editor.update(cx, |editor, cx| editor
            .selections
            .display_ranges(&editor.display_snapshot(cx))),
        &[DisplayPoint::new(DisplayRow(5), 0)..DisplayPoint::new(DisplayRow(0), 0)]
    );

    // Trying to expand the selected syntax node one more time has no effect.
    editor.update_in(cx, |editor, window, cx| {
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    assert_eq!(
        editor.update(cx, |editor, cx| editor
            .selections
            .display_ranges(&editor.display_snapshot(cx))),
        &[DisplayPoint::new(DisplayRow(5), 0)..DisplayPoint::new(DisplayRow(0), 0)]
    );

    editor.update_in(cx, |editor, window, cx| {
        editor.select_smaller_syntax_node(&SelectSmallerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::«{mod3, mod4}ˇ»;

                «ˇfn fn_1(param1: bool, param2: &str) {
                    let var1 = "text";
                }»
            "#},
            cx,
        );
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.select_smaller_syntax_node(&SelectSmallerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, «mod4ˇ»};

                fn fn_1«ˇ(param1: bool, param2: &str)» {
                    let var1 = "«textˇ»";
                }
            "#},
            cx,
        );
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.select_smaller_syntax_node(&SelectSmallerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, moˇd4};

                fn fn_1(para«ˇm1: bool, pa»ram2: &str) {
                    let var1 = "teˇxt";
                }
            "#},
            cx,
        );
    });

    // Trying to shrink the selected syntax node one more time has no effect.
    editor.update_in(cx, |editor, window, cx| {
        editor.select_smaller_syntax_node(&SelectSmallerSyntaxNode, window, cx);
    });
    editor.update_in(cx, |editor, _, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, moˇd4};

                fn fn_1(para«ˇm1: bool, pa»ram2: &str) {
                    let var1 = "teˇxt";
                }
            "#},
            cx,
        );
    });

    // Ensure that we keep expanding the selection if the larger selection starts or ends within
    // a fold.
    editor.update_in(cx, |editor, window, cx| {
        editor.fold_creases(
            vec![
                Crease::simple(
                    Point::new(0, 21)..Point::new(0, 24),
                    FoldPlaceholder::test(),
                ),
                Crease::simple(
                    Point::new(3, 20)..Point::new(3, 22),
                    FoldPlaceholder::test(),
                ),
            ],
            true,
            window,
            cx,
        );
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::«{mod3, mod4}ˇ»;

                fn fn_1«ˇ(param1: bool, param2: &str)» {
                    let var1 = "«textˇ»";
                }
            "#},
            cx,
        );
    });

    // Ensure multiple cursors have consistent direction after expanding
    editor.update_in(cx, |editor, window, cx| {
        editor.unfold_all(&UnfoldAll, window, cx);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 25)..DisplayPoint::new(DisplayRow(0), 25),
                DisplayPoint::new(DisplayRow(3), 18)..DisplayPoint::new(DisplayRow(3), 18),
            ]);
        });
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, «mod4ˇ»};

                fn fn_1(param1: bool, param2: &str) {
                    let var1 = "«textˇ»";
                }
            "#},
            cx,
        );
    });
}

#[gpui::test]
async fn test_select_larger_syntax_node_for_cursor_at_end(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(Language::new(
        LanguageConfig::default(),
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    let text = "let a = 2;";

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    // Test case 1: Cursor at end of word
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 5)..DisplayPoint::new(DisplayRow(0), 5)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, "let aˇ = 2;", cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, "let «ˇa» = 2;", cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, "«ˇlet a = 2;»", cx);
    });

    // Test case 2: Cursor at end of statement
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 11)..DisplayPoint::new(DisplayRow(0), 11)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, "let a = 2;ˇ", cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, "«ˇlet a = 2;»", cx);
    });
}

#[gpui::test]
async fn test_select_larger_syntax_node_for_cursor_at_symbol(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(Language::new(
        LanguageConfig {
            name: "JavaScript".into(),
            ..Default::default()
        },
        Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
    ));

    let text = r#"
        let a = {
            key: "value",
        };
    "#
    .unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    // Test case 1: Cursor after '{'
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 9)..DisplayPoint::new(DisplayRow(0), 9)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                let a = {ˇ
                    key: "value",
                };
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                let a = «ˇ{
                    key: "value",
                }»;
            "#},
            cx,
        );
    });

    // Test case 2: Cursor after ':'
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(1), 8)..DisplayPoint::new(DisplayRow(1), 8)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                let a = {
                    key:ˇ "value",
                };
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                let a = {
                    «ˇkey: "value"»,
                };
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                let a = «ˇ{
                    key: "value",
                }»;
            "#},
            cx,
        );
    });

    // Test case 3: Cursor after ','
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(1), 17)..DisplayPoint::new(DisplayRow(1), 17)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                let a = {
                    key: "value",ˇ
                };
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                let a = «ˇ{
                    key: "value",
                }»;
            "#},
            cx,
        );
    });

    // Test case 4: Cursor after ';'
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(2), 2)..DisplayPoint::new(DisplayRow(2), 2)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                let a = {
                    key: "value",
                };ˇ
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                «ˇlet a = {
                    key: "value",
                };
                »"#},
            cx,
        );
    });
}

#[gpui::test]
async fn test_select_larger_smaller_syntax_node_for_string(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(Language::new(
        LanguageConfig::default(),
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    let text = r#"
        use mod1::mod2::{mod3, mod4};

        fn fn_1(param1: bool, param2: &str) {
            let var1 = "hello world";
        }
    "#
    .unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    // Test 1: Cursor on a letter of a string word
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(3), 17)..DisplayPoint::new(DisplayRow(3), 17)
            ]);
        });
    });
    editor.update_in(cx, |editor, window, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, mod4};

                fn fn_1(param1: bool, param2: &str) {
                    let var1 = "hˇello world";
                }
            "#},
            cx,
        );
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, mod4};

                fn fn_1(param1: bool, param2: &str) {
                    let var1 = "«ˇhello» world";
                }
            "#},
            cx,
        );
    });

    // Test 2: Partial selection within a word
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(3), 17)..DisplayPoint::new(DisplayRow(3), 19)
            ]);
        });
    });
    editor.update_in(cx, |editor, window, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, mod4};

                fn fn_1(param1: bool, param2: &str) {
                    let var1 = "h«elˇ»lo world";
                }
            "#},
            cx,
        );
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, mod4};

                fn fn_1(param1: bool, param2: &str) {
                    let var1 = "«ˇhello» world";
                }
            "#},
            cx,
        );
    });

    // Test 3: Complete word already selected
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(3), 16)..DisplayPoint::new(DisplayRow(3), 21)
            ]);
        });
    });
    editor.update_in(cx, |editor, window, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, mod4};

                fn fn_1(param1: bool, param2: &str) {
                    let var1 = "«helloˇ» world";
                }
            "#},
            cx,
        );
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, mod4};

                fn fn_1(param1: bool, param2: &str) {
                    let var1 = "«hello worldˇ»";
                }
            "#},
            cx,
        );
    });

    // Test 4: Selection spanning across words
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(3), 19)..DisplayPoint::new(DisplayRow(3), 24)
            ]);
        });
    });
    editor.update_in(cx, |editor, window, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, mod4};

                fn fn_1(param1: bool, param2: &str) {
                    let var1 = "hel«lo woˇ»rld";
                }
            "#},
            cx,
        );
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, mod4};

                fn fn_1(param1: bool, param2: &str) {
                    let var1 = "«ˇhello world»";
                }
            "#},
            cx,
        );
    });

    // Test 5: Expansion beyond string
    editor.update_in(cx, |editor, window, cx| {
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
        assert_text_with_selections(
            editor,
            indoc! {r#"
                use mod1::mod2::{mod3, mod4};

                fn fn_1(param1: bool, param2: &str) {
                    «ˇlet var1 = "hello world";»
                }
            "#},
            cx,
        );
    });
}

#[gpui::test]
async fn test_unwrap_syntax_nodes(cx: &mut gpui::TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let language = Arc::new(Language::new(
        LanguageConfig::default(),
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    cx.update_buffer(|buffer, cx| {
        buffer.set_language(Some(language), cx);
    });

    cx.set_state(indoc! { r#"use mod1::{mod2::{«mod3ˇ», mod4}, mod5::{mod6, «mod7ˇ»}};"# });
    cx.update_editor(|editor, window, cx| {
        editor.unwrap_syntax_node(&UnwrapSyntaxNode, window, cx);
    });

    cx.assert_editor_state(indoc! { r#"use mod1::{mod2::«mod3ˇ», mod5::«mod7ˇ»};"# });

    cx.set_state(indoc! { r#"fn a() {
          // what
          // a
          // ˇlong
          // method
          // I
          // sure
          // hope
          // it
          // works
    }"# });

    let buffer = cx.update_multibuffer(|multibuffer, _| multibuffer.as_singleton().unwrap());
    let multi_buffer = cx.new(|_| MultiBuffer::new(Capability::ReadWrite));
    cx.update(|_, cx| {
        multi_buffer.update(cx, |multi_buffer, cx| {
            multi_buffer.set_excerpts_for_path(
                PathKey::for_buffer(&buffer, cx),
                buffer,
                [Point::new(1, 0)..Point::new(1, 0)],
                3,
                cx,
            );
        });
    });

    let editor2 = cx.new_window_entity(|window, cx| {
        Editor::new(EditorMode::full(), multi_buffer, None, window, cx)
    });

    let mut cx = EditorTestContext::for_editor_in(editor2, &mut cx).await;
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(SelectionEffects::default(), window, cx, |s| {
            s.select_ranges([Point::new(3, 0)..Point::new(3, 0)]);
        })
    });

    cx.assert_editor_state(indoc! { "
        fn a() {
              // what
              // a
        ˇ      // long
              // method"});

    cx.update_editor(|editor, window, cx| {
        editor.unwrap_syntax_node(&UnwrapSyntaxNode, window, cx);
    });

    // Although we could potentially make the action work when the syntax node
    // is half-hidden, it seems a bit dangerous as you can't easily tell what it
    // did. Maybe we could also expand the excerpt to contain the range?
    cx.assert_editor_state(indoc! { "
        fn a() {
              // what
              // a
        ˇ      // long
              // method"});
}

#[gpui::test]
async fn test_fold_function_bodies(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let base_text = r#"
        impl A {
            // this is an uncommitted comment

            fn b() {
                c();
            }

            // this is another uncommitted comment

            fn d() {
                // e
                // f
            }
        }

        fn g() {
            // h
        }
    "#
    .unindent();

    let text = r#"
        ˇimpl A {

            fn b() {
                c();
            }

            fn d() {
                // e
                // f
            }
        }

        fn g() {
            // h
        }
    "#
    .unindent();

    let mut cx = EditorLanguageTestContext::new_rust(cx).await;
    cx.set_state(&text);
    cx.set_head_text(&base_text);
    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&Default::default(), window, cx);
    });

    cx.assert_state_with_diff(
        "
        ˇimpl A {
      -     // this is an uncommitted comment

            fn b() {
                c();
            }

      -     // this is another uncommitted comment
      -
            fn d() {
                // e
                // f
            }
        }

        fn g() {
            // h
        }
    "
        .unindent(),
    );

    let expected_display_text = "
        impl A {
            // this is an uncommitted comment

            fn b() {
                ⋯
            }

            // this is another uncommitted comment

            fn d() {
                ⋯
            }
        }

        fn g() {
            ⋯
        }
        "
    .unindent();

    cx.update_editor(|editor, window, cx| {
        editor.fold_function_bodies(&FoldFunctionBodies, window, cx);
        assert_eq!(editor.display_text(cx), expected_display_text);
    });
}

#[gpui::test]
async fn test_autoindent(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(
        Language::new(
            LanguageConfig {
                brackets: BracketPairConfig {
                    pairs: vec![
                        BracketPair {
                            start: "{".to_string(),
                            end: "}".to_string(),
                            close: false,
                            surround: false,
                            newline: true,
                        },
                        BracketPair {
                            start: "(".to_string(),
                            end: ")".to_string(),
                            close: false,
                            surround: false,
                            newline: true,
                        },
                    ],
                    ..Default::default()
                },
                ..Default::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_indents_query(
            r#"
                (_ "(" ")" @end) @indent
                (_ "{" "}" @end) @indent
            "#,
        )
        .unwrap(),
    );

    let text = "fn a() {}";

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));
    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([
                MultiBufferOffset(5)..MultiBufferOffset(5),
                MultiBufferOffset(8)..MultiBufferOffset(8),
                MultiBufferOffset(9)..MultiBufferOffset(9),
            ])
        });
        editor.newline(&Newline, window, cx);
        assert_eq!(editor.text(cx), "fn a(\n    \n) {\n    \n}\n");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            &[
                Point::new(1, 4)..Point::new(1, 4),
                Point::new(3, 4)..Point::new(3, 4),
                Point::new(5, 0)..Point::new(5, 0)
            ]
        );
    });
}

#[gpui::test]
async fn test_autoindent_disabled(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.auto_indent = Some(settings::AutoIndentMode::None)
    });

    let language = Arc::new(
        Language::new(
            LanguageConfig {
                brackets: BracketPairConfig {
                    pairs: vec![
                        BracketPair {
                            start: "{".to_string(),
                            end: "}".to_string(),
                            close: false,
                            surround: false,
                            newline: true,
                        },
                        BracketPair {
                            start: "(".to_string(),
                            end: ")".to_string(),
                            close: false,
                            surround: false,
                            newline: true,
                        },
                    ],
                    ..Default::default()
                },
                ..Default::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_indents_query(
            r#"
                (_ "(" ")" @end) @indent
                (_ "{" "}" @end) @indent
            "#,
        )
        .unwrap(),
    );

    let text = "fn a() {}";

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));
    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([
                MultiBufferOffset(5)..MultiBufferOffset(5),
                MultiBufferOffset(8)..MultiBufferOffset(8),
                MultiBufferOffset(9)..MultiBufferOffset(9),
            ])
        });
        editor.newline(&Newline, window, cx);
        assert_eq!(
            editor.text(cx),
            indoc!(
                "
                fn a(

                ) {

                }
                "
            )
        );
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            &[
                Point::new(1, 0)..Point::new(1, 0),
                Point::new(3, 0)..Point::new(3, 0),
                Point::new(5, 0)..Point::new(5, 0)
            ]
        );
    });
}

#[gpui::test]
async fn test_autoindent_none_does_not_preserve_indentation_on_newline(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.auto_indent = Some(settings::AutoIndentMode::None)
    });

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"
        hello
            indented lineˇ
        world
    "});

    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });

    cx.assert_editor_state(indoc! {"
        hello
            indented line
        ˇ
        world
    "});
}

#[gpui::test]
async fn test_autoindent_preserve_indent_maintains_indentation_on_newline(cx: &mut TestAppContext) {
    // When auto_indent is "preserve_indent", pressing Enter on an indented line
    // should preserve the indentation but not adjust based on syntax.
    init_test(cx, |settings| {
        settings.defaults.auto_indent = Some(settings::AutoIndentMode::PreserveIndent)
    });

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"
        hello
            indented lineˇ
        world
    "});

    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });

    // The new line SHOULD have the same indentation as the previous line
    cx.assert_editor_state(indoc! {"
        hello
            indented line
            ˇ
        world
    "});
}

#[gpui::test]
async fn test_autoindent_preserve_indent_does_not_apply_syntax_indent(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.auto_indent = Some(settings::AutoIndentMode::PreserveIndent)
    });

    let language = Arc::new(
        Language::new(
            LanguageConfig {
                brackets: BracketPairConfig {
                    pairs: vec![BracketPair {
                        start: "{".to_string(),
                        end: "}".to_string(),
                        close: false,
                        surround: false,
                        newline: false, // Disable extra newline behavior to isolate syntax indent test
                    }],
                    ..Default::default()
                },
                ..Default::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_indents_query(r#"(_ "{" "}" @end) @indent"#)
        .unwrap(),
    );

    let buffer =
        cx.new(|cx| Buffer::local("fn foo() {\n}", cx).with_language(language.clone(), cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));
    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    // Position cursor at end of line containing `{`
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(10)..MultiBufferOffset(10)]) // After "fn foo() {"
        });
        editor.newline(&Newline, window, cx);

        // With PreserveIndent, the new line should have 0 indentation (same as the fn line)
        // NOT 4 spaces (which tree-sitter would add for being inside `{}`)
        assert_eq!(editor.text(cx), "fn foo() {\n\n}");
    });
}

#[gpui::test]
async fn test_autoindent_syntax_aware_applies_syntax_indent(cx: &mut TestAppContext) {
    // Companion test to show that SyntaxAware DOES apply tree-sitter indentation
    init_test(cx, |settings| {
        settings.defaults.auto_indent = Some(settings::AutoIndentMode::SyntaxAware)
    });

    let language = Arc::new(
        Language::new(
            LanguageConfig {
                brackets: BracketPairConfig {
                    pairs: vec![BracketPair {
                        start: "{".to_string(),
                        end: "}".to_string(),
                        close: false,
                        surround: false,
                        newline: false, // Disable extra newline behavior to isolate syntax indent test
                    }],
                    ..Default::default()
                },
                ..Default::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_indents_query(r#"(_ "{" "}" @end) @indent"#)
        .unwrap(),
    );

    let buffer =
        cx.new(|cx| Buffer::local("fn foo() {\n}", cx).with_language(language.clone(), cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));
    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    // Position cursor at end of line containing `{`
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(10)..MultiBufferOffset(10)]) // After "fn foo() {"
        });
        editor.newline(&Newline, window, cx);

        // With SyntaxAware, tree-sitter adds indentation for being inside `{}`
        assert_eq!(editor.text(cx), "fn foo() {\n    \n}");
    });
}

#[gpui::test]
async fn test_ime_composition_keeps_manual_indent(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.auto_indent = Some(settings::AutoIndentMode::SyntaxAware)
    });

    let mut cx = EditorTestContext::new(cx).await;
    let language = Arc::new(
        Language::new(
            LanguageConfig {
                brackets: BracketPairConfig {
                    pairs: vec![BracketPair {
                        start: "{".to_string(),
                        end: "}".to_string(),
                        close: false,
                        surround: false,
                        newline: false,
                    }],
                    ..Default::default()
                },
                ..Default::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_indents_query(r#"(_ "{" "}" @end) @indent"#)
        .unwrap(),
    );
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    cx.set_state("fn foo() {ˇ\n}");
    cx.condition(|editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
        // A manual indent here
        editor.tab(&Tab, window, cx);
    });
    assert_eq!(cx.buffer_text(), "fn foo() {\n        \n}");

    cx.update_editor(|editor, window, cx| {
        editor.replace_and_mark_text_in_range(None, "n", None, window, cx);
        editor.replace_and_mark_text_in_range(None, "nn", None, window, cx);
    });

    cx.run_until_parked();

    assert_eq!(cx.buffer_text(), "fn foo() {\n        nn\n}");
}

#[gpui::test]
async fn test_autoindent_disabled_with_nested_language(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.auto_indent = Some(settings::AutoIndentMode::SyntaxAware);
        settings.languages.0.insert(
            "python".into(),
            LanguageSettingsContent {
                auto_indent: Some(settings::AutoIndentMode::None),
                ..Default::default()
            },
        );
    });

    let mut cx = EditorTestContext::new(cx).await;

    let injected_language = Arc::new(
        Language::new(
            LanguageConfig {
                brackets: BracketPairConfig {
                    pairs: vec![
                        BracketPair {
                            start: "{".to_string(),
                            end: "}".to_string(),
                            close: false,
                            surround: false,
                            newline: true,
                        },
                        BracketPair {
                            start: "(".to_string(),
                            end: ")".to_string(),
                            close: true,
                            surround: false,
                            newline: true,
                        },
                    ],
                    ..Default::default()
                },
                name: "python".into(),
                ..Default::default()
            },
            Some(tree_sitter_python::LANGUAGE.into()),
        )
        .with_indents_query(
            r#"
                (_ "(" ")" @end) @indent
                (_ "{" "}" @end) @indent
            "#,
        )
        .unwrap(),
    );

    let language = Arc::new(
        Language::new(
            LanguageConfig {
                brackets: BracketPairConfig {
                    pairs: vec![
                        BracketPair {
                            start: "{".to_string(),
                            end: "}".to_string(),
                            close: false,
                            surround: false,
                            newline: true,
                        },
                        BracketPair {
                            start: "(".to_string(),
                            end: ")".to_string(),
                            close: true,
                            surround: false,
                            newline: true,
                        },
                    ],
                    ..Default::default()
                },
                name: LanguageName::new_static("rust"),
                ..Default::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_indents_query(
            r#"
                (_ "(" ")" @end) @indent
                (_ "{" "}" @end) @indent
            "#,
        )
        .unwrap()
        .with_injection_query(
            r#"
            (macro_invocation
                macro: (identifier) @_macro_name
                (token_tree) @injection.content
                (#set! injection.language "python"))
           "#,
        )
        .unwrap(),
    );

    cx.language_registry().add(injected_language);
    cx.language_registry().add(language.clone());

    cx.update_buffer(|buffer, cx| {
        buffer.set_language(Some(language), cx);
    });

    cx.set_state(r#"struct A {ˇ}"#);

    cx.update_editor(|editor, window, cx| {
        editor.newline(&Default::default(), window, cx);
    });

    cx.assert_editor_state(indoc!(
        "struct A {
            ˇ
        }"
    ));

    cx.set_state(r#"select_biased!(ˇ)"#);

    cx.update_editor(|editor, window, cx| {
        editor.newline(&Default::default(), window, cx);
        editor.handle_input("def ", window, cx);
        editor.handle_input("(", window, cx);
        editor.newline(&Default::default(), window, cx);
        editor.handle_input("a", window, cx);
    });

    cx.assert_editor_state(indoc!(
        "select_biased!(
        def (
        aˇ
        )
        )"
    ));
}

#[gpui::test]
async fn test_autoindent_selections(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    {
        let mut cx = EditorLanguageTestContext::new_rust(cx).await;
        cx.set_state(indoc! {"
            impl A {

                fn b() {}

            «fn c() {

            }ˇ»
            }
        "});

        cx.update_editor(|editor, window, cx| {
            editor.autoindent(&Default::default(), window, cx);
        });
        cx.wait_for_autoindent_applied().await;

        cx.assert_editor_state(indoc! {"
            impl A {

                fn b() {}

                «fn c() {

                }ˇ»
            }
        "});
    }

    {
        let mut cx = EditorTestContext::new_multibuffer(
            cx,
            [indoc! { "
                impl A {
                «
                // a
                fn b(){}
                »
                «
                    }
                    fn c(){}
                »
            "}],
        );

        let buffer = cx.update_editor(|editor, _, cx| {
            let buffer = editor.buffer().update(cx, |buffer, _| {
                buffer.all_buffers().iter().next().unwrap().clone()
            });
            buffer.update(cx, |buffer, cx| buffer.set_language(Some(rust_lang()), cx));
            buffer
        });

        cx.run_until_parked();
        cx.update_editor(|editor, window, cx| {
            editor.select_all(&Default::default(), window, cx);
            editor.autoindent(&Default::default(), window, cx)
        });
        cx.run_until_parked();

        cx.update(|_, cx| {
            assert_eq!(
                buffer.read(cx).text(),
                indoc! { "
                    impl A {

                        // a
                        fn b(){}


                    }
                    fn c(){}

                " }
            )
        });
    }
}

#[gpui::test]
async fn test_autoclose_nested_brackets_in_plain_text(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let language = PLAIN_TEXT.clone();
    cx.language_registry().add(language.clone());
    cx.update_buffer(|buffer, cx| {
        buffer.set_language(Some(language), cx);
    });

    cx.set_state("ˇ");

    // Typing an opening bracket right before an existing closing bracket should
    // still autoclose, not skip because the following char is `)`.
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("(", window, cx);
        editor.handle_input("[", window, cx);
        editor.handle_input("{", window, cx);
    });
    cx.assert_editor_state("([{ˇ}])");
}

#[gpui::test]
async fn test_autoclose_and_auto_surround_pairs(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let language = Arc::new(Language::new(
        LanguageConfig {
            brackets: BracketPairConfig {
                pairs: vec![
                    BracketPair {
                        start: "{".to_string(),
                        end: "}".to_string(),
                        close: true,
                        surround: true,
                        newline: true,
                    },
                    BracketPair {
                        start: "(".to_string(),
                        end: ")".to_string(),
                        close: true,
                        surround: true,
                        newline: true,
                    },
                    BracketPair {
                        start: "/*".to_string(),
                        end: " */".to_string(),
                        close: true,
                        surround: true,
                        newline: true,
                    },
                    BracketPair {
                        start: "[".to_string(),
                        end: "]".to_string(),
                        close: false,
                        surround: false,
                        newline: true,
                    },
                    BracketPair {
                        start: "\"".to_string(),
                        end: "\"".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                    BracketPair {
                        start: "<".to_string(),
                        end: ">".to_string(),
                        close: false,
                        surround: true,
                        newline: true,
                    },
                ],
                ..Default::default()
            },
            autoclose_before: "})]".to_string(),
            ..Default::default()
        },
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    cx.language_registry().add(language.clone());
    cx.update_buffer(|buffer, cx| {
        buffer.set_language(Some(language), cx);
    });

    cx.set_state(
        &r#"
            🏀ˇ
            εˇ
            ❤️ˇ
        "#
        .unindent(),
    );

    // autoclose multiple nested brackets at multiple cursors
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("{", window, cx);
        editor.handle_input("{", window, cx);
        editor.handle_input("{", window, cx);
    });
    cx.assert_editor_state(
        &"
            🏀{{{ˇ}}}
            ε{{{ˇ}}}
            ❤️{{{ˇ}}}
        "
        .unindent(),
    );

    // insert a different closing bracket
    cx.update_editor(|editor, window, cx| {
        editor.handle_input(")", window, cx);
    });
    cx.assert_editor_state(
        &"
            🏀{{{)ˇ}}}
            ε{{{)ˇ}}}
            ❤️{{{)ˇ}}}
        "
        .unindent(),
    );

    // skip over the auto-closed brackets when typing a closing bracket
    cx.update_editor(|editor, window, cx| {
        editor.move_right(&MoveRight, window, cx);
        editor.handle_input("}", window, cx);
        editor.handle_input("}", window, cx);
        editor.handle_input("}", window, cx);
    });
    cx.assert_editor_state(
        &"
            🏀{{{)}}}}ˇ
            ε{{{)}}}}ˇ
            ❤️{{{)}}}}ˇ
        "
        .unindent(),
    );

    // autoclose multi-character pairs
    cx.set_state(
        &"
            ˇ
            ˇ
        "
        .unindent(),
    );
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("/", window, cx);
        editor.handle_input("*", window, cx);
    });
    cx.assert_editor_state(
        &"
            /*ˇ */
            /*ˇ */
        "
        .unindent(),
    );

    // one cursor autocloses a multi-character pair, one cursor
    // does not autoclose.
    cx.set_state(
        &"
            /ˇ
            ˇ
        "
        .unindent(),
    );
    cx.update_editor(|editor, window, cx| editor.handle_input("*", window, cx));
    cx.assert_editor_state(
        &"
            /*ˇ */
            *ˇ
        "
        .unindent(),
    );

    // Don't autoclose if the next character isn't whitespace and isn't
    // listed in the language's "autoclose_before" section.
    cx.set_state("ˇa b");
    cx.update_editor(|editor, window, cx| editor.handle_input("{", window, cx));
    cx.assert_editor_state("{ˇa b");

    // Don't autoclose if `close` is false for the bracket pair
    cx.set_state("ˇ");
    cx.update_editor(|editor, window, cx| editor.handle_input("[", window, cx));
    cx.assert_editor_state("[ˇ");

    // Surround with brackets if text is selected
    cx.set_state("«aˇ» b");
    cx.update_editor(|editor, window, cx| editor.handle_input("{", window, cx));
    cx.assert_editor_state("{«aˇ»} b");

    // Autoclose when not immediately after a word character
    cx.set_state("a ˇ");
    cx.update_editor(|editor, window, cx| editor.handle_input("\"", window, cx));
    cx.assert_editor_state("a \"ˇ\"");

    // Autoclose pair where the start and end characters are the same
    cx.update_editor(|editor, window, cx| editor.handle_input("\"", window, cx));
    cx.assert_editor_state("a \"\"ˇ");

    // Don't autoclose when immediately after a word character
    cx.set_state("aˇ");
    cx.update_editor(|editor, window, cx| editor.handle_input("\"", window, cx));
    cx.assert_editor_state("a\"ˇ");

    // Do autoclose when after a non-word character
    cx.set_state("{ˇ");
    cx.update_editor(|editor, window, cx| editor.handle_input("\"", window, cx));
    cx.assert_editor_state("{\"ˇ\"");

    // Non identical pairs autoclose regardless of preceding character
    cx.set_state("aˇ");
    cx.update_editor(|editor, window, cx| editor.handle_input("{", window, cx));
    cx.assert_editor_state("a{ˇ}");

    // Don't autoclose pair if autoclose is disabled
    cx.set_state("ˇ");
    cx.update_editor(|editor, window, cx| editor.handle_input("<", window, cx));
    cx.assert_editor_state("<ˇ");

    // Surround with brackets if text is selected and auto_surround is enabled, even if autoclose is disabled
    cx.set_state("«aˇ» b");
    cx.update_editor(|editor, window, cx| editor.handle_input("<", window, cx));
    cx.assert_editor_state("<«aˇ»> b");
}

#[gpui::test]
async fn test_always_treat_brackets_as_autoclosed_skip_over(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.always_treat_brackets_as_autoclosed = Some(true);
    });

    let mut cx = EditorTestContext::new(cx).await;

    let language = Arc::new(Language::new(
        LanguageConfig {
            brackets: BracketPairConfig {
                pairs: vec![
                    BracketPair {
                        start: "{".to_string(),
                        end: "}".to_string(),
                        close: true,
                        surround: true,
                        newline: true,
                    },
                    BracketPair {
                        start: "(".to_string(),
                        end: ")".to_string(),
                        close: true,
                        surround: true,
                        newline: true,
                    },
                    BracketPair {
                        start: "[".to_string(),
                        end: "]".to_string(),
                        close: false,
                        surround: false,
                        newline: true,
                    },
                ],
                ..Default::default()
            },
            autoclose_before: "})]".to_string(),
            ..Default::default()
        },
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    cx.language_registry().add(language.clone());
    cx.update_buffer(|buffer, cx| {
        buffer.set_language(Some(language), cx);
    });

    cx.set_state(
        &"
            ˇ
            ˇ
            ˇ
        "
        .unindent(),
    );

    // ensure only matching closing brackets are skipped over
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("}", window, cx);
        editor.move_left(&MoveLeft, window, cx);
        editor.handle_input(")", window, cx);
        editor.move_left(&MoveLeft, window, cx);
    });
    cx.assert_editor_state(
        &"
            ˇ)}
            ˇ)}
            ˇ)}
        "
        .unindent(),
    );

    // skip-over closing brackets at multiple cursors
    cx.update_editor(|editor, window, cx| {
        editor.handle_input(")", window, cx);
        editor.handle_input("}", window, cx);
    });
    cx.assert_editor_state(
        &"
            )}ˇ
            )}ˇ
            )}ˇ
        "
        .unindent(),
    );

    // ignore non-close brackets
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("]", window, cx);
        editor.move_left(&MoveLeft, window, cx);
        editor.handle_input("]", window, cx);
    });
    cx.assert_editor_state(
        &"
            )}]ˇ]
            )}]ˇ]
            )}]ˇ]
        "
        .unindent(),
    );
}

#[gpui::test]
async fn test_autoclose_with_embedded_language(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let html_language = Arc::new(
        Language::new(
            LanguageConfig {
                name: "HTML".into(),
                brackets: BracketPairConfig {
                    pairs: vec![
                        BracketPair {
                            start: "<".into(),
                            end: ">".into(),
                            close: true,
                            ..Default::default()
                        },
                        BracketPair {
                            start: "{".into(),
                            end: "}".into(),
                            close: true,
                            ..Default::default()
                        },
                        BracketPair {
                            start: "(".into(),
                            end: ")".into(),
                            close: true,
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
                autoclose_before: "})]>".into(),
                ..Default::default()
            },
            Some(tree_sitter_html::LANGUAGE.into()),
        )
        .with_injection_query(
            r#"
            (script_element
                (raw_text) @injection.content
                (#set! injection.language "javascript"))
            "#,
        )
        .unwrap(),
    );

    let javascript_language = Arc::new(Language::new(
        LanguageConfig {
            name: "JavaScript".into(),
            brackets: BracketPairConfig {
                pairs: vec![
                    BracketPair {
                        start: "/*".into(),
                        end: " */".into(),
                        close: true,
                        ..Default::default()
                    },
                    BracketPair {
                        start: "{".into(),
                        end: "}".into(),
                        close: true,
                        ..Default::default()
                    },
                    BracketPair {
                        start: "(".into(),
                        end: ")".into(),
                        close: true,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            autoclose_before: "})]>".into(),
            ..Default::default()
        },
        Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
    ));

    cx.language_registry().add(html_language.clone());
    cx.language_registry().add(javascript_language);
    cx.executor().run_until_parked();

    cx.update_buffer(|buffer, cx| {
        buffer.set_language(Some(html_language), cx);
    });

    cx.set_state(
        &r#"
            <body>ˇ
                <script>
                    var x = 1;ˇ
                </script>
            </body>ˇ
        "#
        .unindent(),
    );

    // Precondition: different languages are active at different locations.
    cx.update_editor(|editor, window, cx| {
        let snapshot = editor.snapshot(window, cx);
        let cursors = editor
            .selections
            .ranges::<MultiBufferOffset>(&editor.display_snapshot(cx));
        let languages = cursors
            .iter()
            .map(|c| snapshot.language_at(c.start).unwrap().name())
            .collect::<Vec<_>>();
        assert_eq!(
            languages,
            &[
                LanguageName::from("HTML"),
                LanguageName::from("JavaScript"),
                LanguageName::from("HTML"),
            ]
        );
    });

    // Angle brackets autoclose in HTML, but not JavaScript.
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("<", window, cx);
        editor.handle_input("a", window, cx);
    });
    cx.assert_editor_state(
        &r#"
            <body><aˇ>
                <script>
                    var x = 1;<aˇ
                </script>
            </body><aˇ>
        "#
        .unindent(),
    );

    // Curly braces and parens autoclose in both HTML and JavaScript.
    cx.update_editor(|editor, window, cx| {
        editor.handle_input(" b=", window, cx);
        editor.handle_input("{", window, cx);
        editor.handle_input("c", window, cx);
        editor.handle_input("(", window, cx);
    });
    cx.assert_editor_state(
        &r#"
            <body><a b={c(ˇ)}>
                <script>
                    var x = 1;<a b={c(ˇ)}
                </script>
            </body><a b={c(ˇ)}>
        "#
        .unindent(),
    );

    // Brackets that were already autoclosed are skipped.
    cx.update_editor(|editor, window, cx| {
        editor.handle_input(")", window, cx);
        editor.handle_input("d", window, cx);
        editor.handle_input("}", window, cx);
    });
    cx.assert_editor_state(
        &r#"
            <body><a b={c()d}ˇ>
                <script>
                    var x = 1;<a b={c()d}ˇ
                </script>
            </body><a b={c()d}ˇ>
        "#
        .unindent(),
    );
    cx.update_editor(|editor, window, cx| {
        editor.handle_input(">", window, cx);
    });
    cx.assert_editor_state(
        &r#"
            <body><a b={c()d}>ˇ
                <script>
                    var x = 1;<a b={c()d}>ˇ
                </script>
            </body><a b={c()d}>ˇ
        "#
        .unindent(),
    );

    // Reset
    cx.set_state(
        &r#"
            <body>ˇ
                <script>
                    var x = 1;ˇ
                </script>
            </body>ˇ
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.handle_input("<", window, cx);
    });
    cx.assert_editor_state(
        &r#"
            <body><ˇ>
                <script>
                    var x = 1;<ˇ
                </script>
            </body><ˇ>
        "#
        .unindent(),
    );

    // When backspacing, the closing angle brackets are removed.
    cx.update_editor(|editor, window, cx| {
        editor.backspace(&Backspace, window, cx);
    });
    cx.assert_editor_state(
        &r#"
            <body>ˇ
                <script>
                    var x = 1;ˇ
                </script>
            </body>ˇ
        "#
        .unindent(),
    );

    // Block comments autoclose in JavaScript, but not HTML.
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("/", window, cx);
        editor.handle_input("*", window, cx);
    });
    cx.assert_editor_state(
        &r#"
            <body>/*ˇ
                <script>
                    var x = 1;/*ˇ */
                </script>
            </body>/*ˇ
        "#
        .unindent(),
    );
}

#[gpui::test]
async fn test_autoclose_with_overrides(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let rust_language = Arc::new(
        Language::new(
            LanguageConfig {
                name: "Rust".into(),
                brackets: serde_json::from_value(json!([
                    { "start": "{", "end": "}", "close": true, "newline": true },
                    { "start": "\"", "end": "\"", "close": true, "newline": false, "not_in": ["string"] },
                ]))
                .unwrap(),
                autoclose_before: "})]>".into(),
                ..Default::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_override_query("(string_literal) @string")
        .unwrap(),
    );

    cx.language_registry().add(rust_language.clone());
    cx.update_buffer(|buffer, cx| {
        buffer.set_language(Some(rust_language), cx);
    });

    cx.set_state(
        &r#"
            let x = ˇ
        "#
        .unindent(),
    );

    // Inserting a quotation mark. A closing quotation mark is automatically inserted.
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("\"", window, cx);
    });
    cx.assert_editor_state(
        &r#"
            let x = "ˇ"
        "#
        .unindent(),
    );

    // Inserting another quotation mark. The cursor moves across the existing
    // automatically-inserted quotation mark.
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("\"", window, cx);
    });
    cx.assert_editor_state(
        &r#"
            let x = ""ˇ
        "#
        .unindent(),
    );

    // Reset
    cx.set_state(
        &r#"
            let x = ˇ
        "#
        .unindent(),
    );

    // Inserting a quotation mark inside of a string. A second quotation mark is not inserted.
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("\"", window, cx);
        editor.handle_input(" ", window, cx);
        editor.move_left(&Default::default(), window, cx);
        editor.handle_input("\\", window, cx);
        editor.handle_input("\"", window, cx);
    });
    cx.assert_editor_state(
        &r#"
            let x = "\"ˇ "
        "#
        .unindent(),
    );

    // Inserting a closing quotation mark at the position of an automatically-inserted quotation
    // mark. Nothing is inserted.
    cx.update_editor(|editor, window, cx| {
        editor.move_right(&Default::default(), window, cx);
        editor.handle_input("\"", window, cx);
    });
    cx.assert_editor_state(
        &r#"
            let x = "\" "ˇ
        "#
        .unindent(),
    );
}

#[gpui::test]
async fn test_autoclose_quotes_with_scope_awareness(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    let language = languages::language("python", tree_sitter_python::LANGUAGE.into());

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    // Double quote inside single-quoted string
    cx.set_state(indoc! {r#"
        def main():
            items = ['"', ˇ]
    "#});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("\"", window, cx);
    });
    cx.assert_editor_state(indoc! {r#"
        def main():
            items = ['"', "ˇ"]
    "#});

    // Two double quotes inside single-quoted string
    cx.set_state(indoc! {r#"
        def main():
            items = ['""', ˇ]
    "#});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("\"", window, cx);
    });
    cx.assert_editor_state(indoc! {r#"
        def main():
            items = ['""', "ˇ"]
    "#});

    // Single quote inside double-quoted string
    cx.set_state(indoc! {r#"
        def main():
            items = ["'", ˇ]
    "#});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("'", window, cx);
    });
    cx.assert_editor_state(indoc! {r#"
        def main():
            items = ["'", 'ˇ']
    "#});

    // Two single quotes inside double-quoted string
    cx.set_state(indoc! {r#"
        def main():
            items = ["''", ˇ]
    "#});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("'", window, cx);
    });
    cx.assert_editor_state(indoc! {r#"
        def main():
            items = ["''", 'ˇ']
    "#});

    // Mixed quotes on same line
    cx.set_state(indoc! {r#"
        def main():
            items = ['"""', "'''''", ˇ]
    "#});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("\"", window, cx);
    });
    cx.assert_editor_state(indoc! {r#"
        def main():
            items = ['"""', "'''''", "ˇ"]
    "#});
    cx.update_editor(|editor, window, cx| {
        editor.move_right(&MoveRight, window, cx);
    });
    cx.update_editor(|editor, window, cx| {
        editor.handle_input(", ", window, cx);
    });
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("'", window, cx);
    });
    cx.assert_editor_state(indoc! {r#"
        def main():
            items = ['"""', "'''''", "", 'ˇ']
    "#});
}

#[gpui::test]
async fn test_autoclose_quotes_with_multibyte_characters(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    let language = languages::language("python", tree_sitter_python::LANGUAGE.into());
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    cx.set_state(indoc! {r#"
        def main():
            items = ["🎉", ˇ]
    "#});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("\"", window, cx);
    });
    cx.assert_editor_state(indoc! {r#"
        def main():
            items = ["🎉", "ˇ"]
    "#});
}

#[gpui::test]
async fn test_surround_backticks_in_rust(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| {
        let language = languages::language("rust", tree_sitter_rust::LANGUAGE.into());
        buffer.set_language(Some(language), cx)
    });

    // Surround a selection inside a doc comment with backticks
    cx.set_state(indoc! {"
        /// «Aˇ»
        fn main() {}
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("`", window, cx);
    });
    cx.assert_editor_state(indoc! {"
        /// `«Aˇ»`
        fn main() {}
    "});

    // When inside a string literal, the backtick pair is disabled so typing a
    // backtick should replace the selection instead of surrounding it.
    cx.set_state(indoc! {r#"
        fn main() {
            let name = "«Jesper Kouthoofdˇ»";
        }
    "#});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("`", window, cx);
    });
    cx.assert_editor_state(indoc! {r#"
        fn main() {
            let name = "`ˇ";
        }
    "#});
}

#[gpui::test]
async fn test_surround_with_pair(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(Language::new(
        LanguageConfig {
            brackets: BracketPairConfig {
                pairs: vec![
                    BracketPair {
                        start: "{".to_string(),
                        end: "}".to_string(),
                        close: true,
                        surround: true,
                        newline: true,
                    },
                    BracketPair {
                        start: "/* ".to_string(),
                        end: "*/".to_string(),
                        close: true,
                        surround: true,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            ..Default::default()
        },
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    let text = r#"
        a
        b
        c
    "#
    .unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));
    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(2), 0)..DisplayPoint::new(DisplayRow(2), 1),
            ])
        });

        editor.handle_input("{", window, cx);
        editor.handle_input("{", window, cx);
        editor.handle_input("{", window, cx);
        assert_eq!(
            editor.text(cx),
            "
                {{{a}}}
                {{{b}}}
                {{{c}}}
            "
            .unindent()
        );
        assert_eq!(
            display_ranges(editor, cx),
            [
                DisplayPoint::new(DisplayRow(0), 3)..DisplayPoint::new(DisplayRow(0), 4),
                DisplayPoint::new(DisplayRow(1), 3)..DisplayPoint::new(DisplayRow(1), 4),
                DisplayPoint::new(DisplayRow(2), 3)..DisplayPoint::new(DisplayRow(2), 4)
            ]
        );

        editor.undo(&Undo, window, cx);
        editor.undo(&Undo, window, cx);
        editor.undo(&Undo, window, cx);
        assert_eq!(
            editor.text(cx),
            "
                a
                b
                c
            "
            .unindent()
        );
        assert_eq!(
            display_ranges(editor, cx),
            [
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(2), 0)..DisplayPoint::new(DisplayRow(2), 1)
            ]
        );

        // Ensure inserting the first character of a multi-byte bracket pair
        // doesn't surround the selections with the bracket.
        editor.handle_input("/", window, cx);
        assert_eq!(
            editor.text(cx),
            "
                /
                /
                /
            "
            .unindent()
        );
        assert_eq!(
            display_ranges(editor, cx),
            [
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(1), 1)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(2), 1)..DisplayPoint::new(DisplayRow(2), 1)
            ]
        );

        editor.undo(&Undo, window, cx);
        assert_eq!(
            editor.text(cx),
            "
                a
                b
                c
            "
            .unindent()
        );
        assert_eq!(
            display_ranges(editor, cx),
            [
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(2), 0)..DisplayPoint::new(DisplayRow(2), 1)
            ]
        );

        // Ensure inserting the last character of a multi-byte bracket pair
        // doesn't surround the selections with the bracket.
        editor.handle_input("*", window, cx);
        assert_eq!(
            editor.text(cx),
            "
                *
                *
                *
            "
            .unindent()
        );
        assert_eq!(
            display_ranges(editor, cx),
            [
                DisplayPoint::new(DisplayRow(0), 1)..DisplayPoint::new(DisplayRow(0), 1),
                DisplayPoint::new(DisplayRow(1), 1)..DisplayPoint::new(DisplayRow(1), 1),
                DisplayPoint::new(DisplayRow(2), 1)..DisplayPoint::new(DisplayRow(2), 1)
            ]
        );
    });
}

#[gpui::test]
async fn test_delete_autoclose_pair(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(Language::new(
        LanguageConfig {
            brackets: BracketPairConfig {
                pairs: vec![BracketPair {
                    start: "{".to_string(),
                    end: "}".to_string(),
                    close: true,
                    surround: true,
                    newline: true,
                }],
                ..Default::default()
            },
            autoclose_before: "}".to_string(),
            ..Default::default()
        },
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    let text = r#"
        a
        b
        c
    "#
    .unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));
    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([
                Point::new(0, 1)..Point::new(0, 1),
                Point::new(1, 1)..Point::new(1, 1),
                Point::new(2, 1)..Point::new(2, 1),
            ])
        });

        editor.handle_input("{", window, cx);
        editor.handle_input("{", window, cx);
        editor.handle_input("_", window, cx);
        assert_eq!(
            editor.text(cx),
            "
                a{{_}}
                b{{_}}
                c{{_}}
            "
            .unindent()
        );
        assert_eq!(
            editor
                .selections
                .ranges::<Point>(&editor.display_snapshot(cx)),
            [
                Point::new(0, 4)..Point::new(0, 4),
                Point::new(1, 4)..Point::new(1, 4),
                Point::new(2, 4)..Point::new(2, 4)
            ]
        );

        editor.backspace(&Default::default(), window, cx);
        editor.backspace(&Default::default(), window, cx);
        assert_eq!(
            editor.text(cx),
            "
                a{}
                b{}
                c{}
            "
            .unindent()
        );
        assert_eq!(
            editor
                .selections
                .ranges::<Point>(&editor.display_snapshot(cx)),
            [
                Point::new(0, 2)..Point::new(0, 2),
                Point::new(1, 2)..Point::new(1, 2),
                Point::new(2, 2)..Point::new(2, 2)
            ]
        );

        editor.delete_to_previous_word_start(&Default::default(), window, cx);
        assert_eq!(
            editor.text(cx),
            "
                a
                b
                c
            "
            .unindent()
        );
        assert_eq!(
            editor
                .selections
                .ranges::<Point>(&editor.display_snapshot(cx)),
            [
                Point::new(0, 1)..Point::new(0, 1),
                Point::new(1, 1)..Point::new(1, 1),
                Point::new(2, 1)..Point::new(2, 1)
            ]
        );
    });
}

#[gpui::test]
async fn test_always_treat_brackets_as_autoclosed_delete(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.always_treat_brackets_as_autoclosed = Some(true);
    });

    let mut cx = EditorTestContext::new(cx).await;

    let language = Arc::new(Language::new(
        LanguageConfig {
            brackets: BracketPairConfig {
                pairs: vec![
                    BracketPair {
                        start: "{".to_string(),
                        end: "}".to_string(),
                        close: true,
                        surround: true,
                        newline: true,
                    },
                    BracketPair {
                        start: "(".to_string(),
                        end: ")".to_string(),
                        close: true,
                        surround: true,
                        newline: true,
                    },
                    BracketPair {
                        start: "[".to_string(),
                        end: "]".to_string(),
                        close: false,
                        surround: true,
                        newline: true,
                    },
                ],
                ..Default::default()
            },
            autoclose_before: "})]".to_string(),
            ..Default::default()
        },
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    cx.language_registry().add(language.clone());
    cx.update_buffer(|buffer, cx| {
        buffer.set_language(Some(language), cx);
    });

    cx.set_state(
        &"
            {(ˇ)}
            [[ˇ]]
            {(ˇ)}
        "
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.backspace(&Default::default(), window, cx);
        editor.backspace(&Default::default(), window, cx);
    });

    cx.assert_editor_state(
        &"
            ˇ
            ˇ]]
            ˇ
        "
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.handle_input("{", window, cx);
        editor.handle_input("{", window, cx);
        editor.move_right(&MoveRight, window, cx);
        editor.move_right(&MoveRight, window, cx);
        editor.move_left(&MoveLeft, window, cx);
        editor.move_left(&MoveLeft, window, cx);
        editor.backspace(&Default::default(), window, cx);
    });

    cx.assert_editor_state(
        &"
            {ˇ}
            {ˇ}]]
            {ˇ}
        "
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.backspace(&Default::default(), window, cx);
    });

    cx.assert_editor_state(
        &"
            ˇ
            ˇ]]
            ˇ
        "
        .unindent(),
    );
}

#[gpui::test]
async fn test_auto_replace_emoji_shortcode(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(Language::new(
        LanguageConfig::default(),
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    let buffer = cx.new(|cx| Buffer::local("", cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));
    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.set_auto_replace_emoji_shortcode(true);

        editor.handle_input("Hello ", window, cx);
        editor.handle_input(":wave", window, cx);
        assert_eq!(editor.text(cx), "Hello :wave".unindent());

        editor.handle_input(":", window, cx);
        assert_eq!(editor.text(cx), "Hello 👋".unindent());

        editor.handle_input(" :smile", window, cx);
        assert_eq!(editor.text(cx), "Hello 👋 :smile".unindent());

        editor.handle_input(":", window, cx);
        assert_eq!(editor.text(cx), "Hello 👋 😄".unindent());

        // Ensure shortcode gets replaced when it is part of a word that only consists of emojis
        editor.handle_input(":wave", window, cx);
        assert_eq!(editor.text(cx), "Hello 👋 😄:wave".unindent());

        editor.handle_input(":", window, cx);
        assert_eq!(editor.text(cx), "Hello 👋 😄👋".unindent());

        editor.handle_input(":1", window, cx);
        assert_eq!(editor.text(cx), "Hello 👋 😄👋:1".unindent());

        editor.handle_input(":", window, cx);
        assert_eq!(editor.text(cx), "Hello 👋 😄👋:1:".unindent());

        // Ensure shortcode does not get replaced when it is part of a word
        editor.handle_input(" Test:wave", window, cx);
        assert_eq!(editor.text(cx), "Hello 👋 😄👋:1: Test:wave".unindent());

        editor.handle_input(":", window, cx);
        assert_eq!(editor.text(cx), "Hello 👋 😄👋:1: Test:wave:".unindent());

        editor.set_auto_replace_emoji_shortcode(false);

        // Ensure shortcode does not get replaced when auto replace is off
        editor.handle_input(" :wave", window, cx);
        assert_eq!(
            editor.text(cx),
            "Hello 👋 😄👋:1: Test:wave: :wave".unindent()
        );

        editor.handle_input(":", window, cx);
        assert_eq!(
            editor.text(cx),
            "Hello 👋 😄👋:1: Test:wave: :wave:".unindent()
        );
    });
}

#[gpui::test]
async fn test_snippet_placeholder_choices(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let (text, insertion_ranges) = marked_text_ranges(
        indoc! {"
            ˇ
        "},
        false,
    );

    let buffer = cx.update(|cx| MultiBuffer::build_simple(&text, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    _ = editor.update_in(cx, |editor, window, cx| {
        let snippet = Snippet::parse("type ${1|,i32,u32|} = $2").unwrap();

        editor
            .insert_snippet(
                &insertion_ranges
                    .iter()
                    .map(|range| MultiBufferOffset(range.start)..MultiBufferOffset(range.end))
                    .collect::<Vec<_>>(),
                snippet,
                window,
                cx,
            )
            .unwrap();

        fn assert(editor: &mut Editor, cx: &mut Context<Editor>, marked_text: &str) {
            let (expected_text, selection_ranges) = marked_text_ranges(marked_text, false);
            assert_eq!(editor.text(cx), expected_text);
            assert_eq!(
                editor.selections.ranges(&editor.display_snapshot(cx)),
                selection_ranges
                    .iter()
                    .map(|range| MultiBufferOffset(range.start)..MultiBufferOffset(range.end))
                    .collect::<Vec<_>>()
            );
        }

        assert(
            editor,
            cx,
            indoc! {"
            type «» =•
            "},
        );

        assert!(editor.context_menu_visible(), "There should be a matches");
    });
}

#[gpui::test]
async fn test_snippet_choices_menu_survives_completion_refresh(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let (text, insertion_ranges) = marked_text_ranges(
        indoc! {"
            ˇ
        "},
        false,
    );

    let buffer = cx.update(|cx| MultiBuffer::build_simple(&text, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    _ = editor.update_in(cx, |editor, window, cx| {
        let snippet = Snippet::parse("type ${1|i32,u32|} = $2").unwrap();

        editor
            .insert_snippet(
                &insertion_ranges
                    .iter()
                    .map(|range| MultiBufferOffset(range.start)..MultiBufferOffset(range.end))
                    .collect::<Vec<_>>(),
                snippet,
                window,
                cx,
            )
            .unwrap();

        assert!(
            editor.context_menu_visible(),
            "Snippet choices menu should be visible after inserting the choice tabstop"
        );

        editor.open_or_update_completions_menu(None, None, false, window, cx);

        assert!(
            editor.context_menu_visible(),
            "Snippet choices menu should remain visible after a completion refresh with an empty query"
        );
    });
}

#[gpui::test]
async fn test_snippet_tabstop_navigation_with_placeholders(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    fn assert_state(editor: &mut Editor, cx: &mut Context<Editor>, marked_text: &str) {
        let (expected_text, selection_ranges) = marked_text_ranges(marked_text, false);
        assert_eq!(editor.text(cx), expected_text);
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            selection_ranges
                .iter()
                .map(|range| MultiBufferOffset(range.start)..MultiBufferOffset(range.end))
                .collect::<Vec<_>>()
        );
    }

    let (text, insertion_ranges) = marked_text_ranges(
        indoc! {"
            ˇ
        "},
        false,
    );

    let buffer = cx.update(|cx| MultiBuffer::build_simple(&text, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    _ = editor.update_in(cx, |editor, window, cx| {
        let snippet = Snippet::parse("type ${1|,i32,u32|} = $2; $3").unwrap();

        editor
            .insert_snippet(
                &insertion_ranges
                    .iter()
                    .map(|range| MultiBufferOffset(range.start)..MultiBufferOffset(range.end))
                    .collect::<Vec<_>>(),
                snippet,
                window,
                cx,
            )
            .unwrap();

        assert_state(
            editor,
            cx,
            indoc! {"
            type «» = ;•
            "},
        );

        assert!(
            editor.context_menu_visible(),
            "Context menu should be visible for placeholder choices"
        );

        editor.next_snippet_tabstop(&NextSnippetTabstop, window, cx);

        assert_state(
            editor,
            cx,
            indoc! {"
            type  = «»;•
            "},
        );

        assert!(
            !editor.context_menu_visible(),
            "Context menu should be hidden after moving to next tabstop"
        );

        editor.next_snippet_tabstop(&NextSnippetTabstop, window, cx);

        assert_state(
            editor,
            cx,
            indoc! {"
            type  = ; ˇ
            "},
        );

        editor.next_snippet_tabstop(&NextSnippetTabstop, window, cx);

        assert_state(
            editor,
            cx,
            indoc! {"
            type  = ; ˇ
            "},
        );
    });

    _ = editor.update_in(cx, |editor, window, cx| {
        editor.select_all(&SelectAll, window, cx);
        editor.backspace(&Backspace, window, cx);

        let snippet = Snippet::parse("fn ${1|,foo,bar|} = ${2:value}; $3").unwrap();
        let insertion_ranges = editor
            .selections
            .all(&editor.display_snapshot(cx))
            .iter()
            .map(|s| s.range())
            .collect::<Vec<_>>();

        editor
            .insert_snippet(&insertion_ranges, snippet, window, cx)
            .unwrap();

        assert_state(editor, cx, "fn «» = value;•");

        assert!(
            editor.context_menu_visible(),
            "Context menu should be visible for placeholder choices"
        );

        editor.next_snippet_tabstop(&NextSnippetTabstop, window, cx);

        assert_state(editor, cx, "fn  = «valueˇ»;•");

        editor.previous_snippet_tabstop(&PreviousSnippetTabstop, window, cx);

        assert_state(editor, cx, "fn «» = value;•");

        assert!(
            editor.context_menu_visible(),
            "Context menu should be visible again after returning to first tabstop"
        );

        editor.previous_snippet_tabstop(&PreviousSnippetTabstop, window, cx);

        assert_state(editor, cx, "fn «» = value;•");
    });
}

#[gpui::test]
async fn test_snippets(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"
        a.ˇ b
        a.ˇ b
        a.ˇ b
    "});

    cx.update_editor(|editor, window, cx| {
        let snippet = Snippet::parse("f(${1:one}, ${2:two}, ${1:three})$0").unwrap();
        let insertion_ranges = editor
            .selections
            .all(&editor.display_snapshot(cx))
            .iter()
            .map(|s| s.range())
            .collect::<Vec<_>>();
        editor
            .insert_snippet(&insertion_ranges, snippet, window, cx)
            .unwrap();
    });

    cx.assert_editor_state(indoc! {"
        a.f(«oneˇ», two, «threeˇ») b
        a.f(«oneˇ», two, «threeˇ») b
        a.f(«oneˇ», two, «threeˇ») b
    "});

    // Can't move earlier than the first tab stop
    cx.update_editor(|editor, window, cx| {
        assert!(!editor.move_to_prev_snippet_tabstop(window, cx))
    });
    cx.assert_editor_state(indoc! {"
        a.f(«oneˇ», two, «threeˇ») b
        a.f(«oneˇ», two, «threeˇ») b
        a.f(«oneˇ», two, «threeˇ») b
    "});

    cx.update_editor(|editor, window, cx| assert!(editor.move_to_next_snippet_tabstop(window, cx)));
    cx.assert_editor_state(indoc! {"
        a.f(one, «twoˇ», three) b
        a.f(one, «twoˇ», three) b
        a.f(one, «twoˇ», three) b
    "});

    cx.update_editor(|editor, window, cx| assert!(editor.move_to_prev_snippet_tabstop(window, cx)));
    cx.assert_editor_state(indoc! {"
        a.f(«oneˇ», two, «threeˇ») b
        a.f(«oneˇ», two, «threeˇ») b
        a.f(«oneˇ», two, «threeˇ») b
    "});

    cx.update_editor(|editor, window, cx| assert!(editor.move_to_next_snippet_tabstop(window, cx)));
    cx.assert_editor_state(indoc! {"
        a.f(one, «twoˇ», three) b
        a.f(one, «twoˇ», three) b
        a.f(one, «twoˇ», three) b
    "});
    cx.update_editor(|editor, window, cx| assert!(editor.move_to_next_snippet_tabstop(window, cx)));
    cx.assert_editor_state(indoc! {"
        a.f(one, two, three)ˇ b
        a.f(one, two, three)ˇ b
        a.f(one, two, three)ˇ b
    "});

    // As soon as the last tab stop is reached, snippet state is gone
    cx.update_editor(|editor, window, cx| {
        assert!(!editor.move_to_prev_snippet_tabstop(window, cx))
    });
    cx.assert_editor_state(indoc! {"
        a.f(one, two, three)ˇ b
        a.f(one, two, three)ˇ b
        a.f(one, two, three)ˇ b
    "});
}

#[gpui::test]
async fn test_snippet_indentation(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.update_editor(|editor, window, cx| {
        let snippet = Snippet::parse(indoc! {"
            /*
             * Multiline comment with leading indentation
             *
             * $1
             */
            $0"})
        .unwrap();
        let insertion_ranges = editor
            .selections
            .all(&editor.display_snapshot(cx))
            .iter()
            .map(|s| s.range())
            .collect::<Vec<_>>();
        editor
            .insert_snippet(&insertion_ranges, snippet, window, cx)
            .unwrap();
    });

    cx.assert_editor_state(indoc! {"
        /*
         * Multiline comment with leading indentation
         *
         * ˇ
         */
    "});

    cx.update_editor(|editor, window, cx| assert!(editor.move_to_next_snippet_tabstop(window, cx)));
    cx.assert_editor_state(indoc! {"
        /*
         * Multiline comment with leading indentation
         *
         *•
         */
        ˇ"});
}

#[gpui::test]
async fn test_snippet_with_multi_word_prefix(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_editor(|editor, _, cx| {
        editor.project().unwrap().update(cx, |project, cx| {
            project.snippets().update(cx, |snippets, _cx| {
                let snippet = project::snippet_provider::Snippet {
                    prefix: vec!["multi word".to_string()],
                    body: "this is many words".to_string(),
                    description: Some("description".to_string()),
                    name: "multi-word snippet test".to_string(),
                };
                snippets.add_snippet_for_test(
                    None,
                    PathBuf::from("test_snippets.json"),
                    vec![Arc::new(snippet)],
                );
            });
        })
    });

    for (input_to_simulate, should_match_snippet) in [
        ("m", true),
        ("m ", true),
        ("m w", true),
        ("aa m w", true),
        ("aa m g", false),
    ] {
        cx.set_state("ˇ");
        cx.simulate_input(input_to_simulate); // fails correctly

        cx.update_editor(|editor, _, _| {
            let Some(CodeContextMenu::Completions(context_menu)) = &*editor.context_menu.borrow()
            else {
                assert!(!should_match_snippet); // no completions! don't even show the menu
                return;
            };
            assert!(context_menu.visible());
            let completions = context_menu.completions.borrow();

            assert_eq!(!completions.is_empty(), should_match_snippet);
        });
    }
}

#[gpui::test]
async fn test_redo_after_noop_format(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.ensure_final_newline_on_save = Some(false);
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_file(path!("/file.txt"), "foo".into()).await;

    let project = Project::test(fs, [path!("/file.txt").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/file.txt"), cx)
        })
        .await
        .unwrap();

    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| {
        build_editor_with_project(project.clone(), buffer, window, cx)
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::default(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(0)..MultiBufferOffset(0)])
        });
    });
    assert!(!cx.read(|cx| editor.is_dirty(cx)));

    editor.update_in(cx, |editor, window, cx| {
        editor.handle_input("\n", window, cx)
    });
    cx.run_until_parked();
    save(&editor, &project, cx).await;
    assert_eq!("\nfoo", editor.read_with(cx, |editor, cx| editor.text(cx)));

    editor.update_in(cx, |editor, window, cx| {
        editor.undo(&Default::default(), window, cx);
    });
    save(&editor, &project, cx).await;
    assert_eq!("foo", editor.read_with(cx, |editor, cx| editor.text(cx)));

    editor.update_in(cx, |editor, window, cx| {
        editor.redo(&Default::default(), window, cx);
    });
    cx.run_until_parked();
    assert_eq!("\nfoo", editor.read_with(cx, |editor, cx| editor.text(cx)));

    async fn save(editor: &Entity<Editor>, project: &Entity<Project>, cx: &mut VisualTestContext) {
        let save = editor
            .update_in(cx, |editor, window, cx| {
                editor.save(
                    SaveOptions {
                        format: true,
                        force_format: false,
                        autosave: false,
                    },
                    project.clone(),
                    window,
                    cx,
                )
            })
            .unwrap();
        save.await;
        assert!(!cx.read(|cx| editor.is_dirty(cx)));
    }
}

#[gpui::test]
async fn test_autosave_with_dirty_buffers(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "file1.rs": "fn main() { println!(\"hello\"); }",
            "file2.rs": "fn test() { println!(\"test\"); }",
            "file3.rs": "fn other() { println!(\"other\"); }\n",
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let cx = &mut VisualTestContext::from_window(*window, cx);

    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(rust_lang());

    let worktree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());
    let worktree_id = worktree.update(cx, |worktree, _| worktree.id());

    // Open three buffers
    let buffer_1 = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree_id, rel_path("file1.rs")), cx)
        })
        .await
        .unwrap();
    let buffer_2 = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree_id, rel_path("file2.rs")), cx)
        })
        .await
        .unwrap();
    let buffer_3 = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree_id, rel_path("file3.rs")), cx)
        })
        .await
        .unwrap();

    // Create a multi-buffer with all three buffers
    let multi_buffer = cx.new(|cx| {
        let mut multi_buffer = MultiBuffer::new(ReadWrite);
        multi_buffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1.clone(),
            [Point::new(0, 0)..Point::new(1, 0)],
            0,
            cx,
        );
        multi_buffer.set_excerpts_for_path(
            PathKey::sorted(1),
            buffer_2.clone(),
            [Point::new(0, 0)..Point::new(1, 0)],
            0,
            cx,
        );
        multi_buffer.set_excerpts_for_path(
            PathKey::sorted(2),
            buffer_3.clone(),
            [Point::new(0, 0)..Point::new(1, 0)],
            0,
            cx,
        );
        multi_buffer
    });

    let editor = cx.new_window_entity(|window, cx| {
        Editor::new(
            EditorMode::full(),
            multi_buffer,
            Some(project.clone()),
            window,
            cx,
        )
    });

    // Edit only the first buffer
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(
            SelectionEffects::scroll(Autoscroll::Next),
            window,
            cx,
            |s| s.select_ranges(Some(MultiBufferOffset(10)..MultiBufferOffset(10))),
        );
        editor.insert("// edited", window, cx);
    });

    // Verify that only buffer 1 is dirty
    buffer_1.update(cx, |buffer, _| assert!(buffer.is_dirty()));
    buffer_2.update(cx, |buffer, _| assert!(!buffer.is_dirty()));
    buffer_3.update(cx, |buffer, _| assert!(!buffer.is_dirty()));

    // Get write counts after file creation (files were created with initial content)
    // We expect each file to have been written once during creation
    let write_count_after_creation_1 = fs.write_count_for_path(path!("/dir/file1.rs"));
    let write_count_after_creation_2 = fs.write_count_for_path(path!("/dir/file2.rs"));
    let write_count_after_creation_3 = fs.write_count_for_path(path!("/dir/file3.rs"));

    // Perform autosave
    let save_task = editor.update_in(cx, |editor, window, cx| {
        editor.save(
            SaveOptions {
                format: true,
                force_format: false,
                autosave: true,
            },
            project.clone(),
            window,
            cx,
        )
    });
    save_task.await.unwrap();

    // Only the dirty buffer should have been saved
    assert_eq!(
        fs.write_count_for_path(path!("/dir/file1.rs")) - write_count_after_creation_1,
        1,
        "Buffer 1 was dirty, so it should have been written once during autosave"
    );
    assert_eq!(
        fs.write_count_for_path(path!("/dir/file2.rs")) - write_count_after_creation_2,
        0,
        "Buffer 2 was clean, so it should not have been written during autosave"
    );
    assert_eq!(
        fs.write_count_for_path(path!("/dir/file3.rs")) - write_count_after_creation_3,
        0,
        "Buffer 3 was clean, so it should not have been written during autosave"
    );

    // Verify buffer states after autosave
    buffer_1.update(cx, |buffer, _| assert!(!buffer.is_dirty()));
    buffer_2.update(cx, |buffer, _| assert!(!buffer.is_dirty()));
    buffer_3.update(cx, |buffer, _| assert!(!buffer.is_dirty()));

    // Now perform a manual save (format = true)
    let save_task = editor.update_in(cx, |editor, window, cx| {
        editor.save(
            SaveOptions {
                format: true,
                force_format: false,
                autosave: false,
            },
            project.clone(),
            window,
            cx,
        )
    });
    save_task.await.unwrap();

    // During manual save, clean buffers don't get written to disk
    // They just get did_save called for language server notifications
    assert_eq!(
        fs.write_count_for_path(path!("/dir/file1.rs")) - write_count_after_creation_1,
        1,
        "Buffer 1 should only have been written once total (during autosave, not manual save)"
    );
    assert_eq!(
        fs.write_count_for_path(path!("/dir/file2.rs")) - write_count_after_creation_2,
        0,
        "Buffer 2 should not have been written at all"
    );
    assert_eq!(
        fs.write_count_for_path(path!("/dir/file3.rs")) - write_count_after_creation_3,
        0,
        "Buffer 3 should not have been written at all"
    );
}

#[gpui::test]
async fn test_format_selections_action_hidden_without_range_capable_formatter(
    cx: &mut TestAppContext,
) {
    init_test(cx, |settings| {
        settings.defaults.formatter = Some(FormatterList::Single(Formatter::External {
            command: "awk".into(),
            arguments: Some(vec!["{ print }".to_string()]),
        }));
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_file(path!("/file.rs"), Default::default()).await;

    let project = Project::test(fs, [path!("/").as_ref()], cx).await;
    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(rust_lang());

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/file.rs"), cx)
        })
        .await
        .unwrap();

    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| {
        build_editor_with_project(project.clone(), buffer, window, cx)
    });
    editor.update_in(cx, |editor, window, cx| {
        window.focus(&editor.focus_handle(cx), cx);
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.set_text("one\ntwo\nthree\n", window, cx);
        editor.change_selections(SelectionEffects::default(), window, cx, |s| {
            s.select_ranges([Point::new(0, 0)..Point::new(1, 0)]);
        });
    });

    refresh_editor_actions(cx);

    assert!(!cx.update(|window, cx| { window.is_action_available(&FormatSelections, cx) }));
}

#[gpui::test]
async fn test_markdown_save_respects_disabled_trailing_whitespace_removal(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.ensure_final_newline_on_save = Some(false);
        settings.languages.0.insert(
            "Markdown".into(),
            LanguageSettingsContent {
                remove_trailing_whitespace_on_save: Some(false),
                ..Default::default()
            },
        );
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_file(
        path!("/.editorconfig"),
        "root = true\n[*]\ntrim_trailing_whitespace = true\n".into(),
    )
    .await;
    fs.insert_file(path!("/README.md"), "".into()).await;

    let project = Project::test(fs.clone(), [path!("/").as_ref()], cx).await;
    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(markdown_lang());

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/README.md"), cx)
        })
        .await
        .unwrap();

    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| {
        build_editor_with_project(project.clone(), buffer, window, cx)
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.set_text("line with spaces  \n", window, cx)
    });

    let save = editor
        .update_in(cx, |editor, window, cx| {
            editor.save(
                SaveOptions {
                    format: true,
                    force_format: false,
                    autosave: false,
                },
                project.clone(),
                window,
                cx,
            )
        })
        .unwrap();
    save.await;

    assert_eq!(
        editor.update(cx, |editor, cx| editor.text(cx)),
        "line with spaces  \n"
    );
    assert_eq!(
        fs.load(path!("/README.md").as_ref())
            .await
            .unwrap()
            .replace("\r\n", "\n"),
        "line with spaces  \n"
    );
}

#[gpui::test]
async fn test_external_formatter_with_no_output_leaves_buffer_unchanged(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        #[cfg(windows)]
        let (command, arguments) = ("cmd", vec!["/C".to_string(), "more > nul".to_string()]);
        #[cfg(not(windows))]
        let (command, arguments) = ("sh", vec!["-c".to_string(), "cat >/dev/null".to_string()]);

        // Consume stdin without printing anything, mirroring tools like `cargo fmt`
        // that format files in place instead of writing to stdout.
        settings.defaults.formatter = Some(FormatterList::Single(Formatter::External {
            command: command.into(),
            arguments: Some(arguments),
        }));
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_file(path!("/file.rs"), "fn main() {}\n".into())
        .await;

    let project = Project::test(fs, [path!("/").as_ref()], cx).await;
    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(rust_lang());

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/file.rs"), cx)
        })
        .await
        .unwrap();

    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| {
        build_editor_with_project(project.clone(), buffer, window, cx)
    });

    // Formatting via an external command spawns a real subprocess, whose I/O
    // happens off the deterministic test scheduler.
    cx.executor().allow_parking();

    editor
        .update_in(cx, |editor, window, cx| {
            editor.perform_format(
                project.clone(),
                FormatTrigger::Manual,
                FormatTarget::Buffers(editor.buffer().read(cx).all_buffers()),
                window,
                cx,
            )
        })
        .unwrap()
        .await;

    editor.update(cx, |editor, cx| {
        assert_eq!(
            editor.text(cx),
            "fn main() {}\n",
            "buffer should be left unchanged when the external formatter produces no output"
        );
    });

    let last_failure = project.read_with(cx, |project, cx| {
        project.last_formatting_failure(cx).map(str::to_string)
    });
    assert_eq!(
        last_failure, None,
        "producing no output is not a formatter failure"
    );
}

// This used to crash
#[gpui::test]
async fn test_word_completions_usually_skip_digits(cx: &mut TestAppContext) {
    init_test(cx, |language_settings| {
        language_settings.defaults.completions = Some(CompletionSettingsContent {
            words_min_length: Some(0),
            ..Default::default()
        });
    });

    let mut cx = EditorLanguageTestContext::new_rust(cx).await;

    cx.set_state(indoc! {"ˇ
        0_usize
        let
        33
        4.5f32
    "});
    cx.update_editor(|editor, window, cx| {
        editor.show_completions(&ShowCompletions, window, cx);
    });
    cx.executor().run_until_parked();
    cx.condition(|editor, _| editor.context_menu_visible())
        .await;
    cx.update_editor(|editor, window, cx| {
        if let Some(CodeContextMenu::Completions(menu)) = editor.context_menu.borrow_mut().as_ref()
        {
            assert_eq!(
                completion_menu_entries(menu),
                &["let"],
                "With no digits in the completion query, no digits should be in the word completions"
            );
        } else {
            panic!("expected completion menu to be open");
        }
        editor.cancel(&Cancel, window, cx);
    });

    cx.set_state(indoc! {"3ˇ
        0_usize
        let
        3
        33.35f32
    "});
    cx.update_editor(|editor, window, cx| {
        editor.show_completions(&ShowCompletions, window, cx);
    });
    cx.executor().run_until_parked();
    cx.condition(|editor, _| editor.context_menu_visible())
        .await;
    cx.update_editor(|editor, _, _| {
        if let Some(CodeContextMenu::Completions(menu)) = editor.context_menu.borrow_mut().as_ref()
        {
            assert_eq!(completion_menu_entries(menu), &["33", "35f32"], "The digit is in the completion query, \
                return matching words with digits (`33`, `35f32`) but exclude query duplicates (`3`)");
        } else {
            panic!("expected completion menu to be open");
        }
    });
}

#[gpui::test]
async fn test_word_completions_do_not_show_before_threshold(cx: &mut TestAppContext) {
    init_test(cx, |language_settings| {
        language_settings.defaults.completions = Some(CompletionSettingsContent {
            words: Some(WordsCompletionMode::Enabled),
            words_min_length: Some(3),
            ..Default::default()
        });
    });

    let mut cx = EditorLanguageTestContext::new_rust(cx).await;
    cx.set_state(indoc! {"ˇ
        wow
        wowen
        wowser
    "});
    cx.simulate_keystroke("w");
    cx.executor().run_until_parked();
    cx.update_editor(|editor, _, _| {
        if editor.context_menu.borrow_mut().is_some() {
            panic!(
                "expected completion menu to be hidden, as words completion threshold is not met"
            );
        }
    });

    cx.update_editor(|editor, window, cx| {
        editor.show_word_completions(&ShowWordCompletions, window, cx);
    });
    cx.executor().run_until_parked();
    cx.update_editor(|editor, window, cx| {
        if let Some(CodeContextMenu::Completions(menu)) = editor.context_menu.borrow_mut().as_ref()
        {
            assert_eq!(completion_menu_entries(menu), &["wowser", "wowen", "wow"], "Even though the threshold is not met, invoking word completions with an action should provide the completions");
        } else {
            panic!("expected completion menu to be open after the word completions are called with an action");
        }

        editor.cancel(&Cancel, window, cx);
    });
    cx.update_editor(|editor, _, _| {
        if editor.context_menu.borrow_mut().is_some() {
            panic!("expected completion menu to be hidden after canceling");
        }
    });

    cx.simulate_keystroke("o");
    cx.executor().run_until_parked();
    cx.update_editor(|editor, _, _| {
        if editor.context_menu.borrow_mut().is_some() {
            panic!(
                "expected completion menu to be hidden, as words completion threshold is not met still"
            );
        }
    });

    cx.simulate_keystroke("w");
    cx.executor().run_until_parked();
    cx.update_editor(|editor, _, _| {
        if let Some(CodeContextMenu::Completions(menu)) = editor.context_menu.borrow_mut().as_ref()
        {
            assert_eq!(completion_menu_entries(menu), &["wowen", "wowser"], "After word completion threshold is met, matching words should be shown, excluding the already typed word");
        } else {
            panic!("expected completion menu to be open after the word completions threshold is met");
        }
    });
}

#[gpui::test]
async fn test_word_completions_disabled(cx: &mut TestAppContext) {
    init_test(cx, |language_settings| {
        language_settings.defaults.completions = Some(CompletionSettingsContent {
            words: Some(WordsCompletionMode::Enabled),
            words_min_length: Some(0),
            ..Default::default()
        });
    });

    let mut cx = EditorLanguageTestContext::new_rust(cx).await;
    cx.update_editor(|editor, _, _| {
        editor.disable_word_completions();
    });
    cx.set_state(indoc! {"ˇ
        wow
        wowen
        wowser
    "});
    cx.simulate_keystroke("w");
    cx.executor().run_until_parked();
    cx.update_editor(|editor, _, _| {
        if editor.context_menu.borrow_mut().is_some() {
            panic!(
                "expected completion menu to be hidden, as words completion are disabled for this editor"
            );
        }
    });

    cx.update_editor(|editor, window, cx| {
        editor.show_word_completions(&ShowWordCompletions, window, cx);
    });
    cx.executor().run_until_parked();
    cx.update_editor(|editor, _, _| {
        if editor.context_menu.borrow_mut().is_some() {
            panic!(
                "expected completion menu to be hidden even if called for explicitly, as words completion are disabled for this editor"
            );
        }
    });
}

#[gpui::test]
async fn test_word_completions_disabled_with_no_provider(cx: &mut TestAppContext) {
    init_test(cx, |language_settings| {
        language_settings.defaults.completions = Some(CompletionSettingsContent {
            words: Some(WordsCompletionMode::Disabled),
            words_min_length: Some(0),
            ..Default::default()
        });
    });

    let mut cx = EditorLanguageTestContext::new_rust(cx).await;
    cx.update_editor(|editor, _, _| {
        editor.set_completion_provider(None);
    });
    cx.set_state(indoc! {"ˇ
        wow
        wowen
        wowser
    "});
    cx.simulate_keystroke("w");
    cx.executor().run_until_parked();
    cx.update_editor(|editor, _, _| {
        if editor.context_menu.borrow_mut().is_some() {
            panic!("expected completion menu to be hidden, as disabled in settings");
        }
    });
}

#[gpui::test]
async fn test_toggle_comment(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;
    let language = Arc::new(Language::new(
        LanguageConfig {
            line_comments: vec!["// ".into(), "//! ".into(), "/// ".into()],
            ..Default::default()
        },
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    // If multiple selections intersect a line, the line is only toggled once.
    cx.set_state(indoc! {"
        fn a() {
            «//b();
            ˇ»// «c();
            //ˇ»  d();
        }
    "});

    cx.update_editor(|e, window, cx| e.toggle_comments(&ToggleComments::default(), window, cx));

    cx.assert_editor_state(indoc! {"
        fn a() {
            «b();
            ˇ»«c();
            ˇ» d();
        }
    "});

    // The comment prefix is inserted at the same column for every line in a
    // selection.
    cx.update_editor(|e, window, cx| e.toggle_comments(&ToggleComments::default(), window, cx));

    cx.assert_editor_state(indoc! {"
        fn a() {
            // «b();
            ˇ»// «c();
            ˇ» // d();
        }
    "});

    // If a selection ends at the beginning of a line, that line is not toggled.
    cx.set_selections_state(indoc! {"
        fn a() {
            // b();
            «// c();
        ˇ»     // d();
        }
    "});

    cx.update_editor(|e, window, cx| e.toggle_comments(&ToggleComments::default(), window, cx));

    cx.assert_editor_state(indoc! {"
        fn a() {
            // b();
            «c();
        ˇ»     // d();
        }
    "});

    // If a selection span a single line and is empty, the line is toggled.
    cx.set_state(indoc! {"
        fn a() {
            a();
            b();
        ˇ
        }
    "});

    cx.update_editor(|e, window, cx| e.toggle_comments(&ToggleComments::default(), window, cx));

    cx.assert_editor_state(indoc! {"
        fn a() {
            a();
            b();
        //•ˇ
        }
    "});

    // If a selection span multiple lines, empty lines are not toggled.
    cx.set_state(indoc! {"
        fn a() {
            «a();

            c();ˇ»
        }
    "});

    cx.update_editor(|e, window, cx| e.toggle_comments(&ToggleComments::default(), window, cx));

    cx.assert_editor_state(indoc! {"
        fn a() {
            // «a();

            // c();ˇ»
        }
    "});

    // If a selection includes multiple comment prefixes, all lines are uncommented.
    cx.set_state(indoc! {"
        fn a() {
            «// a();
            /// b();
            //! c();ˇ»
        }
    "});

    cx.update_editor(|e, window, cx| e.toggle_comments(&ToggleComments::default(), window, cx));

    cx.assert_editor_state(indoc! {"
        fn a() {
            «a();
            b();
            c();ˇ»
        }
    "});
}

#[gpui::test]
async fn test_toggle_comment_ignore_indent(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;
    let language = Arc::new(Language::new(
        LanguageConfig {
            line_comments: vec!["// ".into(), "//! ".into(), "/// ".into()],
            ..Default::default()
        },
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    let toggle_comments = &ToggleComments {
        advance_downwards: false,
        ignore_indent: true,
    };

    // If multiple selections intersect a line, the line is only toggled once.
    cx.set_state(indoc! {"
        fn a() {
        //    «b();
        //    c();
        //    ˇ» d();
        }
    "});

    cx.update_editor(|e, window, cx| e.toggle_comments(toggle_comments, window, cx));

    cx.assert_editor_state(indoc! {"
        fn a() {
            «b();
            c();
            ˇ» d();
        }
    "});

    // The comment prefix is inserted at the beginning of each line
    cx.update_editor(|e, window, cx| e.toggle_comments(toggle_comments, window, cx));

    cx.assert_editor_state(indoc! {"
        fn a() {
        //    «b();
        //    c();
        //    ˇ» d();
        }
    "});

    // If a selection ends at the beginning of a line, that line is not toggled.
    cx.set_selections_state(indoc! {"
        fn a() {
        //    b();
        //    «c();
        ˇ»//     d();
        }
    "});

    cx.update_editor(|e, window, cx| e.toggle_comments(toggle_comments, window, cx));

    cx.assert_editor_state(indoc! {"
        fn a() {
        //    b();
            «c();
        ˇ»//     d();
        }
    "});

    // If a selection span a single line and is empty, the line is toggled.
    cx.set_state(indoc! {"
        fn a() {
            a();
            b();
        ˇ
        }
    "});

    cx.update_editor(|e, window, cx| e.toggle_comments(toggle_comments, window, cx));

    cx.assert_editor_state(indoc! {"
        fn a() {
            a();
            b();
        //ˇ
        }
    "});

    // If a selection span multiple lines, empty lines are not toggled.
    cx.set_state(indoc! {"
        fn a() {
            «a();

            c();ˇ»
        }
    "});

    cx.update_editor(|e, window, cx| e.toggle_comments(toggle_comments, window, cx));

    cx.assert_editor_state(indoc! {"
        fn a() {
        //    «a();

        //    c();ˇ»
        }
    "});

    // If a selection includes multiple comment prefixes, all lines are uncommented.
    cx.set_state(indoc! {"
        fn a() {
        //    «a();
        ///    b();
        //!    c();ˇ»
        }
    "});

    cx.update_editor(|e, window, cx| e.toggle_comments(toggle_comments, window, cx));

    cx.assert_editor_state(indoc! {"
        fn a() {
            «a();
            b();
            c();ˇ»
        }
    "});
}

#[gpui::test]
async fn test_advance_downward_on_toggle_comment(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(Language::new(
        LanguageConfig {
            line_comments: vec!["// ".into()],
            ..Default::default()
        },
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    let mut cx = EditorTestContext::new(cx).await;

    cx.language_registry().add(language.clone());
    cx.update_buffer(|buffer, cx| {
        buffer.set_language(Some(language), cx);
    });

    let toggle_comments = &ToggleComments {
        advance_downwards: true,
        ignore_indent: false,
    };

    // Single cursor on one line -> advance
    // Cursor moves horizontally 3 characters as well on non-blank line
    cx.set_state(indoc!(
        "fn a() {
             ˇdog();
             cat();
        }"
    ));
    cx.update_editor(|editor, window, cx| {
        editor.toggle_comments(toggle_comments, window, cx);
    });
    cx.assert_editor_state(indoc!(
        "fn a() {
             // dog();
             catˇ();
        }"
    ));

    // Single selection on one line -> don't advance
    cx.set_state(indoc!(
        "fn a() {
             «dog()ˇ»;
             cat();
        }"
    ));
    cx.update_editor(|editor, window, cx| {
        editor.toggle_comments(toggle_comments, window, cx);
    });
    cx.assert_editor_state(indoc!(
        "fn a() {
             // «dog()ˇ»;
             cat();
        }"
    ));

    // Multiple cursors on one line -> advance
    cx.set_state(indoc!(
        "fn a() {
             ˇdˇog();
             cat();
        }"
    ));
    cx.update_editor(|editor, window, cx| {
        editor.toggle_comments(toggle_comments, window, cx);
    });
    cx.assert_editor_state(indoc!(
        "fn a() {
             // dog();
             catˇ(ˇ);
        }"
    ));

    // Multiple cursors on one line, with selection -> don't advance
    cx.set_state(indoc!(
        "fn a() {
             ˇdˇog«()ˇ»;
             cat();
        }"
    ));
    cx.update_editor(|editor, window, cx| {
        editor.toggle_comments(toggle_comments, window, cx);
    });
    cx.assert_editor_state(indoc!(
        "fn a() {
             // ˇdˇog«()ˇ»;
             cat();
        }"
    ));

    // Single cursor on one line -> advance
    // Cursor moves to column 0 on blank line
    cx.set_state(indoc!(
        "fn a() {
             ˇdog();

             cat();
        }"
    ));
    cx.update_editor(|editor, window, cx| {
        editor.toggle_comments(toggle_comments, window, cx);
    });
    cx.assert_editor_state(indoc!(
        "fn a() {
             // dog();
        ˇ
             cat();
        }"
    ));

    // Single cursor on one line -> advance
    // Cursor starts and ends at column 0
    cx.set_state(indoc!(
        "fn a() {
         ˇ    dog();
             cat();
        }"
    ));
    cx.update_editor(|editor, window, cx| {
        editor.toggle_comments(toggle_comments, window, cx);
    });
    cx.assert_editor_state(indoc!(
        "fn a() {
             // dog();
         ˇ    cat();
        }"
    ));
}

#[gpui::test]
async fn test_toggle_block_comment(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let html_language = Arc::new(
        Language::new(
            LanguageConfig {
                name: "HTML".into(),
                block_comment: Some(BlockCommentConfig {
                    start: "<!-- ".into(),
                    prefix: "".into(),
                    end: " -->".into(),
                    tab_size: 0,
                }),
                ..Default::default()
            },
            Some(tree_sitter_html::LANGUAGE.into()),
        )
        .with_injection_query(
            r#"
            (script_element
                (raw_text) @injection.content
                (#set! injection.language "javascript"))
            "#,
        )
        .unwrap(),
    );

    let javascript_language = Arc::new(Language::new(
        LanguageConfig {
            name: "JavaScript".into(),
            line_comments: vec!["// ".into()],
            ..Default::default()
        },
        Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
    ));

    cx.language_registry().add(html_language.clone());
    cx.language_registry().add(javascript_language);
    cx.update_buffer(|buffer, cx| {
        buffer.set_language(Some(html_language), cx);
    });

    // Toggle comments for empty selections
    cx.set_state(
        &r#"
            <p>A</p>ˇ
            <p>B</p>ˇ
            <p>C</p>ˇ
        "#
        .unindent(),
    );
    cx.update_editor(|editor, window, cx| {
        editor.toggle_comments(&ToggleComments::default(), window, cx)
    });
    cx.assert_editor_state(
        &r#"
            <!-- <p>A</p>ˇ -->
            <!-- <p>B</p>ˇ -->
            <!-- <p>C</p>ˇ -->
        "#
        .unindent(),
    );
    cx.update_editor(|editor, window, cx| {
        editor.toggle_comments(&ToggleComments::default(), window, cx)
    });
    cx.assert_editor_state(
        &r#"
            <p>A</p>ˇ
            <p>B</p>ˇ
            <p>C</p>ˇ
        "#
        .unindent(),
    );

    // Toggle comments for mixture of empty and non-empty selections, where
    // multiple selections occupy a given line.
    cx.set_state(
        &r#"
            <p>A«</p>
            <p>ˇ»B</p>ˇ
            <p>C«</p>
            <p>ˇ»D</p>ˇ
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.toggle_comments(&ToggleComments::default(), window, cx)
    });
    cx.assert_editor_state(
        &r#"
            <!-- <p>A«</p>
            <p>ˇ»B</p>ˇ -->
            <!-- <p>C«</p>
            <p>ˇ»D</p>ˇ -->
        "#
        .unindent(),
    );
    cx.update_editor(|editor, window, cx| {
        editor.toggle_comments(&ToggleComments::default(), window, cx)
    });
    cx.assert_editor_state(
        &r#"
            <p>A«</p>
            <p>ˇ»B</p>ˇ
            <p>C«</p>
            <p>ˇ»D</p>ˇ
        "#
        .unindent(),
    );

    // Toggle comments when different languages are active for different
    // selections.
    cx.set_state(
        &r#"
            ˇ<script>
                ˇvar x = new Y();
            ˇ</script>
        "#
        .unindent(),
    );
    cx.executor().run_until_parked();
    cx.update_editor(|editor, window, cx| {
        editor.toggle_comments(&ToggleComments::default(), window, cx)
    });
    // TODO this is how it actually worked in Zed Stable, which is not very ergonomic.
    // Uncommenting and commenting from this position brings in even more wrong artifacts.
    cx.assert_editor_state(
        &r#"
            <!-- ˇ<script> -->
                // ˇvar x = new Y();
            <!-- ˇ</script> -->
        "#
        .unindent(),
    );
}

#[gpui::test]
fn test_editing_disjoint_excerpts(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let buffer = cx.new(|cx| Buffer::local(sample_text(6, 4, 'a'), cx));
    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer.clone(),
            [
                Point::new(0, 0)..Point::new(0, 4),
                Point::new(5, 0)..Point::new(5, 4),
            ],
            0,
            cx,
        );
        assert_eq!(multibuffer.read(cx).text(), "aaaa\nffff");
        multibuffer
    });

    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(multibuffer, window, cx));
    editor.update_in(cx, |editor, window, cx| {
        assert_eq!(editor.text(cx), "aaaa\nffff");
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([
                Point::new(0, 0)..Point::new(0, 0),
                Point::new(1, 0)..Point::new(1, 0),
            ])
        });

        editor.handle_input("X", window, cx);
        assert_eq!(editor.text(cx), "Xaaaa\nXffff");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [
                Point::new(0, 1)..Point::new(0, 1),
                Point::new(1, 1)..Point::new(1, 1),
            ]
        );

        // Ensure the cursor's head is respected when deleting across an excerpt boundary.
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(0, 2)..Point::new(1, 2)])
        });
        editor.backspace(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "Xa\nfff");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [Point::new(1, 0)..Point::new(1, 0)]
        );

        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(1, 1)..Point::new(0, 1)])
        });
        editor.backspace(&Default::default(), window, cx);
        assert_eq!(editor.text(cx), "X\nff");
        assert_eq!(
            editor.selections.ranges(&editor.display_snapshot(cx)),
            [Point::new(0, 1)..Point::new(0, 1)]
        );
    });
}

#[gpui::test]
fn test_header_jump_data_uses_selection_excerpt(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    // 25-line buffer so excerpts at rows 1, 10, and 20 (each a 1-line range,
    // expanded by 2 context lines) can't merge into a single excerpt.
    let buffer_text = (0..25)
        .map(|row| format!("line {row}"))
        .collect::<Vec<_>>()
        .join("\n");
    let buffer = cx.new(|cx| Buffer::local(buffer_text, cx));
    let buffer_id = buffer.read_with(cx, |buffer, _| buffer.remote_id());

    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer.clone(),
            [
                Point::new(1, 0)..Point::new(1, 0),
                Point::new(10, 0)..Point::new(10, 0),
                Point::new(20, 0)..Point::new(20, 0),
            ],
            2,
            cx,
        );
        multibuffer
    });

    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(multibuffer, window, cx));

    editor.update_in(cx, |editor, window, cx| {
        let snapshot = editor.snapshot(window, cx);
        let display_snapshot = editor.display_snapshot(cx);

        // Ensure the three ranges landed in three separate excerpts.
        let excerpts: Vec<_> = snapshot
            .buffer_snapshot()
            .excerpts_for_buffer(buffer_id)
            .collect();
        assert_eq!(excerpts.len(), 3);

        // Place the cursor at the start of the third excerpt, expressed in
        // terms of the underlying buffer.
        let selection_buffer_row = 20;
        let buffer_entity = editor.buffer().read(cx).buffer(buffer_id).unwrap();
        let selection_anchor = editor.buffer().update(cx, |multibuffer, cx| {
            multibuffer
                .buffer_point_to_anchor(&buffer_entity, Point::new(selection_buffer_row, 0), cx)
                .expect("buffer row 20 maps to a multibuffer anchor")
        });
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_anchor_ranges([selection_anchor..selection_anchor])
        });

        let mut latest_selection_anchors: HashMap<BufferId, Anchor> = HashMap::default();
        for selection in editor.selections.all_anchors(&display_snapshot).iter() {
            let head = selection.head();
            if let Some((text_anchor, _)) = snapshot.buffer_snapshot().anchor_to_buffer_anchor(head)
            {
                latest_selection_anchors.insert(text_anchor.buffer_id, head);
            }
        }

        // The sticky buffer header represents the FIRST excerpt of its buffer,
        // even when the cursor is in a later excerpt. That mismatch is the
        // precondition for the regression.
        let first_excerpt = snapshot
            .buffer_snapshot()
            .excerpt_boundaries_in_range(MultiBufferOffset(0)..snapshot.buffer_snapshot().len())
            .next()
            .expect("multibuffer has at least one excerpt")
            .next;

        let jump_data = header_jump_data(
            &snapshot,
            DisplayRow(0),
            FILE_HEADER_HEIGHT + MULTI_BUFFER_EXCERPT_HEADER_HEIGHT,
            &first_excerpt,
            &latest_selection_anchors,
        );

        match jump_data {
            JumpData::MultiBufferPoint {
                position,
                line_offset_from_top,
                ..
            } => {
                assert_eq!(
                    position.row, selection_buffer_row,
                    "jump should target the cursor's buffer row, not the first excerpt's row"
                );
                assert!(
                    line_offset_from_top < selection_buffer_row,
                    "line_offset_from_top ({line_offset_from_top}) should be measured from the \
                     selection's excerpt, not the first excerpt; expected less than \
                     selection_buffer_row ({selection_buffer_row})"
                );
            }
            JumpData::MultiBufferRow { .. } => {
                panic!("expected MultiBufferPoint jump data when a selection is present")
            }
        }
    });
}

#[gpui::test]
async fn test_extra_newline_insertion(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(
        Language::new(
            LanguageConfig {
                brackets: BracketPairConfig {
                    pairs: vec![
                        BracketPair {
                            start: "{".to_string(),
                            end: "}".to_string(),
                            close: true,
                            surround: true,
                            newline: true,
                        },
                        BracketPair {
                            start: "/* ".to_string(),
                            end: " */".to_string(),
                            close: true,
                            surround: true,
                            newline: true,
                        },
                    ],
                    ..Default::default()
                },
                ..Default::default()
            },
            Some(tree_sitter_rust::LANGUAGE.into()),
        )
        .with_indents_query("")
        .unwrap(),
    );

    let text = concat!(
        "{   }\n",     //
        "  x\n",       //
        "  /*   */\n", //
        "x\n",         //
        "{{} }\n",     //
    );

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));
    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 2)..DisplayPoint::new(DisplayRow(0), 3),
                DisplayPoint::new(DisplayRow(2), 5)..DisplayPoint::new(DisplayRow(2), 5),
                DisplayPoint::new(DisplayRow(4), 4)..DisplayPoint::new(DisplayRow(4), 4),
            ])
        });
        editor.newline(&Newline, window, cx);

        assert_eq!(
            editor.buffer().read(cx).read(cx).text(),
            concat!(
                "{ \n",    // Suppress rustfmt
                "\n",      //
                "}\n",     //
                "  x\n",   //
                "  /* \n", //
                "  \n",    //
                "  */\n",  //
                "x\n",     //
                "{{} \n",  //
                "}\n",     //
            )
        );
    });
}

#[gpui::test]
fn test_highlighted_ranges(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple(&sample_text(16, 8, 'a'), cx);
        build_editor(buffer, window, cx)
    });

    _ = editor.update(cx, |editor, window, cx| {
        let buffer = editor.buffer.read(cx).snapshot(cx);

        let anchor_range =
            |range: Range<Point>| buffer.anchor_after(range.start)..buffer.anchor_after(range.end);

        editor.highlight_background(
            HighlightKey::ColorizeBracket(0),
            &[
                anchor_range(Point::new(2, 1)..Point::new(2, 3)),
                anchor_range(Point::new(4, 2)..Point::new(4, 4)),
                anchor_range(Point::new(6, 3)..Point::new(6, 5)),
                anchor_range(Point::new(8, 4)..Point::new(8, 6)),
            ],
            |_, _| Hsla::red(),
            cx,
        );
        editor.highlight_background(
            HighlightKey::ColorizeBracket(1),
            &[
                anchor_range(Point::new(3, 2)..Point::new(3, 5)),
                anchor_range(Point::new(5, 3)..Point::new(5, 6)),
                anchor_range(Point::new(7, 4)..Point::new(7, 7)),
                anchor_range(Point::new(9, 5)..Point::new(9, 8)),
            ],
            |_, _| Hsla::green(),
            cx,
        );

        let snapshot = editor.snapshot(window, cx);
        let highlighted_ranges = editor.sorted_background_highlights_in_range(
            anchor_range(Point::new(3, 4)..Point::new(7, 4)),
            &snapshot,
            cx.theme(),
        );
        assert_eq!(
            highlighted_ranges,
            &[
                (
                    DisplayPoint::new(DisplayRow(3), 2)..DisplayPoint::new(DisplayRow(3), 5),
                    Hsla::green(),
                ),
                (
                    DisplayPoint::new(DisplayRow(4), 2)..DisplayPoint::new(DisplayRow(4), 4),
                    Hsla::red(),
                ),
                (
                    DisplayPoint::new(DisplayRow(5), 3)..DisplayPoint::new(DisplayRow(5), 6),
                    Hsla::green(),
                ),
                (
                    DisplayPoint::new(DisplayRow(6), 3)..DisplayPoint::new(DisplayRow(6), 5),
                    Hsla::red(),
                ),
            ]
        );
        assert_eq!(
            editor.sorted_background_highlights_in_range(
                anchor_range(Point::new(5, 6)..Point::new(6, 4)),
                &snapshot,
                cx.theme(),
            ),
            &[(
                DisplayPoint::new(DisplayRow(6), 3)..DisplayPoint::new(DisplayRow(6), 5),
                Hsla::red(),
            )]
        );
    });
}

#[gpui::test]
async fn test_copy_highlight_json(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(indoc! {"
        fn main() {
            let x = 1;ˇ
        }
    "});
    setup_syntax_highlighting(rust_lang(), &mut cx);

    cx.update_editor(|editor, window, cx| {
        editor.copy_highlight_json(&CopyHighlightJson, window, cx);
    });

    let clipboard_json: serde_json::Value =
        serde_json::from_str(&cx.read_from_clipboard().unwrap().text().unwrap()).unwrap();
    assert_eq!(
        clipboard_json,
        json!([
            [
                {"text": "fn", "highlight": "keyword"},
                {"text": " ", "highlight": null},
                {"text": "main", "highlight": "function"},
                {"text": "()", "highlight": "punctuation.bracket"},
                {"text": " ", "highlight": null},
                {"text": "{", "highlight": "punctuation.bracket"},
            ],
            [
                {"text": "    ", "highlight": null},
                {"text": "let", "highlight": "keyword"},
                {"text": " ", "highlight": null},
                {"text": "x", "highlight": "variable"},
                {"text": " ", "highlight": null},
                {"text": "=", "highlight": "operator"},
                {"text": " ", "highlight": null},
                {"text": "1", "highlight": "number"},
                {"text": ";", "highlight": "punctuation.delimiter"},
            ],
            [
                {"text": "}", "highlight": "punctuation.bracket"},
            ],
        ])
    );
}

#[gpui::test]
async fn test_copy_highlight_json_selected_range(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(indoc! {"
        fn main() {
            «let x = 1;
            let yˇ» = 2;
        }
    "});
    setup_syntax_highlighting(rust_lang(), &mut cx);

    cx.update_editor(|editor, window, cx| {
        editor.copy_highlight_json(&CopyHighlightJson, window, cx);
    });

    let clipboard_json: serde_json::Value =
        serde_json::from_str(&cx.read_from_clipboard().unwrap().text().unwrap()).unwrap();
    assert_eq!(
        clipboard_json,
        json!([
            [
                {"text": "let", "highlight": "keyword"},
                {"text": " ", "highlight": null},
                {"text": "x", "highlight": "variable"},
                {"text": " ", "highlight": null},
                {"text": "=", "highlight": "operator"},
                {"text": " ", "highlight": null},
                {"text": "1", "highlight": "number"},
                {"text": ";", "highlight": "punctuation.delimiter"},
            ],
            [
                {"text": "    ", "highlight": null},
                {"text": "let", "highlight": "keyword"},
                {"text": " ", "highlight": null},
                {"text": "y", "highlight": "variable"},
            ],
        ])
    );
}

#[gpui::test]
async fn test_copy_highlight_json_selected_line_range(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"
        fn main() {
            «let x = 1;
            let yˇ» = 2;
        }
    "});
    setup_syntax_highlighting(rust_lang(), &mut cx);

    cx.update_editor(|editor, window, cx| {
        editor.selections.set_line_mode(true);
        editor.copy_highlight_json(&CopyHighlightJson, window, cx);
    });

    let clipboard_json: serde_json::Value =
        serde_json::from_str(&cx.read_from_clipboard().unwrap().text().unwrap()).unwrap();
    assert_eq!(
        clipboard_json,
        json!([
            [
                {"text": "    ", "highlight": null},
                {"text": "let", "highlight": "keyword"},
                {"text": " ", "highlight": null},
                {"text": "x", "highlight": "variable"},
                {"text": " ", "highlight": null},
                {"text": "=", "highlight": "operator"},
                {"text": " ", "highlight": null},
                {"text": "1", "highlight": "number"},
                {"text": ";", "highlight": "punctuation.delimiter"},
            ],
            [
                {"text": "    ", "highlight": null},
                {"text": "let", "highlight": "keyword"},
                {"text": " ", "highlight": null},
                {"text": "y", "highlight": "variable"},
                {"text": " ", "highlight": null},
                {"text": "=", "highlight": "operator"},
                {"text": " ", "highlight": null},
                {"text": "2", "highlight": "number"},
                {"text": ";", "highlight": "punctuation.delimiter"},
            ],
        ])
    );
}

#[gpui::test]
async fn test_copy_highlight_json_single_line(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"
        fn main() {
            let ˇx = 1;
            let y = 2;
        }
    "});
    setup_syntax_highlighting(rust_lang(), &mut cx);

    cx.update_editor(|editor, window, cx| {
        editor.selections.set_line_mode(true);
        editor.copy_highlight_json(&CopyHighlightJson, window, cx);
    });

    let clipboard_json: serde_json::Value =
        serde_json::from_str(&cx.read_from_clipboard().unwrap().text().unwrap()).unwrap();
    assert_eq!(
        clipboard_json,
        json!([
            [
                {"text": "    ", "highlight": null},
                {"text": "let", "highlight": "keyword"},
                {"text": " ", "highlight": null},
                {"text": "x", "highlight": "variable"},
                {"text": " ", "highlight": null},
                {"text": "=", "highlight": "operator"},
                {"text": " ", "highlight": null},
                {"text": "1", "highlight": "number"},
                {"text": ";", "highlight": "punctuation.delimiter"},
            ]
        ])
    );
}

#[gpui::test]
async fn test_following(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, ["/file.rs".as_ref()], cx).await;

    let buffer = project.update(cx, |project, cx| {
        let buffer = project.create_local_buffer(&sample_text(16, 8, 'a'), None, false, cx);
        cx.new(|cx| MultiBuffer::singleton(buffer, cx))
    });
    let leader = cx.add_window(|window, cx| build_editor(buffer.clone(), window, cx));
    let follower = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::from_corners(
                    gpui::Point::new(px(0.), px(0.)),
                    gpui::Point::new(px(10.), px(80.)),
                ))),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| build_editor(buffer.clone(), window, cx)),
        )
        .unwrap()
    });

    let is_still_following = Rc::new(RefCell::new(true));
    let follower_edit_event_count = Rc::new(RefCell::new(0));
    let pending_update = Rc::new(RefCell::new(None));
    let leader_entity = leader.root(cx).unwrap();
    let follower_entity = follower.root(cx).unwrap();
    _ = follower.update(cx, {
        let update = pending_update.clone();
        let is_still_following = is_still_following.clone();
        let follower_edit_event_count = follower_edit_event_count.clone();
        |_, window, cx| {
            cx.subscribe_in(
                &leader_entity,
                window,
                move |_, leader, event, window, cx| {
                    leader.update(cx, |leader, cx| {
                        leader.add_event_to_update_proto(
                            event,
                            &mut update.borrow_mut(),
                            window,
                            cx,
                        );
                    });
                },
            )
            .detach();

            cx.subscribe_in(
                &follower_entity,
                window,
                move |_, _, event: &EditorEvent, _window, _cx| {
                    if matches!(Editor::to_follow_event(event), Some(FollowEvent::Unfollow)) {
                        *is_still_following.borrow_mut() = false;
                    }

                    if let EditorEvent::BufferEdited = event {
                        *follower_edit_event_count.borrow_mut() += 1;
                    }
                },
            )
            .detach();
        }
    });

    // Update the selections only
    _ = leader.update(cx, |leader, window, cx| {
        leader.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(1)..MultiBufferOffset(1)])
        });
    });
    follower
        .update(cx, |follower, window, cx| {
            follower.apply_update_proto(
                &project,
                pending_update.borrow_mut().take().unwrap(),
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .unwrap();
    _ = follower.update(cx, |follower, _, cx| {
        assert_eq!(
            follower.selections.ranges(&follower.display_snapshot(cx)),
            vec![MultiBufferOffset(1)..MultiBufferOffset(1)]
        );
    });
    assert!(*is_still_following.borrow());
    assert_eq!(*follower_edit_event_count.borrow(), 0);

    // Update the scroll position only
    _ = leader.update(cx, |leader, window, cx| {
        leader.set_scroll_position(gpui::Point::new(1.5, 3.5), window, cx);
    });
    follower
        .update(cx, |follower, window, cx| {
            follower.apply_update_proto(
                &project,
                pending_update.borrow_mut().take().unwrap(),
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .unwrap();
    assert_eq!(
        follower
            .update(cx, |follower, _, cx| follower.scroll_position(cx))
            .unwrap(),
        gpui::Point::new(1.5, 3.5)
    );
    assert!(*is_still_following.borrow());
    assert_eq!(*follower_edit_event_count.borrow(), 0);

    // Update the selections and scroll position. The follower's scroll position is updated
    // via autoscroll, not via the leader's exact scroll position.
    _ = leader.update(cx, |leader, window, cx| {
        leader.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(0)..MultiBufferOffset(0)])
        });
        leader.request_autoscroll(Autoscroll::newest(), cx);
        leader.set_scroll_position(gpui::Point::new(1.5, 3.5), window, cx);
    });
    follower
        .update(cx, |follower, window, cx| {
            follower.apply_update_proto(
                &project,
                pending_update.borrow_mut().take().unwrap(),
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .unwrap();
    _ = follower.update(cx, |follower, _, cx| {
        assert_eq!(follower.scroll_position(cx), gpui::Point::new(1.5, 0.0));
        assert_eq!(
            follower.selections.ranges(&follower.display_snapshot(cx)),
            vec![MultiBufferOffset(0)..MultiBufferOffset(0)]
        );
    });
    assert!(*is_still_following.borrow());

    // Creating a pending selection that precedes another selection
    _ = leader.update(cx, |leader, window, cx| {
        leader.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(1)..MultiBufferOffset(1)])
        });
        leader.begin_selection(DisplayPoint::new(DisplayRow(0), 0), true, 1, window, cx);
    });
    follower
        .update(cx, |follower, window, cx| {
            follower.apply_update_proto(
                &project,
                pending_update.borrow_mut().take().unwrap(),
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .unwrap();
    _ = follower.update(cx, |follower, _, cx| {
        assert_eq!(
            follower.selections.ranges(&follower.display_snapshot(cx)),
            vec![
                MultiBufferOffset(0)..MultiBufferOffset(0),
                MultiBufferOffset(1)..MultiBufferOffset(1)
            ]
        );
    });
    assert!(*is_still_following.borrow());

    // Extend the pending selection so that it surrounds another selection
    _ = leader.update(cx, |leader, window, cx| {
        leader.extend_selection(DisplayPoint::new(DisplayRow(0), 2), 1, window, cx);
    });
    follower
        .update(cx, |follower, window, cx| {
            follower.apply_update_proto(
                &project,
                pending_update.borrow_mut().take().unwrap(),
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .unwrap();
    _ = follower.update(cx, |follower, _, cx| {
        assert_eq!(
            follower.selections.ranges(&follower.display_snapshot(cx)),
            vec![MultiBufferOffset(0)..MultiBufferOffset(2)]
        );
    });

    // Scrolling locally breaks the follow
    _ = follower.update(cx, |follower, window, cx| {
        let top_anchor = follower
            .buffer()
            .read(cx)
            .read(cx)
            .anchor_after(MultiBufferOffset(0));
        follower.set_scroll_anchor(
            ScrollAnchor {
                anchor: top_anchor,
                offset: gpui::Point::new(0.0, 0.5),
            },
            window,
            cx,
        );
    });
    assert!(!(*is_still_following.borrow()));
}

#[gpui::test]
async fn test_following_with_multiple_excerpts(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, ["/file.rs".as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let pane = workspace.read_with(cx, |workspace, _| workspace.active_pane().clone());

    let cx = &mut VisualTestContext::from_window(*window, cx);

    let leader = pane.update_in(cx, |_, window, cx| {
        let multibuffer = cx.new(|_| MultiBuffer::new(ReadWrite));
        cx.new(|cx| build_editor(multibuffer.clone(), window, cx))
    });

    // Start following the editor when it has no excerpts.
    let mut state_message =
        leader.update_in(cx, |leader, window, cx| leader.to_state_proto(window, cx));
    let workspace_entity = workspace.clone();
    let follower_1 = cx
        .update_window(*window, |_, window, cx| {
            Editor::from_state_proto(
                workspace_entity,
                ViewId {
                    creator: CollaboratorId::PeerId(PeerId::default()),
                    id: 0,
                },
                &mut state_message,
                window,
                cx,
            )
        })
        .unwrap()
        .unwrap()
        .await
        .unwrap();

    let update_message = Rc::new(RefCell::new(None));
    follower_1.update_in(cx, {
        let update = update_message.clone();
        |_, window, cx| {
            cx.subscribe_in(&leader, window, move |_, leader, event, window, cx| {
                leader.update(cx, |leader, cx| {
                    leader.add_event_to_update_proto(event, &mut update.borrow_mut(), window, cx);
                });
            })
            .detach();
        }
    });

    let (buffer_1, buffer_2) = project.update(cx, |project, cx| {
        (
            project.create_local_buffer("abc\ndef\nghi\njkl\nmno\npqr\nstu\nvwx\nyza\nbcd\nefg\nhij\nklm\nnop\nqrs\ntuv\nwxy\nzab\ncde\nfgh\n", None, false, cx),
            project.create_local_buffer("aaa\nbbb\nccc\nddd\neee\nfff\nggg\nhhh\niii\njjj\nkkk\nlll\nmmm\nnnn\nooo\nppp\nqqq\nrrr\nsss\nttt\n", None, false, cx),
        )
    });

    // Insert some excerpts.
    leader.update(cx, |leader, cx| {
        leader.buffer.update(cx, |multibuffer, cx| {
            multibuffer.set_excerpts_for_path(
                PathKey::with_sort_prefix(1, rel_path("b.txt").into_arc()),
                buffer_1.clone(),
                vec![
                    Point::row_range(0..3),
                    Point::row_range(1..6),
                    Point::row_range(12..15),
                ],
                0,
                cx,
            );
            multibuffer.set_excerpts_for_path(
                PathKey::with_sort_prefix(1, rel_path("a.txt").into_arc()),
                buffer_2.clone(),
                vec![Point::row_range(0..6), Point::row_range(8..12)],
                0,
                cx,
            );
        });
    });

    // Apply the update of adding the excerpts.
    follower_1
        .update_in(cx, |follower, window, cx| {
            follower.apply_update_proto(
                &project,
                update_message.borrow().clone().unwrap(),
                window,
                cx,
            )
        })
        .await
        .unwrap();
    assert_eq!(
        follower_1.update(cx, |editor, cx| editor.text(cx)),
        leader.update(cx, |editor, cx| editor.text(cx))
    );
    update_message.borrow_mut().take();

    // Start following separately after it already has excerpts.
    let mut state_message =
        leader.update_in(cx, |leader, window, cx| leader.to_state_proto(window, cx));
    let workspace_entity = workspace.clone();
    let follower_2 = cx
        .update_window(*window, |_, window, cx| {
            Editor::from_state_proto(
                workspace_entity,
                ViewId {
                    creator: CollaboratorId::PeerId(PeerId::default()),
                    id: 0,
                },
                &mut state_message,
                window,
                cx,
            )
        })
        .unwrap()
        .unwrap()
        .await
        .unwrap();
    assert_eq!(
        follower_2.update(cx, |editor, cx| editor.text(cx)),
        leader.update(cx, |editor, cx| editor.text(cx))
    );

    // Remove some excerpts.
    leader.update(cx, |leader, cx| {
        leader.buffer.update(cx, |multibuffer, cx| {
            multibuffer.remove_excerpts(
                PathKey::with_sort_prefix(1, rel_path("b.txt").into_arc()),
                cx,
            );
        });
    });

    // Apply the update of removing the excerpts.
    follower_1
        .update_in(cx, |follower, window, cx| {
            follower.apply_update_proto(
                &project,
                update_message.borrow().clone().unwrap(),
                window,
                cx,
            )
        })
        .await
        .unwrap();
    follower_2
        .update_in(cx, |follower, window, cx| {
            follower.apply_update_proto(
                &project,
                update_message.borrow().clone().unwrap(),
                window,
                cx,
            )
        })
        .await
        .unwrap();
    update_message.borrow_mut().take();
    assert_eq!(
        follower_1.update(cx, |editor, cx| editor.text(cx)),
        leader.update(cx, |editor, cx| editor.text(cx))
    );
}

#[gpui::test]
async fn test_go_to_hunk(executor: BackgroundExecutor, cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let diff_base = r#"
        use some::mod;

        const A: u32 = 42;

        fn main() {
            println!("hello");

            println!("world");
        }
        "#
    .unindent();

    // Edits are modified, removed, modified, added
    cx.set_state(
        &r#"
        use some::modified;

        ˇ
        fn main() {
            println!("hello there");

            println!("around the");
            println!("world");
        }
        "#
        .unindent(),
    );

    cx.set_head_text(&diff_base);
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        //Wrap around the bottom of the buffer
        for _ in 0..3 {
            editor.go_to_next_hunk(&GoToHunk, window, cx);
        }
    });

    cx.assert_editor_state(
        &r#"
        ˇuse some::modified;


        fn main() {
            println!("hello there");

            println!("around the");
            println!("world");
        }
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        //Wrap around the top of the buffer
        for _ in 0..2 {
            editor.go_to_prev_hunk(&GoToPreviousHunk, window, cx);
        }
    });

    cx.assert_editor_state(
        &r#"
        use some::modified;


        fn main() {
        ˇ    println!("hello there");

            println!("around the");
            println!("world");
        }
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.go_to_prev_hunk(&GoToPreviousHunk, window, cx);
    });

    cx.assert_editor_state(
        &r#"
        use some::modified;

        ˇ
        fn main() {
            println!("hello there");

            println!("around the");
            println!("world");
        }
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.go_to_prev_hunk(&GoToPreviousHunk, window, cx);
    });

    cx.assert_editor_state(
        &r#"
        ˇuse some::modified;


        fn main() {
            println!("hello there");

            println!("around the");
            println!("world");
        }
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        for _ in 0..2 {
            editor.go_to_prev_hunk(&GoToPreviousHunk, window, cx);
        }
    });

    cx.assert_editor_state(
        &r#"
        use some::modified;


        fn main() {
        ˇ    println!("hello there");

            println!("around the");
            println!("world");
        }
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.fold(&Fold, window, cx);
    });

    cx.update_editor(|editor, window, cx| {
        editor.go_to_next_hunk(&GoToHunk, window, cx);
    });

    cx.assert_editor_state(
        &r#"
        ˇuse some::modified;


        fn main() {
            println!("hello there");

            println!("around the");
            println!("world");
        }
        "#
        .unindent(),
    );
}

#[test]
fn test_split_words() {
    fn split(text: &str) -> Vec<&str> {
        split_words(text).collect()
    }

    assert_eq!(split("HelloWorld"), &["Hello", "World"]);
    assert_eq!(split("hello_world"), &["hello_", "world"]);
    assert_eq!(split("_hello_world_"), &["_", "hello_", "world_"]);
    assert_eq!(split("Hello_World"), &["Hello_", "World"]);
    assert_eq!(split("helloWOrld"), &["hello", "WOrld"]);
    assert_eq!(split("helloworld"), &["helloworld"]);

    assert_eq!(split(":do_the_thing"), &[":", "do_", "the_", "thing"]);
}

#[test]
fn test_split_words_for_snippet_prefix() {
    fn split(text: &str) -> Vec<&str> {
        snippet_candidate_suffixes(text, &|c| c.is_alphanumeric() || c == '_').collect()
    }

    assert_eq!(split("HelloWorld"), &["HelloWorld"]);
    assert_eq!(split("hello_world"), &["hello_world"]);
    assert_eq!(split("_hello_world_"), &["_hello_world_"]);
    assert_eq!(split("Hello_World"), &["Hello_World"]);
    assert_eq!(split("helloWOrld"), &["helloWOrld"]);
    assert_eq!(split("helloworld"), &["helloworld"]);
    assert_eq!(
        split("this@is!@#$^many   . symbols"),
        &[
            "symbols",
            " symbols",
            ". symbols",
            " . symbols",
            "  . symbols",
            "   . symbols",
            "many   . symbols",
            "^many   . symbols",
            "$^many   . symbols",
            "#$^many   . symbols",
            "@#$^many   . symbols",
            "!@#$^many   . symbols",
            "is!@#$^many   . symbols",
            "@is!@#$^many   . symbols",
            "this@is!@#$^many   . symbols",
        ],
    );
    assert_eq!(split("a.s"), &["s", ".s", "a.s"]);
}

#[gpui::test]
async fn test_move_to_syntax_node_relative_jumps(tcx: &mut TestAppContext) {
    init_test(tcx, |_| {});

    let mut cx =
        EditorLanguageTestContext::new(Arc::into_inner(markdown_lang()).unwrap(), tcx).await;

    async fn assert(offset: i8, before: &str, after: &str, cx: &mut EditorLanguageTestContext) {
        let _state_context = cx.set_state(before);
        cx.run_until_parked();
        cx.update_editor(|editor, window, cx| editor.go_to_symbol_by_offset(window, cx, offset))
            .await
            .unwrap();
        cx.run_until_parked();
        cx.assert_editor_state(after);
    }

    const ABOVE: i8 = -1;
    const BELOW: i8 = 1;

    assert(
        ABOVE,
        indoc! {"
        # Foo

        ˇFoo foo foo

        # Bar

        Bar bar bar
    "},
        indoc! {"
        ˇ# Foo

        Foo foo foo

        # Bar

        Bar bar bar
    "},
        &mut cx,
    )
    .await;

    assert(
        ABOVE,
        indoc! {"
        ˇ# Foo

        Foo foo foo

        # Bar

        Bar bar bar
    "},
        indoc! {"
        ˇ# Foo

        Foo foo foo

        # Bar

        Bar bar bar
    "},
        &mut cx,
    )
    .await;

    assert(
        BELOW,
        indoc! {"
        ˇ# Foo

        Foo foo foo

        # Bar

        Bar bar bar
    "},
        indoc! {"
        # Foo

        Foo foo foo

        ˇ# Bar

        Bar bar bar
    "},
        &mut cx,
    )
    .await;

    assert(
        BELOW,
        indoc! {"
        # Foo

        ˇFoo foo foo

        # Bar

        Bar bar bar
    "},
        indoc! {"
        # Foo

        Foo foo foo

        ˇ# Bar

        Bar bar bar
    "},
        &mut cx,
    )
    .await;

    assert(
        BELOW,
        indoc! {"
        # Foo

        Foo foo foo

        ˇ# Bar

        Bar bar bar
    "},
        indoc! {"
        # Foo

        Foo foo foo

        ˇ# Bar

        Bar bar bar
    "},
        &mut cx,
    )
    .await;

    assert(
        BELOW,
        indoc! {"
        # Foo

        Foo foo foo

        # Bar
        ˇ
        Bar bar bar
    "},
        indoc! {"
        # Foo

        Foo foo foo

        # Bar
        ˇ
        Bar bar bar
    "},
        &mut cx,
    )
    .await;
}

#[gpui::test]
async fn test_move_to_syntax_node_relative_dead_zone(tcx: &mut TestAppContext) {
    init_test(tcx, |_| {});

    let mut cx = EditorLanguageTestContext::new(Arc::into_inner(rust_lang()).unwrap(), tcx).await;

    async fn assert(offset: i8, before: &str, after: &str, cx: &mut EditorLanguageTestContext) {
        let _state_context = cx.set_state(before);
        cx.run_until_parked();
        cx.update_editor(|editor, window, cx| editor.go_to_symbol_by_offset(window, cx, offset))
            .await
            .unwrap();
        cx.run_until_parked();
        cx.assert_editor_state(after);
    }

    const ABOVE: i8 = -1;
    const BELOW: i8 = 1;

    assert(
        ABOVE,
        indoc! {"
        fn foo() {
            // foo fn
        }

        ˇ// this zone is not inside any top level outline node

        fn bar() {
            // bar fn
            let _ = 2;
        }
    "},
        indoc! {"
        ˇfn foo() {
            // foo fn
        }

        // this zone is not inside any top level outline node

        fn bar() {
            // bar fn
            let _ = 2;
        }
    "},
        &mut cx,
    )
    .await;

    assert(
        BELOW,
        indoc! {"
        fn foo() {
            // foo fn
        }

        ˇ// this zone is not inside any top level outline node

        fn bar() {
            // bar fn
            let _ = 2;
        }
    "},
        indoc! {"
        fn foo() {
            // foo fn
        }

        // this zone is not inside any top level outline node

        ˇfn bar() {
            // bar fn
            let _ = 2;
        }
    "},
        &mut cx,
    )
    .await;
}

#[gpui::test]
async fn test_move_to_enclosing_bracket(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorLanguageTestContext::new_typescript(cx).await;

    #[track_caller]
    fn assert(before: &str, after: &str, cx: &mut EditorLanguageTestContext) {
        let _state_context = cx.set_state(before);
        cx.run_until_parked();
        cx.update_editor(|editor, window, cx| {
            editor.move_to_enclosing_bracket(&MoveToEnclosingBracket, window, cx)
        });
        cx.run_until_parked();
        cx.assert_editor_state(after);
    }

    // Outside bracket jumps to outside of matching bracket
    assert("console.logˇ(var);", "console.log(var)ˇ;", &mut cx);
    assert("console.log(var)ˇ;", "console.logˇ(var);", &mut cx);

    // Inside bracket jumps to inside of matching bracket
    assert("console.log(ˇvar);", "console.log(varˇ);", &mut cx);
    assert("console.log(varˇ);", "console.log(ˇvar);", &mut cx);

    // When outside a bracket and inside, favor jumping to the inside bracket
    assert(
        "console.log('foo', [1, 2, 3]ˇ);",
        "console.log('foo', ˇ[1, 2, 3]);",
        &mut cx,
    );
    assert(
        "console.log(ˇ'foo', [1, 2, 3]);",
        "console.log('foo'ˇ, [1, 2, 3]);",
        &mut cx,
    );

    // Bias forward if two options are equally likely
    assert(
        "let result = curried_fun()ˇ();",
        "let result = curried_fun()()ˇ;",
        &mut cx,
    );

    // If directly adjacent to a smaller pair but inside a larger (not adjacent), pick the smaller
    assert(
        indoc! {"
            function test() {
                console.log('test')ˇ
            }"},
        indoc! {"
            function test() {
                console.logˇ('test')
            }"},
        &mut cx,
    );
}

#[gpui::test]
async fn test_move_to_enclosing_bracket_in_markdown_code_block(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let language_registry = Arc::new(language::LanguageRegistry::test(cx.executor()));
    language_registry.add(markdown_lang());
    language_registry.add(rust_lang());
    let buffer = cx.new(|cx| {
        let mut buffer = language::Buffer::local(
            indoc! {"
            ```rs
            impl Worktree {
                pub async fn open_buffers(&self, path: &Path) -> impl Iterator<&Buffer> {
                }
            }
            ```
        "},
            cx,
        );
        buffer.set_language_registry(language_registry.clone());
        buffer.set_language(Some(markdown_lang()), cx);
        buffer
    });
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let editor = cx.add_window(|window, cx| build_editor(buffer.clone(), window, cx));
    cx.executor().run_until_parked();
    _ = editor.update(cx, |editor, window, cx| {
        // Case 1: Test outer enclosing brackets
        select_ranges(
            editor,
            &indoc! {"
                ```rs
                impl Worktree {
                    pub async fn open_buffers(&self, path: &Path) -> impl Iterator<&Buffer> {
                    }
                }ˇ
                ```
            "},
            window,
            cx,
        );
        editor.move_to_enclosing_bracket(&MoveToEnclosingBracket, window, cx);
        assert_text_with_selections(
            editor,
            &indoc! {"
                ```rs
                impl Worktree ˇ{
                    pub async fn open_buffers(&self, path: &Path) -> impl Iterator<&Buffer> {
                    }
                }
                ```
            "},
            cx,
        );
        // Case 2: Test inner enclosing brackets
        select_ranges(
            editor,
            &indoc! {"
                ```rs
                impl Worktree {
                    pub async fn open_buffers(&self, path: &Path) -> impl Iterator<&Buffer> {
                    }ˇ
                }
                ```
            "},
            window,
            cx,
        );
        editor.move_to_enclosing_bracket(&MoveToEnclosingBracket, window, cx);
        assert_text_with_selections(
            editor,
            &indoc! {"
                ```rs
                impl Worktree {
                    pub async fn open_buffers(&self, path: &Path) -> impl Iterator<&Buffer> ˇ{
                    }
                }
                ```
            "},
            cx,
        );
    });
}

// rust-analyzer's ref-match completions (`&some_str`) deliver the `&` as a
// zero-width additionalTextEdit at the primary edit's start.
// Ref: https://github.com/zed-industries/zed/issues/56973
// Additional edits which overlap the primary completion edit must be skipped
// while non-overlapping edits from the same completion are still applied.
// Ref: https://github.com/zed-industries/zed/pull/1871
// When both the primary completion edit and an additional edit (auto-import)
// start at the very beginning of the file, the additional edit must not be
// treated as overlapping. This payload shape matches what
// typescript-language-server actually sends for auto-imports at file start.
// Ref: https://github.com/zed-industries/zed/issues/26136
fn completion_menu_entries(menu: &CompletionsMenu) -> Vec<String> {
    let entries = menu.entries.borrow();
    entries
        .iter()
        .filter_map(|entry| entry.as_match().map(|m| m.string.clone()))
        .collect()
}

#[gpui::test]
async fn test_document_format_with_prettier(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.formatter = Some(FormatterList::Single(Formatter::Prettier))
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_file(path!("/file.ts"), Default::default()).await;

    let project = Project::test(fs, [path!("/file.ts").as_ref()], cx).await;
    let language_registry = project.read_with(cx, |project, _| project.languages().clone());

    language_registry.add(Arc::new(Language::new(
        LanguageConfig {
            name: "TypeScript".into(),
            matcher: (LanguageMatcher {
                path_suffixes: vec!["ts".to_string()],
                ..Default::default()
            })
            .into(),
            ..Default::default()
        },
        Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
    )));
    update_test_language_settings(cx, &|settings| {
        settings.defaults.prettier.get_or_insert_default().allowed = Some(true);
    });

    let prettier_format_suffix = project::TEST_PRETTIER_FORMAT_SUFFIX;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/file.ts"), cx)
        })
        .await
        .unwrap();

    let buffer_text = "one\ntwo\nthree\n";
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));
    editor.update_in(cx, |editor, window, cx| {
        editor.set_text(buffer_text, window, cx)
    });

    editor
        .update_in(cx, |editor, window, cx| {
            editor.perform_format(
                project.clone(),
                FormatTrigger::Manual,
                FormatTarget::Buffers(editor.buffer().read(cx).all_buffers()),
                window,
                cx,
            )
        })
        .unwrap()
        .await;
    assert_eq!(
        editor.update(cx, |editor, cx| editor.text(cx)),
        buffer_text.to_string() + prettier_format_suffix,
        "Test prettier formatting was not applied to the original buffer text",
    );

    update_test_language_settings(cx, &|settings| {
        settings.defaults.formatter = Some(FormatterList::default())
    });
    let format = editor.update_in(cx, |editor, window, cx| {
        editor.perform_format(
            project.clone(),
            FormatTrigger::Manual,
            FormatTarget::Buffers(editor.buffer().read(cx).all_buffers()),
            window,
            cx,
        )
    });
    format.await.unwrap();
    assert_eq!(
        editor.update(cx, |editor, cx| editor.text(cx)),
        buffer_text.to_string() + prettier_format_suffix + "\n" + prettier_format_suffix,
        "Autoformatting (via test prettier) was not applied to the original buffer text",
    );
}

#[gpui::test]
async fn test_document_format_with_prettier_explicit_language(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.formatter = Some(FormatterList::Single(Formatter::Prettier))
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_file(path!("/file.settings"), Default::default())
        .await;

    let project = Project::test(fs, [path!("/file.settings").as_ref()], cx).await;
    let language_registry = project.read_with(cx, |project, _| project.languages().clone());

    let ts_lang = Arc::new(Language::new(
        LanguageConfig {
            name: "TypeScript".into(),
            matcher: (LanguageMatcher {
                path_suffixes: vec!["ts".to_string()],
                ..LanguageMatcher::default()
            })
            .into(),
            prettier_parser_name: Some("typescript".to_string()),
            ..LanguageConfig::default()
        },
        Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
    ));

    language_registry.add(ts_lang.clone());

    update_test_language_settings(cx, &|settings| {
        settings.defaults.prettier.get_or_insert_default().allowed = Some(true);
    });

    let prettier_format_suffix = project::TEST_PRETTIER_FORMAT_SUFFIX;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/file.settings"), cx)
        })
        .await
        .unwrap();

    project.update(cx, |project, cx| {
        project.set_language_for_buffer(&buffer, ts_lang, cx)
    });

    let buffer_text = "one\ntwo\nthree\n";
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));
    editor.update_in(cx, |editor, window, cx| {
        editor.set_text(buffer_text, window, cx)
    });

    editor
        .update_in(cx, |editor, window, cx| {
            editor.perform_format(
                project.clone(),
                FormatTrigger::Manual,
                FormatTarget::Buffers(editor.buffer().read(cx).all_buffers()),
                window,
                cx,
            )
        })
        .unwrap()
        .await;
    assert_eq!(
        editor.update(cx, |editor, cx| editor.text(cx)),
        buffer_text.to_string() + prettier_format_suffix + "\ntypescript",
        "Test prettier formatting was not applied to the original buffer text",
    );

    update_test_language_settings(cx, &|settings| {
        settings.defaults.formatter = Some(FormatterList::default())
    });
    let format = editor.update_in(cx, |editor, window, cx| {
        editor.perform_format(
            project.clone(),
            FormatTrigger::Manual,
            FormatTarget::Buffers(editor.buffer().read(cx).all_buffers()),
            window,
            cx,
        )
    });
    format.await.unwrap();

    assert_eq!(
        editor.update(cx, |editor, cx| editor.text(cx)),
        buffer_text.to_string()
            + prettier_format_suffix
            + "\ntypescript\n"
            + prettier_format_suffix
            + "\ntypescript",
        "Autoformatting (via test prettier) was not applied to the original buffer text",
    );
}

#[gpui::test]
async fn test_range_format_with_prettier(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.formatter = Some(FormatterList::Single(Formatter::Prettier))
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_file(path!("/file.ts"), Default::default()).await;

    let project = Project::test(fs, [path!("/file.ts").as_ref()], cx).await;
    let language_registry = project.read_with(cx, |project, _| project.languages().clone());

    language_registry.add(Arc::new(Language::new(
        LanguageConfig {
            name: "TypeScript".into(),
            matcher: (LanguageMatcher {
                path_suffixes: vec!["ts".to_string()],
                ..Default::default()
            })
            .into(),
            ..Default::default()
        },
        Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
    )));
    update_test_language_settings(cx, &|settings| {
        settings.defaults.prettier.get_or_insert_default().allowed = Some(true);
    });

    let prettier_range_format_suffix = project::TEST_PRETTIER_RANGE_FORMAT_SUFFIX;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/file.ts"), cx)
        })
        .await
        .unwrap();

    let buffer_text = "one\ntwo\nthree\nfour\nfive\n";
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| {
        build_editor_with_project(project.clone(), buffer, window, cx)
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.set_text(buffer_text, window, cx)
    });

    cx.executor().run_until_parked();

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::default(), window, cx, |s| {
            s.select_ranges([Point::new(1, 0)..Point::new(3, 0)])
        });
    });

    let format = editor
        .update_in(cx, |editor, window, cx| {
            editor.format_selections(&FormatSelections, window, cx)
        })
        .unwrap();
    format.await.unwrap();

    assert_eq!(
        editor.update(cx, |editor, cx| editor.text(cx)),
        format!("one\ntwo{prettier_range_format_suffix}\nthree\nfour\nfive\n"),
        "Range formatting (via test prettier) was not applied to the buffer text",
    );
}

#[gpui::test]
async fn test_range_format_with_prettier_explicit_language(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.formatter = Some(FormatterList::Single(Formatter::Prettier))
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_file(path!("/file.settings"), Default::default())
        .await;

    let project = Project::test(fs, [path!("/file.settings").as_ref()], cx).await;
    let language_registry = project.read_with(cx, |project, _| project.languages().clone());

    let ts_lang = Arc::new(Language::new(
        LanguageConfig {
            name: "TypeScript".into(),
            matcher: (LanguageMatcher {
                path_suffixes: vec!["ts".to_string()],
                ..LanguageMatcher::default()
            })
            .into(),
            prettier_parser_name: Some("typescript".to_string()),
            ..LanguageConfig::default()
        },
        Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
    ));

    language_registry.add(ts_lang.clone());

    update_test_language_settings(cx, &|settings| {
        settings.defaults.prettier.get_or_insert_default().allowed = Some(true);
    });

    let prettier_range_format_suffix = project::TEST_PRETTIER_RANGE_FORMAT_SUFFIX;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/file.settings"), cx)
        })
        .await
        .unwrap();

    project.update(cx, |project, cx| {
        project.set_language_for_buffer(&buffer, ts_lang, cx)
    });

    let buffer_text = "one\ntwo\nthree\nfour\nfive\n";
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| {
        build_editor_with_project(project.clone(), buffer, window, cx)
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.set_text(buffer_text, window, cx)
    });

    cx.executor().run_until_parked();

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::default(), window, cx, |s| {
            s.select_ranges([Point::new(1, 0)..Point::new(3, 0)])
        });
    });

    let format = editor
        .update_in(cx, |editor, window, cx| {
            editor.format_selections(&FormatSelections, window, cx)
        })
        .unwrap();
    format.await.unwrap();

    assert_eq!(
        editor.update(cx, |editor, cx| editor.text(cx)),
        format!("one\ntwo{prettier_range_format_suffix}\ntypescript\nthree\nfour\nfive\n"),
        "Range formatting (via test prettier) was not applied with explicit language",
    );
}

#[gpui::test]
async fn test_addition_reverts(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorLanguageTestContext::new_rust(cx).await;
    let base_text = indoc! {r#"
        struct Row;
        struct Row1;
        struct Row2;

        struct Row4;
        struct Row5;
        struct Row6;

        struct Row8;
        struct Row9;
        struct Row10;"#};

    // When addition hunks are not adjacent to carets, no hunk revert is performed
    assert_hunk_revert(
        indoc! {r#"struct Row;
                   struct Row1;
                   struct Row1.1;
                   struct Row1.2;
                   struct Row2;ˇ

                   struct Row4;
                   struct Row5;
                   struct Row6;

                   struct Row8;
                   ˇstruct Row9;
                   struct Row9.1;
                   struct Row9.2;
                   struct Row9.3;
                   struct Row10;"#},
        vec![DiffHunkStatusKind::Added, DiffHunkStatusKind::Added],
        indoc! {r#"struct Row;
                   struct Row1;
                   struct Row1.1;
                   struct Row1.2;
                   struct Row2;ˇ

                   struct Row4;
                   struct Row5;
                   struct Row6;

                   struct Row8;
                   ˇstruct Row9;
                   struct Row9.1;
                   struct Row9.2;
                   struct Row9.3;
                   struct Row10;"#},
        base_text,
        &mut cx,
    );
    // Same for selections
    assert_hunk_revert(
        indoc! {r#"struct Row;
                   struct Row1;
                   struct Row2;
                   struct Row2.1;
                   struct Row2.2;
                   «ˇ
                   struct Row4;
                   struct» Row5;
                   «struct Row6;
                   ˇ»
                   struct Row9.1;
                   struct Row9.2;
                   struct Row9.3;
                   struct Row8;
                   struct Row9;
                   struct Row10;"#},
        vec![DiffHunkStatusKind::Added, DiffHunkStatusKind::Added],
        indoc! {r#"struct Row;
                   struct Row1;
                   struct Row2;
                   struct Row2.1;
                   struct Row2.2;
                   «ˇ
                   struct Row4;
                   struct» Row5;
                   «struct Row6;
                   ˇ»
                   struct Row9.1;
                   struct Row9.2;
                   struct Row9.3;
                   struct Row8;
                   struct Row9;
                   struct Row10;"#},
        base_text,
        &mut cx,
    );

    // When carets and selections intersect the addition hunks, those are reverted.
    // Adjacent carets got merged.
    assert_hunk_revert(
        indoc! {r#"struct Row;
                   ˇ// something on the top
                   struct Row1;
                   struct Row2;
                   struct Roˇw3.1;
                   struct Row2.2;
                   struct Row2.3;ˇ

                   struct Row4;
                   struct ˇRow5.1;
                   struct Row5.2;
                   struct «Rowˇ»5.3;
                   struct Row5;
                   struct Row6;
                   ˇ
                   struct Row9.1;
                   struct «Rowˇ»9.2;
                   struct «ˇRow»9.3;
                   struct Row8;
                   struct Row9;
                   «ˇ// something on bottom»
                   struct Row10;"#},
        vec![
            DiffHunkStatusKind::Added,
            DiffHunkStatusKind::Added,
            DiffHunkStatusKind::Added,
            DiffHunkStatusKind::Added,
            DiffHunkStatusKind::Added,
        ],
        indoc! {r#"struct Row;
                   ˇstruct Row1;
                   struct Row2;
                   ˇ
                   struct Row4;
                   ˇstruct Row5;
                   struct Row6;
                   ˇ
                   ˇstruct Row8;
                   struct Row9;
                   ˇstruct Row10;"#},
        base_text,
        &mut cx,
    );
}

#[gpui::test]
async fn test_modification_reverts(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorLanguageTestContext::new_rust(cx).await;
    let base_text = indoc! {r#"
        struct Row;
        struct Row1;
        struct Row2;

        struct Row4;
        struct Row5;
        struct Row6;

        struct Row8;
        struct Row9;
        struct Row10;"#};

    // Modification hunks behave the same as the addition ones.
    assert_hunk_revert(
        indoc! {r#"struct Row;
                   struct Row1;
                   struct Row33;
                   ˇ
                   struct Row4;
                   struct Row5;
                   struct Row6;
                   ˇ
                   struct Row99;
                   struct Row9;
                   struct Row10;"#},
        vec![DiffHunkStatusKind::Modified, DiffHunkStatusKind::Modified],
        indoc! {r#"struct Row;
                   struct Row1;
                   struct Row33;
                   ˇ
                   struct Row4;
                   struct Row5;
                   struct Row6;
                   ˇ
                   struct Row99;
                   struct Row9;
                   struct Row10;"#},
        base_text,
        &mut cx,
    );
    assert_hunk_revert(
        indoc! {r#"struct Row;
                   struct Row1;
                   struct Row33;
                   «ˇ
                   struct Row4;
                   struct» Row5;
                   «struct Row6;
                   ˇ»
                   struct Row99;
                   struct Row9;
                   struct Row10;"#},
        vec![DiffHunkStatusKind::Modified, DiffHunkStatusKind::Modified],
        indoc! {r#"struct Row;
                   struct Row1;
                   struct Row33;
                   «ˇ
                   struct Row4;
                   struct» Row5;
                   «struct Row6;
                   ˇ»
                   struct Row99;
                   struct Row9;
                   struct Row10;"#},
        base_text,
        &mut cx,
    );

    assert_hunk_revert(
        indoc! {r#"ˇstruct Row1.1;
                   struct Row1;
                   «ˇstr»uct Row22;

                   struct ˇRow44;
                   struct Row5;
                   struct «Rˇ»ow66;ˇ

                   «struˇ»ct Row88;
                   struct Row9;
                   struct Row1011;ˇ"#},
        vec![
            DiffHunkStatusKind::Modified,
            DiffHunkStatusKind::Modified,
            DiffHunkStatusKind::Modified,
            DiffHunkStatusKind::Modified,
            DiffHunkStatusKind::Modified,
            DiffHunkStatusKind::Modified,
        ],
        indoc! {r#"struct Row;
                   ˇstruct Row1;
                   struct Row2;
                   ˇ
                   struct Row4;
                   ˇstruct Row5;
                   struct Row6;
                   ˇ
                   struct Row8;
                   ˇstruct Row9;
                   struct Row10;ˇ"#},
        base_text,
        &mut cx,
    );
}

#[gpui::test]
async fn test_deleting_over_diff_hunk(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorLanguageTestContext::new_rust(cx).await;
    let base_text = indoc! {r#"
        one

        two
        three
        "#};

    cx.set_head_text(base_text);
    cx.set_state("\nˇ\n");
    cx.executor().run_until_parked();
    cx.update_editor(|editor, _window, cx| {
        editor.expand_selected_diff_hunks(cx);
    });
    cx.executor().run_until_parked();
    cx.update_editor(|editor, window, cx| {
        editor.backspace(&Default::default(), window, cx);
    });
    cx.run_until_parked();
    cx.assert_state_with_diff(
        indoc! {r#"

        - two
        - threeˇ
        +
        "#}
        .to_string(),
    );
}

#[gpui::test]
async fn test_deletion_reverts(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorLanguageTestContext::new_rust(cx).await;
    let base_text = indoc! {r#"struct Row;
struct Row1;
struct Row2;

struct Row4;
struct Row5;
struct Row6;

struct Row8;
struct Row9;
struct Row10;"#};

    // Deletion hunks trigger with carets on adjacent rows, so carets and selections have to stay farther to avoid the revert
    assert_hunk_revert(
        indoc! {r#"struct Row;
                   struct Row2;

                   ˇstruct Row4;
                   struct Row5;
                   struct Row6;
                   ˇ
                   struct Row8;
                   struct Row10;"#},
        vec![DiffHunkStatusKind::Deleted, DiffHunkStatusKind::Deleted],
        indoc! {r#"struct Row;
                   struct Row2;

                   ˇstruct Row4;
                   struct Row5;
                   struct Row6;
                   ˇ
                   struct Row8;
                   struct Row10;"#},
        base_text,
        &mut cx,
    );
    assert_hunk_revert(
        indoc! {r#"struct Row;
                   struct Row2;

                   «ˇstruct Row4;
                   struct» Row5;
                   «struct Row6;
                   ˇ»
                   struct Row8;
                   struct Row10;"#},
        vec![DiffHunkStatusKind::Deleted, DiffHunkStatusKind::Deleted],
        indoc! {r#"struct Row;
                   struct Row2;

                   «ˇstruct Row4;
                   struct» Row5;
                   «struct Row6;
                   ˇ»
                   struct Row8;
                   struct Row10;"#},
        base_text,
        &mut cx,
    );

    // Deletion hunks are ephemeral, so it's impossible to place the caret into them — Zed triggers reverts for lines, adjacent to carets and selections.
    assert_hunk_revert(
        indoc! {r#"struct Row;
                   ˇstruct Row2;

                   struct Row4;
                   struct Row5;
                   struct Row6;

                   struct Row8;ˇ
                   struct Row10;"#},
        vec![DiffHunkStatusKind::Deleted, DiffHunkStatusKind::Deleted],
        indoc! {r#"struct Row;
                   struct Row1;
                   ˇstruct Row2;

                   struct Row4;
                   struct Row5;
                   struct Row6;

                   struct Row8;ˇ
                   struct Row9;
                   struct Row10;"#},
        base_text,
        &mut cx,
    );
    assert_hunk_revert(
        indoc! {r#"struct Row;
                   struct Row2«ˇ;
                   struct Row4;
                   struct» Row5;
                   «struct Row6;

                   struct Row8;ˇ»
                   struct Row10;"#},
        vec![
            DiffHunkStatusKind::Deleted,
            DiffHunkStatusKind::Deleted,
            DiffHunkStatusKind::Deleted,
        ],
        indoc! {r#"struct Row;
                   struct Row1;
                   struct Row2«ˇ;

                   struct Row4;
                   struct» Row5;
                   «struct Row6;

                   struct Row8;ˇ»
                   struct Row9;
                   struct Row10;"#},
        base_text,
        &mut cx,
    );
}

#[gpui::test]
async fn test_multibuffer_reverts(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let base_text_1 = "aaaa\nbbbb\ncccc\ndddd\neeee\nffff\ngggg\nhhhh\niiii\njjjj";
    let base_text_2 = "llll\nmmmm\nnnnn\noooo\npppp\nqqqq\nrrrr\nssss\ntttt\nuuuu";
    let base_text_3 =
        "vvvv\nwwww\nxxxx\nyyyy\nzzzz\n{{{{\n||||\n}}}}\n~~~~\n\u{7f}\u{7f}\u{7f}\u{7f}";

    let text_1 = edit_first_char_of_every_line(base_text_1);
    let text_2 = edit_first_char_of_every_line(base_text_2);
    let text_3 = edit_first_char_of_every_line(base_text_3);

    let buffer_1 = cx.new(|cx| Buffer::local(text_1.clone(), cx));
    let buffer_2 = cx.new(|cx| Buffer::local(text_2.clone(), cx));
    let buffer_3 = cx.new(|cx| Buffer::local(text_3.clone(), cx));

    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1.clone(),
            [
                Point::new(0, 0)..Point::new(2, 0),
                Point::new(5, 0)..Point::new(6, 0),
                Point::new(9, 0)..Point::new(9, 4),
            ],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(1),
            buffer_2.clone(),
            [
                Point::new(0, 0)..Point::new(2, 0),
                Point::new(5, 0)..Point::new(6, 0),
                Point::new(9, 0)..Point::new(9, 4),
            ],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(2),
            buffer_3.clone(),
            [
                Point::new(0, 0)..Point::new(2, 0),
                Point::new(5, 0)..Point::new(6, 0),
                Point::new(9, 0)..Point::new(9, 4),
            ],
            0,
            cx,
        );
        multibuffer
    });

    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [path!("/").as_ref()], cx).await;
    let (editor, cx) = cx
        .add_window_view(|window, cx| build_editor_with_project(project, multibuffer, window, cx));
    editor.update_in(cx, |editor, _window, cx| {
        for (buffer, diff_base) in [
            (buffer_1.clone(), base_text_1),
            (buffer_2.clone(), base_text_2),
            (buffer_3.clone(), base_text_3),
        ] {
            let diff = cx.new(|cx| {
                BufferDiff::new_with_base_text(diff_base, &buffer.read(cx).text_snapshot(), cx)
            });
            editor
                .buffer
                .update(cx, |buffer, cx| buffer.add_diff(diff, cx));
        }
    });
    cx.executor().run_until_parked();

    editor.update_in(cx, |editor, window, cx| {
        assert_eq!(editor.display_text(cx), "\n\nXaaa\nXbbb\nXccc\n\nXfff\nXggg\n\nXjjj\n\n\nXlll\nXmmm\nXnnn\n\nXqqq\nXrrr\n\nXuuu\n\n\nXvvv\nXwww\nXxxx\n\nX{{{\nX|||\n\nX\u{7f}\u{7f}\u{7f}");
        editor.select_all(&SelectAll, window, cx);
        editor.git_restore(&Default::default(), window, cx);
    });
    cx.executor().run_until_parked();

    // When all ranges are selected, all buffer hunks are reverted.
    editor.update(cx, |editor, cx| {
        assert_eq!(editor.display_text(cx), "\n\naaaa\nbbbb\ncccc\ndddd\neeee\nffff\ngggg\nhhhh\niiii\njjjj\n\n\n\n\n\n\nllll\nmmmm\nnnnn\noooo\npppp\nqqqq\nrrrr\nssss\ntttt\nuuuu\n\n\n\n\n\n\nvvvv\nwwww\nxxxx\nyyyy\nzzzz\n{{{{\n||||\n}}}}\n~~~~\n\u{7f}\u{7f}\u{7f}\u{7f}\n\n\n\n");
    });
    buffer_1.update(cx, |buffer, _| {
        assert_eq!(buffer.text(), base_text_1);
    });
    buffer_2.update(cx, |buffer, _| {
        assert_eq!(buffer.text(), base_text_2);
    });
    buffer_3.update(cx, |buffer, _| {
        assert_eq!(buffer.text(), base_text_3);
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.undo(&Default::default(), window, cx);
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges(Some(Point::new(0, 0)..Point::new(5, 0)));
        });
        editor.git_restore(&Default::default(), window, cx);
    });

    // Now, when all ranges selected belong to buffer_1, the revert should succeed,
    // but not affect buffer_2 and its related excerpts.
    editor.update(cx, |editor, cx| {
        assert_eq!(
            editor.display_text(cx),
            "\n\naaaa\nbbbb\ncccc\ndddd\neeee\nffff\ngggg\nhhhh\niiii\njjjj\n\n\n\n\n\n\nXlll\nXmmm\nXnnn\n\nXqqq\nXrrr\n\nXuuu\n\n\nXvvv\nXwww\nXxxx\n\nX{{{\nX|||\n\nX\u{7f}\u{7f}\u{7f}"
        );
    });
    buffer_1.update(cx, |buffer, _| {
        assert_eq!(buffer.text(), base_text_1);
    });
    buffer_2.update(cx, |buffer, _| {
        assert_eq!(
            buffer.text(),
            "Xlll\nXmmm\nXnnn\nXooo\nXppp\nXqqq\nXrrr\nXsss\nXttt\nXuuu"
        );
    });
    buffer_3.update(cx, |buffer, _| {
        assert_eq!(
            buffer.text(),
            "Xvvv\nXwww\nXxxx\nXyyy\nXzzz\nX{{{\nX|||\nX}}}\nX~~~\nX\u{7f}\u{7f}\u{7f}"
        );
    });

    fn edit_first_char_of_every_line(text: &str) -> String {
        text.split('\n')
            .map(|line| format!("X{}", &line[1..]))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[gpui::test]
async fn test_multibuffer_in_navigation_history(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let cols = 4;
    let rows = 10;
    let sample_text_1 = sample_text(rows, cols, 'a');
    assert_eq!(
        sample_text_1,
        "aaaa\nbbbb\ncccc\ndddd\neeee\nffff\ngggg\nhhhh\niiii\njjjj"
    );
    let sample_text_2 = sample_text(rows, cols, 'l');
    assert_eq!(
        sample_text_2,
        "llll\nmmmm\nnnnn\noooo\npppp\nqqqq\nrrrr\nssss\ntttt\nuuuu"
    );
    let sample_text_3 = sample_text(rows, cols, 'v');
    assert_eq!(
        sample_text_3,
        "vvvv\nwwww\nxxxx\nyyyy\nzzzz\n{{{{\n||||\n}}}}\n~~~~\n\u{7f}\u{7f}\u{7f}\u{7f}"
    );

    let buffer_1 = cx.new(|cx| Buffer::local(sample_text_1.clone(), cx));
    let buffer_2 = cx.new(|cx| Buffer::local(sample_text_2.clone(), cx));
    let buffer_3 = cx.new(|cx| Buffer::local(sample_text_3.clone(), cx));

    let multi_buffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1.clone(),
            [
                Point::new(0, 0)..Point::new(2, 0),
                Point::new(5, 0)..Point::new(6, 0),
                Point::new(9, 0)..Point::new(9, 4),
            ],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(1),
            buffer_2.clone(),
            [
                Point::new(0, 0)..Point::new(2, 0),
                Point::new(5, 0)..Point::new(6, 0),
                Point::new(9, 0)..Point::new(9, 4),
            ],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(2),
            buffer_3.clone(),
            [
                Point::new(0, 0)..Point::new(2, 0),
                Point::new(5, 0)..Point::new(6, 0),
                Point::new(9, 0)..Point::new(9, 4),
            ],
            0,
            cx,
        );
        multibuffer
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/a",
        json!({
            "main.rs": sample_text_1,
            "other.rs": sample_text_2,
            "lib.rs": sample_text_3,
        }),
    )
    .await;
    let project = Project::test(fs, ["/a".as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(*window, cx);
    let multi_buffer_editor = cx.new_window_entity(|window, cx| {
        Editor::new(
            EditorMode::full(),
            multi_buffer,
            Some(project.clone()),
            window,
            cx,
        )
    });
    let multibuffer_item_id = workspace.update_in(cx, |workspace, window, cx| {
        assert!(
            workspace.active_item(cx).is_none(),
            "active item should be None before the first item is added"
        );
        workspace.add_item_to_active_pane(
            Box::new(multi_buffer_editor.clone()),
            None,
            true,
            window,
            cx,
        );
        let active_item = workspace
            .active_item(cx)
            .expect("should have an active item after adding the multi buffer");
        assert_eq!(
            active_item.buffer_kind(cx),
            ItemBufferKind::Multibuffer,
            "A multi buffer was expected to active after adding"
        );
        active_item.item_id()
    });

    cx.executor().run_until_parked();

    multi_buffer_editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(
            SelectionEffects::scroll(Autoscroll::Next),
            window,
            cx,
            |s| s.select_ranges(Some(MultiBufferOffset(1)..MultiBufferOffset(2))),
        );
        editor.open_excerpts(&OpenExcerpts, window, cx);
    });
    cx.executor().run_until_parked();
    let first_item_id = workspace.update_in(cx, |workspace, window, cx| {
        let active_item = workspace
            .active_item(cx)
            .expect("should have an active item after navigating into the 1st buffer");
        let first_item_id = active_item.item_id();
        assert_ne!(
            first_item_id, multibuffer_item_id,
            "Should navigate into the 1st buffer and activate it"
        );
        assert_eq!(
            active_item.buffer_kind(cx),
            ItemBufferKind::Singleton,
            "New active item should be a singleton buffer"
        );
        assert_eq!(
            active_item
                .act_as::<Editor>(cx)
                .expect("should have navigated into an editor for the 1st buffer")
                .read(cx)
                .text(cx),
            sample_text_1
        );

        workspace
            .go_back(workspace.active_pane().downgrade(), window, cx)
            .detach_and_log_err(cx);

        first_item_id
    });

    cx.executor().run_until_parked();
    workspace.update_in(cx, |workspace, _, cx| {
        let active_item = workspace
            .active_item(cx)
            .expect("should have an active item after navigating back");
        assert_eq!(
            active_item.item_id(),
            multibuffer_item_id,
            "Should navigate back to the multi buffer"
        );
        assert_eq!(active_item.buffer_kind(cx), ItemBufferKind::Multibuffer);
    });

    multi_buffer_editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(
            SelectionEffects::scroll(Autoscroll::Next),
            window,
            cx,
            |s| s.select_ranges(Some(MultiBufferOffset(39)..MultiBufferOffset(40))),
        );
        editor.open_excerpts(&OpenExcerpts, window, cx);
    });
    cx.executor().run_until_parked();
    let second_item_id = workspace.update_in(cx, |workspace, window, cx| {
        let active_item = workspace
            .active_item(cx)
            .expect("should have an active item after navigating into the 2nd buffer");
        let second_item_id = active_item.item_id();
        assert_ne!(
            second_item_id, multibuffer_item_id,
            "Should navigate away from the multibuffer"
        );
        assert_ne!(
            second_item_id, first_item_id,
            "Should navigate into the 2nd buffer and activate it"
        );
        assert_eq!(
            active_item.buffer_kind(cx),
            ItemBufferKind::Singleton,
            "New active item should be a singleton buffer"
        );
        assert_eq!(
            active_item
                .act_as::<Editor>(cx)
                .expect("should have navigated into an editor")
                .read(cx)
                .text(cx),
            sample_text_2
        );

        workspace
            .go_back(workspace.active_pane().downgrade(), window, cx)
            .detach_and_log_err(cx);

        second_item_id
    });

    cx.executor().run_until_parked();
    workspace.update_in(cx, |workspace, _, cx| {
        let active_item = workspace
            .active_item(cx)
            .expect("should have an active item after navigating back from the 2nd buffer");
        assert_eq!(
            active_item.item_id(),
            multibuffer_item_id,
            "Should navigate back from the 2nd buffer to the multi buffer"
        );
        assert_eq!(active_item.buffer_kind(cx), ItemBufferKind::Multibuffer);
    });

    multi_buffer_editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(
            SelectionEffects::scroll(Autoscroll::Next),
            window,
            cx,
            |s| s.select_ranges(Some(MultiBufferOffset(70)..MultiBufferOffset(70))),
        );
        editor.open_excerpts(&OpenExcerpts, window, cx);
    });
    cx.executor().run_until_parked();
    workspace.update_in(cx, |workspace, window, cx| {
        let active_item = workspace
            .active_item(cx)
            .expect("should have an active item after navigating into the 3rd buffer");
        let third_item_id = active_item.item_id();
        assert_ne!(
            third_item_id, multibuffer_item_id,
            "Should navigate into the 3rd buffer and activate it"
        );
        assert_ne!(third_item_id, first_item_id);
        assert_ne!(third_item_id, second_item_id);
        assert_eq!(
            active_item.buffer_kind(cx),
            ItemBufferKind::Singleton,
            "New active item should be a singleton buffer"
        );
        assert_eq!(
            active_item
                .act_as::<Editor>(cx)
                .expect("should have navigated into an editor")
                .read(cx)
                .text(cx),
            sample_text_3
        );

        workspace
            .go_back(workspace.active_pane().downgrade(), window, cx)
            .detach_and_log_err(cx);
    });

    cx.executor().run_until_parked();
    workspace.update_in(cx, |workspace, _, cx| {
        let active_item = workspace
            .active_item(cx)
            .expect("should have an active item after navigating back from the 3rd buffer");
        assert_eq!(
            active_item.item_id(),
            multibuffer_item_id,
            "Should navigate back from the 3rd buffer to the multi buffer"
        );
        assert_eq!(active_item.buffer_kind(cx), ItemBufferKind::Multibuffer);
    });
}

#[gpui::test]
async fn test_merge_base_diff_hunks_are_read_only(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {},
            "file.rs": "worktree\n",
        }),
    )
    .await;
    fs.set_head_and_index_for_repo(
        std::path::Path::new(path!("/project/.git")),
        &[("file.rs", "head\n".to_string())],
    );
    fs.set_merge_base_content_for_repo(
        std::path::Path::new(path!("/project/.git")),
        &[("file.rs", "base\n".to_string())],
    );

    let project = Project::test(fs.clone(), [path!("/project").as_ref()], cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/project/file.rs"), cx)
        })
        .await
        .unwrap();
    let buffer_id = buffer.read_with(cx, |buffer, _| buffer.remote_id());
    let multi_buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| {
        build_editor_with_project(project.clone(), multi_buffer, window, cx)
    });

    cx.update(|_window, cx| {
        SettingsStore::update_global(cx, |settings, cx| {
            settings.update_user_settings(cx, |settings| {
                settings.git.get_or_insert_default().diff_base =
                    Some(settings::GitDiffBaseSetting::DefaultBranch);
            });
        });
    });
    cx.run_until_parked();

    editor.read_with(cx, |editor, cx| {
        let diff = editor
            .buffer()
            .read(cx)
            .diff_for(buffer_id)
            .expect("buffer should have a display diff");
        assert!(!diff.read(cx).is_stageable());
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_all(&SelectAll, window, cx);
        editor.git_restore(&Default::default(), window, cx);
        editor.toggle_staged_selected_diff_hunks(&Default::default(), window, cx);
    });
    cx.run_until_parked();

    assert_eq!(
        editor.read_with(cx, |editor, cx| editor.text(cx)),
        "worktree\n"
    );
    let index_contents = fs
        .with_git_state(
            std::path::Path::new(path!("/project/.git")),
            false,
            |state| state.index_contents.clone(),
        )
        .unwrap();
    assert_eq!(
        index_contents,
        [(::git::repository::repo_path("file.rs"), b"head\n".to_vec(),)]
            .into_iter()
            .collect::<HashMap<_, _>>()
    );
}

#[gpui::test]
async fn test_toggle_selected_diff_hunks(executor: BackgroundExecutor, cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let diff_base = r#"
        use some::mod;

        const A: u32 = 42;

        fn main() {
            println!("hello");

            println!("world");
        }
        "#
    .unindent();

    cx.set_state(
        &r#"
        use some::modified;

        ˇ
        fn main() {
            println!("hello there");

            println!("around the");
            println!("world");
        }
        "#
        .unindent(),
    );

    cx.set_head_text(&diff_base);
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.go_to_next_hunk(&GoToHunk, window, cx);
        editor.toggle_selected_diff_hunks(&ToggleSelectedDiffHunks, window, cx);
    });
    executor.run_until_parked();
    cx.assert_state_with_diff(
        r#"
          use some::modified;


          fn main() {
        -     println!("hello");
        + ˇ    println!("hello there");

              println!("around the");
              println!("world");
          }
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        for _ in 0..2 {
            editor.go_to_next_hunk(&GoToHunk, window, cx);
            editor.toggle_selected_diff_hunks(&ToggleSelectedDiffHunks, window, cx);
        }
    });
    executor.run_until_parked();
    cx.assert_state_with_diff(
        r#"
        - use some::mod;
        + ˇuse some::modified;


          fn main() {
        -     println!("hello");
        +     println!("hello there");

        +     println!("around the");
              println!("world");
          }
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.go_to_next_hunk(&GoToHunk, window, cx);
        editor.toggle_selected_diff_hunks(&ToggleSelectedDiffHunks, window, cx);
    });
    executor.run_until_parked();
    cx.assert_state_with_diff(
        r#"
        - use some::mod;
        + use some::modified;

        - const A: u32 = 42;
          ˇ
          fn main() {
        -     println!("hello");
        +     println!("hello there");

        +     println!("around the");
              println!("world");
          }
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.cancel(&Cancel, window, cx);
    });

    cx.assert_state_with_diff(
        r#"
          use some::modified;

          ˇ
          fn main() {
              println!("hello there");

              println!("around the");
              println!("world");
          }
        "#
        .unindent(),
    );
}

#[gpui::test]
async fn test_diff_base_change_with_expanded_diff_hunks(
    executor: BackgroundExecutor,
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let diff_base = r#"
        use some::mod1;
        use some::mod2;

        const A: u32 = 42;
        const B: u32 = 42;
        const C: u32 = 42;

        fn main() {
            println!("hello");

            println!("world");
        }
        "#
    .unindent();

    cx.set_state(
        &r#"
        use some::mod2;

        const A: u32 = 42;
        const C: u32 = 42;

        fn main(ˇ) {
            //println!("hello");

            println!("world");
            //
            //
        }
        "#
        .unindent(),
    );

    cx.set_head_text(&diff_base);
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&ExpandAllDiffHunks, window, cx);
    });
    executor.run_until_parked();
    cx.assert_state_with_diff(
        r#"
        - use some::mod1;
          use some::mod2;

          const A: u32 = 42;
        - const B: u32 = 42;
          const C: u32 = 42;

          fn main(ˇ) {
        -     println!("hello");
        +     //println!("hello");

              println!("world");
        +     //
        +     //
          }
        "#
        .unindent(),
    );

    cx.set_head_text("new diff base!");
    executor.run_until_parked();
    cx.assert_state_with_diff(
        r#"
        - new diff base!
        + use some::mod2;
        +
        + const A: u32 = 42;
        + const C: u32 = 42;
        +
        + fn main(ˇ) {
        +     //println!("hello");
        +
        +     println!("world");
        +     //
        +     //
        + }
        "#
        .unindent(),
    );
}

#[gpui::test]
async fn test_go_to_singleton_buffer_point_with_expanded_deleted_hunks(
    executor: BackgroundExecutor,
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let diff_base = r#"
        removed line 1
        removed line 2
        line 1
        line 2
        line 3
        "#
    .unindent();

    cx.set_state(
        &r#"
        ˇline 1
        line 2
        line 3
        "#
        .unindent(),
    );

    cx.set_head_text(&diff_base);
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&ExpandAllDiffHunks, window, cx);
    });
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.go_to_singleton_buffer_point(Point::new(2, 0), window, cx);
    });

    cx.assert_state_with_diff(
        r#"
        - removed line 1
        - removed line 2
          line 1
          line 2
          ˇline 3
        "#
        .unindent(),
    );
}

#[gpui::test]
async fn test_toggle_diff_expand_in_multi_buffer(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let file_1_old = "aaa\nbbb\nccc\nddd\neee\nfff\nggg\nhhh\niii\njjj";
    let file_1_new = "aaa\nccc\nddd\neee\nfff\nggg\nhhh\niii\njjj";
    let file_2_old = "lll\nmmm\nnnn\nooo\nppp\nqqq\nrrr\nsss\nttt\nuuu";
    let file_2_new = "lll\nmmm\nNNN\nooo\nppp\nqqq\nrrr\nsss\nttt\nuuu";
    let file_3_old = "111\n222\n333\n444\n555\n777\n888\n999\n000\n!!!";
    let file_3_new = "111\n222\n333\n444\n555\n666\n777\n888\n999\n000\n!!!";

    let buffer_1 = cx.new(|cx| Buffer::local(file_1_new.to_string(), cx));
    let buffer_2 = cx.new(|cx| Buffer::local(file_2_new.to_string(), cx));
    let buffer_3 = cx.new(|cx| Buffer::local(file_3_new.to_string(), cx));

    let multi_buffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1.clone(),
            [
                Point::new(0, 0)..Point::new(2, 3),
                Point::new(5, 0)..Point::new(6, 3),
                Point::new(9, 0)..Point::new(10, 3),
            ],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(1),
            buffer_2.clone(),
            [
                Point::new(0, 0)..Point::new(2, 3),
                Point::new(5, 0)..Point::new(6, 3),
                Point::new(9, 0)..Point::new(10, 3),
            ],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(2),
            buffer_3.clone(),
            [
                Point::new(0, 0)..Point::new(2, 3),
                Point::new(5, 0)..Point::new(6, 3),
                Point::new(9, 0)..Point::new(10, 3),
            ],
            0,
            cx,
        );
        assert_eq!(multibuffer.read(cx).excerpts().count(), 9);
        multibuffer
    });

    let editor =
        cx.add_window(|window, cx| Editor::new(EditorMode::full(), multi_buffer, None, window, cx));
    editor
        .update(cx, |editor, _window, cx| {
            for (buffer, diff_base) in [
                (buffer_1.clone(), file_1_old),
                (buffer_2.clone(), file_2_old),
                (buffer_3.clone(), file_3_old),
            ] {
                let diff = cx.new(|cx| {
                    BufferDiff::new_with_base_text(diff_base, &buffer.read(cx).text_snapshot(), cx)
                });
                editor
                    .buffer
                    .update(cx, |buffer, cx| buffer.add_diff(diff, cx));
            }
        })
        .unwrap();

    let mut cx = EditorTestContext::for_editor(editor, cx).await;
    cx.run_until_parked();

    cx.assert_editor_state(
        &"
            ˇaaa
            ccc
            ddd
            ggg
            hhh

            lll
            mmm
            NNN
            qqq
            rrr
            uuu
            111
            222
            333
            666
            777
            000
            !!!"
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.select_all(&SelectAll, window, cx);
        editor.toggle_selected_diff_hunks(&ToggleSelectedDiffHunks, window, cx);
    });
    cx.executor().run_until_parked();

    cx.assert_state_with_diff(
        "
            «aaa
          - bbb
            ccc
            ddd
            ggg
            hhh

            lll
            mmm
          - nnn
          + NNN
            qqq
            rrr
            uuu
            111
            222
            333
          + 666
            777
            000
            !!!ˇ»"
            .unindent(),
    );
}

#[gpui::test]
async fn test_expand_diff_hunk_at_excerpt_boundary(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let base = "aaa\nbbb\nccc\nddd\neee\nfff\nggg\n";
    let text = "aaa\nBBB\nBB2\nccc\nDDD\nEEE\nfff\nggg\nhhh\niii\n";

    let buffer = cx.new(|cx| Buffer::local(text.to_string(), cx));
    let multi_buffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer.clone(),
            [
                Point::new(0, 0)..Point::new(1, 3),
                Point::new(4, 0)..Point::new(6, 3),
                Point::new(9, 0)..Point::new(9, 3),
            ],
            0,
            cx,
        );
        assert_eq!(multibuffer.read(cx).excerpts().count(), 3);
        multibuffer
    });

    let editor =
        cx.add_window(|window, cx| Editor::new(EditorMode::full(), multi_buffer, None, window, cx));
    editor
        .update(cx, |editor, _window, cx| {
            let diff = cx.new(|cx| {
                BufferDiff::new_with_base_text(base, &buffer.read(cx).text_snapshot(), cx)
            });
            editor
                .buffer
                .update(cx, |buffer, cx| buffer.add_diff(diff, cx))
        })
        .unwrap();

    let mut cx = EditorTestContext::for_editor(editor, cx).await;
    cx.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&Default::default(), window, cx)
    });
    cx.executor().run_until_parked();

    // When the start of a hunk coincides with the start of its excerpt,
    // the hunk is expanded. When the start of a hunk is earlier than
    // the start of its excerpt, the hunk is not expanded.
    cx.assert_state_with_diff(
        "
            ˇaaa
          - bbb
          + BBB
          - ddd
          - eee
          + DDD
          + EEE
            fff
            iii"
        .unindent(),
    );
}

#[gpui::test]
async fn test_edits_around_expanded_insertion_hunks(
    executor: BackgroundExecutor,
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let diff_base = r#"
        use some::mod1;
        use some::mod2;

        const A: u32 = 42;

        fn main() {
            println!("hello");

            println!("world");
        }
        "#
    .unindent();
    executor.run_until_parked();
    cx.set_state(
        &r#"
        use some::mod1;
        use some::mod2;

        const A: u32 = 42;
        const B: u32 = 42;
        const C: u32 = 42;
        ˇ

        fn main() {
            println!("hello");

            println!("world");
        }
        "#
        .unindent(),
    );

    cx.set_head_text(&diff_base);
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&ExpandAllDiffHunks, window, cx);
    });
    executor.run_until_parked();

    cx.assert_state_with_diff(
        r#"
        use some::mod1;
        use some::mod2;

        const A: u32 = 42;
      + const B: u32 = 42;
      + const C: u32 = 42;
      + ˇ

        fn main() {
            println!("hello");

            println!("world");
        }
      "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| editor.handle_input("const D: u32 = 42;\n", window, cx));
    executor.run_until_parked();

    cx.assert_state_with_diff(
        r#"
        use some::mod1;
        use some::mod2;

        const A: u32 = 42;
      + const B: u32 = 42;
      + const C: u32 = 42;
      + const D: u32 = 42;
      + ˇ

        fn main() {
            println!("hello");

            println!("world");
        }
      "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| editor.handle_input("const E: u32 = 42;\n", window, cx));
    executor.run_until_parked();

    cx.assert_state_with_diff(
        r#"
        use some::mod1;
        use some::mod2;

        const A: u32 = 42;
      + const B: u32 = 42;
      + const C: u32 = 42;
      + const D: u32 = 42;
      + const E: u32 = 42;
      + ˇ

        fn main() {
            println!("hello");

            println!("world");
        }
      "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.delete_line(&DeleteLine, window, cx);
    });
    executor.run_until_parked();

    cx.assert_state_with_diff(
        r#"
        use some::mod1;
        use some::mod2;

        const A: u32 = 42;
      + const B: u32 = 42;
      + const C: u32 = 42;
      + const D: u32 = 42;
      + const E: u32 = 42;
        ˇ
        fn main() {
            println!("hello");

            println!("world");
        }
      "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.move_up(&MoveUp, window, cx);
        editor.delete_line(&DeleteLine, window, cx);
        editor.move_up(&MoveUp, window, cx);
        editor.delete_line(&DeleteLine, window, cx);
        editor.move_up(&MoveUp, window, cx);
        editor.delete_line(&DeleteLine, window, cx);
    });
    executor.run_until_parked();
    cx.assert_state_with_diff(
        r#"
        use some::mod1;
        use some::mod2;

        const A: u32 = 42;
      + const B: u32 = 42;
        ˇ
        fn main() {
            println!("hello");

            println!("world");
        }
      "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.select_up_by_lines(&SelectUpByLines { lines: 5 }, window, cx);
        editor.delete_line(&DeleteLine, window, cx);
    });
    executor.run_until_parked();
    cx.assert_state_with_diff(
        r#"
        ˇ
        fn main() {
            println!("hello");

            println!("world");
        }
      "#
        .unindent(),
    );
}

#[gpui::test]
async fn test_toggling_adjacent_diff_hunks(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_head_text(indoc! { "
        one
        two
        three
        four
        five
        "
    });
    cx.set_state(indoc! { "
        one
        ˇthree
        five
    "});
    cx.run_until_parked();
    cx.update_editor(|editor, window, cx| {
        editor.toggle_selected_diff_hunks(&Default::default(), window, cx);
    });
    cx.assert_state_with_diff(
        indoc! { "
        one
      - two
        ˇthree
      - four
        five
    "}
        .to_string(),
    );
    cx.update_editor(|editor, window, cx| {
        editor.toggle_selected_diff_hunks(&Default::default(), window, cx);
    });

    cx.assert_state_with_diff(
        indoc! { "
        one
        ˇthree
        five
    "}
        .to_string(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.move_up(&MoveUp, window, cx);
        editor.toggle_selected_diff_hunks(&Default::default(), window, cx);
    });
    cx.assert_state_with_diff(
        indoc! { "
        ˇone
      - two
        three
        five
    "}
        .to_string(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.move_down(&MoveDown, window, cx);
        editor.move_down(&MoveDown, window, cx);
        editor.toggle_selected_diff_hunks(&Default::default(), window, cx);
    });
    cx.assert_state_with_diff(
        indoc! { "
        one
      - two
        ˇthree
      - four
        five
    "}
        .to_string(),
    );

    cx.set_state(indoc! { "
        one
        ˇTWO
        three
        four
        five
    "});
    cx.run_until_parked();
    cx.update_editor(|editor, window, cx| {
        editor.toggle_selected_diff_hunks(&Default::default(), window, cx);
    });

    cx.assert_state_with_diff(
        indoc! { "
            one
          - two
          + ˇTWO
            three
            four
            five
        "}
        .to_string(),
    );
    cx.update_editor(|editor, window, cx| {
        editor.move_up(&Default::default(), window, cx);
        editor.toggle_selected_diff_hunks(&Default::default(), window, cx);
    });
    cx.assert_state_with_diff(
        indoc! { "
            one
            ˇTWO
            three
            four
            five
        "}
        .to_string(),
    );
}

#[gpui::test]
async fn test_toggling_adjacent_diff_hunks_2(
    executor: BackgroundExecutor,
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let diff_base = r#"
        lineA
        lineB
        lineC
        lineD
        "#
    .unindent();

    cx.set_state(
        &r#"
        ˇlineA1
        lineB
        lineD
        "#
        .unindent(),
    );
    cx.set_head_text(&diff_base);
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.toggle_selected_diff_hunks(&ToggleSelectedDiffHunks, window, cx);
    });
    executor.run_until_parked();
    cx.assert_state_with_diff(
        r#"
        - lineA
        + ˇlineA1
          lineB
          lineD
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.move_down(&MoveDown, window, cx);
        editor.move_right(&MoveRight, window, cx);
        editor.toggle_selected_diff_hunks(&ToggleSelectedDiffHunks, window, cx);
    });
    executor.run_until_parked();
    cx.assert_state_with_diff(
        r#"
        - lineA
        + lineA1
          lˇineB
        - lineC
          lineD
        "#
        .unindent(),
    );
}

#[gpui::test]
async fn test_edits_around_expanded_deletion_hunks(
    executor: BackgroundExecutor,
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let diff_base = r#"
        use some::mod1;
        use some::mod2;

        const A: u32 = 42;
        const B: u32 = 42;
        const C: u32 = 42;


        fn main() {
            println!("hello");

            println!("world");
        }
    "#
    .unindent();
    executor.run_until_parked();
    cx.set_state(
        &r#"
        use some::mod1;
        use some::mod2;

        ˇconst B: u32 = 42;
        const C: u32 = 42;


        fn main() {
            println!("hello");

            println!("world");
        }
        "#
        .unindent(),
    );

    cx.set_head_text(&diff_base);
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&ExpandAllDiffHunks, window, cx);
    });
    executor.run_until_parked();

    cx.assert_state_with_diff(
        r#"
        use some::mod1;
        use some::mod2;

      - const A: u32 = 42;
        ˇconst B: u32 = 42;
        const C: u32 = 42;


        fn main() {
            println!("hello");

            println!("world");
        }
      "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.delete_line(&DeleteLine, window, cx);
    });
    executor.run_until_parked();
    cx.assert_state_with_diff(
        r#"
        use some::mod1;
        use some::mod2;

      - const A: u32 = 42;
      - const B: u32 = 42;
        ˇconst C: u32 = 42;


        fn main() {
            println!("hello");

            println!("world");
        }
      "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.delete_line(&DeleteLine, window, cx);
    });
    executor.run_until_parked();
    cx.assert_state_with_diff(
        r#"
        use some::mod1;
        use some::mod2;

      - const A: u32 = 42;
      - const B: u32 = 42;
      - const C: u32 = 42;
        ˇ

        fn main() {
            println!("hello");

            println!("world");
        }
      "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.handle_input("replacement", window, cx);
    });
    executor.run_until_parked();
    cx.assert_state_with_diff(
        r#"
        use some::mod1;
        use some::mod2;

      - const A: u32 = 42;
      - const B: u32 = 42;
      - const C: u32 = 42;
      -
      + replacementˇ

        fn main() {
            println!("hello");

            println!("world");
        }
      "#
        .unindent(),
    );
}

#[gpui::test]
async fn test_backspace_after_deletion_hunk(executor: BackgroundExecutor, cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let base_text = r#"
        one
        two
        three
        four
        five
    "#
    .unindent();
    executor.run_until_parked();
    cx.set_state(
        &r#"
        one
        two
        fˇour
        five
        "#
        .unindent(),
    );

    cx.set_head_text(&base_text);
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&ExpandAllDiffHunks, window, cx);
    });
    executor.run_until_parked();

    cx.assert_state_with_diff(
        r#"
          one
          two
        - three
          fˇour
          five
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.backspace(&Backspace, window, cx);
        editor.backspace(&Backspace, window, cx);
    });
    executor.run_until_parked();
    cx.assert_state_with_diff(
        r#"
          one
          two
        - threeˇ
        - four
        + our
          five
        "#
        .unindent(),
    );
}

#[gpui::test]
async fn test_edit_after_expanded_modification_hunk(
    executor: BackgroundExecutor,
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let diff_base = r#"
        use some::mod1;
        use some::mod2;

        const A: u32 = 42;
        const B: u32 = 42;
        const C: u32 = 42;
        const D: u32 = 42;


        fn main() {
            println!("hello");

            println!("world");
        }"#
    .unindent();

    cx.set_state(
        &r#"
        use some::mod1;
        use some::mod2;

        const A: u32 = 42;
        const B: u32 = 42;
        const C: u32 = 43ˇ
        const D: u32 = 42;


        fn main() {
            println!("hello");

            println!("world");
        }"#
        .unindent(),
    );

    cx.set_head_text(&diff_base);
    executor.run_until_parked();
    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&ExpandAllDiffHunks, window, cx);
    });
    executor.run_until_parked();

    cx.assert_state_with_diff(
        r#"
        use some::mod1;
        use some::mod2;

        const A: u32 = 42;
        const B: u32 = 42;
      - const C: u32 = 42;
      + const C: u32 = 43ˇ
        const D: u32 = 42;


        fn main() {
            println!("hello");

            println!("world");
        }"#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.handle_input("\nnew_line\n", window, cx);
    });
    executor.run_until_parked();

    cx.assert_state_with_diff(
        r#"
        use some::mod1;
        use some::mod2;

        const A: u32 = 42;
        const B: u32 = 42;
      - const C: u32 = 42;
      + const C: u32 = 43
      + new_line
      + ˇ
        const D: u32 = 42;


        fn main() {
            println!("hello");

            println!("world");
        }"#
        .unindent(),
    );
}

#[gpui::test]
async fn test_stage_and_unstage_added_file_hunk(
    executor: BackgroundExecutor,
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_editor(|editor, _, cx| {
        editor.set_expand_all_diff_hunks(cx);
    });

    let working_copy = r#"
            ˇfn main() {
                println!("hello, world!");
            }
        "#
    .unindent();

    cx.set_state(&working_copy);
    executor.run_until_parked();

    cx.assert_state_with_diff(
        r#"
            + ˇfn main() {
            +     println!("hello, world!");
            + }
        "#
        .unindent(),
    );
    cx.assert_index_text(None);

    cx.update_editor(|editor, window, cx| {
        editor.toggle_staged_selected_diff_hunks(&Default::default(), window, cx);
    });
    executor.run_until_parked();
    cx.assert_index_text(Some(&working_copy.replace("ˇ", "")));
    cx.assert_state_with_diff(
        r#"
            + ˇfn main() {
            +     println!("hello, world!");
            + }
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.toggle_staged_selected_diff_hunks(&Default::default(), window, cx);
    });
    executor.run_until_parked();
    cx.assert_index_text(None);
}

async fn setup_indent_guides_editor(
    text: &str,
    cx: &mut TestAppContext,
) -> (BufferId, EditorTestContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    let buffer_id = cx.update_editor(|editor, window, cx| {
        editor.set_text(text, window, cx);
        editor
            .buffer()
            .read(cx)
            .as_singleton()
            .unwrap()
            .read(cx)
            .remote_id()
    });

    (buffer_id, cx)
}

fn assert_indent_guides(
    range: Range<u32>,
    expected: Vec<IndentGuide>,
    active_indices: Option<Vec<usize>>,
    cx: &mut EditorTestContext,
) {
    let indent_guides = cx.update_editor(|editor, window, cx| {
        let snapshot = editor.snapshot(window, cx).display_snapshot;
        let mut indent_guides: Vec<_> = crate::indent_guides::indent_guides_in_range(
            editor,
            MultiBufferRow(range.start)..MultiBufferRow(range.end),
            true,
            &snapshot,
            cx,
        );

        indent_guides.sort_by(|a, b| {
            a.depth.cmp(&b.depth).then(
                a.start_row
                    .cmp(&b.start_row)
                    .then(a.end_row.cmp(&b.end_row)),
            )
        });
        indent_guides
    });

    if let Some(expected) = active_indices {
        let active_indices = cx.update_editor(|editor, window, cx| {
            let snapshot = editor.snapshot(window, cx).display_snapshot;
            editor.find_active_indent_guide_indices(&indent_guides, &snapshot, window, cx)
        });

        assert_eq!(
            active_indices.unwrap().into_iter().collect::<Vec<_>>(),
            expected,
            "Active indent guide indices do not match"
        );
    }

    assert_eq!(indent_guides, expected, "Indent guides do not match");
}

fn indent_guide(buffer_id: BufferId, start_row: u32, end_row: u32, depth: u32) -> IndentGuide {
    IndentGuide {
        buffer_id,
        start_row: MultiBufferRow(start_row),
        end_row: MultiBufferRow(end_row),
        depth,
        tab_size: 4,
        settings: IndentGuideSettings {
            enabled: true,
            line_width: 1,
            active_line_width: 1,
            coloring: IndentGuideColoring::default(),
            background_coloring: IndentGuideBackgroundColoring::default(),
        },
    }
}

#[gpui::test]
async fn test_indent_guide_single_line(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        fn main() {
            let a = 1;
        }"
        .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(0..3, vec![indent_guide(buffer_id, 1, 1, 0)], None, &mut cx);
}

#[gpui::test]
async fn test_indent_guide_simple_block(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        fn main() {
            let a = 1;
            let b = 2;
        }"
        .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(0..4, vec![indent_guide(buffer_id, 1, 2, 0)], None, &mut cx);
}

#[gpui::test]
async fn test_indent_guide_nested(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        fn main() {
            let a = 1;
            if a == 3 {
                let b = 2;
            } else {
                let c = 3;
            }
        }"
        .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(
        0..8,
        vec![
            indent_guide(buffer_id, 1, 6, 0),
            indent_guide(buffer_id, 3, 3, 1),
            indent_guide(buffer_id, 5, 5, 1),
        ],
        None,
        &mut cx,
    );
}

#[gpui::test]
async fn test_indent_guide_tab(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        fn main() {
            let a = 1;
                let b = 2;
            let c = 3;
        }"
        .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(
        0..5,
        vec![
            indent_guide(buffer_id, 1, 3, 0),
            indent_guide(buffer_id, 2, 2, 1),
        ],
        None,
        &mut cx,
    );
}

#[gpui::test]
async fn test_indent_guide_continues_on_empty_line(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        fn main() {
            let a = 1;

            let c = 3;
        }"
        .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(0..5, vec![indent_guide(buffer_id, 1, 3, 0)], None, &mut cx);
}

#[gpui::test]
async fn test_indent_guide_complex(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        fn main() {
            let a = 1;

            let c = 3;

            if a == 3 {
                let b = 2;
            } else {
                let c = 3;
            }
        }"
        .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(
        0..11,
        vec![
            indent_guide(buffer_id, 1, 9, 0),
            indent_guide(buffer_id, 6, 6, 1),
            indent_guide(buffer_id, 8, 8, 1),
        ],
        None,
        &mut cx,
    );
}

#[gpui::test]
async fn test_indent_guide_starts_off_screen(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        fn main() {
            let a = 1;

            let c = 3;

            if a == 3 {
                let b = 2;
            } else {
                let c = 3;
            }
        }"
        .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(
        1..11,
        vec![
            indent_guide(buffer_id, 1, 9, 0),
            indent_guide(buffer_id, 6, 6, 1),
            indent_guide(buffer_id, 8, 8, 1),
        ],
        None,
        &mut cx,
    );
}

#[gpui::test]
async fn test_indent_guide_ends_off_screen(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        fn main() {
            let a = 1;

            let c = 3;

            if a == 3 {
                let b = 2;
            } else {
                let c = 3;
            }
        }"
        .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(
        1..10,
        vec![
            indent_guide(buffer_id, 1, 9, 0),
            indent_guide(buffer_id, 6, 6, 1),
            indent_guide(buffer_id, 8, 8, 1),
        ],
        None,
        &mut cx,
    );
}

#[gpui::test]
async fn test_indent_guide_with_folds(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        fn main() {
            if a {
                b(
                    c,
                    d,
                )
            } else {
                e(
                    f
                )
            }
        }"
        .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(
        0..11,
        vec![
            indent_guide(buffer_id, 1, 10, 0),
            indent_guide(buffer_id, 2, 5, 1),
            indent_guide(buffer_id, 7, 9, 1),
            indent_guide(buffer_id, 3, 4, 2),
            indent_guide(buffer_id, 8, 8, 2),
        ],
        None,
        &mut cx,
    );

    cx.update_editor(|editor, window, cx| {
        editor.fold_at(MultiBufferRow(2), window, cx);
        assert_eq!(
            editor.display_text(cx),
            "
            fn main() {
                if a {
                    b(⋯)
                } else {
                    e(
                        f
                    )
                }
            }"
            .unindent()
        );
    });

    assert_indent_guides(
        0..11,
        vec![
            indent_guide(buffer_id, 1, 10, 0),
            indent_guide(buffer_id, 2, 5, 1),
            indent_guide(buffer_id, 7, 9, 1),
            indent_guide(buffer_id, 8, 8, 2),
        ],
        None,
        &mut cx,
    );
}

#[gpui::test]
async fn test_indent_guide_without_brackets(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        block1
            block2
                block3
                    block4
            block2
        block1
        block1"
            .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(
        1..10,
        vec![
            indent_guide(buffer_id, 1, 4, 0),
            indent_guide(buffer_id, 2, 3, 1),
            indent_guide(buffer_id, 3, 3, 2),
        ],
        None,
        &mut cx,
    );
}

#[gpui::test]
async fn test_indent_guide_ends_before_empty_line(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        block1
            block2
                block3

        block1
        block1"
            .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(
        0..6,
        vec![
            indent_guide(buffer_id, 1, 2, 0),
            indent_guide(buffer_id, 2, 2, 1),
        ],
        None,
        &mut cx,
    );
}

#[gpui::test]
async fn test_indent_guide_ignored_only_whitespace_lines(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        function component() {
        \treturn (
        \t\t\t
        \t\t<div>
        \t\t\t<abc></abc>
        \t\t</div>
        \t)
        }"
        .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(
        0..8,
        vec![
            indent_guide(buffer_id, 1, 6, 0),
            indent_guide(buffer_id, 2, 5, 1),
            indent_guide(buffer_id, 4, 4, 2),
        ],
        None,
        &mut cx,
    );
}

#[gpui::test]
async fn test_indent_guide_fallback_to_next_non_entirely_whitespace_line(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        function component() {
        \treturn (
        \t
        \t\t<div>
        \t\t\t<abc></abc>
        \t\t</div>
        \t)
        }"
        .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(
        0..8,
        vec![
            indent_guide(buffer_id, 1, 6, 0),
            indent_guide(buffer_id, 2, 5, 1),
            indent_guide(buffer_id, 4, 4, 2),
        ],
        None,
        &mut cx,
    );
}

#[gpui::test]
async fn test_indent_guide_continuing_off_screen(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        block1



            block2
        "
        .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(0..1, vec![indent_guide(buffer_id, 1, 1, 0)], None, &mut cx);
}

#[gpui::test]
async fn test_indent_guide_tabs(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
        def a:
        \tb = 3
        \tif True:
        \t\tc = 4
        \t\td = 5
        \tprint(b)
        "
        .unindent(),
        cx,
    )
    .await;

    assert_indent_guides(
        0..6,
        vec![
            indent_guide(buffer_id, 1, 5, 0),
            indent_guide(buffer_id, 3, 4, 1),
        ],
        None,
        &mut cx,
    );
}

#[gpui::test]
async fn test_active_indent_guide_single_line(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
    fn main() {
        let a = 1;
    }"
        .unindent(),
        cx,
    )
    .await;

    cx.update_editor(|editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(1, 0)..Point::new(1, 0)])
        });
    });

    cx.run_until_parked();

    assert_indent_guides(
        0..3,
        vec![indent_guide(buffer_id, 1, 1, 0)],
        Some(vec![0]),
        &mut cx,
    );
}

#[gpui::test]
async fn test_active_indent_guide_respect_indented_range(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
    fn main() {
        if 1 == 2 {
            let a = 1;
        }
    }"
        .unindent(),
        cx,
    )
    .await;

    cx.update_editor(|editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(1, 0)..Point::new(1, 0)])
        });
    });
    cx.run_until_parked();

    assert_indent_guides(
        0..4,
        vec![
            indent_guide(buffer_id, 1, 3, 0),
            indent_guide(buffer_id, 2, 2, 1),
        ],
        Some(vec![1]),
        &mut cx,
    );

    cx.update_editor(|editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(2, 0)..Point::new(2, 0)])
        });
    });
    cx.run_until_parked();

    assert_indent_guides(
        0..4,
        vec![
            indent_guide(buffer_id, 1, 3, 0),
            indent_guide(buffer_id, 2, 2, 1),
        ],
        Some(vec![1]),
        &mut cx,
    );

    cx.update_editor(|editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(3, 0)..Point::new(3, 0)])
        });
    });
    cx.run_until_parked();

    assert_indent_guides(
        0..4,
        vec![
            indent_guide(buffer_id, 1, 3, 0),
            indent_guide(buffer_id, 2, 2, 1),
        ],
        Some(vec![0]),
        &mut cx,
    );
}

#[gpui::test]
async fn test_active_indent_guide_empty_line(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
    fn main() {
        let a = 1;

        let b = 2;
    }"
        .unindent(),
        cx,
    )
    .await;

    cx.update_editor(|editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(2, 0)..Point::new(2, 0)])
        });
    });

    cx.run_until_parked();

    assert_indent_guides(
        0..5,
        vec![indent_guide(buffer_id, 1, 3, 0)],
        Some(vec![0]),
        &mut cx,
    );
}

#[gpui::test]
async fn test_active_indent_guide_non_matching_indent(cx: &mut TestAppContext) {
    let (buffer_id, mut cx) = setup_indent_guides_editor(
        &"
    def m:
        a = 1
        pass"
            .unindent(),
        cx,
    )
    .await;

    cx.update_editor(|editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([Point::new(1, 0)..Point::new(1, 0)])
        });
    });

    cx.run_until_parked();

    assert_indent_guides(
        0..3,
        vec![indent_guide(buffer_id, 1, 2, 0)],
        Some(vec![0]),
        &mut cx,
    );
}

#[gpui::test]
async fn test_indent_guide_with_expanded_diff_hunks(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;
    let text = indoc! {
        "
        impl A {
            fn b() {
                0;
                3;
                5;
                6;
                7;
            }
        }
        "
    };
    let base_text = indoc! {
        "
        impl A {
            fn b() {
                0;
                1;
                2;
                3;
                4;
            }
            fn c() {
                5;
                6;
                7;
            }
        }
        "
    };

    cx.update_editor(|editor, window, cx| {
        editor.set_text(text, window, cx);

        editor.buffer().update(cx, |multibuffer, cx| {
            let buffer = multibuffer.as_singleton().unwrap();
            let diff = cx.new(|cx| {
                BufferDiff::new_with_base_text(base_text, &buffer.read(cx).text_snapshot(), cx)
            });

            multibuffer.set_all_diff_hunks_expanded(cx);
            multibuffer.add_diff(diff, cx);

            buffer.read(cx).remote_id()
        })
    });
    cx.run_until_parked();

    cx.assert_state_with_diff(
        indoc! { "
          impl A {
              fn b() {
                  0;
        -         1;
        -         2;
                  3;
        -         4;
        -     }
        -     fn c() {
                  5;
                  6;
                  7;
              }
          }
          ˇ"
        }
        .to_string(),
    );

    let mut actual_guides = cx.update_editor(|editor, window, cx| {
        editor
            .snapshot(window, cx)
            .buffer_snapshot()
            .indent_guides_in_range(Anchor::Min..Anchor::Max, false, cx)
            .map(|guide| (guide.start_row..=guide.end_row, guide.depth))
            .collect::<Vec<_>>()
    });
    actual_guides.sort_by_key(|item| (*item.0.start(), item.1));
    assert_eq!(
        actual_guides,
        vec![
            (MultiBufferRow(1)..=MultiBufferRow(12), 0),
            (MultiBufferRow(2)..=MultiBufferRow(6), 1),
            (MultiBufferRow(9)..=MultiBufferRow(11), 1),
        ]
    );
}

#[gpui::test]
async fn test_adjacent_diff_hunks(executor: BackgroundExecutor, cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let diff_base = r#"
        a
        b
        c
        "#
    .unindent();

    cx.set_state(
        &r#"
        ˇA
        b
        C
        "#
        .unindent(),
    );
    cx.set_head_text(&diff_base);
    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&ExpandAllDiffHunks, window, cx);
    });
    executor.run_until_parked();

    let both_hunks_expanded = r#"
        - a
        + ˇA
          b
        - c
        + C
        "#
    .unindent();

    cx.assert_state_with_diff(both_hunks_expanded.clone());

    let hunk_ranges = cx.update_editor(|editor, window, cx| {
        let snapshot = editor.snapshot(window, cx);
        let hunks = editor
            .diff_hunks_in_ranges(&[Anchor::Min..Anchor::Max], &snapshot.buffer_snapshot())
            .collect::<Vec<_>>();
        let multibuffer_snapshot = editor.buffer.read(cx).snapshot(cx);
        hunks
            .into_iter()
            .map(|hunk| {
                multibuffer_snapshot
                    .anchor_in_excerpt(hunk.buffer_range.start)
                    .unwrap()
                    ..multibuffer_snapshot
                        .anchor_in_excerpt(hunk.buffer_range.end)
                        .unwrap()
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(hunk_ranges.len(), 2);

    cx.update_editor(|editor, _, cx| {
        editor.toggle_single_diff_hunk(hunk_ranges[0].clone(), cx);
    });
    executor.run_until_parked();

    let second_hunk_expanded = r#"
          ˇA
          b
        - c
        + C
        "#
    .unindent();

    cx.assert_state_with_diff(second_hunk_expanded);

    cx.update_editor(|editor, _, cx| {
        editor.toggle_single_diff_hunk(hunk_ranges[0].clone(), cx);
    });
    executor.run_until_parked();

    cx.assert_state_with_diff(both_hunks_expanded.clone());

    cx.update_editor(|editor, _, cx| {
        editor.toggle_single_diff_hunk(hunk_ranges[1].clone(), cx);
    });
    executor.run_until_parked();

    let first_hunk_expanded = r#"
        - a
        + ˇA
          b
          C
        "#
    .unindent();

    cx.assert_state_with_diff(first_hunk_expanded);

    cx.update_editor(|editor, _, cx| {
        editor.toggle_single_diff_hunk(hunk_ranges[1].clone(), cx);
    });
    executor.run_until_parked();

    cx.assert_state_with_diff(both_hunks_expanded);

    cx.set_state(
        &r#"
        ˇA
        b
        "#
        .unindent(),
    );
    cx.run_until_parked();

    // TODO this cursor position seems bad
    cx.assert_state_with_diff(
        r#"
        - ˇa
        + A
          b
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&ExpandAllDiffHunks, window, cx);
    });

    cx.assert_state_with_diff(
        r#"
            - ˇa
            + A
              b
            - c
            "#
        .unindent(),
    );

    let hunk_ranges = cx.update_editor(|editor, window, cx| {
        let snapshot = editor.snapshot(window, cx);
        let hunks = editor
            .diff_hunks_in_ranges(&[Anchor::Min..Anchor::Max], &snapshot.buffer_snapshot())
            .collect::<Vec<_>>();
        let multibuffer_snapshot = snapshot.buffer_snapshot();
        hunks
            .into_iter()
            .map(|hunk| {
                multibuffer_snapshot
                    .anchor_in_excerpt(hunk.buffer_range.start)
                    .unwrap()
                    ..multibuffer_snapshot
                        .anchor_in_excerpt(hunk.buffer_range.end)
                        .unwrap()
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(hunk_ranges.len(), 2);

    cx.update_editor(|editor, _, cx| {
        editor.toggle_single_diff_hunk(hunk_ranges[1].clone(), cx);
    });
    executor.run_until_parked();

    cx.assert_state_with_diff(
        r#"
        - ˇa
        + A
          b
        "#
        .unindent(),
    );
}

#[gpui::test]
async fn test_toggle_deletion_hunk_at_start_of_file(
    executor: BackgroundExecutor,
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let diff_base = r#"
        a
        b
        c
        "#
    .unindent();

    cx.set_state(
        &r#"
        ˇb
        c
        "#
        .unindent(),
    );
    cx.set_head_text(&diff_base);
    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&ExpandAllDiffHunks, window, cx);
    });
    executor.run_until_parked();

    let hunk_expanded = r#"
        - a
          ˇb
          c
        "#
    .unindent();

    cx.assert_state_with_diff(hunk_expanded.clone());

    let hunk_ranges = cx.update_editor(|editor, window, cx| {
        let snapshot = editor.snapshot(window, cx);
        let hunks = editor
            .diff_hunks_in_ranges(&[Anchor::Min..Anchor::Max], &snapshot.buffer_snapshot())
            .collect::<Vec<_>>();
        let multibuffer_snapshot = editor.buffer.read(cx).snapshot(cx);
        hunks
            .into_iter()
            .map(|hunk| {
                multibuffer_snapshot
                    .anchor_in_excerpt(hunk.buffer_range.start)
                    .unwrap()
                    ..multibuffer_snapshot
                        .anchor_in_excerpt(hunk.buffer_range.end)
                        .unwrap()
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(hunk_ranges.len(), 1);

    cx.update_editor(|editor, _, cx| {
        editor.toggle_single_diff_hunk(hunk_ranges[0].clone(), cx);
    });
    executor.run_until_parked();

    let hunk_collapsed = r#"
          ˇb
          c
        "#
    .unindent();

    cx.assert_state_with_diff(hunk_collapsed);

    cx.update_editor(|editor, _, cx| {
        editor.toggle_single_diff_hunk(hunk_ranges[0].clone(), cx);
    });
    executor.run_until_parked();

    cx.assert_state_with_diff(hunk_expanded);
}

#[gpui::test]
async fn test_select_smaller_syntax_node_after_diff_hunk_collapse(
    executor: BackgroundExecutor,
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(rust_lang()), cx));

    cx.set_state(
        &r#"
        fn main() {
            let x = ˇ1;
        }
        "#
        .unindent(),
    );

    let diff_base = r#"
        fn removed_one() {
            println!("this function was deleted");
        }

        fn removed_two() {
            println!("this function was also deleted");
        }

        fn main() {
            let x = 1;
        }
        "#
    .unindent();
    cx.set_head_text(&diff_base);
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&ExpandAllDiffHunks, window, cx);
    });
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.select_larger_syntax_node(&SelectLargerSyntaxNode, window, cx);
    });

    cx.update_editor(|editor, window, cx| {
        editor.collapse_all_diff_hunks(&CollapseAllDiffHunks, window, cx);
    });
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.select_smaller_syntax_node(&SelectSmallerSyntaxNode, window, cx);
    });
}

#[gpui::test]
async fn test_expand_first_line_diff_hunk_keeps_deleted_lines_visible(
    executor: BackgroundExecutor,
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("ˇnew\nsecond\nthird\n");
    cx.set_head_text("old\nsecond\nthird\n");
    cx.update_editor(|editor, window, cx| {
        editor.scroll(gpui::Point { x: 0., y: 0. }, window, cx);
    });
    executor.run_until_parked();
    assert_eq!(cx.update_editor(|e, _, cx| e.scroll_position(cx)).y, 0.0);

    // Expanding a diff hunk at the first line inserts deleted lines above the first buffer line.
    cx.update_editor(|editor, window, cx| {
        let snapshot = editor.snapshot(window, cx);
        let multibuffer_snapshot = editor.buffer.read(cx).snapshot(cx);
        let hunks = editor
            .diff_hunks_in_ranges(&[Anchor::Min..Anchor::Max], &snapshot.buffer_snapshot())
            .collect::<Vec<_>>();
        assert_eq!(hunks.len(), 1);
        let hunk_range = multibuffer_snapshot
            .anchor_in_excerpt(hunks[0].buffer_range.start)
            .unwrap()
            ..multibuffer_snapshot
                .anchor_in_excerpt(hunks[0].buffer_range.end)
                .unwrap();
        editor.toggle_single_diff_hunk(hunk_range, cx)
    });
    executor.run_until_parked();
    cx.assert_state_with_diff("- old\n+ ˇnew\n  second\n  third\n".to_string());

    // Keep the editor scrolled to the top so the full hunk remains visible.
    assert_eq!(cx.update_editor(|e, _, cx| e.scroll_position(cx)).y, 0.0);
}

#[gpui::test]
async fn test_display_diff_hunks(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/test"),
        json!({
            ".git": {},
            "file-1": "ONE\n",
            "file-2": "TWO\n",
            "file-3": "THREE\n",
        }),
    )
    .await;

    fs.set_head_for_repo(
        path!("/test/.git").as_ref(),
        &[
            ("file-1", "one\n".into()),
            ("file-2", "two\n".into()),
            ("file-3", "three\n".into()),
        ],
        "deadbeef",
    );

    let project = Project::test(fs, [path!("/test").as_ref()], cx).await;
    let mut buffers = vec![];
    for i in 1..=3 {
        let buffer = project
            .update(cx, |project, cx| {
                let path = format!(path!("/test/file-{}"), i);
                project.open_local_buffer(path, cx)
            })
            .await
            .unwrap();
        buffers.push(buffer);
    }

    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(Capability::ReadWrite);
        multibuffer.set_all_diff_hunks_expanded(cx);
        for buffer in &buffers {
            let snapshot = buffer.read(cx).snapshot();
            multibuffer.set_excerpts_for_path(
                PathKey::with_sort_prefix(0, buffer.read(cx).file().unwrap().path().clone()),
                buffer.clone(),
                vec![Point::zero()..snapshot.max_point()],
                2,
                cx,
            );
        }
        multibuffer
    });

    let editor = cx.add_window(|window, cx| {
        Editor::new(EditorMode::full(), multibuffer, Some(project), window, cx)
    });
    cx.run_until_parked();

    let snapshot = editor
        .update(cx, |editor, window, cx| editor.snapshot(window, cx))
        .unwrap();
    let hunks = snapshot
        .display_diff_hunks_for_rows(DisplayRow(0)..DisplayRow(u32::MAX), &Default::default())
        .map(|hunk| match hunk {
            DisplayDiffHunk::Unfolded {
                display_row_range, ..
            } => display_row_range,
            DisplayDiffHunk::Folded { .. } => unreachable!(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        hunks,
        [
            DisplayRow(2)..DisplayRow(4),
            DisplayRow(7)..DisplayRow(9),
            DisplayRow(12)..DisplayRow(14),
        ]
    );
}

#[gpui::test]
async fn test_partially_staged_hunk(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_head_text(indoc! { "
        one
        two
        three
        four
        five
        "
    });
    cx.set_index_text(indoc! { "
        one
        two
        three
        four
        five
        "
    });
    cx.set_state(indoc! {"
        one
        TWO
        ˇTHREE
        FOUR
        five
    "});
    cx.run_until_parked();
    cx.update_editor(|editor, window, cx| {
        editor.toggle_staged_selected_diff_hunks(&Default::default(), window, cx);
    });
    cx.run_until_parked();
    cx.assert_index_text(Some(indoc! {"
        one
        TWO
        THREE
        FOUR
        five
    "}));
    cx.set_state(indoc! { "
        one
        TWO
        ˇTHREE-HUNDRED
        FOUR
        five
    "});
    cx.run_until_parked();
    cx.update_editor(|editor, window, cx| {
        let snapshot = editor.snapshot(window, cx);
        let hunks = editor
            .diff_hunks_in_ranges(&[Anchor::Min..Anchor::Max], &snapshot.buffer_snapshot())
            .collect::<Vec<_>>();
        assert_eq!(hunks.len(), 1);
        assert_eq!(
            hunks[0].status(),
            DiffHunkStatus {
                kind: DiffHunkStatusKind::Modified,
                secondary: DiffHunkSecondaryStatus::OverlapsWithSecondaryHunk
            }
        );

        editor.toggle_staged_selected_diff_hunks(&Default::default(), window, cx);
    });
    cx.run_until_parked();
    cx.assert_index_text(Some(indoc! {"
        one
        TWO
        THREE-HUNDRED
        FOUR
        five
    "}));
}

#[gpui::test]
fn test_crease_insertion_and_rendering(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("aaaaaa\nbbbbbb\ncccccc\nddddddd\n", cx);
        build_editor(buffer, window, cx)
    });

    let render_args = Arc::new(Mutex::new(None));
    let snapshot = editor
        .update(cx, |editor, window, cx| {
            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let range =
                snapshot.anchor_before(Point::new(1, 0))..snapshot.anchor_after(Point::new(2, 6));

            struct RenderArgs {
                row: MultiBufferRow,
                folded: bool,
                callback: Arc<dyn Fn(bool, &mut Window, &mut App) + Send + Sync>,
            }

            let crease = Crease::inline(
                range,
                FoldPlaceholder::test(),
                {
                    let toggle_callback = render_args.clone();
                    move |row, folded, callback, _window, _cx| {
                        *toggle_callback.lock() = Some(RenderArgs {
                            row,
                            folded,
                            callback,
                        });
                        div()
                    }
                },
                |_row, _folded, _window, _cx| div(),
            );

            editor.insert_creases(Some(crease), cx);
            let snapshot = editor.snapshot(window, cx);
            let _div =
                snapshot.render_crease_toggle(MultiBufferRow(1), false, cx.entity(), window, cx);
            snapshot
        })
        .unwrap();

    let render_args = render_args.lock().take().unwrap();
    assert_eq!(render_args.row, MultiBufferRow(1));
    assert!(!render_args.folded);
    assert!(!snapshot.is_line_folded(MultiBufferRow(1)));

    cx.update_window(*editor, |_, window, cx| {
        (render_args.callback)(true, window, cx)
    })
    .unwrap();
    let snapshot = editor
        .update(cx, |editor, window, cx| editor.snapshot(window, cx))
        .unwrap();
    assert!(snapshot.is_line_folded(MultiBufferRow(1)));

    cx.update_window(*editor, |_, window, cx| {
        (render_args.callback)(false, window, cx)
    })
    .unwrap();
    let snapshot = editor
        .update(cx, |editor, window, cx| editor.snapshot(window, cx))
        .unwrap();
    assert!(!snapshot.is_line_folded(MultiBufferRow(1)));
}

#[gpui::test]
async fn test_input_text(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(
        &r#"ˇone
        two

        three
        fourˇ
        five

        siˇx"#
            .unindent(),
    );

    cx.dispatch_action(HandleInput(String::new()));
    cx.assert_editor_state(
        &r#"ˇone
        two

        three
        fourˇ
        five

        siˇx"#
            .unindent(),
    );

    cx.dispatch_action(HandleInput("AAAA".to_string()));
    cx.assert_editor_state(
        &r#"AAAAˇone
        two

        three
        fourAAAAˇ
        five

        siAAAAˇx"#
            .unindent(),
    );
}

#[gpui::test]
async fn test_scroll_cursor_center_top_bottom(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(
        r#"let foo = 1;
let foo = 2;
let foo = 3;
let fooˇ = 4;
let foo = 5;
let foo = 6;
let foo = 7;
let foo = 8;
let foo = 9;
let foo = 10;
let foo = 11;
let foo = 12;
let foo = 13;
let foo = 14;
let foo = 15;"#,
    );

    cx.update_editor(|e, window, cx| {
        assert_eq!(
            e.next_scroll_position,
            NextScrollCursorCenterTopBottom::Center,
            "Default next scroll direction is center",
        );

        e.scroll_cursor_center_top_bottom(&ScrollCursorCenterTopBottom, window, cx);
        assert_eq!(
            e.next_scroll_position,
            NextScrollCursorCenterTopBottom::Top,
            "After center, next scroll direction should be top",
        );

        e.scroll_cursor_center_top_bottom(&ScrollCursorCenterTopBottom, window, cx);
        assert_eq!(
            e.next_scroll_position,
            NextScrollCursorCenterTopBottom::Bottom,
            "After top, next scroll direction should be bottom",
        );

        e.scroll_cursor_center_top_bottom(&ScrollCursorCenterTopBottom, window, cx);
        assert_eq!(
            e.next_scroll_position,
            NextScrollCursorCenterTopBottom::Center,
            "After bottom, scrolling should start over",
        );

        e.scroll_cursor_center_top_bottom(&ScrollCursorCenterTopBottom, window, cx);
        assert_eq!(
            e.next_scroll_position,
            NextScrollCursorCenterTopBottom::Top,
            "Scrolling continues if retriggered fast enough"
        );
    });

    cx.executor()
        .advance_clock(SCROLL_CENTER_TOP_BOTTOM_DEBOUNCE_TIMEOUT + Duration::from_millis(200));
    cx.executor().run_until_parked();
    cx.update_editor(|e, _, _| {
        assert_eq!(
            e.next_scroll_position,
            NextScrollCursorCenterTopBottom::Center,
            "If scrolling is not triggered fast enough, it should reset"
        );
    });
}

#[gpui::test]
async fn test_folding_buffers(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let sample_text_1 = "aaaa\nbbbb\ncccc\ndddd\neeee\nffff\ngggg\nhhhh\niiii\njjjj".to_string();
    let sample_text_2 = "llll\nmmmm\nnnnn\noooo\npppp\nqqqq\nrrrr\nssss\ntttt\nuuuu".to_string();
    let sample_text_3 = "vvvv\nwwww\nxxxx\nyyyy\nzzzz\n1111\n2222\n3333\n4444\n5555".to_string();

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/a"),
        json!({
            "first.rs": sample_text_1,
            "second.rs": sample_text_2,
            "third.rs": sample_text_3,
        }),
    )
    .await;
    let project = Project::test(fs, [path!("/a").as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let cx = &mut VisualTestContext::from_window(*window, cx);
    let worktree = project.update(cx, |project, cx| {
        let mut worktrees = project.worktrees(cx).collect::<Vec<_>>();
        assert_eq!(worktrees.len(), 1);
        worktrees.pop().unwrap()
    });
    let worktree_id = worktree.update(cx, |worktree, _| worktree.id());

    let buffer_1 = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree_id, rel_path("first.rs")), cx)
        })
        .await
        .unwrap();
    let buffer_2 = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree_id, rel_path("second.rs")), cx)
        })
        .await
        .unwrap();
    let buffer_3 = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree_id, rel_path("third.rs")), cx)
        })
        .await
        .unwrap();

    let multi_buffer = cx.new(|cx| {
        let mut multi_buffer = MultiBuffer::new(ReadWrite);
        multi_buffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1.clone(),
            [
                Point::new(0, 0)..Point::new(2, 0),
                Point::new(5, 0)..Point::new(6, 0),
                Point::new(9, 0)..Point::new(10, 4),
            ],
            0,
            cx,
        );
        multi_buffer.set_excerpts_for_path(
            PathKey::sorted(1),
            buffer_2.clone(),
            [
                Point::new(0, 0)..Point::new(2, 0),
                Point::new(5, 0)..Point::new(6, 0),
                Point::new(9, 0)..Point::new(10, 4),
            ],
            0,
            cx,
        );
        multi_buffer.set_excerpts_for_path(
            PathKey::sorted(2),
            buffer_3.clone(),
            [
                Point::new(0, 0)..Point::new(2, 0),
                Point::new(5, 0)..Point::new(6, 0),
                Point::new(9, 0)..Point::new(10, 4),
            ],
            0,
            cx,
        );
        multi_buffer
    });
    let multi_buffer_editor = cx.new_window_entity(|window, cx| {
        Editor::new(
            EditorMode::full(),
            multi_buffer.clone(),
            Some(project.clone()),
            window,
            cx,
        )
    });

    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        "\n\naaaa\nbbbb\ncccc\n\nffff\ngggg\n\njjjj\n\n\nllll\nmmmm\nnnnn\n\nqqqq\nrrrr\n\nuuuu\n\n\nvvvv\nwwww\nxxxx\n\n1111\n2222\n\n5555",
    );

    multi_buffer_editor.update(cx, |editor, cx| {
        editor.fold_buffer(buffer_1.read(cx).remote_id(), cx)
    });
    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        "\n\n\n\nllll\nmmmm\nnnnn\n\nqqqq\nrrrr\n\nuuuu\n\n\nvvvv\nwwww\nxxxx\n\n1111\n2222\n\n5555",
        "After folding the first buffer, its text should not be displayed"
    );

    multi_buffer_editor.update(cx, |editor, cx| {
        editor.fold_buffer(buffer_2.read(cx).remote_id(), cx)
    });
    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        "\n\n\n\n\n\nvvvv\nwwww\nxxxx\n\n1111\n2222\n\n5555",
        "After folding the second buffer, its text should not be displayed"
    );

    multi_buffer_editor.update(cx, |editor, cx| {
        editor.fold_buffer(buffer_3.read(cx).remote_id(), cx)
    });
    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        "\n\n\n\n\n",
        "After folding the third buffer, its text should not be displayed"
    );

    // Emulate selection inside the fold logic, that should work
    multi_buffer_editor.update_in(cx, |editor, window, cx| {
        editor
            .snapshot(window, cx)
            .next_line_boundary(Point::new(0, 4));
    });

    multi_buffer_editor.update(cx, |editor, cx| {
        editor.unfold_buffer(buffer_2.read(cx).remote_id(), cx)
    });
    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        "\n\n\n\nllll\nmmmm\nnnnn\n\nqqqq\nrrrr\n\nuuuu\n\n",
        "After unfolding the second buffer, its text should be displayed"
    );

    // Typing inside of buffer 1 causes that buffer to be unfolded.
    multi_buffer_editor.update_in(cx, |editor, window, cx| {
        assert_eq!(
            multi_buffer
                .read(cx)
                .snapshot(cx)
                .text_for_range(Point::new(1, 0)..Point::new(1, 4))
                .collect::<String>(),
            "bbbb"
        );
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
            selections.select_ranges(vec![Point::new(1, 0)..Point::new(1, 0)]);
        });
        editor.handle_input("B", window, cx);
    });

    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        "\n\naaaa\nBbbbb\ncccc\n\nffff\ngggg\n\njjjj\n\n\nllll\nmmmm\nnnnn\n\nqqqq\nrrrr\n\nuuuu\n\n",
        "After unfolding the first buffer, its and 2nd buffer's text should be displayed"
    );

    multi_buffer_editor.update(cx, |editor, cx| {
        editor.unfold_buffer(buffer_3.read(cx).remote_id(), cx)
    });
    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        "\n\naaaa\nBbbbb\ncccc\n\nffff\ngggg\n\njjjj\n\n\nllll\nmmmm\nnnnn\n\nqqqq\nrrrr\n\nuuuu\n\n\nvvvv\nwwww\nxxxx\n\n1111\n2222\n\n5555",
        "After unfolding the all buffers, all original text should be displayed"
    );
}

#[gpui::test]
async fn test_folded_buffers_cleared_on_excerpts_removed(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/root"),
        json!({
            "file_a.txt": "File A\nFile A\nFile A",
            "file_b.txt": "File B\nFile B\nFile B",
        }),
    )
    .await;

    let project = Project::test(fs, [path!("/root").as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let cx = &mut VisualTestContext::from_window(*window, cx);
    let worktree = project.update(cx, |project, cx| {
        let mut worktrees = project.worktrees(cx).collect::<Vec<_>>();
        assert_eq!(worktrees.len(), 1);
        worktrees.pop().unwrap()
    });
    let worktree_id = worktree.update(cx, |worktree, _| worktree.id());

    let buffer_a = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree_id, rel_path("file_a.txt")), cx)
        })
        .await
        .unwrap();
    let buffer_b = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree_id, rel_path("file_b.txt")), cx)
        })
        .await
        .unwrap();

    let multi_buffer = cx.new(|cx| {
        let mut multi_buffer = MultiBuffer::new(ReadWrite);
        let range_a = Point::new(0, 0)..Point::new(2, 4);
        let range_b = Point::new(0, 0)..Point::new(2, 4);

        multi_buffer.set_excerpts_for_path(PathKey::sorted(0), buffer_a.clone(), [range_a], 0, cx);
        multi_buffer.set_excerpts_for_path(PathKey::sorted(1), buffer_b.clone(), [range_b], 0, cx);
        multi_buffer
    });

    let editor = cx.new_window_entity(|window, cx| {
        Editor::new(
            EditorMode::full(),
            multi_buffer.clone(),
            Some(project.clone()),
            window,
            cx,
        )
    });

    editor.update(cx, |editor, cx| {
        editor.fold_buffer(buffer_a.read(cx).remote_id(), cx);
    });
    assert!(editor.update(cx, |editor, cx| editor.has_any_buffer_folded(cx)));

    // When the excerpts for `buffer_a` are removed, a
    // `multi_buffer::Event::ExcerptsRemoved` event is emitted, which should be
    // picked up by the editor and update the display map accordingly.
    multi_buffer.update(cx, |multi_buffer, cx| {
        multi_buffer.remove_excerpts(PathKey::sorted(0), cx)
    });
    assert!(!editor.update(cx, |editor, cx| editor.has_any_buffer_folded(cx)));
}

#[gpui::test]
async fn test_folding_buffers_with_one_excerpt(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let sample_text_1 = "1111\n2222\n3333".to_string();
    let sample_text_2 = "4444\n5555\n6666".to_string();
    let sample_text_3 = "7777\n8888\n9999".to_string();

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/a"),
        json!({
            "first.rs": sample_text_1,
            "second.rs": sample_text_2,
            "third.rs": sample_text_3,
        }),
    )
    .await;
    let project = Project::test(fs, [path!("/a").as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let cx = &mut VisualTestContext::from_window(*window, cx);
    let worktree = project.update(cx, |project, cx| {
        let mut worktrees = project.worktrees(cx).collect::<Vec<_>>();
        assert_eq!(worktrees.len(), 1);
        worktrees.pop().unwrap()
    });
    let worktree_id = worktree.update(cx, |worktree, _| worktree.id());

    let buffer_1 = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree_id, rel_path("first.rs")), cx)
        })
        .await
        .unwrap();
    let buffer_2 = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree_id, rel_path("second.rs")), cx)
        })
        .await
        .unwrap();
    let buffer_3 = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree_id, rel_path("third.rs")), cx)
        })
        .await
        .unwrap();

    let multi_buffer = cx.new(|cx| {
        let mut multi_buffer = MultiBuffer::new(ReadWrite);
        multi_buffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1.clone(),
            [Point::new(0, 0)..Point::new(3, 0)],
            0,
            cx,
        );
        multi_buffer.set_excerpts_for_path(
            PathKey::sorted(1),
            buffer_2.clone(),
            [Point::new(0, 0)..Point::new(3, 0)],
            0,
            cx,
        );
        multi_buffer.set_excerpts_for_path(
            PathKey::sorted(2),
            buffer_3.clone(),
            [Point::new(0, 0)..Point::new(3, 0)],
            0,
            cx,
        );
        multi_buffer
    });

    let multi_buffer_editor = cx.new_window_entity(|window, cx| {
        Editor::new(
            EditorMode::full(),
            multi_buffer,
            Some(project.clone()),
            window,
            cx,
        )
    });

    let full_text = "\n\n1111\n2222\n3333\n\n\n4444\n5555\n6666\n\n\n7777\n8888\n9999";
    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        full_text,
    );

    multi_buffer_editor.update(cx, |editor, cx| {
        editor.fold_buffer(buffer_1.read(cx).remote_id(), cx)
    });
    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        "\n\n\n\n4444\n5555\n6666\n\n\n7777\n8888\n9999",
        "After folding the first buffer, its text should not be displayed"
    );

    multi_buffer_editor.update(cx, |editor, cx| {
        editor.fold_buffer(buffer_2.read(cx).remote_id(), cx)
    });

    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        "\n\n\n\n\n\n7777\n8888\n9999",
        "After folding the second buffer, its text should not be displayed"
    );

    multi_buffer_editor.update(cx, |editor, cx| {
        editor.fold_buffer(buffer_3.read(cx).remote_id(), cx)
    });
    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        "\n\n\n\n\n",
        "After folding the third buffer, its text should not be displayed"
    );

    multi_buffer_editor.update(cx, |editor, cx| {
        editor.unfold_buffer(buffer_2.read(cx).remote_id(), cx)
    });
    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        "\n\n\n\n4444\n5555\n6666\n\n",
        "After unfolding the second buffer, its text should be displayed"
    );

    multi_buffer_editor.update(cx, |editor, cx| {
        editor.unfold_buffer(buffer_1.read(cx).remote_id(), cx)
    });
    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        "\n\n1111\n2222\n3333\n\n\n4444\n5555\n6666\n\n",
        "After unfolding the first buffer, its text should be displayed"
    );

    multi_buffer_editor.update(cx, |editor, cx| {
        editor.unfold_buffer(buffer_3.read(cx).remote_id(), cx)
    });
    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        full_text,
        "After unfolding all buffers, all original text should be displayed"
    );
}

#[gpui::test]
async fn test_folding_buffer_when_multibuffer_has_only_one_excerpt(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let sample_text = "aaaa\nbbbb\ncccc\ndddd\neeee\nffff\ngggg\nhhhh\niiii\njjjj".to_string();

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/a"),
        json!({
            "main.rs": sample_text,
        }),
    )
    .await;
    let project = Project::test(fs, [path!("/a").as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let cx = &mut VisualTestContext::from_window(*window, cx);
    let worktree = project.update(cx, |project, cx| {
        let mut worktrees = project.worktrees(cx).collect::<Vec<_>>();
        assert_eq!(worktrees.len(), 1);
        worktrees.pop().unwrap()
    });
    let worktree_id = worktree.update(cx, |worktree, _| worktree.id());

    let buffer_1 = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree_id, rel_path("main.rs")), cx)
        })
        .await
        .unwrap();

    let multi_buffer = cx.new(|cx| {
        let mut multi_buffer = MultiBuffer::new(ReadWrite);
        multi_buffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1.clone(),
            [Point::new(0, 0)
                ..Point::new(
                    sample_text.chars().filter(|&c| c == '\n').count() as u32 + 1,
                    0,
                )],
            0,
            cx,
        );
        multi_buffer
    });
    let multi_buffer_editor = cx.new_window_entity(|window, cx| {
        Editor::new(
            EditorMode::full(),
            multi_buffer,
            Some(project.clone()),
            window,
            cx,
        )
    });

    let selection_range = Point::new(1, 0)..Point::new(2, 0);
    multi_buffer_editor.update_in(cx, |editor, window, cx| {
        let multi_buffer_snapshot = editor.buffer().read(cx).snapshot(cx);
        let highlight_range = selection_range.clone().to_anchors(&multi_buffer_snapshot);
        editor.highlight_text(
            HighlightKey::Editor,
            vec![highlight_range.clone()],
            HighlightStyle::color(Hsla::green()),
            cx,
        );
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges(Some(highlight_range))
        });
    });

    let full_text = format!("\n\n{sample_text}");
    assert_eq!(
        multi_buffer_editor.update(cx, |editor, cx| editor.display_text(cx)),
        full_text,
    );
}

#[gpui::test]
async fn test_multi_buffer_navigation_with_folded_buffers(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    cx.update(|cx| {
        let default_key_bindings = settings::KeymapFile::load_asset_allow_partial_failure(
            "keymaps/default-linux.json",
            cx,
        )
        .unwrap();
        cx.bind_keys(default_key_bindings);
    });

    let (editor, cx) = cx.add_window_view(|window, cx| {
        let multi_buffer = MultiBuffer::build_multi(
            [
                ("a0\nb0\nc0\nd0\ne0\n", vec![Point::row_range(0..2)]),
                ("a1\nb1\nc1\nd1\ne1\n", vec![Point::row_range(0..2)]),
                ("a2\nb2\nc2\nd2\ne2\n", vec![Point::row_range(0..2)]),
                ("a3\nb3\nc3\nd3\ne3\n", vec![Point::row_range(0..2)]),
            ],
            cx,
        );
        let mut editor = Editor::new(EditorMode::full(), multi_buffer.clone(), None, window, cx);

        let buffer_ids = multi_buffer
            .read(cx)
            .snapshot(cx)
            .excerpts()
            .map(|excerpt| excerpt.context.start.buffer_id)
            .collect::<Vec<_>>();
        // fold all but the second buffer, so that we test navigating between two
        // adjacent folded buffers, as well as folded buffers at the start and
        // end the multibuffer
        editor.fold_buffer(buffer_ids[0], cx);
        editor.fold_buffer(buffer_ids[2], cx);
        editor.fold_buffer(buffer_ids[3], cx);

        editor
    });
    cx.simulate_resize(size(px(1000.), px(1000.)));

    let mut cx = EditorTestContext::for_editor_in(editor.clone(), cx).await;
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        ˇ[FOLDED]
        [EXCERPT]
        a1
        b1
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        [FOLDED]
        "
    });
    cx.simulate_keystroke("down");
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        ˇa1
        b1
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        [FOLDED]
        "
    });
    cx.simulate_keystroke("down");
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        a1
        ˇb1
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        [FOLDED]
        "
    });
    cx.simulate_keystroke("down");
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        a1
        b1
        ˇ[EXCERPT]
        [FOLDED]
        [EXCERPT]
        [FOLDED]
        "
    });
    cx.simulate_keystroke("down");
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        a1
        b1
        [EXCERPT]
        ˇ[FOLDED]
        [EXCERPT]
        [FOLDED]
        "
    });
    for _ in 0..5 {
        cx.simulate_keystroke("down");
        cx.assert_excerpts_with_selections(indoc! {"
            [EXCERPT]
            [FOLDED]
            [EXCERPT]
            a1
            b1
            [EXCERPT]
            [FOLDED]
            [EXCERPT]
            ˇ[FOLDED]
            "
        });
    }

    cx.simulate_keystroke("up");
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        a1
        b1
        [EXCERPT]
        ˇ[FOLDED]
        [EXCERPT]
        [FOLDED]
        "
    });
    cx.simulate_keystroke("up");
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        a1
        b1
        ˇ[EXCERPT]
        [FOLDED]
        [EXCERPT]
        [FOLDED]
        "
    });
    cx.simulate_keystroke("up");
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        a1
        ˇb1
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        [FOLDED]
        "
    });
    cx.simulate_keystroke("up");
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        ˇa1
        b1
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        [FOLDED]
        "
    });
    for _ in 0..5 {
        cx.simulate_keystroke("up");
        cx.assert_excerpts_with_selections(indoc! {"
            [EXCERPT]
            ˇ[FOLDED]
            [EXCERPT]
            a1
            b1
            [EXCERPT]
            [FOLDED]
            [EXCERPT]
            [FOLDED]
            "
        });
    }
}

#[gpui::test]
async fn test_edit_prediction_text(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    // Simple insertion
    assert_highlighted_edits(
        "Hello, world!",
        vec![(Point::new(0, 6)..Point::new(0, 6), " beautiful".into())],
        true,
        cx,
        &|highlighted_edits, cx| {
            assert_eq!(highlighted_edits.text, "Hello, beautiful world!");
            assert_eq!(highlighted_edits.highlights.len(), 1);
            assert_eq!(highlighted_edits.highlights[0].0, 6..16);
            assert_eq!(
                highlighted_edits.highlights[0].1.background_color,
                Some(cx.theme().status().created_background)
            );
        },
    )
    .await;

    // Replacement
    assert_highlighted_edits(
        "This is a test.",
        vec![(Point::new(0, 0)..Point::new(0, 4), "That".into())],
        false,
        cx,
        &|highlighted_edits, cx| {
            assert_eq!(highlighted_edits.text, "That is a test.");
            assert_eq!(highlighted_edits.highlights.len(), 1);
            assert_eq!(highlighted_edits.highlights[0].0, 0..4);
            assert_eq!(
                highlighted_edits.highlights[0].1.background_color,
                Some(cx.theme().status().created_background)
            );
        },
    )
    .await;

    // Multiple edits
    assert_highlighted_edits(
        "Hello, world!",
        vec![
            (Point::new(0, 0)..Point::new(0, 5), "Greetings".into()),
            (Point::new(0, 12)..Point::new(0, 12), " and universe".into()),
        ],
        false,
        cx,
        &|highlighted_edits, cx| {
            assert_eq!(highlighted_edits.text, "Greetings, world and universe!");
            assert_eq!(highlighted_edits.highlights.len(), 2);
            assert_eq!(highlighted_edits.highlights[0].0, 0..9);
            assert_eq!(highlighted_edits.highlights[1].0, 16..29);
            assert_eq!(
                highlighted_edits.highlights[0].1.background_color,
                Some(cx.theme().status().created_background)
            );
            assert_eq!(
                highlighted_edits.highlights[1].1.background_color,
                Some(cx.theme().status().created_background)
            );
        },
    )
    .await;

    // Multiple lines with edits
    assert_highlighted_edits(
        "First line\nSecond line\nThird line\nFourth line",
        vec![
            (Point::new(1, 7)..Point::new(1, 11), "modified".to_string()),
            (
                Point::new(2, 0)..Point::new(2, 10),
                "New third line".to_string(),
            ),
            (Point::new(3, 6)..Point::new(3, 6), " updated".to_string()),
        ],
        false,
        cx,
        &|highlighted_edits, cx| {
            assert_eq!(
                highlighted_edits.text,
                "Second modified\nNew third line\nFourth updated line"
            );
            assert_eq!(highlighted_edits.highlights.len(), 3);
            assert_eq!(highlighted_edits.highlights[0].0, 7..15); // "modified"
            assert_eq!(highlighted_edits.highlights[1].0, 16..30); // "New third line"
            assert_eq!(highlighted_edits.highlights[2].0, 37..45); // " updated"
            for highlight in &highlighted_edits.highlights {
                assert_eq!(
                    highlight.1.background_color,
                    Some(cx.theme().status().created_background)
                );
            }
        },
    )
    .await;
}

#[gpui::test]
async fn test_edit_prediction_text_with_deletions(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    // Deletion
    assert_highlighted_edits(
        "Hello, world!",
        vec![(Point::new(0, 5)..Point::new(0, 11), "".to_string())],
        true,
        cx,
        &|highlighted_edits, cx| {
            assert_eq!(highlighted_edits.text, "Hello, world!");
            assert_eq!(highlighted_edits.highlights.len(), 1);
            assert_eq!(highlighted_edits.highlights[0].0, 5..11);
            assert_eq!(
                highlighted_edits.highlights[0].1.background_color,
                Some(cx.theme().status().deleted_background)
            );
        },
    )
    .await;

    // Insertion
    assert_highlighted_edits(
        "Hello, world!",
        vec![(Point::new(0, 6)..Point::new(0, 6), " digital".to_string())],
        true,
        cx,
        &|highlighted_edits, cx| {
            assert_eq!(highlighted_edits.highlights.len(), 1);
            assert_eq!(highlighted_edits.highlights[0].0, 6..14);
            assert_eq!(
                highlighted_edits.highlights[0].1.background_color,
                Some(cx.theme().status().created_background)
            );
        },
    )
    .await;
}

async fn assert_highlighted_edits(
    text: &str,
    edits: Vec<(Range<Point>, String)>,
    include_deletions: bool,
    cx: &mut TestAppContext,
    assertion_fn: &dyn Fn(HighlightedText, &App),
) {
    let window = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple(text, cx);
        Editor::new(EditorMode::full(), buffer, None, window, cx)
    });
    let cx = &mut VisualTestContext::from_window(*window, cx);

    let (buffer, snapshot) = window
        .update(cx, |editor, _window, cx| {
            (
                editor.buffer().clone(),
                editor.buffer().read(cx).snapshot(cx),
            )
        })
        .unwrap();

    let edits = edits
        .into_iter()
        .map(|(range, edit)| {
            (
                snapshot.anchor_after(range.start)..snapshot.anchor_before(range.end),
                edit,
            )
        })
        .collect::<Vec<_>>();

    let text_anchor_edits = edits
        .clone()
        .into_iter()
        .map(|(range, edit)| {
            (
                range.start.expect_text_anchor()..range.end.expect_text_anchor(),
                edit.into(),
            )
        })
        .collect::<Vec<_>>();

    let edit_preview = window
        .update(cx, |_, _window, cx| {
            buffer
                .read(cx)
                .as_singleton()
                .unwrap()
                .read(cx)
                .preview_edits(text_anchor_edits.into(), cx)
        })
        .unwrap()
        .await;

    cx.update(|_window, cx| {
        let highlighted_edits = edit_prediction_edit_text(
            snapshot.as_singleton().unwrap(),
            &edits,
            &edit_preview,
            include_deletions,
            &snapshot,
            cx,
        );
        assertion_fn(highlighted_edits, cx)
    });
}

#[gpui::test]
fn test_gutter_button_tooltip_updates_intent_with_secondary_modifier(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let focus_handle = cx.update(|cx| cx.focus_handle());
    let tooltip = GutterButtonTooltip {
        primary: GutterButtonIntent::SetBookmark,
        secondary: GutterButtonIntent::SetBookmark,
        focus_handle,
    };

    let primary_intent = tooltip.active_intent(Modifiers::none());
    assert_eq!(primary_intent, GutterButtonIntent::SetBookmark);
    assert!(primary_intent.action().as_any().is::<ToggleBookmark>());
    assert_eq!(
        tooltip.meta_text(primary_intent),
        "right-click for more options"
    );
}

struct BookmarkTestContext {
    project: Entity<Project>,
    editor: Entity<Editor>,
    cx: VisualTestContext,
}

impl BookmarkTestContext {
    async fn new(sample_text: &str, cx: &mut TestAppContext) -> BookmarkTestContext {
        init_test(cx, |_| {});

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/a"),
            json!({
                "main.rs": sample_text,
            }),
        )
        .await;
        let project = Project::test(fs, [path!("/a").as_ref()], cx).await;
        let window =
            cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
        let workspace = window
            .read_with(cx, |mw, _| mw.workspace().clone())
            .unwrap();
        let mut visual_cx = VisualTestContext::from_window(*window, cx);
        let worktree_id = workspace.update_in(&mut visual_cx, |workspace, _window, cx| {
            workspace.project().update(cx, |project, cx| {
                project.worktrees(cx).next().unwrap().read(cx).id()
            })
        });

        let buffer = project
            .update(&mut visual_cx, |project, cx| {
                project.open_buffer((worktree_id, rel_path("main.rs")), cx)
            })
            .await
            .unwrap();

        let (editor, editor_cx) = cx.add_window_view(|window, cx| {
            Editor::new(
                EditorMode::full(),
                MultiBuffer::build_from_buffer(buffer, cx),
                Some(project.clone()),
                window,
                cx,
            )
        });
        let cx = editor_cx.clone();

        BookmarkTestContext {
            project,
            editor,
            cx,
        }
    }

    fn abs_path(&self) -> Arc<Path> {
        let project_path = self.editor.read_with(&self.cx, |editor, cx| {
            editor.active_project_path(cx).unwrap()
        });
        self.project.read_with(&self.cx, |project, cx| {
            project
                .absolute_path(&project_path, cx)
                .map(Arc::from)
                .unwrap()
        })
    }

    fn all_bookmarks(&self) -> BTreeMap<Arc<Path>, Vec<SerializedBookmark>> {
        self.project.read_with(&self.cx, |project, cx| {
            project
                .bookmark_store()
                .read(cx)
                .all_serialized_bookmarks(cx)
        })
    }

    fn assert_bookmarked_file_count(&self, expected_count: usize) {
        assert_eq!(expected_count, self.all_bookmarks().len());
    }

    fn assert_bookmark_rows(&self, expected_rows: Vec<u32>) {
        let abs_path = self.abs_path();
        let bookmarks = self.all_bookmarks();
        if expected_rows.is_empty() {
            assert!(
                !bookmarks.contains_key(&abs_path),
                "Expected no bookmarks for {}",
                abs_path.display()
            );
        } else {
            let mut rows: Vec<u32> = bookmarks
                .get(&abs_path)
                .unwrap()
                .iter()
                .map(|b| b.row)
                .collect();
            rows.sort();
            assert_eq!(expected_rows, rows);
        }
    }

    fn assert_bookmark_labels(&self, expected_labels: Vec<(u32, &str)>) {
        let abs_path = self.abs_path();
        let bookmarks = self.all_bookmarks();
        let mut labels: Vec<(u32, &str)> = bookmarks
            .get(&abs_path)
            .unwrap()
            .iter()
            .map(|bookmark| (bookmark.row, bookmark.label.as_str()))
            .collect();
        labels.sort_by_key(|(row, _)| *row);
        assert_eq!(expected_labels, labels);
    }

    fn confirm_action_available(&mut self) -> bool {
        self.cx
            .update(|window, cx| window.is_action_available(&menu::Confirm, cx))
    }

    fn select_rows(&mut self, rows: &[u32]) {
        assert!(!rows.is_empty(), "expected at least one row to select");

        self.editor
            .update_in(&mut self.cx, |editor: &mut Editor, window, cx| {
                editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                    selections.select_ranges(
                        rows.iter()
                            .copied()
                            .map(|row| Point::new(row, 0)..Point::new(row, 0)),
                    )
                });
            });
    }

    fn prompt_blocks(&mut self) -> Vec<(DisplayRow, u32)> {
        self.editor.update(&mut self.cx, |editor, cx| {
            let snapshot = editor.display_snapshot(cx);
            let max_row = snapshot.max_point().row().next_row();

            snapshot
                .blocks_in_range(DisplayRow(0)..max_row)
                .filter_map(|(row, block)| match block {
                    crate::display_map::Block::Custom(_) => Some((row, block.height())),
                    _ => None,
                })
                .collect()
        })
    }

    fn assert_prompt_block_count(&mut self, expected_count: usize) {
        assert_eq!(expected_count, self.prompt_blocks().len());
    }

    fn draw_window(&mut self) {
        self.cx.update(|window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        });
    }

    fn focus_bookmark_prompt_block(&mut self, block_index: usize) {
        self.draw_window();

        let prompt_blocks = self.prompt_blocks();
        let (block_row, block_height) = *prompt_blocks
            .get(block_index)
            .expect("expected bookmark prompt block");

        let click_position =
            self.editor
                .update_in(&mut self.cx, |editor: &mut Editor, window, cx| {
                    let snapshot = editor.snapshot(window, cx);
                    let block_top = DisplayPoint::new(block_row, 0);
                    let relative_block_top = editor
                        .display_to_pixel_point(block_top, &snapshot, window, cx)
                        .expect("expected prompt block to be visible");
                    let line_height = editor
                        .style(cx)
                        .text
                        .line_height_in_pixels(window.rem_size());
                    let editor_origin = editor
                        .last_position_map
                        .as_ref()
                        .expect("expected editor position map")
                        .text_hitbox
                        .bounds
                        .origin;
                    let editor_center_x = editor
                        .last_bounds
                        .expect("expected editor bounds")
                        .center()
                        .x;

                    gpui::Point {
                        x: editor_center_x,
                        y: editor_origin.y
                            + relative_block_top.y
                            + line_height * (block_height as f32 / 2.),
                    }
                });

        self.cx
            .simulate_click(click_position, gpui::Modifiers::none());
        self.cx.run_until_parked();

        assert!(
            self.confirm_action_available(),
            "expected bookmark prompt block to be focused"
        );
    }

    fn confirm_bookmark_prompt_at_block_index(&mut self, block_index: usize, label: &str) {
        // Confirming a PromptEditor returns focus to the parent editor, so each remaining
        // prompt block must be focused explicitly before typing into it.
        self.focus_bookmark_prompt_block(block_index);
        self.confirm_bookmark_prompt(label);
    }

    fn cursor_row(&mut self) -> u32 {
        self.cursor_point().row
    }

    fn cursor_point(&mut self) -> Point {
        self.editor.update(&mut self.cx, |editor, cx| {
            let snapshot = editor.display_snapshot(cx);
            editor.selections.newest::<Point>(&snapshot).head()
        })
    }

    fn move_to_row(&mut self, row: u32) {
        self.editor
            .update_in(&mut self.cx, |editor: &mut Editor, window, cx| {
                editor.move_to_beginning(&MoveToBeginning, window, cx);
                for _ in 0..row {
                    editor.move_down(&MoveDown, window, cx);
                }
            });
    }

    fn toggle_bookmark(&mut self) {
        self.editor
            .update_in(&mut self.cx, |editor: &mut Editor, window, cx| {
                editor.toggle_bookmark(&actions::ToggleBookmark, window, cx);
            });
    }

    fn toggle_bookmark_with_label(&mut self) {
        self.editor
            .update_in(&mut self.cx, |editor: &mut Editor, window, cx| {
                editor.toggle_bookmark_with_label(&actions::ToggleBookmarkWithLabel, window, cx);
            });
    }

    fn confirm_bookmark_prompt(&mut self, label: &str) {
        if !label.is_empty() {
            self.cx.simulate_input(label);
        }
        self.cx.dispatch_action(menu::Confirm);
        self.cx.run_until_parked();
    }

    fn add_bookmark_with_label(&mut self, label: &str) {
        self.toggle_bookmark_with_label();
        self.confirm_bookmark_prompt(label);
    }

    fn toggle_bookmarks_at_rows(&mut self, rows: &[u32]) {
        for &row in rows {
            self.move_to_row(row);
            self.add_bookmark_with_label("");
        }
    }

    fn go_to_next_bookmark(&mut self) {
        self.editor
            .update_in(&mut self.cx, |editor: &mut Editor, window, cx| {
                editor.go_to_next_bookmark(&actions::GoToNextBookmark, window, cx);
            });
    }

    fn go_to_previous_bookmark(&mut self) {
        self.editor
            .update_in(&mut self.cx, |editor: &mut Editor, window, cx| {
                editor.go_to_previous_bookmark(&actions::GoToPreviousBookmark, window, cx);
            });
    }
}

#[gpui::test]
async fn test_bookmark_toggling(cx: &mut TestAppContext) {
    let mut ctx =
        BookmarkTestContext::new("First line\nSecond line\nThird line\nFourth line", cx).await;

    ctx.add_bookmark_with_label("");
    ctx.editor
        .update_in(&mut ctx.cx, |editor: &mut Editor, window, cx| {
            editor.move_to_end(&MoveToEnd, window, cx);
        });
    ctx.add_bookmark_with_label("");

    ctx.assert_bookmarked_file_count(1);
    ctx.assert_bookmark_rows(vec![0, 3]);

    ctx.move_to_row(0);
    ctx.toggle_bookmark();

    ctx.assert_bookmarked_file_count(1);
    ctx.assert_bookmark_rows(vec![3]);

    ctx.move_to_row(3);
    ctx.toggle_bookmark();

    ctx.assert_bookmarked_file_count(0);
    ctx.assert_bookmark_rows(vec![]);
}

#[gpui::test]
async fn test_bookmark_toggling_unnamed_with_multiple_selections(cx: &mut TestAppContext) {
    let mut ctx =
        BookmarkTestContext::new("First line\nSecond line\nThird line\nFourth line", cx).await;

    ctx.select_rows(&[0, 1, 2]);
    ctx.toggle_bookmark();

    ctx.assert_prompt_block_count(0);
    ctx.assert_bookmarked_file_count(1);
    ctx.assert_bookmark_labels(vec![(0, ""), (1, ""), (2, "")]);

    ctx.select_rows(&[0, 1, 2, 3]);
    ctx.toggle_bookmark();

    ctx.assert_prompt_block_count(0);
    ctx.assert_bookmark_rows(vec![0, 1, 2, 3]);

    ctx.select_rows(&[0, 1, 2, 3]);
    ctx.toggle_bookmark();

    ctx.assert_prompt_block_count(0);
    ctx.assert_bookmarked_file_count(0);
    ctx.assert_bookmark_rows(vec![]);
}

#[gpui::test]
async fn test_bookmark_toggling_with_label_with_multiple_selections(cx: &mut TestAppContext) {
    let mut ctx =
        BookmarkTestContext::new("First line\nSecond line\nThird line\nFourth line", cx).await;

    ctx.select_rows(&[0, 1, 2]);
    ctx.toggle_bookmark_with_label();

    ctx.assert_prompt_block_count(3);
    ctx.assert_bookmarked_file_count(0);

    ctx.confirm_bookmark_prompt_at_block_index(0, "first label");
    ctx.assert_prompt_block_count(2);
    ctx.confirm_bookmark_prompt_at_block_index(0, "second label");
    ctx.assert_prompt_block_count(1);
    ctx.confirm_bookmark_prompt_at_block_index(0, "third label");
    ctx.assert_prompt_block_count(0);

    ctx.assert_bookmarked_file_count(1);
    ctx.assert_bookmark_labels(vec![
        (0, "first label"),
        (1, "second label"),
        (2, "third label"),
    ]);

    ctx.select_rows(&[0, 1, 2, 3]);
    ctx.toggle_bookmark_with_label();

    ctx.assert_prompt_block_count(1);
    ctx.assert_bookmark_labels(vec![
        (0, "first label"),
        (1, "second label"),
        (2, "third label"),
    ]);

    ctx.confirm_bookmark_prompt_at_block_index(0, "fourth label");

    ctx.assert_prompt_block_count(0);
    ctx.assert_bookmark_labels(vec![
        (0, "first label"),
        (1, "second label"),
        (2, "third label"),
        (3, "fourth label"),
    ]);

    ctx.select_rows(&[0, 1, 2, 3]);
    ctx.toggle_bookmark_with_label();

    ctx.assert_prompt_block_count(0);
    ctx.assert_bookmarked_file_count(0);
    ctx.assert_bookmark_rows(vec![]);
}

#[gpui::test]
async fn test_bookmark_toggle_deduplicates_by_row(cx: &mut TestAppContext) {
    let mut ctx =
        BookmarkTestContext::new("First line\nSecond line\nThird line\nFourth line", cx).await;

    ctx.editor
        .update_in(&mut ctx.cx, |editor: &mut Editor, window, cx| {
            editor.move_to_beginning(&MoveToBeginning, window, cx);
        });
    ctx.add_bookmark_with_label("");

    ctx.assert_bookmark_rows(vec![0]);

    ctx.editor
        .update_in(&mut ctx.cx, |editor: &mut Editor, window, cx| {
            editor.move_to_end_of_line(
                &MoveToEndOfLine {
                    stop_at_soft_wraps: true,
                },
                window,
                cx,
            );
        });
    ctx.toggle_bookmark();

    ctx.assert_bookmark_rows(vec![]);
}

#[gpui::test]
async fn test_bookmark_survives_edits(cx: &mut TestAppContext) {
    let mut ctx =
        BookmarkTestContext::new("First line\nSecond line\nThird line\nFourth line", cx).await;

    ctx.move_to_row(2);
    ctx.add_bookmark_with_label("");
    ctx.assert_bookmark_rows(vec![2]);

    ctx.editor
        .update_in(&mut ctx.cx, |editor: &mut Editor, window, cx| {
            editor.move_to_beginning(&MoveToBeginning, window, cx);
            editor.newline(&Newline, window, cx);
        });

    ctx.assert_bookmark_rows(vec![3]);

    ctx.move_to_row(3);
    ctx.toggle_bookmark();
    ctx.assert_bookmark_rows(vec![]);
}

#[gpui::test]
async fn test_active_bookmarks(cx: &mut TestAppContext) {
    let mut ctx = BookmarkTestContext::new(
        "Line 0\nLine 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7\nLine 8\nLine 9",
        cx,
    )
    .await;

    ctx.toggle_bookmarks_at_rows(&[1, 3, 5, 8]);

    let active = ctx
        .editor
        .update_in(&mut ctx.cx, |editor: &mut Editor, window, cx| {
            editor.active_bookmarks(DisplayRow(0)..DisplayRow(10), window, cx)
        });
    assert!(active.contains(&DisplayRow(1)));
    assert!(active.contains(&DisplayRow(3)));
    assert!(active.contains(&DisplayRow(5)));
    assert!(active.contains(&DisplayRow(8)));
    assert!(!active.contains(&DisplayRow(0)));
    assert!(!active.contains(&DisplayRow(2)));
    assert!(!active.contains(&DisplayRow(9)));

    let active = ctx
        .editor
        .update_in(&mut ctx.cx, |editor: &mut Editor, window, cx| {
            editor.active_bookmarks(DisplayRow(2)..DisplayRow(6), window, cx)
        });
    assert!(active.contains(&DisplayRow(3)));
    assert!(active.contains(&DisplayRow(5)));
    assert!(!active.contains(&DisplayRow(1)));
    assert!(!active.contains(&DisplayRow(8)));
}

#[gpui::test]
async fn test_bookmark_not_available_in_single_line_editor(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let (editor, _cx) = cx.add_window_view(|window, cx| Editor::single_line(window, cx));

    editor.update(cx, |editor, _cx| {
        assert!(
            editor.bookmark_store.is_none(),
            "Single-line editors should not have a bookmark store"
        );
    });
}

#[gpui::test]
async fn test_edit_bookmark_does_not_open_prompt_without_existing_bookmark(
    cx: &mut TestAppContext,
) {
    let mut ctx =
        BookmarkTestContext::new("First line\nSecond line\nThird line\nFourth line", cx).await;

    assert!(!ctx.confirm_action_available());

    ctx.editor
        .update_in(&mut ctx.cx, |editor: &mut Editor, window, cx| {
            editor.edit_bookmark(&actions::EditBookmark, window, cx);
        });

    assert!(!ctx.confirm_action_available());
    ctx.assert_bookmark_rows(vec![]);
}

#[gpui::test]
async fn test_edit_bookmark_updates_label_after_confirmation(cx: &mut TestAppContext) {
    let mut ctx =
        BookmarkTestContext::new("First line\nSecond line\nThird line\nFourth line", cx).await;

    ctx.add_bookmark_with_label("old label");
    ctx.assert_bookmark_labels(vec![(0, "old label")]);

    ctx.editor
        .update_in(&mut ctx.cx, |editor: &mut Editor, window, cx| {
            editor.edit_bookmark(&actions::EditBookmark, window, cx);
        });

    assert!(ctx.confirm_action_available());
    ctx.cx.dispatch_action(SelectAll);
    ctx.cx.simulate_input("new label");
    ctx.cx.dispatch_action(menu::Confirm);

    ctx.assert_bookmark_labels(vec![(0, "new label")]);
}

#[gpui::test]
async fn test_bookmark_navigation_lands_at_column_zero(cx: &mut TestAppContext) {
    let mut ctx =
        BookmarkTestContext::new("First line\nSecond line\nThird line\nFourth line", cx).await;

    ctx.editor
        .update_in(&mut ctx.cx, |editor: &mut Editor, window, cx| {
            editor.move_to_beginning(&MoveToBeginning, window, cx);
            editor.move_down(&MoveDown, window, cx);
            editor.move_to_end_of_line(
                &MoveToEndOfLine {
                    stop_at_soft_wraps: true,
                },
                window,
                cx,
            );
        });

    let column_before_toggle = ctx.cursor_point().column;
    assert_eq!(
        column_before_toggle, 11,
        "Cursor should be at the 11th column before toggling bookmark, got column {column_before_toggle}"
    );

    ctx.add_bookmark_with_label("");

    ctx.editor
        .update_in(&mut ctx.cx, |editor: &mut Editor, window, cx| {
            editor.move_to_beginning(&MoveToBeginning, window, cx);
        });

    ctx.go_to_next_bookmark();

    let cursor = ctx.cursor_point();
    assert_eq!(cursor.row, 1, "Should navigate to the bookmarked row");
    assert_eq!(
        cursor.column, 0,
        "Bookmark navigation should always land at column 0"
    );
}

#[gpui::test]
async fn test_bookmark_set_from_nonzero_column_toggles_off_from_column_zero(
    cx: &mut TestAppContext,
) {
    let mut ctx =
        BookmarkTestContext::new("First line\nSecond line\nThird line\nFourth line", cx).await;

    ctx.editor
        .update_in(&mut ctx.cx, |editor: &mut Editor, window, cx| {
            editor.move_to_beginning(&MoveToBeginning, window, cx);
            editor.move_down(&MoveDown, window, cx);
            editor.move_to_end_of_line(
                &MoveToEndOfLine {
                    stop_at_soft_wraps: true,
                },
                window,
                cx,
            );
        });
    ctx.add_bookmark_with_label("");

    ctx.assert_bookmark_rows(vec![1]);

    ctx.editor
        .update_in(&mut ctx.cx, |editor: &mut Editor, window, cx| {
            editor.move_to_beginning_of_line(
                &MoveToBeginningOfLine {
                    stop_at_soft_wraps: true,
                    stop_at_indent: false,
                },
                window,
                cx,
            );
        });
    ctx.toggle_bookmark();

    ctx.assert_bookmark_rows(vec![]);
}

#[gpui::test]
async fn test_go_to_next_bookmark(cx: &mut TestAppContext) {
    let mut ctx = BookmarkTestContext::new(
        "Line 0\nLine 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7\nLine 8\nLine 9",
        cx,
    )
    .await;

    ctx.toggle_bookmarks_at_rows(&[2, 5, 8]);

    ctx.move_to_row(0);

    ctx.go_to_next_bookmark();
    assert_eq!(
        ctx.cursor_row(),
        2,
        "First next-bookmark should go to row 2"
    );

    ctx.go_to_next_bookmark();
    assert_eq!(
        ctx.cursor_row(),
        5,
        "Second next-bookmark should go to row 5"
    );

    ctx.go_to_next_bookmark();
    assert_eq!(
        ctx.cursor_row(),
        8,
        "Third next-bookmark should go to row 8"
    );

    ctx.go_to_next_bookmark();
    assert_eq!(
        ctx.cursor_row(),
        2,
        "Next-bookmark should wrap around to row 2"
    );
}

#[gpui::test]
async fn test_go_to_previous_bookmark(cx: &mut TestAppContext) {
    let mut ctx = BookmarkTestContext::new(
        "Line 0\nLine 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7\nLine 8\nLine 9",
        cx,
    )
    .await;

    ctx.toggle_bookmarks_at_rows(&[2, 5, 8]);

    ctx.editor
        .update_in(&mut ctx.cx, |editor: &mut Editor, window, cx| {
            editor.move_to_end(&MoveToEnd, window, cx);
        });

    ctx.go_to_previous_bookmark();
    assert_eq!(
        ctx.cursor_row(),
        8,
        "First prev-bookmark should go to row 8"
    );

    ctx.go_to_previous_bookmark();
    assert_eq!(
        ctx.cursor_row(),
        5,
        "Second prev-bookmark should go to row 5"
    );

    ctx.go_to_previous_bookmark();
    assert_eq!(
        ctx.cursor_row(),
        2,
        "Third prev-bookmark should go to row 2"
    );

    ctx.go_to_previous_bookmark();
    assert_eq!(
        ctx.cursor_row(),
        8,
        "Prev-bookmark should wrap around to row 8"
    );
}

#[gpui::test]
async fn test_go_to_bookmark_when_cursor_on_bookmarked_line(cx: &mut TestAppContext) {
    let mut ctx = BookmarkTestContext::new(
        "Line 0\nLine 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7\nLine 8\nLine 9",
        cx,
    )
    .await;

    ctx.toggle_bookmarks_at_rows(&[3, 7]);

    ctx.move_to_row(3);

    ctx.go_to_next_bookmark();
    assert_eq!(
        ctx.cursor_row(),
        7,
        "Next from bookmarked row 3 should go to row 7"
    );

    ctx.go_to_previous_bookmark();
    assert_eq!(
        ctx.cursor_row(),
        3,
        "Previous from bookmarked row 7 should go to row 3"
    );

    ctx.go_to_next_bookmark();
    assert_eq!(ctx.cursor_row(), 7, "Next from row 3 should go to row 7");

    ctx.go_to_next_bookmark();
    assert_eq!(ctx.cursor_row(), 3, "Next from row 7 should wrap to row 3");
}

#[gpui::test]
async fn test_go_to_bookmark_with_out_of_order_bookmarks(cx: &mut TestAppContext) {
    let mut ctx = BookmarkTestContext::new(
        "Line 0\nLine 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7\nLine 8\nLine 9",
        cx,
    )
    .await;

    ctx.toggle_bookmarks_at_rows(&[8, 1, 5]);

    ctx.move_to_row(0);

    ctx.go_to_next_bookmark();
    assert_eq!(ctx.cursor_row(), 1, "First next should go to row 1");

    ctx.go_to_next_bookmark();
    assert_eq!(ctx.cursor_row(), 5, "Second next should go to row 5");

    ctx.go_to_next_bookmark();
    assert_eq!(ctx.cursor_row(), 8, "Third next should go to row 8");

    ctx.go_to_next_bookmark();
    assert_eq!(ctx.cursor_row(), 1, "Fourth next should wrap to row 1");

    ctx.go_to_previous_bookmark();
    assert_eq!(
        ctx.cursor_row(),
        8,
        "Prev from row 1 should wrap around to row 8"
    );

    ctx.go_to_previous_bookmark();
    assert_eq!(ctx.cursor_row(), 5, "Prev from row 8 should go to row 5");

    ctx.go_to_previous_bookmark();
    assert_eq!(ctx.cursor_row(), 1, "Prev from row 5 should go to row 1");
}

#[gpui::test]
async fn test_tree_sitter_brackets_newline_insertion(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let language = Arc::new(
        Language::new(
            LanguageConfig::default(),
            Some(tree_sitter_html::LANGUAGE.into()),
        )
        .with_brackets_query(
            r#"
            ("<" @open "/>" @close)
            ("</" @open ">" @close)
            ("<" @open ">" @close)
            ("\"" @open "\"" @close)
            ((element (start_tag) @open (end_tag) @close) (#set! newline.only))
        "#,
        )
        .unwrap(),
    );
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    cx.set_state(indoc! {"
        <span>ˇ</span>
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.assert_editor_state(indoc! {"
        <span>
        ˇ
        </span>
    "});

    cx.set_state(indoc! {"
        <span><span></span>ˇ</span>
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.assert_editor_state(indoc! {"
        <span><span></span>
        ˇ</span>
    "});

    cx.set_state(indoc! {"
        <span>ˇ
        </span>
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.assert_editor_state(indoc! {"
        <span>
        ˇ
        </span>
    "});
}

#[gpui::test]
async fn test_editor_restore_data_different_in_panes(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    let main_text = r#"fn main() {
println!("1");
println!("2");
println!("3");
println!("4");
println!("5");
}"#;
    let lib_text = "mod foo {}";
    fs.insert_tree(
        path!("/a"),
        json!({
            "lib.rs": lib_text,
            "main.rs": main_text,
        }),
    )
    .await;

    let project = Project::test(fs, [path!("/a").as_ref()], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
    let worktree_id = workspace.update(cx, |workspace, cx| {
        workspace.project().update(cx, |project, cx| {
            project.worktrees(cx).next().unwrap().read(cx).id()
        })
    });

    let expected_ranges = vec![
        Point::new(0, 0)..Point::new(0, 0),
        Point::new(1, 0)..Point::new(1, 1),
        Point::new(2, 0)..Point::new(2, 2),
        Point::new(3, 0)..Point::new(3, 3),
    ];

    let pane_1 = workspace.update(cx, |workspace, _| workspace.active_pane().clone());
    let editor_1 = workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.open_path(
                (worktree_id, rel_path("main.rs")),
                Some(pane_1.downgrade()),
                true,
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .downcast::<Editor>()
        .unwrap();
    pane_1.update(cx, |pane, cx| {
        let open_editor = pane.active_item().unwrap().downcast::<Editor>().unwrap();
        open_editor.update(cx, |editor, cx| {
            assert_eq!(
                editor.display_text(cx),
                main_text,
                "Original main.rs text on initial open",
            );
            assert_eq!(
                editor
                    .selections
                    .all::<Point>(&editor.display_snapshot(cx))
                    .into_iter()
                    .map(|s| s.range())
                    .collect::<Vec<_>>(),
                vec![Point::zero()..Point::zero()],
                "Default selections on initial open",
            );
        })
    });
    editor_1.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges(expected_ranges.clone());
        });
    });

    let pane_2 = workspace.update_in(cx, |workspace, window, cx| {
        workspace.split_pane(pane_1.clone(), SplitDirection::Right, window, cx)
    });
    let editor_2 = workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.open_path(
                (worktree_id, rel_path("main.rs")),
                Some(pane_2.downgrade()),
                true,
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .downcast::<Editor>()
        .unwrap();
    pane_2.update(cx, |pane, cx| {
        let open_editor = pane.active_item().unwrap().downcast::<Editor>().unwrap();
        open_editor.update(cx, |editor, cx| {
            assert_eq!(
                editor.display_text(cx),
                main_text,
                "Original main.rs text on initial open in another panel",
            );
            assert_eq!(
                editor
                    .selections
                    .all::<Point>(&editor.display_snapshot(cx))
                    .into_iter()
                    .map(|s| s.range())
                    .collect::<Vec<_>>(),
                vec![Point::zero()..Point::zero()],
                "Default selections on initial open in another panel",
            );
        })
    });

    editor_2.update_in(cx, |editor, window, cx| {
        editor.fold_ranges(expected_ranges.clone(), false, window, cx);
    });

    let _other_editor_1 = workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.open_path(
                (worktree_id, rel_path("lib.rs")),
                Some(pane_1.downgrade()),
                true,
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .downcast::<Editor>()
        .unwrap();
    pane_1
        .update_in(cx, |pane, window, cx| {
            pane.close_other_items(&CloseOtherItems::default(), None, window, cx)
        })
        .await
        .unwrap();
    drop(editor_1);
    pane_1.update(cx, |pane, cx| {
        pane.active_item()
            .unwrap()
            .downcast::<Editor>()
            .unwrap()
            .update(cx, |editor, cx| {
                assert_eq!(
                    editor.display_text(cx),
                    lib_text,
                    "Other file should be open and active",
                );
            });
        assert_eq!(pane.items().count(), 1, "No other editors should be open");
    });

    let _other_editor_2 = workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.open_path(
                (worktree_id, rel_path("lib.rs")),
                Some(pane_2.downgrade()),
                true,
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .downcast::<Editor>()
        .unwrap();
    pane_2
        .update_in(cx, |pane, window, cx| {
            pane.close_other_items(&CloseOtherItems::default(), None, window, cx)
        })
        .await
        .unwrap();
    drop(editor_2);
    pane_2.update(cx, |pane, cx| {
        let open_editor = pane.active_item().unwrap().downcast::<Editor>().unwrap();
        open_editor.update(cx, |editor, cx| {
            assert_eq!(
                editor.display_text(cx),
                lib_text,
                "Other file should be open and active in another panel too",
            );
        });
        assert_eq!(
            pane.items().count(),
            1,
            "No other editors should be open in another pane",
        );
    });

    let _editor_1_reopened = workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.open_path(
                (worktree_id, rel_path("main.rs")),
                Some(pane_1.downgrade()),
                true,
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .downcast::<Editor>()
        .unwrap();
    let _editor_2_reopened = workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.open_path(
                (worktree_id, rel_path("main.rs")),
                Some(pane_2.downgrade()),
                true,
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .downcast::<Editor>()
        .unwrap();
    pane_1.update(cx, |pane, cx| {
        let open_editor = pane.active_item().unwrap().downcast::<Editor>().unwrap();
        open_editor.update(cx, |editor, cx| {
            assert_eq!(
                editor.display_text(cx),
                main_text,
                "Previous editor in the 1st panel had no extra text manipulations and should get none on reopen",
            );
            assert_eq!(
                editor
                    .selections
                    .all::<Point>(&editor.display_snapshot(cx))
                    .into_iter()
                    .map(|s| s.range())
                    .collect::<Vec<_>>(),
                expected_ranges,
                "Previous editor in the 1st panel had selections and should get them restored on reopen",
            );
        })
    });
    pane_2.update(cx, |pane, cx| {
        let open_editor = pane.active_item().unwrap().downcast::<Editor>().unwrap();
        open_editor.update(cx, |editor, cx| {
            assert_eq!(
                editor.display_text(cx),
                r#"fn main() {
⋯rintln!("1");
⋯intln!("2");
⋯ntln!("3");
println!("4");
println!("5");
}"#,
                "Previous editor in the 2nd pane had folds and should restore those on reopen in the same pane",
            );
            assert_eq!(
                editor
                    .selections
                    .all::<Point>(&editor.display_snapshot(cx))
                    .into_iter()
                    .map(|s| s.range())
                    .collect::<Vec<_>>(),
                vec![Point::zero()..Point::zero()],
                "Previous editor in the 2nd pane had no selections changed hence should restore none",
            );
        })
    });
}

#[gpui::test]
async fn test_editor_does_not_restore_data_when_turned_off(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    let main_text = r#"fn main() {
println!("1");
println!("2");
println!("3");
println!("4");
println!("5");
}"#;
    let lib_text = "mod foo {}";
    fs.insert_tree(
        path!("/a"),
        json!({
            "lib.rs": lib_text,
            "main.rs": main_text,
        }),
    )
    .await;

    let project = Project::test(fs, [path!("/a").as_ref()], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
    let worktree_id = workspace.update(cx, |workspace, cx| {
        workspace.project().update(cx, |project, cx| {
            project.worktrees(cx).next().unwrap().read(cx).id()
        })
    });

    let pane = workspace.update(cx, |workspace, _| workspace.active_pane().clone());
    let editor = workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.open_path(
                (worktree_id, rel_path("main.rs")),
                Some(pane.downgrade()),
                true,
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .downcast::<Editor>()
        .unwrap();
    pane.update(cx, |pane, cx| {
        let open_editor = pane.active_item().unwrap().downcast::<Editor>().unwrap();
        open_editor.update(cx, |editor, cx| {
            assert_eq!(
                editor.display_text(cx),
                main_text,
                "Original main.rs text on initial open",
            );
        })
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.fold_ranges(vec![Point::new(0, 0)..Point::new(0, 0)], false, window, cx);
    });

    cx.update_global(|store: &mut SettingsStore, cx| {
        store.update_user_settings(cx, |s| {
            s.workspace.restore_on_file_reopen = Some(false);
        });
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.fold_ranges(
            vec![
                Point::new(1, 0)..Point::new(1, 1),
                Point::new(2, 0)..Point::new(2, 2),
                Point::new(3, 0)..Point::new(3, 3),
            ],
            false,
            window,
            cx,
        );
    });
    pane.update_in(cx, |pane, window, cx| {
        pane.close_all_items(&CloseAllItems::default(), window, cx)
    })
    .await
    .unwrap();
    pane.update(cx, |pane, _| {
        assert!(pane.active_item().is_none());
    });
    cx.update_global(|store: &mut SettingsStore, cx| {
        store.update_user_settings(cx, |s| {
            s.workspace.restore_on_file_reopen = Some(true);
        });
    });

    let _editor_reopened = workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.open_path(
                (worktree_id, rel_path("main.rs")),
                Some(pane.downgrade()),
                true,
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .downcast::<Editor>()
        .unwrap();
    pane.update(cx, |pane, cx| {
        let open_editor = pane.active_item().unwrap().downcast::<Editor>().unwrap();
        open_editor.update(cx, |editor, cx| {
            assert_eq!(
                editor.display_text(cx),
                main_text,
                "No folds: even after enabling the restoration, previous editor's data should not be saved to be used for the restoration"
            );
        })
    });
}

struct EmptyModalView {
    focus_handle: gpui::FocusHandle,
}

impl EventEmitter<DismissEvent> for EmptyModalView {}

impl Render for EmptyModalView {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        div()
    }
}

impl Focusable for EmptyModalView {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

impl workspace::ModalView for EmptyModalView {}

impl EmptyModalView {
    fn new(cx: &App) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }
}

#[gpui::test]
async fn test_hide_mouse_context_menu_on_modal_opened(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let buffer = cx.update(|cx| MultiBuffer::build_simple("hello world!", cx));
    let cx = &mut VisualTestContext::from_window(*window, cx);
    let editor = cx.new_window_entity(|window, cx| {
        Editor::new(
            EditorMode::full(),
            buffer,
            Some(project.clone()),
            window,
            cx,
        )
    });
    workspace.update_in(cx, |workspace, window, cx| {
        workspace.add_item_to_active_pane(Box::new(editor.clone()), None, true, window, cx);
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.open_context_menu(&OpenContextMenu, window, cx);
        assert!(editor.mouse_context_menu.is_some());
    });
    workspace.update_in(cx, |workspace, window, cx| {
        workspace.toggle_modal(window, cx, |_, cx| EmptyModalView::new(cx));
    });

    cx.read(|cx| {
        assert!(editor.read(cx).mouse_context_menu.is_none());
    });
}

#[gpui::test]
async fn test_hide_pending_blame_popover_when_modal_opens(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window
        .read_with(cx, |multi_workspace, _| multi_workspace.workspace().clone())
        .unwrap();
    let multi_buffer = cx.update(|cx| MultiBuffer::build_simple("Buffer Contents!", cx));
    let buffer_id = multi_buffer.read_with(cx, |multi_buffer, cx| {
        multi_buffer
            .all_buffers_iter()
            .next()
            .expect("Should have at least one buffer")
            .read(cx)
            .remote_id()
    });
    let cx = &mut VisualTestContext::from_window(*window, cx);
    let editor = cx.new_window_entity(|window, cx| {
        Editor::new(
            EditorMode::full(),
            multi_buffer,
            Some(project.clone()),
            window,
            cx,
        )
    });

    workspace.update_in(cx, |workspace, window, cx| {
        workspace.add_item_to_active_pane(Box::new(editor.clone()), None, true, window, cx);
    });

    editor.update_in(cx, |editor, _, cx| {
        editor.blame = Some(
            cx.new(|cx| GitBlame::new(editor.buffer.clone(), project.clone(), false, true, cx)),
        );
        editor.show_blame_popover(
            buffer_id,
            &::git::blame::BlameEntry {
                sha: "1b1b1b".parse().unwrap(),
                range: 0..1,
                original_line_number: 0,
                author: None,
                author_mail: None,
                author_time: None,
                author_tz: None,
                committer_name: None,
                committer_email: None,
                committer_time: None,
                committer_tz: None,
                summary: None,
                previous: None,
                filename: String::new(),
            },
            gpui::point(gpui::px(0.), gpui::px(0.)),
            false,
            cx,
        );

        assert!(editor.inline_blame_popover_show_task.is_some());
        assert!(editor.inline_blame_popover.is_none());
    });

    workspace.update_in(cx, |workspace, window, cx| {
        workspace.toggle_modal(window, cx, |_, cx| EmptyModalView::new(cx));
    });

    // Toggling a modal while the blame popover task is still pending should
    // clear both the task and any rendered popover.
    editor.update_in(cx, |editor, _, _| {
        assert!(editor.inline_blame_popover.is_none());
        assert!(editor.inline_blame_popover_show_task.is_none());
    });
}

#[gpui::test]
async fn test_tab_in_leading_whitespace_auto_indents_for_python(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    let language = languages::language("python", tree_sitter_python::LANGUAGE.into());
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    // test cursor move to start of each line on tab
    // for `if`, `elif`, `else`, `while`, `with` and `for`
    cx.set_state(indoc! {"
        def main():
        ˇ    for item in items:
        ˇ        while item.active:
        ˇ            if item.value > 10:
        ˇ                continue
        ˇ            elif item.value < 0:
        ˇ                break
        ˇ            else:
        ˇ                with item.context() as ctx:
        ˇ                    yield count
        ˇ        else:
        ˇ            log('while else')
        ˇ    else:
        ˇ        log('for else')
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def main():
            ˇfor item in items:
                ˇwhile item.active:
                    ˇif item.value > 10:
                        ˇcontinue
                    ˇelif item.value < 0:
                        ˇbreak
                    ˇelse:
                        ˇwith item.context() as ctx:
                            ˇyield count
                ˇelse:
                    ˇlog('while else')
            ˇelse:
                ˇlog('for else')
    "});
    // test relative indent is preserved when tab
    // for `if`, `elif`, `else`, `while`, `with` and `for`
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def main():
                ˇfor item in items:
                    ˇwhile item.active:
                        ˇif item.value > 10:
                            ˇcontinue
                        ˇelif item.value < 0:
                            ˇbreak
                        ˇelse:
                            ˇwith item.context() as ctx:
                                ˇyield count
                    ˇelse:
                        ˇlog('while else')
                ˇelse:
                    ˇlog('for else')
    "});

    // test cursor move to start of each line on tab
    // for `try`, `except`, `else`, `finally`, `match` and `def`
    cx.set_state(indoc! {"
        def main():
        ˇ    try:
        ˇ        fetch()
        ˇ    except ValueError:
        ˇ        handle_error()
        ˇ    else:
        ˇ        match value:
        ˇ            case _:
        ˇ    finally:
        ˇ        def status():
        ˇ            return 0
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def main():
            ˇtry:
                ˇfetch()
            ˇexcept ValueError:
                ˇhandle_error()
            ˇelse:
                ˇmatch value:
                    ˇcase _:
            ˇfinally:
                ˇdef status():
                    ˇreturn 0
    "});
    // test relative indent is preserved when tab
    // for `try`, `except`, `else`, `finally`, `match` and `def`
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def main():
                ˇtry:
                    ˇfetch()
                ˇexcept ValueError:
                    ˇhandle_error()
                ˇelse:
                    ˇmatch value:
                        ˇcase _:
                ˇfinally:
                    ˇdef status():
                        ˇreturn 0
    "});
}

#[gpui::test]
async fn test_outdent_after_input_for_python(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    let language = languages::language("python", tree_sitter_python::LANGUAGE.into());
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    // test `else` auto outdents when typed inside `if` block
    cx.set_state(indoc! {"
        def main():
            if i == 2:
                return
                ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("else:", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def main():
            if i == 2:
                return
            else:ˇ
    "});

    // test `except` auto outdents when typed inside `try` block
    cx.set_state(indoc! {"
        def main():
            try:
                i = 2
                ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("except:", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def main():
            try:
                i = 2
            except:ˇ
    "});

    // test `else` auto outdents when typed inside `except` block
    cx.set_state(indoc! {"
        def main():
            try:
                i = 2
            except:
                j = 2
                ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("else:", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def main():
            try:
                i = 2
            except:
                j = 2
            else:ˇ
    "});

    // test `finally` auto outdents when typed inside `else` block
    cx.set_state(indoc! {"
        def main():
            try:
                i = 2
            except:
                j = 2
            else:
                k = 2
                ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("finally:", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def main():
            try:
                i = 2
            except:
                j = 2
            else:
                k = 2
            finally:ˇ
    "});

    // test `else` does not outdents when typed inside `except` block right after for block
    cx.set_state(indoc! {"
        def main():
            try:
                i = 2
            except:
                for i in range(n):
                    pass
                ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("else:", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def main():
            try:
                i = 2
            except:
                for i in range(n):
                    pass
                else:ˇ
    "});

    // test `finally` auto outdents when typed inside `else` block right after for block
    cx.set_state(indoc! {"
        def main():
            try:
                i = 2
            except:
                j = 2
            else:
                for i in range(n):
                    pass
                ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("finally:", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def main():
            try:
                i = 2
            except:
                j = 2
            else:
                for i in range(n):
                    pass
            finally:ˇ
    "});

    // test `except` outdents to inner "try" block
    cx.set_state(indoc! {"
        def main():
            try:
                i = 2
                if i == 2:
                    try:
                        i = 3
                        ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("except:", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def main():
            try:
                i = 2
                if i == 2:
                    try:
                        i = 3
                    except:ˇ
    "});

    // test `except` outdents to outer "try" block
    cx.set_state(indoc! {"
        def main():
            try:
                i = 2
                if i == 2:
                    try:
                        i = 3
                ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("except:", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def main():
            try:
                i = 2
                if i == 2:
                    try:
                        i = 3
            except:ˇ
    "});

    // test `else` stays at correct indent when typed after `for` block
    cx.set_state(indoc! {"
        def main():
            for i in range(10):
                if i == 3:
                    break
            ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("else:", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def main():
            for i in range(10):
                if i == 3:
                    break
            else:ˇ
    "});

    // test does not outdent on typing after line with square brackets
    cx.set_state(indoc! {"
        def f() -> list[str]:
            ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("a", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        def f() -> list[str]:
            aˇ
    "});

    // test does not outdent on typing : after case keyword
    cx.set_state(indoc! {"
        match 1:
            caseˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input(":", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        match 1:
            case:ˇ
    "});
}

#[gpui::test]
async fn test_indent_on_newline_for_python(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    update_test_language_settings(cx, &|settings| {
        settings.defaults.extend_comment_on_newline = Some(false);
    });
    let mut cx = EditorTestContext::new(cx).await;
    let language = languages::language("python", tree_sitter_python::LANGUAGE.into());
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    // test correct indent after newline on comment
    cx.set_state(indoc! {"
        # COMMENT:ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        # COMMENT:
        ˇ
    "});

    // test correct indent after newline in brackets
    cx.set_state(indoc! {"
        {ˇ}
    "});
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        {
            ˇ
        }
    "});

    cx.set_state(indoc! {"
        (ˇ)
    "});
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.run_until_parked();
    cx.assert_editor_state(indoc! {"
        (
            ˇ
        )
    "});

    // do not indent after empty lists or dictionaries
    cx.set_state(indoc! {"
        a = []ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.run_until_parked();
    cx.assert_editor_state(indoc! {"
        a = []
        ˇ
    "});
}

#[gpui::test]
async fn test_python_indent_in_markdown(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language_registry = Arc::new(language::LanguageRegistry::test(cx.executor()));
    let python_lang = languages::language("python", tree_sitter_python::LANGUAGE.into());
    language_registry.add(markdown_lang());
    language_registry.add(python_lang);

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| {
        buffer.set_language_registry(language_registry);
        buffer.set_language(Some(markdown_lang()), cx);
    });

    // Test that `else:` correctly outdents to match `if:` inside the Python code block
    cx.set_state(indoc! {"
        # Heading

        ```python
        def main():
            if condition:
                pass
                ˇ
        ```
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("else:", window, cx);
    });
    cx.run_until_parked();
    cx.assert_editor_state(indoc! {"
        # Heading

        ```python
        def main():
            if condition:
                pass
            else:ˇ
        ```
    "});
}

#[gpui::test]
async fn test_tab_in_leading_whitespace_auto_indents_for_bash(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    let language = languages::language("bash", tree_sitter_bash::LANGUAGE.into());
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    // test cursor move to start of each line on tab
    // for `if`, `elif`, `else`, `while`, `for`, `case` and `function`
    cx.set_state(indoc! {"
        function main() {
        ˇ    for item in $items; do
        ˇ        while [ -n \"$item\" ]; do
        ˇ            if [ \"$value\" -gt 10 ]; then
        ˇ                continue
        ˇ            elif [ \"$value\" -lt 0 ]; then
        ˇ                break
        ˇ            else
        ˇ                echo \"$item\"
        ˇ            fi
        ˇ        done
        ˇ    done
        ˇ}
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        function main() {
            ˇfor item in $items; do
                ˇwhile [ -n \"$item\" ]; do
                    ˇif [ \"$value\" -gt 10 ]; then
                        ˇcontinue
                    ˇelif [ \"$value\" -lt 0 ]; then
                        ˇbreak
                    ˇelse
                        ˇecho \"$item\"
                    ˇfi
                ˇdone
            ˇdone
        ˇ}
    "});
    // test relative indent is preserved when tab
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        function main() {
                ˇfor item in $items; do
                    ˇwhile [ -n \"$item\" ]; do
                        ˇif [ \"$value\" -gt 10 ]; then
                            ˇcontinue
                        ˇelif [ \"$value\" -lt 0 ]; then
                            ˇbreak
                        ˇelse
                            ˇecho \"$item\"
                        ˇfi
                    ˇdone
                ˇdone
            ˇ}
    "});

    // test cursor move to start of each line on tab
    // for `case` statement with patterns
    cx.set_state(indoc! {"
        function handle() {
        ˇ    case \"$1\" in
        ˇ        start)
        ˇ            echo \"a\"
        ˇ            ;;
        ˇ        stop)
        ˇ            echo \"b\"
        ˇ            ;;
        ˇ        *)
        ˇ            echo \"c\"
        ˇ            ;;
        ˇ    esac
        ˇ}
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        function handle() {
            ˇcase \"$1\" in
                ˇstart)
                    ˇecho \"a\"
                    ˇ;;
                ˇstop)
                    ˇecho \"b\"
                    ˇ;;
                ˇ*)
                    ˇecho \"c\"
                    ˇ;;
            ˇesac
        ˇ}
    "});
}

#[gpui::test]
async fn test_indent_after_input_for_bash(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    let language = languages::language("bash", tree_sitter_bash::LANGUAGE.into());
    cx.update_buffer(|buffer, cx| {
        buffer.set_language(Some(language), cx);
        buffer.set_sync_parse_timeout(None);
    });

    // test indents on comment insert
    cx.set_state(indoc! {"
        function main() {
        ˇ    for item in $items; do
        ˇ        while [ -n \"$item\" ]; do
        ˇ            if [ \"$value\" -gt 10 ]; then
        ˇ                continue
        ˇ            elif [ \"$value\" -lt 0 ]; then
        ˇ                break
        ˇ            else
        ˇ                echo \"$item\"
        ˇ            fi
        ˇ        done
        ˇ    done
        ˇ}
    "});
    cx.update_editor(|e, window, cx| e.handle_input("#", window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        function main() {
        #ˇ    for item in $items; do
        #ˇ        while [ -n \"$item\" ]; do
        #ˇ            if [ \"$value\" -gt 10 ]; then
        #ˇ                continue
        #ˇ            elif [ \"$value\" -lt 0 ]; then
        #ˇ                break
        #ˇ            else
        #ˇ                echo \"$item\"
        #ˇ            fi
        #ˇ        done
        #ˇ    done
        #ˇ}
    "});
}

#[gpui::test]
async fn test_outdent_after_input_for_bash(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    let language = languages::language("bash", tree_sitter_bash::LANGUAGE.into());
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    // test `else` auto outdents when typed inside `if` block
    cx.set_state(indoc! {"
        if [ \"$1\" = \"test\" ]; then
            echo \"foo bar\"
            ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("else", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        if [ \"$1\" = \"test\" ]; then
            echo \"foo bar\"
        elseˇ
    "});

    // test `elif` auto outdents when typed inside `if` block
    cx.set_state(indoc! {"
        if [ \"$1\" = \"test\" ]; then
            echo \"foo bar\"
            ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("elif", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        if [ \"$1\" = \"test\" ]; then
            echo \"foo bar\"
        elifˇ
    "});

    // test `fi` auto outdents when typed inside `else` block
    cx.set_state(indoc! {"
        if [ \"$1\" = \"test\" ]; then
            echo \"foo bar\"
        else
            echo \"bar baz\"
            ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("fi", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        if [ \"$1\" = \"test\" ]; then
            echo \"foo bar\"
        else
            echo \"bar baz\"
        fiˇ
    "});

    // test `done` auto outdents when typed inside `while` block
    cx.set_state(indoc! {"
        while read line; do
            echo \"$line\"
            ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("done", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        while read line; do
            echo \"$line\"
        doneˇ
    "});

    // test `done` auto outdents when typed inside `for` block
    cx.set_state(indoc! {"
        for file in *.txt; do
            cat \"$file\"
            ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("done", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        for file in *.txt; do
            cat \"$file\"
        doneˇ
    "});

    // test `esac` auto outdents when typed inside `case` block
    cx.set_state(indoc! {"
        case \"$1\" in
            start)
                echo \"foo bar\"
                ;;
            stop)
                echo \"bar baz\"
                ;;
            ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("esac", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        case \"$1\" in
            start)
                echo \"foo bar\"
                ;;
            stop)
                echo \"bar baz\"
                ;;
        esacˇ
    "});

    // test `*)` auto outdents when typed inside `case` block
    cx.set_state(indoc! {"
        case \"$1\" in
            start)
                echo \"foo bar\"
                ;;
                ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("*)", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        case \"$1\" in
            start)
                echo \"foo bar\"
                ;;
            *)ˇ
    "});

    // test `fi` outdents to correct level with nested if blocks
    cx.set_state(indoc! {"
        if [ \"$1\" = \"test\" ]; then
            echo \"outer if\"
            if [ \"$2\" = \"debug\" ]; then
                echo \"inner if\"
                ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("fi", window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        if [ \"$1\" = \"test\" ]; then
            echo \"outer if\"
            if [ \"$2\" = \"debug\" ]; then
                echo \"inner if\"
            fiˇ
    "});
}

#[gpui::test]
async fn test_indent_on_newline_for_bash(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    update_test_language_settings(cx, &|settings| {
        settings.defaults.extend_comment_on_newline = Some(false);
    });
    let mut cx = EditorTestContext::new(cx).await;
    let language = languages::language("bash", tree_sitter_bash::LANGUAGE.into());
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));

    // test correct indent after newline on comment
    cx.set_state(indoc! {"
        # COMMENT:ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        # COMMENT:
        ˇ
    "});

    // test correct indent after newline after `then`
    cx.set_state(indoc! {"

        if [ \"$1\" = \"test\" ]; thenˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"

        if [ \"$1\" = \"test\" ]; then
            ˇ
    "});

    // test correct indent after newline after `else`
    cx.set_state(indoc! {"
        if [ \"$1\" = \"test\" ]; then
        elseˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        if [ \"$1\" = \"test\" ]; then
        else
            ˇ
    "});

    // test correct indent after newline after `elif`
    cx.set_state(indoc! {"
        if [ \"$1\" = \"test\" ]; then
        elifˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        if [ \"$1\" = \"test\" ]; then
        elif
            ˇ
    "});

    // test correct indent after newline after `do`
    cx.set_state(indoc! {"
        for file in *.txt; doˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        for file in *.txt; do
            ˇ
    "});

    // test correct indent after newline after case pattern
    cx.set_state(indoc! {"
        case \"$1\" in
            start)ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        case \"$1\" in
            start)
                ˇ
    "});

    // test correct indent after newline after case pattern
    cx.set_state(indoc! {"
        case \"$1\" in
            start)
                ;;
            *)ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        case \"$1\" in
            start)
                ;;
            *)
                ˇ
    "});

    // test correct indent after newline after function opening brace
    cx.set_state(indoc! {"
        function test() {ˇ}
    "});
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        function test() {
            ˇ
        }
    "});

    // test no extra indent after semicolon on same line
    cx.set_state(indoc! {"
        echo \"test\";ˇ
    "});
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        echo \"test\";
        ˇ
    "});
}

fn empty_range(row: usize, column: usize) -> Range<DisplayPoint> {
    let point = DisplayPoint::new(DisplayRow(row as u32), column as u32);
    point..point
}

#[track_caller]
fn assert_selection_ranges(marked_text: &str, editor: &mut Editor, cx: &mut Context<Editor>) {
    let (text, ranges) = marked_text_ranges(marked_text, true);
    assert_eq!(editor.text(cx), text);
    assert_eq!(
        editor.selections.ranges(&editor.display_snapshot(cx)),
        ranges
            .iter()
            .map(|range| MultiBufferOffset(range.start)..MultiBufferOffset(range.end))
            .collect::<Vec<_>>(),
        "Assert selections are {}",
        marked_text
    );
}

pub(crate) fn update_test_language_settings(
    cx: &mut TestAppContext,
    f: &dyn Fn(&mut AllLanguageSettingsContent),
) {
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, &|settings: &mut SettingsContent| {
                f(&mut settings.project.all_languages)
            });
        });
    });
}

pub(crate) fn update_test_editor_settings(
    cx: &mut TestAppContext,
    f: &dyn Fn(&mut EditorSettingsContent),
) {
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| f(&mut settings.editor));
        })
    })
}

pub(crate) fn init_test(cx: &mut TestAppContext, f: fn(&mut AllLanguageSettingsContent)) {
    cx.update(|cx| {
        assets::Assets.load_test_fonts(cx);
        let store = SettingsStore::test(cx);
        cx.set_global(store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        release_channel::init(semver::Version::new(0, 0, 0), cx);
        crate::init(cx);
    });
    zlog::init_test();
    update_test_language_settings(cx, &f);
}

#[track_caller]
fn assert_hunk_revert(
    not_reverted_text_with_selections: &str,
    expected_hunk_statuses_before: Vec<DiffHunkStatusKind>,
    expected_reverted_text_with_selections: &str,
    base_text: &str,
    cx: &mut EditorLanguageTestContext,
) {
    cx.set_state(not_reverted_text_with_selections);
    cx.set_head_text(base_text);
    cx.executor().run_until_parked();

    let actual_hunk_statuses_before = cx.update_editor(|editor, window, cx| {
        let snapshot = editor.snapshot(window, cx);
        let reverted_hunk_statuses = snapshot
            .buffer_snapshot()
            .diff_hunks_in_range(MultiBufferOffset(0)..snapshot.buffer_snapshot().len())
            .map(|hunk| hunk.status().kind)
            .collect::<Vec<_>>();

        editor.git_restore(&Default::default(), window, cx);
        reverted_hunk_statuses
    });
    cx.executor().run_until_parked();
    cx.assert_editor_state(expected_reverted_text_with_selections);
    assert_eq!(actual_hunk_statuses_before, expected_hunk_statuses_before);
}

#[gpui::test]
async fn test_add_selection_after_moving_with_multiple_cursors(cx: &mut TestAppContext) {
    // Regression test for issue #11671
    // Previously, adding a cursor after moving multiple cursors would reset
    // the cursor count instead of adding to the existing cursors.
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    // Create a simple buffer with cursor at start
    cx.set_state(indoc! {"
        ˇaaaa
        bbbb
        cccc
        dddd
        eeee
        ffff
        gggg
        hhhh"});

    // Add 2 cursors below (so we have 3 total)
    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
        editor.add_selection_below(&Default::default(), window, cx);
    });

    // Verify we have 3 cursors
    let initial_count = cx.update_editor(|editor, _, _| editor.selections.count());
    assert_eq!(
        initial_count, 3,
        "Should have 3 cursors after adding 2 below"
    );

    // Move down one line
    cx.update_editor(|editor, window, cx| {
        editor.move_down(&MoveDown, window, cx);
    });

    // Add another cursor below
    cx.update_editor(|editor, window, cx| {
        editor.add_selection_below(&Default::default(), window, cx);
    });

    // Should now have 4 cursors (3 original + 1 new)
    let final_count = cx.update_editor(|editor, _, _| editor.selections.count());
    assert_eq!(
        final_count, 4,
        "Should have 4 cursors after moving and adding another"
    );
}

#[gpui::test]
async fn test_add_selection_skip_soft_wrap_option(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc!(
        r#"ˇThis is a very long line that will be wrapped when soft wrapping is enabled
           Second line here"#
    ));

    cx.update_editor(|editor, window, cx| {
        // Enable soft wrapping with a narrow width to force soft wrapping and
        // confirm that more than 2 rows are being displayed.
        editor.set_wrap_width(Some(100.0.into()), cx);
        assert!(editor.display_text(cx).lines().count() > 2);

        editor.add_selection_below(
            &AddSelectionBelow {
                skip_soft_wrap: true,
            },
            window,
            cx,
        );

        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(8), 0)..DisplayPoint::new(DisplayRow(8), 0),
            ]
        );

        editor.add_selection_above(
            &AddSelectionAbove {
                skip_soft_wrap: true,
            },
            window,
            cx,
        );

        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0)]
        );

        editor.add_selection_below(
            &AddSelectionBelow {
                skip_soft_wrap: false,
            },
            window,
            cx,
        );

        assert_eq!(
            display_ranges(editor, cx),
            &[
                DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0),
                DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0),
            ]
        );

        editor.add_selection_above(
            &AddSelectionAbove {
                skip_soft_wrap: false,
            },
            window,
            cx,
        );

        assert_eq!(
            display_ranges(editor, cx),
            &[DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0)]
        );
    });

    // Set up text where selections are in the middle of a soft-wrapped line.
    // When adding selection below with `skip_soft_wrap` set to `true`, the new
    // selection should be at the same buffer column, not the same pixel
    // position.
    cx.set_state(indoc!(
        r#"1. Very long line to show «howˇ» a wrapped line would look
           2. Very long line to show how a wrapped line would look"#
    ));

    cx.update_editor(|editor, window, cx| {
        // Enable soft wrapping with a narrow width to force soft wrapping and
        // confirm that more than 2 rows are being displayed.
        editor.set_wrap_width(Some(100.0.into()), cx);
        assert!(editor.display_text(cx).lines().count() > 2);

        editor.add_selection_below(
            &AddSelectionBelow {
                skip_soft_wrap: true,
            },
            window,
            cx,
        );

        // Assert that there's now 2 selections, both selecting the same column
        // range in the buffer row.
        let display_map = editor.display_map.update(cx, |map, cx| map.snapshot(cx));
        let selections = editor.selections.all::<Point>(&display_map);
        assert_eq!(selections.len(), 2);
        assert_eq!(selections[0].start.column, selections[1].start.column);
        assert_eq!(selections[0].end.column, selections[1].end.column);
    });
}

#[gpui::test]
async fn test_insert_snippet(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    cx.update_editor(|editor, _, cx| {
        editor.project().unwrap().update(cx, |project, cx| {
            project.snippets().update(cx, |snippets, _cx| {
                let snippet = project::snippet_provider::Snippet {
                    prefix: vec![], // no prefix needed!
                    body: "an Unspecified".to_string(),
                    description: Some("shhhh it's a secret".to_string()),
                    name: "super secret snippet".to_string(),
                };
                snippets.add_snippet_for_test(
                    None,
                    PathBuf::from("test_snippets.json"),
                    vec![Arc::new(snippet)],
                );

                let snippet = project::snippet_provider::Snippet {
                    prefix: vec![], // no prefix needed!
                    body: " Location".to_string(),
                    description: Some("the word 'location'".to_string()),
                    name: "location word".to_string(),
                };
                snippets.add_snippet_for_test(
                    Some("Markdown".to_string()),
                    PathBuf::from("test_snippets.json"),
                    vec![Arc::new(snippet)],
                );
            });
        })
    });

    cx.set_state(indoc!(r#"First cursor at ˇ and second cursor at ˇ"#));

    cx.update_editor(|editor, window, cx| {
        editor.insert_snippet_at_selections(
            &InsertSnippet {
                language: None,
                name: Some("super secret snippet".to_string()),
                snippet: None,
            },
            window,
            cx,
        );

        // Language is specified in the action,
        // so the buffer language does not need to match
        editor.insert_snippet_at_selections(
            &InsertSnippet {
                language: Some("Markdown".to_string()),
                name: Some("location word".to_string()),
                snippet: None,
            },
            window,
            cx,
        );

        editor.insert_snippet_at_selections(
            &InsertSnippet {
                language: None,
                name: None,
                snippet: Some("$0 after".to_string()),
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(
        r#"First cursor at an Unspecified Locationˇ after and second cursor at an Unspecified Locationˇ after"#,
    );
}

#[gpui::test]
async fn test_newline_replacement_in_single_line(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let (editor, cx) = cx.add_window_view(Editor::single_line);
    editor.update_in(cx, |editor, window, cx| {
        editor.set_text("oops\n\nwow\n", window, cx)
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        assert_eq!(editor.display_text(cx), "oops⋯⋯wow⋯");
    });
    editor.update(cx, |editor, cx| {
        editor.edit([(MultiBufferOffset(3)..MultiBufferOffset(5), "")], cx)
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        assert_eq!(editor.display_text(cx), "oop⋯wow⋯");
    });
}

#[gpui::test]
async fn test_non_utf_8_opens(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    cx.update(|cx| {
        register_project_item::<Editor>(cx);
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root1", json!({})).await;
    fs.insert_file("/root1/one.pdf", vec![0xff, 0xfe, 0xfd])
        .await;

    let project = Project::test(fs, ["/root1".as_ref()], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    let worktree_id = project.update(cx, |project, cx| {
        project.worktrees(cx).next().unwrap().read(cx).id()
    });

    let handle = workspace
        .update_in(cx, |workspace, window, cx| {
            let project_path = (worktree_id, rel_path("one.pdf"));
            workspace.open_path(project_path, None, true, window, cx)
        })
        .await
        .unwrap();
    // The test file content `vec![0xff, 0xfe, ...]` starts with a UTF-16 LE BOM.
    // Previously, this fell back to `InvalidItemView` because it wasn't valid UTF-8.
    // With auto-detection enabled, this is now recognized as UTF-16 and opens in the Editor.
    assert_eq!(handle.to_any_view().entity_type(), TypeId::of::<Editor>());
}

#[gpui::test]
async fn test_select_next_prev_syntax_node(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(Language::new(
        LanguageConfig::default(),
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    // Test hierarchical sibling navigation
    let text = r#"
        fn outer() {
            if condition {
                let a = 1;
            }
            let b = 2;
        }

        fn another() {
            let c = 3;
        }
    "#;

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    // Wait for parsing to complete
    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        // Start by selecting "let a = 1;" inside the if block
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(3), 16)..DisplayPoint::new(DisplayRow(3), 26)
            ]);
        });

        let initial_selection = editor
            .selections
            .display_ranges(&editor.display_snapshot(cx));
        assert_eq!(initial_selection.len(), 1, "Should have one selection");

        // Test select next sibling - should move up levels to find the next sibling
        // Since "let a = 1;" has no siblings in the if block, it should move up
        // to find "let b = 2;" which is a sibling of the if block
        editor.select_next_syntax_node(&SelectNextSyntaxNode, window, cx);
        let next_selection = editor
            .selections
            .display_ranges(&editor.display_snapshot(cx));

        // Should have a selection and it should be different from the initial
        assert_eq!(
            next_selection.len(),
            1,
            "Should have one selection after next"
        );
        assert_ne!(
            next_selection[0], initial_selection[0],
            "Next sibling selection should be different"
        );

        // Test hierarchical navigation by going to the end of the current function
        // and trying to navigate to the next function
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(5), 12)..DisplayPoint::new(DisplayRow(5), 22)
            ]);
        });

        editor.select_next_syntax_node(&SelectNextSyntaxNode, window, cx);
        let function_next_selection = editor
            .selections
            .display_ranges(&editor.display_snapshot(cx));

        // Should move to the next function
        assert_eq!(
            function_next_selection.len(),
            1,
            "Should have one selection after function next"
        );

        // Test select previous sibling navigation
        editor.select_prev_syntax_node(&SelectPreviousSyntaxNode, window, cx);
        let prev_selection = editor
            .selections
            .display_ranges(&editor.display_snapshot(cx));

        // Should have a selection and it should be different
        assert_eq!(
            prev_selection.len(),
            1,
            "Should have one selection after prev"
        );
        assert_ne!(
            prev_selection[0], function_next_selection[0],
            "Previous sibling selection should be different from next"
        );
    });
}

#[gpui::test]
async fn test_paste_url_from_other_app_creates_markdown_link_over_selected_text(
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx, |_| {});

    let url = "https://zed.dev";

    let markdown_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Markdown".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_language), cx));
    cx.set_state("Hello, «editorˇ».\nZed is «ˇgreat» (see this link: ˇ)");

    cx.update_editor(|editor, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(url.to_string()));
        editor.paste(&Paste, window, cx);
    });

    cx.assert_editor_state(&format!(
        "Hello, [editor]({url})ˇ.\nZed is [great]({url})ˇ (see this link: {url}ˇ)"
    ));
}

#[gpui::test]
async fn test_markdown_indents(cx: &mut gpui::TestAppContext) {
    init_test(cx, |_| {});

    let markdown_language = languages::language("markdown", tree_sitter_md::LANGUAGE.into());
    let mut cx = EditorTestContext::new(cx).await;

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_language), cx));

    // Case 1: Test if adding a character with multi cursors preserves nested list indents
    cx.set_state(&indoc! {"
        - [ ] Item 1
            - [ ] Item 1.a
        - [ˇ] Item 2
            - [ˇ] Item 2.a
            - [ˇ] Item 2.b
        "
    });
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("x", window, cx);
    });
    cx.run_until_parked();
    cx.assert_editor_state(indoc! {"
        - [ ] Item 1
            - [ ] Item 1.a
        - [xˇ] Item 2
            - [xˇ] Item 2.a
            - [xˇ] Item 2.b
        "
    });

    // Case 2: Test adding new line after nested list continues the list with unchecked task
    cx.set_state(&indoc! {"
        - [ ] Item 1
            - [ ] Item 1.a
        - [x] Item 2
            - [x] Item 2.a
            - [x] Item 2.bˇ"
    });
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.assert_editor_state(indoc! {"
        - [ ] Item 1
            - [ ] Item 1.a
        - [x] Item 2
            - [x] Item 2.a
            - [x] Item 2.b
            - [ ] ˇ"
    });

    // Case 3: Test adding content to continued list item
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("Item 2.c", window, cx);
    });
    cx.run_until_parked();
    cx.assert_editor_state(indoc! {"
        - [ ] Item 1
            - [ ] Item 1.a
        - [x] Item 2
            - [x] Item 2.a
            - [x] Item 2.b
            - [ ] Item 2.cˇ"
    });

    // Case 4: Test adding new line after nested ordered list continues with next number
    cx.set_state(indoc! {"
        1. Item 1
            1. Item 1.a
        2. Item 2
            1. Item 2.a
            2. Item 2.bˇ"
    });
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.assert_editor_state(indoc! {"
        1. Item 1
            1. Item 1.a
        2. Item 2
            1. Item 2.a
            2. Item 2.b
            3. ˇ"
    });

    // Case 5: Adding content to continued ordered list item
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("Item 2.c", window, cx);
    });
    cx.run_until_parked();
    cx.assert_editor_state(indoc! {"
        1. Item 1
            1. Item 1.a
        2. Item 2
            1. Item 2.a
            2. Item 2.b
            3. Item 2.cˇ"
    });

    // Case 6: Test adding new line after nested ordered list preserves indent of previous line
    cx.set_state(indoc! {"
        - Item 1
            - Item 1.a
            - Item 1.a
        ˇ"});
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("-", window, cx);
    });
    cx.run_until_parked();
    cx.assert_editor_state(indoc! {"
        - Item 1
            - Item 1.a
            - Item 1.a
        -ˇ"});

    // Case 7: Test blockquote newline preserves something
    cx.set_state(indoc! {"
        > Item 1ˇ"
    });
    cx.update_editor(|editor, window, cx| {
        editor.newline(&Newline, window, cx);
    });
    cx.assert_editor_state(indoc! {"
        > Item 1
        ˇ"
    });
}

#[gpui::test]
async fn test_paste_url_from_zed_copy_creates_markdown_link_over_selected_text(
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx, |_| {});

    let url = "https://zed.dev";

    let markdown_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Markdown".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_language), cx));
    cx.set_state(&format!(
        "Hello, editor.\nZed is great (see this link: )\n«{url}ˇ»"
    ));

    cx.update_editor(|editor, window, cx| {
        editor.copy(&Copy, window, cx);
    });

    cx.set_state(&format!(
        "Hello, «editorˇ».\nZed is «ˇgreat» (see this link: ˇ)\n{url}"
    ));

    cx.update_editor(|editor, window, cx| {
        editor.paste(&Paste, window, cx);
    });

    cx.assert_editor_state(&format!(
        "Hello, [editor]({url})ˇ.\nZed is [great]({url})ˇ (see this link: {url}ˇ)\n{url}"
    ));
}

#[gpui::test]
async fn test_paste_url_from_other_app_replaces_existing_url_without_creating_markdown_link(
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx, |_| {});

    let url = "https://zed.dev";

    let markdown_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Markdown".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_language), cx));
    cx.set_state("Please visit zed's homepage: «https://www.apple.comˇ»");

    cx.update_editor(|editor, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(url.to_string()));
        editor.paste(&Paste, window, cx);
    });

    cx.assert_editor_state(&format!("Please visit zed's homepage: {url}ˇ"));
}

#[gpui::test]
async fn test_paste_plain_text_from_other_app_replaces_selection_without_creating_markdown_link(
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx, |_| {});

    let text = "Awesome";

    let markdown_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Markdown".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_language), cx));
    cx.set_state("Hello, «editorˇ».\nZed is «ˇgreat»");

    cx.update_editor(|editor, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        editor.paste(&Paste, window, cx);
    });

    cx.assert_editor_state(&format!("Hello, {text}ˇ.\nZed is {text}ˇ"));
}

#[gpui::test]
async fn test_paste_text_with_scheme_like_prefix_replaces_selection_without_creating_markdown_link(
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx, |_| {});

    // `url::Url::parse` accepts this as a URL with the scheme `editor`, but it
    // should not be treated as one when pasting.
    let text = "editor: Fix double-click bracket selection for large spans";

    let markdown_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Markdown".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_language), cx));
    cx.set_state("«(feat on git-ui-add-info-exclude-to-context-menus) Fmtˇ»");

    cx.update_editor(|editor, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        editor.paste(&Paste, window, cx);
    });

    cx.assert_editor_state(&format!("{text}ˇ"));
}

#[gpui::test]
async fn test_paste_url_from_other_app_without_creating_markdown_link_in_non_markdown_language(
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx, |_| {});

    let url = "https://zed.dev";

    let markdown_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Rust".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_language), cx));
    cx.set_state("// Hello, «editorˇ».\n// Zed is «ˇgreat» (see this link: ˇ)");

    cx.update_editor(|editor, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(url.to_string()));
        editor.paste(&Paste, window, cx);
    });

    cx.assert_editor_state(&format!(
        "// Hello, {url}ˇ.\n// Zed is {url}ˇ (see this link: {url}ˇ)"
    ));
}

#[gpui::test]
async fn test_paste_url_from_other_app_creates_markdown_link_selectively_in_multi_buffer(
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let url = "https://zed.dev";

    let markdown_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Markdown".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    let (editor, cx) = cx.add_window_view(|window, cx| {
        let multi_buffer = MultiBuffer::build_multi(
            [
                ("this will embed -> link", vec![Point::row_range(0..1)]),
                ("this will replace -> link", vec![Point::row_range(0..1)]),
            ],
            cx,
        );
        let mut editor = Editor::new(EditorMode::full(), multi_buffer.clone(), None, window, cx);
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges(vec![
                Point::new(0, 19)..Point::new(0, 23),
                Point::new(1, 21)..Point::new(1, 25),
            ])
        });
        let snapshot = multi_buffer.read(cx).snapshot(cx);
        let first_buffer_id = snapshot.all_buffer_ids().next().unwrap();
        let first_buffer = multi_buffer.read(cx).buffer(first_buffer_id).unwrap();
        first_buffer.update(cx, |buffer, cx| {
            buffer.set_language(Some(markdown_language.clone()), cx);
        });

        editor
    });
    let mut cx = EditorTestContext::for_editor_in(editor.clone(), cx).await;

    cx.update_editor(|editor, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(url.to_string()));
        editor.paste(&Paste, window, cx);
    });

    cx.assert_editor_state(&format!(
        "this will embed -> [link]({url})ˇ\nthis will replace -> {url}ˇ"
    ));
}

#[gpui::test]
async fn test_paste_image_in_markdown_saves_file_and_inserts_markdown(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let markdown_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Markdown".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/test", serde_json::json!({"test.md": ""}))
        .await;
    let project = Project::test(fs.clone(), [std::path::Path::new("/test")], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/test/test.md", cx)
        })
        .await
        .unwrap();
    buffer.update(cx, |buffer, cx| {
        buffer.set_language(Some(markdown_language), cx);
    });

    let editor_window = cx.add_window(|window, cx| {
        let editor = build_editor_with_project(
            project,
            MultiBuffer::build_from_buffer(buffer, cx),
            window,
            cx,
        );
        window.focus(&editor.focus_handle(cx), cx);
        editor
    });
    cx.run_until_parked();
    let mut cx = EditorTestContext::for_editor(editor_window, cx).await;

    let png_bytes: Vec<u8> = vec![1, 2, 3, 4, 5];
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, png_bytes.clone());

    cx.set_state("ˇ");
    cx.update_editor(|editor, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&image));
        editor.paste(&Paste, window, cx);
    });
    cx.run_until_parked();

    let buffer_text = cx.buffer_text();
    assert!(
        buffer_text.starts_with("![](image") && buffer_text.ends_with(".png)"),
        "expected markdown image syntax, got: {buffer_text:?}"
    );

    // Cursor should land inside [] so the user can type alt text immediately.
    let filename_in_parens = &buffer_text["![](".len()..buffer_text.len() - 1];
    let expected_state = format!("![ˇ]({filename_in_parens})");
    cx.assert_editor_state(&expected_state);

    let filename = &buffer_text["![](".len()..buffer_text.len() - 1];
    assert_eq!(
        fs.read_file_sync(format!("/test/{filename}")).unwrap(),
        png_bytes,
        "image file contents should match clipboard bytes"
    );
}

#[gpui::test]
async fn test_paste_multiple_images_in_markdown_increments_filename(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let markdown_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Markdown".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/test", serde_json::json!({"test.md": ""}))
        .await;
    let project = Project::test(fs.clone(), [std::path::Path::new("/test")], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/test/test.md", cx)
        })
        .await
        .unwrap();
    buffer.update(cx, |buffer, cx| {
        buffer.set_language(Some(markdown_language), cx);
    });

    let editor_window = cx.add_window(|window, cx| {
        let editor = build_editor_with_project(
            project,
            MultiBuffer::build_from_buffer(buffer, cx),
            window,
            cx,
        );
        window.focus(&editor.focus_handle(cx), cx);
        editor
    });
    cx.run_until_parked();
    let mut cx = EditorTestContext::for_editor(editor_window, cx).await;

    let png_bytes_1: Vec<u8> = vec![1, 2, 3, 4, 5];
    let png_bytes_2: Vec<u8> = vec![6, 7, 8, 9, 10];
    let png_bytes_3: Vec<u8> = vec![11, 12, 13, 14, 15];
    let image_1 = gpui::Image::from_bytes(gpui::ImageFormat::Png, png_bytes_1.clone());
    let image_2 = gpui::Image::from_bytes(gpui::ImageFormat::Png, png_bytes_2.clone());
    let image_3 = gpui::Image::from_bytes(gpui::ImageFormat::Png, png_bytes_3.clone());

    // Paste first image — should produce image.png
    cx.set_state("ˇ");
    cx.update_editor(|editor, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&image_1));
        editor.paste(&Paste, window, cx);
    });
    cx.run_until_parked();
    let text_after_first = cx.buffer_text();
    assert_eq!(
        text_after_first, "![](image.png)",
        "first paste should produce image.png"
    );
    assert_eq!(fs.read_file_sync("/test/image.png").unwrap(), png_bytes_1);

    // Paste second image at end — should produce image_1.png
    cx.update_editor(|editor, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&image_2));
        editor.paste(&Paste, window, cx);
    });
    cx.run_until_parked();
    let text_after_second = cx.buffer_text();
    assert!(
        text_after_second.contains("![](image_1.png)"),
        "second paste should produce image_1.png, got: {text_after_second:?}"
    );
    assert_eq!(fs.read_file_sync("/test/image_1.png").unwrap(), png_bytes_2);

    // Paste third image — should produce image_2.png
    cx.update_editor(|editor, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&image_3));
        editor.paste(&Paste, window, cx);
    });
    cx.run_until_parked();
    let text_after_third = cx.buffer_text();
    assert!(
        text_after_third.contains("![](image_2.png)"),
        "third paste should produce image_2.png, got: {text_after_third:?}"
    );
    assert_eq!(fs.read_file_sync("/test/image_2.png").unwrap(), png_bytes_3);
}

#[gpui::test]
async fn test_paste_image_in_non_markdown_does_not_insert_markdown(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let rust_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Rust".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(rust_language), cx));
    cx.set_state("ˇ");

    let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, vec![1, 2, 3]);
    cx.update_editor(|editor, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&image));
        editor.paste(&Paste, window, cx);
    });

    cx.assert_editor_state("ˇ");
}

#[gpui::test]
async fn test_paste_image_in_markdown_without_open_file_falls_through(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let markdown_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Markdown".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    // Build an editor whose buffer has no associated file path on disk.
    let (editor, cx) = cx.add_window_view(|window, cx| {
        let buffer = cx.new(|cx| language::Buffer::local("", cx));
        buffer.update(cx, |buffer, cx| {
            buffer.set_language(Some(markdown_language), cx);
        });
        let multibuffer = MultiBuffer::build_from_buffer(buffer, cx);
        build_editor(multibuffer, window, cx)
    });
    let mut cx = EditorTestContext::for_editor_in(editor, cx).await;

    cx.set_state("ˇ");
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, vec![1, 2, 3]);
    cx.update_editor(|editor, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&image));
        editor.paste(&Paste, window, cx);
    });

    cx.assert_editor_state("ˇ");
}

#[gpui::test]
async fn test_paste_image_in_markdown_single_file_worktree_falls_through(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let markdown_language = Arc::new(Language::new(
        LanguageConfig {
            name: "Markdown".into(),
            ..LanguageConfig::default()
        },
        None,
    ));

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", serde_json::json!({"test.md": ""}))
        .await;
    let project = Project::test(fs.clone(), [std::path::Path::new("/root/test.md")], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/root/test.md", cx)
        })
        .await
        .unwrap();
    buffer.update(cx, |buffer, cx| {
        buffer.set_language(Some(markdown_language), cx);
    });

    let editor_window = cx.add_window(|window, cx| {
        let editor = build_editor_with_project(
            project,
            MultiBuffer::build_from_buffer(buffer, cx),
            window,
            cx,
        );
        window.focus(&editor.focus_handle(cx), cx);
        editor
    });
    cx.run_until_parked();
    let mut cx = EditorTestContext::for_editor(editor_window, cx).await;

    cx.set_state("ˇ");
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, vec![1, 2, 3]);
    cx.update_editor(|editor, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&image));
        editor.paste(&Paste, window, cx);
    });
    cx.run_until_parked();

    cx.assert_editor_state("ˇ");
    assert!(
        fs.read_file_sync("/root/image.png").is_err(),
        "no image file should be created for a single-file worktree"
    );
}

#[gpui::test]
fn test_duplicate_line_up_on_last_line_without_newline(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = MultiBuffer::build_simple("line1\nline2", cx);
        build_editor(buffer, window, cx)
    });

    editor
        .update(cx, |editor, window, cx| {
            editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
                s.select_display_ranges([
                    DisplayPoint::new(DisplayRow(1), 0)..DisplayPoint::new(DisplayRow(1), 0)
                ])
            });

            editor.duplicate_line_up(&DuplicateLineUp, window, cx);

            assert_eq!(
                editor.display_text(cx),
                "line1\nline2\nline2",
                "Duplicating last line upward should create duplicate above, not on same line"
            );

            assert_eq!(
                editor
                    .selections
                    .display_ranges(&editor.display_snapshot(cx)),
                vec![DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 0)],
                "Selection should move to the duplicated line"
            );
        })
        .unwrap();
}

#[gpui::test]
async fn test_copy_line_without_trailing_newline(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("line1\nline2ˇ");

    cx.update_editor(|e, window, cx| e.copy(&Copy, window, cx));

    let clipboard_text = cx
        .read_from_clipboard()
        .and_then(|item| item.text().as_deref().map(str::to_string));

    assert_eq!(
        clipboard_text,
        Some("line2\n".to_string()),
        "Copying a line without trailing newline should include a newline"
    );

    cx.set_state("line1\nˇ");

    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));

    cx.assert_editor_state("line1\nline2\nˇ");
}

#[gpui::test]
async fn test_multi_selection_copy_with_newline_between_copied_lines(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("ˇline1\nˇline2\nˇline3\n");

    cx.update_editor(|e, window, cx| e.copy(&Copy, window, cx));

    let clipboard_text = cx
        .read_from_clipboard()
        .and_then(|item| item.text().as_deref().map(str::to_string));

    assert_eq!(
        clipboard_text,
        Some("line1\nline2\nline3\n".to_string()),
        "Copying multiple lines should include a single newline between lines"
    );

    cx.set_state("lineA\nˇ");

    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));

    cx.assert_editor_state("lineA\nline1\nline2\nline3\nˇ");
}

#[gpui::test]
async fn test_multi_selection_cut_with_newline_between_copied_lines(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("ˇline1\nˇline2\nˇline3\n");

    cx.update_editor(|e, window, cx| e.cut(&Cut, window, cx));

    let clipboard_text = cx
        .read_from_clipboard()
        .and_then(|item| item.text().as_deref().map(str::to_string));

    assert_eq!(
        clipboard_text,
        Some("line1\nline2\nline3\n".to_string()),
        "Copying multiple lines should include a single newline between lines"
    );

    cx.set_state("lineA\nˇ");

    cx.update_editor(|e, window, cx| e.paste(&Paste, window, cx));

    cx.assert_editor_state("lineA\nline1\nline2\nline3\nˇ");
}

#[gpui::test]
async fn test_end_of_editor_context(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state("line1\nline2ˇ");
    cx.update_editor(|e, window, cx| {
        e.set_mode(EditorMode::SingleLine);
        assert!(!e.key_context(window, cx).contains("start_of_input"));
        assert!(e.key_context(window, cx).contains("end_of_input"));
    });
    cx.set_state("ˇline1\nline2");
    cx.update_editor(|e, window, cx| {
        e.set_mode(EditorMode::SingleLine);
        assert!(e.key_context(window, cx).contains("start_of_input"));
        assert!(!e.key_context(window, cx).contains("end_of_input"));
    });
    cx.set_state("line1ˇ\nline2");
    cx.update_editor(|e, window, cx| {
        e.set_mode(EditorMode::SingleLine);
        assert!(!e.key_context(window, cx).contains("start_of_input"));
        assert!(!e.key_context(window, cx).contains("end_of_input"));
    });

    cx.set_state("line1\nline2ˇ");
    cx.update_editor(|e, window, cx| {
        e.set_mode(EditorMode::AutoHeight {
            min_lines: 1,
            max_lines: Some(4),
        });
        assert!(!e.key_context(window, cx).contains("start_of_input"));
        assert!(e.key_context(window, cx).contains("end_of_input"));
    });
    cx.set_state("ˇline1\nline2");
    cx.update_editor(|e, window, cx| {
        e.set_mode(EditorMode::AutoHeight {
            min_lines: 1,
            max_lines: Some(4),
        });
        assert!(e.key_context(window, cx).contains("start_of_input"));
        assert!(!e.key_context(window, cx).contains("end_of_input"));
    });
    cx.set_state("line1ˇ\nline2");
    cx.update_editor(|e, window, cx| {
        e.set_mode(EditorMode::AutoHeight {
            min_lines: 1,
            max_lines: Some(4),
        });
        assert!(!e.key_context(window, cx).contains("start_of_input"));
        assert!(!e.key_context(window, cx).contains("end_of_input"));
    });
}

#[gpui::test]
async fn test_sticky_scroll(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let buffer = indoc! {"
            ˇfn foo() {
                let abc = 123;
            }
            struct Bar;
            impl Bar {
                fn new() -> Self {
                    Self
                }
            }
            fn baz() {
            }
        "};
    cx.set_state(&buffer);

    cx.update_editor(|e, _, cx| {
        e.buffer()
            .read(cx)
            .as_singleton()
            .unwrap()
            .update(cx, |buffer, cx| {
                buffer.set_language(Some(rust_lang()), cx);
            })
    });

    let mut sticky_headers = |offset: ScrollOffset| {
        cx.update_editor(|e, window, cx| {
            e.scroll(gpui::Point { x: 0., y: offset }, window, cx);
        });
        cx.run_until_parked();
        cx.update_editor(|e, window, cx| {
            EditorElement::sticky_headers(&e, &e.snapshot(window, cx))
                .into_iter()
                .map(
                    |StickyHeader {
                         start_point,
                         offset,
                         ..
                     }| { (start_point, offset) },
                )
                .collect::<Vec<_>>()
        })
    };

    let fn_foo = Point { row: 0, column: 0 };
    let impl_bar = Point { row: 4, column: 0 };
    let fn_new = Point { row: 5, column: 4 };

    assert_eq!(sticky_headers(0.0), vec![]);
    assert_eq!(sticky_headers(0.5), vec![(fn_foo, 0.0)]);
    assert_eq!(sticky_headers(1.0), vec![(fn_foo, 0.0)]);
    assert_eq!(sticky_headers(1.5), vec![(fn_foo, -0.5)]);
    assert_eq!(sticky_headers(2.0), vec![]);
    assert_eq!(sticky_headers(2.5), vec![]);
    assert_eq!(sticky_headers(3.0), vec![]);
    assert_eq!(sticky_headers(3.5), vec![]);
    assert_eq!(sticky_headers(4.0), vec![]);
    assert_eq!(sticky_headers(4.5), vec![(impl_bar, 0.0), (fn_new, 1.0)]);
    assert_eq!(sticky_headers(5.0), vec![(impl_bar, 0.0), (fn_new, 1.0)]);
    assert_eq!(sticky_headers(5.5), vec![(impl_bar, 0.0), (fn_new, 0.5)]);
    assert_eq!(sticky_headers(6.0), vec![(impl_bar, 0.0)]);
    assert_eq!(sticky_headers(6.5), vec![(impl_bar, 0.0)]);
    assert_eq!(sticky_headers(7.0), vec![(impl_bar, 0.0)]);
    assert_eq!(sticky_headers(7.5), vec![(impl_bar, -0.5)]);
    assert_eq!(sticky_headers(8.0), vec![]);
    assert_eq!(sticky_headers(8.5), vec![]);
    assert_eq!(sticky_headers(9.0), vec![]);
    assert_eq!(sticky_headers(9.5), vec![]);
    assert_eq!(sticky_headers(10.0), vec![]);
}

#[gpui::test]
async fn test_sticky_scroll_with_decoration_prefix_in_item(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let language = Arc::new(
        Language::new(
            LanguageConfig {
                name: "TypeScript".into(),
                ..Default::default()
            },
            Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        )
        .with_outline_query(
            r#"
            (class_declaration
                "class" @context
                name: (_) @name) @item
            "#,
        )
        .expect("TypeScript outline query"),
    );

    let buffer = indoc! {"
        ˇ@Decorator
        class Foo {
            x = 1;
            y = 2;
            z = 3;
            w = 4;
        }
    "};
    cx.set_state(buffer);
    cx.update_editor(|e, _, cx| {
        e.buffer()
            .read(cx)
            .as_singleton()
            .unwrap()
            .update(cx, |buffer, cx| {
                buffer.set_language(Some(language), cx);
            })
    });

    let mut sticky_headers = |offset: ScrollOffset| {
        cx.update_editor(|e, window, cx| {
            e.scroll(gpui::Point { x: 0., y: offset }, window, cx);
        });
        cx.run_until_parked();
        cx.update_editor(|e, window, cx| {
            EditorElement::sticky_headers(&e, &e.snapshot(window, cx))
                .into_iter()
                .map(
                    |StickyHeader {
                         start_point,
                         offset,
                         ..
                     }| { (start_point, offset) },
                )
                .collect::<Vec<_>>()
        })
    };

    let class_foo = Point { row: 1, column: 0 };

    assert_eq!(sticky_headers(0.0), vec![]);
    assert_eq!(sticky_headers(1.5), vec![(class_foo, 0.0)]);
    assert_eq!(sticky_headers(2.5), vec![(class_foo, 0.0)]);
    assert_eq!(sticky_headers(5.5), vec![(class_foo, -0.5)]);
    assert_eq!(sticky_headers(6.0), vec![]);
    assert_eq!(sticky_headers(7.0), vec![]);
}

#[gpui::test]
async fn test_sticky_scroll_anchors_multiline_c_signature_on_name_row(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let buffer = indoc! {"
        ˇvoid
        evdev_post_scroll(struct evdev_device *device,
                  usec_t time,
                  enum libinput_pointer_axis_source source,
                  const struct normalized_coords *delta)
        {
            const struct normalized_coords tilt_rot = {
                cos(SCROLL_DELTA_TILT_ANGLE),
                sin(SCROLL_DELTA_TILT_ANGLE),
            };
        }
    "};
    cx.set_state(buffer);

    cx.update_editor(|editor, _, cx| {
        editor
            .buffer()
            .read(cx)
            .as_singleton()
            .unwrap()
            .update(cx, |buffer, cx| {
                buffer.set_language(
                    Some(languages::language("c", tree_sitter_c::LANGUAGE.into())),
                    cx,
                );
            })
    });

    let mut sticky_headers = |offset: ScrollOffset| {
        cx.update_editor(|editor, window, cx| {
            editor.scroll(gpui::Point { x: 0., y: offset }, window, cx);
        });
        cx.run_until_parked();
        cx.update_editor(|editor, window, cx| {
            EditorElement::sticky_headers(&editor, &editor.snapshot(window, cx))
                .into_iter()
                .map(
                    |StickyHeader {
                         start_point,
                         offset,
                         ..
                     }| { (start_point, offset) },
                )
                .collect::<Vec<_>>()
        })
    };

    let function_name_row = Point { row: 1, column: 0 };

    assert_eq!(sticky_headers(1.0), vec![]);
    assert_eq!(sticky_headers(1.5), vec![(function_name_row, 0.0)]);
    assert_eq!(sticky_headers(5.0), vec![(function_name_row, 0.0)]);
}

#[gpui::test]
async fn test_sticky_scroll_with_expanded_deleted_diff_hunks(
    executor: BackgroundExecutor,
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let diff_base = indoc! {"
        fn foo() {
            let a = 1;
            let b = 2;
            let c = 3;
            let d = 4;
            let e = 5;
        }
    "};

    let buffer = indoc! {"
        ˇfn foo() {
        }
    "};

    cx.set_state(&buffer);

    cx.update_editor(|e, _, cx| {
        e.buffer()
            .read(cx)
            .as_singleton()
            .unwrap()
            .update(cx, |buffer, cx| {
                buffer.set_language(Some(rust_lang()), cx);
            })
    });

    cx.set_head_text(diff_base);
    executor.run_until_parked();

    cx.update_editor(|editor, window, cx| {
        editor.expand_all_diff_hunks(&ExpandAllDiffHunks, window, cx);
    });
    executor.run_until_parked();

    // After expanding, the display should look like:
    //   row 0: fn foo() {
    //   row 1: -    let a = 1;   (deleted)
    //   row 2: -    let b = 2;   (deleted)
    //   row 3: -    let c = 3;   (deleted)
    //   row 4: -    let d = 4;   (deleted)
    //   row 5: -    let e = 5;   (deleted)
    //   row 6: }
    //
    // fn foo() spans display rows 0-6. Scrolling into the deleted region
    // (rows 1-5) should still show fn foo() as a sticky header.

    let fn_foo = Point { row: 0, column: 0 };

    let mut sticky_headers = |offset: ScrollOffset| {
        cx.update_editor(|e, window, cx| {
            e.scroll(gpui::Point { x: 0., y: offset }, window, cx);
        });
        cx.run_until_parked();
        cx.update_editor(|e, window, cx| {
            EditorElement::sticky_headers(&e, &e.snapshot(window, cx))
                .into_iter()
                .map(
                    |StickyHeader {
                         start_point,
                         offset,
                         ..
                     }| { (start_point, offset) },
                )
                .collect::<Vec<_>>()
        })
    };

    assert_eq!(sticky_headers(0.0), vec![]);
    assert_eq!(sticky_headers(0.5), vec![(fn_foo, 0.0)]);
    assert_eq!(sticky_headers(1.0), vec![(fn_foo, 0.0)]);
    // Scrolling into deleted lines: fn foo() should still be a sticky header.
    assert_eq!(sticky_headers(2.0), vec![(fn_foo, 0.0)]);
    assert_eq!(sticky_headers(3.0), vec![(fn_foo, 0.0)]);
    assert_eq!(sticky_headers(4.0), vec![(fn_foo, 0.0)]);
    assert_eq!(sticky_headers(5.0), vec![(fn_foo, 0.0)]);
    assert_eq!(sticky_headers(5.5), vec![(fn_foo, -0.5)]);
    // Past the closing brace: no more sticky header.
    assert_eq!(sticky_headers(6.0), vec![]);
}

#[gpui::test]
async fn test_no_duplicated_sticky_headers(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"
        ˇimpl Foo { fn bar() {
            let x = 1;
            fn baz() {
                let y = 2;
            }
        } }
    "});

    cx.update_editor(|e, _, cx| {
        e.buffer()
            .read(cx)
            .as_singleton()
            .unwrap()
            .update(cx, |buffer, cx| {
                buffer.set_language(Some(rust_lang()), cx);
            })
    });

    let mut sticky_headers = |offset: ScrollOffset| {
        cx.update_editor(|e, window, cx| {
            e.scroll(gpui::Point { x: 0., y: offset }, window, cx);
        });
        cx.run_until_parked();
        cx.update_editor(|e, window, cx| {
            EditorElement::sticky_headers(&e, &e.snapshot(window, cx))
                .into_iter()
                .map(
                    |StickyHeader {
                         start_point,
                         offset,
                         ..
                     }| { (start_point, offset) },
                )
                .collect::<Vec<_>>()
        })
    };

    let struct_foo = Point { row: 0, column: 0 };
    let fn_baz = Point { row: 2, column: 4 };

    assert_eq!(sticky_headers(0.0), vec![]);
    assert_eq!(sticky_headers(0.5), vec![(struct_foo, 0.0)]);
    assert_eq!(sticky_headers(1.0), vec![(struct_foo, 0.0)]);
    assert_eq!(sticky_headers(1.5), vec![(struct_foo, 0.0), (fn_baz, 1.0)]);
    assert_eq!(sticky_headers(2.0), vec![(struct_foo, 0.0), (fn_baz, 1.0)]);
    assert_eq!(sticky_headers(2.5), vec![(struct_foo, 0.0), (fn_baz, 0.5)]);
    assert_eq!(sticky_headers(3.0), vec![(struct_foo, 0.0)]);
    assert_eq!(sticky_headers(3.5), vec![(struct_foo, 0.0)]);
    assert_eq!(sticky_headers(4.0), vec![(struct_foo, 0.0)]);
    assert_eq!(sticky_headers(4.5), vec![(struct_foo, -0.5)]);
    assert_eq!(sticky_headers(5.0), vec![]);
}

#[gpui::test]
async fn test_autoscroll_keeps_cursor_visible_below_sticky_headers(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    update_test_editor_settings(cx, &|settings| {
        settings.vertical_scroll_margin = Some(0.0);
        settings.scroll_beyond_last_line = Some(ScrollBeyondLastLine::OnePage);
        settings.sticky_scroll = Some(settings::StickyScrollContent {
            enabled: Some(true),
        });
    });
    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"
        impl Foo { fn bar() {
            let x = 1;
            fn baz() {
                let y = 2;
            }
        } }
        ˇ
    "});

    let mut previous_cursor_row = cx.update_editor(|editor, window, cx| {
        editor
            .buffer()
            .read(cx)
            .as_singleton()
            .unwrap()
            .update(cx, |buffer, cx| buffer.set_language(Some(rust_lang()), cx));
        let cursor_row = editor
            .selections
            .newest_display(&editor.display_snapshot(cx))
            .head()
            .row();
        editor.set_scroll_top_row(cursor_row, window, cx);
        cursor_row
    });

    for _ in 0..6 {
        cx.update_editor(|editor, window, cx| editor.move_up(&MoveUp, window, cx));
        cx.run_until_parked();

        cx.update_editor(|editor, window, cx| {
            let snapshot = editor.snapshot(window, cx);
            let scroll_top = snapshot.scroll_position().y;
            let sticky_header_count = EditorElement::sticky_headers(editor, &snapshot).len();
            let cursor_row = editor
                .selections
                .newest_display(&snapshot.display_snapshot)
                .head()
                .row();
            assert_eq!(
                cursor_row,
                previous_cursor_row
                    .previous_row()
                    .max(DisplayRow(scroll_top as u32) + DisplayRow(sticky_header_count as u32))
            );
            previous_cursor_row = cursor_row;
        });

        // The `ScrollCursorTop` action shouldn't change the scroll position, as the cursor is
        // already as high up as the sticky headers allow.
        let scroll_top_before =
            cx.update_editor(|editor, window, cx| editor.snapshot(window, cx).scroll_position().y);
        cx.update_editor(|editor, window, cx| {
            editor.scroll_cursor_top(&ScrollCursorTop, window, cx)
        });
        cx.run_until_parked();
        let scroll_top_after =
            cx.update_editor(|editor, window, cx| editor.snapshot(window, cx).scroll_position().y);
        assert_eq!(scroll_top_before, scroll_top_after);
    }
}

#[gpui::test]
fn test_relative_line_numbers(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let buffer_1 = cx.new(|cx| Buffer::local("aaaaaaaaaa\nbbb\n", cx));
    let buffer_2 = cx.new(|cx| Buffer::local("cccccccccc\nddd\n", cx));
    let buffer_3 = cx.new(|cx| Buffer::local("eee\nffffffffff\n", cx));

    let multibuffer = cx.new(|cx| {
        let mut multibuffer = MultiBuffer::new(ReadWrite);
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(0),
            buffer_1.clone(),
            [Point::new(0, 0)..Point::new(2, 0)],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(1),
            buffer_2.clone(),
            [Point::new(0, 0)..Point::new(2, 0)],
            0,
            cx,
        );
        multibuffer.set_excerpts_for_path(
            PathKey::sorted(2),
            buffer_3.clone(),
            [Point::new(0, 0)..Point::new(2, 0)],
            0,
            cx,
        );
        multibuffer
    });

    // wrapped contents of multibuffer:
    //    aaa
    //    aaa
    //    aaa
    //    a
    //    bbb
    //
    //    ccc
    //    ccc
    //    ccc
    //    c
    //    ddd
    //
    //    eee
    //    fff
    //    fff
    //    fff
    //    f

    let editor = cx.add_window(|window, cx| build_editor(multibuffer, window, cx));
    _ = editor.update(cx, |editor, window, cx| {
        editor.set_wrap_width(Some(30.0.into()), cx); // every 3 characters

        // includes trailing newlines.
        let expected_line_numbers = [2, 6, 7, 10, 14, 15, 18, 19, 23];
        let expected_wrapped_line_numbers = [
            2, 3, 4, 5, 6, 7, 10, 11, 12, 13, 14, 15, 18, 19, 20, 21, 22, 23,
        ];

        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_ranges([
                Point::new(7, 0)..Point::new(7, 1), // second row of `ccc`
            ]);
        });

        let snapshot = editor.snapshot(window, cx);

        // these are all 0-indexed
        let base_display_row = DisplayRow(11);
        let base_row = 3;
        let wrapped_base_row = 7;

        // test not counting wrapped lines
        let expected_relative_numbers = expected_line_numbers
            .into_iter()
            .enumerate()
            .map(|(i, row)| (DisplayRow(row), i.abs_diff(base_row) as u32))
            .filter(|(_, relative_line_number)| *relative_line_number != 0)
            .collect_vec();
        let actual_relative_numbers = snapshot
            .calculate_relative_line_numbers(
                &(DisplayRow(0)..DisplayRow(24)),
                base_display_row,
                false,
            )
            .into_iter()
            .sorted()
            .collect_vec();
        assert_eq!(expected_relative_numbers, actual_relative_numbers);
        // check `calculate_relative_line_numbers()` against `relative_line_delta()` for each line
        for (display_row, relative_number) in expected_relative_numbers {
            assert_eq!(
                relative_number,
                snapshot
                    .relative_line_delta(display_row, base_display_row, false)
                    .unsigned_abs() as u32,
            );
        }

        // test counting wrapped lines
        let expected_wrapped_relative_numbers = expected_wrapped_line_numbers
            .into_iter()
            .enumerate()
            .map(|(i, row)| (DisplayRow(row), i.abs_diff(wrapped_base_row) as u32))
            .filter(|(row, _)| *row != base_display_row)
            .collect_vec();
        let actual_relative_numbers = snapshot
            .calculate_relative_line_numbers(
                &(DisplayRow(0)..DisplayRow(24)),
                base_display_row,
                true,
            )
            .into_iter()
            .sorted()
            .collect_vec();
        assert_eq!(expected_wrapped_relative_numbers, actual_relative_numbers);
        // check `calculate_relative_line_numbers()` against `relative_wrapped_line_delta()` for each line
        for (display_row, relative_number) in expected_wrapped_relative_numbers {
            assert_eq!(
                relative_number,
                snapshot
                    .relative_line_delta(display_row, base_display_row, true)
                    .unsigned_abs() as u32,
            );
        }
    });
}

#[gpui::test]
async fn test_scroll_by_clicking_sticky_header(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.editor.sticky_scroll = Some(settings::StickyScrollContent {
                    enabled: Some(true),
                })
            });
        });
    });
    let mut cx = EditorTestContext::new(cx).await;

    let line_height = cx.update_editor(|editor, window, cx| {
        editor
            .style(cx)
            .text
            .line_height_in_pixels(window.rem_size())
    });

    let buffer = indoc! {"
            ˇfn foo() {
                let abc = 123;
            }
            struct Bar;
            impl Bar {
                fn new() -> Self {
                    Self
                }
            }
            fn baz() {
            }
        "};
    cx.set_state(&buffer);

    cx.update_editor(|e, _, cx| {
        e.buffer()
            .read(cx)
            .as_singleton()
            .unwrap()
            .update(cx, |buffer, cx| {
                buffer.set_language(Some(rust_lang()), cx);
            })
    });

    let fn_foo = || empty_range(0, 0);
    let impl_bar = || empty_range(4, 0);
    let fn_new = || empty_range(5, 0);

    let mut scroll_and_click = |scroll_offset: ScrollOffset, click_offset: ScrollOffset| {
        cx.update_editor(|e, window, cx| {
            e.scroll(
                gpui::Point {
                    x: 0.,
                    y: scroll_offset,
                },
                window,
                cx,
            );
        });
        cx.run_until_parked();
        cx.simulate_click(
            gpui::Point {
                x: px(0.),
                y: click_offset as f32 * line_height,
            },
            Modifiers::none(),
        );
        cx.run_until_parked();
        cx.update_editor(|e, _, cx| (e.scroll_position(cx), display_ranges(e, cx)))
    };
    assert_eq!(
        scroll_and_click(
            4.5, // impl Bar is halfway off the screen
            0.0  // click top of screen
        ),
        // scrolled to impl Bar
        (gpui::Point { x: 0., y: 4. }, vec![impl_bar()])
    );

    assert_eq!(
        scroll_and_click(
            4.5,  // impl Bar is halfway off the screen
            0.25  // click middle of impl Bar
        ),
        // scrolled to impl Bar
        (gpui::Point { x: 0., y: 4. }, vec![impl_bar()])
    );

    assert_eq!(
        scroll_and_click(
            4.5, // impl Bar is halfway off the screen
            1.5  // click below impl Bar (e.g. fn new())
        ),
        // scrolled to fn new() - this is below the impl Bar header which has persisted
        (gpui::Point { x: 0., y: 4. }, vec![fn_new()])
    );

    assert_eq!(
        scroll_and_click(
            5.5,  // fn new is halfway underneath impl Bar
            0.75  // click on the overlap of impl Bar and fn new()
        ),
        (gpui::Point { x: 0., y: 4. }, vec![impl_bar()])
    );

    assert_eq!(
        scroll_and_click(
            5.5,  // fn new is halfway underneath impl Bar
            1.25  // click on the visible part of fn new()
        ),
        (gpui::Point { x: 0., y: 4. }, vec![fn_new()])
    );

    assert_eq!(
        scroll_and_click(
            1.5, // fn foo is halfway off the screen
            0.0  // click top of screen
        ),
        (gpui::Point { x: 0., y: 0. }, vec![fn_foo()])
    );

    assert_eq!(
        scroll_and_click(
            1.5,  // fn foo is halfway off the screen
            0.75  // click visible part of let abc...
        )
        .0,
        // no change in scroll
        // we don't assert on the visible_range because if we clicked the gutter, our line is fully selected
        (gpui::Point { x: 0., y: 1.5 })
    );

    // Verify clicking at a specific x position within a sticky header places
    // the cursor at the corresponding column.
    let (text_origin_x, em_width) = cx.update_editor(|editor, _, _| {
        let position_map = editor.last_position_map.as_ref().unwrap();
        (
            position_map.text_hitbox.bounds.origin.x,
            position_map.em_layout_width,
        )
    });

    // Click on "impl Bar {" sticky header at column 5 (the 'B' in 'Bar').
    // The text "impl Bar {" starts at column 0, so column 5 = 'B'.
    let click_x = text_origin_x + em_width * 5.5;
    cx.update_editor(|e, window, cx| {
        e.scroll(gpui::Point { x: 0., y: 4.5 }, window, cx);
    });
    cx.run_until_parked();
    cx.simulate_click(
        gpui::Point {
            x: click_x,
            y: 0.25 * line_height,
        },
        Modifiers::none(),
    );
    cx.run_until_parked();
    let (scroll_pos, selections) =
        cx.update_editor(|e, _, cx| (e.scroll_position(cx), display_ranges(e, cx)));
    assert_eq!(scroll_pos, gpui::Point { x: 0., y: 4. });
    assert_eq!(selections, vec![empty_range(4, 5)]);
}

#[gpui::test]
async fn test_clicking_sticky_header_sets_character_select_mode(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.editor.sticky_scroll = Some(settings::StickyScrollContent {
                    enabled: Some(true),
                })
            });
        });
    });
    let mut cx = EditorTestContext::new(cx).await;

    let line_height = cx.update_editor(|editor, window, cx| {
        editor
            .style(cx)
            .text
            .line_height_in_pixels(window.rem_size())
    });

    let buffer = indoc! {"
            fn foo() {
                let abc = 123;
            }
            ˇstruct Bar;
        "};
    cx.set_state(&buffer);

    cx.update_editor(|editor, _, cx| {
        editor
            .buffer()
            .read(cx)
            .as_singleton()
            .unwrap()
            .update(cx, |buffer, cx| {
                buffer.set_language(Some(rust_lang()), cx);
            })
    });

    let text_origin_x = cx.update_editor(|editor, _, _| {
        editor
            .last_position_map
            .as_ref()
            .unwrap()
            .text_hitbox
            .bounds
            .origin
            .x
    });

    cx.update_editor(|editor, window, cx| {
        // Double click on `struct` to select it
        editor.begin_selection(DisplayPoint::new(DisplayRow(3), 1), false, 2, window, cx);
        editor.end_selection(window, cx);

        // Scroll down one row to make `fn foo() {` a sticky header
        editor.scroll(gpui::Point { x: 0., y: 1. }, window, cx);
    });
    cx.run_until_parked();

    // Click at the start of the `fn foo() {` sticky header
    cx.simulate_click(
        gpui::Point {
            x: text_origin_x,
            y: 0.5 * line_height,
        },
        Modifiers::none(),
    );
    cx.run_until_parked();

    // Shift-click at the end of `fn foo() {` to select the whole row
    cx.update_editor(|editor, window, cx| {
        editor.extend_selection(DisplayPoint::new(DisplayRow(0), 10), 1, window, cx);
        editor.end_selection(window, cx);
    });
    cx.run_until_parked();

    let selections = cx.update_editor(|editor, _, cx| display_ranges(editor, cx));
    assert_eq!(
        selections,
        vec![DisplayPoint::new(DisplayRow(0), 0)..DisplayPoint::new(DisplayRow(0), 10)]
    );
}

#[gpui::test]
async fn test_multibuffer_selections_with_folding(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let (editor, cx) = cx.add_window_view(|window, cx| {
        let multi_buffer = MultiBuffer::build_multi(
            [
                ("1\n2\n3\n", vec![Point::row_range(0..3)]),
                ("1\n2\n3\n", vec![Point::row_range(0..3)]),
            ],
            cx,
        );
        Editor::new(EditorMode::full(), multi_buffer, None, window, cx)
    });

    let mut cx = EditorTestContext::for_editor_in(editor.clone(), cx).await;
    let buffer_ids = cx.multibuffer(|mb, cx| {
        mb.snapshot(cx)
            .excerpts()
            .map(|excerpt| excerpt.context.start.buffer_id)
            .collect::<Vec<_>>()
    });

    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        ˇ1
        2
        3
        [EXCERPT]
        1
        2
        3
        "});

    // Scenario 1: Unfolded buffers, position cursor on "2", select all matches, then insert
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(None.into(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(2)..MultiBufferOffset(3)]);
        });
    });
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        1
        2ˇ
        3
        [EXCERPT]
        1
        2
        3
        "});

    cx.update_editor(|editor, window, cx| {
        editor
            .select_all_matches(&SelectAllMatches, window, cx)
            .unwrap();
    });
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        1
        2ˇ
        3
        [EXCERPT]
        1
        2ˇ
        3
        "});

    cx.update_editor(|editor, window, cx| {
        editor.handle_input("X", window, cx);
    });
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        1
        Xˇ
        3
        [EXCERPT]
        1
        Xˇ
        3
        "});

    // Scenario 2: Select "2", then fold second buffer before insertion
    cx.update_multibuffer(|mb, cx| {
        for buffer_id in buffer_ids.iter() {
            let buffer = mb.buffer(*buffer_id).unwrap();
            buffer.update(cx, |buffer, cx| {
                buffer.edit([(0..buffer.len(), "1\n2\n3\n")], None, cx);
            });
        }
    });

    // Select "2" and select all matches
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(None.into(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(2)..MultiBufferOffset(3)]);
        });
        editor
            .select_all_matches(&SelectAllMatches, window, cx)
            .unwrap();
    });

    // Fold second buffer - should remove selections from folded buffer
    cx.update_editor(|editor, _, cx| {
        editor.fold_buffer(buffer_ids[1], cx);
    });
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        1
        2ˇ
        3
        [EXCERPT]
        [FOLDED]
        "});

    // Insert text - should only affect first buffer
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("Y", window, cx);
    });
    cx.update_editor(|editor, _, cx| {
        editor.unfold_buffer(buffer_ids[1], cx);
    });
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        1
        Yˇ
        3
        [EXCERPT]
        1
        2
        3
        "});

    // Scenario 3: Select "2", then fold first buffer before insertion
    cx.update_multibuffer(|mb, cx| {
        for buffer_id in buffer_ids.iter() {
            let buffer = mb.buffer(*buffer_id).unwrap();
            buffer.update(cx, |buffer, cx| {
                buffer.edit([(0..buffer.len(), "1\n2\n3\n")], None, cx);
            });
        }
    });

    // Select "2" and select all matches
    cx.update_editor(|editor, window, cx| {
        editor.change_selections(None.into(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(2)..MultiBufferOffset(3)]);
        });
        editor
            .select_all_matches(&SelectAllMatches, window, cx)
            .unwrap();
    });

    // Fold first buffer - should remove selections from folded buffer
    cx.update_editor(|editor, _, cx| {
        editor.fold_buffer(buffer_ids[0], cx);
    });
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        1
        2ˇ
        3
        "});

    // Insert text - should only affect second buffer
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("Z", window, cx);
    });
    cx.update_editor(|editor, _, cx| {
        editor.unfold_buffer(buffer_ids[0], cx);
    });
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        1
        2
        3
        [EXCERPT]
        1
        Zˇ
        3
        "});

    // Test correct folded header is selected upon fold
    cx.update_editor(|editor, _, cx| {
        editor.fold_buffer(buffer_ids[0], cx);
        editor.fold_buffer(buffer_ids[1], cx);
    });
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        [FOLDED]
        [EXCERPT]
        ˇ[FOLDED]
        "});

    // Test selection inside folded buffer unfolds it on type
    cx.update_editor(|editor, window, cx| {
        editor.handle_input("W", window, cx);
    });
    cx.update_editor(|editor, _, cx| {
        editor.unfold_buffer(buffer_ids[0], cx);
    });
    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        1
        2
        3
        [EXCERPT]
        Wˇ1
        Z
        3
        "});
}

#[gpui::test]
async fn test_multibuffer_scroll_cursor_top_margin(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let (editor, cx) = cx.add_window_view(|window, cx| {
        let multi_buffer = MultiBuffer::build_multi(
            [
                ("1\n2\n3\n", vec![Point::row_range(0..3)]),
                ("1\n2\n3\n4\n5\n6\n7\n8\n9\n", vec![Point::row_range(0..9)]),
            ],
            cx,
        );
        Editor::new(EditorMode::full(), multi_buffer, None, window, cx)
    });

    let mut cx = EditorTestContext::for_editor_in(editor.clone(), cx).await;

    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        ˇ1
        2
        3
        [EXCERPT]
        1
        2
        3
        4
        5
        6
        7
        8
        9
        "});

    cx.update_editor(|editor, window, cx| {
        editor.change_selections(None.into(), window, cx, |s| {
            s.select_ranges([MultiBufferOffset(19)..MultiBufferOffset(19)]);
        });
    });

    cx.assert_excerpts_with_selections(indoc! {"
        [EXCERPT]
        1
        2
        3
        [EXCERPT]
        1
        2
        3
        4
        5
        6
        ˇ7
        8
        9
        "});

    cx.update_editor(|editor, _window, cx| {
        editor.set_vertical_scroll_margin(0, cx);
    });

    cx.update_editor(|editor, window, cx| {
        assert_eq!(editor.vertical_scroll_margin(), 0);
        editor.scroll_cursor_top(&ScrollCursorTop, window, cx);
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 12.0)
        );
    });

    cx.update_editor(|editor, _window, cx| {
        editor.set_vertical_scroll_margin(3, cx);
    });

    cx.update_editor(|editor, window, cx| {
        assert_eq!(editor.vertical_scroll_margin(), 3);
        editor.scroll_cursor_top(&ScrollCursorTop, window, cx);
        assert_eq!(
            editor.snapshot(window, cx).scroll_position(),
            gpui::Point::new(0., 9.0)
        );
    });
}

#[gpui::test]
async fn test_newline_task_list_continuation(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = Some(2.try_into().unwrap());
    });

    let markdown_language = languages::language("markdown", tree_sitter_md::LANGUAGE.into());
    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_language), cx));

    // Case 1: Adding newline after (whitespace + prefix + any non-whitespace) adds marker
    cx.set_state(indoc! {"
        - [ ] taskˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - [ ] task
        - [ ] ˇ
    "});

    // Case 2: Works with checked task items too
    cx.set_state(indoc! {"
        - [x] completed taskˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - [x] completed task
        - [ ] ˇ
    "});

    // Case 2.1: Works with uppercase checked marker too
    cx.set_state(indoc! {"
        - [X] completed taskˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - [X] completed task
        - [ ] ˇ
    "});

    // Case 3: Cursor position doesn't matter - content after marker is what counts
    cx.set_state(indoc! {"
        - [ ] taˇsk
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - [ ] ta
        - [ ] ˇsk
    "});

    // Case 4: Adding newline after (whitespace + prefix + some whitespace) does NOT add marker
    cx.set_state(indoc! {"
        - [ ]  ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(
        indoc! {"
        - [ ]$$
        ˇ
    "}
        .replace("$", " ")
        .as_str(),
    );

    // Case 5: Adding newline with content adds marker preserving indentation
    cx.set_state(indoc! {"
        - [ ] task
          - [ ] indentedˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - [ ] task
          - [ ] indented
          - [ ] ˇ
    "});

    // Case 6: Adding newline with cursor right after prefix, unindents
    cx.set_state(indoc! {"
        - [ ] task
          - [ ] sub task
            - [ ] ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - [ ] task
          - [ ] sub task
          - [ ] ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;

    // Case 7: Adding newline with cursor right after prefix, removes marker
    cx.assert_editor_state(indoc! {"
        - [ ] task
          - [ ] sub task
        - [ ] ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - [ ] task
          - [ ] sub task
        ˇ
    "});

    // Case 8: Cursor before or inside prefix does not add marker
    cx.set_state(indoc! {"
        ˇ- [ ] task
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"

        ˇ- [ ] task
    "});

    cx.set_state(indoc! {"
        - [ˇ ] task
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - [
        ˇ
        ] task
    "});
}

#[gpui::test]
async fn test_newline_unordered_list_continuation(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = Some(2.try_into().unwrap());
    });

    let markdown_language = languages::language("markdown", tree_sitter_md::LANGUAGE.into());
    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_language), cx));

    // Case 1: Adding newline after (whitespace + marker + any non-whitespace) adds marker
    cx.set_state(indoc! {"
        - itemˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - item
        - ˇ
    "});

    // Case 2: Works with different markers
    cx.set_state(indoc! {"
        * starred itemˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        * starred item
        * ˇ
    "});

    cx.set_state(indoc! {"
        + plus itemˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        + plus item
        + ˇ
    "});

    // Case 3: Cursor position doesn't matter - content after marker is what counts
    cx.set_state(indoc! {"
        - itˇem
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - it
        - ˇem
    "});

    // Case 4: Adding newline after (whitespace + marker + some whitespace) does NOT add marker
    cx.set_state(indoc! {"
        -  ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(
        indoc! {"
        - $
        ˇ
    "}
        .replace("$", " ")
        .as_str(),
    );

    // Case 5: Adding newline with content adds marker preserving indentation
    cx.set_state(indoc! {"
        - item
          - indentedˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - item
          - indented
          - ˇ
    "});

    // Case 6: Adding newline with cursor right after marker, unindents
    cx.set_state(indoc! {"
        - item
          - sub item
            - ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - item
          - sub item
          - ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;

    // Case 7: Adding newline with cursor right after marker, removes marker
    cx.assert_editor_state(indoc! {"
        - item
          - sub item
        - ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - item
          - sub item
        ˇ
    "});

    // Case 8: Cursor before or inside prefix does not add marker
    cx.set_state(indoc! {"
        ˇ- item
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"

        ˇ- item
    "});

    cx.set_state(indoc! {"
        -ˇ item
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        -
        ˇitem
    "});

    update_test_language_settings(&mut cx, &|settings| {
        settings.defaults.tab_size = Some(4.try_into().unwrap());
    });

    // Case 9: Empty list item unindent works when tab size is larger than list indentation
    cx.set_state(indoc! {"
        - item
          - sub item
          - ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        - item
          - sub item
        - ˇ
    "});

    // Case 10: Empty list item unindent moves to the previous tab stop
    cx.set_state(
        indoc! {"
        $$$$$$- ˇ
    "}
        .replace("$", " ")
        .as_str(),
    );
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(
        indoc! {"
        $$$$- ˇ
    "}
        .replace("$", " ")
        .as_str(),
    );
}

#[gpui::test]
async fn test_newline_ordered_list_continuation(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = Some(2.try_into().unwrap());
    });

    let markdown_language = languages::language("markdown", tree_sitter_md::LANGUAGE.into());
    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_language), cx));

    // Case 1: Adding newline after (whitespace + marker + any non-whitespace) increments number
    cx.set_state(indoc! {"
        1. first itemˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        1. first item
        2. ˇ
    "});

    // Case 2: Works with larger numbers
    cx.set_state(indoc! {"
        10. tenth itemˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        10. tenth item
        11. ˇ
    "});

    // Case 3: Cursor position doesn't matter - content after marker is what counts
    cx.set_state(indoc! {"
        1. itˇem
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        1. it
        2. ˇem
    "});

    // Case 4: Adding newline after (whitespace + marker + some whitespace) does NOT add marker
    cx.set_state(indoc! {"
        1.  ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(
        indoc! {"
        1. $
        ˇ
    "}
        .replace("$", " ")
        .as_str(),
    );

    // Case 5: Adding newline with content adds marker preserving indentation
    cx.set_state(indoc! {"
        1. item
          2. indentedˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        1. item
          2. indented
          3. ˇ
    "});

    // Case 6: Adding newline with cursor right after marker, unindents
    cx.set_state(indoc! {"
        1. item
          2. sub item
            3. ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        1. item
          2. sub item
          1. ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;

    // Case 7: Adding newline with cursor right after marker, removes marker
    cx.assert_editor_state(indoc! {"
        1. item
          2. sub item
        1. ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        1. item
          2. sub item
        ˇ
    "});

    // Case 8: Cursor before or inside prefix does not add marker
    cx.set_state(indoc! {"
        ˇ1. item
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"

        ˇ1. item
    "});

    cx.set_state(indoc! {"
        1ˇ. item
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        1
        ˇ. item
    "});
}

#[gpui::test]
async fn test_newline_should_not_autoindent_ordered_list(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = Some(2.try_into().unwrap());
    });

    let markdown_language = languages::language("markdown", tree_sitter_md::LANGUAGE.into());
    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_language), cx));

    // Case 1: Adding newline after (whitespace + marker + any non-whitespace) increments number
    cx.set_state(indoc! {"
        1. first item
          1. sub first item
          2. sub second item
          3. ˇ
    "});
    cx.update_editor(|e, window, cx| e.newline(&Newline, window, cx));
    cx.wait_for_autoindent_applied().await;
    cx.assert_editor_state(indoc! {"
        1. first item
          1. sub first item
          2. sub second item
        1. ˇ
    "});
}

#[gpui::test]
async fn test_tab_list_indent(cx: &mut TestAppContext) {
    init_test(cx, |settings| {
        settings.defaults.tab_size = Some(2.try_into().unwrap());
    });

    let markdown_language = languages::language("markdown", tree_sitter_md::LANGUAGE.into());
    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_language), cx));

    // Case 1: Unordered list - cursor after prefix, adds indent before prefix
    cx.set_state(indoc! {"
        - ˇitem
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    let expected = indoc! {"
        $$- ˇitem
    "};
    cx.assert_editor_state(expected.replace("$", " ").as_str());

    // Case 2: Task list - cursor after prefix
    cx.set_state(indoc! {"
        - [ ] ˇtask
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    let expected = indoc! {"
        $$- [ ] ˇtask
    "};
    cx.assert_editor_state(expected.replace("$", " ").as_str());

    // Case 3: Ordered list - cursor after prefix
    cx.set_state(indoc! {"
        1. ˇfirst
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    let expected = indoc! {"
        $$1. ˇfirst
    "};
    cx.assert_editor_state(expected.replace("$", " ").as_str());

    // Case 4: With existing indentation - adds more indent
    let initial = indoc! {"
        $$- ˇitem
    "};
    cx.set_state(initial.replace("$", " ").as_str());
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    let expected = indoc! {"
        $$$$- ˇitem
    "};
    cx.assert_editor_state(expected.replace("$", " ").as_str());

    // Case 5: Empty list item
    cx.set_state(indoc! {"
        - ˇ
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    let expected = indoc! {"
        $$- ˇ
    "};
    cx.assert_editor_state(expected.replace("$", " ").as_str());

    // Case 6: Cursor at end of line with content
    cx.set_state(indoc! {"
        - itemˇ
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    let expected = indoc! {"
        $$- itemˇ
    "};
    cx.assert_editor_state(expected.replace("$", " ").as_str());

    // Case 7: Cursor at start of list item, indents it
    cx.set_state(indoc! {"
        - item
        ˇ  - sub item
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    let expected = indoc! {"
        - item
          ˇ  - sub item
    "};
    cx.assert_editor_state(expected);

    // Case 8: Cursor at start of list item, moves the cursor when "indent_list_on_tab" is false
    cx.update_editor(|_, _, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.project.all_languages.defaults.indent_list_on_tab = Some(false);
            });
        });
    });
    cx.set_state(indoc! {"
        - item
        ˇ  - sub item
    "});
    cx.update_editor(|e, window, cx| e.tab(&Tab, window, cx));
    cx.wait_for_autoindent_applied().await;
    let expected = indoc! {"
        - item
          ˇ- sub item
    "};
    cx.assert_editor_state(expected);
}

#[gpui::test]
fn test_editor_rendering_when_positioned_above_viewport(cx: &mut TestAppContext) {
    // This test reproduces a bug where drawing an editor at a position above the viewport
    // (simulating what happens when an AutoHeight editor inside a List is scrolled past)
    // causes an infinite loop in blocks_in_range.
    //
    // The issue: when the editor's bounds.origin.y is very negative (above the viewport),
    // the content mask intersection produces visible_bounds with origin at the viewport top.
    // This makes clipped_top_in_lines very large, causing start_row to exceed max_row.
    // When blocks_in_range is called with start_row > max_row, the cursor seeks to the end
    // but the while loop after seek never terminates because cursor.next() is a no-op at end.
    init_test(cx, |_| {});

    let window = cx.add_window(|_, _| gpui::Empty);
    let mut cx = VisualTestContext::from_window(*window, cx);

    let buffer = cx.update(|_, cx| MultiBuffer::build_simple("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n", cx));
    let editor = cx.new_window_entity(|window, cx| build_editor(buffer, window, cx));

    // Simulate a small viewport (500x500 pixels at origin 0,0)
    cx.simulate_resize(gpui::size(px(500.), px(500.)));

    // Draw the editor at a very negative Y position, simulating an editor that's been
    // scrolled way above the visible viewport (like in a List that has scrolled past it).
    // The editor is 3000px tall but positioned at y=-10000, so it's entirely above the viewport.
    // This should NOT hang - it should just render nothing.
    cx.draw(
        gpui::point(px(0.), px(-10000.)),
        gpui::size(px(500.), px(3000.)),
        |_, _| editor.clone().into_any_element(),
    );

    // If we get here without hanging, the test passes
}

#[gpui::test]
async fn test_diff_review_indicator_created_on_gutter_hover(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/root"), json!({ "file.txt": "hello\nworld\n" }))
        .await;

    let project = Project::test(fs, [path!("/root").as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(*window, cx);

    let editor = workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.open_abs_path(
                PathBuf::from(path!("/root/file.txt")),
                OpenOptions::default(),
                window,
                cx,
            )
        })
        .await
        .unwrap()
        .downcast::<Editor>()
        .unwrap();

    // Enable diff review button mode
    editor.update(cx, |editor, cx| {
        editor.set_show_diff_review_button(true, cx);
    });

    // Initially, no indicator should be present
    editor.update(cx, |editor, _cx| {
        assert!(
            editor.gutter_diff_review_indicator.0.is_none(),
            "Indicator should be None initially"
        );
    });
}

#[gpui::test]
async fn test_diff_review_button_hidden_when_ai_disabled(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    // Register DisableAiSettings and set disable_ai to true
    cx.update(|cx| {
        project::DisableAiSettings::register(cx);
        project::DisableAiSettings::override_global(
            project::DisableAiSettings { disable_ai: true },
            cx,
        );
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/root"), json!({ "file.txt": "hello\nworld\n" }))
        .await;

    let project = Project::test(fs, [path!("/root").as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(*window, cx);

    let editor = workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.open_abs_path(
                PathBuf::from(path!("/root/file.txt")),
                OpenOptions::default(),
                window,
                cx,
            )
        })
        .await
        .unwrap()
        .downcast::<Editor>()
        .unwrap();

    // Enable diff review button mode
    editor.update(cx, |editor, cx| {
        editor.set_show_diff_review_button(true, cx);
    });

    // Verify AI is disabled
    cx.read(|cx| {
        assert!(
            project::DisableAiSettings::get_global(cx).disable_ai,
            "AI should be disabled"
        );
    });

    // The indicator should not be created when AI is disabled
    // (The mouse_moved handler checks DisableAiSettings before creating the indicator)
    editor.update(cx, |editor, _cx| {
        assert!(
            editor.gutter_diff_review_indicator.0.is_none(),
            "Indicator should be None when AI is disabled"
        );
    });
}

#[gpui::test]
async fn test_diff_review_button_shown_when_ai_enabled(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    // Register DisableAiSettings and set disable_ai to false
    cx.update(|cx| {
        project::DisableAiSettings::register(cx);
        project::DisableAiSettings::override_global(
            project::DisableAiSettings { disable_ai: false },
            cx,
        );
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/root"), json!({ "file.txt": "hello\nworld\n" }))
        .await;

    let project = Project::test(fs, [path!("/root").as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(*window, cx);

    let editor = workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.open_abs_path(
                PathBuf::from(path!("/root/file.txt")),
                OpenOptions::default(),
                window,
                cx,
            )
        })
        .await
        .unwrap()
        .downcast::<Editor>()
        .unwrap();

    // Enable diff review button mode
    editor.update(cx, |editor, cx| {
        editor.set_show_diff_review_button(true, cx);
    });

    // Verify AI is enabled
    cx.read(|cx| {
        assert!(
            !project::DisableAiSettings::get_global(cx).disable_ai,
            "AI should be enabled"
        );
    });

    // The show_diff_review_button flag should be true
    editor.update(cx, |editor, _cx| {
        assert!(
            editor.show_diff_review_button(),
            "show_diff_review_button should be true"
        );
    });
}

/// Helper function to create a DiffHunkKey for testing.
/// Uses Anchor::Min as a placeholder anchor since these tests don't need
/// real buffer positioning.
fn test_hunk_key(file_path: &str) -> DiffHunkKey {
    DiffHunkKey {
        file_path: if file_path.is_empty() {
            Arc::from(util::rel_path::RelPath::empty())
        } else {
            Arc::from(util::rel_path::RelPath::from_unix_str(file_path).unwrap())
        },
        hunk_start_anchor: Anchor::Min,
    }
}

/// Helper function to create a DiffHunkKey with a specific anchor for testing.
fn test_hunk_key_with_anchor(file_path: &str, anchor: Anchor) -> DiffHunkKey {
    DiffHunkKey {
        file_path: if file_path.is_empty() {
            Arc::from(util::rel_path::RelPath::empty())
        } else {
            Arc::from(util::rel_path::RelPath::from_unix_str(file_path).unwrap())
        },
        hunk_start_anchor: anchor,
    }
}

/// Helper function to add a review comment with default anchors for testing.
fn add_test_comment(
    editor: &mut Editor,
    key: DiffHunkKey,
    comment: &str,
    cx: &mut Context<Editor>,
) -> usize {
    editor.add_review_comment(key, comment.to_string(), Anchor::Min..Anchor::Max, cx)
}

#[gpui::test]
fn test_review_comment_add_to_hunk(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    _ = editor.update(cx, |editor: &mut Editor, _window, cx| {
        let key = test_hunk_key("");

        let id = add_test_comment(editor, key.clone(), "Test comment", cx);

        let snapshot = editor.buffer().read(cx).snapshot(cx);
        assert_eq!(editor.total_review_comment_count(), 1);
        assert_eq!(editor.hunk_comment_count(&key, &snapshot), 1);

        let comments = editor.comments_for_hunk(&key, &snapshot);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].comment, "Test comment");
        assert_eq!(comments[0].id, id);
    });
}

#[gpui::test]
fn test_review_comments_are_per_hunk(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    _ = editor.update(cx, |editor: &mut Editor, _window, cx| {
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        let anchor1 = snapshot.anchor_before(Point::new(0, 0));
        let anchor2 = snapshot.anchor_before(Point::new(0, 0));
        let key1 = test_hunk_key_with_anchor("file1.rs", anchor1);
        let key2 = test_hunk_key_with_anchor("file2.rs", anchor2);

        add_test_comment(editor, key1.clone(), "Comment for file1", cx);
        add_test_comment(editor, key2.clone(), "Comment for file2", cx);

        let snapshot = editor.buffer().read(cx).snapshot(cx);
        assert_eq!(editor.total_review_comment_count(), 2);
        assert_eq!(editor.hunk_comment_count(&key1, &snapshot), 1);
        assert_eq!(editor.hunk_comment_count(&key2, &snapshot), 1);

        assert_eq!(
            editor.comments_for_hunk(&key1, &snapshot)[0].comment,
            "Comment for file1"
        );
        assert_eq!(
            editor.comments_for_hunk(&key2, &snapshot)[0].comment,
            "Comment for file2"
        );
    });
}

#[gpui::test]
fn test_review_comment_remove(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    _ = editor.update(cx, |editor: &mut Editor, _window, cx| {
        let key = test_hunk_key("");

        let id = add_test_comment(editor, key, "To be removed", cx);

        assert_eq!(editor.total_review_comment_count(), 1);

        let removed = editor.remove_review_comment(id, cx);
        assert!(removed);
        assert_eq!(editor.total_review_comment_count(), 0);

        // Try to remove again
        let removed_again = editor.remove_review_comment(id, cx);
        assert!(!removed_again);
    });
}

#[gpui::test]
fn test_review_comment_update(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    _ = editor.update(cx, |editor: &mut Editor, _window, cx| {
        let key = test_hunk_key("");

        let id = add_test_comment(editor, key.clone(), "Original text", cx);

        let updated = editor.update_review_comment(id, "Updated text".to_string(), cx);
        assert!(updated);

        let snapshot = editor.buffer().read(cx).snapshot(cx);
        let comments = editor.comments_for_hunk(&key, &snapshot);
        assert_eq!(comments[0].comment, "Updated text");
        assert!(!comments[0].is_editing); // Should clear editing flag
    });
}

#[gpui::test]
fn test_review_comment_take_all(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    _ = editor.update(cx, |editor: &mut Editor, _window, cx| {
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        let anchor1 = snapshot.anchor_before(Point::new(0, 0));
        let anchor2 = snapshot.anchor_before(Point::new(0, 0));
        let key1 = test_hunk_key_with_anchor("file1.rs", anchor1);
        let key2 = test_hunk_key_with_anchor("file2.rs", anchor2);

        let id1 = add_test_comment(editor, key1.clone(), "Comment 1", cx);
        let id2 = add_test_comment(editor, key1.clone(), "Comment 2", cx);
        let id3 = add_test_comment(editor, key2.clone(), "Comment 3", cx);

        // IDs should be sequential starting from 0
        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        assert_eq!(id3, 2);

        assert_eq!(editor.total_review_comment_count(), 3);

        let taken = editor.take_all_review_comments(cx);

        // Should have 2 entries (one per hunk)
        assert_eq!(taken.len(), 2);

        // Total comments should be 3
        let total: usize = taken
            .iter()
            .map(|(_, comments): &(DiffHunkKey, Vec<StoredReviewComment>)| comments.len())
            .sum();
        assert_eq!(total, 3);

        // Storage should be empty
        assert_eq!(editor.total_review_comment_count(), 0);

        // After taking all comments, ID counter should reset
        // New comments should get IDs starting from 0 again
        let new_id1 = add_test_comment(editor, key1, "New Comment 1", cx);
        let new_id2 = add_test_comment(editor, key2, "New Comment 2", cx);

        assert_eq!(new_id1, 0, "ID counter should reset after take_all");
        assert_eq!(new_id2, 1, "IDs should be sequential after reset");
    });
}

#[gpui::test]
fn test_diff_review_overlay_show_and_dismiss(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    // Show overlay
    editor
        .update(cx, |editor, window, cx| {
            editor.show_diff_review_overlay(DisplayRow(0)..DisplayRow(0), window, cx);
        })
        .unwrap();

    // Verify overlay is shown
    editor
        .update(cx, |editor, _window, cx| {
            assert!(!editor.diff_review_overlays.is_empty());
            assert_eq!(editor.diff_review_line_range(cx), Some((0, 0)));
            assert!(editor.diff_review_prompt_editor().is_some());
        })
        .unwrap();

    // Dismiss overlay
    editor
        .update(cx, |editor, _window, cx| {
            editor.dismiss_all_diff_review_overlays(cx);
        })
        .unwrap();

    // Verify overlay is dismissed
    editor
        .update(cx, |editor, _window, cx| {
            assert!(editor.diff_review_overlays.is_empty());
            assert_eq!(editor.diff_review_line_range(cx), None);
            assert!(editor.diff_review_prompt_editor().is_none());
        })
        .unwrap();
}

#[gpui::test]
fn test_diff_review_overlay_dismiss_via_cancel(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    // Show overlay
    editor
        .update(cx, |editor, window, cx| {
            editor.show_diff_review_overlay(DisplayRow(0)..DisplayRow(0), window, cx);
        })
        .unwrap();

    // Verify overlay is shown
    editor
        .update(cx, |editor, _window, _cx| {
            assert!(!editor.diff_review_overlays.is_empty());
        })
        .unwrap();

    // Dismiss via dismiss_menus_and_popups (which is called by cancel action)
    editor
        .update(cx, |editor, window, cx| {
            editor.dismiss_menus_and_popups(true, window, cx);
        })
        .unwrap();

    // Verify overlay is dismissed
    editor
        .update(cx, |editor, _window, _cx| {
            assert!(editor.diff_review_overlays.is_empty());
        })
        .unwrap();
}

#[gpui::test]
fn test_diff_review_empty_comment_not_submitted(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    // Show overlay
    editor
        .update(cx, |editor, window, cx| {
            editor.show_diff_review_overlay(DisplayRow(0)..DisplayRow(0), window, cx);
        })
        .unwrap();

    // Try to submit without typing anything (empty comment)
    editor
        .update(cx, |editor, window, cx| {
            editor.submit_diff_review_comment(window, cx);
        })
        .unwrap();

    // Verify no comment was added
    editor
        .update(cx, |editor, _window, _cx| {
            assert_eq!(editor.total_review_comment_count(), 0);
        })
        .unwrap();

    // Try to submit with whitespace-only comment
    editor
        .update(cx, |editor, window, cx| {
            if let Some(prompt_editor) = editor.diff_review_prompt_editor().cloned() {
                prompt_editor.update(cx, |pe, cx| {
                    pe.insert("   \n\t  ", window, cx);
                });
            }
            editor.submit_diff_review_comment(window, cx);
        })
        .unwrap();

    // Verify still no comment was added
    editor
        .update(cx, |editor, _window, _cx| {
            assert_eq!(editor.total_review_comment_count(), 0);
        })
        .unwrap();
}

#[gpui::test]
fn test_diff_review_inline_edit_flow(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    // Add a comment directly
    let comment_id = editor
        .update(cx, |editor, _window, cx| {
            let key = test_hunk_key("");
            add_test_comment(editor, key, "Original comment", cx)
        })
        .unwrap();

    // Set comment to editing mode
    editor
        .update(cx, |editor, _window, cx| {
            editor.set_comment_editing(comment_id, true, cx);
        })
        .unwrap();

    // Verify editing flag is set
    editor
        .update(cx, |editor, _window, cx| {
            let key = test_hunk_key("");
            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let comments = editor.comments_for_hunk(&key, &snapshot);
            assert_eq!(comments.len(), 1);
            assert!(comments[0].is_editing);
        })
        .unwrap();

    // Update the comment
    editor
        .update(cx, |editor, _window, cx| {
            let updated =
                editor.update_review_comment(comment_id, "Updated comment".to_string(), cx);
            assert!(updated);
        })
        .unwrap();

    // Verify comment was updated and editing flag is cleared
    editor
        .update(cx, |editor, _window, cx| {
            let key = test_hunk_key("");
            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let comments = editor.comments_for_hunk(&key, &snapshot);
            assert_eq!(comments[0].comment, "Updated comment");
            assert!(!comments[0].is_editing);
        })
        .unwrap();
}

#[gpui::test]
fn test_orphaned_comments_are_cleaned_up(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    // Create an editor with some text
    let editor = cx.add_window(|window, cx| {
        let buffer = cx.new(|cx| Buffer::local("line 1\nline 2\nline 3\n", cx));
        let multi_buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
        Editor::new(EditorMode::full(), multi_buffer, None, window, cx)
    });

    // Add a comment with an anchor on line 2
    editor
        .update(cx, |editor, _window, cx| {
            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let anchor = snapshot.anchor_after(Point::new(1, 0)); // Line 2
            let key = DiffHunkKey {
                file_path: Arc::from(util::rel_path::RelPath::empty()),
                hunk_start_anchor: anchor,
            };
            editor.add_review_comment(key, "Comment on line 2".to_string(), anchor..anchor, cx);
            assert_eq!(editor.total_review_comment_count(), 1);
        })
        .unwrap();

    // Delete all content (this should orphan the comment's anchor)
    editor
        .update(cx, |editor, window, cx| {
            editor.select_all(&SelectAll, window, cx);
            editor.insert("completely new content", window, cx);
        })
        .unwrap();

    // Trigger cleanup
    editor
        .update(cx, |editor, _window, cx| {
            editor.cleanup_orphaned_review_comments(cx);
            // Comment should be removed because its anchor is invalid
            assert_eq!(editor.total_review_comment_count(), 0);
        })
        .unwrap();
}

#[gpui::test]
fn test_orphaned_comments_cleanup_called_on_buffer_edit(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    // Create an editor with some text
    let editor = cx.add_window(|window, cx| {
        let buffer = cx.new(|cx| Buffer::local("line 1\nline 2\nline 3\n", cx));
        let multi_buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
        Editor::new(EditorMode::full(), multi_buffer, None, window, cx)
    });

    // Add a comment with an anchor on line 2
    editor
        .update(cx, |editor, _window, cx| {
            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let anchor = snapshot.anchor_after(Point::new(1, 0)); // Line 2
            let key = DiffHunkKey {
                file_path: Arc::from(util::rel_path::RelPath::empty()),
                hunk_start_anchor: anchor,
            };
            editor.add_review_comment(key, "Comment on line 2".to_string(), anchor..anchor, cx);
            assert_eq!(editor.total_review_comment_count(), 1);
        })
        .unwrap();

    // Edit the buffer - this should trigger cleanup via on_buffer_event
    // Delete all content which orphans the anchor
    editor
        .update(cx, |editor, window, cx| {
            editor.select_all(&SelectAll, window, cx);
            editor.insert("completely new content", window, cx);
            // The cleanup is called automatically in on_buffer_event when Edited fires
        })
        .unwrap();

    // Verify cleanup happened automatically (not manually triggered)
    editor
        .update(cx, |editor, _window, _cx| {
            // Comment should be removed because its anchor became invalid
            // and cleanup was called automatically on buffer edit
            assert_eq!(editor.total_review_comment_count(), 0);
        })
        .unwrap();
}

#[gpui::test]
fn test_comments_stored_for_multiple_hunks(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    // This test verifies that comments can be stored for multiple different hunks
    // and that hunk_comment_count correctly identifies comments per hunk.
    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    _ = editor.update(cx, |editor, _window, cx| {
        let snapshot = editor.buffer().read(cx).snapshot(cx);

        // Create two different hunk keys (simulating two different files)
        let anchor = snapshot.anchor_before(Point::new(0, 0));
        let key1 = DiffHunkKey {
            file_path: Arc::from(util::rel_path::RelPath::from_unix_str("file1.rs").unwrap()),
            hunk_start_anchor: anchor,
        };
        let key2 = DiffHunkKey {
            file_path: Arc::from(util::rel_path::RelPath::from_unix_str("file2.rs").unwrap()),
            hunk_start_anchor: anchor,
        };

        // Add comments to first hunk
        editor.add_review_comment(
            key1.clone(),
            "Comment 1 for file1".to_string(),
            anchor..anchor,
            cx,
        );
        editor.add_review_comment(
            key1.clone(),
            "Comment 2 for file1".to_string(),
            anchor..anchor,
            cx,
        );

        // Add comment to second hunk
        editor.add_review_comment(
            key2.clone(),
            "Comment for file2".to_string(),
            anchor..anchor,
            cx,
        );

        // Verify total count
        assert_eq!(editor.total_review_comment_count(), 3);

        // Verify per-hunk counts
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        assert_eq!(
            editor.hunk_comment_count(&key1, &snapshot),
            2,
            "file1 should have 2 comments"
        );
        assert_eq!(
            editor.hunk_comment_count(&key2, &snapshot),
            1,
            "file2 should have 1 comment"
        );

        // Verify comments_for_hunk returns correct comments
        let file1_comments = editor.comments_for_hunk(&key1, &snapshot);
        assert_eq!(file1_comments.len(), 2);
        assert_eq!(file1_comments[0].comment, "Comment 1 for file1");
        assert_eq!(file1_comments[1].comment, "Comment 2 for file1");

        let file2_comments = editor.comments_for_hunk(&key2, &snapshot);
        assert_eq!(file2_comments.len(), 1);
        assert_eq!(file2_comments[0].comment, "Comment for file2");
    });
}

#[gpui::test]
fn test_same_hunk_detected_by_matching_keys(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    // This test verifies that hunk_keys_match correctly identifies when two
    // DiffHunkKeys refer to the same hunk (same file path and anchor point).
    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    _ = editor.update(cx, |editor, _window, cx| {
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        let anchor = snapshot.anchor_before(Point::new(0, 0));

        // Create two keys with the same file path and anchor
        let key1 = DiffHunkKey {
            file_path: Arc::from(util::rel_path::RelPath::from_unix_str("file.rs").unwrap()),
            hunk_start_anchor: anchor,
        };
        let key2 = DiffHunkKey {
            file_path: Arc::from(util::rel_path::RelPath::from_unix_str("file.rs").unwrap()),
            hunk_start_anchor: anchor,
        };

        // Add comment to first key
        editor.add_review_comment(key1, "Test comment".to_string(), anchor..anchor, cx);

        // Verify second key (same hunk) finds the comment
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        assert_eq!(
            editor.hunk_comment_count(&key2, &snapshot),
            1,
            "Same hunk should find the comment"
        );

        // Create a key with different file path
        let different_file_key = DiffHunkKey {
            file_path: Arc::from(util::rel_path::RelPath::from_unix_str("other.rs").unwrap()),
            hunk_start_anchor: anchor,
        };

        // Different file should not find the comment
        assert_eq!(
            editor.hunk_comment_count(&different_file_key, &snapshot),
            0,
            "Different file should not find the comment"
        );
    });
}

#[gpui::test]
fn test_overlay_comments_expanded_state(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    // This test verifies that set_diff_review_comments_expanded correctly
    // updates the expanded state of overlays.
    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    // Show overlay
    editor
        .update(cx, |editor, window, cx| {
            editor.show_diff_review_overlay(DisplayRow(0)..DisplayRow(0), window, cx);
        })
        .unwrap();

    // Verify initially expanded (default)
    editor
        .update(cx, |editor, _window, _cx| {
            assert!(
                editor.diff_review_overlays[0].comments_expanded,
                "Should be expanded by default"
            );
        })
        .unwrap();

    // Set to collapsed using the public method
    editor
        .update(cx, |editor, _window, cx| {
            editor.set_diff_review_comments_expanded(false, cx);
        })
        .unwrap();

    // Verify collapsed
    editor
        .update(cx, |editor, _window, _cx| {
            assert!(
                !editor.diff_review_overlays[0].comments_expanded,
                "Should be collapsed after setting to false"
            );
        })
        .unwrap();

    // Set back to expanded
    editor
        .update(cx, |editor, _window, cx| {
            editor.set_diff_review_comments_expanded(true, cx);
        })
        .unwrap();

    // Verify expanded again
    editor
        .update(cx, |editor, _window, _cx| {
            assert!(
                editor.diff_review_overlays[0].comments_expanded,
                "Should be expanded after setting to true"
            );
        })
        .unwrap();
}

#[gpui::test]
fn test_diff_review_multiline_selection(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    // Create an editor with multiple lines of text
    let editor = cx.add_window(|window, cx| {
        let buffer = cx.new(|cx| Buffer::local("line 1\nline 2\nline 3\nline 4\nline 5\n", cx));
        let multi_buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
        Editor::new(EditorMode::full(), multi_buffer, None, window, cx)
    });

    // Test showing overlay with a multi-line selection (lines 1-3, which are rows 0-2)
    editor
        .update(cx, |editor, window, cx| {
            editor.show_diff_review_overlay(DisplayRow(0)..DisplayRow(2), window, cx);
        })
        .unwrap();

    // Verify line range
    editor
        .update(cx, |editor, _window, cx| {
            assert!(!editor.diff_review_overlays.is_empty());
            assert_eq!(editor.diff_review_line_range(cx), Some((0, 2)));
        })
        .unwrap();

    // Dismiss and test with reversed range (end < start)
    editor
        .update(cx, |editor, _window, cx| {
            editor.dismiss_all_diff_review_overlays(cx);
        })
        .unwrap();

    // Show overlay with reversed range - should normalize it
    editor
        .update(cx, |editor, window, cx| {
            editor.show_diff_review_overlay(DisplayRow(3)..DisplayRow(1), window, cx);
        })
        .unwrap();

    // Verify range is normalized (start <= end)
    editor
        .update(cx, |editor, _window, cx| {
            assert_eq!(editor.diff_review_line_range(cx), Some((1, 3)));
        })
        .unwrap();
}

#[gpui::test]
fn test_diff_review_drag_state(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| {
        let buffer = cx.new(|cx| Buffer::local("line 1\nline 2\nline 3\n", cx));
        let multi_buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
        Editor::new(EditorMode::full(), multi_buffer, None, window, cx)
    });

    // Initially no drag state
    editor
        .update(cx, |editor, _window, _cx| {
            assert!(editor.diff_review_drag_state.is_none());
        })
        .unwrap();

    // Start drag at row 1
    editor
        .update(cx, |editor, window, cx| {
            editor.start_diff_review_drag(DisplayRow(1), window, cx);
        })
        .unwrap();

    // Verify drag state is set
    editor
        .update(cx, |editor, window, cx| {
            assert!(editor.diff_review_drag_state.is_some());
            let snapshot = editor.snapshot(window, cx);
            let range = editor
                .diff_review_drag_state
                .as_ref()
                .unwrap()
                .row_range(&snapshot.display_snapshot);
            assert_eq!(*range.start(), DisplayRow(1));
            assert_eq!(*range.end(), DisplayRow(1));
        })
        .unwrap();

    // Update drag to row 3
    editor
        .update(cx, |editor, window, cx| {
            editor.update_diff_review_drag(DisplayRow(3), window, cx);
        })
        .unwrap();

    // Verify drag state is updated
    editor
        .update(cx, |editor, window, cx| {
            assert!(editor.diff_review_drag_state.is_some());
            let snapshot = editor.snapshot(window, cx);
            let range = editor
                .diff_review_drag_state
                .as_ref()
                .unwrap()
                .row_range(&snapshot.display_snapshot);
            assert_eq!(*range.start(), DisplayRow(1));
            assert_eq!(*range.end(), DisplayRow(3));
        })
        .unwrap();

    // End drag - should show overlay
    editor
        .update(cx, |editor, window, cx| {
            editor.end_diff_review_drag(window, cx);
        })
        .unwrap();

    // Verify drag state is cleared and overlay is shown
    editor
        .update(cx, |editor, _window, cx| {
            assert!(editor.diff_review_drag_state.is_none());
            assert!(!editor.diff_review_overlays.is_empty());
            assert_eq!(editor.diff_review_line_range(cx), Some((1, 3)));
        })
        .unwrap();
}

#[gpui::test]
fn test_diff_review_drag_cancel(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    // Start drag
    editor
        .update(cx, |editor, window, cx| {
            editor.start_diff_review_drag(DisplayRow(0), window, cx);
        })
        .unwrap();

    // Verify drag state is set
    editor
        .update(cx, |editor, _window, _cx| {
            assert!(editor.diff_review_drag_state.is_some());
        })
        .unwrap();

    // Cancel drag
    editor
        .update(cx, |editor, _window, cx| {
            editor.cancel_diff_review_drag(cx);
        })
        .unwrap();

    // Verify drag state is cleared and no overlay was created
    editor
        .update(cx, |editor, _window, _cx| {
            assert!(editor.diff_review_drag_state.is_none());
            assert!(editor.diff_review_overlays.is_empty());
        })
        .unwrap();
}

#[gpui::test]
fn test_calculate_overlay_height(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    // This test verifies that calculate_overlay_height returns correct heights
    // based on comment count and expanded state.
    let editor = cx.add_window(|window, cx| Editor::single_line(window, cx));

    _ = editor.update(cx, |editor, _window, cx| {
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        let anchor = snapshot.anchor_before(Point::new(0, 0));
        let key = DiffHunkKey {
            file_path: Arc::from(util::rel_path::RelPath::empty()),
            hunk_start_anchor: anchor,
        };

        // No comments: base height of 2
        let height_no_comments = editor.calculate_overlay_height(&key, true, &snapshot);
        assert_eq!(
            height_no_comments, 2,
            "Base height should be 2 with no comments"
        );

        // Add one comment
        editor.add_review_comment(key.clone(), "Comment 1".to_string(), anchor..anchor, cx);

        let snapshot = editor.buffer().read(cx).snapshot(cx);

        // With comments expanded: base (2) + header (1) + 2 per comment
        let height_expanded = editor.calculate_overlay_height(&key, true, &snapshot);
        assert_eq!(
            height_expanded,
            2 + 1 + 2, // base + header + 1 comment * 2
            "Height with 1 comment expanded"
        );

        // With comments collapsed: base (2) + header (1)
        let height_collapsed = editor.calculate_overlay_height(&key, false, &snapshot);
        assert_eq!(
            height_collapsed,
            2 + 1, // base + header only
            "Height with comments collapsed"
        );

        // Add more comments
        editor.add_review_comment(key.clone(), "Comment 2".to_string(), anchor..anchor, cx);
        editor.add_review_comment(key.clone(), "Comment 3".to_string(), anchor..anchor, cx);

        let snapshot = editor.buffer().read(cx).snapshot(cx);

        // With 3 comments expanded
        let height_3_expanded = editor.calculate_overlay_height(&key, true, &snapshot);
        assert_eq!(
            height_3_expanded,
            2 + 1 + (3 * 2), // base + header + 3 comments * 2
            "Height with 3 comments expanded"
        );

        // Collapsed height stays the same regardless of comment count
        let height_3_collapsed = editor.calculate_overlay_height(&key, false, &snapshot);
        assert_eq!(
            height_3_collapsed,
            2 + 1, // base + header only
            "Height with 3 comments collapsed should be same as 1 comment collapsed"
        );
    });
}

#[gpui::test]
async fn test_move_to_start_end_of_larger_syntax_node_single_cursor(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(Language::new(
        LanguageConfig::default(),
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    let text = r#"
        fn main() {
            let x = foo(1, 2);
        }
    "#
    .unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    // Test case 1: Move to end of syntax nodes
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(1), 16)..DisplayPoint::new(DisplayRow(1), 16)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(ˇ1, 2);
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_end_of_larger_syntax_node(&MoveToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(1ˇ, 2);
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_end_of_larger_syntax_node(&MoveToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(1, 2)ˇ;
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_end_of_larger_syntax_node(&MoveToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(1, 2);ˇ
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_end_of_larger_syntax_node(&MoveToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(1, 2);
                }ˇ
            "#},
            cx,
        );
    });

    // Test case 2: Move to start of syntax nodes
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(1), 20)..DisplayPoint::new(DisplayRow(1), 20)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(1, 2ˇ);
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_start_of_larger_syntax_node(&MoveToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = fooˇ(1, 2);
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_start_of_larger_syntax_node(&MoveToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = ˇfoo(1, 2);
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_start_of_larger_syntax_node(&MoveToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    ˇlet x = foo(1, 2);
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_start_of_larger_syntax_node(&MoveToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() ˇ{
                    let x = foo(1, 2);
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_start_of_larger_syntax_node(&MoveToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                ˇfn main() {
                    let x = foo(1, 2);
                }
            "#},
            cx,
        );
    });
}

#[gpui::test]
async fn test_move_to_start_end_of_larger_syntax_node_two_cursors(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(Language::new(
        LanguageConfig::default(),
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    let text = r#"
        fn main() {
            let x = foo(1, 2);
            let y = bar(3, 4);
        }
    "#
    .unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    // Test case 1: Move to end of syntax nodes with two cursors
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(1), 20)..DisplayPoint::new(DisplayRow(1), 20),
                DisplayPoint::new(DisplayRow(2), 20)..DisplayPoint::new(DisplayRow(2), 20),
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(1, 2ˇ);
                    let y = bar(3, 4ˇ);
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_end_of_larger_syntax_node(&MoveToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(1, 2)ˇ;
                    let y = bar(3, 4)ˇ;
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_end_of_larger_syntax_node(&MoveToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(1, 2);ˇ
                    let y = bar(3, 4);ˇ
                }
            "#},
            cx,
        );
    });

    // Test case 2: Move to start of syntax nodes with two cursors
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(1), 19)..DisplayPoint::new(DisplayRow(1), 19),
                DisplayPoint::new(DisplayRow(2), 19)..DisplayPoint::new(DisplayRow(2), 19),
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(1, ˇ2);
                    let y = bar(3, ˇ4);
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_start_of_larger_syntax_node(&MoveToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = fooˇ(1, 2);
                    let y = barˇ(3, 4);
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_start_of_larger_syntax_node(&MoveToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = ˇfoo(1, 2);
                    let y = ˇbar(3, 4);
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_start_of_larger_syntax_node(&MoveToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    ˇlet x = foo(1, 2);
                    ˇlet y = bar(3, 4);
                }
            "#},
            cx,
        );
    });
}

#[gpui::test]
async fn test_move_to_start_end_of_larger_syntax_node_with_selections_and_strings(
    cx: &mut TestAppContext,
) {
    init_test(cx, |_| {});

    let language = Arc::new(Language::new(
        LanguageConfig::default(),
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    let text = r#"
        fn main() {
            let x = foo(1, 2);
            let msg = "hello world";
        }
    "#
    .unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language, cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    // Test case 1: With existing selection, move_to_end keeps selection
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(1), 12)..DisplayPoint::new(DisplayRow(1), 21)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = «foo(1, 2)ˇ»;
                    let msg = "hello world";
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_end_of_larger_syntax_node(&MoveToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = «foo(1, 2)ˇ»;
                    let msg = "hello world";
                }
            "#},
            cx,
        );
    });

    // Test case 2: Move to end within a string
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(2), 15)..DisplayPoint::new(DisplayRow(2), 15)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(1, 2);
                    let msg = "ˇhello world";
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_end_of_larger_syntax_node(&MoveToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(1, 2);
                    let msg = "hello worldˇ";
                }
            "#},
            cx,
        );
    });

    // Test case 3: Move to start within a string
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(2), 21)..DisplayPoint::new(DisplayRow(2), 21)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(1, 2);
                    let msg = "hello ˇworld";
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.move_to_start_of_larger_syntax_node(&MoveToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x = foo(1, 2);
                    let msg = "ˇhello world";
                }
            "#},
            cx,
        );
    });
}

#[gpui::test]
async fn test_select_to_start_end_of_larger_syntax_node(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let language = Arc::new(Language::new(
        LanguageConfig::default(),
        Some(tree_sitter_rust::LANGUAGE.into()),
    ));

    // Test Group 1.1: Cursor in String - First Jump (Select to End)
    let text = r#"let msg = "foo bar baz";"#.unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language.clone(), cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 14)..DisplayPoint::new(DisplayRow(0), 14)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let msg = "fooˇ bar baz";"#}, cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let msg = "foo« bar bazˇ»";"#}, cx);
    });

    // Test Group 1.2: Cursor in String - Second Jump (Select to End)
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let msg = "foo« bar baz"ˇ»;"#}, cx);
    });

    // Test Group 1.3: Cursor in String - Third Jump (Select to End)
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let msg = "foo« bar baz";ˇ»"#}, cx);
    });

    // Test Group 1.4: Cursor in String - First Jump (Select to Start)
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 18)..DisplayPoint::new(DisplayRow(0), 18)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let msg = "foo barˇ baz";"#}, cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_start_of_larger_syntax_node(&SelectToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let msg = "«ˇfoo bar» baz";"#}, cx);
    });

    // Test Group 1.5: Cursor in String - Second Jump (Select to Start)
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_start_of_larger_syntax_node(&SelectToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let msg = «ˇ"foo bar» baz";"#}, cx);
    });

    // Test Group 1.6: Cursor in String - Third Jump (Select to Start)
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_start_of_larger_syntax_node(&SelectToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"«ˇlet msg = "foo bar» baz";"#}, cx);
    });

    // Test Group 2.1: Let Statement Progression (Select to End)
    let text = r#"
fn main() {
    let x = "hello";
}
"#
    .unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language.clone(), cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(1), 9)..DisplayPoint::new(DisplayRow(1), 9)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let xˇ = "hello";
                }
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r##"
                fn main() {
                    let x« = "hello";ˇ»
                }
            "##},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                fn main() {
                    let x« = "hello";
                }ˇ»
            "#},
            cx,
        );
    });

    // Test Group 2.2a: From Inside String Content Node To String Content Boundary
    let text = r#"let x = "hello";"#.unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language.clone(), cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 12)..DisplayPoint::new(DisplayRow(0), 12)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let x = "helˇlo";"#}, cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_start_of_larger_syntax_node(&SelectToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let x = "«ˇhel»lo";"#}, cx);
    });

    // Test Group 2.2b: From Edge of String Content Node To String Literal Boundary
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 9)..DisplayPoint::new(DisplayRow(0), 9)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let x = "ˇhello";"#}, cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_start_of_larger_syntax_node(&SelectToStartOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let x = «ˇ"»hello";"#}, cx);
    });

    // Test Group 3.1: Create Selection from Cursor (Select to End)
    let text = r#"let x = "hello world";"#.unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language.clone(), cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 14)..DisplayPoint::new(DisplayRow(0), 14)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let x = "helloˇ world";"#}, cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let x = "hello« worldˇ»";"#}, cx);
    });

    // Test Group 3.2: Extend Existing Selection (Select to End)
    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 11)..DisplayPoint::new(DisplayRow(0), 17)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let x = "he«llo woˇ»rld";"#}, cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let x = "he«llo worldˇ»";"#}, cx);
    });

    // Test Group 4.1: Multiple Cursors - All Expand to Different Syntax Nodes
    let text = r#"let x = "hello"; let y = 42;"#.unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language.clone(), cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                // Cursor inside string content
                DisplayPoint::new(DisplayRow(0), 12)..DisplayPoint::new(DisplayRow(0), 12),
                // Cursor at let statement semicolon
                DisplayPoint::new(DisplayRow(0), 18)..DisplayPoint::new(DisplayRow(0), 18),
                // Cursor inside integer literal
                DisplayPoint::new(DisplayRow(0), 26)..DisplayPoint::new(DisplayRow(0), 26),
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let x = "helˇlo"; lˇet y = 4ˇ2;"#}, cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let x = "hel«loˇ»"; l«et y = 42;ˇ»"#}, cx);
    });

    // Test Group 4.2: Multiple Cursors on Separate Lines
    let text = r#"
let x = "hello";
let y = 42;
"#
    .unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language.clone(), cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 12)..DisplayPoint::new(DisplayRow(0), 12),
                DisplayPoint::new(DisplayRow(1), 9)..DisplayPoint::new(DisplayRow(1), 9),
            ]);
        });
    });

    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                let x = "helˇlo";
                let y = 4ˇ2;
            "#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
                let x = "hel«loˇ»";
                let y = 4«2ˇ»;
            "#},
            cx,
        );
    });

    // Test Group 5.1: Nested Function Calls
    let text = r#"let result = foo(bar("arg"));"#.unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language.clone(), cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 22)..DisplayPoint::new(DisplayRow(0), 22)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let result = foo(bar("ˇarg"));"#}, cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let result = foo(bar("«argˇ»"));"#}, cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let result = foo(bar("«arg"ˇ»));"#}, cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let result = foo(bar("«arg")ˇ»);"#}, cx);
    });

    // Test Group 6.1: Block Comments
    let text = r#"let x = /* multi
                             line
                             comment */;"#
        .unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language.clone(), cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 16)..DisplayPoint::new(DisplayRow(0), 16)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
let x = /* multiˇ
line
comment */;"#},
            cx,
        );
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(
            editor,
            indoc! {r#"
let x = /* multi«
line
comment */ˇ»;"#},
            cx,
        );
    });

    // Test Group 6.2: Array/Vector Literals
    let text = r#"let arr = [1, 2, 3];"#.unindent();

    let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language.clone(), cx));
    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
    let (editor, cx) = cx.add_window_view(|window, cx| build_editor(buffer, window, cx));

    editor
        .condition::<crate::EditorEvent>(cx, |editor, cx| !editor.buffer.read(cx).is_parsing(cx))
        .await;

    editor.update_in(cx, |editor, window, cx| {
        editor.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select_display_ranges([
                DisplayPoint::new(DisplayRow(0), 11)..DisplayPoint::new(DisplayRow(0), 11)
            ]);
        });
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let arr = [ˇ1, 2, 3];"#}, cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let arr = [«1ˇ», 2, 3];"#}, cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.select_to_end_of_larger_syntax_node(&SelectToEndOfLargerSyntaxNode, window, cx);
    });
    editor.update(cx, |editor, cx| {
        assert_text_with_selections(editor, indoc! {r#"let arr = [«1, 2, 3]ˇ»;"#}, cx);
    });
}

#[gpui::test]
async fn test_restore_and_next(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let diff_base = r#"
        one
        two
        three
        four
        five
        "#
    .unindent();

    cx.set_state(
        &r#"
        ONE
        two
        ˇTHREE
        four
        FIVE
        "#
        .unindent(),
    );
    cx.set_head_text(&diff_base);

    cx.update_editor(|editor, window, cx| {
        editor.set_expand_all_diff_hunks(cx);
        editor.restore_and_next(&Default::default(), window, cx);
    });
    cx.run_until_parked();

    cx.assert_state_with_diff(
        r#"
        - one
        + ONE
          two
          three
          four
        - ˇfive
        + FIVE
        "#
        .unindent(),
    );

    cx.update_editor(|editor, window, cx| {
        editor.restore_and_next(&Default::default(), window, cx);
    });
    cx.run_until_parked();

    cx.assert_state_with_diff(
        r#"
        - one
        + ONE
          two
          three
          four
          ˇfive
        "#
        .unindent(),
    );
}

#[gpui::test]
async fn test_align_selections(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    // 1) one cursor, no action
    let before = " abc\n  abc\nabc\n     ˇabc";
    cx.set_state(before);
    cx.update_editor(|e, window, cx| e.align_selections(&AlignSelections, window, cx));
    cx.assert_editor_state(before);

    // 2) multiple cursors at different rows
    let before = indoc!(
        r#"
            let aˇbc = 123;
            let  xˇyz = 456;
            let   fˇoo = 789;
            let    bˇar = 0;
        "#
    );
    let after = indoc!(
        r#"
            let a   ˇbc = 123;
            let  x  ˇyz = 456;
            let   f ˇoo = 789;
            let    bˇar = 0;
        "#
    );
    cx.set_state(before);
    cx.update_editor(|e, window, cx| e.align_selections(&AlignSelections, window, cx));
    cx.assert_editor_state(after);

    // 3) multiple selections at different rows
    let before = indoc!(
        r#"
            let «ˇabc» = 123;
            let  «ˇxyz» = 456;
            let   «ˇfoo» = 789;
            let    «ˇbar» = 0;
        "#
    );
    let after = indoc!(
        r#"
            let    «ˇabc» = 123;
            let    «ˇxyz» = 456;
            let    «ˇfoo» = 789;
            let    «ˇbar» = 0;
        "#
    );
    cx.set_state(before);
    cx.update_editor(|e, window, cx| e.align_selections(&AlignSelections, window, cx));
    cx.assert_editor_state(after);

    // 4) multiple selections at different rows, inverted head
    let before = indoc!(
        r#"
            let    «abcˇ» = 123;
            // comment
            let  «xyzˇ» = 456;
            let «fooˇ» = 789;
            let    «barˇ» = 0;
        "#
    );
    let after = indoc!(
        r#"
            let    «abcˇ» = 123;
            // comment
            let    «xyzˇ» = 456;
            let    «fooˇ» = 789;
            let    «barˇ» = 0;
        "#
    );
    cx.set_state(before);
    cx.update_editor(|e, window, cx| e.align_selections(&AlignSelections, window, cx));
    cx.assert_editor_state(after);
}

#[gpui::test]
async fn test_align_selections_multicolumn(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    // 1) Multicolumn, one non affected editor row
    let before = indoc!(
        r#"
            name «|ˇ» age «|ˇ» height «|ˇ» note
            Matthew «|ˇ» 7 «|ˇ» 2333 «|ˇ» smart
            Mike «|ˇ» 1234 «|ˇ» 567 «|ˇ» lazy
            Anything that is not selected
            Miles «|ˇ» 88 «|ˇ» 99 «|ˇ» funny
        "#
    );
    let after = indoc!(
        r#"
            name    «|ˇ» age  «|ˇ» height «|ˇ» note
            Matthew «|ˇ» 7    «|ˇ» 2333   «|ˇ» smart
            Mike    «|ˇ» 1234 «|ˇ» 567    «|ˇ» lazy
            Anything that is not selected
            Miles   «|ˇ» 88   «|ˇ» 99     «|ˇ» funny
        "#
    );
    cx.set_state(before);
    cx.update_editor(|e, window, cx| e.align_selections(&AlignSelections, window, cx));
    cx.assert_editor_state(after);

    // 2) not all alignment rows has the number of alignment columns
    let before = indoc!(
        r#"
            name «|ˇ» age «|ˇ» height
            Matthew «|ˇ» 7 «|ˇ» 2333
            Mike «|ˇ» 1234
            Miles «|ˇ» 88 «|ˇ» 99
        "#
    );
    let after = indoc!(
        r#"
            name    «|ˇ» age «|ˇ» height
            Matthew «|ˇ» 7   «|ˇ» 2333
            Mike    «|ˇ» 1234
            Miles   «|ˇ» 88  «|ˇ» 99
        "#
    );
    cx.set_state(before);
    cx.update_editor(|e, window, cx| e.align_selections(&AlignSelections, window, cx));
    cx.assert_editor_state(after);

    // 3) A aligned column shall stay aligned
    let before = indoc!(
        r#"
            $ ˇa    ˇa
            $  ˇa   ˇa
            $   ˇa  ˇa
            $    ˇa ˇa
        "#
    );
    let after = indoc!(
        r#"
            $    ˇa    ˇa
            $    ˇa    ˇa
            $    ˇa    ˇa
            $    ˇa    ˇa
        "#
    );
    cx.set_state(before);
    cx.update_editor(|e, window, cx| e.align_selections(&AlignSelections, window, cx));
    cx.assert_editor_state(after);
}

#[gpui::test]
async fn test_align_selections_with_multibyte_chars(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    // The reported case: `←` and `π` take more bytes than they take columns, so
    // aligning on the buffer column padded the first row one space too far.
    let before = "a ← 1  ˇ# one\nbc ← π  ˇ# two";
    let after = "a ← 1   ˇ# one\nbc ← π  ˇ# two";
    cx.set_state(before);
    cx.update_editor(|e, window, cx| e.align_selections(&AlignSelections, window, cx));
    cx.assert_editor_state(after);

    // A multi-byte character before the first column also has to shift the
    // offset that is carried into the second column.
    let before = "π aˇ bbbˇc\nxy aˇ bˇc";
    let after = "π a ˇ bbbˇc\nxy aˇ b  ˇc";
    cx.set_state(before);
    cx.update_editor(|e, window, cx| e.align_selections(&AlignSelections, window, cx));
    cx.assert_editor_state(after);

    // The display map expands tabs before the row is laid out, so a leading tab
    // counts as its expanded width rather than as one byte.
    let before = "\taˇbc\nxyzaˇbc";
    let after = "\taˇbc\nxyza ˇbc";
    cx.set_state(before);
    cx.update_editor(|e, window, cx| e.align_selections(&AlignSelections, window, cx));
    cx.assert_editor_state(after);

    // A non-BMP character advances two columns, so counting characters rather
    // than measuring advances would pad this row twice as far as it needs.
    let before = "😀ˇb\nxyzˇb";
    let after = "😀 ˇb\nxyzˇb";
    cx.set_state(before);
    cx.update_editor(|e, window, cx| e.align_selections(&AlignSelections, window, cx));
    cx.assert_editor_state(after);

    // Multi-byte characters after the cursors do not move them.
    let before = "abˇ←z\ncdˇqz";
    cx.set_state(before);
    cx.update_editor(|e, window, cx| e.align_selections(&AlignSelections, window, cx));
    cx.assert_editor_state(before);
}

#[gpui::test]
async fn test_align_selections_with_soft_wrap(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;

    let before = "aaaaaaaaaaaaˇx\nbbbbbˇy";
    let after = "aaaaaaaaaaaaˇx\nbbbbb       ˇy";
    cx.set_state(before);
    cx.update_editor(|e, _, cx| e.set_wrap_width(Some(100.0.into()), cx));
    cx.update_editor(|e, window, cx| {
        assert!(e.display_text(cx).lines().count() > 2);
        e.align_selections(&AlignSelections, window, cx)
    });
    cx.assert_editor_state(after);
}

#[gpui::test]
async fn test_custom_fallback_highlights(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state(indoc! {"fn main(self, variable: TType) {ˇ}"});

    let variable_color = Hsla::green();
    let function_color = Hsla::blue();

    let test_cases = [
        ("@variable", Some(variable_color)),
        ("@type", None),
        ("@type @variable", Some(variable_color)),
        ("@variable @type", Some(variable_color)),
        ("@variable @function", Some(function_color)),
        ("@function @variable", Some(variable_color)),
    ];

    for (test_case, expected) in test_cases {
        let custom_rust_lang = Arc::into_inner(rust_lang())
            .unwrap()
            .with_highlights_query(format! {r#"(type_identifier) {test_case}"#}.as_str())
            .unwrap();
        let theme = setup_syntax_highlighting(Arc::new(custom_rust_lang), &mut cx);
        let expected = expected.map_or_else(Vec::new, |expected_color| {
            vec![(24..29, HighlightStyle::color(expected_color))]
        });

        cx.update_editor(|editor, window, cx| {
            let snapshot = editor.snapshot(window, cx);
            assert_eq!(
                expected,
                snapshot.combined_highlights(MultiBufferOffset(0)..snapshot.buffer().len(), &theme),
                "Test case with '{test_case}' highlights query did not pass",
            );
        });
    }
}

#[gpui::test]
async fn test_tsx_nested_jsx_member_expression_highlights(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;
    cx.set_state("<A.B.C></A.B.C>ˇ;");

    let language = Arc::new(
        Language::new(
            LanguageConfig {
                name: "TSX".into(),
                matcher: (LanguageMatcher {
                    path_suffixes: vec!["tsx".to_string()],
                    ..LanguageMatcher::default()
                })
                .into(),
                ..LanguageConfig::default()
            },
            Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        )
        .with_highlights_query(include_str!("../../grammars/src/tsx/highlights.scm"))
        .unwrap(),
    );

    let component_color = Hsla::green();
    let theme = Arc::new(SyntaxTheme::new_test(vec![
        ("tag.component.jsx", component_color),
        ("type", Hsla::blue()),
        ("property", Hsla::red()),
        ("punctuation.bracket", Hsla::default()),
        ("punctuation.delimiter", Hsla::default()),
    ]));
    setup_syntax_highlighting_with_theme(language, theme.clone(), &mut cx);
    cx.update_editor(|editor, window, cx| {
        let snapshot = editor.snapshot(window, cx);
        assert_eq!(
            snapshot
                .combined_highlights(MultiBufferOffset(0)..snapshot.buffer().len(), &theme)
                .iter()
                .filter(|(_, style)| *style == HighlightStyle::color(component_color))
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                (1..2, HighlightStyle::color(component_color)),
                (3..4, HighlightStyle::color(component_color)),
                (5..6, HighlightStyle::color(component_color)),
                (9..10, HighlightStyle::color(component_color)),
                (11..12, HighlightStyle::color(component_color)),
                (13..14, HighlightStyle::color(component_color)),
            ],
        );
    });
}

fn setup_syntax_highlighting(
    language: Arc<Language>,
    cx: &mut EditorTestContext,
) -> Arc<SyntaxTheme> {
    let syntax = Arc::new(SyntaxTheme::new_test(vec![
        ("keyword", Hsla::red()),
        ("function", Hsla::blue()),
        ("variable", Hsla::green()),
        ("number", Hsla::default()),
        ("operator", Hsla::default()),
        ("punctuation.bracket", Hsla::default()),
        ("punctuation.delimiter", Hsla::default()),
    ]));

    setup_syntax_highlighting_with_theme(language, syntax.clone(), cx);
    syntax
}

fn setup_syntax_highlighting_with_theme(
    language: Arc<Language>,
    syntax: Arc<SyntaxTheme>,
    cx: &mut EditorTestContext,
) {
    language.set_theme(&syntax);

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(language), cx));
    cx.executor().run_until_parked();
    cx.update_editor(|editor, window, cx| {
        editor.set_style(
            EditorStyle {
                syntax,
                ..EditorStyle::default()
            },
            window,
            cx,
        );
    });
}

#[gpui::test]
async fn test_columnar_selection_with_multibyte_chars(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    // The middle row contains a 2-byte char (ã) before the dragged column. A
    // column selection that uses byte columns directly puts the ã row's
    // selection at a different visual position than the ASCII rows; anchoring
    // in x pixels keeps all rows at the same character offset.
    cx.set_state(indoc! {"
        ˇabcde
        abcde
        aãcde
        abcde
        abcde
    "});

    // Drag column-wise from (row 0, col 0) past the ã column on every row.
    cx.update_editor(|editor, window, cx| {
        editor.select(
            SelectPhase::BeginColumnar {
                position: DisplayPoint::new(DisplayRow(0), 0),
                goal_column: 0,
                reset: true,
                mode: ColumnarMode::FromMouse,
            },
            window,
            cx,
        );
        editor.select(
            SelectPhase::Update {
                position: DisplayPoint::new(DisplayRow(4), 4),
                goal_column: 4,
                scroll_delta: gpui::Point::default(),
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(indoc! {"
        «abcdˇ»e
        «abcdˇ»e
        «aãcdˇ»e
        «abcdˇ»e
        «abcdˇ»e
    "});

    // Control: drag stops before the ã column, where byte columns and x
    // positions agree.
    cx.update_editor(|editor, window, cx| {
        editor.select(
            SelectPhase::BeginColumnar {
                position: DisplayPoint::new(DisplayRow(0), 0),
                goal_column: 0,
                reset: true,
                mode: ColumnarMode::FromMouse,
            },
            window,
            cx,
        );
        editor.select(
            SelectPhase::Update {
                position: DisplayPoint::new(DisplayRow(4), 1),
                goal_column: 1,
                scroll_delta: gpui::Point::default(),
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(indoc! {"
        «aˇ»bcde
        «aˇ»bcde
        «aˇ»ãcde
        «aˇ»bcde
        «aˇ»bcde
    "});
}

#[gpui::test]
async fn test_columnar_selection_past_end_of_line(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"
        ˇaaaaaaaaaa
        bb
        cccccccccc
    "});

    // Drag from the start of the long first row to a point past the EOL of
    // the short second row: the mouse handlers encode that as the nearest
    // valid position (1, 2) plus an unclipped goal column of 8. The rectangle
    // must keep tracking the mouse x on the long row instead of collapsing to
    // the short row's width.
    cx.update_editor(|editor, window, cx| {
        editor.select(
            SelectPhase::BeginColumnar {
                position: DisplayPoint::new(DisplayRow(0), 0),
                goal_column: 0,
                reset: true,
                mode: ColumnarMode::FromMouse,
            },
            window,
            cx,
        );
        editor.select(
            SelectPhase::Update {
                position: DisplayPoint::new(DisplayRow(1), 2),
                goal_column: 8,
                scroll_delta: gpui::Point::default(),
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(indoc! {"
        «aaaaaaaaˇ»aa
        «bbˇ»
        cccccccccc
    "});

    // Starting the drag past the EOL of the short row must anchor that edge
    // of the rectangle at the click position, not at the short row's EOL.
    cx.update_editor(|editor, window, cx| {
        editor.select(
            SelectPhase::BeginColumnar {
                position: DisplayPoint::new(DisplayRow(1), 2),
                goal_column: 8,
                reset: true,
                mode: ColumnarMode::FromMouse,
            },
            window,
            cx,
        );
        editor.select(
            SelectPhase::Update {
                position: DisplayPoint::new(DisplayRow(2), 4),
                goal_column: 4,
                scroll_delta: gpui::Point::default(),
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(indoc! {"
        aaaaaaaaaa
        bb
        cccc«ˇcccc»cc
    "});
}

#[gpui::test]
async fn test_columnar_selection_with_soft_wrap(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    cx.set_state(indoc! {"
        ˇ1. Very long line to show how a wrapped line would look
        2. Very long line to show how a wrapped line would look
    "});

    let soft_wrap_second_line_row = |editor: &mut Editor, cx: &mut Context<Editor>| {
        editor.set_wrap_width(Some(100.0.into()), cx);
        let snapshot = editor.display_snapshot(cx);
        let second_line_row = Point::new(1, 0).to_display_point(&snapshot).row();
        assert!(
            second_line_row.0 > 1,
            "expected the first line to be soft wrapped"
        );
        second_line_row
    };

    // Dragging a columnar selection across the soft-wrapped rows of the first
    // line must produce one selection per buffer line, not one per display row.
    cx.update_editor(|editor, window, cx| {
        let second_line_row = soft_wrap_second_line_row(editor, cx);
        editor.select(
            SelectPhase::BeginColumnar {
                position: DisplayPoint::new(DisplayRow(0), 0),
                goal_column: 0,
                reset: true,
                mode: ColumnarMode::FromMouse,
            },
            window,
            cx,
        );
        editor.select(
            SelectPhase::Update {
                position: DisplayPoint::new(second_line_row, 1),
                goal_column: 1,
                scroll_delta: gpui::Point::default(),
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(indoc! {"
        «1ˇ». Very long line to show how a wrapped line would look
        «2ˇ». Very long line to show how a wrapped line would look
    "});

    cx.update_editor(|editor, window, cx| {
        let second_line_row = soft_wrap_second_line_row(editor, cx);
        editor.select(
            SelectPhase::BeginColumnar {
                position: DisplayPoint::new(DisplayRow(0), 0),
                goal_column: 0,
                reset: true,
                mode: ColumnarMode::FromMouse,
            },
            window,
            cx,
        );
        editor.select(
            SelectPhase::Update {
                position: DisplayPoint::new(second_line_row, 0),
                goal_column: 0,
                scroll_delta: gpui::Point::default(),
            },
            window,
            cx,
        );
    });

    cx.assert_editor_state(indoc! {"
        ˇ1. Very long line to show how a wrapped line would look
        ˇ2. Very long line to show how a wrapped line would look
    "});
}

#[gpui::test]
async fn test_toggle_markdown_block_quote(cx: &mut TestAppContext) {
    init_test(cx, |_| {});

    let mut cx = EditorTestContext::new(cx).await;

    // No-op with no language
    cx.set_state(indoc! {"
        «helloˇ» world
    "});
    cx.update_editor(|e, window, cx| e.toggle_markdown_block_quote(&ToggleBlockQuote, window, cx));
    cx.assert_editor_state(indoc! {"
        «helloˇ» world
    "});

    // No-op in non-Markdown language (Rust)
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(rust_lang()), cx));
    cx.set_state(indoc! {"
        «helloˇ» world
    "});
    cx.update_editor(|e, window, cx| e.toggle_markdown_block_quote(&ToggleBlockQuote, window, cx));
    cx.assert_editor_state(indoc! {"
        «helloˇ» world
    "});

    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_lang()), cx));

    // Line is quoted with an empty selection
    cx.set_state(indoc! {"
        helˇlo world
    "});
    cx.update_editor(|e, window, cx| e.toggle_markdown_block_quote(&ToggleBlockQuote, window, cx));
    cx.assert_editor_state(indoc! {"
        «> hello worldˇ»
    "});

    // Line is unquoted with an empty selection
    cx.update_editor(|e, window, cx| e.toggle_markdown_block_quote(&ToggleBlockQuote, window, cx));
    cx.assert_editor_state(indoc! {"
        «hello worldˇ»
    "});

    // Multi-line selection is quoted, including blank lines
    cx.set_state(indoc! {"
        «first

        thirdˇ»
    "});
    cx.update_editor(|e, window, cx| e.toggle_markdown_block_quote(&ToggleBlockQuote, window, cx));
    cx.assert_editor_state(indoc! {"
        «> first
        >
        > thirdˇ»
    "});

    // Multi-line selection is unquoted, including blank lines
    cx.update_editor(|e, window, cx| e.toggle_markdown_block_quote(&ToggleBlockQuote, window, cx));
    cx.assert_editor_state(indoc! {"
        «first

        thirdˇ»
    "});

    // A multi-line selection, including a mixture of quoted and unquoted lines
    // and a mixture of empty and non-empty lines, normalizes each line to a
    // single quote.
    cx.set_state(indoc! {"
        «> first
        second
        >

        > third
        >fourthˇ»
    "});
    cx.update_editor(|e, window, cx| e.toggle_markdown_block_quote(&ToggleBlockQuote, window, cx));
    cx.assert_editor_state(indoc! {"
        «> first
        > second
        >
        >
        > third
        > fourthˇ»
    "});

    // A multi-line selection is unquoted.
    cx.update_editor(|e, window, cx| e.toggle_markdown_block_quote(&ToggleBlockQuote, window, cx));
    cx.assert_editor_state(indoc! {"
        «first
        second


        third
        fourthˇ»
    "});
}

#[track_caller]
fn assert_select_delimiters(around: bool, before: &str, after: &str, cx: &mut EditorTestContext) {
    let _state_context = cx.set_state(before);

    if around {
        cx.dispatch_action(SelectAroundDelimiters);
    } else {
        cx.dispatch_action(SelectInsideDelimiters);
    }

    cx.assert_editor_state(after);
}

#[gpui::test]
async fn test_select_delimiters(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorLanguageTestContext::new_typescript(cx).await;

    // Inside.
    assert_select_delimiters(false, "foo(ˇbar);", "foo(«barˇ»);", &mut cx);
    assert_select_delimiters(false, "foo(a, ˇb, c);", "foo(«a, b, cˇ»);", &mut cx);
    assert_select_delimiters(false, "foo([1, ˇ2, 3]);", "foo([«1, 2, 3ˇ»]);", &mut cx);
    assert_select_delimiters(false, "let x = { aˇ: 1 };", "let x = {« a: 1 ˇ»};", &mut cx);
    assert_select_delimiters(false, "let xˇ = 42;", "let xˇ = 42;", &mut cx);
    assert_select_delimiters(false, "foo(a, «bˇ», c);", "foo(«a, b, cˇ»);", &mut cx);
    assert_select_delimiters(
        false,
        "const s = \"hello ˇworld\";",
        "const s = \"«hello worldˇ»\";",
        &mut cx,
    );

    assert_select_delimiters(
        false,
        "const s = \"ˇhello world\";",
        "const s = \"«hello worldˇ»\";",
        &mut cx,
    );

    assert_select_delimiters(
        false,
        "const s = \"hello worldˇ\";",
        "const s = \"«hello worldˇ»\";",
        &mut cx,
    );

    assert_select_delimiters(
        false,
        "console.log(\"deˇbug\");",
        "console.log(\"«debugˇ»\");",
        &mut cx,
    );

    // Around.
    assert_select_delimiters(true, "foo(ˇbar);", "foo«(bar)ˇ»;", &mut cx);
    assert_select_delimiters(true, "foo([1, ˇ2, 3]);", "foo(«[1, 2, 3]ˇ»);", &mut cx);
    assert_select_delimiters(true, "let x = {ˇ a: 1 };", "let x = «{ a: 1 }ˇ»;", &mut cx);
    assert_select_delimiters(true, "let xˇ = 42;", "let xˇ = 42;", &mut cx);
    assert_select_delimiters(
        true,
        "console.log(\"deˇbug\");",
        "console.log(«\"debug\"ˇ»);",
        &mut cx,
    );
}

#[gpui::test]
async fn test_select_delimiters_in_markdown(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorTestContext::new(cx).await;
    cx.update_buffer(|buffer, cx| buffer.set_language(Some(markdown_lang()), cx));

    // Inside.
    assert_select_delimiters(
        false,
        r#"This is "ˇhello, world!"."#,
        r#"This is "«hello, world!ˇ»"."#,
        &mut cx,
    );
    assert_select_delimiters(
        false,
        r#"This is "hello, ˇworld!"."#,
        r#"This is "«hello, world!ˇ»"."#,
        &mut cx,
    );
    assert_select_delimiters(
        false,
        r#"This is "hello, world!ˇ"."#,
        r#"This is "«hello, world!ˇ»"."#,
        &mut cx,
    );
    assert_select_delimiters(
        false,
        r#"This is ˇ"hello, world!"."#,
        r#"This is "«hello, world!ˇ»"."#,
        &mut cx,
    );
    assert_select_delimiters(
        false,
        r#"This is "hello, world!"ˇ."#,
        r#"This is "«hello, world!ˇ»"."#,
        &mut cx,
    );
    assert_select_delimiters(
        false,
        r#"This is 'hello, ˇworld!'."#,
        r#"This is '«hello, world!ˇ»'."#,
        &mut cx,
    );
    assert_select_delimiters(
        false,
        r#"This is `hello, ˇworld!`."#,
        r#"This is `«hello, world!ˇ»`."#,
        &mut cx,
    );
    assert_select_delimiters(
        false,
        r#"This is ("hello, ˇworld!")."#,
        r#"This is ("«hello, world!ˇ»")."#,
        &mut cx,
    );
    assert_select_delimiters(
        false,
        r#"This is hello, ˇworld!."#,
        r#"This is hello, ˇworld!."#,
        &mut cx,
    );

    // Around.
    assert_select_delimiters(
        true,
        r#"This is "hello, ˇworld!"."#,
        r#"This is «"hello, world!"ˇ»."#,
        &mut cx,
    );
    assert_select_delimiters(
        true,
        r#"This is 'hello, ˇworld!'."#,
        r#"This is «'hello, world!'ˇ»."#,
        &mut cx,
    );
    assert_select_delimiters(
        true,
        r#"This is `hello, ˇworld!`."#,
        r#"This is «`hello, world!`ˇ»."#,
        &mut cx,
    );
}

#[gpui::test]
async fn test_select_delimiters_expansion(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    let mut cx = EditorLanguageTestContext::new_typescript(cx).await;

    let _state_context = cx.set_state("foo([1, ˇ2, 3]);");
    cx.dispatch_action(SelectInsideDelimiters);
    cx.assert_editor_state("foo([«1, 2, 3ˇ»]);");
    cx.dispatch_action(SelectInsideDelimiters);
    cx.assert_editor_state("foo(«[1, 2, 3]ˇ»);");

    let _state_context = cx.set_state("foo([1, ˇ2, 3]);");
    cx.dispatch_action(SelectInsideDelimiters);
    cx.assert_editor_state("foo([«1, 2, 3ˇ»]);");
    cx.dispatch_action(SelectAroundDelimiters);
    cx.assert_editor_state("foo(«[1, 2, 3]ˇ»);");
    cx.dispatch_action(SelectAroundDelimiters);
    cx.assert_editor_state("foo«([1, 2, 3])ˇ»;");

    let _state_context = cx.set_state("foo(x, { ˇa: 1 });");
    cx.dispatch_action(SelectInsideDelimiters);
    cx.assert_editor_state("foo(x, {« a: 1 ˇ»});");
    cx.dispatch_action(SelectAroundDelimiters);
    cx.assert_editor_state("foo(x, «{ a: 1 }ˇ»);");
    cx.dispatch_action(SelectInsideDelimiters);
    cx.assert_editor_state("foo(«x, { a: 1 }ˇ»);");
}

#[gpui::test]
async fn test_in_preview_key_context(cx: &mut TestAppContext) {
    init_test(cx, |_| {});
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings: &mut SettingsContent| {
                settings.preview_tabs.get_or_insert_default().enabled = Some(true);
            });
        });
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/root"),
        serde_json::json!({
            "test.txt": "hello",
        }),
    )
    .await;
    let project = Project::test(fs, [path!("/root").as_ref()], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
    let worktree_id = workspace.update(cx, |workspace, cx| {
        workspace.project().update(cx, |project, cx| {
            project.worktrees(cx).next().unwrap().read(cx).id()
        })
    });

    let editor = workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.open_path_preview(
                (worktree_id, rel_path("test.txt")),
                None,
                true,
                true,
                true,
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .downcast::<Editor>()
        .unwrap();

    let pane = workspace.update(cx, |workspace, _| workspace.active_pane().clone());
    pane.read_with(cx, |pane, _| {
        assert!(pane.is_active_preview_item(editor.entity_id()));
    });

    cx.focus(&editor);
    editor.update_in(cx, |editor, window, cx| {
        assert!(
            editor.key_context(window, cx).contains("in_preview"),
            "Expected 'in_preview' context when the editor is the pane's preview item"
        );
    });

    pane.update(cx, |pane, _cx| {
        pane.unpreview_item_if_preview(editor.entity_id());
    });

    editor.update_in(cx, |editor, window, cx| {
        assert!(
            !editor.key_context(window, cx).contains("in_preview"),
            "Expected no 'in_preview' context after the tab is unpreviewed"
        );
    });
}

fn refresh_editor_actions(cx: &mut VisualTestContext) {
    cx.executor().run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}
