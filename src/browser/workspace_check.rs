//! Native visual acceptance of production Home, SERP and the Agent place with
//! labeled fixtures. Enabled only by `snapshot`; uses temporary recents, no
//! browsing profile, and Theorem's deterministic demo for anything sent.
use super::*;
use crate::search::{Degradation, SerpHit, SerpResult};
use gpui_kit::component::Root;
use gpui_kit::{AsyncApp, WindowBounds, WindowOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;
use theorem_surface_contracts::intent::ComposerIntent;
use theoremweb_agent_runtime::ChatHostPhase;

pub fn run(output: PathBuf) -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    std::fs::create_dir_all(&output)?;
    let profile = output.join("temporary-servo-profile");
    std::fs::create_dir_all(&profile)?;
    let cleanup = profile.clone();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            crate::install_commands(cx);
            Engine::start(&profile, cx);
            recents::init(None, cx);
            crate::theorem::init(None, cx);
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
                handle.update(cx, |_, window, cx| {
                    let field = browser.update(cx, |browser, cx| {
                        browser.reset_underlay(window, cx);
                        let serp = cx.new(|cx| Serp::loaded(fixture(false), window, cx));
                        let events = cx.subscribe_in(&serp, window, Browser::serp_event);
                        browser.underlay_go(underlay::Route::Results(serp, Rc::new(events)), window, cx);
                        browser.underlay.field.clone()
                    });
                    field.update(cx, |field, cx| {
                        field.set_value("data:text/html,<title>Shortcut</title><h1>Shortcut</h1>", window, cx);
                        cx.emit(InputEvent::PressEnter { secondary: true, shift: false });
                    });
                }).unwrap();
                cx.background_executor().timer(Duration::from_millis(150)).await;
                assert!(browser.read_with(cx, |browser, _| {
                    browser.tabs.len() == 2 && browser.active == 1
                        && browser.tabs[0].showing == Showing::Serp
                        && browser.tabs[1].showing == Showing::Page
                }), "Command-Return address preserves results and opens a separate tab");

                // Theorem's workbench in the Agent place, opened from the page
                // tab: before any authority, then on the demo with a
                // conversation open and a message sent; last, in the
                // workspace once every tab has closed.
                handle.update(cx, |_, window, cx| browser.update(cx, |browser, cx| {
                    browser.open_place(Place::Agent, window, cx);
                })).unwrap();
                let driver = browser.read_with(cx, |browser, cx| {
                    assert_eq!(browser.tabs[browser.active].showing, Showing::Place(Place::Agent));
                    assert_eq!(browser.theorem_pane(), Some(browser.active), "the active tab mounts the workbench");
                    let (workbench, _) = browser.theorem.as_ref().expect("the Agent place makes the workbench");
                    workbench.read(cx).driver().clone()
                });
                let connection = driver.read_with(cx, |driver, _| format!("{:?}", driver.connection()));
                for (name, appearance) in [("agent-start-light", Appearance::Light), ("agent-start-dark", Appearance::Dark)] {
                    capture(handle, appearance, &output.join(format!("{name}.png")), cx).await;
                    captures.push(name);
                }

                driver.update(cx, |driver, cx| driver.use_demo(cx));
                assert!(until(cx, |cx| driver.read_with(cx, |driver, _| {
                    let projection = driver.projection();
                    projection.phase == ChatHostPhase::Ready && !projection.threads.is_empty()
                })).await, "the demo hydrates its projects and conversations");
                driver.update(cx, |driver, cx| driver.select_thread("demo-thread-controller", cx));
                assert!(until(cx, |cx| driver.read_with(cx, |driver, _| {
                    let thread = &driver.projection().thread;
                    thread.thread_id == "demo-thread-controller" && !thread.messages.is_empty()
                })).await, "the conversation opens with its transcript");
                for (name, appearance) in [("agent-demo-light", Appearance::Light), ("agent-demo-dark", Appearance::Dark)] {
                    capture(handle, appearance, &output.join(format!("{name}.png")), cx).await;
                    captures.push(name);
                }

                let text = "Where does the controller hand a run to the transport?";
                let before = driver.update(cx, |driver, cx| {
                    let projection = driver.projection();
                    assert!(projection.composer.affordance(text).is_send(), "the composer can send");
                    let (before, quote) = (projection.thread.messages.len(), projection.composer.quote.clone());
                    driver.composer_intent(ComposerIntent::Submit { text: text.into(), quote, steer: false, context: Vec::new() }, cx);
                    before
                });
                assert!(until(cx, |cx| driver.read_with(cx, |driver, _| {
                    let projection = driver.projection();
                    projection.thread.messages.len() >= before + 2 && !projection.composer.can_cancel()
                })).await, "the demo answers the message sent");
                for (name, appearance) in [("agent-sent-light", Appearance::Light), ("agent-sent-dark", Appearance::Dark)] {
                    capture(handle, appearance, &output.join(format!("{name}.png")), cx).await;
                    captures.push(name);
                }

                handle.update(cx, |_, window, cx| browser.update(cx, |browser, cx| {
                    for _ in 0..browser.tabs.len() {
                        if browser.underlay.shown {
                            break;
                        }
                        browser.close_tab(0, window, cx);
                    }
                    assert!(browser.underlay.shown, "with every tab closed, the workspace shows");
                    browser.open_place(Place::Agent, window, cx);
                    assert!(matches!(browser.underlay.route, underlay::Route::Place(Place::Agent)));
                })).unwrap();
                for (name, appearance) in [("agent-workspace-light", Appearance::Light), ("agent-workspace-dark", Appearance::Dark)] {
                    capture(handle, appearance, &output.join(format!("{name}.png")), cx).await;
                    captures.push(name);
                }

                std::fs::write(
                    output.join("report.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "fixture": true, "native_renderer": true, "captures": captures,
                        "live_search": false, "servo_navigation": true, "back_to_native_results": true,
                        "command_return_address_preserves_results": true,
                        "agent_place": { "connection_at_start": connection, "demo_hydrated": true,
                            "conversation_opened": true, "message_answered": true, "workspace_mounted": true }
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

/// Draw the window in `appearance` once it has settled, and save it.
async fn capture(
    handle: gpui_kit::AnyWindowHandle,
    appearance: Appearance,
    path: &Path,
    cx: &mut AsyncApp,
) {
    handle
        .update(cx, |_, window, cx| {
            theme::set_appearance(appearance, cx);
            window.refresh();
        })
        .expect("set the appearance");
    cx.background_executor()
        .timer(Duration::from_millis(350))
        .await;
    handle
        .update(cx, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            window.render_to_image()?.save(path)?;
            anyhow::Ok(())
        })
        .expect("render window")
        .expect("save screenshot");
}

/// Whether `ready` holds within five seconds.
async fn until(cx: &mut AsyncApp, mut ready: impl FnMut(&mut AsyncApp) -> bool) -> bool {
    for _ in 0..50 {
        if ready(cx) {
            return true;
        }
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
    ready(cx)
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
