//! Native M1 probe using the production Page and SpatialCanvas, with an isolated profile.
extern crate gpui_kit as gpui;
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
#[path = "../src/engine.rs"]
mod engine;
#[allow(dead_code)]
#[path = "../src/icons.rs"]
mod icons;
#[allow(dead_code)]
#[path = "../src/keys.rs"]
mod keys;
#[allow(dead_code)]
#[path = "../src/page.rs"]
mod page;
#[allow(dead_code)]
#[path = "../src/scene.rs"]
mod scene;
#[allow(dead_code)]
#[path = "../src/search.rs"]
mod search;
#[allow(dead_code)]
#[path = "../src/site.rs"]
mod site;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;

#[path = "canvas_check/acceptance.rs"]
mod acceptance;
#[path = "canvas_check/foreground.rs"]
mod foreground;
#[path = "canvas_check/measurements.rs"]
mod measurements;

use clap::Parser;
use futures::{FutureExt as _, channel::oneshot};
use gpui_kit::component::Root;
use gpui_kit::{
    AppContext as _, AsyncApp, Bounds, Entity, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, ScrollDelta, ScrollWheelEvent,
    WindowBounds, WindowOptions, point, px, size,
};
use serde_json::{Value, json};
use servo::{JSValue, LoadStatus, WebView};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Parser)]
struct Args {
    #[arg(default_value = "http://127.0.0.1:8766/zoom-probe.html")]
    url: url::Url,
    #[arg(long, value_delimiter = ',', default_value = "1,0.35,2,1")]
    zoom: Vec<f64>,
    #[arg(long)]
    evidence: PathBuf,
    #[arg(long)]
    measure: bool,
    #[arg(long)]
    diagnose: bool,
}

type NativeWindow = gpui_kit::AnyWindowHandle;

async fn visible_window(
    window: NativeWindow,
    dir: &std::path::Path,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    window.update(cx, |_, w, _| w.activate_window())?;
    pause(cx, 1000).await;
    let mut state = window.update(cx, |_, w, cx| {
        json!({"visible":w.is_visible(), "visibility":format!("{:?}",w.visibility()),
            "active":w.is_window_active(), "display_refresh_hz":w.refresh_interval(cx).map(|d|1. / d.as_secs_f64()), "viewport_points":[f32::from(w.viewport_size().width),f32::from(w.viewport_size().height)]})
    })?;
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        state["native_windows"] = json!(app.windows().iter().map(|w| json!({
            "on_active_space":w.isOnActiveSpace(), "shown":w.isVisible(),
            "miniaturized":w.isMiniaturized(), "occlusion":format!("{:?}",w.occlusionState())
        })).collect::<Vec<_>>());
    }
    record(dir, "window.json", &state)?;
    anyhow::ensure!(
        state["visible"] == true && state["active"] == true,
        "needs the foreground: native window inactive, hidden or occluded after activation; no retry"
    );
    Ok(())
}

async fn pause(cx: &AsyncApp, ms: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(ms))
        .await;
}

async fn evaluate(view: &WebView, expression: &str, cx: &AsyncApp) -> anyhow::Result<Value> {
    let (send, receive) = oneshot::channel();
    view.evaluate_javascript(format!("JSON.stringify({expression})"), move |result| {
        let value = match result {
            Ok(JSValue::String(text)) => serde_json::from_str(&text).map_err(anyhow::Error::from),
            other => Err(anyhow::anyhow!("probe evaluation: {other:?}")),
        };
        let _ = send.send(value);
    });
    let receive = receive.fuse();
    let timeout = cx
        .background_executor()
        .timer(Duration::from_secs(10))
        .fuse();
    futures::pin_mut!(receive, timeout);
    futures::select! {
        result = receive => result?,
        _ = timeout => Err(anyhow::anyhow!("probe evaluation timed out")),
    }
}

fn record(dir: &std::path::Path, file: &str, value: &Value) -> anyhow::Result<()> {
    std::fs::write(dir.join(file), serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn write_outcome(dir: &std::path::Path, checked: &anyhow::Result<bool>) {
    if let Err(error) = record(
        dir,
        "result.json",
        &json!({
            "pid":std::process::id(), "passed":matches!(checked, Ok(true)),
            "error":checked.as_ref().err().map(|e|format!("{e:#}"))
        }),
    ) {
        eprintln!("cannot record probe result: {error:#}");
    }
}

fn pointer(
    window: NativeWindow,
    position: Point<Pixels>,
    pressed: Option<MouseButton>,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    window.update(cx, |_, w, cx| {
        w.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position,
                pressed_button: pressed,
                modifiers: Modifiers::default(),
            }),
            cx,
        );
    })?;
    Ok(())
}

fn click(window: NativeWindow, position: Point<Pixels>, cx: &mut AsyncApp) -> anyhow::Result<()> {
    pointer(window, position, None, cx)?;
    window.update(cx, |_, w, cx| {
        w.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                position,
                button: MouseButton::Left,
                click_count: 1,
                ..Default::default()
            }),
            cx,
        );
        w.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                position,
                button: MouseButton::Left,
                click_count: 1,
                ..Default::default()
            }),
            cx,
        );
    })?;
    Ok(())
}

async fn target(
    view: &WebView,
    page: &Entity<page::Page>,
    board: &Entity<canvas::SpatialCanvas>,
    selector: &str,
    cx: &mut AsyncApp,
) -> anyhow::Result<Point<Pixels>> {
    evaluate(view, &format!("(() => {{document.querySelector({}).scrollIntoView({{block:'center'}}); return true;}})()", serde_json::to_string(selector)?), cx).await?;
    pause(cx, 80).await;
    let rect = evaluate(view, &format!("(() => {{ const r = document.querySelector({}).getBoundingClientRect(); return [r.x + r.width/2, r.y + r.height/2]; }})()", serde_json::to_string(selector)?), cx).await?;
    let x = rect[0]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("target x"))? as f32;
    let y = rect[1]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("target y"))? as f32;
    board.update(cx, |b, cx| b.reveal_page_point([x, y], cx));
    pause(cx, 250).await;
    Ok(page.read_with(cx, |p, _| {
        p.transform()
            .expect("placed page")
            .window_point(x * p.scale, y * p.scale)
    }))
}

async fn run(
    args: Args,
    page: Entity<page::Page>,
    board: Entity<canvas::SpatialCanvas>,
    window: NativeWindow,
    cx: &mut AsyncApp,
) -> anyhow::Result<bool> {
    visible_window(window, &args.evidence, cx).await?;
    let guard = foreground::Guard::new(window, args.evidence.clone(), !args.diagnose, cx)?;
    let mut monitor_cx = cx.clone();
    let checked = {
        let work = run_body(args, page, board, window, cx).fuse();
        let monitor = guard.watch(window, &mut monitor_cx).fuse();
        futures::pin_mut!(work, monitor);
        futures::select_biased! { stopped = monitor => stopped, result = work => result }
    };
    guard.finish()?;
    checked
}

async fn run_body(
    args: Args,
    page: Entity<page::Page>,
    board: Entity<canvas::SpatialCanvas>,
    window: NativeWindow,
    cx: &mut AsyncApp,
) -> anyhow::Result<bool> {
    let view = page.read_with(cx, |p, _| p.webview.clone());
    let webview_id = format!("{:?}", view.id());
    let started = Instant::now();
    while view.load_status() != LoadStatus::Complete {
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(30),
            "fixture did not load"
        );
        pause(cx, 50).await;
    }
    pause(cx, 500).await;
    if args.diagnose && args.url.path().contains("benchmark") {
        return measurements::diagnose_surface(&args.evidence, &page, window, cx).await;
    }
    if args.diagnose {
        return acceptance::diagnose(&args.evidence, &view, &page, &board, window, cx).await;
    }
    let signin = args.url.path().contains("signin");
    if signin {
        let pos = target(&view, &page, &board, "#draft", cx).await?;
        click(window, pos, cx)?;
        pause(cx, 100).await;
        for ch in "I was halfway through".chars() {
            let keystroke = Keystroke {
                key: if ch == ' ' {
                    "space".into()
                } else {
                    ch.to_string()
                },
                key_char: Some(ch.to_string()),
                modifiers: Modifiers::default(),
            };
            window.update(cx, |_, w, cx| {
                w.dispatch_event(
                    PlatformInput::KeyDown(gpui_kit::KeyDownEvent {
                        keystroke: keystroke.clone(),
                        is_held: false,
                        prefer_character_input: true,
                    }),
                    cx,
                );
                w.dispatch_event(PlatformInput::KeyUp(gpui_kit::KeyUpEvent { keystroke }), cx);
            })?;
        }
        pause(cx, 200).await;
        // Fixture state setup is explicit; all typing above uses the native input path.
        evaluate(&view, "(() => { const t=document.querySelector('#draft'); t.setSelectionRange(8,8); t.scrollTop=0; window.scrollTo(0,40); return true; })()", cx).await?;
        pause(cx, 100).await;
    }
    let navigation_baseline = page.read_with(cx, |p, _| p.delegate.navigation_starts.get());
    let baseline = evaluate(&view, "window.canvasProbe.startTrace()", cx).await?;
    record(
        &args.evidence,
        "navigation.json",
        &json!({
            "initial_navigation": {"source":"Page::open", "url":args.url, "webview_id":webview_id, "document_id":baseline["documentId"]},
            "started_notifications_at_baseline":navigation_baseline,
            "note":"Servo starts a new WebView in Started, so the initial load does not generate a status-change notification. Subsequent Started transitions are counted by the production delegate."
        }),
    )?;
    let mut steps = Vec::new();
    let mut session_passed = !signin || baseline["text"] == "I was halfway through";
    for (ix, zoom) in args.zoom.iter().copied().enumerate() {
        let time = Instant::now();
        board.update(cx, |b, cx| b.zoom_to(zoom, cx));
        // This intermediate sample is before the 180 ms settle debounce.
        pause(cx, 60).await;
        let during = evaluate(&view, "window.canvasProbe.snapshot()", cx).await?;
        pause(cx, 650).await;
        if signin && zoom == 0.35 {
            board.update(cx, |b, cx| b.pan_by([120., 45.], cx));
            pause(cx, 250).await;
        }
        let sample = evaluate(&view, "window.canvasProbe.snapshot()", cx).await?;
        if signin {
            for key in [
                "documentId",
                "cookie",
                "text",
                "selectionStart",
                "selectionEnd",
                "selectionDirection",
                "scrollX",
                "scrollY",
                "textareaScrollTop",
            ] {
                session_passed &= sample[key] == baseline[key];
            }
        }
        let png =
            std::env::temp_dir().join(format!("frame-canvas-{}-{ix}.png", std::process::id()));
        window.update(cx, |_, w, _| {
            w.render_to_image()?.save(&png).map_err(anyhow::Error::from)
        })??;
        std::fs::copy(&png, args.evidence.join(format!("step-{ix}.png")))?;
        let rendered_zoom = page.read_with(cx, |p, _| p.canvas_zoom as f64);
        let row = json!({"rendered_zoom":rendered_zoom,"camera_applied":(rendered_zoom-zoom).abs()<0.00001,"step": ix, "zoom": zoom, "webview_id": webview_id, "navigation_starts": page.read_with(cx, |p, _| p.delegate.navigation_starts.get()), "same_webview": format!("{:?}", view.id()) == webview_id, "during_gesture": during, "settled": sample, "resize_delta": sample["resizeCount"].as_i64().unwrap_or(0) - baseline["resizeCount"].as_i64().unwrap_or(0), "width_drift": sample["innerWidth"].as_i64().unwrap_or(0) - baseline["innerWidth"].as_i64().unwrap_or(0), "height_drift": sample["innerHeight"].as_i64().unwrap_or(0) - baseline["innerHeight"].as_i64().unwrap_or(0), "elapsed_ms": time.elapsed().as_secs_f64()*1000., "screenshot": args.evidence.join(format!("step-{ix}.png"))});
        println!("{row}");
        steps.push(row);
        anyhow::ensure!(
            (rendered_zoom - zoom).abs() < 0.00001,
            "page did not render the requested camera zoom; this is not a valid resize-gate sample"
        );
    }
    let trace = evaluate(&view, "window.canvasProbe.finishTrace()", cx).await?;
    let constant_size = |sample: &Value| {
        sample["innerWidth"] == baseline["innerWidth"]
            && sample["innerHeight"] == baseline["innerHeight"]
    };
    let gate_passed = steps.iter().all(|s| {
        s["camera_applied"] == true
            && s["resize_delta"] == 0
            && s["width_drift"] == 0
            && s["height_drift"] == 0
            && s["during_gesture"]["resizeCount"] == baseline["resizeCount"]
            && constant_size(&s["during_gesture"])
            && constant_size(&s["settled"])
    }) && trace.as_array().is_some_and(|samples| {
        !samples.is_empty()
            && samples
                .iter()
                .all(|sample| sample["kind"] != "resize" && constant_size(sample))
    });
    record(
        &args.evidence,
        "zoom.json",
        &json!({"baseline": baseline, "steps": steps, "trace": trace, "passed": gate_passed}),
    )?;
    if signin {
        // Restore keyboard focus without clicking away from the retained caret.
        window.update(cx, |_, w, cx| {
            use gpui_kit::Focusable as _;
            w.focus(&page.read(cx).focus_handle(cx), cx);
            let keystroke = Keystroke {
                key: "!".into(),
                key_char: Some("!".into()),
                modifiers: Modifiers::default(),
            };
            w.dispatch_event(
                PlatformInput::KeyDown(gpui_kit::KeyDownEvent {
                    keystroke: keystroke.clone(),
                    is_held: false,
                    prefer_character_input: true,
                }),
                cx,
            );
            w.dispatch_event(PlatformInput::KeyUp(gpui_kit::KeyUpEvent { keystroke }), cx);
        })?;
        pause(cx, 150).await;
        let continued = evaluate(&view, "window.canvasProbe.snapshot()", cx).await?;
        session_passed &=
            continued["text"] == "I was ha!lfway through" && continued["selectionStart"] == 9;
        record(&args.evidence, "continued.json", &continued)?;
    }
    let mut input_passed = true;
    if signin {
        record(
            &args.evidence,
            "session.json",
            &json!({"passed": session_passed, "webview_id": webview_id, "baseline": baseline, "checks": ["text", "caret", "scroll", "cookie", "document_identity"]}),
        )?;
        input_passed = acceptance::check(&args.evidence, &view, &page, &board, window, cx).await?;
    }
    let navigation_final = page.read_with(cx, |p, _| p.delegate.navigation_starts.get());
    record(
        &args.evidence,
        "acceptance.json",
        &json!({"session_passed":session_passed,
        "input_gestures_passed":input_passed,"resize_gate_passed":gate_passed,
        "navigation_starts_baseline":navigation_baseline,"navigation_starts_final":navigation_final,
        "webview_id":webview_id,"document_id":baseline["documentId"]}),
    )?;
    Ok(session_passed
        && input_passed
        && navigation_baseline == navigation_final
        && steps.iter().all(|s| s["camera_applied"] == true))
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    anyhow::ensure!(
        !args.zoom.is_empty()
            && args
                .zoom
                .iter()
                .all(|z| z.is_finite() && (0.02..=4.).contains(z)),
        "zoom outside whiteboard range"
    );
    std::fs::create_dir_all(&args.evidence)?;
    let profile = tempfile::tempdir()?;
    let profile_path = profile.path().to_owned();
    let outcome = Rc::new(RefCell::new(None));
    let result = outcome.clone();
    let _ = rustls::crypto::ring::default_provider().install_default();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            engine::Engine::start(&profile_path, cx);
            if args.measure {
                measurements::start(args, result, cx);
                return;
            }
            let mut entities = None;
            let window: NativeWindow = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(50.), px(50.)),
                            size(px(900.), px(650.)),
                        ))),
                        ..Default::default()
                    },
                    |window, cx| {
                        let page =
                            page::Page::open(args.url.clone(), window, cx).expect("native page");
                        let board = cx.new(|cx| canvas::SpatialCanvas::new(page.clone(), cx));
                        entities = Some((page, board.clone()));
                        cx.new(|cx| Root::new(board, window, cx))
                    },
                )
                .expect("native canvas window")
                .into();
            cx.activate(true);
            let (page, board) = entities.expect("mounted entities");
            cx.spawn(async move |cx| {
                let evidence = args.evidence.clone();
                let checked = run(args, page, board, window, cx).await;
                write_outcome(&evidence, &checked);
                if let Err(error) = &checked {
                    eprintln!("canvas check: {error:#}");
                }
                *result.borrow_mut() = Some(matches!(checked, Ok(true)));
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    anyhow::ensure!(
        *outcome.borrow() == Some(true),
        "canvas acceptance failed or could not complete"
    );
    Ok(())
}
