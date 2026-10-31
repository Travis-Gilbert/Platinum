//! UI integration tests of the editor tab: real windows, real keystrokes and
//! a real vault on its own thread, each on a temporary copy of the notes
//! fixtures vault with the vault's data folder outside it.
//!
//! The vault's thread runs on its own, so a test waits for what it expects
//! (`until`), looking at the tab's text, its status and the files on disk,
//! never at how long anything took.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use futures::FutureExt as _;

use gpui_kit::component::{Root, Theme};
use gpui_kit::{AppContext as _, Context, Entity, TestAppContext, VisualTestContext, px, size};
use tempfile::TempDir;

use super::{NoteEditor, NoteEditorEvent, Status};
use crate::notes::host::service::Service;

/// A note the tests write themselves, in the copy of the fixtures.
const SCRATCH: &str = "scratch.md";

/// How long a test waits for the vault before it calls the wait a hang. A
/// vault opens in a few seconds in a debug build, and several times that when
/// the tests share a busy machine; a real hang stays a hang at any length.
const PATIENCE: Duration = Duration::from_secs(180);

/// A vault on its own thread: the fixtures, copied, and a data folder beside
/// them rather than inside.
struct Vault {
    root: TempDir,
    _data: TempDir,
    service: Service,
}

impl Vault {
    fn new(scratch: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/notes-fixtures/vault"),
            root.path(),
        );
        fs::write(root.path().join(SCRATCH), scratch).unwrap();
        let service = Service::start(root.path(), data.path());
        Self {
            root,
            _data: data,
            service,
        }
    }

    fn file(&self, note: &str) -> PathBuf {
        self.root.path().join(note)
    }

    fn bytes(&self, note: &str) -> Vec<u8> {
        fs::read(self.file(note)).unwrap()
    }

    fn text(&self, note: &str) -> String {
        fs::read_to_string(self.file(note)).unwrap()
    }

    /// A change made by something else than the tab, then the vault told to
    /// look (a test's sandbox delivers no file events).
    fn changed_outside(&self, note: &str, text: &str) {
        fs::write(self.file(note), text).unwrap();
        futures::executor::block_on(self.service.write(|vault| vault.refresh().map(drop))).unwrap();
    }

    /// The vault's own text of a note, as any other view of it would get it.
    fn head(&self, note: &str) -> String {
        let note = note.to_owned();
        futures::executor::block_on(
            self.service
                .read(move |vault| super::head_of(vault, &note).map(|head| head.text.to_string())),
        )
        .unwrap()
    }

    /// Stops the vault's thread until the returned sender is dropped, so that
    /// whatever is asked of the vault meanwhile waits in its queue, in order.
    fn hold(&self) -> mpsc::Sender<()> {
        let (release, held) = mpsc::channel();
        // The request is queued by its first poll; the answer is not wanted.
        drop(
            self.service
                .write(move |_| {
                    let _ = held.recv();
                    Ok(())
                })
                .now_or_never(),
        );
        release
    }

    /// Waits for the file of `note` to hold exactly `expected`.
    fn file_becomes(&self, note: &str, expected: &str) {
        let deadline = Instant::now() + PATIENCE;
        while self.text(note) != expected {
            assert!(
                Instant::now() < deadline,
                "{note} still holds {:?}, not {expected:?}",
                self.text(note)
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Another view of the note writes to the vault, as the reading panel
    /// does when a task is ticked.
    fn edited_elsewhere(&self, note: &str, text: &str) {
        let (note, text) = (note.to_owned(), text.to_owned());
        futures::executor::block_on(self.service.write(move |vault| {
            let head = vault.open_document(&note)?;
            vault.edit(&note, head.revision, "another-view", &text)?;
            Ok(())
        }))
        .unwrap();
    }
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

/// An open tab in a headless window.
struct Tab {
    editor: Entity<NoteEditor>,
    cx: VisualTestContext,
}

impl Tab {
    /// Opens `note` and waits for the vault to answer.
    fn open(cx: &mut TestAppContext, vault: &Vault, note: &str) -> Self {
        // The vault answers from its own thread, which the deterministic
        // scheduler reports as non-determinism unless told to expect it.
        cx.executor().allow_parking();
        cx.update(|cx| {
            if !cx.has_global::<Theme>() {
                gpui_kit::init(cx);
            }
        });
        let (service, note) = (vault.service.clone(), note.to_owned());
        let mut editor = None;
        let window = cx.open_window(size(px(900.), px(700.)), |window, cx| {
            let view = cx.new(|cx| NoteEditor::new(service, note, window, cx));
            editor = Some(view.clone());
            Root::new(view, window, cx)
        });
        let mut tab = Self {
            editor: editor.expect("the tab"),
            cx: VisualTestContext::from_window(window.into(), cx),
        };
        tab.cx.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear(cx);
        });
        tab.until("the note to open", |tab| tab.status() != Status::Opening);
        tab
    }

    fn with<R>(&mut self, f: impl FnOnce(&mut NoteEditor, &mut Context<NoteEditor>) -> R) -> R {
        self.editor.update(&mut self.cx, f)
    }

    fn status(&mut self) -> Status {
        self.editor.read_with(&self.cx, |tab, cx| tab.status(cx))
    }

    fn text(&mut self) -> String {
        self.editor
            .read_with(&self.cx, |tab, cx| tab.text(cx).to_owned())
    }

    fn revision(&mut self) -> u64 {
        self.editor
            .read_with(&self.cx, |tab, _| tab.revision().expect("an open note"))
    }

    fn until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let deadline = Instant::now() + PATIENCE;
        loop {
            self.cx.run_until_parked();
            if done(self) {
                return;
            }
            if Instant::now() >= deadline {
                let (busy, load_wanted, requests) = self.editor.read_with(&self.cx, |tab, _| {
                    (tab.busy, tab.load_wanted, tab.service.submitted())
                });
                panic!(
                    "timed out waiting for {what}; the tab says {:?} and shows {:?} (busy {busy}, load wanted {load_wanted}, {requests} requests made)",
                    self.status(),
                    self.text(),
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Everything typed has reached the vault and no request is out.
    fn settled(&mut self) {
        self.until("the tab to settle", |tab| {
            tab.editor.read_with(&tab.cx, |tab, cx| {
                !tab.busy
                    && !tab.save_wanted
                    && tab.history.is_empty()
                    && !(tab.error.is_none() && tab.ahead(cx))
            })
        });
    }

    fn keys(&mut self, keystrokes: &str) {
        self.cx.simulate_keystrokes(keystrokes);
    }

    fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
    }

    fn type_at_end(&mut self, text: &str) {
        self.keys("ctrl-end");
        self.type_text(text);
    }

    fn set_cursor(&mut self, offset: usize) {
        self.with(|tab, cx| {
            tab.editor
                .update(cx, |editor, cx| editor.set_cursor(offset, cx));
        });
    }

    fn cursor(&mut self) -> usize {
        self.editor
            .read_with(&self.cx, |tab, cx| tab.editor.read(cx).cursor())
    }

    /// Closes the window and lets the tab go.
    fn close(self) {
        let Self { editor, mut cx } = self;
        // Release runs when effects are next flushed, which an update does.
        // With the test's own handle gone, only the window holds the tab, so
        // removing the window is what releases it.
        drop(editor);
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn a_note_opens_with_its_exact_bytes_and_an_unchanged_save_leaves_the_file(
    cx: &mut TestAppContext,
) {
    let vault = Vault::new("");
    for note in [
        "syntax/Line Endings.md",
        "Unicode é 日本.md",
        "syntax/Byte Order Mark.md",
        "Home.md",
    ] {
        let before = vault.bytes(note);
        let mut tab = Tab::open(cx, &vault, note);
        assert_eq!(tab.text().as_bytes(), before, "{note} on screen");
        assert_eq!(tab.status(), Status::Saved, "{note}");
        tab.keys("cmd-s");
        tab.settled();
        assert_eq!(vault.bytes(note), before, "{note} after an unchanged save");
        assert_eq!(tab.text().as_bytes(), before, "{note} on screen after");
        tab.close();
    }
}

#[gpui_kit::test]
fn typing_then_saving_writes_the_typed_bytes(cx: &mut TestAppContext) {
    let vault = Vault::new("first\nsecond\n");
    let mut tab = Tab::open(cx, &vault, SCRATCH);
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    let editor = tab.editor.clone();
    tab.cx.update(|_, cx| {
        cx.subscribe(&editor, move |_, event: &NoteEditorEvent, _| {
            seen.borrow_mut().push(event.clone());
        })
        .detach();
    });
    let opened = tab.revision();

    tab.type_at_end("日本語😀 typed");
    tab.settled();
    assert_eq!(tab.text(), "first\nsecond\n日本語😀 typed");
    assert_eq!(tab.status(), Status::Unsaved);
    assert!(tab.revision() > opened, "the vault was given the text");
    assert_eq!(vault.head(SCRATCH), tab.text(), "the vault has it");
    assert_eq!(vault.text(SCRATCH), "first\nsecond\n", "not on disk yet");

    tab.keys("cmd-s");
    tab.until("the file to be written", |_| {
        vault.text(SCRATCH) == "first\nsecond\n日本語😀 typed"
    });
    tab.settled();
    assert_eq!(tab.status(), Status::Saved);
    let last = events.borrow().last().cloned();
    assert_eq!(
        last,
        Some(NoteEditorEvent::Synced {
            revision: tab.revision(),
            saved: true
        })
    );
}

#[gpui_kit::test]
fn a_reopened_note_shows_the_saved_text(cx: &mut TestAppContext) {
    let vault = Vault::new("first\n");
    let mut tab = Tab::open(cx, &vault, SCRATCH);
    tab.type_at_end("more");
    tab.keys("cmd-s");
    tab.until("the file to be written", |_| {
        vault.text(SCRATCH) == "first\nmore"
    });
    tab.close();

    let mut tab = Tab::open(cx, &vault, SCRATCH);
    assert_eq!(tab.text(), "first\nmore");
    assert_eq!(tab.status(), Status::Saved);
    tab.close();
}

#[gpui_kit::test]
fn a_tab_closed_with_unsaved_text_saves_it(cx: &mut TestAppContext) {
    let vault = Vault::new("first\n");
    let mut tab = Tab::open(cx, &vault, SCRATCH);
    tab.type_at_end("typed just before closing");
    tab.close();
    vault.file_becomes(SCRATCH, "first\ntyped just before closing");
}

/// The vault holds a push of the text as it was after the first keystroke
/// when the tab closes with the rest of it unsent. That push is the tab's own:
/// the closing save puts the whole text over it, and does not mistake it for
/// a change from elsewhere (which would write conflict markers into the file).
#[gpui_kit::test]
fn a_tab_closed_while_a_push_is_still_out_saves_everything_typed(cx: &mut TestAppContext) {
    let vault = Vault::new("first\n");
    let mut tab = Tab::open(cx, &vault, SCRATCH);
    tab.settled();
    let release = vault.hold();
    tab.type_at_end("typed just before closing");
    // The first keystroke's push is queued behind the hold, in flight.
    tab.cx.run_until_parked();
    tab.close();
    drop(release);
    vault.file_becomes(SCRATCH, "first\ntyped just before closing");
}

#[gpui_kit::test]
fn an_outside_write_reaches_the_buffer_and_an_undo_does_not_revert_it(cx: &mut TestAppContext) {
    let vault = Vault::new("alpha\nbeta\ngamma\n");
    let mut tab = Tab::open(cx, &vault, SCRATCH);
    tab.set_cursor(5);

    let outside = "alpha\nbeta\ngamma\ndelta from disk\n";
    vault.changed_outside(SCRATCH, outside);
    tab.until("the outside change on screen", |tab| tab.text() == outside);
    assert_eq!(tab.cursor(), 5, "the caret stays where it was");
    assert_eq!(tab.status(), Status::Saved);

    tab.type_text("X");
    tab.settled();
    assert_eq!(tab.text(), "alphaX\nbeta\ngamma\ndelta from disk\n");
    tab.keys("cmd-z");
    tab.until("the typed letter to be undone", |tab| tab.text() == outside);
    // Nothing is left to undo but the change that came from disk.
    tab.keys("cmd-z");
    tab.settled();
    assert_eq!(
        tab.text(),
        outside,
        "an undo never reverts the outside change"
    );
    assert_eq!(vault.head(SCRATCH), outside);
    // Redo brings back the typed letter and nothing else.
    tab.keys("cmd-shift-z");
    tab.until("the letter to come back", |tab| {
        tab.text() == "alphaX\nbeta\ngamma\ndelta from disk\n"
    });
}

#[gpui_kit::test]
fn an_outside_change_beside_unsaved_text_merges_and_both_are_saved(cx: &mut TestAppContext) {
    let vault = Vault::new("one\ntwo\nthree\nfour\nfive\n");
    let mut tab = Tab::open(cx, &vault, SCRATCH);
    tab.type_at_end("mine");
    tab.settled();

    vault.changed_outside(SCRATCH, "ONE\ntwo\nthree\nfour\nfive\n");
    let merged = "ONE\ntwo\nthree\nfour\nfive\nmine";
    tab.until("both changes on screen", |tab| tab.text() == merged);
    assert_eq!(tab.status(), Status::Unsaved);

    tab.keys("cmd-s");
    tab.until("the file to be written", |_| vault.text(SCRATCH) == merged);
    tab.settled();
    assert_eq!(tab.status(), Status::Saved);
}

#[gpui_kit::test]
fn an_overlapping_outside_change_is_reported_and_never_overwritten(cx: &mut TestAppContext) {
    let vault = Vault::new("one\ntwo\nthree\n");
    let mut tab = Tab::open(cx, &vault, SCRATCH);
    tab.set_cursor(7);
    tab.type_text("-mine");
    tab.settled();
    assert_eq!(tab.text(), "one\ntwo-mine\nthree\n");

    let outside = "one\ntwo-theirs\nthree\n";
    vault.changed_outside(SCRATCH, outside);
    tab.keys("cmd-s");
    tab.until("the conflict to be reported", |tab| {
        tab.status() == Status::Conflict
    });
    tab.settled();
    assert_eq!(
        vault.text(SCRATCH),
        outside,
        "the outside change is on disk still"
    );
    assert_eq!(
        tab.text(),
        "one\ntwo-mine\nthree\n",
        "the typed text is still on screen"
    );
    assert!(Status::Conflict.label().contains("Conflict"));
}

#[gpui_kit::test]
fn autosave_writes_two_seconds_after_the_last_edit(cx: &mut TestAppContext) {
    let vault = Vault::new("first\n");
    let mut tab = Tab::open(cx, &vault, SCRATCH);
    tab.type_at_end("a");
    tab.settled();
    tab.cx.executor().advance_clock(Duration::from_millis(1900));
    tab.settled();
    assert_eq!(vault.text(SCRATCH), "first\n", "not yet");

    // Another keystroke starts the wait over.
    tab.type_text("b");
    tab.settled();
    tab.cx.executor().advance_clock(Duration::from_millis(1900));
    tab.settled();
    assert_eq!(vault.text(SCRATCH), "first\n", "the wait started over");

    tab.cx.executor().advance_clock(Duration::from_millis(200));
    tab.until("the autosave", |_| vault.text(SCRATCH) == "first\nab");
    tab.settled();
    assert_eq!(tab.status(), Status::Saved);
}

/// Another writer moves the vault's head while the tab holds text it has not
/// sent. The tab's text is merged onto the new head, not put over it.
#[gpui_kit::test]
fn a_head_that_moved_under_unsent_text_is_merged_not_overwritten(cx: &mut TestAppContext) {
    let vault = Vault::new("one\ntwo\nthree\nfour\nfive\nsix\n");
    let mut tab = Tab::open(cx, &vault, SCRATCH);
    // Hold the tab's requests back while the other writer goes first. No
    // flight may be out when they are held, or it would land mid-typing.
    tab.settled();
    tab.with(|tab, _| tab.busy = true);
    tab.type_at_end("mine");
    vault.edited_elsewhere(SCRATCH, "ONE\ntwo\nthree\nfour\nfive\nsix\n");
    tab.with(|tab, cx| {
        tab.busy = false;
        tab.pump(cx);
    });

    let merged = "ONE\ntwo\nthree\nfour\nfive\nsix\nmine";
    tab.until("the merge to reach the vault", |tab| {
        tab.text() == merged && vault.head(SCRATCH) == merged
    });
    tab.settled();
    assert_eq!(tab.status(), Status::Unsaved, "merged, and no error");
}

#[gpui_kit::test]
fn overlapping_changes_to_a_moved_head_keep_both_texts(cx: &mut TestAppContext) {
    let vault = Vault::new("one\ntwo\nthree\nfour\n");
    let mut tab = Tab::open(cx, &vault, SCRATCH);
    tab.settled();
    tab.with(|tab, _| tab.busy = true);
    tab.set_cursor(13);
    tab.type_text("-mine");
    vault.edited_elsewhere(SCRATCH, "one\ntwo\nTHREE\nfour\n");
    tab.with(|tab, cx| {
        tab.busy = false;
        tab.pump(cx);
    });
    tab.until("the overlap to be marked", |tab| {
        tab.status() == Status::Conflict
    });
    tab.settled();
    let shown = tab.text();
    assert!(
        shown.contains("three-mine") && shown.contains("THREE"),
        "{shown}"
    );
    assert_eq!(vault.head(SCRATCH), shown, "the vault holds what is shown");
}
