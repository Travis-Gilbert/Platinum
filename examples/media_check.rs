//! Exercise Frame's actual Servo configuration and GPU context, without the
//! search service or the user's profile. See docs/plans/youtube.md.
extern crate gpui_kit as gpui;

#[allow(dead_code)]
#[path = "../src/engine.rs"]
mod engine;
#[allow(dead_code)]
#[path = "../src/scene.rs"]
mod scene;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use clap::Parser;
use servo::{
    ConsoleLogLevel, LoadStatus, UserContentManager, WebView, WebViewBuilder, WebViewDelegate,
};

#[derive(Parser)]
#[command(about = "Check Frame's native Servo media path using a temporary profile")]
struct Args {
    /// HTTP base URL of Servo's WPT media directory; omit for codec reporting.
    fixture_base: Option<url::Url>,
    /// Inspect a YouTube watch page and test whether its video clock advances.
    #[arg(long, conflicts_with = "fixture_base")]
    youtube: Option<url::Url>,
    /// HTTP base URL of Servo WPT media-source fixtures.
    #[arg(long, conflicts_with_all = ["fixture_base", "youtube"])]
    mse: Option<url::Url>,
    /// Physical viewport size for reproducing desktop rendering issues.
    #[arg(long, default_value_t = 1280, value_parser = clap::value_parser!(u32).range(1..))]
    width: u32,
    #[arg(long, default_value_t = 820, value_parser = clap::value_parser!(u32).range(1..))]
    height: u32,
    #[arg(long, default_value_t = 1.0)]
    scale: f32,
    /// Match a browser window using the dark system appearance.
    #[arg(long)]
    dark: bool,
}

#[derive(Default)]
struct Results(RefCell<Option<bool>>, Cell<u64>);

fn finish(profile: &std::path::Path, passed: bool) -> ! {
    // AppKit's application termination exits with 0, so an acceptance probe
    // must return its own status before asking AppKit to quit.
    let _ = std::fs::remove_dir_all(profile);
    std::process::exit(if passed { 0 } else { 1 });
}

impl WebViewDelegate for Results {
    fn notify_load_status_changed(&self, _: WebView, status: LoadStatus) {
        if status == LoadStatus::Started {
            self.1.set(self.1.get() + 1);
        }
        eprintln!("media check: document {} {status:?}", self.1.get());
    }

    fn notify_new_frame_ready(&self, view: WebView) {
        view.paint();
        view.rendering_context().present();
    }

    fn show_console_message(&self, view: WebView, _: ConsoleLogLevel, message: String) {
        if let Some(
            file @ ("movie_5.mp4" | "movie_5.webm" | "youtube" | "mse" | "svg-initial"
            | "svg-restyled"),
        ) = message.strip_prefix("FRAME_MEDIA_FRAME ")
        {
            let path = std::env::temp_dir().join(format!("frame-media-{file}.png"));
            view.take_screenshot(None, move |result| match result {
                Ok(image) => match image.save(&path) {
                    Ok(()) => eprintln!("frame screenshot: {}", path.display()),
                    Err(error) => eprintln!("frame screenshot failed: {error}"),
                },
                Err(error) => eprintln!("frame screenshot failed: {error:?}"),
            });
        }
        if let Some(json) = message.strip_prefix("FRAME_MEDIA_RESULT ") {
            println!("{json}");
            let passed = serde_json::from_str::<serde_json::Value>(json)
                .ok()
                .and_then(|value| value["passed"].as_bool())
                .unwrap_or(false);
            *self.0.borrow_mut() = Some(passed);
        }
    }

    fn notify_crashed(&self, _: WebView, reason: String, _: Option<String>) {
        eprintln!("media check: Servo crashed: {reason}");
        *self.0.borrow_mut() = Some(false);
    }
}

fn main() -> anyhow::Result<()> {
    // Supply an HTTP base URL ending in /media/ from Servo's WPT fixtures.
    // Without one, only capability reporting is tested.
    let args = Args::parse();
    anyhow::ensure!(
        args.scale.is_finite() && args.scale > 0.0,
        "invalid display scale"
    );
    let page_url = args
        .youtube
        .clone()
        .or(args.mse.clone())
        .or(args.fixture_base.clone())
        .unwrap_or(url::Url::parse("about:blank")?);
    let script = if args.youtube.is_some() {
        include_str!("../tests/media/youtube.js").to_owned()
    } else if let Some(base) = &args.mse {
        format!(
            "const fixtureBase = {};\n{}",
            serde_json::to_string(base.as_str())?,
            include_str!("../tests/media/mse.js")
        )
    } else {
        format!(
            "const fixtureBase = {};\n{}",
            serde_json::to_string(&args.fixture_base.as_ref().map(url::Url::as_str))?,
            include_str!("../tests/media/playback.js")
        )
    };
    let script = match std::env::var_os("FRAME_MEDIA_SCRIPT") {
        Some(path) => std::fs::read_to_string(path)?,
        None => script,
    };
    let script = if args.youtube.is_some() {
        format!(
            "(() => {{ const run = () => {{ {script} }}; if (document.readyState === 'loading') addEventListener('DOMContentLoaded', run, {{once: true}}); else run(); }})()"
        )
    } else {
        script
    };
    let profile = std::env::temp_dir().join(format!("frame-media-{}", std::process::id()));
    std::fs::create_dir_all(&profile)?;
    let app_profile = profile.clone();
    let result = Rc::new(Results::default());
    let app_result = result.clone();
    let _ = rustls::crypto::ring::default_provider().install_default();
    gpui_kit::application().run(move |cx| {
        engine::Engine::start(&app_profile, cx);
        let context = match scene::SceneRenderingContext::new(dpi::PhysicalSize::new(
            args.width,
            args.height,
        )) {
            Ok(context) => Rc::new(context),
            Err(error) => {
                eprintln!("media check: GPU context failed: {error:?}");
                *app_result.0.borrow_mut() = Some(false);
                finish(&app_profile, false);
            }
        };
        let servo = engine::Engine::servo(cx);
        let user_contents = Rc::new(UserContentManager::new(&servo));
        user_contents.add_script(Rc::new(include_str!("../tests/media/instrument.js").into()));
        let view = WebViewBuilder::new(&servo, context)
            .user_content_manager(user_contents)
            .url(page_url)
            .hidpi_scale_factor(euclid::Scale::new(args.scale))
            .delegate(app_result.clone())
            .build();
        if args.dark {
            view.notify_theme_change(servo::Theme::Dark);
        }
        view.show();
        let mut started = None;
        let pending_result = app_result.clone();
        let pending_profile = app_profile.clone();
        engine::Engine::after_spin(
            move |_| {
                if let Some(passed) = *pending_result.0.borrow() {
                    finish(&pending_profile, passed);
                }
                // Watch pages can keep an unrelated subresource pending even
                // while the player is usable. Their scripts already wait for
                // the relevant DOM; don't make diagnostics depend on `load`.
                let ready = view.load_status() == LoadStatus::Complete
                    || (args.youtube.is_some() && view.load_status() == LoadStatus::HeadParsed);
                let document = pending_result.1.get();
                if started != Some(document) && ready {
                    started = Some(document);
                    let pending_result = pending_result.clone();
                    view.evaluate_javascript(&script, move |result| {
                        if let Err(error) = result {
                            eprintln!("media check: script failed: {error:?}");
                            *pending_result.0.borrow_mut() = Some(false);
                        }
                    });
                }
                true
            },
            cx,
        );
        cx.spawn(async move |cx| {
            cx.background_executor()
                .timer(Duration::from_secs(60))
                .await;
            if app_result.0.borrow().is_none() {
                eprintln!("media check: timed out before completion");
                *app_result.0.borrow_mut() = Some(false);
            }
            finish(&app_profile, *app_result.0.borrow() == Some(true));
        })
        .detach();
    });
    // This directory belongs only to this invocation, never Frame's profile.
    let _ = std::fs::remove_dir_all(profile);
    if *result.0.borrow() != Some(true) {
        anyhow::bail!("media acceptance failed");
    }
    Ok(())
}
