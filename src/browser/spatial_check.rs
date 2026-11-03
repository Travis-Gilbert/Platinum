//! Opt-in native acceptance. An isolated profile, real Servo pages and real
//! editor entities; supplied transcripts exercise insertion, not microphone ASR.
use super::workspace_check::{capture, until_within};
use super::*;
use gpui_kit::component::Root;
use gpui_kit::{AsyncApp, WindowBounds, WindowOptions};
use std::{path::PathBuf, time::Duration};

pub fn run(output: PathBuf) -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    std::fs::create_dir_all(&output)?;
    let store = frame_profile::ProfileStore::open(&output.join("profile"))?;
    let profile = store.directory();
    let restore = std::env::var_os("FRAME_SPATIAL_RESTORE").is_some();
    let outcome = Rc::new(std::cell::RefCell::new(None));
    let result = outcome.clone();
    gpui_kit::application().with_assets(gpui_kit::assets::AllAssets).run(move |cx| {
        gpui_kit::init(cx); theme::init(cx); crate::install_commands(cx);
        Engine::start(&profile, cx); recents::init(None, cx); crate::theorem::init(None, cx);
        crate::session::init(&profile, cx); cx.set_global(ActiveProfile(store));
        let mut browser = None;
        let handle = cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1440.), px(960.)), cx)),
            ..TitleBar::window_options()
        }, |window, cx| {
            glass::install(window, cx);
            let view = cx.new(|cx| Browser::new(window, cx)); browser = Some(view.clone());
            window.activate_window(); cx.new(|cx| Root::new(view, window, cx))
        }).unwrap().into();
        let browser = browser.unwrap(); cx.activate(true);
        cx.spawn(async move |cx| {
            let checked = scenario(handle, browser, &output, restore, cx).await;
            let report = serde_json::json!({"workspace_passed":checked.is_ok(),"restore":restore,
                "error":checked.as_ref().err().map(|e|format!("{e:#}")),
                "voice_input":"supplied transcript through production destinations; ASR is a separate audio-fixture check"});
            let _ = std::fs::write(output.join(if restore {"restore-report.json"} else {"report.json"}), serde_json::to_vec_pretty(&report).unwrap());
            *result.borrow_mut() = Some(checked);
            cx.update(|cx| cx.quit());
        }).detach();
    });
    outcome
        .borrow_mut()
        .take()
        .unwrap_or_else(|| Err(anyhow::anyhow!("Native check did not finish")))
}

async fn js(view: &servo::WebView, script: &str, cx: &AsyncApp) -> anyhow::Result<servo::JSValue> {
    use futures::FutureExt as _;
    let (tx, rx) = futures::channel::oneshot::channel();
    view.evaluate_javascript(script, move |result| {
        let _ = tx.send(result);
    });
    let result = rx.fuse();
    let timeout = cx
        .background_executor()
        .timer(Duration::from_secs(5))
        .fuse();
    futures::pin_mut!(result, timeout);
    futures::select! { result = result => result?.map_err(|e| anyhow::anyhow!("JavaScript: {e:?}")), _ = timeout => anyhow::bail!("JavaScript timed out") }
}
async fn pause(cx: &AsyncApp) {
    cx.background_executor()
        .timer(Duration::from_millis(500))
        .await;
}
async fn scenario(
    handle: gpui_kit::AnyWindowHandle,
    browser: Entity<Browser>,
    output: &std::path::Path,
    restore: bool,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    if restore {
        if output.join("voice-shortcut.json").exists() {
            anyhow::ensure!(browser.read_with(cx, |b, _| b.speaker.preferences.dictation_hotkey == "ctrl-shift-space"), "edited speech shortcut did not restore");
        }
        handle.update(cx, |_, window, cx| {
            browser.update(cx, |b, cx| b.toggle_canvas(window, cx))
        })?;
        anyhow::ensure!(
            until_within(Duration::from_secs(20), cx, |cx| browser.read_with(
                cx,
                |b, cx| b
                    .canvas
                    .as_ref()
                    .is_some_and(|c| c.read(cx).state(cx).notes.len() == 1)
            ))
            .await,
            "restored note did not mount"
        );
        let state = browser.read_with(cx, |b, cx| b.canvas.as_ref().unwrap().read(cx).state(cx));
        anyhow::ensure!(
            state.cards.len() >= 2 && state.notes.len() == 1,
            "workspace did not restore"
        );
        let expected: crate::session::format::BoardState =
            serde_json::from_slice(&std::fs::read(output.join("saved-board.json"))?)?;
        anyhow::ensure!(state == expected, "restored geometry or camera changed");
        let restored_text = browser.read_with(cx, |b, cx| {
            b.notebook
                .as_ref()
                .unwrap()
                .read(cx)
                .note(&state.notes[0].path)
                .unwrap()
                .read(cx)
                .editor
                .read(cx)
                .text()
                .to_owned()
        });
        anyhow::ensure!(
            restored_text.contains("A local spoken note."),
            "restored note lost content"
        );
        capture(handle, Appearance::Light, &output.join("restored.png"), cx).await;
        return Ok(());
    }
    let url = Url::parse(
        &std::env::var("FRAME_SPATIAL_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8768/probe.html".into()),
    )?;
    if let Ok(public_url) = std::env::var("FRAME_SPATIAL_PUBLIC_URL") {
        let public_url = Url::parse(&public_url)?;
        handle.update(cx, |_, window, cx| {
            browser.update(cx, |b, cx| b.go(public_url.clone(), None, true, window, cx))
        })?;
        anyhow::ensure!(
            until_within(Duration::from_secs(30), cx, |cx| browser.read_with(
                cx,
                |b, cx| b.tabs[b.active]
                    .page
                    .as_ref()
                    .is_some_and(|p| !p.read(cx).is_loading()
                        && p.read(cx).title().contains("Example Domain"))
            ))
            .await,
            "public page did not load"
        );
        let public_view = browser.read_with(cx, |b, cx| {
            b.tabs[b.active]
                .page
                .as_ref()
                .unwrap()
                .read(cx)
                .webview
                .clone()
        });
        let text = js(&public_view, "document.body.textContent", cx).await?;
        anyhow::ensure!(
            matches!(text, servo::JSValue::String(ref s) if s.contains("documentation examples")),
            "public page did not return expected content: {text:?}"
        );
        capture(
            handle,
            Appearance::Light,
            &output.join("public-page.png"),
            cx,
        )
        .await;
    }
    handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| b.go(url.clone(), None, false, window, cx))
    })?;
    anyhow::ensure!(
        until_within(Duration::from_secs(20), cx, |cx| browser.read_with(
            cx,
            |b, cx| b.tabs[b.active]
                .page
                .as_ref()
                .is_some_and(|p| !p.read(cx).is_loading())
        ))
        .await,
        "first page did not load"
    );
    let parent = browser.read_with(cx, |b, _| b.tabs[b.active].id);
    let mut child = url;
    child.set_query(Some("child=1"));
    handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| {
            b.go(child, Some(Origin::From(parent)), true, window, cx)
        })
    })?;
    anyhow::ensure!(
        until_within(Duration::from_secs(20), cx, |cx| browser.read_with(
            cx,
            |b, cx| b.tabs[b.active]
                .page
                .as_ref()
                .is_some_and(|p| !p.read(cx).is_loading())
        ))
        .await,
        "child page did not load"
    );
    let view = browser.read_with(cx, |b, cx| {
        b.tabs[b.active]
            .page
            .as_ref()
            .unwrap()
            .read(cx)
            .webview
            .clone()
    });
    js(&view, "document.querySelector('textarea').focus(); document.querySelector('textarea').value='Before after'; document.querySelector('textarea').setSelectionRange(7,12); true", cx).await?;
    let (tx, rx) = futures::channel::oneshot::channel();
    view.native_text(servo::NativeTextRequest::Snapshot, move |result| {
        let _ = tx.send(result);
    });
    let context = crate::voice::native_result(rx, cx.background_executor())
        .await?
        .ok_or_else(|| anyhow::anyhow!("missing native editor snapshot"))?;
    let stale = context.clone();
    let target = handle.update(cx, |_, window, cx| {
        crate::voice::WebDestination::new(view.clone(), context, window, cx)
    })?;
    let mut target = crate::voice::Target::Web(target);
    handle.update(cx, |_, window, cx| {
        anyhow::ensure!(target.partial("spoken", window, cx), "partial refused");
        Ok::<_, anyhow::Error>(())
    })??;
    pause(cx).await;
    anyhow::ensure!(
        matches!(js(&view, "document.querySelector('textarea').value", cx).await?, servo::JSValue::String(s) if s == "Before spoken"),
        "web preedit did not replace selected range"
    );
    if let crate::voice::Target::Web(target) = target {
        target.finish("spoken words");
    }
    pause(cx).await;
    anyhow::ensure!(
        matches!(js(&view, "document.querySelector('textarea').value", cx).await?, servo::JSValue::String(s) if s == "Before spoken words"),
        "web final did not replace preedit"
    );
    let (tx, rx) = futures::channel::oneshot::channel();
    view.native_text(
        servo::NativeTextRequest::Edit {
            expected: stale,
            action: servo::NativeTextAction::Replace {
                range: None,
                text: "stale overwrite".into(),
            },
        },
        move |result| {
            let _ = tx.send(result);
        },
    );
    anyhow::ensure!(
        crate::voice::native_result(rx, cx.background_executor())
            .await
            .is_err(),
        "stale website edit was accepted"
    );
    rich_dictation(handle, &view, cx).await?;
    handle.update(cx, |_, window, cx| crate::theorem::open_models(window, cx))?;
    capture(
        handle,
        Appearance::Light,
        &output.join("model-settings.png"),
        cx,
    )
    .await;
    handle.update(cx, |_, window, cx| window.close_sheet(cx))?;
    voice_shortcut_check(handle, &browser, output, cx).await?;
    web_modifier_release_check(handle, &browser, &view, output, cx).await?;
    handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| {
            b.open_place(crate::places::Place::Crm, window, cx);
            b.crm
                .as_ref()
                .unwrap()
                .update(cx, |crm, cx| crm.show_review_fixture(window, cx));
        })
    })?;
    capture(
        handle,
        Appearance::Light,
        &output.join("index-proposal-fixture.png"),
        cx,
    )
    .await;
    handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| {
            b.leave_place(window, cx);
        })
    })?;
    handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| b.toggle_canvas(window, cx))
    })?;
    pause(cx).await;
    let board = browser.read_with(cx, |b, _| b.canvas.clone().unwrap());
    handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| b.activate_id(parent, window, cx))
    })?;
    anyhow::ensure!(
        browser.read_with(cx, |b, _| b.canvas.as_ref().unwrap().entity_id()
            == board.entity_id()),
        "tab activation replaced board"
    );
    handle.update(cx, |_, _, cx| board.update(cx, |b, cx| b.overview(cx)))?;
    capture(
        handle,
        Appearance::Light,
        &output.join("board-light.png"),
        cx,
    )
    .await;
    capture(handle, Appearance::Dark, &output.join("board-dark.png"), cx).await;
    handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| {
            b.ensure_notes(window, cx);
            b.notes_sidebar = true;
            b.notebook.as_ref().unwrap().update(cx, |n, cx| {
                n.create(Some("http://127.0.0.1:8768/probe.html".into()), window, cx)
            });
            cx.notify();
        })
    })?;
    anyhow::ensure!(
        until_within(Duration::from_secs(30), cx, |cx| browser.read_with(
            cx,
            |b, cx| b
                .notebook
                .as_ref()
                .and_then(|n| n.read(cx).selected())
                .is_some()
        ))
        .await,
        "note did not open"
    );
    let note = browser.read_with(cx, |b, cx| {
        b.notebook.as_ref().unwrap().read(cx).selected().unwrap()
    });
    let editor = note.read_with(cx, |n, _| n.editor.clone());
    handle.update(cx, |_, window, cx| {
        window.activate_window();
        cx.activate(true);
        window.focus(&editor.focus_handle(cx), cx);
    })?;
    pause(cx).await;
    let native_active = handle.update(cx, |_, window, _| window.is_window_active())?;
    std::fs::write(
        output.join("native-focus.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"dictation_window_active":native_active,"blocker":(!native_active).then_some("AppKit did not make this window active; native focused dictation was not executed. Editor persistence uses an explicit editor edit instead.")}),
        )?,
    )?;
    if native_active {
        handle.update(cx, |_, window, cx| {
            let mut destination = crate::voice::Destination::capture(window, cx)
                .ok_or_else(|| anyhow::anyhow!("note input not mounted"))?;
            anyhow::ensure!(
                destination.apply("A local spoken note.", false, window, cx),
                "note preedit failed"
            );
            anyhow::ensure!(
                destination.apply("A local spoken note.", true, window, cx),
                "note commit failed"
            );
            Ok::<_, anyhow::Error>(())
        })??;
    } else {
        editor.update(cx, |editor, cx| {
            let cursor = editor.cursor();
            editor.replace_range(cursor..cursor, "A local spoken note.", cx)
        });
    }
    capture(
        handle,
        Appearance::Light,
        &output.join("sidebar-note.png"),
        cx,
    )
    .await;
    handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| {
            b.open_place(crate::places::Place::Notes, window, cx);
            b.notebook.as_ref().unwrap().update(cx, |_, cx| {
                cx.emit(crate::notes::NotesEvent::Promote(note.clone()))
            })
        })
    })?;
    pause(cx).await;
    anyhow::ensure!(
        note.read_with(cx, |n, _| n.editor.entity_id()) == editor.entity_id(),
        "promotion duplicated editor"
    );
    anyhow::ensure!(
        browser.read_with(cx, |b, cx| b.place.is_none()
            && b.theorem.as_ref().is_some_and(|(workbench, _)| workbench
                .read(cx)
                .board_slot()
                .is_some())),
        "note promotion did not restore the compact board composer"
    );
    let note_path = note.read_with(cx, |n, _| n.path.clone());
    let profile = cx.update(|cx| cx.global::<ActiveProfile>().0.directory());
    anyhow::ensure!(
        until_within(Duration::from_secs(15), cx, |_| std::fs::read_to_string(
            profile.join("notes").join(&note_path)
        )
        .is_ok_and(|s| s.contains("A local spoken note.")))
        .await,
        "note was not saved"
    );
    capture(
        handle,
        Appearance::Light,
        &output.join("board-note.png"),
        cx,
    )
    .await;
    handle.update(cx, |_, _, cx| browser.update(cx, |b, cx| b.save(cx)))?;
    let saved = cx.update(|cx| Store::global(cx).unwrap().load_session().unwrap());
    anyhow::ensure!(
        saved.board.notes.len() == 1 && saved.board.cards.len() == 2,
        "workspace save lost cards"
    );
    std::fs::write(
        output.join("saved-board.json"),
        serde_json::to_vec_pretty(&saved.board)?,
    )?;
    Ok(())
}

async fn rich_dictation(
    handle: gpui_kit::AnyWindowHandle,
    view: &servo::WebView,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    js(view, "(()=>{const e=document.createElement('div');e.id='dictation-rich';e.contentEditable='true';e.innerHTML='Before <b>after</b>';document.body.appendChild(e);e.focus();const r=document.createRange();r.selectNodeContents(e.lastChild);getSelection().removeAllRanges();getSelection().addRange(r);return true})()", cx).await?;
    let (tx, rx) = futures::channel::oneshot::channel();
    view.native_text(servo::NativeTextRequest::Snapshot, move |result| {
        let _ = tx.send(result);
    });
    let context = crate::voice::native_result(rx, cx.background_executor())
        .await?
        .ok_or_else(|| anyhow::anyhow!("missing rich editor snapshot"))?;
    anyhow::ensure!(
        context.text == "Before after" && context.selection == (7, 12),
        "rich selection was not projected correctly: {context:?}"
    );
    let stale = context.clone();
    let target = handle.update(cx, |_, window, cx| {
        crate::voice::WebDestination::new(view.clone(), context, window, cx)
    })?;
    let mut target = crate::voice::Target::Web(target);
    handle.update(cx, |_, window, cx| {
        anyhow::ensure!(target.partial("spoken", window, cx), "rich partial refused");
        Ok::<_, anyhow::Error>(())
    })??;
    pause(cx).await;
    anyhow::ensure!(
        matches!(js(view, "document.getElementById('dictation-rich').textContent", cx).await?, servo::JSValue::String(s) if s == "Before spoken"),
        "rich preedit failed"
    );
    if let crate::voice::Target::Web(target) = target {
        target.finish("spoken words");
    }
    pause(cx).await;
    anyhow::ensure!(
        matches!(js(view, "document.getElementById('dictation-rich').textContent", cx).await?, servo::JSValue::String(s) if s == "Before spoken words"),
        "rich final duplicated or lost the composition"
    );
    let (tx, rx) = futures::channel::oneshot::channel();
    view.native_text(
        servo::NativeTextRequest::Edit {
            expected: stale,
            action: servo::NativeTextAction::Replace {
                range: None,
                text: "stale".into(),
            },
        },
        move |result| {
            let _ = tx.send(result);
        },
    );
    anyhow::ensure!(
        crate::voice::native_result(rx, cx.background_executor())
            .await
            .is_err(),
        "rich stale overwrite accepted"
    );
    // A recognizer can finish without publishing any interim words.
    let (tx, rx) = futures::channel::oneshot::channel();
    view.native_text(servo::NativeTextRequest::Snapshot, move |result| {
        let _ = tx.send(result);
    });
    let context = crate::voice::native_result(rx, cx.background_executor())
        .await?
        .unwrap();
    let target = handle.update(cx, |_, window, cx| {
        crate::voice::WebDestination::new(view.clone(), context, window, cx)
    })?;
    target.finish(" again");
    pause(cx).await;
    anyhow::ensure!(
        matches!(js(view, "document.getElementById('dictation-rich').textContent", cx).await?, servo::JSValue::String(s) if s == "Before spoken words again"),
        "one-shot rich final failed"
    );

    let (tx, rx) = futures::channel::oneshot::channel();
    view.native_text(servo::NativeTextRequest::Snapshot, move |result| {
        let _ = tx.send(result);
    });
    let context = crate::voice::native_result(rx, cx.background_executor())
        .await?
        .unwrap();
    let target = handle.update(cx, |_, window, cx| {
        crate::voice::WebDestination::new(view.clone(), context, window, cx)
    })?;
    js(view, "document.querySelector('input').focus(); true", cx).await?;
    target.finish("must not move to another field");
    pause(cx).await;
    anyhow::ensure!(
        matches!(js(view, "document.querySelector('input').value", cx).await?, servo::JSValue::String(s) if s == "Unchanged"),
        "late dictation moved to a newly focused field"
    );
    Ok(())
}

async fn voice_shortcut_check(
    handle: gpui_kit::AnyWindowHandle,
    browser: &Entity<Browser>,
    output: &std::path::Path,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    use gpui_kit::component::speech::SpeechState;
    use gpui_kit::{InputEvent as _, KeyDownEvent, KeyUpEvent, Keystroke};
    let input = crate::voice::fixture::Input::default();
    handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| b.voice_settings(window, cx))
    })?;
    capture(
        handle,
        Appearance::Light,
        &output.join("voice-settings.png"),
        cx,
    )
    .await;
    let original = handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| {
            b.set_voice_shortcut("ctrl-shift-space", window, cx);
            b.focus_address_bar(window, cx);
            b.omnibox
                .update(cx, |field, cx| field.set_value("", window, cx));
            window.activate_window();
            cx.activate(true);
            let speech = cx.new(|cx| {
                SpeechState::new(cx)
                    .recognizer(crate::voice::fixture::Recognizer)
                    .input(input.clone())
                    .system_fallback(false)
            });
            let subscription = cx.subscribe_in(&speech, window, |b, _, event, window, cx| {
                b.speech_event(event, window, cx)
            });
            (
                std::mem::replace(&mut b.speech, speech),
                std::mem::replace(&mut b._speech_events, subscription),
            )
        })
    })?;
    pause(cx).await;
    anyhow::ensure!(
        handle.update(cx, |_, window, _| window.is_window_active())?,
        "native hotkey test window did not activate"
    );
    let key = Keystroke::parse("ctrl-shift-space")?;
    handle.update(cx, |_, window, cx| {
        window.dispatch_event(
            KeyDownEvent {
                keystroke: key.clone(),
                is_held: false,
                prefer_character_input: false,
            }
            .to_platform_input(),
            cx,
        );
    })?;
    pause(cx).await;
    anyhow::ensure!(
        input.starts.get() == 1,
        "held shortcut did not start capture"
    );
    handle.update(cx, |_, window, cx| {
        window.dispatch_event(
            KeyDownEvent {
                keystroke: key.clone(),
                is_held: true,
                prefer_character_input: false,
            }
            .to_platform_input(),
            cx,
        );
    })?;
    anyhow::ensure!(input.starts.get() == 1, "key repeat restarted capture");
    capture(
        handle,
        Appearance::Light,
        &output.join("voice-hold.png"),
        cx,
    )
    .await;
    handle.update(cx, |_, window, cx| {
        window.dispatch_event(KeyUpEvent { keystroke: key }.to_platform_input(), cx);
    })?;
    pause(cx).await;
    anyhow::ensure!(input.stops.get() == 1, "release did not stop capture");
    anyhow::ensure!(
        browser.read_with(cx, |b, cx| b.omnibox.read(cx).value() == "spoken words"),
        "released shortcut did not commit exactly one transcript"
    );
    handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| {
            b.speech = original.0;
            b._speech_events = original.1;
            b.voice_settings(window, cx);
            b.save(cx);
        })
    })?;
    capture(
        handle,
        Appearance::Light,
        &output.join("voice-settings-edited.png"),
        cx,
    )
    .await;
    handle.update(cx, |_, window, cx| window.close_sheet(cx))?;
    std::fs::write(
        output.join("voice-shortcut.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "passed": true, "starts": input.starts.get(), "stops": input.stops.get(),
            "hotkey": "ctrl-shift-space", "input": "synthetic capture and transcript through native window key dispatch; no live microphone or physical key injection"
        }))?,
    )?;
    Ok(())
}

/// A real focused Page consumes GPUI's synthetic modifier keystroke. Verify the
/// release is handled before that consumer, with a labeled synthetic microphone.
async fn web_modifier_release_check(
    handle: gpui_kit::AnyWindowHandle,
    browser: &Entity<Browser>,
    view: &servo::WebView,
    output: &std::path::Path,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    use gpui_kit::component::speech::SpeechState;
    use gpui_kit::{InputEvent as _, ModifiersChangedEvent};
    let input = crate::voice::fixture::Input::default();
    let original = handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| {
            b.set_voice_shortcut("left-cmd", window, cx);
            let page = b.tabs[b.active].page.as_ref().unwrap();
            window.focus(&page.focus_handle(cx), cx);
            let speech = cx.new(|cx| {
                SpeechState::new(cx)
                    .recognizer(crate::voice::fixture::Recognizer)
                    .input(input.clone())
                    .system_fallback(false)
            });
            let subscription = cx.subscribe_in(&speech, window, |b, _, event, window, cx| {
                b.speech_event(event, window, cx)
            });
            (
                std::mem::replace(&mut b.speech, speech),
                std::mem::replace(&mut b._speech_events, subscription),
            )
        })
    })?;
    js(view, "(() => { const e = document.querySelector('textarea'); e.value = ''; e.focus(); e.setSelectionRange(0, 0); return true; })()", cx).await?;
    for quick in [true, false] {
        handle.update(cx, |_, window, cx| {
            let modifiers = gpui_kit::Modifiers {
                platform: true,
                ..Default::default()
            };
            window.dispatch_event(
                ModifiersChangedEvent {
                    modifiers,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            // Headless/native synthetic events cannot press a physical left key.
            // Supply only the side; release goes through unmodified GPUI dispatch.
            browser.update(cx, |b, cx| {
                b.voice_modifiers(modifiers, true, false, window, cx)
            });
        })?;
        if !quick {
            anyhow::ensure!(
                until_within(Duration::from_secs(5), cx, |_| input.starts.get() == 1).await,
                "focused website hold did not start"
            );
        }
        handle.update(cx, |_, window, cx| {
            anyhow::ensure!(
                browser.read(cx).tabs[browser.read(cx).active]
                    .page
                    .as_ref()
                    .unwrap()
                    .focus_handle(cx)
                    .is_focused(window),
                "release must target the production Page"
            );
            window.dispatch_event(ModifiersChangedEvent::default().to_platform_input(), cx);
            Ok::<_, anyhow::Error>(())
        })??;
        pause(cx).await;
        if quick {
            anyhow::ensure!(
                input.starts.get() == 0,
                "a website Command tap started delayed capture"
            );
        }
    }
    anyhow::ensure!(
        input.stops.get() == 1,
        "website release did not stop exactly once"
    );
    anyhow::ensure!(
        matches!(js(view, "document.querySelector('textarea').value", cx).await?, servo::JSValue::String(s) if s == "spoken words"),
        "website release did not commit the transcript"
    );
    capture(
        handle,
        Appearance::Light,
        &output.join("web-modifier-release.png"),
        cx,
    )
    .await;
    handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| {
            b.speech = original.0;
            b._speech_events = original.1;
            b.set_voice_shortcut("ctrl-shift-space", window, cx);
            b.save(cx);
        })
    })?;
    std::fs::write(
        output.join("web-modifier-release.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "passed": true, "starts": input.starts.get(), "stops": input.stops.get(),
            "page_focused_on_release": true, "quick_tap_did_not_start": true,
            "input": "supplied left-key state and synthetic capture; real Page, modifier-release dispatch and web insertion"
        }))?,
    )?;
    Ok(())
}
