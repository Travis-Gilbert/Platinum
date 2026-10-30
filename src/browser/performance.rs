//! Native harness: the production Browser, Page, delegate and GPU surfaces.
use super::*;
use gpui_kit::component::Root;
use gpui_kit::{WindowBounds, WindowOptions};
use servo::JSValue;
use std::time::{Duration, Instant};

pub fn run() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("FRAME_PERF_ROOT").unwrap());
    let url = Url::parse(&std::env::var("FRAME_PERF_URL")?)?;
    let output = std::path::PathBuf::from(std::env::var_os("FRAME_PERF_OUTPUT").unwrap());
    let store = frame_profile::ProfileStore::open(&root)?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            crate::install_commands(cx);
            if url.query() == Some("search") {
                crate::search::init(&root.join("search"), cx);
                let measurement = crate::search::performance::measure(cx);
                cx.spawn(async move |cx| {
                    let result = measurement.await.expect("search measurement");
                    std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
                    cx.update(|cx| cx.quit());
                })
                .detach();
                return;
            }
            Engine::start(&root, cx);
            cx.set_global(ActiveProfile(store.clone()));
            let mut view = None;
            let window_handle = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(100.), px(100.)),
                            size(px(1280.), px(820.)),
                        ))),
                        ..TitleBar::window_options()
                    },
                    |window, cx| {
                        let browser = cx.new(|cx| Browser::new(window, cx));
                        view = Some(browser.clone());
                        browser.update(cx, |browser, cx| {
                            browser.go(url.clone(), None, false, window, cx)
                        });
                        window.activate_window();
                        cx.new(|cx| Root::new(browser, window, cx))
                    },
                )
                .unwrap();
            cx.activate(true);
            let browser = view.unwrap();
            cx.spawn(async move |cx| {
                let mut video_ready_after_ms = None;
                if matches!(url.query(), Some("video-only-hd" | "video-hd")) {
                    let started = Instant::now();
                    loop {
                        let (tx, rx) = futures::channel::oneshot::channel();
                        browser.update(cx, |browser, cx| {
                            browser.tabs[0].page.as_ref().unwrap().read(cx).webview
                                .evaluate_javascript(
                                    "(() => { const v = document.querySelector('video'); return !!v && v.readyState >= 3 && v.currentTime > 0.25; })()",
                                    move |result| { let _ = tx.send(matches!(result, Ok(JSValue::Boolean(true)))); },
                                );
                        });
                        if rx.await.unwrap_or(false) {
                            video_ready_after_ms = Some(started.elapsed().as_millis());
                            break;
                        }
                        if started.elapsed() > Duration::from_secs(30) {
                            std::fs::write(&output, b"{\"error\":\"video did not start within 30 seconds\"}").unwrap();
                            cx.update(|cx| cx.quit());
                            return;
                        }
                        cx.background_executor().timer(Duration::from_millis(100)).await;
                    }
                }
                cx.background_executor().timer(Duration::from_secs(3)).await;
                let mut hidden_surface_presents = None;
                if url.query() == Some("resume") {
                    window_handle.update(cx, |_, window, cx| {
                        browser.update(cx, |browser, cx| browser.new_tab(window, cx));
                    }).unwrap();
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    crate::performance::reset();
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    hidden_surface_presents = Some(crate::performance::report()["surface_present"]["count"].as_u64().unwrap_or(0));
                    window_handle.update(cx, |_, window, cx| {
                        browser.update(cx, |browser, cx| browser.activate(0, window, cx));
                    }).unwrap();
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                }
                crate::performance::reset();
                if url.query() == Some("wheel") {
                    window_handle.update(cx, |_, window, _| wheel_frames(window, Instant::now())).unwrap();
                }
                let mut transition_tints = Vec::new();
                if url.query() == Some("video-transition") {
                    transition_tints.push(page_tint(&browser, cx));
                    cx.background_executor().timer(Duration::from_secs(6)).await;
                    transition_tints.push(page_tint(&browser, cx));
                    cx.background_executor().timer(Duration::from_secs(6)).await;
                } else {
                    cx.background_executor().timer(Duration::from_secs(12)).await;
                }
                let (tx, rx) = futures::channel::oneshot::channel();
                browser.update(cx, |browser, cx| {
                    browser.tabs[0]
                        .page
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .webview
                        .evaluate_javascript(
                            "JSON.stringify(window.perfResults())",
                            move |result| {
                                let _ = tx.send(match result {
                                    Ok(JSValue::String(value)) => {
                                        serde_json::from_str::<serde_json::Value>(&value).ok()
                                    }
                                    _ => None,
                                });
                            },
                        );
                });
                let fixture = rx.await.unwrap_or_default();
                let active = window_handle.update(cx, |_, window, _| window.is_window_active()).unwrap();
                let tint = page_tint(&browser, cx);
                std::fs::write(
                    &output,
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "native":crate::performance::report(),"fixture":fixture,"tint_rgb":tint,"window_active":active,"hidden_surface_presents":hidden_surface_presents,"transition_tints":transition_tints,"video_ready_after_ms":video_ready_after_ms
                    }))
                    .unwrap(),
                )
                .unwrap();
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    Ok(())
}

// Inject at the platform-event boundary so the production hit testing and wheel
// handler run. This measures native input routing, without OS input automation.
fn wheel_frames(window: &Window, start: Instant) {
    window.on_next_frame(move |window, cx| {
        if start.elapsed() >= Duration::from_secs(12) {
            return;
        }
        window.dispatch_event(
            gpui_kit::PlatformInput::ScrollWheel(gpui_kit::ScrollWheelEvent {
                position: point(px(600.), px(400.)),
                delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-8.))),
                ..Default::default()
            }),
            cx,
        );
        wheel_frames(window, start);
    });
}

fn page_tint(browser: &Entity<Browser>, cx: &gpui_kit::AsyncApp) -> Option<[f32; 3]> {
    browser.read_with(cx, |browser, cx| {
        browser.tabs[0].tint(cx).map(|tint| {
            let rgb = Rgba::from(tint);
            [rgb.r, rgb.g, rgb.b]
        })
    })
}
