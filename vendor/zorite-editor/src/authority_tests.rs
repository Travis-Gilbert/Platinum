//! Replay tests use the actual mutation methods and GPUI event delivery, not synthetic diffs.
use std::{cell::RefCell, rc::Rc, time::Instant};

use frame_notes::Offsets;
use gpui::{
    AppContext, Context, EntityInputHandler, TestAppContext, VisualTestContext, Window,
    WindowHandle,
};

use crate::*;

struct Harness {
    window: WindowHandle<EditorState>,
    events: Rc<RefCell<Vec<EditorEvent>>>,
    _subscription: gpui::Subscription,
}

impl Harness {
    fn new(cx: &mut TestAppContext, hosted: bool, text: &str) -> Self {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| {
                    let mut e = EditorState::new(window, cx);
                    e.set_hosted(hosted);
                    let mut e = e.with_text(text);
                    e.set_markdown_style(markdown_syntax::tests::test_style(), cx);
                    e.focus(window, cx);
                    e
                })
            })
            .unwrap()
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let out = events.clone();
        let subscription = cx.update(|cx| {
            cx.subscribe(
                &window.entity(cx).unwrap(),
                move |_, event: &EditorEvent, _| out.borrow_mut().push(event.clone()),
            )
        });
        Self {
            window,
            events,
            _subscription: subscription,
        }
    }

    fn update<R>(
        &self,
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut EditorState, &mut Window, &mut Context<EditorState>) -> R,
    ) -> R {
        self.window.update(cx, f).unwrap()
    }

    fn run(
        &self,
        cx: &mut TestAppContext,
        name: &str,
        count: usize,
        f: impl FnOnce(&mut EditorState, &mut Window, &mut Context<EditorState>),
    ) {
        VisualTestContext::from_window(self.window.into(), cx).update(|w, cx| {
            w.draw(cx).clear(cx);
        });
        self.events.borrow_mut().clear();
        let (mut replay, generation) =
            self.update(cx, |e, _, _| (e.text().to_owned(), e.generation()));
        self.update(cx, f);
        cx.run_until_parked();
        let events = self.events.borrow();
        let edits: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                EditorEvent::Edited(e) => Some(e),
                _ => None,
            })
            .collect();
        assert_eq!(edits.len(), count, "{name}: events = {events:?}");
        if count == 0 {
            assert!(
                !events.iter().any(|e| matches!(e, EditorEvent::Changed)),
                "{name}: feedback"
            );
        }
        for (index, e) in edits.iter().enumerate() {
            assert_eq!(
                e.generation,
                generation + index as u64 + 1,
                "{name}: generation"
            );
            assert!(
                replay.get(e.edit.range.clone()).is_some(),
                "{name}: old-text byte range"
            );
            // Exercise the same scalar boundary the session consumer uses.
            let scalar_range = Offsets::new(&replay).byte_range_to_scalar(e.edit.range.clone());
            assert_eq!(
                Offsets::new(&replay).scalar_range_to_byte(scalar_range),
                e.edit.range
            );
            replay.replace_range(e.edit.range.clone(), &e.edit.text);
            assert!(
                replay.get(e.selection.clone()).is_some(),
                "{name}: selection boundary"
            );
        }
        self.update(cx, |e, _, _| {
            if count > 0 {
                assert_eq!(replay, e.text(), "{name}: replay");
            }
            if let Some(last) = edits.last() {
                assert_eq!(last.selection, e.selected_range, "{name}: final selection");
                assert_eq!(
                    last.selection_reversed, e.selection_reversed,
                    "{name}: direction"
                );
            }
        });
    }

    fn load(&self, cx: &mut TestAppContext, text: &str, caret: usize) {
        self.run(cx, "set_text bypass", 1, |e, _, cx| e.set_text(text, cx));
        self.update(cx, |e, _, _| {
            e.selected_range = caret..caret;
        });
    }
}

#[gpui::test]
fn every_original_splice_and_bypass_replays(cx: &mut TestAppContext) {
    let h = Harness::new(cx, false, "hello");
    h.run(cx, "replace_range", 1, |e, _, cx| {
        e.replace_range(1..2, "😀", cx)
    });
    h.load(cx, "word", 4);
    h.run(cx, "typing", 1, |e, w, cx| {
        e.replace_text_in_range(None, "a", w, cx)
    });
    h.run(cx, "IME composition bypass", 1, |e, w, cx| {
        e.replace_and_mark_text_in_range(None, "かな", Some(2..2), w, cx)
    });
    h.run(cx, "IME update", 1, |e, w, cx| {
        e.replace_and_mark_text_in_range(None, "仮名", Some(2..2), w, cx)
    });
    h.run(cx, "IME commit", 1, |e, w, cx| {
        e.replace_text_in_range(None, "仮名", w, cx)
    });
    h.run(cx, "undo restore bypass", 1, |e, w, cx| {
        e.undo(&Undo, w, cx)
    });
    h.run(cx, "redo restore bypass", 1, |e, w, cx| {
        e.redo(&Redo, w, cx)
    });
    h.load(cx, "word", 4);
    h.update(cx, |e, _, _| {
        e.set_auto_replace(|_| Some((0..4, "[[word]]".into())))
    });
    h.run(cx, "auto replacement", 2, |e, w, cx| {
        e.replace_text_in_range(None, " ", w, cx)
    });
    h.update(cx, |e, _, _| e.auto_replace = None);
    h.load(cx, "abc", 0);
    h.update(cx, |e, _, _| e.selected_range = 0..3);
    h.run(cx, "wrap splice", 1, |e, w, cx| e.bold(&Bold, w, cx));
    h.run(cx, "unwrap splice", 1, |e, w, cx| e.bold(&Bold, w, cx));
    h.run(cx, "color splice", 1, |e, _, cx| {
        e.color_selection(ColorKind::Text, Some(0xff0000ff), cx)
    });
    h.run(cx, "strip color splice", 1, |e, _, cx| {
        e.color_selection(ColorKind::Text, None, cx)
    });
    h.load(cx, "2. item", 7);
    h.run(cx, "list indent", 1, |e, w, cx| e.indent(&Indent, w, cx));
    h.run(cx, "outdent", 1, |e, w, cx| e.outdent(&Outdent, w, cx));
    h.load(cx, "- item", 6);
    h.run(cx, "list continuation", 1, |e, w, cx| {
        e.newline(&Newline, w, cx)
    });
    h.load(cx, "- [ ] task", 10);
    h.run(cx, "checkbox mouse edit", 1, |e, w, cx| {
        e.checkbox_rects = vec![(
            0,
            Bounds::new(point(px(0.), px(0.)), size(px(10.), px(10.))),
        )];
        e.on_mouse_down(
            &MouseDownEvent {
                position: point(px(5.), px(5.)),
                ..Default::default()
            },
            w,
            cx,
        );
    });
    h.load(cx, "> [!NOTE]- title\n> body", 0);
    h.run(cx, "callout fold mouse edit", 1, |e, w, cx| {
        e.alert_fold_rects = vec![(
            0,
            Bounds::new(point(px(0.), px(0.)), size(px(10.), px(10.))),
        )];
        e.checkbox_rects.clear();
        e.on_mouse_down(
            &MouseDownEvent {
                position: point(px(5.), px(5.)),
                ..Default::default()
            },
            w,
            cx,
        );
    });
    h.load(cx, "![a](image.png)", 0);
    h.run(cx, "image resize", 1, |e, _, cx| {
        e.commit_image_resize(
            ImageResize {
                line: 0,
                start_width: 100.,
                start_x: px(0.),
                width: 200.,
            },
            cx,
        )
    });
    // Hosted load is needed here so the edit-time normalizer has work to do.
    h.update(cx, |e, _, _| e.set_hosted(true));
    h.load(cx, "words $$x$$ after", 17);
    h.run(cx, "math normalization", 1, |e, _, cx| {
        e.normalize_math_at(0, cx)
    });
    h.load(cx, "first\nsecond\nthird", 0);
    h.run(cx, "drag down", 1, |e, _, cx| {
        e.apply_line_drag(0, 0, 3, cx)
    });
    h.run(cx, "drag up", 1, |e, _, cx| e.apply_line_drag(2, 2, 0, cx));
    h.load(cx, "```\nbody\n```", 0);
    h.run(cx, "code language command", 1, |e, _, cx| {
        e.set_code_lang(0, "rust", cx)
    });

    let table = "| A | B |\n| --- | --- |\n| one | two |\n| three | four |";
    h.load(cx, table, 2);
    h.run(cx, "table align", 1, |e, _, cx| {
        e.set_caret_table_align(CellAlign::Center, cx)
    });
    h.load(cx, table, 2);
    h.run(cx, "insert table row", 1, |e, _, cx| {
        e.insert_table_row(true, cx)
    });
    h.run(cx, "delete table row", 1, |e, _, cx| e.delete_table_row(cx));
    h.load(cx, table, 2);
    h.run(cx, "duplicate table row", 1, |e, _, cx| {
        e.duplicate_table_row(cx)
    });
    h.load(cx, table, 2);
    h.run(cx, "rewrite table marker", 1, |e, _, cx| {
        e.set_table_style(Some("striped"), cx)
    });
    h.run(cx, "remove table marker", 1, |e, _, cx| {
        e.set_table_style(None, cx)
    });
    h.run(cx, "table column resize commit", 1, |e, _, cx| {
        e.commit_table_col_widths(
            TableColResize {
                header_row: 0,
                col: 0,
                start_x: px(0.),
                orig: 50.,
                width: 180.,
            },
            cx,
        );
    });
    h.load(cx, table, 2);
    h.run(cx, "insert table column", 1, |e, _, cx| {
        e.insert_table_column(true, cx)
    });
    h.run(cx, "delete table column", 1, |e, _, cx| {
        e.delete_table_column(cx)
    });
    h.run(cx, "delete whole table", 1, |e, _, cx| e.delete_table(cx));
}

const UNICODE: &[&str] = &["😀", "👨‍👩‍👧", "e\u{301}", "日本語", "مرحبا שלום"];

#[gpui::test]
fn unicode_local_and_remote_edits_use_old_text_byte_ranges(cx: &mut TestAppContext) {
    let h = Harness::new(cx, true, "");
    for &fixture in UNICODE {
        let text = format!("a{fixture}z");
        h.load(cx, &text, 1);
        h.run(cx, "Unicode typing over selection", 1, |e, w, cx| {
            let units = Offsets::new(e.text()).byte_range_to_utf16(1..1 + fixture.len());
            e.replace_text_in_range(Some(units), "👩‍💻e\u{301}中文ש", w, cx);
        });
        h.run(cx, "Unicode remote insert", 0, |e, _, cx| {
            let end = e.text().len() - 1;
            e.selected_range = 1..end;
            e.selection_reversed = true;
            e.marked_range = Some(1..end);
            e.diagnostics = vec![
                Diagnostic { range: 0..1 },
                Diagnostic {
                    range: end..end + 1,
                },
            ];
            // Check conversion on both sides of the remote update.
            assert_eq!(
                e.offset_to_utf16(e.text().len()),
                e.text().encode_utf16().count()
            );
            e.apply_remote(
                e.generation(),
                &[TextEdit {
                    range: 1..1,
                    text: fixture.into(),
                }],
                cx,
            )
            .unwrap();
            assert_eq!(e.selected_range, 1 + fixture.len()..end + fixture.len());
            assert_eq!(e.marked_range, Some(e.selected_range.clone()));
            assert!(e.selection_reversed);
            assert_eq!(
                e.diagnostics[1].range,
                end + fixture.len()..end + fixture.len() + 1
            );
            assert_eq!(
                e.offset_to_utf16(e.text().len()),
                e.text().encode_utf16().count()
            );
        });
        h.run(
            cx,
            "Unicode remote replacement and deletion",
            0,
            |e, _, cx| {
                let end = e.text().len() - 1;
                e.apply_remote(
                    e.generation(),
                    &[
                        TextEdit {
                            range: 1..end,
                            text: fixture.into(),
                        },
                        TextEdit {
                            range: 0..1,
                            text: String::new(),
                        },
                    ],
                    cx,
                )
                .unwrap();
                assert_eq!(e.text(), format!("{fixture}z"));
                assert_eq!(e.selected_range, fixture.len()..fixture.len());
                assert!(e.marked_range.is_none());
                assert_eq!(e.diagnostics.len(), 1);
                assert_eq!(e.diagnostics[0].range, fixture.len()..fixture.len() + 1);
            },
        );
    }
}

#[gpui::test]
fn remote_batch_validation_is_atomic_and_preserves_history(cx: &mut TestAppContext) {
    let h = Harness::new(cx, false, "a😀b");
    h.run(cx, "local snapshot", 1, |e, _, cx| {
        e.replace_range(6..6, "!", cx)
    });
    h.run(cx, "remote errors", 0, |e, _, cx| {
        let before = e.text().to_owned();
        let generation = e.generation();
        e.selected_range = 1..5;
        e.marked_range = Some(1..5);
        for (edits, expected) in [
            (
                vec![TextEdit {
                    range: 0..0,
                    text: "x".into(),
                }],
                RemoteEditError::StaleGeneration {
                    expected: generation - 1,
                    actual: generation,
                },
            ),
            (
                vec![
                    TextEdit {
                        range: 0..0,
                        text: "x".into(),
                    },
                    TextEdit {
                        range: 3..4,
                        text: "z".into(),
                    },
                ],
                RemoteEditError::InvalidRange { index: 1 },
            ),
            (
                vec![TextEdit {
                    range: 99..100,
                    text: "".into(),
                }],
                RemoteEditError::InvalidRange { index: 0 },
            ),
        ] {
            let base = if matches!(expected, RemoteEditError::StaleGeneration { .. }) {
                generation - 1
            } else {
                generation
            };
            assert_eq!(e.apply_remote(base, &edits, cx), Err(expected));
            assert_eq!(e.text(), before);
            assert_eq!(e.generation(), generation);
            assert_eq!(e.selected_range, 1..5);
            assert_eq!(e.marked_range, Some(1..5));
        }
        let (undo, redo) = (e.undo_stack.len(), e.redo_stack.len());
        e.apply_remote(
            generation,
            &[TextEdit {
                range: 0..0,
                text: "前".into(),
            }],
            cx,
        )
        .unwrap();
        assert_eq!((e.undo_stack.len(), e.redo_stack.len()), (undo, redo));
        assert_eq!(e.marked_range, Some(4..8));
    });
}

#[gpui::test]
fn hosted_load_is_byte_identical_and_history_requests_do_not_restore_snapshots(
    cx: &mut TestAppContext,
) {
    let original = "\u{feff}words $$x$$ after\r\n\r\n中文 👨‍👩‍👧 e\u{301} שלום\n";
    let h = Harness::new(cx, true, original);
    h.update(cx, |e, _, _| {
        assert_eq!(e.text().as_bytes(), original.as_bytes())
    });
    h.load(cx, original, 0);
    assert!(matches!(
        &h.events.borrow()[0],
        EditorEvent::Edited(BufferEdit {
            origin: EditOrigin::Load,
            ..
        })
    ));
    h.run(cx, "hosted insert", 1, |e, w, cx| {
        e.replace_text_in_range(None, "a", w, cx)
    });
    h.run(cx, "host undo request", 0, |e, w, cx| e.undo(&Undo, w, cx));
    assert!(matches!(
        h.events.borrow().as_slice(),
        [EditorEvent::HistoryRequested {
            action: HistoryAction::Undo,
            ..
        }]
    ));
    h.run(cx, "host undo answer", 0, |e, _, cx| {
        e.apply_remote(
            e.generation(),
            &[TextEdit {
                range: 0..1,
                text: String::new(),
            }],
            cx,
        )
        .unwrap()
    });
    h.run(cx, "host redo request", 0, |e, w, cx| e.redo(&Redo, w, cx));
    assert!(matches!(
        h.events.borrow().as_slice(),
        [EditorEvent::HistoryRequested {
            action: HistoryAction::Redo,
            ..
        }]
    ));
    h.update(cx, |e, _, _| {
        assert_eq!(e.text().as_bytes(), original.as_bytes());
        assert!(e.undo_stack.is_empty() && e.redo_stack.is_empty());
    });
}

#[gpui::test]
fn utf16_conversions_match_host_offsets_at_every_boundary(cx: &mut TestAppContext) {
    let h = Harness::new(cx, true, "");
    for fixture in ["abc"].into_iter().chain(UNICODE.iter().copied()) {
        h.update(cx, |e, _, cx| {
            e.set_text(fixture, cx);
            let offsets = Offsets::new(fixture);
            for byte in (0..fixture.len() + 3).chain([usize::MAX]) {
                assert_eq!(
                    e.offset_to_utf16(byte),
                    offsets.byte_to_utf16(byte),
                    "{fixture}: byte {byte}"
                );
            }
            for unit in (0..fixture.encode_utf16().count() + 3)
                .rev()
                .chain([usize::MAX])
            {
                assert_eq!(
                    e.offset_from_utf16(unit),
                    offsets.utf16_to_byte(unit),
                    "{fixture}: unit {unit}"
                );
                assert_eq!(utf16_to_byte(fixture, unit), offsets.utf16_to_byte(unit));
            }
        });
    }
}

fn report_cost(label: &str, bytes: usize, samples: &mut [u128]) {
    samples.sort_unstable();
    let mean = samples.iter().sum::<u128>() as f64 / samples.len() as f64 / 1000.;
    eprintln!(
        "{label} sections=900 bytes={bytes} warmup=100 samples=1000 mean_us={mean:.2} p50_us={:.2} p95_us={:.2} max_us={:.2}",
        samples[500] as f64 / 1000.,
        samples[950] as f64 / 1000.,
        samples[999] as f64 / 1000.
    );
}

/// Real input mutation and uncached conversion paths, excluding rendering, files and sessions.
#[gpui::test]
fn typing_cost_on_900_sections(cx: &mut TestAppContext) {
    use std::hint::black_box;

    let note: String = (0..900).map(|i| format!("## Section {i}\n\nA note about emoji 😀, combining e\u{301}, 中文 and שלום. {}\n\n- First item\n- Second item\n\n", "Body text for a large note. ".repeat(20))).collect();
    let bytes = note.len();
    let h = Harness::new(cx, true, "");
    let mut typing = Vec::new();
    let mut to_units = Vec::new();
    let mut to_bytes = Vec::new();
    let mut ime_queries = Vec::new();
    h.update(cx, |e, w, cx| {
        e.set_text(note.clone(), cx);
        e.selected_range = e.text().len()..e.text().len();
        for i in 0..1100 {
            let start = Instant::now();
            e.replace_text_in_range(None, "a", w, cx);
            let elapsed = start.elapsed();
            if i >= 100 {
                typing.push(elapsed.as_nanos());
            }
        }
        assert_eq!(e.text().len(), bytes + 1100);

        // Restore the exact 600,190-byte fixture. End-of-document conversions
        // scan the full String every time, with no anchor or retained index.
        e.set_text(note, cx);
        let units = e.text().encode_utf16().count();
        for i in 0..1100 {
            let start = Instant::now();
            black_box(e.offset_to_utf16(black_box(bytes)));
            let elapsed = start.elapsed();
            if i >= 100 {
                to_units.push(elapsed.as_nanos());
            }
        }
        for i in 0..1100 {
            let start = Instant::now();
            black_box(e.offset_from_utf16(black_box(units)));
            let elapsed = start.elapsed();
            if i >= 100 {
                to_bytes.push(elapsed.as_nanos());
            }
        }
        assert_eq!(e.offset_to_utf16(bytes), units);
        assert_eq!(e.offset_from_utf16(units), bytes);

        // Native IME's selection, marked-range and text queries: eight
        // conversions near the tail, plus returning the requested emoji text.
        let start = e.text().rfind('😀').unwrap();
        let marked = start..start + '😀'.len_utf8();
        let marked_utf16 = e.range_to_utf16(&marked);
        e.selected_range = marked.end..marked.end;
        e.marked_range = Some(marked.clone());
        for i in 0..1100 {
            let started = Instant::now();
            black_box(e.selected_text_range(false, w, cx));
            black_box(e.marked_text_range(w, cx));
            black_box(e.text_for_range(black_box(marked_utf16.clone()), &mut None, w, cx));
            let elapsed = started.elapsed();
            if i >= 100 {
                ime_queries.push(elapsed.as_nanos());
            }
        }
        assert_eq!(
            e.selected_text_range(false, w, cx).unwrap().range,
            marked_utf16.end..marked_utf16.end
        );
        assert_eq!(e.marked_text_range(w, cx), Some(marked_utf16.clone()));
        assert_eq!(
            e.text_for_range(marked_utf16, &mut None, w, cx).as_deref(),
            Some("😀")
        );
        // Do not render the unbounded-height 900-section root at update exit.
        e.set_text("", cx);
    });
    report_cost("TYPING_COST", bytes, &mut typing);
    report_cost("BYTE_TO_UTF16_COST", bytes, &mut to_units);
    report_cost("UTF16_TO_BYTE_COST", bytes, &mut to_bytes);
    report_cost("IME_QUERY_BATCH_COST", bytes, &mut ime_queries);
}

#[gpui::test]
fn clipboard_delete_and_command_paths_replay(cx: &mut TestAppContext) {
    let h = Harness::new(cx, true, "hello 😀 world");
    h.update(cx, |e, _, _| e.selected_range = 6..10);
    h.run(cx, "cut", 1, |e, w, cx| e.cut(&Cut, w, cx));
    h.run(cx, "paste", 1, |e, w, cx| e.paste(&Paste, w, cx));
    h.run(cx, "backspace grapheme", 1, |e, w, cx| {
        e.backspace(&Backspace, w, cx)
    });
    h.run(cx, "delete grapheme", 1, |e, w, cx| {
        e.delete(&Delete, w, cx)
    });
    h.load(cx, "one two three", 4);
    h.run(cx, "delete word right", 1, |e, w, cx| {
        e.delete_word_right(&DeleteWordRight, w, cx)
    });
    h.run(cx, "delete word left", 1, |e, w, cx| {
        e.delete_word_left(&DeleteWordLeft, w, cx)
    });
    h.load(cx, "- ", 2);
    h.run(cx, "exit empty list", 1, |e, w, cx| {
        e.newline(&Newline, w, cx)
    });
    h.load(cx, "plain", 0);
    h.run(cx, "plain indent", 1, |e, w, cx| e.indent(&Indent, w, cx));
    h.load(cx, "abc", 0);
    h.update(cx, |e, _, _| e.selected_range = 0..3);
    h.run(cx, "italic command", 1, |e, w, cx| e.italic(&Italic, w, cx));
    h.run(cx, "underline command", 1, |e, w, cx| {
        e.underline(&Underline, w, cx)
    });
    h.run(cx, "strike command", 1, |e, w, cx| e.strike(&Strike, w, cx));
    h.run(cx, "inline code command", 1, |e, w, cx| {
        e.code(&Code, w, cx)
    });
    h.run(cx, "highlight command", 1, |e, w, cx| {
        e.highlight(&Highlight, w, cx)
    });
    h.load(cx, "abc", 0);
    h.run(cx, "suggestion", 1, |e, w, cx| {
        e.apply_suggestion(0..3, "日本語", w, cx)
    });
}

/// A supported host consumer, using the real durable authority and the real key map.
/// All temporary files are inside this worktree and removed by TempDir.
#[gpui::test]
fn host_round_trips_real_vault_edits_undo_redo_and_unchanged_save(cx: &mut TestAppContext) {
    let temp = tempfile::Builder::new()
        .prefix(".authority-test-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    let root = temp.path().join("vault");
    std::fs::create_dir(&root).unwrap();
    let original = "words $$x$$ after\r\n\n👨‍👩‍👧 e\u{301} 中文 שלום\n";
    std::fs::write(root.join("note.md"), original).unwrap();
    let vault = Rc::new(RefCell::new(
        frame_notes::Vault::open(&root, temp.path().join("data")).unwrap(),
    ));
    let document = vault.borrow_mut().open_document("note.md").unwrap();
    let h = Harness::new(cx, true, &document.text);
    // An unchanged editor -> authority -> disk round trip preserves exact bytes.
    h.update(cx, |e, _, _| {
        assert_eq!(e.text().as_bytes(), original.as_bytes())
    });
    vault.borrow_mut().save("note.md").unwrap();
    assert_eq!(
        std::fs::read(root.join("note.md")).unwrap(),
        original.as_bytes()
    );

    let authority = vault.clone();
    let revision = Rc::new(RefCell::new(document.revision));
    let revision_out = revision.clone();
    let projection = Rc::new(RefCell::new(original.to_owned()));
    let projection_out = projection.clone();
    let subscription = cx.update(|cx| {
        cx.subscribe(
            &h.window.entity(cx).unwrap(),
            move |entity, event: &EditorEvent, cx| match event {
                EditorEvent::Edited(edit) if edit.origin == EditOrigin::Local => {
                    let mut text = projection_out.borrow_mut();
                    text.replace_range(edit.edit.range.clone(), &edit.edit.text);
                    let document = authority
                        .borrow_mut()
                        .edit(
                            "note.md",
                            *revision_out.borrow(),
                            &format!("editor-{}", edit.generation),
                            &text,
                        )
                        .unwrap();
                    assert_eq!(document.text, *text);
                    *revision_out.borrow_mut() = document.revision;
                }
                EditorEvent::HistoryRequested { action, generation } => {
                    let document = match action {
                        HistoryAction::Undo => authority.borrow_mut().undo("note.md"),
                        HistoryAction::Redo => authority.borrow_mut().redo("note.md"),
                    }
                    .unwrap();
                    let mut text = projection_out.borrow_mut();
                    let edits = [TextEdit {
                        range: 0..text.len(),
                        text: document.text.clone(),
                    }];
                    entity.update(cx, |e, cx| e.apply_remote(*generation, &edits, cx).unwrap());
                    *text = document.text;
                    *revision_out.borrow_mut() = document.revision;
                }
                _ => {}
            },
        )
    });
    cx.update(bind_keys);
    let mut visual = VisualTestContext::from_window(h.window.into(), cx);
    visual.update(|w, cx| {
        w.activate_window();
        w.draw(cx).clear(cx);
    });
    visual.simulate_keystrokes("ctrl-end");
    visual.simulate_input("日本語😀");
    h.update(cx, |e, _, _| {
        assert_eq!(e.text(), format!("{original}日本語😀"))
    });
    // simulate_input delivers four native input operations; the session owns
    // their grouping, so undo/redo must follow its four steps, not editor snapshots.
    for suffix in ["日本語", "日本", "日", ""] {
        visual.simulate_keystrokes("cmd-z");
        h.update(cx, |e, _, _| {
            assert_eq!(e.text(), format!("{original}{suffix}"))
        });
    }
    for suffix in ["日", "日本", "日本語", "日本語😀"] {
        visual.simulate_keystrokes("cmd-shift-z");
        h.update(cx, |e, _, _| {
            assert_eq!(e.text(), format!("{original}{suffix}"))
        });
    }
    assert_eq!(
        *projection.borrow(),
        vault.borrow_mut().open_document("note.md").unwrap().text
    );
    drop(subscription);
}
