//! Live check for local spelling and grammar corrections in editable web
//! text: `<input>`, `<textarea>` and `contenteditable` on a real `Page`, with
//! the production `Controls` context-menu flow, `src/controls.rs`, and a
//! temporary profile.
//!
//! ```text
//! script/media-env cargo build --locked --example text_check_live
//! text_check_live              # check tests/text-check/index.html
//! text_check_live --shots dir  # write dir/web-menu.png instead
//! ```
//!
//! Each step right-clicks a field through GPUI's input path, reads the
//! corrections the page's context menu would show, picks one, and reads the
//! field's text back from the page. It exits 1 if any step is wrong.
//!
//! With `--shots dir` it runs no checks: it right-clicks the misspelled
//! textarea of the same page, lets the navigator draw the menu over it as the
//! browser does, and writes GPUI's render of the window, the Servo page and
//! the "Spelling and Grammar" rows together, to `dir/web-menu.png`.
extern crate gpui_kit as gpui;

mod shot;

// Session preferences share the production shortcut parser without loading ASR.
#[allow(dead_code)]
#[path = "../src/voice/shortcut.rs"]
pub(crate) mod voice_shortcut;
mod voice {
    pub(crate) use crate::voice_shortcut as shortcut;
}

#[path = "../src/actions.rs"]
mod actions;
#[path = "../src/theme.rs"]
mod theme;
#[path = "../src/icons.rs"]
mod icons;
#[path = "../src/markup.rs"]
mod markup;
#[path = "../src/address.rs"]
mod address;
#[path = "../src/bump.rs"]
mod bump;
#[path = "../src/notes.rs"]
mod notes;
#[path = "../src/places.rs"]
mod places;
#[allow(dead_code)]
#[path = "../src/session.rs"]
mod session;

#[allow(dead_code)]
#[path = "../src/brand.rs"]
mod brand;
#[allow(dead_code)]
#[path = "../src/canvas/mod.rs"]
mod canvas;
#[allow(dead_code)]
#[path = "../src/controls.rs"]
mod controls;
#[allow(dead_code)]
#[path = "../src/delegate.rs"]
mod delegate;
#[allow(dead_code)]
#[path = "../src/embedder.rs"]
mod embedder;
#[allow(dead_code)]
#[path = "../src/engine.rs"]
mod engine;
#[allow(dead_code)]
#[path = "../src/keys.rs"]
mod keys;
#[allow(dead_code)]
#[path = "../src/navigator.rs"]
mod navigator;
#[allow(dead_code)]
#[path = "../src/page.rs"]
mod page;
#[cfg(feature = "performance-check")]
#[allow(dead_code)]
#[path = "../src/performance.rs"]
mod performance;
#[allow(dead_code)]
#[path = "../src/scene.rs"]
mod scene;
#[allow(dead_code)]
#[path = "../src/search.rs"]
mod search;
#[allow(dead_code)]
#[path = "../src/site.rs"]
mod site;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use clap::Parser;
use frame_text_check::{CheckRequest, CheckStatus, ProfileStore, Service};
use gpui_kit::component::command::CommandState;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{
    AnyWindowHandle, App, AsyncApp, Bounds, Context, Entity, EntityInputHandler as _, IntoElement,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    PlatformInput, Render, Styled as _, Subscription, Window, WindowBounds, WindowOptions, div,
    point, px, size,
};
use servo::JSValue;
use url::Url;

use crate::navigator::{BrowserState, Navigator, Placement};
use crate::page::{Page, PageEvent, PageMenu};

#[derive(Parser)]
#[command(about = "Check spelling and grammar corrections in editable web text")]
struct Args {
    /// The window's size in points.
    #[arg(long, default_value_t = 900.0)]
    width: f32,
    #[arg(long, default_value_t = 700.0)]
    height: f32,
    /// Instead of the checks, write `web-menu.png` to this directory.
    #[arg(long)]
    shots: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let scratch = std::env::temp_dir().join(format!("frame-text-check-{}", std::process::id()));
    std::fs::create_dir_all(scratch.join("engine"))?;
    let profile_file = scratch.join("text-check-profile.json");
    // The personal dictionary lives in a temporary file, never the real one.
    frame_text_check::install(Service::open(ProfileStore::at(profile_file.clone()))?);
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Light, None, cx);
            engine::Engine::start(&scratch.join("engine"), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(60.), px(60.)),
                    size(px(args.width), px(args.height)),
                ))),
                ..Default::default()
            };
            let (window, page): (AnyWindowHandle, Entity<Page>) = if args.shots.is_some() {
                // The capture puts the navigator over the page, as the browser does.
                let window = cx
                    .open_window(options, |window, cx| {
                        let page = Page::open(fixture(), window, cx).expect("a page");
                        cx.new(|cx| Shot::new(page, window, cx))
                    })
                    .expect("a window");
                let page = window.read(cx).expect("the window's shot").page.clone();
                (window.into(), page)
            } else {
                let window = cx
                    .open_window(options, |window, cx| {
                        Page::open(fixture(), window, cx).expect("a page")
                    })
                    .expect("a window");
                let page = window.entity(cx).expect("the window's page");
                (window.into(), page)
            };
            let loads = Rc::new(Cell::new(0));
            let menus = Rc::new(RefCell::new(Vec::new()));
            let (counted, seen) = (loads.clone(), menus.clone());
            cx.subscribe(&page, move |_, event: &PageEvent, _| match event {
                PageEvent::Visited => counted.set(counted.get() + 1),
                PageEvent::ContextMenu(menu) => seen.borrow_mut().push(menu.clone()),
                _ => {}
            })
            .detach();
            let live = Live {
                page,
                window,
                menus,
                profile_file,
                last_value: RefCell::new(None),
            };
            let shots = args.shots;
            cx.spawn(async move |cx| {
                let passed = match &shots {
                    Some(dir) => shoot(&live, &loads, dir, cx).await,
                    None => run(&live, &loads, cx).await,
                };
                // AppKit's quit exits with 0; an acceptance check needs its own status.
                let _ = std::fs::remove_dir_all(&scratch);
                std::process::exit(if passed { 0 } else { 1 });
            })
            .detach();
        });
    Ok(())
}

fn fixture() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/text-check/index.html");
    Url::from_file_path(path).expect("an absolute fixture path")
}

/// The page under test and what it has told the embedder so far.
struct Live {
    page: Entity<Page>,
    window: AnyWindowHandle,
    menus: Rc<RefCell<Vec<PageMenu>>>,
    profile_file: PathBuf,
    /// The last text read from a field, for the report when a step fails.
    last_value: RefCell<Option<String>>,
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

/// The page has loaded and drawn its first frame, so it is placed in the window.
async fn ready(live: &Live, loads: &Rc<Cell<u32>>, cx: &mut AsyncApp) -> bool {
    if !until(cx, "the page to load", Duration::from_secs(90), |_| {
        loads.get() > 0
    })
    .await
    {
        return false;
    }
    // The page is placed once it has been drawn: its first frame.
    if !until(cx, "the page to be placed", Duration::from_secs(30), |cx| {
        live.page.read(cx).transform().is_some()
    })
    .await
    {
        return false;
    }
    sleep(cx, Duration::from_secs(1)).await;
    true
}

/// What the browser draws over a page for a right-click: Platinum's navigator,
/// whose first group is "Spelling and Grammar". `Browser` owns it in the
/// product; a capture needs the same `Navigator` over the one page, nothing
/// else of the browser.
struct Shot {
    page: Entity<Page>,
    navigator: Option<Navigator>,
    _menu: Subscription,
}

impl Shot {
    fn new(page: Entity<Page>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let menu = cx.subscribe_in(
            &page,
            window,
            |this, page, event: &PageEvent, window, cx| {
                if let PageEvent::ContextMenu(menu) = event {
                    let state = cx.new(|cx| CommandState::new(window, cx));
                    state.update(cx, |state, cx| state.focus(window, cx));
                    this.navigator = Some(Navigator {
                        state,
                        placement: Placement::Pointer(menu.position),
                        page: Some((page.clone(), menu.clone())),
                        tab: None,
                        restore_focus: None,
                    });
                    cx.notify();
                }
            },
        );
        Self {
            page,
            navigator: None,
            _menu: menu,
        }
    }
}

impl Render for Shot {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let navigator = self.navigator.as_ref().map(|navigator| {
            let browser = BrowserState {
                tabs: &[],
                compact: false,
                appearance: theme::appearance(cx),
                ask: false,
                shareable: false,
                sidebar: false,
                places_pinned: false,
                pinnable: false,
            };
            navigator.render(&browser, |_, _, _| {}, |_, _| {}, cx)
        });
        div()
            .size_full()
            .relative()
            .child(self.page.clone())
            .children(navigator)
    }
}

/// Right-click the misspelled textarea and capture the window with the
/// navigator's menu over the page.
async fn shoot(live: &Live, loads: &Rc<Cell<u32>>, dir: &Path, cx: &mut AsyncApp) -> bool {
    if !ready(live, loads, cx).await {
        return false;
    }
    if let Err(error) = std::fs::create_dir_all(dir) {
        println!("cannot create {}: {error}", dir.display());
        return false;
    }
    let Some(menu) = live.right_click(cx, "spell").await else {
        println!("the page showed no context menu");
        return false;
    };
    println!("corrections offered: {:?}", menu.corrections);
    if menu.corrections.is_empty() {
        return false;
    }
    // The navigator is drawn from the menu event; give GPUI the frame.
    sleep(cx, Duration::from_millis(800)).await;
    shot::capture(live.window, cx, &dir.join("web-menu.png"))
}

async fn run(live: &Live, loads: &Rc<Cell<u32>>, cx: &mut AsyncApp) -> bool {
    let started = Instant::now();
    let mut report = Report::default();
    if !ready(live, loads, cx).await {
        return false;
    }

    println!("dictionary spelling in a textarea, corrected and undone");
    corrects_and_undoes(
        live,
        cx,
        &mut report,
        "spell",
        "Please recieve this letter today.",
        "Please receive this letter today.",
    )
    .await;

    println!("grammar in a textarea, corrected and undone");
    corrects_and_undoes(
        live,
        cx,
        &mut report,
        "grammar",
        "He are late for the meeting.",
        "He is late for the meeting.",
    )
    .await;

    println!("common misspelling in a single-line input, corrected and undone");
    corrects_and_undoes(
        live,
        cx,
        &mut report,
        "line",
        "This has teh typo.",
        "This has the typo.",
    )
    .await;

    println!("contenteditable, corrected and undone");
    corrects_and_undoes(
        live,
        cx,
        &mut report,
        "rich",
        "We will recieve it soon.",
        "We will receive it soon.",
    )
    .await;

    println!("emoji and a combining mark before the error: UTF-16 offsets");
    corrects_and_undoes(
        live,
        cx,
        &mut report,
        "emoji",
        "\u{1f600} e\u{301} teh end.",
        "\u{1f600} e\u{301} the end.",
    )
    .await;

    println!("fields that must never be read");
    for (id, why) in [
        ("secret", "a password field"),
        ("quiet", "spellcheck=\"false\""),
    ] {
        let menu = live.right_click(cx, id).await;
        let offered = menu.as_ref().map(|menu| menu.corrections.len());
        report.expect(
            menu.is_some() && offered == Some(0),
            &format!("{id} offers no correction ({why})"),
            || format!("the menu was {offered:?}"),
        );
        live.dismiss(cx);
    }

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

/// Right-click the field, take the first replacement, check the field, undo
/// it the way Cmd-Z does, and check the field again.
async fn corrects_and_undoes(
    live: &Live,
    cx: &mut AsyncApp,
    report: &mut Report,
    id: &str,
    before: &str,
    after: &str,
) {
    report.expect(
        live.value(cx, id).await.as_deref() == Some(before),
        &format!("{id} starts as {before:?}"),
        || format!("it is {:?}", live.last_value.take()),
    );
    let Some(menu) = live.right_click(cx, id).await else {
        report.expect(false, &format!("{id} shows a context menu"), || {
            "none arrived".into()
        });
        return;
    };
    let replace = menu
        .corrections
        .iter()
        .position(|label| label.starts_with("Correct "));
    report.expect(
        replace.is_some(),
        &format!("{id} offers a correction"),
        || format!("rows: {:?}", menu.corrections),
    );
    // A word can be added to the dictionary; a grammar finding cannot, and
    // is ignored by rule instead.
    let grammar = id == "grammar";
    let adds = menu
        .corrections
        .iter()
        .any(|label| label.starts_with("Add "));
    report.expect(
        adds != grammar,
        &format!(
            "{id} {} Add to Dictionary",
            if grammar { "does not offer" } else { "offers" }
        ),
        || format!("rows: {:?}", menu.corrections),
    );
    if grammar {
        report.expect(
            menu.corrections
                .iter()
                .any(|label| label.starts_with("Ignore: ")),
            "grammar offers to ignore the rule",
            || format!("rows: {:?}", menu.corrections),
        );
    }
    let Some(row) = replace else {
        live.dismiss(cx);
        return;
    };
    println!("        row: {}", menu.corrections[row]);
    live.choose(cx, row);
    let corrected = live.settle(cx, id, after).await;
    report.expect(corrected, &format!("{id} becomes {after:?}"), || {
        format!(
            "it is {:?}; note {:?}",
            live.last_value.take(),
            live.note(cx)
        )
    });
    sleep(cx, Duration::from_millis(150)).await;
    let reversed = live.reverse(cx);
    report.expect(reversed, &format!("{id} has a correction to undo"), || {
        "reverse_correction found nothing".into()
    });
    let restored = live.settle(cx, id, before).await;
    report.expect(restored, &format!("undo puts {before:?} back"), || {
        format!(
            "it is {:?}; note {:?}",
            live.last_value.take(),
            live.note(cx)
        )
    });
}

/// The menu is shown, then the person types; choosing a row must not touch
/// the text.
async fn stale_menu(live: &Live, cx: &mut AsyncApp, report: &mut Report) {
    let original = "Please recieve this letter today.";
    live.set_value(cx, "spell", original).await;
    let Some(menu) = live.right_click(cx, "spell").await else {
        report.expect(false, "the menu opens", || "none arrived".into());
        return;
    };
    let Some(row) = menu
        .corrections
        .iter()
        .position(|label| label.starts_with("Correct "))
    else {
        report.expect(false, "the menu offers a correction", || {
            format!("rows: {:?}", menu.corrections)
        });
        live.dismiss(cx);
        return;
    };
    let typed = "Oh. Please recieve this letter today.";
    live.set_value(cx, "spell", typed).await;
    live.choose(cx, row);
    sleep(cx, Duration::from_millis(600)).await;
    let value = live.value(cx, "spell").await;
    report.expect(
        value.as_deref() == Some(typed),
        "the typed text is untouched",
        || format!("the field is {value:?}"),
    );
    let note = live.note(cx);
    report.expect(
        note.is_some(),
        "the person is told why nothing changed",
        || "no note".into(),
    );
    println!("        note: {note:?}");
    live.set_value(cx, "spell", original).await;
}

/// While an input method composes, no menu offers a correction and none is
/// applied.
async fn composition(live: &Live, cx: &mut AsyncApp, report: &mut Report) {
    let original = "Please recieve this letter today.";
    live.set_value(cx, "spell", original).await;
    let Some(menu) = live.right_click(cx, "spell").await else {
        report.expect(false, "the menu opens", || "none arrived".into());
        return;
    };
    let Some(row) = menu
        .corrections
        .iter()
        .position(|label| label.starts_with("Correct "))
    else {
        report.expect(false, "the menu offers a correction", || {
            format!("rows: {:?}", menu.corrections)
        });
        live.dismiss(cx);
        return;
    };
    // The person starts composing after the menu was shown.
    live.compose(cx, "ni");
    sleep(cx, Duration::from_millis(300)).await;
    let composing = live.page.read_with(cx, |page, _| page.composing());
    report.expect(composing, "the page is composing", || "it is not".into());
    let during = live.value(cx, "spell").await;
    live.choose(cx, row);
    sleep(cx, Duration::from_millis(600)).await;
    let after = live.value(cx, "spell").await;
    report.expect(
        after == during,
        "choosing during a composition changes nothing",
        || format!("{during:?} became {after:?}"),
    );
    let note = live.note(cx);
    report.expect(
        note.as_deref()
            .is_some_and(|note| note.contains("composition")),
        "the refusal names the composition",
        || format!("note {note:?}"),
    );
    // A menu opened during the composition offers nothing, and reads nothing.
    let menu = live.right_click(cx, "spell").await;
    report.expect(
        menu.as_ref()
            .is_some_and(|menu| menu.corrections.is_empty()),
        "a menu opened during a composition offers no correction",
        || format!("rows: {:?}", menu.map(|menu| menu.corrections)),
    );
    live.dismiss(cx);
    live.end_composition(cx);
    sleep(cx, Duration::from_millis(300)).await;
    live.set_value(cx, "spell", original).await;
}

/// Add a word to the dictionary: it is no longer flagged in the page, it is
/// in the file, and a service opened afresh from that file still knows it.
async fn custom_word(live: &Live, cx: &mut AsyncApp, report: &mut Report) {
    let Some(menu) = live.right_click(cx, "custom").await else {
        report.expect(false, "the menu opens", || "none arrived".into());
        return;
    };
    let add = menu
        .corrections
        .iter()
        .position(|label| label.starts_with("Add ") && label.contains("Zorvath"));
    report.expect(
        add.is_some(),
        "Zorvath is offered for the dictionary",
        || format!("rows: {:?}", menu.corrections),
    );
    let Some(row) = add else {
        live.dismiss(cx);
        return;
    };
    live.choose(cx, row);
    sleep(cx, Duration::from_millis(300)).await;
    let service = frame_text_check::shared().expect("the installed service");
    report.expect(
        service.profile().knows("Zorvath"),
        "the service knows the word",
        || "it does not".into(),
    );
    let menu = live.right_click(cx, "custom").await;
    report.expect(
        menu.as_ref()
            .is_some_and(|menu| menu.corrections.is_empty()),
        "the word is no longer flagged in the page",
        || format!("rows: {:?}", menu.map(|menu| menu.corrections)),
    );
    live.dismiss(cx);
    let text = std::fs::read_to_string(&live.profile_file).unwrap_or_default();
    report.expect(
        text.contains("Zorvath"),
        "the profile file holds the word",
        || format!("file: {text:?}"),
    );
    // A restart: a service that has only the file.
    let reopened = Service::open(ProfileStore::at(live.profile_file.clone()))
        .expect("a service over the saved profile");
    let outcome = reopened.check(&CheckRequest::new("restart", "Ask Zorvath about the plan."));
    report.expect(
        matches!(outcome.status, CheckStatus::Checked) && outcome.diagnostics.is_empty(),
        "a restarted service does not flag the word",
        || format!("{:?}", outcome.diagnostics),
    );
    let other = reopened.check(&CheckRequest::new(
        "restart",
        "Ask Zorvathh about the plan.",
    ));
    report.expect(
        !other.diagnostics.is_empty(),
        "a different unknown word is still flagged",
        || "nothing flagged".into(),
    );
}

impl Live {
    /// Evaluate `script`, which must produce a string.
    async fn eval(&self, cx: &mut AsyncApp, script: String) -> Option<String> {
        let answer = Rc::new(RefCell::new(None));
        let slot = answer.clone();
        cx.update(|cx| {
            self.page
                .read(cx)
                .webview
                .evaluate_javascript(script, move |result| {
                    *slot.borrow_mut() = Some(match result {
                        Ok(JSValue::String(text)) => text,
                        other => format!("{other:?}"),
                    })
                })
        });
        until(cx, "the page's answer", Duration::from_secs(10), |_| {
            answer.borrow().is_some()
        })
        .await;
        answer.take()
    }

    /// The field's text, as the page has it.
    async fn value(&self, cx: &mut AsyncApp, id: &str) -> Option<String> {
        let reply = self
            .eval(
                cx,
                format!(
                    "(() => {{ const e = document.getElementById({id}); \
                     return JSON.stringify(e.value !== undefined ? e.value : e.textContent); }})()",
                    id = js(id),
                ),
            )
            .await?;
        let value = serde_json::from_str::<String>(&reply).ok();
        *self.last_value.borrow_mut() = value.clone();
        value
    }

    /// Wait for the field to read `want`.
    async fn settle(&self, cx: &mut AsyncApp, id: &str, want: &str) -> bool {
        let started = Instant::now();
        loop {
            if self.value(cx, id).await.as_deref() == Some(want) {
                return true;
            }
            if started.elapsed() > Duration::from_secs(5) {
                return false;
            }
            sleep(cx, Duration::from_millis(100)).await;
        }
    }

    /// Put `text` in the field, as typing would, with the caret at its end.
    async fn set_value(&self, cx: &mut AsyncApp, id: &str, text: &str) {
        let _ = self
            .eval(
                cx,
                format!(
                    "(() => {{ const e = document.getElementById({id}); e.focus(); \
                     if (e.value !== undefined) {{ e.value = {text}; \
                     e.setSelectionRange(e.value.length, e.value.length); }} \
                     else {{ e.textContent = {text}; }} \
                     return JSON.stringify(true); }})()",
                    id = js(id),
                    text = js(text),
                ),
            )
            .await;
    }

    /// Focus the field and give the middle of its box in CSS pixels.
    async fn focus(&self, cx: &mut AsyncApp, id: &str) -> Option<(f32, f32)> {
        let reply = self
            .eval(
                cx,
                format!(
                    "(() => {{ const e = document.getElementById({id}); e.scrollIntoView({{block: 'center'}}); \
                     e.focus(); \
                     if (e.setSelectionRange && e.type !== 'password') {{ e.setSelectionRange(e.value.length, e.value.length); }} \
                     const b = e.getBoundingClientRect(); \
                     return JSON.stringify([b.x + b.width / 2, b.y + b.height / 2]); }})()",
                    id = js(id),
                ),
            )
            .await?;
        let [x, y] = serde_json::from_str::<[f32; 2]>(&reply).ok()?;
        Some((x, y))
    }

    /// Right-click the middle of the field through GPUI's input path and wait
    /// for the menu the page is to show.
    async fn right_click(&self, cx: &mut AsyncApp, id: &str) -> Option<PageMenu> {
        let (x, y) = self.focus(cx, id).await?;
        sleep(cx, Duration::from_millis(150)).await;
        let before = self.menus.borrow().len();
        let position = self.page.read_with(cx, |page, _| {
            page.window_point((x * page.scale) as i32, (y * page.scale) as i32)
        });
        let sent = self.window.update(cx, |_, window, cx| {
            window.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    pressed_button: None,
                    modifiers: Modifiers::default(),
                }),
                cx,
            );
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    position,
                    button: MouseButton::Right,
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    position,
                    button: MouseButton::Right,
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
        });
        sent.ok()?;
        let menus = self.menus.clone();
        until(
            cx,
            "the page's context menu",
            Duration::from_secs(8),
            move |_| menus.borrow().len() > before,
        )
        .await;
        self.menus.borrow().get(before).cloned()
    }

    /// The person picked correction `index`.
    fn choose(&self, cx: &mut AsyncApp, index: usize) {
        cx.update(|cx| {
            self.page
                .update(cx, |page, cx| page.choose_correction(index, cx))
        });
    }

    /// Close the menu without a pick, so Servo's responder is answered.
    fn dismiss(&self, cx: &mut AsyncApp) {
        cx.update(|cx| self.page.update(cx, |page, _| page.dismiss_context_menu()));
    }

    /// Cmd-Z right after a correction.
    fn reverse(&self, cx: &mut AsyncApp) -> bool {
        cx.update(|cx| self.page.update(cx, |page, cx| page.reverse_correction(cx)))
    }

    fn note(&self, cx: &mut AsyncApp) -> Option<String> {
        cx.update(|cx| self.page.read(cx).correction_note().map(str::to_owned))
    }

    /// The input method starts composing `text` in the focused field.
    fn compose(&self, cx: &mut AsyncApp, text: &str) {
        let page = self.page.clone();
        let _ = self.window.update(cx, |_, window, cx| {
            page.update(cx, |page, cx| {
                page.replace_and_mark_text_in_range(None, text, None, window, cx)
            })
        });
    }

    fn end_composition(&self, cx: &mut AsyncApp) {
        self.compose(cx, "");
    }
}

/// `text` as a JavaScript string literal.
fn js(text: &str) -> String {
    serde_json::to_string(text).expect("a string serializes")
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
#[path = "../src/spoken.rs"]
mod spoken;
