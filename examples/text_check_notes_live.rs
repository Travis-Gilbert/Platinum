//! Live check for local spelling and grammar corrections in a native note: a
//! real `zorite-editor` in a real GPUI window on this platform, real timers and
//! executors, the real dictionary and `englishc`, and a temporary profile.
//!
//! ```text
//! script/media-env cargo build --locked --example text_check_notes_live
//! text_check_notes_live [--shots dir]
//! ```
//!
//! Text goes in through the editor's own input handler, the check runs after
//! the real pause, and a correction is the same `SpanAction` event the editor's
//! menu emits. It exits 1 if any step is wrong. With `--shots dir` it runs no
//! checks: it writes `notes-underline.png` (the flagged word, underlined) and
//! `notes-menu.png` (the editor's own menu, opened by a right-click on the
//! word) from GPUI's own rendering of the window, then exits.
extern crate gpui_kit as gpui;

mod shot;

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use frame_text_check::{CheckRequest, CheckStatus, FieldPolicy, ProfileStore, Service};
use frame_text_check_editor::TextChecking;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, AsyncApp, Bounds, Context, Entity,
    EntityInputHandler as _, IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    ParentElement as _, PlatformInput, Render, Styled as _, Window, WindowBounds, WindowHandle,
    WindowOptions, div, point, px, size,
};
use zorite_editor::{EditorEvent, EditorState, Undo};

#[derive(Parser)]
#[command(about = "Check spelling and grammar corrections in a native note")]
struct Args {
    /// Instead of the checks, write `notes-underline.png` and `notes-menu.png`
    /// to this directory.
    #[arg(long)]
    shots: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let scratch =
        std::env::temp_dir().join(format!("frame-text-check-notes-{}", std::process::id()));
    std::fs::create_dir_all(&scratch)?;
    let profile = scratch.join("text-check-profile.json");
    // The personal dictionary lives in a temporary file, never the real one.
    let service = Arc::new(Service::open(ProfileStore::at(profile.clone()))?);
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Light, None, cx);
            // A capture needs the room for the whole menu; the checks do not.
            let height = if args.shots.is_some() { 420. } else { 200. };
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(60.), px(60.)),
                    size(px(640.), px(height)),
                ))),
                ..Default::default()
            };
            if let Some(dir) = args.shots {
                // The capture opens the editor on a ground of its own (`Ground`).
                let window = cx
                    .open_window(options, |window, cx| {
                        let editor = cx.new(|cx| EditorState::new(window, cx).with_text(""));
                        cx.new(|_| Ground { editor })
                    })
                    .expect("a window");
                let editor = window.read(cx).expect("the window's ground").editor.clone();
                let checking =
                    TextChecking::attach(&editor, service.clone(), FieldPolicy::default(), cx);
                cx.spawn(async move |cx| {
                    let passed = shoot(window.into(), &editor, &dir, cx).await;
                    drop(checking);
                    let _ = std::fs::remove_dir_all(&scratch);
                    std::process::exit(if passed { 0 } else { 1 });
                })
                .detach();
                return;
            }
            let window = cx
                .open_window(options, |window, cx| {
                    cx.new(|cx| EditorState::new(window, cx).with_text(""))
                })
                .expect("a window");
            let editor = window.entity(cx).expect("the window's editor");
            let checking =
                TextChecking::attach(&editor, service.clone(), FieldPolicy::default(), cx);
            let live = Live {
                window,
                editor,
                checking,
                service,
                profile,
            };
            cx.spawn(async move |cx| {
                let passed = run(&live, cx).await;
                // AppKit's quit exits with 0; an acceptance check needs its own status.
                let _ = std::fs::remove_dir_all(&scratch);
                std::process::exit(if passed { 0 } else { 1 });
            })
            .detach();
        });
    Ok(())
}

struct Live {
    window: WindowHandle<EditorState>,
    editor: Entity<EditorState>,
    checking: Entity<TextChecking>,
    service: Arc<Service>,
    profile: PathBuf,
}

#[derive(Default)]
struct Report {
    checks: usize,
    failures: Vec<String>,
}

impl Report {
    fn expect(&mut self, ok: bool, what: &str, detail: impl FnOnce() -> String) {
        self.checks += 1;
        if ok {
            println!("  ok    {what}");
        } else {
            let detail = detail();
            println!("  FAIL  {what}: {detail}");
            self.failures.push(format!("{what}: {detail}"));
        }
    }
}

async fn run(live: &Live, cx: &mut AsyncApp) -> bool {
    let started = Instant::now();
    let mut report = Report::default();

    println!("dictionary spelling, corrected and undone");
    corrects_and_undoes(
        live,
        cx,
        &mut report,
        "Please recieve this letter today.",
        "recieve",
        "receive",
    )
    .await;

    println!("grammar, corrected and undone");
    corrects_and_undoes(
        live,
        cx,
        &mut report,
        "He are late for the meeting.",
        "are",
        "is",
    )
    .await;

    println!("common misspelling after an emoji and a combining mark: offsets");
    corrects_and_undoes(
        live,
        cx,
        &mut report,
        "\u{1f600} e\u{301} teh end.",
        "teh",
        "the",
    )
    .await;

    println!("a stale menu cannot change text typed after it was shown");
    stale_menu(live, cx, &mut report).await;

    println!("an active composition blocks corrections");
    composition(live, cx, &mut report).await;

    println!("a custom word is kept, across a restart");
    custom_word(live, cx, &mut report).await;

    for failure in &report.failures {
        println!("FAIL: {failure}");
    }
    println!(
        "{} ({} checks) after {:?}",
        if report.failures.is_empty() {
            "PASS"
        } else {
            "FAIL"
        },
        report.checks,
        started.elapsed()
    );
    report.failures.is_empty()
}

/// Put `text` in the note and wait for the real pause and the check, which
/// leaves one underline. Underlines left from the text before stay until the
/// new check lands, and the editor offers nothing for them (a result is only
/// used for the text it was computed from), so wait for a menu, not just a
/// mark. Gives the underline's byte range.
async fn show(live: &Live, cx: &mut AsyncApp, text: &str) -> Option<Range<usize>> {
    live.editor
        .update(cx, |editor, cx| editor.set_text(text, cx));
    let editor = live.editor.clone();
    let shown = until(cx, "the underline", Duration::from_secs(20), move |cx| {
        let editor = editor.read(cx);
        editor
            .diagnostics()
            .first()
            .is_some_and(|d| !editor.flagged_rows(d.range.start + 1).1.is_empty())
    })
    .await;
    shown.then(|| {
        live.editor
            .read_with(cx, |editor, _| editor.diagnostics()[0].range.clone())
    })
}

fn text(live: &Live, cx: &mut AsyncApp) -> String {
    live.editor
        .read_with(cx, |editor, _| editor.text().to_owned())
}

/// What a right-click on `offset` would list.
fn rows_at(live: &Live, cx: &mut AsyncApp, offset: usize) -> Vec<String> {
    live.window
        .update(cx, |editor, _, _| editor.flagged_rows(offset).1)
        .unwrap_or_default()
}

/// The person picks row `index` of the menu for `range`.
fn pick(live: &Live, cx: &mut AsyncApp, range: Range<usize>, index: usize) {
    live.editor.update(cx, |_, cx| {
        cx.emit(EditorEvent::SpanAction { range, index })
    });
}

fn refusal(live: &Live, cx: &mut AsyncApp) -> Option<String> {
    live.checking
        .read_with(cx, |checking, _| checking.refusal().map(str::to_owned))
}

async fn corrects_and_undoes(
    live: &Live,
    cx: &mut AsyncApp,
    report: &mut Report,
    before: &str,
    word: &str,
    replacement: &str,
) {
    let Some(range) = show(live, cx, before).await else {
        report.expect(false, &format!("{before:?} is underlined"), || {
            "no underline arrived".into()
        });
        return;
    };
    let flagged = &before[range.clone()];
    report.expect(
        flagged == word,
        &format!("{word:?} is the underlined word"),
        || format!("underlined {flagged:?}"),
    );
    let rows = rows_at(live, cx, range.start + 1);
    let row = rows.iter().position(|label| label == replacement);
    report.expect(
        row.is_some(),
        &format!("{replacement:?} is offered on demand"),
        || format!("rows: {rows:?}"),
    );
    let Some(row) = row else { return };
    let after = format!(
        "{}{replacement}{}",
        &before[..range.start],
        &before[range.end..]
    );
    pick(live, cx, range, row);
    sleep(cx, Duration::from_millis(100)).await;
    let now = text(live, cx);
    report.expect(now == after, &format!("the note becomes {after:?}"), || {
        format!("it is {now:?}; refusal {:?}", refusal(live, cx))
    });
    let _ = live
        .window
        .update(cx, |editor, window, cx| editor.undo(&Undo, window, cx));
    let restored = text(live, cx);
    report.expect(
        restored == before,
        &format!("one undo puts {before:?} back"),
        || format!("it is {restored:?}"),
    );
}

async fn stale_menu(live: &Live, cx: &mut AsyncApp, report: &mut Report) {
    let before = "This has teh typo.";
    let Some(range) = show(live, cx, before).await else {
        report.expect(false, "the underline arrives", || "none arrived".into());
        return;
    };
    let rows = rows_at(live, cx, range.start + 1);
    let row = rows.iter().position(|label| label == "the");
    report.expect(row.is_some(), "the menu is shown", || {
        format!("rows: {rows:?}")
    });
    let Some(row) = row else { return };
    // The person types before choosing; the text the menu was built for is gone.
    let _ = live.window.update(cx, |editor, window, cx| {
        editor.replace_text_in_range(Some(0..0), "Hey ", window, cx);
    });
    let typed = text(live, cx);
    pick(live, cx, range, row);
    sleep(cx, Duration::from_millis(100)).await;
    let now = text(live, cx);
    report.expect(now == typed, "the typed text is untouched", || {
        format!("it is {now:?}, was {typed:?}")
    });
    let why = refusal(live, cx);
    report.expect(
        why.is_some(),
        "the person is told why nothing changed",
        || "no refusal".into(),
    );
    println!("        note: {why:?}");
}

async fn composition(live: &Live, cx: &mut AsyncApp, report: &mut Report) {
    let before = "This has teh typo.";
    let Some(range) = show(live, cx, before).await else {
        report.expect(false, "the underline arrives", || "none arrived".into());
        return;
    };
    let rows = rows_at(live, cx, range.start + 1);
    let row = rows.iter().position(|label| label == "the");
    let Some(row) = row else {
        report.expect(false, "the menu is shown", || format!("rows: {rows:?}"));
        return;
    };
    let _ = live.window.update(cx, |editor, window, cx| {
        let end = editor.text().len();
        editor.set_cursor(end, cx);
        editor.replace_and_mark_text_in_range(None, "\u{304b}", None, window, cx);
    });
    let composing = live
        .editor
        .read_with(cx, |editor, _| editor.marked_range().is_some());
    report.expect(composing, "the note is composing", || "it is not".into());
    let during = text(live, cx);
    pick(live, cx, range.clone(), row);
    sleep(cx, Duration::from_millis(100)).await;
    let now = text(live, cx);
    report.expect(
        now == during,
        "choosing during a composition changes nothing",
        || format!("it is {now:?}, was {during:?}"),
    );
    let why = refusal(live, cx);
    report.expect(
        why.as_deref()
            .is_some_and(|why| why.contains("composition")),
        "the refusal names the composition",
        || format!("refusal {why:?}"),
    );
    let offered = rows_at(live, cx, range.start + 1);
    report.expect(
        offered.is_empty(),
        "a menu opened during a composition offers nothing",
        || format!("rows: {offered:?}"),
    );
    let _ = live
        .window
        .update(cx, |editor, window, cx| editor.unmark_text(window, cx));
}

async fn custom_word(live: &Live, cx: &mut AsyncApp, report: &mut Report) {
    let note = "Ask Zorvath about the plan.";
    let Some(range) = show(live, cx, note).await else {
        report.expect(false, "Zorvath is underlined", || "none arrived".into());
        return;
    };
    let rows = rows_at(live, cx, range.start + 1);
    let add = rows.iter().position(|label| label == "Add to Dictionary");
    report.expect(
        add.is_some(),
        "Zorvath is offered for the dictionary",
        || format!("rows: {rows:?}"),
    );
    let Some(add) = add else { return };
    pick(live, cx, range, add);
    let editor = live.editor.clone();
    let cleared = until(
        cx,
        "the underline to clear",
        Duration::from_secs(20),
        move |cx| editor.read(cx).diagnostics().is_empty(),
    )
    .await;
    report.expect(cleared, "the word is no longer underlined", || {
        "the underline stayed".into()
    });
    report.expect(
        text(live, cx) == note,
        "adding a word changes no text",
        || "the text changed".into(),
    );
    report.expect(
        live.service.profile().knows("Zorvath"),
        "the service knows the word",
        || "it does not".into(),
    );
    let file = std::fs::read_to_string(&live.profile).unwrap_or_default();
    report.expect(
        file.contains("Zorvath"),
        "the profile file holds the word",
        || format!("file: {file:?}"),
    );
    // A restart: a service that has only the file.
    let reopened = Service::open(ProfileStore::at(live.profile.clone()))
        .expect("a service over the saved profile");
    let outcome = reopened.check(&CheckRequest::new("restart", note));
    report.expect(
        matches!(outcome.status, CheckStatus::Checked) && outcome.diagnostics.is_empty(),
        "a restarted service does not flag the word",
        || format!("{:?}", outcome.diagnostics),
    );
}

/// The editor on the window's own ground. A frame GPUI renders to an image has
/// no window behind it, and a bare editor inherits black ink, so a capture of
/// the editor alone is black on black. The notes surface supplies this ground
/// in the product.
struct Ground {
    editor: Entity<EditorState>,
}

/// The space between the window's edge and the editor.
const GROUND_PAD: f32 = 16.;

impl Render for Ground {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .size_full()
            .p(px(GROUND_PAD))
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(px(16.))
            .child(self.editor.clone())
    }
}

/// The two captures: the underline, then the menu a right-click on the
/// flagged word opens. Both are GPUI's own render of the window, which needs
/// no screen recording permission.
async fn shoot(
    window: AnyWindowHandle,
    editor: &Entity<EditorState>,
    dir: &Path,
    cx: &mut AsyncApp,
) -> bool {
    if let Err(error) = std::fs::create_dir_all(dir) {
        println!("cannot create {}: {error}", dir.display());
        return false;
    }
    let note = "Please recieve this letter today.";
    editor.update(cx, |editor, cx| editor.set_text(note, cx));
    let watched = editor.clone();
    let shown = until(cx, "the underline", Duration::from_secs(20), move |cx| {
        let editor = watched.read(cx);
        editor
            .diagnostics()
            .first()
            .is_some_and(|d| !editor.flagged_rows(d.range.start + 1).1.is_empty())
    })
    .await;
    if !shown {
        return false;
    }
    let word = editor.read_with(cx, |editor, _| editor.diagnostics()[0].range.clone());
    println!("underlined {:?}", &note[word.clone()]);
    let _ = window.update(cx, |_, window, cx| {
        cx.activate(true);
        window.activate_window();
    });
    sleep(cx, Duration::from_millis(1500)).await;
    if !shot::capture(window, cx, &dir.join("notes-underline.png")) {
        return false;
    }

    // Where the word is, in window points: the editor starts at the ground's
    // padding, and the word's bounds follow from its own layout.
    let word_at = window.update(cx, |_, window, cx| {
        let width = window.bounds().size.width - px(2. * GROUND_PAD);
        let editor_bounds =
            Bounds::new(point(px(GROUND_PAD), px(GROUND_PAD)), size(width, px(100.)));
        let utf16 = word.clone();
        editor.update(cx, |editor, cx| {
            editor.bounds_for_range(utf16, editor_bounds, window, cx)
        })
    });
    let Ok(Some(word_at)) = word_at else {
        println!("the flagged word has no bounds");
        return false;
    };
    let position = word_at.center();
    let _ = window.update(cx, |_, window, cx| {
        for event in [
            PlatformInput::MouseMove(MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Modifiers::default(),
            }),
            PlatformInput::MouseDown(MouseDownEvent {
                position,
                button: MouseButton::Right,
                click_count: 1,
                ..Default::default()
            }),
        ] {
            window.dispatch_event(event, cx);
        }
    });
    sleep(cx, Duration::from_millis(800)).await;
    shot::capture(window, cx, &dir.join("notes-menu.png"))
}

async fn sleep(cx: &mut AsyncApp, time: Duration) {
    cx.background_executor().timer(time).await;
}

async fn until(
    cx: &mut AsyncApp,
    what: &str,
    timeout: Duration,
    mut done: impl FnMut(&mut App) -> bool,
) -> bool {
    let started = Instant::now();
    loop {
        if cx.update(|cx| done(cx)) {
            return true;
        }
        if started.elapsed() > timeout {
            eprintln!("timed out waiting for {what}");
            return false;
        }
        sleep(cx, Duration::from_millis(50)).await;
    }
}
