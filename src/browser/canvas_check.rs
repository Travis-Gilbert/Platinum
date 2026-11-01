//! Opt-in native acceptance for the shared board (`theorem.canvas.v1`).
//!
//! With `THEOREMWEB_AUTHORITY_URL` and `THEOREMWEB_AUTH_TOKEN_FILE` naming a
//! running Theorem node whose Space is saved, `FRAME_CANVAS_CHECK=<dir>`
//! opens an isolated profile, puts a real page on the board and checks that:
//! the card becomes an object on the shared board; objects placed elsewhere
//! (an agent's, through `canvas_mutate`) draw as typed preview cards; and a
//! stale move of this board's card is refused with the newer placement shown
//! and the conflict on screen. A second run with `FRAME_CANVAS_RESTORE=1` and
//! the same directory checks that the card keeps its object across a
//! restart. Each step leaves a native window capture beside `report.json`.
use super::workspace_check::{capture, until_within};
use super::*;
use crate::canvas::authority::{Answer, Client};
use gpui_kit::component::Root;
use gpui_kit::{AsyncApp, WindowBounds, WindowOptions};
use std::{path::PathBuf, time::Duration};
use theorem_canvas::{CanvasBoard, CanvasMutation, CanvasRequest, Rect};

pub fn run(output: PathBuf) -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    std::fs::create_dir_all(&output)?;
    let store = frame_profile::ProfileStore::open(&output.join("profile"))?;
    let profile = store.directory();
    let restore = std::env::var_os("FRAME_CANVAS_RESTORE").is_some();
    let outcome = Rc::new(std::cell::RefCell::new(None));
    let result = outcome.clone();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            crate::install_commands(cx);
            Engine::start(&profile, cx);
            recents::init(None, cx);
            crate::theorem::init(None, cx);
            crate::session::init(&profile, cx);
            cx.set_global(ActiveProfile(store));
            let mut browser = None;
            let handle = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::centered(size(px(1440.), px(960.)), cx)),
                        ..TitleBar::window_options()
                    },
                    |window, cx| {
                        glass::install(window, cx);
                        let view = cx.new(|cx| Browser::new(window, cx));
                        browser = Some(view.clone());
                        window.activate_window();
                        cx.new(|cx| Root::new(view, window, cx))
                    },
                )
                .unwrap()
                .into();
            let browser = browser.unwrap();
            cx.activate(true);
            cx.spawn(async move |cx| {
                let mut report = serde_json::Map::new();
                let checked = scenario(handle, browser, &output, restore, &mut report, cx).await;
                report.insert("passed".into(), checked.is_ok().into());
                report.insert("restore".into(), restore.into());
                report.insert(
                    "error".into(),
                    checked.as_ref().err().map(|e| format!("{e:#}")).into(),
                );
                let name = if restore {
                    "restore-report.json"
                } else {
                    "report.json"
                };
                let _ = std::fs::write(
                    output.join(name),
                    serde_json::to_vec_pretty(&report).unwrap(),
                );
                *result.borrow_mut() = Some(checked);
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    outcome
        .borrow_mut()
        .take()
        .unwrap_or_else(|| Err(anyhow::anyhow!("Native check did not finish")))
}

/// The shared board as another client reads it.
async fn shared(client: &Client) -> anyhow::Result<CanvasBoard> {
    async_compat::Compat::new(client.board())
        .await
        .map_err(anyhow::Error::msg)?
        .ok_or_else(|| anyhow::anyhow!("the Space is not saved in Theorem"))
}

async fn scenario(
    handle: gpui_kit::AnyWindowHandle,
    browser: Entity<Browser>,
    output: &std::path::Path,
    restore: bool,
    report: &mut serde_json::Map<String, serde_json::Value>,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    // Another client on the same credential: a person's second device.
    let other = Client::from_env().ok_or_else(|| {
        anyhow::anyhow!("THEOREMWEB_AUTHORITY_URL and THEOREMWEB_AUTH_TOKEN_FILE must name a node")
    })?;
    let before = shared(&other).await?;
    report.insert("objects_before".into(), before.objects.len().into());
    if !restore {
        let url = Url::parse(&std::env::var("FRAME_CANVAS_URL").unwrap_or_else(|_| {
            "data:text/html,<title>Shared board check</title><h1>Shared board check</h1>".into()
        }))?;
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
            "the page did not load"
        );
    }
    handle.update(cx, |_, window, cx| {
        browser.update(cx, |b, cx| b.toggle_canvas(window, cx))
    })?;
    let board = browser.read_with(cx, |b, _| b.canvas.clone().unwrap());
    let linked = |cx: &mut AsyncApp| board.read_with(cx, |b, cx| b.state(cx).canvas_links);
    anyhow::ensure!(
        until_within(Duration::from_secs(30), cx, |cx| !linked(cx).is_empty()).await,
        "no card became an object on the shared board"
    );
    let links = linked(cx);
    report.insert("links".into(), serde_json::to_value(&links)?);
    let card = links[0].clone();
    let after = shared(&other).await?;
    let object = after
        .objects
        .iter()
        .find(|o| o.id == card.id)
        .ok_or_else(|| anyhow::anyhow!("the linked object is not on the shared board"))?
        .clone();
    report.insert("card_object".into(), serde_json::to_value(&object)?);
    let previews = std::env::var("FRAME_CANVAS_PREVIEWS")
        .ok()
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(1);
    anyhow::ensure!(
        until_within(Duration::from_secs(15), cx, |cx| board
            .read_with(cx, |b, _| b.previews().len() >= previews))
        .await,
        "objects placed elsewhere did not draw as preview cards"
    );
    report.insert(
        "previews".into(),
        board
            .read_with(cx, |b, _| {
                b.previews()
                    .iter()
                    .map(|p| serde_json::json!({"kind": p.kind, "title": p.title, "detail": p.detail}))
                    .collect::<Vec<_>>()
            })
            .into(),
    );
    if restore {
        let saved: Vec<crate::session::format::CanvasLink> =
            serde_json::from_slice(&std::fs::read(output.join("links.json"))?)?;
        anyhow::ensure!(
            links == saved,
            "the card's object changed across the restart"
        );
        anyhow::ensure!(
            after.objects.len() == before.objects.len(),
            "the restart placed a second object"
        );
        let shown = board.read_with(cx, |b, _| b.rect(card.local));
        report.insert("card_rect".into(), serde_json::to_value(shown)?);
        anyhow::ensure!(
            shown == Some([object.rect.x, object.rect.y, object.rect.w, object.rect.h]),
            "the card is not where the shared board holds it"
        );
        handle.update(cx, |_, _, cx| board.update(cx, |b, cx| b.overview(cx)))?;
        capture(handle, Appearance::Light, &output.join("restored.png"), cx).await;
        return Ok(());
    }
    std::fs::write(
        output.join("links.json"),
        serde_json::to_vec_pretty(&links)?,
    )?;
    handle.update(cx, |_, _, cx| board.update(cx, |b, cx| b.overview(cx)))?;
    capture(handle, Appearance::Light, &output.join("board.png"), cx).await;

    // The other device moves the card first; this board's move, based on the
    // revision it saw, is then stale.
    let newer = Rect::new(
        object.rect.x + 480.,
        object.rect.y + 120.,
        object.rect.w,
        object.rect.h,
    );
    let request = CanvasRequest {
        operation_key: format!(
            "canvas-check:{}:other-device",
            crate::canvas::authority::run_id()
        ),
        mutation: CanvasMutation::MoveResize {
            id: card.id.clone(),
            base_placement_revision: object.placement_revision,
            rect: newer,
        },
    };
    match async_compat::Compat::new(other.mutate(&request))
        .await
        .map_err(anyhow::Error::msg)?
    {
        Answer::Accepted(receipt) => report.insert(
            "other_device_receipt".into(),
            serde_json::to_value(&receipt)?,
        ),
        Answer::Refused(refusal) => anyhow::bail!("the other device's move was refused: {refusal}"),
    };
    let mine = [
        object.rect.x - 240.,
        object.rect.y,
        object.rect.w,
        object.rect.h,
    ];
    handle.update(cx, |_, _, cx| {
        board.update(cx, |b, cx| b.move_card(card.local, mine, cx))
    })?;
    anyhow::ensure!(
        until_within(Duration::from_secs(10), cx, |cx| board
            .read_with(cx, |b, _| b.conflict().is_some()))
        .await,
        "the stale move showed no conflict"
    );
    let conflict = board.read_with(cx, |b, _| b.conflict().map(str::to_owned));
    let shown = board.read_with(cx, |b, _| b.rect(card.local));
    report.insert("conflict".into(), conflict.clone().into());
    report.insert(
        "card_rect_after_conflict".into(),
        serde_json::to_value(shown)?,
    );
    anyhow::ensure!(
        shown == Some([newer.x, newer.y, newer.w, newer.h]),
        "the board did not take the newer placement: {shown:?}"
    );
    let held = shared(&other).await?;
    let kept = held
        .objects
        .iter()
        .find(|o| o.id == card.id)
        .map(|o| o.rect);
    anyhow::ensure!(
        kept == Some(newer),
        "the stale move overwrote the newer placement"
    );
    handle.update(cx, |_, _, cx| board.update(cx, |b, cx| b.overview(cx)))?;
    capture(handle, Appearance::Light, &output.join("conflict.png"), cx).await;
    capture(
        handle,
        Appearance::Dark,
        &output.join("conflict-dark.png"),
        cx,
    )
    .await;
    handle.update(cx, |_, _, cx| browser.update(cx, |b, cx| b.save(cx)))?;
    Ok(())
}
