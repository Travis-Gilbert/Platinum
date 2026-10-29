//! Native visual acceptance of production Home and SERP with labeled fixtures.
//! Enabled only by `snapshot`; uses temporary recents and no browsing profile.
use super::*;
use crate::search::{Degradation, SerpHit, SerpResult};
use gpui_kit::component::Root;
use gpui_kit::{WindowBounds, WindowOptions};
use std::path::PathBuf;
use std::time::Duration;

pub fn run(output: PathBuf) -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    std::fs::create_dir_all(&output)?;
    let profile = output.join("temporary-servo-profile");
    std::fs::create_dir_all(&profile)?;
    let cleanup = profile.clone();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            crate::install_commands(cx);
            Engine::start(&profile, cx);
            recents::init(None, cx);
            recents::record_search("native browser rendering", false, cx);
            recents::record_page(
                &Url::parse("https://example.test/article").unwrap(),
                "Example article",
                cx,
            );
            let mut view = None;
            let handle = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::centered(size(px(1280.), px(820.)), cx)),
                        ..TitleBar::window_options()
                    },
                    |window, cx| {
                        glass::install(window, cx);
                        let browser = cx.new(|cx| Browser::new(window, cx));
                        view = Some(browser.clone());
                        window.activate_window();
                        cx.new(|cx| {
                            Root::new(browser, window, cx).bg(gpui_kit::transparent_black())
                        })
                    },
                )
                .expect("workspace check window");
            // An erased handle permits drawing without borrowing the root view.
            let handle: gpui_kit::AnyWindowHandle = handle.into();
            let browser = view.unwrap();
            cx.activate(true);
            cx.spawn(async move |cx| {
                let mut captures = Vec::new();
                for (name, appearance, state) in [
                    ("home-dark", Appearance::Dark, 0),
                    ("home-light", Appearance::Light, 0),
                    ("tab-home-light", Appearance::Light, 1),
                    ("results-light", Appearance::Light, 2),
                    ("results-dark", Appearance::Dark, 2),
                    ("empty-dark", Appearance::Dark, 3),
                ] {
                    handle
                        .update(cx, |_, window, cx| {
                            theme::set_appearance(appearance, cx);
                            browser.update(cx, |browser, cx| match state {
                                0 => browser.reset_underlay(window, cx),
                                1 => browser.reveal_tab(window, cx),
                                _ => {
                                    browser.reset_underlay(window, cx);
                                    let result = fixture(state == 3);
                                    let serp = cx.new(|cx| Serp::loaded(result, window, cx));
                                    let events =
                                        cx.subscribe_in(&serp, window, Browser::serp_event);
                                    browser.underlay_go(
                                        underlay::Route::Results(serp, Rc::new(events)),
                                        window,
                                        cx,
                                    );
                                }
                            });
                            window.refresh();
                        })
                        .expect("update native workspace");
                    cx.background_executor()
                        .timer(Duration::from_millis(350))
                        .await;
                    let path = output.join(format!("{name}.png"));
                    handle
                        .update(cx, |_, window, cx| {
                            // As in canvas_chrome_check: capture a fresh scene even when the window is idle.
                            window.refresh();
                            window.draw(cx).clear(cx);
                            window.render_to_image()?.save(&path)?;
                            anyhow::Ok(())
                        })
                        .expect("render window")
                        .expect("save screenshot");
                    captures.push(name);
                }
                // Exercise the real native view transfer and Servo navigation.
                handle.update(cx, |_, window, cx| browser.update(cx, |browser, cx| {
                    browser.go(Url::parse("data:text/html,<title>Workspace navigation fixture</title><h1>Native workspace navigation</h1>").unwrap(), None, true, window, cx);
                })).unwrap();
                let mut loaded = false;
                for _ in 0..100 {
                    cx.background_executor().timer(Duration::from_millis(100)).await;
                    loaded = browser.read_with(cx, |browser, cx| {
                        let tab = &browser.tabs[browser.active];
                        !browser.underlay.shown && tab.showing == Showing::Page
                            && tab.page.as_ref().is_some_and(|page| !page.read(cx).is_loading() && page.read(cx).title() == "Workspace navigation fixture")
                    });
                    if loaded { break; }
                }
                assert!(loaded, "real Servo navigation completed");
                handle.update(cx, |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear(cx);
                    window.render_to_image().unwrap().save(output.join("servo-page.png")).unwrap();
                    browser.update(cx, |browser, cx| browser.back(window, cx));
                }).unwrap();
                assert!(browser.read_with(cx, |browser, _| browser.tabs[browser.active].showing == Showing::Serp), "Back restores native results");
                std::fs::write(
                    output.join("report.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "fixture": true, "native_renderer": true, "captures": captures,
                        "live_search": false, "servo_navigation": true, "back_to_native_results": true
                    }))
                    .unwrap(),
                )
                .unwrap();
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    std::fs::remove_dir_all(cleanup)?;
    Ok(())
}

fn fixture(empty: bool) -> SerpResult {
    SerpResult {
        query: "native browser rendering".into(),
        ask: false,
        providers: vec!["example provider".into()],
        hits: if empty {
            vec![]
        } else {
            vec![SerpHit {
                title: "Native rendering and accessible browser chrome".into(),
                url: Url::parse("https://example.test/article").unwrap(),
                host: "example.test".into(),
                snippet: crate::markup::parse(
                    "A <strong>native browser</strong> can keep its interface responsive while showing readable results and honest search coverage.",
                ),
                providers: vec![],
                lane: "warm".into(),
                published_at: Some("2026-10-28".into()),
            }]
        },
        degradations: vec![Degradation {
            code: "fixture".into(),
            message: "Example provider unavailable".into(),
        }],
        answer: None,
        answer_note: None,
        notes: vec!["Visual fixture: partial search coverage".into()],
        searched_at: chrono::Local::now(),
        elapsed_ms: 120,
    }
}
