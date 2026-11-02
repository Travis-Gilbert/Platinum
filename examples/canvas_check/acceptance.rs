use super::*;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{Focusable as _, KeyDownEvent, KeyUpEvent};

fn key(window: NativeWindow, name: &str, down: bool, cx: &mut AsyncApp) -> anyhow::Result<()> {
    let stroke = Keystroke {
        key: name.into(),
        key_char: if name == "space" {
            Some(" ".into())
        } else {
            None
        },
        modifiers: Modifiers::default(),
    };
    window.update(cx, |_, w, cx| {
        w.dispatch_event(
            if down {
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: stroke,
                    is_held: false,
                    prefer_character_input: false,
                })
            } else {
                PlatformInput::KeyUp(KeyUpEvent { keystroke: stroke })
            },
            cx,
        );
    })?;
    Ok(())
}
async fn tap(window: NativeWindow, name: &str, cx: &mut AsyncApp) -> anyhow::Result<()> {
    key(window, name, true, cx)?;
    key(window, name, false, cx)?;
    pause(cx, 120).await;
    Ok(())
}
async fn drag(
    window: NativeWindow,
    start: Point<Pixels>,
    end: Point<Pixels>,
    modifiers: Modifiers,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    pointer(window, start, None, cx)?;
    window.update(cx, |_, w, cx| {
        w.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                position: start,
                button: MouseButton::Left,
                click_count: 1,
                modifiers,
                ..Default::default()
            }),
            cx,
        );
    })?;
    pause(cx, 50).await;
    for i in 1..=5 {
        let position = start + (end - start) * (i as f32 / 5.);
        window.update(cx, |_, w, cx| {
            w.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    pressed_button: Some(MouseButton::Left),
                    modifiers,
                }),
                cx,
            );
        })?;
        pause(cx, 20).await;
    }
    window.update(cx, |_, w, cx| {
        w.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                position: end,
                button: MouseButton::Left,
                click_count: 1,
                modifiers,
                ..Default::default()
            }),
            cx,
        );
    })?;
    pause(cx, 220).await;
    Ok(())
}
fn state(board: &Entity<canvas::SpatialCanvas>, cx: &AsyncApp) -> Value {
    board.read_with(cx, |b, _| b.probe_state())
}
fn shot(window: NativeWindow, path: PathBuf, cx: &mut AsyncApp) -> anyhow::Result<()> {
    window.update(cx, |_, w, _| {
        w.render_to_image()?.save(path).map_err(anyhow::Error::from)
    })??;
    Ok(())
}
fn focused(
    window: NativeWindow,
    page: &Entity<page::Page>,
    cx: &mut AsyncApp,
) -> anyhow::Result<bool> {
    Ok(window.update(cx, |_, w, cx| page.read(cx).focus_handle(cx).is_focused(w))?)
}

pub async fn check(
    dir: &std::path::Path,
    view: &WebView,
    page: &Entity<page::Page>,
    board: &Entity<canvas::SpatialCanvas>,
    window: NativeWindow,
    cx: &mut AsyncApp,
) -> anyhow::Result<bool> {
    let mut results = Vec::new();
    for zoom in [0.35, 1., 2.] {
        board.update(cx, |b, cx| b.zoom_to(zoom, cx));
        pause(cx, 650).await;
        evaluate(view, "(() => { canvasInput.drag=null; document.querySelector('#scroller').scrollTop=0; document.querySelector('#choice').selectedIndex=0; getSelection().removeAllRanges(); return true; })()", cx).await?;
        pause(cx, 100).await;
        board.update(cx, |b, cx| b.pan_by([-5000., -5000.], cx));
        pause(cx, 250).await;
        pointer(window, point(px(10.), px(10.)), None, cx)?;
        pause(cx, 100).await;
        let off_card = evaluate(
            view,
            "document.querySelector('#target').matches(':hover')",
            cx,
        )
        .await?;
        let outside = target(view, page, board, "#session", cx).await?;
        pointer(window, outside, None, cx)?;
        pause(cx, 100).await;
        let before = evaluate(view, "window.canvasInput", cx).await?;
        let pos = target(view, page, board, "#target", cx).await?;
        click(window, pos, cx)?;
        pause(cx, 150).await;
        let start = target(view, page, board, "#drag", cx).await?;
        drag(
            window,
            start,
            start + point(px(25.), px(15.)),
            Modifiers::default(),
            cx,
        )
        .await?;
        let pos = target(view, page, board, "#scroller", cx).await?;
        let camera = state(board, cx)["camera"].clone();
        window.update(cx, |_, w, cx| {
            w.dispatch_event(
                PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position: pos,
                    delta: ScrollDelta::Pixels(point(px(0.), px(-40.))),
                    ..Default::default()
                }),
                cx,
            );
        })?;
        pause(cx, 250).await;
        let after = evaluate(view, "window.canvasInput", cx).await?;
        results.push(json!({"check":"pointer and wheel","zoom":zoom,"before":before,"after":after,
            "off_card_hover":off_card,"passed":off_card==false && after["clicks"].as_u64()>before["clicks"].as_u64() && after["hovers"].as_u64()>before["hovers"].as_u64() && !after["drag"]["end"].is_null() && after["drag"]["end"]!=after["drag"]["start"] && after["wheel"].as_f64().unwrap_or(0.)>before["wheel"].as_f64().unwrap_or(0.) && state(board,cx)["camera"]==camera}));
        let pos = target(view, page, board, "#selection", cx).await?;
        drag(
            window,
            pos - point(px(130. * zoom as f32), px(0.)),
            pos + point(px(100. * zoom as f32), px(0.)),
            Modifiers::default(),
            cx,
        )
        .await?;
        let selection = evaluate(view, "String(getSelection())", cx).await?;
        results.push(json!({"check":"text selection","zoom":zoom,"selected":selection,"passed":selection.as_str().is_some_and(|s|!s.is_empty())}));
        for selector in ["#choice", "#selection"] {
            let pos = target(view, page, board, selector, cx).await?;
            if selector == "#choice" {
                click(window, pos, cx)?;
            } else {
                window.update(cx, |_, w, cx| {
                    w.dispatch_event(
                        PlatformInput::MouseDown(MouseDownEvent {
                            position: pos,
                            button: MouseButton::Right,
                            click_count: 1,
                            ..Default::default()
                        }),
                        cx,
                    );
                    w.dispatch_event(
                        PlatformInput::MouseUp(MouseUpEvent {
                            position: pos,
                            button: MouseButton::Right,
                            click_count: 1,
                            ..Default::default()
                        }),
                        cx,
                    );
                })?;
            }
            pause(cx, 250).await;
            let open = page.read_with(cx, |p, _| p.controls.popup.is_some());
            if selector == "#choice" {
                let rect = evaluate(view, "(() => {let r=document.querySelector('#choice').getBoundingClientRect();return [r.left,r.bottom]})()", cx).await?;
                let (expected, actual) = page.read_with(cx, |p, _| {
                    let t = p.transform().expect("placed page");
                    let expected = [
                        f32::from(t.bounds.origin.x) + rect[0].as_f64().unwrap() as f32 * t.zoom,
                        f32::from(t.bounds.origin.y) + rect[1].as_f64().unwrap() as f32 * t.zoom,
                    ];
                    (
                        expected,
                        p.controls
                            .popup_bounds
                            .map(|b| [f32::from(b.origin.x), f32::from(b.origin.y)]),
                    )
                });
                let passed = actual.is_some_and(|a| {
                    (a[0] - expected[0]).abs() <= 2. && (a[1] - expected[1]).abs() <= 2.
                });
                results.push(json!({"check":"select anchor geometry","zoom":zoom,"expected_points":expected,"actual_points":actual,"tolerance_points":2,"passed":passed}));
            }

            shot(
                window,
                dir.join(format!("menu-{zoom}-{selector}-open.png")),
                cx,
            )?;
            board.update(cx, |b, cx| b.pan_by([30., 20.], cx));
            pause(cx, 250).await;
            let closed = page.read_with(cx, |p, _| p.controls.popup.is_none());
            shot(
                window,
                dir.join(format!("menu-{zoom}-{selector}-closed.png")),
                cx,
            )?;
            results.push(json!({"check":"visible popup closes on camera motion","zoom":zoom,"selector":selector,"opened":open,"closed":closed,"passed":open&&closed}));
        }
        for kind in ["color", "file"] {
            let pos = target(view, page, board, &format!("#{kind}"), cx).await?;
            click(window, pos, cx)?;
            pause(cx, 500).await;
            let opened = page.read_with(cx, |p, _| p.native_picker_visible(kind));
            let focused = evaluate(view, "document.activeElement.id", cx).await?;
            board.update(cx, |b, cx| b.pan_by([20., 10.], cx));
            pause(cx, 400).await;
            let closed = !page.read_with(cx, |p, _| p.native_picker_visible(kind));
            let refocused = window.update(cx, |_, w, _| w.is_window_active())?;
            results.push(json!({"check":"native picker shown and camera-dismissed", "zoom":zoom,
                "kind":kind,"opened":opened,"focused":focused,"closed":closed,"refocused":refocused,
                "passed":opened && focused == kind && closed && refocused,
                "observation":"AppKit panel isVisible; render_to_image cannot capture system panels"}));
        }
        let pos = target(view, page, board, "#choice", cx).await?;
        click(window, pos, cx)?;
        pause(cx, 200).await;
        tap(window, "down", cx).await?;
        tap(window, "down", cx).await?;
        tap(window, "enter", cx).await?;
        let choice = evaluate(view, "document.querySelector('#choice').value", cx).await?;
        results.push(json!({"check":"select option through keyboard","zoom":zoom,"choice":choice,"passed":choice=="Second"}));
        for kind in ["alert", "confirm", "prompt"] {
            let pos = target(view, page, board, &format!("#{kind}"), cx).await?;
            click(window, pos, cx)?;
            board.update(cx, |b, cx| b.pan_by([-2500., -1800.], cx));
            pause(cx, 650).await;
            let open = window.update(cx, |_, w, cx| w.has_active_dialog(cx))?;
            let card_visible = page.read_with(cx, |p, _| {
                p.bounds
                    .is_some_and(|b| f32::from(b.origin.x) >= 0. && f32::from(b.origin.y) >= 0.)
            });
            shot(window, dir.join(format!("dialog-{zoom}-{kind}.png")), cx)?;
            tap(window, "enter", cx).await?;
            pause(cx, 300).await;
            let closed = window.update(cx, |_, w, cx| !w.has_active_dialog(cx))?;
            if !closed {
                tap(window, "escape", cx).await?;
            }
            let response = evaluate(view, "window.canvasInput.dialogs", cx).await?;
            let answered = response.as_array().and_then(|r| r.last()).is_some_and(|r| {
                r["kind"] == kind
                    && match kind {
                        "confirm" => r["response"] == true,
                        "prompt" => r["response"].is_string(),
                        _ => true,
                    }
            });
            results.push(json!({"check":"modal reveal and response","zoom":zoom,"kind":kind,"opened":open,"closed":closed,"card_visible":card_visible,"focused_after":focused(window,page,cx)?,"responses":response,"passed":open&&closed&&card_visible&&answered&&focused(window,page,cx)?}));
            board.update(cx, |b, cx| b.zoom_to(zoom, cx));
            pause(cx, 250).await;
        }
        let pos = target(view, page, board, "#target", cx).await?;
        click(window, pos, cx)?;
        pause(cx, 100).await;
        let before = state(board, cx);
        key(window, "space", true, cx)?;
        pause(cx, 100).await;
        drag(
            window,
            pos,
            pos + point(px(75.), px(35.)),
            Modifiers::default(),
            cx,
        )
        .await?;
        key(window, "space", false, cx)?;
        results.push(json!({"check":"Space drag pans noneditable","zoom":zoom,"before":before,"after":state(board,cx),"passed":before["camera"]!=state(board,cx)["camera"]}));
        let pos = target(view, page, board, "#target", cx).await?;
        click(window, pos, cx)?;
        pause(cx, 100).await;
        let space = evaluate(view, "window.canvasInput.space", cx).await?;
        tap(window, "space", cx).await?;
        let next = evaluate(view, "window.canvasInput.space", cx).await?;
        results.push(json!({"check":"Space without drag delivered","zoom":zoom,"passed":next.as_u64()==space.as_u64().map(|n|n+1)}));
        let caption = target(view, page, board, "#target", cx).await?;
        let _ = caption;
        board.update(cx, |b, cx| b.reveal_page_point([200., -16.], cx));
        pause(cx, 250).await;
        let pos = page.read_with(cx, |p, _| {
            p.transform()
                .unwrap()
                .window_point(200. * p.scale, -16. * p.scale)
        });
        let before = state(board, cx);
        drag(
            window,
            pos,
            pos + point(px(50.), px(30.)),
            Modifiers::default(),
            cx,
        )
        .await?;
        let after = state(board, cx);
        results.push(json!({"check":"caption drag","zoom":zoom,"before":before,"after":after,"passed":before["card"]!=after["card"] && before["camera"]==after["camera"]}));
        record(dir, "input.json", &json!({"checks":results}))?;
    }
    board.update(cx, |b, cx| b.zoom_to(1., cx));
    pause(cx, 250).await;
    for selector in [
        "#draft",
        "#edit-text",
        "#edit-search",
        "#edit-email",
        "#edit-url",
        "#edit-tel",
        "#edit-password",
        "#edit-number",
        "#edit-date",
        "#edit-time",
        "#edit-datetime-local",
        "#edit-month",
        "#edit-week",
        "#edit-contenteditable",
        "#edit-plaintext",
    ] {
        let pos = target(view, page, board, selector, cx).await?;
        click(window, pos, cx)?;
        pause(cx, 120).await;
        let before = state(board, cx);
        let ime = page.read_with(cx, |p, _| p.input_method.is_some());
        let focused = evaluate(view, "document.activeElement.id", cx).await?;
        let spaces = evaluate(view, "canvasInput.space", cx).await?;
        key(window, "space", true, cx)?;
        pause(cx, 80).await;
        drag(
            window,
            pos,
            pos + point(px(40.), px(20.)),
            Modifiers::default(),
            cx,
        )
        .await?;
        key(window, "space", false, cx)?;
        let delivered = evaluate(view, "canvasInput.space", cx).await?;
        results.push(json!({"check":"Space stays in editable","selector":selector,"ime":ime,"focused":focused,"space_before":spaces,"space_after":delivered,"passed":focused==selector.trim_start_matches('#') && delivered.as_u64()==spaces.as_u64().map(|n|n+1) && before["camera"]==state(board,cx)["camera"]}));
    }
    evaluate(view, "(document.designMode='on')", cx).await?;
    let pos = target(view, page, board, "#selection", cx).await?;
    click(window, pos, cx)?;
    let before = state(board, cx);
    key(window, "space", true, cx)?;
    // Exercise a pointer press before the editable query has returned.
    drag(
        window,
        pos,
        pos + point(px(40.), px(20.)),
        Modifiers::default(),
        cx,
    )
    .await?;
    key(window, "space", false, cx)?;
    results.push(
        json!({"check":"Space stays in designMode, including pending-query drag",
        "passed":before["camera"]==state(board,cx)["camera"]}),
    );
    evaluate(view, "(document.designMode='off')", cx).await?;
    for (tool, shift) in [("h", false), ("h", true), ("v", false)] {
        board.update(cx, |b, cx| b.pan_by([-5000., -5000.], cx));
        pause(cx, 250).await;
        click(window, point(px(10.), px(10.)), cx)?;
        tap(window, tool, cx).await?;
        let before = state(board, cx);
        let start = point(px(20.), px(20.));
        let end = point(px(220.), px(180.));
        let modifiers = Modifiers {
            shift,
            ..Default::default()
        };
        window.update(cx, |_, w, cx| {
            w.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    position: start,
                    button: MouseButton::Left,
                    click_count: 1,
                    modifiers,
                    ..Default::default()
                }),
                cx,
            );
            w.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: end,
                    pressed_button: Some(MouseButton::Left),
                    modifiers,
                }),
                cx,
            );
        })?;
        pause(cx, 100).await;
        let during = state(board, cx);
        shot(window, dir.join(format!("gesture-{tool}-{shift}.png")), cx)?;
        window.update(cx, |_, w, cx| {
            w.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    position: end,
                    button: MouseButton::Left,
                    click_count: 1,
                    modifiers,
                    ..Default::default()
                }),
                cx,
            );
        })?;
        pause(cx, 220).await;
        let after = state(board, cx);
        results.push(json!({"check":"empty-board ownership","tool":tool,"shift":shift,"before":before,"after":after,
            "during":during,"passed":if tool=="h"&&!shift {before["camera"]!=after["camera"]} else {during["marquee"]==true && before["camera"]==after["camera"]}}));
    }
    let pos = target(view, page, board, "#target", cx).await?;
    click(window, pos, cx)?;
    let before = state(board, cx);
    window.update(cx, |_, w, cx| {
        w.dispatch_event(
            PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: pos,
                modifiers: Modifiers {
                    platform: true,
                    ..Default::default()
                },
                delta: ScrollDelta::Pixels(point(px(0.), px(-50.))),
                ..Default::default()
            }),
            cx,
        );
    })?;
    pause(cx, 250).await;
    results.push(json!({"check":"Command wheel zooms board","passed":before["camera"][2]!=state(board,cx)["camera"][2]}));
    let before = state(board, cx);
    window.update(cx, |_, w, cx| {
        w.dispatch_event(
            PlatformInput::Pinch(gpui_kit::PinchEvent {
                position: pos,
                delta: 0.15,
                ..Default::default()
            }),
            cx,
        );
    })?;
    pause(cx, 250).await;
    results.push(json!({"check":"synthetic pinch zooms board","passed":before["camera"][2]!=state(board,cx)["camera"][2]}));
    let pos = target(view, page, board, "#target", cx).await?;
    let clicks = evaluate(view, "window.canvasInput.clicks", cx).await?;
    window.update(cx, |_, w, cx| {
        board.update(cx, |b, cx| b.set_cached(true, w, cx))
    })?;
    let before = state(board, cx);
    drag(
        window,
        pos,
        pos + point(px(45.), px(25.)),
        Modifiers::default(),
        cx,
    )
    .await?;
    let moved = state(board, cx);
    let pos = pos + point(px(45.), px(25.));
    click(window, pos, cx)?;
    pause(cx, 250).await;
    let after = state(board, cx);
    let after_clicks = evaluate(view, "window.canvasInput.clicks", cx).await?;
    results.push(json!({"check":"cached card moves anywhere and activation does not click page",
        "before":before,"moved":moved,"after":after,"passed":before["card"]!=moved["card"] && moved["cached"]==true && after["cached"]==false && clicks==after_clicks}));
    for consume in [true, false] {
        let pos = target(view, page, board, "#target", cx).await?;
        click(window, pos, cx)?;
        evaluate(
            view,
            &format!("(document.querySelector('#consumeEscape').checked={consume})"),
            cx,
        )
        .await?;
        tap(window, "escape", cx).await?;
        results.push(json!({"check":"page-first Escape","consumed":consume,"passed":focused(window,page,cx)?==consume}));
    }
    let passed = results.iter().all(|r| r["passed"] == true);
    record(
        dir,
        "input.json",
        &json!({"checks":results,"passed":passed,"manual":["VoiceOver","IME candidate window","physical trackpad pinch","HTML drag/drop after R3"]}),
    )?;
    Ok(passed)
}

pub async fn diagnose(
    dir: &std::path::Path,
    view: &WebView,
    page: &Entity<page::Page>,
    board: &Entity<canvas::SpatialCanvas>,
    window: NativeWindow,
    cx: &mut AsyncApp,
) -> anyhow::Result<bool> {
    let mut rows = Vec::new();
    evaluate(view, "(() => { window.focusLog=[]; for (const kind of ['focus','blur','focusin','focusout']) addEventListener(kind,e=>focusLog.push({kind,id:e.target.id,at:performance.now()}),true); return true; })()", cx).await?;
    for selector in ["#choice", "#color", "#file"] {
        let pos = target(view, page, board, selector, cx).await?;
        eprintln!(
            "DIAG {} click {selector} {pos:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis()
        );
        click(window, pos, cx)?;
        for ms in [50, 200, 500] {
            pause(cx, ms).await;
            let state = page.read_with(cx, |p, _| json!({"popup":p.controls.popup.is_some(),"color":p.native_picker_visible("color"),"file":p.native_picker_visible("file")}));
            eprintln!(
                "DIAG {} {selector} {state}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis()
            );
            let focus = evaluate(
                view,
                "({active:document.activeElement.id,hasFocus:document.hasFocus(),events:focusLog})",
                cx,
            )
            .await?;
            rows.push(json!({"selector":selector,"delay_ms":ms,"state":state,"focus":focus}));
        }
        shot(window, dir.join(format!("{selector}.png")), cx)?;
        board.update(cx, |b, cx| b.pan_by([20., 10.], cx));
        pause(cx, 300).await;
        window.update(cx, |_, w, _| w.activate_window())?;
        anyhow::ensure!(
            window.update(cx, |_, w, _| w.is_visible())?,
            "diagnostic window became occluded"
        );
    }
    for zoom in [0.35, 1., 2.] {
        board.update(cx, |b, cx| b.zoom_to(zoom, cx));
        pause(cx, 300).await;
        let before=evaluate(view,"({count:canvasInput.hovers,hover:document.querySelector('#target').matches(':hover')})",cx).await?;
        pointer(window, point(px(2.), px(2.)), None, cx)?;
        let pos = target(view, page, board, "#target", cx).await?;
        click(window, pos, cx)?;
        pause(cx, 150).await;
        let after=evaluate(view,"({count:canvasInput.hovers,hover:document.querySelector('#target').matches(':hover')})",cx).await?;
        rows.push(json!({"zoom":zoom,"old_hover_before":before,"old_hover_after":after}));
        board.update(cx, |b, cx| b.pan_by([-5000., -5000.], cx));
        pause(cx, 250).await;
        pointer(window, point(px(10.), px(10.)), None, cx)?;
        pause(cx, 100).await;
        let exited=evaluate(view,"({count:canvasInput.hovers,hover:document.querySelector('#target').matches(':hover')})",cx).await?;
        let pos = target(view, page, board, "#target", cx).await?;
        pointer(window, pos, None, cx)?;
        pause(cx, 150).await;
        let reentered=evaluate(view,"({count:canvasInput.hovers,hover:document.querySelector('#target').matches(':hover')})",cx).await?;
        rows.push(json!({"zoom":zoom,"explicit_exit":exited,"reentered":reentered}));
    }
    record(dir, "diagnosis.json", &json!(rows))?;
    Ok(true)
}
