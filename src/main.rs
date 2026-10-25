//! Frame: a standalone browser. Servo renders the web, GPUI Kit draws the
//! chrome, and RustyWeb is the default search engine.

// The `Action` derive names its crate `gpui`.
extern crate gpui_kit as gpui;

mod actions;
mod address;
mod browser;
mod controls;
mod delegate;
mod downloads;
mod engine;
mod glass;
mod home;
mod icons;
mod keys;
mod markup;
mod navigator;
mod page;
mod places;
mod recents;
mod scene;
mod search;
mod serp;
mod share;
mod theme;

use anyhow::Context as _;
use gpui_kit::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
use gpui_kit::component::{Root, TitleBar};
use gpui_kit::{
    App, AppContext as _, KeyBinding, Menu, MenuItem, OsAction, Styled as _, WindowBounds,
    WindowOptions, point, px, size, transparent_black,
};

use crate::actions::{
    ActivateTab, CloseTab, FocusAddressBar, GoBack, GoForward, NewTab, NextTab, OpenPalette,
    PreviousTab, Quit, Reload, SetAppearance, SharePage, ShowDownloads, ToggleAsk, ToggleCompact,
};
use crate::browser::Browser;
use crate::engine::Engine;
use crate::theme::Appearance;

fn main() -> anyhow::Result<()> {
    // Logging is Servo's (`Engine::start`): it installs the one logger and
    // reads `RUST_LOG`. Servo's graph links more than one rustls provider, so
    // one has to be named before the first TLS connection.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let data_dir = dirs::data_dir()
        .context("no application data directory")?
        .join("Frame");
    let profile_dir = data_dir.join("profile");
    std::fs::create_dir_all(&profile_dir)?;
    // RustyWeb's provider keys: a checkout's `.env` while developing, then
    // the app's own. Neither overrides what the environment already sets.
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_path(data_dir.join(".env"));

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            install_commands(cx);
            Engine::start(&profile_dir, cx);
            // GPUI's images load from a URL through this, a result's favicon
            // among them: with Servo's user agent, and without cookies.
            match reqwest_client::ReqwestClient::user_agent(&Engine::user_agent()) {
                Ok(client) => cx.set_http_client(std::sync::Arc::new(client)),
                Err(error) => log::error!("http client: {error:#}"),
            }
            search::init(&data_dir, cx);
            recents::init(Some(data_dir.join("recents.json")), cx);
            if let Err(error) = open_window(cx) {
                log::error!("{error:#}");
                cx.quit();
            }
        });
    Ok(())
}

fn install_commands(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    // The appearance is the app's: every window follows it, and the menu
    // bar's check moves with it.
    cx.on_action(|action: &SetAppearance, cx| {
        theme::set_appearance(action.appearance, cx);
        install_menus(cx);
    });
    let mut bindings = vec![
        KeyBinding::new("cmd-t", NewTab, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("cmd-shift-]", NextTab, None),
        KeyBinding::new("cmd-}", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
        KeyBinding::new("cmd-shift-[", PreviousTab, None),
        KeyBinding::new("cmd-{", PreviousTab, None),
        KeyBinding::new("cmd-r", Reload, None),
        KeyBinding::new("cmd-[", GoBack, None),
        KeyBinding::new("cmd-]", GoForward, None),
        KeyBinding::new("cmd-l", FocusAddressBar, None),
        KeyBinding::new("cmd-k", OpenPalette, None),
        KeyBinding::new("ctrl-cmd-c", ToggleCompact, None),
        KeyBinding::new("alt-cmd-l", ShowDownloads, None),
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-z", Undo, Some(page::CONTEXT)),
        KeyBinding::new("cmd-shift-z", Redo, Some(page::CONTEXT)),
        KeyBinding::new("cmd-x", Cut, Some(page::CONTEXT)),
        KeyBinding::new("cmd-c", Copy, Some(page::CONTEXT)),
        KeyBinding::new("cmd-v", Paste, Some(page::CONTEXT)),
        KeyBinding::new("cmd-a", SelectAll, Some(page::CONTEXT)),
    ];
    for index in 0..9 {
        bindings.push(KeyBinding::new(
            &format!("cmd-{}", index + 1),
            ActivateTab { index },
            None,
        ));
    }
    cx.bind_keys(bindings);
    install_menus(cx);
}

fn install_menus(cx: &mut App) {
    let current = theme::appearance(cx);
    let appearances = Appearance::ALL.map(|appearance| {
        MenuItem::action(appearance.label(), SetAppearance { appearance })
            .checked(appearance == current)
    });
    cx.set_menus([
        Menu::new("Frame").items([MenuItem::action("Quit Frame", Quit)]),
        Menu::new("File").items([
            MenuItem::action("New Tab", NewTab),
            MenuItem::action("Open Location", FocusAddressBar),
            MenuItem::action("Search", OpenPalette),
            MenuItem::separator(),
            MenuItem::action("Share...", SharePage),
            MenuItem::separator(),
            MenuItem::action("Close Tab", CloseTab),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items([
            MenuItem::action("Reload Page", Reload),
            MenuItem::separator(),
            MenuItem::action("Compact Mode", ToggleCompact),
            MenuItem::submenu(Menu::new("Appearance").items(appearances)),
            MenuItem::separator(),
            MenuItem::action("Show Downloads", ShowDownloads),
            MenuItem::separator(),
            MenuItem::action("Ask AI", ToggleAsk),
        ]),
        Menu::new("History").items([
            MenuItem::action("Back", GoBack),
            MenuItem::action("Forward", GoForward),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Next Tab", NextTab),
            MenuItem::action("Previous Tab", PreviousTab),
        ]),
    ]);
}

fn open_window(cx: &mut App) -> anyhow::Result<()> {
    let mut options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1280.), px(820.)), cx)),
        window_min_size: Some(size(px(560.), px(360.))),
        ..TitleBar::window_options()
    };
    // Centered on the row of tabs, which is taller than GPUI Kit's title bar.
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.traffic_light_position = Some(point(px(9.), px(10.)));
    }
    cx.open_window(options, |window, cx| {
        glass::install(window, cx);
        let browser = cx.new(|cx| Browser::new(window, cx));
        // The browser paints the frame; the root leaves the glass under it.
        cx.new(|cx| Root::new(browser, window, cx).bg(transparent_black()))
    })?;
    // One window for now: closing it quits.
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
    Ok(())
}
