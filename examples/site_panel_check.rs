//! Live check for the site panel's data: `Page::read_site` with
//! `src/scripts/site-panel.js` and `src/site.rs`, on a real `Page` in a small
//! window, with a temporary profile.
//!
//! ```text
//! script/media-env cargo build --locked --example site_panel_check
//! site_panel_check                              # check tests/site-panel, print each reading
//! site_panel_check https://www.vitsoe.com       # read one page and print it
//! ```
//!
//! The fixture run loads `index.html` under the light scheme and then the
//! dark one, and `sparse.html`; it exits 1 if a field or a dominant color is
//! wrong. A URL run exits 1 only if no reading comes back.
extern crate gpui_kit as gpui;
#[path = "../src/actions.rs"]
mod actions;
#[path = "../src/theme.rs"]
mod theme;
#[path = "../src/icons.rs"]
mod icons;
#[path = "../src/markup.rs"]
mod markup;
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
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, AsyncApp, Bounds, Entity, WindowOptions, point, px, size};
use serde_json::json;
use url::Url;

use crate::page::{Page, PageEvent};
use crate::site::{FontRole, SiteInfo};

#[derive(Parser)]
#[command(about = "Check the site panel's data on a real page")]
struct Args {
    /// A page to read and print; without one, the fixtures are checked.
    url: Option<Url>,
    /// Seconds to wait after a load for scripts and web fonts to settle.
    #[arg(long)]
    settle: Option<f64>,
    /// A directory to write each reading to, as JSON.
    #[arg(long)]
    out: Option<PathBuf>,
    /// The window's size in points.
    #[arg(long, default_value_t = 1000.0)]
    width: f32,
    #[arg(long, default_value_t = 700.0)]
    height: f32,
}

/// What `tests/site-panel/index.html` is made of, as fractions of the frame.
const BLOCKS: [([u8; 3], f32); 4] = [
    ([0x1f, 0x4e, 0x79], 0.50),
    ([0xe8, 0xb9, 0x23], 0.25),
    ([0xc0, 0x39, 0x2b], 0.15),
    ([0x2e, 0x8b, 0x57], 0.10),
];
const TITLE: &str = "Site Panel Fixture";
const DESCRIPTION: &str =
    "A page with known metadata, fonts and flat colour blocks, for the site panel check.";
const LIGHT_THEME: [u8; 3] = [0xe8, 0xd9, 0xb0];
const DARK_THEME: [u8; 3] = [0x1b, 0x3a, 0x6b];

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let profile = std::env::temp_dir().join(format!("frame-site-panel-{}", std::process::id()));
    std::fs::create_dir_all(&profile)?;
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Light, None, cx);
            engine::Engine::start(&profile, cx);
            let first = args.url.clone().unwrap_or_else(|| fixture("index.html"));
            let options = WindowOptions {
                window_bounds: Some(gpui_kit::WindowBounds::Windowed(Bounds::new(
                    point(px(60.), px(60.)),
                    size(px(args.width), px(args.height)),
                ))),
                ..Default::default()
            };
            let window = cx
                .open_window(options, |window, cx| {
                    Page::open(first, window, cx).expect("a page")
                })
                .expect("a window");
            let page = window.entity(cx).expect("the window's page");
            let loads = Rc::new(Cell::new(0));
            let counted = loads.clone();
            cx.subscribe(&page, move |_, event: &PageEvent, _| {
                if matches!(event, PageEvent::Visited) {
                    counted.set(counted.get() + 1);
                }
            })
            .detach();
            cx.spawn(async move |cx| {
                let passed = run(&args, &page, &loads, cx).await;
                // AppKit's quit exits with 0; an acceptance check needs its own status.
                let _ = std::fs::remove_dir_all(&profile);
                std::process::exit(if passed { 0 } else { 1 });
            })
            .detach();
        });
    Ok(())
}

fn fixture(name: &str) -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/site-panel")
        .join(name);
    Url::from_file_path(path).expect("an absolute fixture path")
}

async fn run(args: &Args, page: &Entity<Page>, loads: &Rc<Cell<u32>>, cx: &mut AsyncApp) -> bool {
    let started = Instant::now();
    let mut failures = Vec::new();
    let settle = |default: f64| Duration::from_secs_f64(args.settle.unwrap_or(default));

    if !loaded(cx, loads, 0).await {
        return false;
    }
    sleep(cx, settle(if args.url.is_some() { 5. } else { 1. })).await;
    println!(
        "document.fonts in this Servo: {}",
        fonts_support(cx, page).await
    );

    if args.url.is_some() {
        let Some(info) = read(cx, page, None).await else {
            return false;
        };
        println!("{}", report(&info));
        save(args, "reading", &info);
        println!("url run took {:?}", started.elapsed());
        return !info.fonts.is_empty() || info.title.is_some();
    }

    // index.html under the light scheme.
    let Some(light) = read(cx, page, None).await else {
        return false;
    };
    println!("index.html, light:\n{}", report(&light));
    save(args, "index-light", &light);
    check_index(&light, LIGHT_THEME, &mut failures);

    // The same page under the dark scheme: only the theme color follows it.
    cx.update(|cx| Theme::change(ThemeMode::Dark, None, cx));
    sleep(cx, Duration::from_millis(600)).await;
    let Some(dark) = read(cx, page, Some(&light)).await else {
        return false;
    };
    println!("index.html, dark:\n{}", report(&dark));
    save(args, "index-dark", &dark);
    check_index(&dark, DARK_THEME, &mut failures);

    // A page with little to read.
    let before = loads.get();
    cx.update(|cx| page.update(cx, |page, _| page.load(fixture("sparse.html"))));
    if !loaded(cx, loads, before).await {
        return false;
    }
    sleep(cx, settle(1.)).await;
    let Some(sparse) = read(cx, page, Some(&dark)).await else {
        return false;
    };
    println!("sparse.html:\n{}", report(&sparse));
    save(args, "sparse", &sparse);
    check_sparse(&sparse, &mut failures);

    for failure in &failures {
        println!("FAIL: {failure}");
    }
    println!(
        "{} after {:?}",
        if failures.is_empty() { "PASS" } else { "FAIL" },
        started.elapsed()
    );
    failures.is_empty()
}

fn check_index(info: &SiteInfo, theme: [u8; 3], failures: &mut Vec<String>) {
    let scheme = site::hex(theme);
    expect(
        info.url.as_ref() == Some(&fixture("index.html")),
        format!("url is {:?}", info.url),
        failures,
    );
    expect(
        info.title.as_deref() == Some(TITLE),
        format!("title is {:?}", info.title),
        failures,
    );
    expect(
        info.description.as_deref() == Some(DESCRIPTION),
        format!("description is {:?}", info.description),
        failures,
    );
    expect(
        info.theme_color == Some(theme),
        format!(
            "theme color is {:?}, want {scheme}",
            info.theme_color.map(site::hex)
        ),
        failures,
    );
    let fonts: Vec<_> = info
        .fonts
        .iter()
        .map(|font| (font.family.as_str(), font.roles.as_slice()))
        .collect();
    expect(
        fonts
            == [
                ("Georgia", &[FontRole::Headings][..]),
                ("Helvetica Neue", &[FontRole::Body][..]),
                ("Menlo", &[FontRole::Code][..]),
            ],
        format!("fonts are {fonts:?}"),
        failures,
    );
    // Each flat block is a swatch of about its share of the frame; text over
    // the largest block moves its color a little, so the colors have room.
    for (color, share) in BLOCKS {
        let nearest = info
            .palette
            .iter()
            .min_by(|a, b| distance(a.color, color).total_cmp(&distance(b.color, color)));
        let ok = nearest.is_some_and(|swatch| {
            distance(swatch.color, color) <= 30. && (swatch.share - share).abs() <= 0.05
        });
        expect(
            ok,
            format!(
                "no swatch near {} at about {:.0}%: nearest is {:?}",
                site::hex(color),
                share * 100.,
                nearest.map(|swatch| (site::hex(swatch.color), swatch.share))
            ),
            failures,
        );
    }
    expect(
        info.palette
            .first()
            .is_some_and(|swatch| distance(swatch.color, BLOCKS[0].0) <= 30.),
        "the largest swatch is not the largest block".into(),
        failures,
    );
    expect(
        info.palette
            .windows(2)
            .all(|pair| pair[0].share >= pair[1].share),
        "the palette is not largest first".into(),
        failures,
    );
}

fn check_sparse(info: &SiteInfo, failures: &mut Vec<String>) {
    expect(
        info.title.as_deref() == Some("Sparse Fixture"),
        format!("title is {:?}", info.title),
        failures,
    );
    expect(
        info.description.as_deref()
            == Some("Only an Open Graph description, no theme colour and no code."),
        format!("description is {:?}", info.description),
        failures,
    );
    expect(
        info.theme_color.is_none(),
        format!("theme color is {:?}", info.theme_color.map(site::hex)),
        failures,
    );
    let fonts: Vec<_> = info
        .fonts
        .iter()
        .map(|font| (font.family.as_str(), font.roles.as_slice()))
        .collect();
    expect(
        fonts == [("Georgia", &[FontRole::Headings, FontRole::Body][..])],
        format!("fonts are {fonts:?}"),
        failures,
    );
    let white = info.palette.first().is_some_and(|swatch| {
        distance(swatch.color, [255, 255, 255]) <= 30. && swatch.share >= 0.85
    });
    expect(white, "the palette is not mostly white".into(), failures);
}

fn expect(ok: bool, message: String, failures: &mut Vec<String>) {
    if !ok {
        failures.push(message);
    }
}

fn distance(a: [u8; 3], b: [u8; 3]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(a, b)| f32::from(a.abs_diff(b)).powi(2))
        .sum::<f32>()
        .sqrt()
}

fn save(args: &Args, name: &str, info: &SiteInfo) {
    let Some(directory) = &args.out else {
        return;
    };
    let written = std::fs::create_dir_all(directory)
        .and_then(|()| std::fs::write(directory.join(format!("{name}.json")), report(info) + "\n"));
    if let Err(error) = written {
        eprintln!("could not write {name}.json: {error}");
    }
}

/// The reading, as JSON.
fn report(info: &SiteInfo) -> String {
    let value = json!({
        "url": info.url.as_ref().map(Url::as_str),
        "title": info.title,
        "description": info.description,
        "theme_color": info.theme_color.map(site::hex),
        "fonts": info.fonts.iter().map(|font| json!({
            "family": font.family,
            "roles": font.roles.iter().map(|role| role.label()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "palette": info.palette.iter().map(|swatch| json!({
            "color": site::hex(swatch.color),
            "share": (f64::from(swatch.share) * 1000.).round() / 1000.,
        })).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).expect("JSON")
}

/// Ask for a reading and wait for the one that is not `previous`.
async fn read(
    cx: &mut AsyncApp,
    page: &Entity<Page>,
    previous: Option<&SiteInfo>,
) -> Option<SiteInfo> {
    let asked = Instant::now();
    cx.update(|cx| page.read(cx).read_site());
    let arrived = until(
        cx,
        "the site panel's reading",
        Duration::from_secs(20),
        |cx| {
            page.read(cx)
                .site()
                .is_some_and(|info| Some(info) != previous)
        },
    )
    .await;
    println!("reading arrived after {:?}", asked.elapsed());
    if !arrived {
        return None;
    }
    cx.update(|cx| page.read(cx).site().cloned())
}

/// What `document.fonts` offers in this build of Servo.
async fn fonts_support(cx: &mut AsyncApp, page: &Entity<Page>) -> String {
    let answer = Rc::new(RefCell::new(None));
    let slot = answer.clone();
    cx.update(|cx| {
        page.read(cx).webview.evaluate_javascript(
            "JSON.stringify({check: typeof document.fonts.check, status: typeof document.fonts.status, \
             ready: typeof document.fonts.ready, forEach: typeof document.fonts.forEach, \
             size: document.fonts.size, FontFace: typeof FontFace})",
            move |result| {
                *slot.borrow_mut() = Some(match result {
                    Ok(servo::JSValue::String(text)) => text,
                    other => format!("{other:?}"),
                })
            },
        )
    });
    until(
        cx,
        "document.fonts support",
        Duration::from_secs(10),
        |_| answer.borrow().is_some(),
    )
    .await;
    answer.take().unwrap_or_else(|| "no answer".into())
}

/// Wait for a load beyond the `before`th to finish.
async fn loaded(cx: &mut AsyncApp, loads: &Rc<Cell<u32>>, before: u32) -> bool {
    until(cx, "the page to load", Duration::from_secs(90), |_| {
        loads.get() > before
    })
    .await
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
