//! Open every note of a vault in the notes reading view, in real Servo, and
//! write the DOM of `.markdown-reading-view` for each, laid out like
//! `crates/notes-fixtures/reference/fixture-vault/dom/` (`<note path>.html`),
//! so `notes-fixtures compare-dom` can set it beside what Obsidian recorded.
//!
//! It runs the code the app runs: the `frame-notes:` protocol handler, a vault
//! worker, and a `Session` over a web view. The only things missing are the
//! window and the dock. A note is read until three reads in a row agree, and
//! only once the page script has finished its post-processors (MathJax, Prism,
//! the copy button), so the recording includes what they produce. A note that
//! has not settled by the deadline is still written, and reported.
//!
//! The vault is copied into a temporary profile first, and the profile is
//! removed on exit: the check never writes to the vault it was given.
extern crate gpui_kit as gpui;

#[allow(dead_code)]
#[path = "../src/engine.rs"]
mod engine;
#[allow(dead_code)]
#[path = "../src/notes/host/mod.rs"]
mod host;
#[allow(dead_code)]
#[path = "../src/scene.rs"]
mod scene;

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use clap::Parser;
use futures::channel::oneshot;
use serde::Deserialize;
use servo::{
    ConsoleLogLevel, JSValue, WebView, WebViewBuilder, WebViewDelegate,
};

use gpui_kit::BackgroundExecutor;
use host::service::Vaults;
use host::session::{Session, Wiring};

/// What the page script reports, with the reading view's markup.
const READ: &str = r#"JSON.stringify({
  path: JSON.parse(document.getElementById("frame-notes-config").textContent).path,
  idle: window.frameNotes?.idle?.() === true,
  rendered: (document.querySelector(".markdown-preview-sizer")?.childElementCount ?? 0) > 0,
  html: document.querySelector(".markdown-reading-view")?.outerHTML ?? null,
})"#;

/// How long between reads, and how long a note gets to settle.
const INTERVAL: Duration = Duration::from_millis(300);
const DEADLINE: Duration = Duration::from_secs(30);
/// Reads in a row that must agree.
const AGREEING: u32 = 3;

#[derive(Parser)]
#[command(about = "Record the notes reading view's DOM for every note of a vault, in real Servo")]
struct Args {
    /// Where the recordings go: `<out>/<note path>.html`.
    out: PathBuf,
    /// The vault. Defaults to the fixture vault.
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/crates/notes-fixtures/vault"))]
    vault: PathBuf,
    /// Only the notes whose path contains this text.
    #[arg(long)]
    note: Option<String>,
    /// Physical viewport size.
    #[arg(long, default_value_t = 1280, value_parser = clap::value_parser!(u32).range(1..))]
    width: u32,
    #[arg(long, default_value_t = 820, value_parser = clap::value_parser!(u32).range(1..))]
    height: u32,
}

#[derive(Deserialize)]
struct Reading {
    path: String,
    idle: bool,
    rendered: bool,
    html: Option<String>,
}

/// How a note's reading ended.
enum Outcome {
    /// Three reads in a row agreed.
    Settled(String),
    /// The deadline came first; the last read.
    Unsettled(String),
    /// The page never showed the note.
    Missing,
}

/// The delegate under the session's: it draws what Servo paints and says what
/// the page's script complains of.
struct Frames;

impl WebViewDelegate for Frames {
    fn notify_new_frame_ready(&self, view: WebView) {
        view.paint();
        view.rendering_context().present();
    }

    fn show_console_message(&self, _: WebView, level: ConsoleLogLevel, message: String) {
        if matches!(level, ConsoleLogLevel::Warn | ConsoleLogLevel::Error) {
            eprintln!("notes check: page {level:?}: {message}");
        }
    }

    fn notify_crashed(&self, _: WebView, reason: String, _: Option<String>) {
        eprintln!("notes check: Servo crashed: {reason}");
    }
}

fn finish(profile: &Path, passed: bool) -> ! {
    // AppKit's application termination exits with 0, so an acceptance probe
    // must return its own status before asking AppKit to quit.
    let _ = std::fs::remove_dir_all(profile);
    std::process::exit(if passed { 0 } else { 1 });
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// The vault's notes, as paths from its folder, in a stable order.
fn notes_of(root: &Path, directory: &Path, found: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let Some(relative) = relative.to_str() else {
            continue;
        };
        if frame_notes::is_hidden(relative) {
            continue;
        }
        if path.is_dir() {
            notes_of(root, &path, found)?;
        } else if relative.ends_with(".md") {
            found.push(relative.to_owned());
        }
    }
    found.sort();
    Ok(())
}

/// The path as a shell would take it.
fn quoted(path: &Path) -> String {
    let text = path.to_string_lossy();
    if text.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+=:@".contains(c)) {
        text.into_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

async fn read(view: &WebView) -> Option<Reading> {
    let (sender, receiver) = oneshot::channel();
    view.evaluate_javascript(READ, move |result| {
        let _ = sender.send(result);
    });
    match receiver.await {
        Ok(Ok(JSValue::String(json))) => serde_json::from_str(&json).ok(),
        _ => None,
    }
}

/// Reads `note`'s page until `AGREEING` reads in a row agree, or the deadline.
async fn settle(view: &WebView, note: &str, executor: &BackgroundExecutor) -> Outcome {
    let deadline = Instant::now() + DEADLINE;
    let (mut last, mut agreeing): (Option<String>, u32) = (None, 0);
    while Instant::now() < deadline {
        executor.timer(INTERVAL).await;
        // A page that is not this note's, has not put it on screen, or is not
        // done, agrees with nothing.
        let html = read(view)
            .await
            .filter(|reading| reading.path == note && reading.rendered && reading.idle)
            .and_then(|reading| reading.html);
        match html {
            Some(html) if last.as_ref() == Some(&html) => agreeing += 1,
            Some(html) => (last, agreeing) = (Some(html), 1),
            None => (last, agreeing) = (None, 0),
        }
        if agreeing >= AGREEING {
            return Outcome::Settled(last.unwrap_or_default());
        }
    }
    match last {
        Some(html) => Outcome::Unsettled(html),
        None => Outcome::Missing,
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let source = args.vault.canonicalize().with_context(|| args.vault.display().to_string())?;
    let out = std::path::absolute(&args.out)?;
    let profile = std::env::temp_dir().join(format!("frame-notes-check-{}", std::process::id()));
    let root = profile.join("vault");
    copy_tree(&source, &root)?;
    let mut notes = Vec::new();
    notes_of(&root, &root, &mut notes)?;
    notes.retain(|note| args.note.as_ref().is_none_or(|filter| note.contains(filter.as_str())));
    anyhow::ensure!(!notes.is_empty(), "no notes to check in {}", source.display());
    let _ = rustls::crypto::ring::default_provider().install_default();
    let app_profile = profile.clone();
    gpui_kit::application().run(move |cx| {
        let vaults = Vaults::default();
        engine::Engine::start_with_protocols(&app_profile, host::protocols(&vaults), cx);
        let context = match scene::SceneRenderingContext::new(dpi::PhysicalSize::new(args.width, args.height)) {
            Ok(context) => Rc::new(context),
            Err(error) => {
                eprintln!("notes check: GPU context failed: {error:?}");
                finish(&app_profile, false);
            }
        };
        let service = vaults.open(&root, &app_profile.join("data"));
        let wiring = Wiring::new(service.host(), Rc::new(Frames));
        let view = WebViewBuilder::new(&engine::Engine::servo(cx), context)
            .hidpi_scale_factor(euclid::Scale::new(1.0))
            .delegate(wiring.delegate())
            .build();
        view.show();
        let session = Session::start(wiring, view.clone(), service.clone(), cx);
        let (root, out, profile) = (root.clone(), out.clone(), app_profile.clone());
        cx.spawn(async move |cx| {
            let executor = cx.background_executor().clone();
            // The settings the reference recording was made with.
            let applied = service
                .write({
                    let root = root.clone();
                    move |vault| {
                        let read = |file: &str| std::fs::read_to_string(root.join(".obsidian").join(file));
                        let mut settings = match read("app.json") {
                            Ok(json) => frame_notes::VaultSettings::from_app_json(&json)?,
                            Err(_) => frame_notes::VaultSettings::default(),
                        };
                        if let Ok(json) = read("types.json") {
                            settings.read_property_types(&json)?;
                        }
                        vault.set_settings(settings)
                    }
                })
                .await;
            if let Err(error) = applied {
                eprintln!("notes check: the vault's settings were not applied: {error}");
                finish(&profile, false);
            }
            let (mut settled, mut unsettled, mut missing) = (0, 0, Vec::new());
            for note in &notes {
                session.update(cx, |session, cx| session.show(Some(note.clone()), None, cx));
                let (html, label) = match settle(&view, note, &executor).await {
                    Outcome::Settled(html) => {
                        settled += 1;
                        (Some(html), "settled")
                    }
                    Outcome::Unsettled(html) => {
                        unsettled += 1;
                        (Some(html), "UNSETTLED")
                    }
                    Outcome::Missing => {
                        missing.push(note.clone());
                        (None, "MISSING")
                    }
                };
                eprintln!("notes check: {label:>9} {note}");
                if let Some(html) = html {
                    let file = out.join(format!("{note}.html"));
                    let written = file
                        .parent()
                        .map_or(Ok(()), std::fs::create_dir_all)
                        .and_then(|()| std::fs::write(&file, html));
                    if let Err(error) = written {
                        eprintln!("notes check: {}: {error}", file.display());
                        finish(&profile, false);
                    }
                }
            }
            println!(
                "notes check: {} notes, {settled} settled, {unsettled} unsettled, {} missing; wrote {}",
                notes.len(),
                missing.len(),
                out.display()
            );
            println!(
                "cargo run --locked -p notes-fixtures -- compare-dom --expected crates/notes-fixtures/reference/fixture-vault/dom --actual {}",
                quoted(&out)
            );
            finish(&profile, unsettled == 0 && missing.is_empty());
        })
        .detach();
    });
    // This directory belongs only to this invocation.
    let _ = std::fs::remove_dir_all(profile);
    Ok(())
}
