//! Synthetic fresh-process storage acceptance; uses the production Engine.
extern crate gpui_kit as gpui;
#[allow(dead_code)]
#[path = "../src/engine.rs"]
mod engine;
#[cfg(feature = "performance-check")]
#[allow(dead_code)]
#[path = "../src/performance.rs"]
mod performance;
#[allow(dead_code)]
#[path = "../src/scene.rs"]
mod scene;
use clap::Parser;
use servo::{JSValue, LoadStatus, WebView, WebViewBuilder, WebViewDelegate};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};
#[derive(Parser)]
struct Args {
    #[arg(long)]
    root: PathBuf,
    #[arg(long)]
    base: url::Url,
    #[arg(long)]
    phase: String,
}
#[derive(Default)]
struct Probe {
    painted: Cell<bool>,
    pending: Cell<bool>,
    result: RefCell<Option<bool>>,
}
impl WebViewDelegate for Probe {
    fn notify_new_frame_ready(&self, view: WebView) {
        view.paint();
        view.rendering_context().present();
        self.painted.set(true);
    }
    fn notify_crashed(&self, _: WebView, _: String, _: Option<String>) {
        *self.result.borrow_mut() = Some(false);
    }
}
fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let store = frame_profile::ProfileStore::open(&args.root)?;
    let store = if args.phase == "isolated" {
        store.imported_profile("isolated", "Isolated", "")?
    } else {
        store
    };
    if args.phase == "write" {
        store.bookmark("https://servo.org/", "Servo browser")?;
        store.save_session(&frame_profile::Session {
            tabs: vec![frame_profile::SessionTab {
                url: Some(args.base.to_string()),
                query: None,
            }],
            ..Default::default()
        })?;
    } else if args.phase == "read" {
        anyhow::ensure!(
            store.suggestions("servo bro", true, 10)?.len() == 1,
            "bookmark/index not restored"
        );
        anyhow::ensure!(store.session()?.tabs.len() == 1, "session not restored");
    } else {
        anyhow::ensure!(
            store.suggestions("servo", true, 10)?.is_empty(),
            "profile leak"
        );
    }
    let profile = store.directory();
    let url = args.base.join(&format!("?{}", args.phase))?;
    let result = Rc::new(Probe::default());
    let final_result = result.clone();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let phase = args.phase.clone();
    gpui_kit::application().run(move |cx| {
        engine::Engine::start(&profile, cx);
        let context = Rc::new(
            scene::SceneRenderingContext::new(dpi::PhysicalSize::new(900, 700))
                .expect("GPU context"),
        );
        let view = WebViewBuilder::new(&engine::Engine::servo(cx), context)
            .url(url)
            .delegate(result.clone())
            .build();
        view.show();
        let probe = result.clone();
        let event_phase = phase.clone();
        engine::Engine::after_spin(
            move |cx| {
                if let Some(passed) = *probe.result.borrow() {
                    println!("{}", serde_json::json!({"phase": event_phase, "passed": passed}));
                    cx.quit();
                    return false;
                }
                if view.load_status() == LoadStatus::Complete
                    && probe.painted.get()
                    && !probe.pending.replace(true)
                {
                    let probe = probe.clone();
                    view.evaluate_javascript(
                        "window.storageResult === undefined ? null : window.storageResult",
                        move |r| {
                            if let Ok(JSValue::Boolean(passed)) = r {
                                *probe.result.borrow_mut() = Some(passed);
                            }
                            probe.pending.set(false);
                        },
                    );
                }
                true
            },
            cx,
        );
        cx.spawn(async move |cx| {
            // Poll via Servo's ordinary wake loop without keeping a WebView alive
            // across the quit hook; storage flushes after the delegate is dropped.
            for _ in 0..400 {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if let Some(passed) = *result.result.borrow() {
                    println!("{}", serde_json::json!({"phase": phase, "passed": passed}));
                    cx.update(|cx| cx.quit());
                    return;
                }
                cx.update(|cx| {
                    engine::Engine::servo(cx).spin_event_loop();
                });
            }
            *result.result.borrow_mut() = Some(false);
            println!(
                "{}",
                serde_json::json!({"phase": phase, "passed": false, "error": "timeout"})
            );
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    let passed = *final_result.result.borrow() == Some(true);
    println!(
        "{}",
        serde_json::json!({"phase":args.phase,"passed":passed})
    );
    anyhow::ensure!(passed, "native storage acceptance failed");
    Ok(())
}
