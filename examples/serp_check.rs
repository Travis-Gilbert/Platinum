//! Compile and live check for `src/search.rs` and `src/serp.rs` (with the
//! `src/markup.rs`, `src/favicon.rs` and `src/icons.rs` it draws with).
//!
//! ```text
//! cargo build --example serp_check
//! serp_check --headless "rust async runtimes"        # one real search, printed
//! serp_check --headless "why is the sky blue" --ask  # same, through the Ask path
//! serp_check --headless "tide tables" --again 60     # search, wait, search again
//! serp_check "rust async runtimes"                   # a window showing the page
//! ```
//!
//! `--again` gives the crawl campaign and the embedding worker that many
//! seconds before the second search; `RUST_LOG` (default `serp_check=info`)
//! shows what they did.
//!
//! Keys and `THEOREM_WEB_SYNTHESIS_CONFIG` are read from the environment and a
//! `.env` file in the working directory or above it. The session store lives in
//! the system temp directory so a check never touches real browsing data.

extern crate gpui_kit as gpui;
#[allow(dead_code)]
#[path = "../src/brand.rs"]
mod brand;
#[path = "../src/embedder.rs"]
mod embedder;
#[path = "../src/favicon.rs"]
mod favicon;
#[path = "../src/icons.rs"]
mod icons;
#[path = "../src/markup.rs"]
mod markup;
#[cfg(feature = "performance-check")]
#[allow(dead_code)]
#[path = "../src/performance.rs"]
mod performance;
#[path = "../src/search.rs"]
mod search;
#[path = "../src/serp.rs"]
mod serp;

use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};

const ASCIINEMA_THEME: &str = include_str!("../themes/asciinema.json");
const DEFAULT_QUERY: &str = "rust async runtimes";

struct Args {
    headless: bool,
    ask: bool,
    /// Seconds to wait before searching a second time.
    again: Option<u64>,
    query: String,
}

impl Args {
    fn parse() -> Self {
        let mut args = Self {
            headless: false,
            ask: false,
            again: None,
            query: String::new(),
        };
        let mut words = Vec::new();
        let mut argv = std::env::args().skip(1);
        while let Some(arg) = argv.next() {
            match arg.as_str() {
                "--headless" => args.headless = true,
                "--ask" => args.ask = true,
                "--again" => {
                    args.again = argv.next().and_then(|seconds| seconds.parse().ok());
                }
                _ => words.push(arg),
            }
        }
        args.query = if words.is_empty() {
            DEFAULT_QUERY.to_string()
        } else {
            words.join(" ")
        };
        args
    }
}

fn main() {
    dotenvy::dotenv().ok();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("serp_check=info"))
        .init();
    let args = Args::parse();
    let data_dir = std::env::temp_dir().join("frame-serp-check");

    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);
    if args.headless {
        app.run(move |cx| headless(&args, &data_dir, cx));
    } else {
        app.run(move |cx| windowed(&args, &data_dir, cx));
    }
}

/// Run one search without a window and print what came back; with `--again`,
/// wait and run it a second time.
fn headless(args: &Args, data_dir: &std::path::Path, cx: &mut App) {
    search::init(data_dir, cx);
    let (query, ask, again) = (args.query.clone(), args.ask, args.again);
    cx.spawn(async move |cx| {
        let waits = std::iter::once(None).chain(again.map(Some));
        for wait in waits {
            if let Some(seconds) = wait {
                println!("waiting {seconds} s for the crawler and the embedder");
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(seconds))
                    .await;
            }
            let task = cx.update(|cx| search::run(query.clone(), ask, cx));
            match task.await {
                Ok(result) => print_result(&result),
                Err(error) => {
                    eprintln!("search failed: {error:#}");
                    std::process::exit(1);
                }
            }
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

fn print_result(result: &search::SerpResult) {
    println!(
        "query: {:?}  mode: {}  elapsed: {} ms",
        result.query,
        if result.ask { "ask" } else { "search" },
        result.elapsed_ms
    );
    println!("providers: {}", result.providers.join(", "));
    println!("hits: {}", result.hits.len());
    for (ix, hit) in result.hits.iter().enumerate() {
        println!(
            "{:>3}. [{}] {} ({})",
            ix + 1,
            search::lane_label(&hit.lane),
            hit.title,
            hit.host
        );
        println!("     {}", hit.url);
        println!(
            "     providers: {}{}",
            hit.providers.join(", "),
            hit.published_at
                .as_deref()
                .map(|date| format!("  date: {date}"))
                .unwrap_or_default()
        );
        if !hit.snippet.text.is_empty() {
            let snippet: String = hit.snippet.text.chars().take(140).collect();
            println!("     {snippet}");
        }
    }
    println!("degradations: {}", result.degradations.len());
    for degradation in &result.degradations {
        println!("  - {}: {}", degradation.code, degradation.message);
    }
    for note in &result.notes {
        println!("note: {note}");
    }
    if let Some(answer) = &result.answer {
        println!("answer (model {}):", answer.model);
        for claim in &answer.claims {
            println!("  - {}", claim.text);
        }
    }
    if let Some(note) = &result.answer_note {
        println!("answer note: {note}");
    }
}

/// Open a window showing the page, themed like the browser.
fn windowed(args: &Args, data_dir: &std::path::Path, cx: &mut App) {
    gpui_kit::init(cx);
    load_theme(cx);
    search::init(data_dir, cx);

    let mode = if args.ask {
        serp::SerpMode::Ask
    } else {
        serp::SerpMode::Search
    };
    let query = args.query.clone();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1000.), px(800.)),
            cx,
        ))),
        ..Default::default()
    };
    let (_, page) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| serp::Serp::new(query, mode, window, cx))
    })
    .expect("failed to open the window");

    let (tab_title, tab_query) = {
        let page = page.read(cx);
        (page.title(), page.query().clone())
    };
    println!("tab title: {tab_title:?}  query: {tab_query:?}");

    cx.subscribe(&page, |_, event: &serp::SerpEvent, _| {
        println!("{event:?}");
    })
    .detach();
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
    cx.activate(true);
}

/// Install the Asciinema theme from `themes/asciinema.json` and switch to it.
fn load_theme(cx: &mut App) {
    ThemeRegistry::global_mut(cx)
        .load_themes_from_str(ASCIINEMA_THEME)
        .expect("themes/asciinema.json should parse");
    let config = ThemeRegistry::global(cx)
        .themes()
        .get("Asciinema")
        .cloned()
        .expect("themes/asciinema.json should define Asciinema");
    Theme::update(cx, |theme| theme.apply_config(&config));
    Theme::change(ThemeMode::Dark, None, cx);
}
