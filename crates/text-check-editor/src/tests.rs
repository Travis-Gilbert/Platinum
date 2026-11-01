//! Headless GPUI tests: a real `EditorState` in a test window, the real
//! dictionary and `englishc`, and the real profile file.

use std::{ops::Range, path::PathBuf, sync::Arc};

use gpui::{EntityInputHandler as _, TestAppContext, WindowHandle};
use zorite_editor::{EditorEvent, EditorState, Redo, Undo};

use super::*;
use frame_text_check::{FieldKind, ProfileStore, check_note};

struct Harness {
    window: WindowHandle<EditorState>,
    editor: Entity<EditorState>,
    checking: Entity<TextChecking>,
    service: Arc<Service>,
    profile: PathBuf,
    _dir: tempfile::TempDir,
}

fn open(cx: &mut TestAppContext, text: &str) -> Harness {
    open_with(cx, text, FieldPolicy::default())
}

fn open_with(cx: &mut TestAppContext, text: &str, policy: FieldPolicy) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("profile.json");
    let service = Arc::new(Service::open(ProfileStore::at(&profile)).unwrap());
    let text = text.to_owned();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |window, cx| {
            cx.new(|cx| EditorState::new(window, cx).with_text(text.clone()))
        })
        .unwrap()
    });
    let editor = window.update(cx, |_, _, cx| cx.entity()).unwrap();
    let checking = cx.update(|cx| TextChecking::attach(&editor, service.clone(), policy, cx));
    Harness {
        window,
        editor,
        checking,
        service,
        profile,
        _dir: dir,
    }
}

/// Let the pause elapse and the background check finish.
fn settle(cx: &mut TestAppContext) {
    cx.executor()
        .advance_clock(DEBOUNCE + std::time::Duration::from_millis(1));
    cx.run_until_parked();
}

fn text(h: &Harness, cx: &TestAppContext) -> String {
    h.editor.read_with(cx, |e, _| e.text().to_owned())
}

fn underlined(h: &Harness, cx: &TestAppContext) -> Vec<Range<usize>> {
    h.editor.read_with(cx, |e, _| {
        e.diagnostics().iter().map(|d| d.range.clone()).collect()
    })
}

/// What a right-click at `offset` would list.
fn right_click(h: &Harness, cx: &mut TestAppContext, offset: usize) -> Vec<String> {
    h.window
        .update(cx, |e, _, _| e.flagged_rows(offset).1)
        .unwrap()
}

fn choose(h: &Harness, cx: &mut TestAppContext, range: Range<usize>, label: &str, rows: &[String]) {
    let index = rows
        .iter()
        .position(|row| row == label)
        .unwrap_or_else(|| panic!("no row {label:?} in {rows:?}"));
    h.editor
        .update(cx, |_, cx| cx.emit(EditorEvent::SpanAction { range, index }));
    cx.run_until_parked();
}

fn undo(h: &Harness, cx: &mut TestAppContext) {
    h.window
        .update(cx, |e, window, cx| e.undo(&Undo, window, cx))
        .unwrap();
}

#[gpui::test]
fn a_known_misspelling_is_underlined_after_the_pause_and_corrected_as_one_undo_step(
    cx: &mut TestAppContext,
) {
    let h = open(cx, "This has teh typo.");
    assert!(underlined(&h, cx).is_empty(), "nothing before the pause");
    settle(cx);
    assert_eq!(underlined(&h, cx), [9..12]);

    let rows = right_click(&h, cx, 10);
    assert!(rows.contains(&"the".to_owned()), "{rows:?}");
    assert!(rows.contains(&"Add to Dictionary".to_owned()), "{rows:?}");
    choose(&h, cx, 9..12, "the", &rows);
    assert_eq!(text(&h, cx), "This has the typo.");
    assert_eq!(h.checking.read_with(cx, |c, _| c.refusal().map(str::to_owned)), None);

    undo(&h, cx);
    assert_eq!(text(&h, cx), "This has teh typo.", "one undo reverses it");
    h.window
        .update(cx, |e, window, cx| e.redo(&Redo, window, cx))
        .unwrap();
    assert_eq!(text(&h, cx), "This has the typo.");
}

#[gpui::test]
fn a_supported_grammar_error_is_underlined_and_corrected(cx: &mut TestAppContext) {
    let h = open(cx, "He are late for the meeting.");
    settle(cx);
    let flagged = underlined(&h, cx);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    let before = text(&h, cx);
    let rows = right_click(&h, cx, flagged[0].start);
    let fix = rows
        .iter()
        .find(|row| !row.starts_with("Ignore: ") && *row != "Add to Dictionary")
        .unwrap_or_else(|| panic!("a replacement in {rows:?}"))
        .clone();
    choose(&h, cx, flagged[0].clone(), &fix, &rows);
    let after = text(&h, cx);
    assert_ne!(after, before);
    assert_eq!(after, "He is late for the meeting.");
    undo(&h, cx);
    assert_eq!(text(&h, cx), before);
}

#[gpui::test]
fn typing_after_the_menu_opened_refuses_the_correction(cx: &mut TestAppContext) {
    let h = open(cx, "This has teh typo.");
    settle(cx);
    let rows = right_click(&h, cx, 10);
    // The person types before choosing.
    h.editor.update(cx, |e, cx| e.replace_range(0..0, "Oh. ", cx));
    choose(&h, cx, 9..12, "the", &rows);
    assert_eq!(text(&h, cx), "Oh. This has teh typo.", "nothing was replaced");
    let refusal = h.checking.read_with(cx, |c, _| c.refusal().map(str::to_owned));
    assert!(refusal.is_some_and(|r| r.contains("revision")), "stale revision named");
}

#[gpui::test]
fn a_result_for_text_that_has_moved_on_offers_nothing(cx: &mut TestAppContext) {
    let h = open(cx, "This has teh typo.");
    settle(cx);
    h.editor.update(cx, |e, cx| e.replace_range(0..0, "Oh. ", cx));
    // The underline shifted with the text, but the result is for the old text.
    assert_eq!(underlined(&h, cx), [13..16]);
    assert!(right_click(&h, cx, 14).is_empty());
    settle(cx);
    assert!(!right_click(&h, cx, 14).is_empty(), "the new check supplies rows");
}

#[gpui::test]
fn an_active_composition_refuses_the_correction_and_the_text_is_not_touched(
    cx: &mut TestAppContext,
) {
    let h = open(cx, "This has teh typo.");
    settle(cx);
    let rows = right_click(&h, cx, 10);
    h.window
        .update(cx, |e, window, cx| {
            let end = e.text().len();
            e.set_cursor(end, cx);
            e.replace_and_mark_text_in_range(None, "\u{304b}", None, window, cx);
        })
        .unwrap();
    let during = text(&h, cx);
    choose(&h, cx, 9..12, "the", &rows);
    assert_eq!(text(&h, cx), during, "nothing changed while composing");
    let refusal = h.checking.read_with(cx, |c, _| c.refusal().map(str::to_owned));
    assert!(refusal.is_some_and(|r| r.contains("composition")));
    // And while composing, nothing is offered at all.
    assert!(right_click(&h, cx, 10).is_empty());
}

#[gpui::test]
fn the_word_being_typed_is_not_flagged_until_the_caret_leaves(cx: &mut TestAppContext) {
    let h = open(cx, "This has teh typo.");
    h.window
        .update(cx, |e, _, cx| e.set_cursor(11, cx))
        .unwrap();
    settle(cx);
    assert!(underlined(&h, cx).is_empty(), "caret inside the word");
    h.window.update(cx, |e, _, cx| e.set_cursor(0, cx)).unwrap();
    settle(cx);
    assert_eq!(underlined(&h, cx), [9..12]);
}

#[gpui::test]
fn a_custom_word_is_kept_across_a_restart(cx: &mut TestAppContext) {
    let h = open(cx, "The frobnicator works.");
    settle(cx);
    let flagged = underlined(&h, cx);
    assert_eq!(flagged.len(), 1);
    let rows = right_click(&h, cx, flagged[0].start);
    choose(&h, cx, flagged[0].clone(), "Add to Dictionary", &rows);
    settle(cx);
    assert!(underlined(&h, cx).is_empty());
    assert_eq!(text(&h, cx), "The frobnicator works.", "adding a word changes no text");

    // A new process opening the same profile file.
    let reopened = Service::open(ProfileStore::at(&h.profile)).unwrap();
    assert!(check_note(&reopened, "The frobnicator works.", None, None)
        .diagnostics
        .is_empty());
    assert!(!check_note(&Service::open(ProfileStore::at(h._dir.path().join("other.json"))).unwrap(), "The frobnicator works.", None, None)
        .diagnostics
        .is_empty(), "a different profile does not know it");
    assert!(h.service.profile().knows("frobnicator"));
}

#[gpui::test]
fn ignoring_a_rule_stops_it_being_reported(cx: &mut TestAppContext) {
    let h = open(cx, "This has teh typo.");
    settle(cx);
    let rows = right_click(&h, cx, 10);
    let ignore = rows
        .iter()
        .find(|row| row.starts_with("Ignore: "))
        .unwrap()
        .clone();
    choose(&h, cx, 9..12, &ignore, &rows);
    settle(cx);
    assert!(underlined(&h, cx).is_empty());
}

#[gpui::test]
fn emoji_zwj_and_combining_marks_do_not_shift_the_correction(cx: &mut TestAppContext) {
    let source = "\u{1f600} e\u{301} \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} teh end.";
    let h = open(cx, source);
    settle(cx);
    let at = source.find("teh").unwrap();
    assert_eq!(underlined(&h, cx), [at..at + 3]);
    let rows = right_click(&h, cx, at + 1);
    choose(&h, cx, at..at + 3, "the", &rows);
    assert_eq!(
        text(&h, cx),
        "\u{1f600} e\u{301} \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} the end."
    );
}

#[gpui::test]
fn code_links_and_front_matter_in_a_note_are_not_checked(cx: &mut TestAppContext) {
    let source = "---\ntitle: teh\n---\n\nprose teh here\n\n```\nlet teh = 1;\n```\n\n[teh](http://teh.example/teh)\n";
    let h = open(cx, source);
    settle(cx);
    let at = source.find("prose teh").unwrap() + "prose ".len();
    let link = source.find("[teh]").unwrap() + 1;
    let flagged = underlined(&h, cx);
    assert!(flagged.contains(&(at..at + 3)), "{flagged:?}");
    assert!(flagged.contains(&(link..link + 3)), "link text is prose: {flagged:?}");
    assert_eq!(flagged.len(), 2, "front matter, code and the url are not: {flagged:?}");
}

#[gpui::test]
fn a_protected_or_opted_out_editor_is_never_checked(cx: &mut TestAppContext) {
    let protected = FieldPolicy {
        kind: FieldKind::Protected,
        ..FieldPolicy::default()
    };
    let h = open_with(cx, "This has teh typo.", protected);
    settle(cx);
    assert!(underlined(&h, cx).is_empty());
    assert!(right_click(&h, cx, 10).is_empty());

    let opted_out = FieldPolicy {
        spellcheck: Some(false),
        ..FieldPolicy::default()
    };
    let h = open_with(cx, "This has teh typo.", opted_out);
    settle(cx);
    assert!(underlined(&h, cx).is_empty());

    // Switching it on later takes effect at the next check.
    h.checking
        .update(cx, |c, cx| c.set_policy(FieldPolicy::default(), cx));
    settle(cx);
    assert_eq!(underlined(&h, cx), [9..12]);
}
