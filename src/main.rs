//! Platinum: a standalone browser. Servo renders the web, GPUI Kit draws the
//! chrome, and RustyWeb is the default search engine.

// The `Action` derive names its crate `gpui`.
extern crate gpui_kit as gpui;

mod actions;
mod address;
mod brand;
mod browser;
mod canvas;
mod controls;
mod delegate;
mod downloads;
mod embedder;
mod engine;
mod favicon;
mod glass;
mod home;
mod icons;
mod inspector;
mod keys;
mod markup;
mod navigator;
mod notes;
mod page;
#[cfg(feature = "performance-check")]
mod performance;
mod places;
mod profile_ui;
mod recents;
mod scene;
mod search;
mod serp;
mod session;
mod share;
mod site;
mod tabtree;
mod theme;

use anyhow::Context as _;
use gpui_kit::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
use gpui_kit::component::{Root, TitleBar};
use gpui_kit::{
    App, AppContext as _, KeyBinding, Menu, MenuItem, OsAction, Styled as _, WindowBounds,
    WindowOptions, point, px, size, transparent_black,
};

use crate::actions::{
    ActivateTab, BookmarkPage, CloseTab, FillForm, FillPassword, FocusAddressBar, GoBack,
    GoForward, ImportBrowserData, NewTab, NextTab, OpenPalette, PinTab, PreviousTab, Quit, Reload,
    SavePassword, SetAppearance, SharePage, ShowBookmarks, ShowConsole, ShowDownloads, ShowHistory,
    ShowProfiles, ToggleAsk, ToggleCompact, ToggleSidebar,
};
use crate::browser::Browser;
use crate::engine::Engine;
use crate::theme::Appearance;

fn main() -> anyhow::Result<()> {
    #[cfg(feature = "snapshot")]
    if let Some(output) = std::env::var_os("FRAME_WORKSPACE_CHECK") {
        return browser::workspace_check::run(output.into());
    }
    #[cfg(feature = "performance-check")]
    if std::env::var_os("FRAME_PERF_ROOT").is_some() {
        return browser::performance::run();
    }
    // Logging is Servo's (`Engine::start`): it installs the one logger and
    // reads `RUST_LOG`. Servo's graph links more than one rustls provider, so
    // one has to be named before the first TLS connection.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let data_dir = brand::app_dir(&dirs::data_dir().context("no application data directory")?);
    let profile = frame_profile::ProfileStore::open(&data_dir)?;
    let profile_dir = profile.directory();
    let search_dir = profile.search_directory();
    session::adopt(&data_dir, &profile.default_directory());
    // RustyWeb's provider keys: a checkout's `.env` while developing, then
    // the app's own. Neither overrides what the environment already sets.
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_path(data_dir.join(".env"));

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            brand::install();
            install_commands(cx);
            Engine::start(&profile_dir, cx);
            // GPUI's images load from a URL through this, a result's favicon
            // among them: with Servo's user agent, and without cookies.
            match reqwest_client::ReqwestClient::user_agent(&Engine::user_agent()) {
                Ok(client) => cx.set_http_client(std::sync::Arc::new(client)),
                Err(error) => log::error!("http client: {error:#}"),
            }
            recents::init(Some(profile_dir.join("recents.json")), cx);
            search::init(&search_dir, cx);
            session::init(&profile_dir, cx);
            session::open_imported_tabs(&profile, cx);
            cx.set_global(profile_ui::ActiveProfile(profile));
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
        KeyBinding::new("cmd-d", BookmarkPage, None),
        KeyBinding::new("cmd-y", ShowHistory, None),
        KeyBinding::new("cmd-shift-p", FillPassword, None),
        KeyBinding::new("ctrl-cmd-c", ToggleCompact, None),
        KeyBinding::new("ctrl-cmd-s", ToggleSidebar, None),
        KeyBinding::new("ctrl-cmd-b", crate::actions::ShowCanvas, None),
        KeyBinding::new("alt-cmd-l", ShowDownloads, None),
        KeyBinding::new("alt-cmd-j", ShowConsole, None),
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
        Menu::new(brand::NAME).items([MenuItem::action(format!("Quit {}", brand::NAME), Quit)]),
        Menu::new("File").items([
            MenuItem::action("New Tab", NewTab),
            MenuItem::action("Open Location", FocusAddressBar),
            MenuItem::action("Search", OpenPalette),
            MenuItem::separator(),
            MenuItem::action("Import Browser Data…", ImportBrowserData),
            MenuItem::action("Profiles…", ShowProfiles),
            MenuItem::action("Share...", SharePage),
            MenuItem::action("Pin Tab", PinTab),
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
            MenuItem::separator(),
            MenuItem::action("Fill Form Information…", FillForm),
        ]),
        Menu::new("View").items([
            MenuItem::action("Reload Page", Reload),
            MenuItem::separator(),
            MenuItem::action("Compact Mode", ToggleCompact),
            MenuItem::action("Sidebar", ToggleSidebar),
            MenuItem::submenu(Menu::new("Appearance").items(appearances)),
            MenuItem::separator(),
            MenuItem::action("Show Downloads", ShowDownloads),
            MenuItem::action("Show Console", ShowConsole),
            MenuItem::separator(),
            MenuItem::action("Ask AI", ToggleAsk),
        ]),
        Menu::new("History").items([
            MenuItem::action("Back", GoBack),
            MenuItem::action("Forward", GoForward),
            MenuItem::action("Show History…", ShowHistory),
        ]),
        Menu::new("Bookmarks").items([
            MenuItem::action("Bookmark This Page", BookmarkPage),
            MenuItem::action("Show Bookmarks…", ShowBookmarks),
        ]),
        Menu::new("Passwords").items([
            MenuItem::action("Fill Saved Password…", FillPassword),
            MenuItem::action("Save or Update Password…", SavePassword),
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
    let _window = cx.open_window(options, |window, cx| {
        glass::install(window, cx);
        let browser = cx.new(|cx| Browser::new(window, cx));
        // The browser paints the frame; the root leaves the glass under it.
        cx.new(|cx| Root::new(browser, window, cx).bg(transparent_black()))
    })?;
    #[cfg(feature = "snapshot")]
    keep_snapshots(_window.into(), cx);
    // One window for now: closing it quits.
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
    Ok(())
}

/// Keep a picture of `window` in the file `FRAME_SNAPSHOT` names, redrawn
/// every two seconds (the `snapshot` feature, for development).
#[cfg(feature = "snapshot")]
fn keep_snapshots(window: gpui_kit::AnyWindowHandle, cx: &mut App) {
    let Some(path) = std::env::var_os("FRAME_SNAPSHOT") else {
        return;
    };
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(2))
                .await;
            let Ok(image) = cx.update_window(window, |_, window, _| window.render_to_image())
            else {
                break;
            };
            // Written aside and moved over, so the file is always whole.
            let partial = std::path::Path::new(&path).with_extension("partial.png");
            let saved = image.and_then(|image| {
                image.save(&partial)?;
                Ok(std::fs::rename(&partial, &path)?)
            });
            if let Err(error) = saved {
                log::warn!("snapshot: {error:#}");
            }
        }
    })
    .detach();
}
