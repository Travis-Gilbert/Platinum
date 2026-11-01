//! UI integration tests of the chrome in headless windows. Pages need Servo
//! and results need the network, so these cover what is Platinum's own: the
//! right-click search, the palette, tab shortcuts, dragging tabs, the Ask
//! button, the appearance button, the downloads list, compact mode, the
//! sidebar's tree, pinned sites, restoring the session and the Agent place.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::{Root, Theme};
use gpui_kit::{
    AppContext as _, Bounds, Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point,
    RenderImage, TestAppContext, VisualTestContext, point, px, size,
};
use url::Url;

use super::{Browser, Showing};
use crate::downloads::DownloadState;
use crate::navigator::{Pick, Placement, TabAction};
use crate::places::Place as NativePlace;
use crate::session::{self, Content, Layout, Pin};
use crate::tabtree::Place;
use crate::theme::{self, Appearance};

#[test]
fn search_results_do_not_keep_the_inspected_page() {
    assert_eq!(
        super::displayed_page_id(Showing::Serp, Some("page-a".into())),
        None
    );
    assert_eq!(
        super::displayed_page_id(Showing::Start, Some("page-a".into())),
        None
    );
    assert_eq!(
        super::displayed_page_id(Showing::Page, Some("page-a".into())).as_deref(),
        Some("page-a")
    );
    assert_eq!(super::displayed_page_id(Showing::Page, None), None);
}

fn open(cx: &mut TestAppContext) -> (Entity<Browser>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        theme::init(cx);
        crate::install_commands(cx);
        crate::recents::init(None, cx);
    });
    let mut browser = None;
    let window = cx.open_window(size(px(1000.), px(700.)), |window, cx| {
        let view = cx.new(|cx| Browser::new(window, cx));
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    // As the person's window is: GPUI tells focus listeners nothing while
    // the window is in the background.
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (browser.expect("browser view"), cx)
}

/// Frame with one tab open, from ⌘T on the workspace.
fn open_with_tab(cx: &mut TestAppContext) -> (Entity<Browser>, &mut VisualTestContext) {
    let (browser, cx) = open(cx);
    keys(cx, "cmd-t");
    (browser, cx)
}

#[gpui_kit::test]
fn board_keeps_sleeping_pages_and_their_saved_placement(cx: &mut TestAppContext) {
    let (browser, cx) = open(cx);
    cx.update(|window, cx| {
        browser.update(cx, |b, cx| {
            let url = Url::parse("https://example.org/saved").unwrap();
            b.tabs[0].asleep = Some(Content::Page {
                url: url.clone(),
                title: "Saved source".into(),
            });
            assert_eq!(b.tabs[0].url(cx), Some(url));
            let id = b.tabs[0].id;
            b.board_state.cards = vec![crate::session::format::CardPlacement {
                tab: id,
                rect: [230., 450., 900., 600.],
            }];
            b.toggle_canvas(window, cx);
            let state = b.canvas.as_ref().unwrap().read(cx).state(cx);
            assert_eq!(state.cards, b.board_state.cards);
            assert!(
                b.tabs[0].page.is_none(),
                "overview must not wake sleeping pages"
            );
        });
    });
}

fn workspace(browser: &Entity<Browser>, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| browser.read(cx).underlay.shown)
}

/// The workspace's route, by its title.
fn route(browser: &Entity<Browser>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| browser.read(cx).underlay.title(cx).to_string())
}

fn in_workspace_field(browser: &Entity<Browser>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        browser
            .read(cx)
            .underlay
            .field
            .focus_handle(cx)
            .is_focused(window)
    })
}

fn showing(browser: &Entity<Browser>, cx: &mut VisualTestContext) -> Showing {
    cx.update(|_, cx| {
        let browser = browser.read(cx);
        browser.tabs[browser.active].showing
    })
}

fn right_click(cx: &mut VisualTestContext, at: Point<Pixels>) {
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
}

fn hover(cx: &mut VisualTestContext, at: Point<Pixels>) {
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

/// The navigator is open and its search field has the keyboard.
fn typing_in_navigator(browser: &Entity<Browser>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        browser
            .read(cx)
            .navigator
            .as_ref()
            .is_some_and(|(navigator, _)| navigator.state.focus_handle(cx).is_focused(window))
    })
}

fn query(browser: &Entity<Browser>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| {
        let (navigator, _) = browser.read(cx).navigator.as_ref().expect("navigator open");
        navigator.state.read(cx).query(cx).to_string()
    })
}

/// What the open navigator offers for `query`, group by group.
fn offered(browser: &Entity<Browser>, query: &str, cx: &mut VisualTestContext) -> Vec<Vec<Pick>> {
    cx.update(|_, cx| {
        let browser = browser.read(cx);
        let (navigator, _) = browser.navigator.as_ref().expect("navigator open");
        let tabs = browser.navigator_tabs(cx);
        navigator
            .entries(query, &browser.navigator_state(&tabs, cx))
            .1
    })
}

fn lists_tabs(groups: &[Vec<Pick>]) -> bool {
    groups
        .iter()
        .flatten()
        .any(|pick| matches!(pick, Pick::SwitchTab(_)))
}

fn in_address_bar(browser: &Entity<Browser>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| browser.read(cx).omnibox.focus_handle(cx).is_focused(window))
}

fn tabs(browser: &Entity<Browser>, cx: &mut VisualTestContext) -> (usize, usize) {
    cx.update(|_, cx| {
        let browser = browser.read(cx);
        (browser.tabs.len(), browser.active)
    })
}

fn dark(cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| Theme::global(cx).mode.is_dark())
}

fn ids(browser: &Entity<Browser>, cx: &mut VisualTestContext) -> Vec<u64> {
    cx.update(|_, cx| browser.read(cx).tabs.iter().map(|tab| tab.id).collect())
}

/// The element drawn with `debug_selector(name)`.
fn bounds_of(cx: &mut VisualTestContext, name: String) -> Bounds<Pixels> {
    // `debug_bounds` takes a static name; these few live as long as the test.
    let name: &'static str = Box::leak(name.into_boxed_str());
    cx.debug_bounds(name)
        .unwrap_or_else(|| panic!("{name} drawn"))
}

/// Whether an element was drawn with `debug_selector(name)`.
fn drawn(cx: &mut VisualTestContext, name: String) -> bool {
    let name: &'static str = Box::leak(name.into_boxed_str());
    cx.debug_bounds(name).is_some()
}

fn active_id(browser: &Entity<Browser>, cx: &mut VisualTestContext) -> u64 {
    cx.update(|_, cx| {
        let browser = browser.read(cx);
        browser.tabs[browser.active].id
    })
}

/// A fresh data directory for one test.
fn data_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("frame-test-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temporary data directory");
    dir
}

/// The middle of the element drawn with `debug_selector(name)`.
fn center_of(cx: &mut VisualTestContext, name: String) -> Point<Pixels> {
    bounds_of(cx, name).center()
}

fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
    let held = Some(MouseButton::Left);
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    // Past GPUI's drag threshold, then over the target.
    cx.simulate_mouse_move(from + point(px(8.), px(0.)), held, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_mouse_move(to, held, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

fn click(cx: &mut VisualTestContext, at: Point<Pixels>) {
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

#[gpui_kit::test]
fn right_click_opens_search_at_the_pointer_ready_to_type(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    assert!(cx.debug_bounds("navigator").is_none());

    // Beside Home's tiles: right-clicking a button focuses it.
    let pointer = point(px(40.), px(300.));
    right_click(cx, pointer);
    let placed_at = cx.update(|_, cx| {
        browser
            .read(cx)
            .navigator
            .as_ref()
            .map(|(navigator, _)| navigator.placement)
    });
    assert!(matches!(placed_at, Some(Placement::Pointer(at)) if at == pointer));
    let menu = cx.debug_bounds("navigator").expect("menu drawn");
    assert_eq!(menu.origin, pointer);
    // The cursor is already in the search field: typing goes straight in.
    assert!(typing_in_navigator(&browser, cx));
    cx.simulate_input("light");
    cx.run_until_parked();
    assert_eq!(query(&browser, cx), "light");

    assert!(dark(cx));
    keys(cx, "enter");
    assert!(
        cx.update(|_, cx| browser.read(cx).navigator.is_none()),
        "enter picked something"
    );
    assert!(!dark(cx), "Light Mode was picked");
    assert!(cx.debug_bounds("navigator").is_none());
    // The cursor goes back to where it was: the start tab's address bar.
    assert!(in_address_bar(&browser, cx));
}

#[gpui_kit::test]
fn the_right_click_menu_leads_with_tools_and_finds_tabs_once_typed(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t");
    right_click(cx, point(px(500.), px(300.)));
    let untyped = offered(&browser, "", cx);
    assert!(
        untyped[0].iter().all(|pick| matches!(pick, Pick::Command)),
        "Platinum's tools come first"
    );
    assert!(!lists_tabs(&untyped), "no list of tabs until typing");
    assert!(lists_tabs(&offered(&browser, "example", cx)));

    // The palette stays a switcher.
    keys(cx, "escape");
    keys(cx, "cmd-k");
    let palette = offered(&browser, "", cx);
    assert!(
        palette[0]
            .iter()
            .all(|pick| matches!(pick, Pick::SwitchTab(_)))
    );
}

#[gpui_kit::test]
fn a_tabs_right_click_menu_pins_that_tab(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t");
    let [a, b] = ids(&browser, cx)[..] else {
        panic!("two tabs")
    };
    // A page, as a restored session leaves it before it loads, in the tab
    // that is not active.
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            let index = browser.tab_index(a).unwrap();
            browser.tabs[index].asleep = Some(Content::Page {
                url: Url::parse("https://example.org/").unwrap(),
                title: "Example".into(),
            });
            cx.notify();
        })
    });
    assert_eq!(active_id(&browser, cx), b);

    let tab = center_of(cx, format!("tab-{a}"));
    right_click(cx, tab);
    let menu = offered(&browser, "", cx);
    assert!(
        matches!(
            menu[0][..],
            [
                Pick::Tab(TabAction::Rename(x)),
                Pick::Tab(TabAction::Pin(y)),
                Pick::Tab(TabAction::Close(z)),
            ] if [x, y, z] == [a; 3]
        ),
        "the tab's own entries come first"
    );
    assert!(!lists_tabs(&menu));
    assert!(typing_in_navigator(&browser, cx));

    cx.simulate_input("pin");
    cx.run_until_parked();
    keys(cx, "enter");
    let pinned = cx.update(|_, cx| {
        let pins = &browser.read(cx).pins;
        pins.iter()
            .map(|pin| pin.url.to_string())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        pinned,
        ["https://example.org/"],
        "the tab clicked, not the active one"
    );
    assert!(!drawn(cx, format!("tab-{a}")), "its tab left the row");
}

#[gpui_kit::test]
fn escape_closes_the_right_click_search(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    right_click(cx, point(px(400.), px(400.)));
    assert!(typing_in_navigator(&browser, cx));
    keys(cx, "escape");
    assert!(cx.debug_bounds("navigator").is_none());
    assert!(cx.update(|_, cx| browser.read(cx).navigator.is_none()));
    assert!(in_address_bar(&browser, cx));
}

#[gpui_kit::test]
fn palette_works_with_the_toolbar_hidden(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    assert!(cx.debug_bounds("address-bar").is_some());

    keys(cx, "ctrl-cmd-c");
    assert!(cx.update(|_, cx| browser.read(cx).compact));
    assert!(cx.debug_bounds("address-bar").is_none());

    keys(cx, "cmd-k");
    let palette = cx.debug_bounds("navigator").expect("palette drawn");
    assert!(
        palette.origin.y < px(200.),
        "palette sits near the top: {palette:?}"
    );
    assert!(typing_in_navigator(&browser, cx));
    cx.simulate_input("new tab");
    cx.run_until_parked();
    keys(cx, "enter");

    assert_eq!(tabs(&browser, cx), (2, 1));
    assert!(cx.debug_bounds("navigator").is_none());
    // A new tab puts the cursor in the address bar, so the chrome shows.
    assert!(cx.debug_bounds("address-bar").is_some());
    assert!(in_address_bar(&browser, cx));
}

#[gpui_kit::test]
fn palette_toggles_and_escape_closes_it(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-k");
    assert!(typing_in_navigator(&browser, cx));
    keys(cx, "cmd-k");
    assert!(cx.debug_bounds("navigator").is_none());
    assert!(in_address_bar(&browser, cx));

    keys(cx, "cmd-k");
    keys(cx, "escape");
    assert!(cx.debug_bounds("navigator").is_none());
    assert!(in_address_bar(&browser, cx));
}

#[gpui_kit::test]
fn tab_shortcuts(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t cmd-t");
    assert_eq!(tabs(&browser, cx), (3, 2));
    assert!(in_address_bar(&browser, cx));

    keys(cx, "cmd-1");
    assert_eq!(tabs(&browser, cx), (3, 0));
    keys(cx, "cmd-9");
    assert_eq!(tabs(&browser, cx), (3, 2));
    keys(cx, "ctrl-tab");
    assert_eq!(tabs(&browser, cx), (3, 0));
    keys(cx, "cmd-shift-[");
    assert_eq!(tabs(&browser, cx), (3, 2));

    let last = cx.update(|_, cx| browser.read(cx).tabs[2].id);
    keys(cx, "cmd-1 cmd-w");
    assert_eq!(tabs(&browser, cx), (2, 0));
    assert_eq!(cx.update(|_, cx| browser.read(cx).tabs[1].id), last);

    // Closing the last tab leaves a fresh one, behind the workspace.
    keys(cx, "cmd-w");
    assert!(!workspace(&browser, cx));
    keys(cx, "cmd-w");
    assert_eq!(tabs(&browser, cx), (1, 0));
    assert!(workspace(&browser, cx));
}

#[gpui_kit::test]
fn dragging_a_tab_onto_another_moves_it_there(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t cmd-t");
    let [a, b, c] = ids(&browser, cx)[..] else {
        panic!("three tabs")
    };

    let from = center_of(cx, format!("tab-{c}"));
    let to = center_of(cx, format!("tab-{a}"));
    drag(cx, from, to);
    assert_eq!(ids(&browser, cx), [c, a, b]);
    assert_eq!(
        tabs(&browser, cx),
        (3, 0),
        "the dragged tab is still the active one"
    );
    // The shortcuts follow the new order.
    keys(cx, "cmd-2");
    let active = cx.update(|_, cx| {
        let browser = browser.read(cx);
        browser.tabs[browser.active].id
    });
    assert_eq!(active, a);
}

#[gpui_kit::test]
fn dropping_a_tab_on_a_pages_edge_puts_the_two_side_by_side(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t");
    let [a, b] = ids(&browser, cx)[..] else {
        panic!("two tabs")
    };

    let from = center_of(cx, format!("tab-{a}"));
    drag(cx, from, point(px(900.), px(400.)));
    let (shown, top) = cx.update(|_, cx| {
        let browser = browser.read(cx);
        (browser.displayed.len(), browser.top_groups.len())
    });
    assert_eq!(shown, 2, "a group for each tab, both on screen");
    assert_eq!(top, 2, "side by side, each with its row of tabs at the top");
    assert_eq!(ids(&browser, cx), [b, a], "tabs run left to right");
    assert_eq!(
        tabs(&browser, cx),
        (2, 0),
        "the tab that stayed is still active"
    );
    // Only the active tab's card holds the address bar.
    assert!(cx.debug_bounds("address-bar").is_some());

    // The address bar moves to whichever side is clicked.
    let right = point(px(750.), px(400.));
    click(cx, right);
    assert_eq!(tabs(&browser, cx), (2, 1));
    keys(cx, "cmd-l");
    let bar = cx.debug_bounds("address-bar").expect("address bar drawn");
    assert!(bar.origin.x > px(500.), "on the right-hand card: {bar:?}");
}

#[gpui_kit::test]
fn ask_in_the_address_bar_switches_it_between_searching_and_asking(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    let id = ids(&browser, cx)[0];
    let state = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let browser = browser.read(cx);
            let placeholder = browser
                .omnibox
                .read(cx)
                .presentation()
                .placeholder()
                .to_string();
            (browser.ask, placeholder)
        })
    };
    assert_eq!(state(cx), (false, "Search or go somewhere".to_owned()));

    // An icon alone, square, inside the address bar at its end.
    let ask = bounds_of(cx, format!("ask-{id}"));
    assert_eq!(ask.size.width, ask.size.height);
    let bar = bounds_of(cx, "address-bar".to_owned());
    assert!(
        bar.contains(&ask.origin) && bar.contains(&ask.bottom_right()),
        "{ask:?} in {bar:?}"
    );
    assert!(
        ask.center().x > bar.center().x,
        "at the end: {ask:?} in {bar:?}"
    );

    let button = ask.center();
    click(cx, button);
    assert_eq!(state(cx), (true, "Ask...".to_owned()));
    assert!(in_address_bar(&browser, cx), "ready for the question");

    click(cx, button);
    assert_eq!(state(cx), (false, "Search or go somewhere".to_owned()));
}

#[gpui_kit::test]
fn compact_chrome_comes_back_at_the_top_edge(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "ctrl-cmd-c");
    assert!(cx.debug_bounds("address-bar").is_none());
    let revealed = |cx: &mut VisualTestContext| cx.update(|_, cx| browser.read(cx).revealed);

    hover(cx, point(px(500.), px(2.)));
    hover(cx, point(px(500.), px(20.)));
    assert!(revealed(cx));
    assert!(cx.debug_bounds("address-bar").is_some());

    hover(cx, point(px(500.), px(400.)));
    assert!(!revealed(cx));
    assert!(cx.debug_bounds("address-bar").is_none());

    // ⌘L brings the chrome back with the cursor in the address bar.
    keys(cx, "cmd-l");
    assert!(revealed(cx));
    assert!(in_address_bar(&browser, cx));
    assert!(cx.debug_bounds("address-bar").is_some());
}

#[gpui_kit::test]
fn appearance_button_switches_and_the_palette_matches_the_system(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    let id = ids(&browser, cx)[0];
    let appearance = |cx: &mut VisualTestContext| cx.update(|_, cx| theme::appearance(cx));
    assert_eq!(appearance(cx), Appearance::Dark);

    // The toolbar's button switches at once, with no menu between.
    let button = center_of(cx, format!("appearance-{id}"));
    click(cx, button);
    assert_eq!(appearance(cx), Appearance::Light);
    assert!(!dark(cx));
    click(cx, button);
    assert_eq!(appearance(cx), Appearance::Dark);
    assert!(dark(cx));

    // Match System is in the palette.
    keys(cx, "cmd-k");
    cx.simulate_input("match system");
    cx.run_until_parked();
    keys(cx, "enter");
    assert_eq!(appearance(cx), Appearance::System);
    let system_dark = cx.update(|window, _| {
        matches!(
            window.appearance(),
            gpui_kit::WindowAppearance::Dark | gpui_kit::WindowAppearance::VibrantDark
        )
    });
    assert_eq!(dark(cx), system_dark);
}

#[gpui_kit::test]
fn a_file_address_downloads_instead_of_opening(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    let id = ids(&browser, cx)[0];
    assert!(in_address_bar(&browser, cx));
    // Nothing listens on the discard port, so nothing reaches the disk.
    cx.simulate_input("http://127.0.0.1:9/files/Frame%20Test.dmg");
    keys(cx, "enter");

    let (showing, downloads, open) = cx.update(|_, cx| {
        let browser = browser.read(cx);
        let downloads: Vec<(String, bool)> = browser
            .downloads
            .read(cx)
            .items()
            .iter()
            .map(|download| {
                let failed = matches!(download.state, DownloadState::Failed(_));
                (download.name.to_string(), failed)
            })
            .collect();
        (browser.tabs[0].showing, downloads, browser.downloads_open)
    });
    assert!(showing == Showing::Start, "no page opened for the file");
    assert_eq!(downloads, [("Frame Test.dmg".to_owned(), true)]);
    assert_eq!(open, Some(id), "the list shows the download");

    // ⌥⌘L closes and opens the list.
    keys(cx, "alt-cmd-l");
    assert_eq!(cx.update(|_, cx| browser.read(cx).downloads_open), None);
    keys(cx, "alt-cmd-l");
    assert_eq!(cx.update(|_, cx| browser.read(cx).downloads_open), Some(id));
}

/// Tabs grow to a width and stay side by side; New Tab is at the row's end.
#[gpui_kit::test]
fn tabs_sit_in_capsules_with_new_tab_after_them(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t");
    let [a, b] = ids(&browser, cx)[..] else {
        panic!("two tabs")
    };
    let bounds = |cx: &mut VisualTestContext| [a, b].map(|id| bounds_of(cx, format!("tab-{id}")));
    let tabs = bounds(cx);
    assert!(
        tabs.iter().all(|tab| tab.size.width == px(160.)),
        "Dia's width, with room to spare: {tabs:?}"
    );
    let apart = tabs[1].left() - tabs[0].right();
    assert_eq!(apart, px(6.), "two branches, two capsules: {tabs:?}");

    let row = cx.update(|_, cx| browser.read(cx).top_groups[0].as_u64());
    let new_tab = bounds_of(cx, format!("new-tab-{row}"));
    let after = new_tab.left() - tabs[1].right();
    assert!(
        after >= px(0.) && after < px(16.),
        "New Tab follows the last tab: {new_tab:?}"
    );

    // One branch shares a capsule: its tabs sit closer.
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            browser.tree.move_to(b, Place::Into(a));
            cx.notify();
        })
    });
    let tabs = bounds(cx);
    assert!(tabs[1].left() - tabs[0].right() < apart, "{tabs:?}");

    // A full row gives way, down to the narrowest a tab gets.
    keys(cx, "cmd-t cmd-t cmd-t cmd-t cmd-t cmd-t cmd-t");
    let widths: Vec<_> = ids(&browser, cx)
        .into_iter()
        .map(|id| bounds_of(cx, format!("tab-{id}")).size.width)
        .collect();
    assert!(
        widths
            .iter()
            .all(|width| *width < px(160.) && *width >= px(96.)),
        "{widths:?}"
    );
}

#[gpui_kit::test]
fn the_sidebar_draws_the_tabs_as_a_tree(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t cmd-t");
    let [a, b, c] = ids(&browser, cx)[..] else {
        panic!("three tabs")
    };
    // `c` under `a`, as if `a` had opened it.
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            assert!(browser.tree.move_to(c, Place::Into(a)));
            browser.sync_tree(cx);
        })
    });
    assert!(!drawn(cx, "sidebar".into()), "rows of tabs by default");

    keys(cx, "ctrl-cmd-s");
    assert!(drawn(cx, "sidebar".into()));
    assert!(!drawn(cx, format!("tab-{a}")), "the tree replaces the rows");
    let [row_a, row_b, row_c] = [a, b, c].map(|id| bounds_of(cx, format!("tree-tab-{id}")));
    assert!(row_a.top() < row_c.top() && row_c.top() < row_b.top());
    assert!(
        row_c.left() == row_a.left(),
        "rows share a left edge; their content is indented"
    );

    // The fold hides `c` and leaves the active tab alone.
    let before = active_id(&browser, cx);
    let fold = center_of(cx, format!("fold-{a}"));
    click(cx, fold);
    assert!(!drawn(cx, format!("tree-tab-{c}")));
    assert_eq!(active_id(&browser, cx), before);

    // A row's click makes its tab the active one.
    let row = center_of(cx, format!("tree-tab-{a}"));
    click(cx, row);
    assert_eq!(active_id(&browser, cx), a);
    // The shortcuts count the rows drawn: `c` is folded away.
    keys(cx, "cmd-2");
    assert_eq!(active_id(&browser, cx), b);

    // Going to a folded tab unfolds its branch.
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.picked(Pick::SwitchTab(c), window, cx)
        })
    });
    assert!(drawn(cx, format!("tree-tab-{c}")));

    keys(cx, "ctrl-cmd-s");
    assert!(!drawn(cx, "sidebar".into()));
    assert!(drawn(cx, format!("tab-{a}")));

    // The rows of tabs have a button to bring the sidebar back.
    let show = center_of(cx, "show-sidebar".into());
    click(cx, show);
    assert!(drawn(cx, "sidebar".into()));
    assert!(
        !drawn(cx, "show-sidebar".into()),
        "the sidebar has its own hide button"
    );
    let hide = center_of(cx, "hide-sidebar".into());
    click(cx, hide);
    assert!(!drawn(cx, "sidebar".into()));
}

#[gpui_kit::test]
fn dragging_a_row_onto_another_nests_it(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t cmd-t ctrl-cmd-s");
    let [a, b, c] = ids(&browser, cx)[..] else {
        panic!("three tabs")
    };
    let from = center_of(cx, format!("tree-tab-{c}"));
    let to = center_of(cx, format!("tree-tab-{a}"));
    drag(cx, from, to);
    let (children, roots) = cx.update(|_, cx| {
        let tree = &browser.read(cx).tree;
        (tree.children(a).to_vec(), tree.roots().to_vec())
    });
    assert_eq!(children, [c]);
    assert_eq!(roots, [a, b]);

    // Starting a drag moves no row: the pin target lies over the header.
    let before = bounds_of(cx, format!("tree-tab-{b}"));
    let held = Some(MouseButton::Left);
    let from = before.center();
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(from + point(px(8.), px(0.)), held, Modifiers::none());
    cx.run_until_parked();
    assert!(drawn(cx, "pin-drop".into()));
    assert_eq!(bounds_of(cx, format!("tree-tab-{b}")), before);
    // Dropped there, the tab is pinned and leaves the tree.
    let target = center_of(cx, "pin-drop".into());
    cx.simulate_mouse_move(target, held, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_mouse_up(target, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    // A start tab shows no site, so there is nothing to pin.
    assert!(cx.update(|_, cx| browser.read(cx).pins.is_empty()));
    assert!(drawn(cx, format!("tree-tab-{b}")));
}

#[gpui_kit::test]
fn closing_a_branch_closes_the_tabs_under_it(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t cmd-t cmd-t");
    let [a, b, c, d] = ids(&browser, cx)[..] else {
        panic!("four tabs")
    };
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            browser.tree.move_to(b, Place::Into(a));
            browser.tree.move_to(c, Place::Into(b));
            browser.sync_tree(cx);
        })
    });
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.picked(Pick::Tab(TabAction::CloseBranch(a)), window, cx)
        })
    });
    assert_eq!(ids(&browser, cx), [d]);
    assert_eq!(cx.update(|_, cx| browser.read(cx).tree.order()), [d]);
}

#[gpui_kit::test]
fn closing_a_tab_moves_the_tabs_under_it_up(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t cmd-t");
    let [a, b, c] = ids(&browser, cx)[..] else {
        panic!("three tabs")
    };
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            browser.tree.move_to(b, Place::Into(a));
            browser.tree.move_to(c, Place::Into(a));
            browser.sync_tree(cx);
        })
    });
    keys(cx, "ctrl-cmd-s");
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.picked(Pick::Tab(TabAction::Close(a)), window, cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(ids(&browser, cx), [b, c]);
    assert_eq!(
        cx.update(|_, cx| browser.read(cx).tree.rows()),
        [(b, 0), (c, 0)]
    );
    assert!(!drawn(cx, format!("tree-tab-{a}")));
    assert!(drawn(cx, format!("tree-tab-{b}")) && drawn(cx, format!("tree-tab-{c}")));
}

#[gpui_kit::test]
fn closing_a_branch_wakes_only_the_tab_left(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t cmd-t cmd-t cmd-t");
    let [a, b, c, d, e] = ids(&browser, cx)[..] else {
        panic!("five tabs")
    };
    assert_eq!(active_id(&browser, cx), e);
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.tree.move_to(b, Place::Into(a));
            browser.tree.move_to(c, Place::Into(a));
            for id in [b, c, d] {
                let index = browser.tab_index(id).unwrap();
                browser.tabs[index].asleep = Some(Content::Start);
            }
            // Taking a tab out passes the active tab on without waking it.
            let index = browser.tab_index(e).unwrap();
            assert!(browser.remove_tab(index, window, cx));
            assert!(browser.tabs[browser.active].asleep.is_some());
            let index = browser.tab_index(a).unwrap();
            browser.activate(index, window, cx);
        })
    });
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.picked(Pick::Tab(TabAction::CloseBranch(a)), window, cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(ids(&browser, cx), [d]);
    let woke = cx.update(|_, cx| browser.read(cx).tabs[0].asleep.is_none());
    assert!(woke, "the tab left is shown");
}

#[gpui_kit::test]
fn renaming_a_tab_in_a_folded_branch_shows_its_row(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t");
    let [a, b] = ids(&browser, cx)[..] else {
        panic!("two tabs")
    };
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            browser.tree.move_to(b, Place::Into(a));
            browser.tree.set_collapsed(a, true);
            browser.sync_tree(cx);
        })
    });
    // From its tab in the rows, with the sidebar shut.
    let tab = center_of(cx, format!("tab-{b}"));
    right_click(cx, tab);
    cx.simulate_input("rename");
    cx.run_until_parked();
    keys(cx, "enter");
    assert!(cx.update(|_, cx| browser.read(cx).sidebar));
    assert!(
        drawn(cx, format!("tree-tab-{b}")),
        "the row with the name field"
    );
}

#[gpui_kit::test]
fn kept_site_icons_show_for_pins_and_unloaded_tabs(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t");
    let [a, _] = ids(&browser, cx)[..] else {
        panic!("two tabs")
    };
    // A pin that redirects elsewhere, and a tab restored without its page.
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            browser.pins.push(Pin {
                id: 0,
                url: Url::parse("https://www.notion.so/").unwrap(),
                title: "Notion".into(),
            });
            let index = browser.tab_index(a).unwrap();
            browser.tabs[index].asleep = Some(Content::Page {
                url: Url::parse("https://example.org/").unwrap(),
                title: "Example Domain".into(),
            });
            cx.notify();
        })
    });
    assert!(!drawn(cx, "pin-icon-0".into()), "no icon kept yet");
    assert!(!drawn(cx, format!("site-icon-{a}")));

    let icon = Arc::new(RenderImage::new(vec![image::Frame::new(
        image::RgbaImage::new(1, 1),
    )]));
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            browser.icons.insert("www.notion.so".into(), icon.clone());
            browser.icons.insert("example.org".into(), icon);
            cx.notify();
        })
    });
    assert!(drawn(cx, "pin-icon-0".into()));
    assert!(drawn(cx, format!("site-icon-{a}")));
}

#[gpui_kit::test]
fn a_sidebar_row_dropped_on_a_pages_edge_splits_it(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t ctrl-cmd-s");
    let [a, _] = ids(&browser, cx)[..] else {
        panic!("two tabs")
    };
    let from = center_of(cx, format!("tree-tab-{a}"));
    drag(cx, from, point(px(900.), px(400.)));
    let shown = cx.update(|_, cx| browser.read(cx).displayed.len());
    assert_eq!(shown, 2, "side by side, as a tab dragged from the rows");
}

#[gpui_kit::test]
fn the_sidebars_edge_drags_its_width(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "ctrl-cmd-s");
    let sidebar = bounds_of(cx, "sidebar".into());
    assert_eq!(sidebar.size.width, px(240.));

    let edge = point(sidebar.right() - px(2.), sidebar.center().y);
    drag(cx, edge, edge + point(px(100.), px(0.)));
    let wider = bounds_of(cx, "sidebar".into()).size.width;
    assert!(
        (wider - px(340.)).abs() <= px(4.),
        "followed the pointer: {wider:?}"
    );

    let edge = point(px(f32::from(wider) - 2.), sidebar.center().y);
    drag(cx, edge, point(px(900.), edge.y));
    assert_eq!(
        bounds_of(cx, "sidebar".into()).size.width,
        px(480.),
        "no wider"
    );
    assert_eq!(
        cx.update(|_, cx| browser.read(cx).sidebar_width),
        px(480.),
        "the width the session saves"
    );
}

#[gpui_kit::test]
fn the_tray_pins_the_active_tab_and_tabs_dropped_on_it(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t cmd-t");
    let [_, b, c] = ids(&browser, cx)[..] else {
        panic!("three tabs")
    };
    // Two pages, as a restored session leaves them before they load.
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            for (id, url) in [(b, "https://example.org/"), (c, "https://example.com/")] {
                let index = browser.tab_index(id).unwrap();
                browser.tabs[index].asleep = Some(Content::Page {
                    url: Url::parse(url).unwrap(),
                    title: "Example".into(),
                });
            }
            cx.notify();
        })
    });
    assert!(
        drawn(cx, "pin-tray".into()),
        "the tray waits for a first pin"
    );

    let pin = center_of(cx, "pin-tab".into());
    click(cx, pin);
    assert!(
        drawn(cx, "pin-0".into()),
        "the active tab's site, in the tray"
    );
    assert!(!drawn(cx, format!("tab-{c}")), "its tab left the row");

    let from = center_of(cx, format!("tab-{b}"));
    let to = center_of(cx, "pin-tray".into());
    drag(cx, from, to);
    assert!(
        drawn(cx, "pin-1".into()),
        "a tab dropped on the tray is pinned"
    );
    assert_eq!(cx.update(|_, cx| browser.read(cx).pins.len()), 2);
}

#[gpui_kit::test]
fn a_pinned_site_owns_a_tab_outside_the_tree(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t");
    let [a, b] = ids(&browser, cx)[..] else {
        panic!("two tabs")
    };
    // As pinning a tab on the site would leave it.
    cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            browser.pins.push(Pin {
                id: 0,
                url: Url::parse("https://example.org/").unwrap(),
                title: "Example".into(),
            });
            browser.next_pin = 1;
            let index = browser.tab_index(b).unwrap();
            browser.tabs[index].pin = Some(0);
            browser.tree.remove(b);
            browser.sync_tree(cx);
            cx.notify();
        })
    });
    // In the rows of tabs, the site's button stands for its tab.
    assert!(drawn(cx, "pin-0".into()));
    assert!(!drawn(cx, format!("tab-{b}")));
    assert!(drawn(cx, format!("tab-{a}")));

    keys(cx, "ctrl-cmd-s");
    assert!(drawn(cx, "pins".into()));
    assert!(!drawn(cx, format!("tree-tab-{b}")));
    keys(cx, "cmd-2");
    assert_eq!(active_id(&browser, cx), a, "the pinned tab counts first");
    let pin = center_of(cx, "pin-0".into());
    click(cx, pin);
    assert_eq!(active_id(&browser, cx), b, "the button goes to its tab");

    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.tab_action(TabAction::Unpin(0), window, cx)
        })
    });
    assert!(drawn(cx, format!("tree-tab-{b}")), "back in the tree");
    assert!(!drawn(cx, "pins".into()), "no pins left to draw");
}

#[gpui_kit::test]
fn the_tree_and_the_sidebar_survive_a_restart(cx: &mut TestAppContext) {
    let dir = data_dir("restart");
    std::fs::write(
        dir.join("session.json"),
        r#"{
            "version": 1,
            "sidebar": true,
            "active": 7,
            "tabs": [
                { "id": 4, "name": "Browser research", "collapsed": false },
                { "id": 7, "parent": 4, "opener": 4 },
                { "id": 9 }
            ],
            "dock": [9, 4, 7]
        }"#,
    )
    .unwrap();
    cx.update(|cx| session::init(&dir, cx));
    let (browser, cx) = open(cx);

    assert_eq!(ids(&browser, cx), [9, 4, 7], "in the dock's order");
    assert_eq!(active_id(&browser, cx), 7);
    let (order, parent, name) = cx.update(|_, cx| {
        let tree = &browser.read(cx).tree;
        (
            tree.order(),
            tree.node(7).unwrap().parent,
            tree.node(4).unwrap().name.clone(),
        )
    });
    assert_eq!(order, [4, 7, 9]);
    assert_eq!(parent, Some(4));
    assert_eq!(name.as_deref(), Some("Browser research"));
    assert!(drawn(cx, "sidebar".into()));

    // A change is written once things settle.
    keys(cx, "cmd-t");
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    let saved = std::fs::read_to_string(dir.join("session.json")).unwrap();
    let saved = session::parse_session(&saved).expect("Platinum's session");
    assert_eq!(saved.tabs.len(), 4);
    assert!(saved.sidebar);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A layout's two side-by-side groups, and the left one's share of the width.
fn halves(layout: &Layout) -> (&Layout, &Layout, f32) {
    let Layout::Split {
        vertical: false,
        children,
    } = layout
    else {
        panic!("side by side: {layout:?}")
    };
    let [(left, Some(l)), (right, Some(r))] = &children[..] else {
        panic!("two groups, each drawn at a size: {children:?}")
    };
    (left, right, l / (l + r))
}

#[gpui_kit::test]
fn a_split_survives_a_restart(cx: &mut TestAppContext) {
    let dir = data_dir("split-restart");
    // Saved from a wider window than the test's 1000pt.
    std::fs::write(
        dir.join("session.json"),
        r#"{
            "version": 1,
            "active": 3,
            "tabs": [{ "id": 1 }, { "id": 2 }, { "id": 3 }],
            "dock": {
                "split": "horizontal",
                "sizes": [900, 600],
                "children": [
                    { "tabs": [1, 2], "shown": 1 },
                    { "tabs": [3], "shown": 0 }
                ]
            }
        }"#,
    )
    .unwrap();
    cx.update(|cx| session::init(&dir, cx));
    let (browser, cx) = open(cx);

    assert_eq!(ids(&browser, cx), [1, 2, 3]);
    assert_eq!(active_id(&browser, cx), 3);
    let shown: Vec<u64> = cx.update(|_, cx| {
        let browser = browser.read(cx);
        let tab_of = |panel| Some(browser.tabs[browser.panel_index(panel)?].id);
        browser
            .displayed
            .iter()
            .filter_map(|panel| tab_of(*panel))
            .collect()
    });
    assert_eq!(shown, [2, 3], "each group shows the tab it did");
    let layout = cx.update(|_, cx| browser.read(cx).layout(cx));
    let (left, right, share) = halves(&layout);
    let group = |tabs: &[u64], shown| Layout::Group {
        tabs: tabs.to_vec(),
        shown,
    };
    assert_eq!(left, &group(&[1, 2], 1));
    assert_eq!(right, &group(&[3], 0));
    assert!(
        (share - 0.6).abs() < 0.02,
        "the left card keeps its share of a narrower window: {share}"
    );

    // A new tab joins the right-hand group; the split is written as it is.
    keys(cx, "cmd-t");
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    let saved = std::fs::read_to_string(dir.join("session.json")).unwrap();
    let saved = session::parse_session(&saved).expect("Platinum's session");
    let (left, right, share) = halves(&saved.dock);
    assert_eq!(left, &group(&[1, 2], 1));
    let Layout::Group { tabs, shown: 1 } = right else {
        panic!("the new tab shown beside 3: {right:?}")
    };
    assert_eq!(tabs[0], 3);
    assert!((share - 0.6).abs() < 0.02, "saved at its share: {share}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
fn without_a_saved_active_tab_each_group_keeps_its_tab(cx: &mut TestAppContext) {
    let dir = data_dir("no-active");
    std::fs::write(
        dir.join("session.json"),
        r#"{
            "version": 1,
            "tabs": [{ "id": 1, "search": "one" }, { "id": 2, "search": "two" }],
            "dock": { "tabs": [1, 2], "shown": 1 }
        }"#,
    )
    .unwrap();
    cx.update(|cx| session::init(&dir, cx));
    let (browser, cx) = open(cx);
    cx.run_until_parked();

    assert_eq!(active_id(&browser, cx), 2, "the tab its group showed");
    let asleep: Vec<bool> = cx.update(|_, cx| {
        let browser = browser.read(cx);
        browser
            .tabs
            .iter()
            .map(|tab| tab.asleep.is_some())
            .collect()
    });
    assert_eq!(asleep, [true, false], "only that one woke");
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
fn cycling_from_a_folded_branch_goes_on_from_its_row(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t cmd-t cmd-t ctrl-cmd-s");
    let [_, b, c, d] = ids(&browser, cx)[..] else {
        panic!("four tabs")
    };
    // c hangs under b; b is folded while c is the active tab.
    let fold_over_c = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.tree.move_to(c, Place::Into(b));
                browser.activate_id(c, window, cx);
                browser.tab_action(TabAction::Fold(b), window, cx);
            })
        })
    };
    fold_over_c(cx);
    keys(cx, "ctrl-tab");
    assert_eq!(active_id(&browser, cx), d, "the row after the branch");
    fold_over_c(cx);
    keys(cx, "ctrl-shift-tab");
    assert_eq!(active_id(&browser, cx), b, "the branch's own row");
}

#[gpui_kit::test]
fn a_site_is_pinned_once(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t");
    let [a, b] = ids(&browser, cx)[..] else {
        panic!("two tabs")
    };
    let (offered, pins) = cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            let site = Content::Page {
                url: Url::parse("https://example.org/").unwrap(),
                title: "Example".into(),
            };
            for id in [a, b] {
                let index = browser.tab_index(id).unwrap();
                browser.tabs[index].asleep = Some(site.clone());
            }
            browser.pin_tab(a, cx);
            let other = browser.tab_index(b).unwrap();
            let offered = browser.pinnable_url(other, cx).is_some();
            browser.pin_tab(b, cx);
            (offered, browser.pins.len())
        })
    });
    assert!(!offered, "Pin Tab is not offered for a site pinned already");
    assert_eq!(pins, 1);
}

#[gpui_kit::test]
fn a_stream_of_changes_cannot_put_the_save_off(cx: &mut TestAppContext) {
    let dir = data_dir("save-stream");
    cx.update(|cx| session::init(&dir, cx));
    let (browser, cx) = open_with_tab(cx);
    // A change every 0.4s, as a pointer over links makes them.
    for width in [200., 210., 220., 230., 240., 250.] {
        cx.update(|_, cx| {
            browser.update(cx, |browser, cx| {
                browser.sidebar_width = px(width);
                browser.schedule_save(cx);
            })
        });
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
    }
    let saved = std::fs::read_to_string(dir.join("session.json"))
        .expect("written while the changes kept coming");
    assert!(session::parse_session(&saved).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
fn console_panel_opens_and_closes(cx: &mut TestAppContext) {
    let (_browser, cx) = open_with_tab(cx);
    assert!(cx.debug_bounds("Console").is_none());
    keys(cx, "alt-cmd-j");
    assert!(cx.debug_bounds("Console").is_some());
    assert!(cx.debug_bounds("Elements").is_some());
    assert!(cx.debug_bounds("Network").is_some());
    assert!(cx.debug_bounds("Tools").is_some());
    keys(cx, "alt-cmd-j");
    assert!(cx.debug_bounds("Console").is_none());
}

#[gpui_kit::test]
fn imported_history_appears_when_typing_in_address_bar_and_has_a_keyboard_path(
    cx: &mut TestAppContext,
) {
    let temp = tempfile::tempdir().unwrap();
    let store = frame_profile::ProfileStore::open(temp.path()).unwrap();
    store
        .visit(
            "https://docs.example.test/",
            "Familiar project documentation",
        )
        .unwrap();
    store
        .bookmark("https://bookmarks.example.test/", "Familiar bookmark")
        .unwrap();
    cx.update(|cx| cx.set_global(crate::profile_ui::ActiveProfile(store)));
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-l");
    cx.simulate_input("Familiar");
    cx.run_until_parked();
    let suggestions = cx.update(|_, cx| {
        browser
            .read(cx)
            .suggestions
            .iter()
            .map(|e| e.url.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(suggestions.len(), 2);
    assert!(suggestions.contains(&"https://docs.example.test/".into()));
    keys(cx, "down");
    assert!(cx.update(|window, cx| {
        browser
            .read(cx)
            .suggestion_state
            .focus_handle(cx)
            .is_focused(window)
    }));
    keys(cx, "escape");
    assert!(cx.update(|_, cx| browser.read(cx).suggestions.is_empty()));
    assert!(in_address_bar(&browser, cx));
}

#[gpui_kit::test]
fn history_command_opens_native_profile_library(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let temp = tempfile::tempdir().unwrap();
    let store = frame_profile::ProfileStore::open(temp.path()).unwrap();
    cx.update(|cx| cx.set_global(crate::profile_ui::ActiveProfile(store)));
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-y");
    assert!(cx.update(|window, cx| window.has_active_sheet(cx)));
    assert!(cx.update(|_, cx| browser.read(cx).profile_panel.is_some()));
    keys(cx, "escape");
    assert!(!cx.update(|window, cx| window.has_active_sheet(cx)));
}

#[gpui_kit::test]
fn home_status_bar_pins_import_to_bottom_left_and_opens_importer(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let temp = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        cx.set_global(crate::profile_ui::ActiveProfile(
            frame_profile::ProfileStore::open(temp.path()).unwrap(),
        ))
    });
    let (browser, cx) = open(cx);
    let bar = bounds_of(cx, "home-status-bar-0".into());
    let button = bounds_of(cx, "import-browser-data-0".into());
    assert!(
        bar.origin.y > px(600.),
        "bar is at the bottom of the home pane"
    );
    assert!(
        button.origin.x < bar.origin.x + px(16.),
        "import is pinned to the left"
    );
    assert!(button.bottom() <= bar.bottom());
    click(cx, button.center());
    assert!(cx.update(|window, cx| window.has_active_sheet(cx)));
    assert!(cx.update(|_, cx| browser.read(cx).profile_panel.is_some()));
    keys(cx, "escape");
    assert!(!cx.update(|window, cx| window.has_active_sheet(cx)));

    // Search results use a real native Serp; without a search service the
    // fixture settles into its unavailable state without any network access.
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.search(
                "fixture".into(),
                crate::serp::SerpMode::Search,
                false,
                window,
                cx,
            )
        })
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("home-status-bar-0").is_none());
    keys(cx, "cmd-t");
    assert!(cx.debug_bounds("home-status-bar-1").is_some());
    keys(cx, "cmd-1");
    assert!(cx.debug_bounds("home-status-bar-0").is_none());
    assert!(cx.debug_bounds("home-status-bar-1").is_none());
}

#[gpui_kit::test]
fn visual_chrome_updates_do_not_write_the_session(cx: &mut TestAppContext) {
    let dir = data_dir("visual-chrome");
    cx.update(|cx| session::init(&dir, cx));
    let (browser, cx) = open_with_tab(cx);
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            // What is written now is what the next save compares with.
            assert!(browser.snapshot(true, cx).is_some());
            for _ in 0..100 {
                browser.sync_chrome(window, cx);
            }
            assert!(browser.snapshot(false, cx).is_none());
        })
    });
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
fn closing_before_the_save_keeps_the_latest_tabs(cx: &mut TestAppContext) {
    let dir = data_dir("close-before-save");
    cx.update(|cx| session::init(&dir, cx));
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t cmd-t");
    let active = cx.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            browser.save(cx);
            browser.tabs[browser.active].id
        })
    });
    let saved = cx
        .update(|_, cx| cx.global::<session::Store>().load_session())
        .expect("written on close");
    assert_eq!(saved.tabs.len(), 3);
    assert_eq!(saved.active, Some(active));
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
fn frame_opens_on_the_workspace_and_new_tab_leaves_it(cx: &mut TestAppContext) {
    let (browser, cx) = open(cx);
    assert!(workspace(&browser, cx));
    assert_eq!(route(&browser, cx), "Home");
    assert!(cx.debug_bounds("underlay").is_some());
    assert!(cx.debug_bounds("address-bar").is_none(), "no tab is open");
    assert!(in_workspace_field(&browser, cx), "ready for a search");

    // There is no tab to close or switch to.
    keys(cx, "cmd-w ctrl-tab cmd-1");
    assert!(workspace(&browser, cx));
    assert_eq!(tabs(&browser, cx), (1, 0));

    // ⌘T opens the tab behind it, ready for an address.
    keys(cx, "cmd-t");
    assert!(!workspace(&browser, cx));
    assert_eq!(tabs(&browser, cx), (1, 0));
    assert!(cx.debug_bounds("underlay").is_none());
    assert!(in_address_bar(&browser, cx));
}

#[gpui_kit::test]
fn closing_the_last_tab_returns_to_the_workspace_at_home(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t");
    assert_eq!(tabs(&browser, cx), (2, 1));
    keys(cx, "cmd-w cmd-w");
    assert!(workspace(&browser, cx));
    assert_eq!(route(&browser, cx), "Home");
    assert!(in_workspace_field(&browser, cx));
}

#[gpui_kit::test]
fn a_search_in_the_workspace_shows_results_with_their_own_history(cx: &mut TestAppContext) {
    let (browser, cx) = open(cx);
    cx.simulate_input("tides and the moon");
    keys(cx, "enter");
    assert!(workspace(&browser, cx), "results show in the workspace");
    assert_eq!(route(&browser, cx), "tides and the moon");
    let searches = cx.update(|_, cx| crate::recents::searches(cx).to_vec());
    assert_eq!(searches[0].query, "tides and the moon", "Home lists it");

    keys(cx, "cmd-[");
    assert_eq!(route(&browser, cx), "Home");
    assert!(in_workspace_field(&browser, cx));
    keys(cx, "cmd-]");
    assert_eq!(route(&browser, cx), "tides and the moon");
}

/// The place the window shows, if one is.
fn place(browser: &Entity<Browser>, cx: &mut VisualTestContext) -> Option<NativePlace> {
    cx.update(|_, cx| browser.read(cx).place)
}

/// Open a place from the palette.
fn open_from_palette(cx: &mut VisualTestContext, name: &str) {
    keys(cx, "cmd-k");
    cx.simulate_input(name);
    cx.run_until_parked();
    keys(cx, "enter");
}

/// Open the Agent place from the palette.
fn open_agent(cx: &mut VisualTestContext) {
    open_from_palette(cx, "agent");
}

#[gpui_kit::test]
fn a_place_shows_over_the_workspace_and_its_bump_moves_between_places(cx: &mut TestAppContext) {
    let (browser, cx) = open(cx);
    assert!(
        !drawn(cx, "places-bump".into()),
        "Home lists the places itself"
    );
    open_from_palette(cx, "mail");
    assert_eq!(place(&browser, cx), Some(NativePlace::Mail));
    assert!(workspace(&browser, cx), "the workspace stays beneath");
    assert!(cx.debug_bounds("underlay").is_none());
    assert!(drawn(cx, "places-bump".into()));

    // The bump moves between the places and back out.
    let at = center_of(cx, "bump-library".into());
    click(cx, at);
    assert_eq!(place(&browser, cx), Some(NativePlace::Library));
    let at = center_of(cx, "bump-exit".into());
    click(cx, at);
    assert_eq!(place(&browser, cx), None);
    assert_eq!(route(&browser, cx), "Home");
    assert!(in_workspace_field(&browser, cx));

    // Back leaves a place for what is beneath it, as it was.
    cx.simulate_input("tides and the moon");
    keys(cx, "enter");
    assert_eq!(route(&browser, cx), "tides and the moon");
    open_from_palette(cx, "library");
    keys(cx, "cmd-[");
    assert_eq!(place(&browser, cx), None);
    assert_eq!(route(&browser, cx), "tides and the moon");
    // The bump's way out goes Home.
    open_from_palette(cx, "library");
    let at = center_of(cx, "bump-exit".into());
    click(cx, at);
    assert_eq!(route(&browser, cx), "Home");
}

#[gpui_kit::test]
fn a_place_shows_over_the_tabs_without_their_chrome(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    let [tab] = ids(&browser, cx)[..] else {
        panic!("one tab")
    };
    assert!(drawn(cx, format!("tab-{tab}")));
    assert!(cx.debug_bounds("address-bar").is_some(), "Home searches");
    open_agent(cx);
    assert_eq!(place(&browser, cx), Some(NativePlace::Agent));
    assert!(drawn(cx, "theorem-card".into()));
    assert!(drawn(cx, "places-bump".into()));
    assert!(!drawn(cx, format!("tab-{tab}")), "no tab row over a place");
    assert!(
        cx.debug_bounds("address-bar").is_none(),
        "the agent is the application's, not a page"
    );

    // ⌘W closes the place, not the tab beneath it.
    keys(cx, "cmd-w");
    assert_eq!(place(&browser, cx), None);
    assert_eq!(tabs(&browser, cx), (1, 0));
    assert!(showing(&browser, cx) == Showing::Start);

    // An address leaves the place for the tab's address bar.
    open_agent(cx);
    keys(cx, "cmd-l");
    assert_eq!(place(&browser, cx), None);
    assert_eq!(tabs(&browser, cx), (1, 0));
    assert!(in_address_bar(&browser, cx));

    // Over the tabs, the bump's way out shows them.
    open_from_palette(cx, "notes");
    let at = center_of(cx, "bump-exit".into());
    click(cx, at);
    assert_eq!(place(&browser, cx), None);
    assert!(drawn(cx, format!("tab-{tab}")));
    assert!(!workspace(&browser, cx));
}

#[gpui_kit::test]
fn the_agent_place_is_theorem_with_the_keyboard_in_it(cx: &mut TestAppContext) {
    let (browser, cx) = open(cx);
    open_agent(cx);
    assert_eq!(place(&browser, cx), Some(NativePlace::Agent));
    assert!(drawn(cx, "theorem-navigator".into()));
    assert!(drawn(cx, "theorem-card".into()));
    let bump = bounds_of(cx, "places-bump".into());
    let card = bounds_of(cx, "theorem-card".into());
    let navigator = bounds_of(cx, "theorem-navigator".into());
    assert!(
        bump.left() == navigator.left() && bump.right() <= card.left(),
        "the bump stands on the window's leading edge, clear of the card: \
         {bump:?} beside {card:?}"
    );
    let in_theorem = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let (workbench, _) = browser
                .read(cx)
                .theorem
                .as_ref()
                .expect("made as it opened");
            workbench.focus_handle(cx).contains_focused(window, cx)
        })
    };
    assert!(in_theorem(cx), "the keyboard goes to Theorem");

    // Back reaches the browser from inside the workbench.
    keys(cx, "cmd-[");
    assert_eq!(place(&browser, cx), None);
    assert_eq!(route(&browser, cx), "Home");
    assert!(!drawn(cx, "theorem-navigator".into()));
    open_agent(cx);
    assert!(in_theorem(cx));
}

#[gpui_kit::test]
fn the_crm_place_is_the_record_workspace(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    open_from_palette(cx, "index");
    assert_eq!(place(&browser, cx), Some(NativePlace::Crm));
    assert!(drawn(cx, "crm-shell".into()));
    assert!(drawn(cx, "crm-card".into()));
    assert!(drawn(cx, "crm-nav".into()));
    assert!(drawn(cx, "places-bump".into()));
    assert!(
        cx.debug_bounds("address-bar").is_none(),
        "the CRM is the application's, not a page"
    );
    let bump = bounds_of(cx, "places-bump".into());
    let card = bounds_of(cx, "crm-card".into());
    let navigator = bounds_of(cx, "crm-nav".into());
    assert!(
        bump.left() == navigator.left() && bump.right() <= card.left(),
        "the bump stands on the window's leading edge, clear of the card: \
         {bump:?} beside {card:?}"
    );
}

#[gpui_kit::test]
fn always_show_places_stands_the_bump_over_home_and_tabs_and_is_saved(cx: &mut TestAppContext) {
    let dir = data_dir("places-pinned");
    cx.update(|cx| session::init(&dir, cx));
    let (browser, cx) = open(cx);
    assert!(!drawn(cx, "places-bump".into()));
    keys(cx, "cmd-k");
    cx.simulate_input("always show places");
    cx.run_until_parked();
    keys(cx, "enter");
    assert!(cx.update(|_, cx| browser.read(cx).places_pinned));
    assert!(drawn(cx, "places-bump".into()), "over Home");
    assert!(!drawn(cx, "bump-exit".into()), "nothing to leave");
    keys(cx, "cmd-t");
    assert!(drawn(cx, "places-bump".into()), "over the tabs");

    // A bump opens a place; there it leads with its way out.
    let at = center_of(cx, "bump-notes".into());
    click(cx, at);
    assert_eq!(place(&browser, cx), Some(NativePlace::Notes));
    assert!(drawn(cx, "bump-exit".into()));

    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    let saved = std::fs::read_to_string(dir.join("session.json")).unwrap();
    let saved = session::parse_session(&saved).expect("Platinum's session");
    assert!(saved.places_pinned);
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
fn command_return_searches_in_a_tab_from_workspace(cx: &mut TestAppContext) {
    let (browser, cx) = open(cx);
    cx.simulate_input("tides and the moon");
    keys(cx, "cmd-enter");
    assert!(!workspace(&browser, cx));
    assert_eq!(showing(&browser, cx), Showing::Serp);
    assert_eq!(tabs(&browser, cx), (1, 0));
}

#[gpui_kit::test]
fn command_click_recent_search_opens_a_results_tab(cx: &mut TestAppContext) {
    let (browser, cx) = open(cx);
    cx.update(|_, cx| crate::recents::record_search("tides", false, cx));
    cx.run_until_parked();
    let row = cx
        .debug_bounds("recent-search:search:tides")
        .expect("recent search")
        .center();
    cx.simulate_click(
        row,
        Modifiers {
            platform: true,
            ..Modifiers::none()
        },
    );
    cx.run_until_parked();
    assert!(!workspace(&browser, cx));
    assert_eq!(showing(&browser, cx), Showing::Serp);
}

#[gpui_kit::test]
fn downloading_a_workspace_result_preserves_its_history(cx: &mut TestAppContext) {
    let (browser, cx) = open(cx);
    cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            let (serp, events) = browser.new_serp(
                "downloads".into(),
                crate::serp::SerpMode::Search,
                window,
                cx,
            );
            browser.underlay_go(
                super::underlay::Route::Results(serp.clone(), events),
                window,
                cx,
            );
            let url = Url::parse("http://127.0.0.1:9/file.zip").unwrap();
            assert!(browser.underlay_opened(&serp, &url, true, window, cx));
        })
    });
    cx.run_until_parked();
    assert!(workspace(&browser, cx));
    assert_eq!(route(&browser, cx), "downloads");
    keys(cx, "cmd-[");
    assert_eq!(route(&browser, cx), "Home");
}

#[gpui_kit::test]
fn workspace_backing_tab_is_not_saved_as_an_open_tab(cx: &mut TestAppContext) {
    let dir = data_dir("workspace-empty");
    cx.update(|cx| session::init(&dir, cx));
    let (browser, cx) = open(cx);
    cx.update(|_, cx| browser.update(cx, |browser, cx| browser.save(cx)));
    let saved = cx.update(|_, cx| session::Store::global(cx).unwrap().load_session().unwrap());
    assert!(saved.tabs.is_empty());
    assert_eq!(saved.active, None);
}

#[gpui_kit::test]
fn workspace_results_keep_the_query_in_the_saved_session(cx: &mut TestAppContext) {
    let dir = data_dir("workspace-search");
    cx.update(|cx| session::init(&dir, cx));
    let (browser, cx) = open(cx);
    cx.simulate_input("native search");
    keys(cx, "enter");
    cx.update(|_, cx| browser.update(cx, |browser, cx| browser.save(cx)));
    let saved = cx.update(|_, cx| session::Store::global(cx).unwrap().load_session().unwrap());
    assert_eq!(saved.tabs.len(), 1);
    assert_eq!(
        saved.tabs[0].content,
        Content::Search {
            query: "native search".into(),
            ask: false
        }
    );
    assert_eq!(saved.active, Some(saved.tabs[0].id));
}

#[gpui_kit::test]
fn tab_shortcut_leaves_place_and_restores_board_composer(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t");
    cx.update(|window, cx| {
        browser.update(cx, |b, cx| {
            b.toggle_canvas(window, cx);
            b.open_place(NativePlace::Agent, window, cx);
            assert!(b.canvas_shown);
            assert!(
                b.theorem
                    .as_ref()
                    .unwrap()
                    .0
                    .read(cx)
                    .board_slot()
                    .is_none()
            );
        })
    });
    keys(cx, "cmd-1");
    cx.update(|_, cx| {
        let b = browser.read(cx);
        assert!(b.place.is_none());
        assert!(b.canvas_shown);
        assert!(
            b.theorem
                .as_ref()
                .unwrap()
                .0
                .read(cx)
                .board_slot()
                .is_some()
        );
    });
    assert!(!drawn(cx, "theorem-card".into()));
    assert!(drawn(cx, "theorem-board-composer".into()));
}

#[gpui_kit::test]
fn closed_tab_ids_are_not_reused_after_restart(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    cx.update(|cx| session::init(root.path(), cx));
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t");
    let closed = *ids(&browser, cx).last().unwrap();
    keys(cx, "cmd-w");
    let saved = cx.update(|_, cx| {
        browser.update(cx, |b, cx| {
            b.save(cx);
            b.written.clone().unwrap().0
        })
    });
    assert!(saved.next_tab_id.unwrap() > closed);
    // Load the actual persisted allocator in a fresh browser entity.
    let replacement = cx.update(|window, cx| cx.new(|cx| Browser::new(window, cx)));
    cx.update(|window, cx| {
        replacement.update(cx, |b, cx| {
            assert_eq!(b.visit_session, saved.visit_session);
            b.new_tab_beside(b.active, window, cx);
            assert!(b.tabs[b.active].id > closed);
        })
    });
}

#[gpui_kit::test]
fn promoting_from_expanded_notes_restores_the_board_composer(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let profile = frame_profile::ProfileStore::open(root.path()).unwrap();
    cx.update(|cx| cx.set_global(crate::profile_ui::ActiveProfile(profile)));
    let (browser, cx) = open_with_tab(cx);
    cx.update(|window, cx| {
        browser.update(cx, |b, cx| {
            b.toggle_canvas(window, cx);
            b.open_place(NativePlace::Agent, window, cx);
            b.open_place(NativePlace::Notes, window, cx);
            b.notebook
                .as_ref()
                .unwrap()
                .update(cx, |notes, cx| notes.create(None, window, cx));
        })
    });
    cx.run_until_parked();
    let note = cx.update(|_, cx| {
        browser
            .read(cx)
            .notebook
            .as_ref()
            .unwrap()
            .read(cx)
            .selected()
            .unwrap()
    });
    cx.update(|_, cx| {
        browser
            .read(cx)
            .notebook
            .as_ref()
            .unwrap()
            .clone()
            .update(cx, |_, cx| {
                cx.emit(crate::notes::NotesEvent::Promote(note.clone()));
            });
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let b = browser.read(cx);
        assert!(b.place.is_none());
        assert!(b.canvas_shown);
        assert!(
            b.theorem
                .as_ref()
                .unwrap()
                .0
                .read(cx)
                .board_slot()
                .is_some()
        );
        let placement = &b.canvas.as_ref().unwrap().read(cx).state(cx).notes[0];
        assert_eq!(
            b.canvas
                .as_ref()
                .unwrap()
                .read(cx)
                .note(placement.id)
                .unwrap()
                .entity_id(),
            note.entity_id()
        );
    });
    assert!(!drawn(cx, "theorem-card".into()));
    assert!(drawn(cx, "theorem-board-composer".into()));
}

mod dictation_shortcuts {
    use super::*;
    use gpui_kit::component::speech::SpeechState;
    use gpui_kit::{Context, KeyDownEvent, KeyUpEvent, Keystroke};

    use crate::voice::fixture::{Input, Recognizer};
    fn fixture(b: &mut Browser, window: &mut gpui_kit::Window, cx: &mut Context<Browser>) -> Input {
        let input = Input::default();
        b.speech = cx.new(|cx| {
            SpeechState::new(cx)
                .recognizer(Recognizer)
                .input(input.clone())
                .system_fallback(false)
        });
        b._speech_events = cx.subscribe_in(&b.speech, window, |b, _, event, window, cx| {
            b.speech_event(event, window, cx)
        });
        input
    }
    fn modifiers(browser: &Entity<Browser>, cx: &mut VisualTestContext, left: bool, right: bool) {
        cx.update(|window, cx| {
            browser.update(cx, |b, cx| {
                b.voice_modifiers(
                    Modifiers {
                        platform: left || right,
                        ..Modifiers::none()
                    },
                    left,
                    right,
                    window,
                    cx,
                )
            })
        });
        cx.run_until_parked();
    }
    fn held(cx: &mut VisualTestContext) {
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn left_command_hold_releases_capture_and_inserts_once(cx: &mut TestAppContext) {
        let (browser, cx) = open_with_tab(cx);
        let input = cx.update(|window, cx| browser.update(cx, |b, cx| fixture(b, window, cx)));
        modifiers(&browser, cx, false, true);
        held(cx);
        assert_eq!(input.starts.get(), 0, "right Command must not capture");
        modifiers(&browser, cx, false, false);
        modifiers(&browser, cx, true, false);
        modifiers(&browser, cx, false, false);
        held(cx);
        assert_eq!(input.starts.get(), 0, "a quick tap must not start later");
        modifiers(&browser, cx, true, false);
        held(cx);
        assert_eq!(input.starts.get(), 1);
        modifiers(&browser, cx, false, false);
        assert_eq!(input.stops.get(), 1);
        let text = cx.update(|_, cx| browser.read(cx).omnibox.read(cx).value().to_string());
        assert_eq!(text, "spoken words");
        modifiers(&browser, cx, false, false);
        assert_eq!(
            cx.update(|_, cx| browser.read(cx).omnibox.read(cx).value().to_string()),
            text
        );
        assert_eq!(input.starts.get(), 1);
    }

    #[gpui_kit::test]
    fn modifier_release_finishes_through_gpui_dispatch(cx: &mut TestAppContext) {
        let (browser, cx) = open_with_tab(cx);
        let input = cx.update(|window, cx| browser.update(cx, |b, cx| fixture(b, window, cx)));
        // Supply the side information separately: headless events cannot press
        // a physical macOS key. The release still uses GPUI's real dispatch,
        // including its synthesized modifier-only keystroke.
        cx.simulate_event(gpui_kit::ModifiersChangedEvent {
            modifiers: Modifiers {
                platform: true,
                ..Modifiers::none()
            },
            ..Default::default()
        });
        modifiers(&browser, cx, true, false);
        held(cx);
        assert_eq!(input.starts.get(), 1);
        cx.simulate_event(gpui_kit::ModifiersChangedEvent::default());
        cx.run_until_parked();
        assert_eq!(input.stops.get(), 1);
        assert_eq!(
            cx.update(|_, cx| browser.read(cx).omnibox.read(cx).value().to_string()),
            "spoken words"
        );
    }

    #[gpui_kit::test]
    fn command_shortcuts_and_escape_cancel_hold_without_delayed_restart(cx: &mut TestAppContext) {
        let (browser, cx) = open_with_tab(cx);
        let input = cx.update(|window, cx| browser.update(cx, |b, cx| fixture(b, window, cx)));
        modifiers(&browser, cx, true, false);
        keys(cx, "cmd-l");
        modifiers(&browser, cx, true, false);
        held(cx);
        assert_eq!(input.starts.get(), 0);
        modifiers(&browser, cx, false, false);
        modifiers(&browser, cx, true, false);
        held(cx);
        assert_eq!(input.starts.get(), 1);
        keys(cx, "escape");
        modifiers(&browser, cx, false, false);
        assert_eq!(input.stops.get(), 1);
        assert!(cx.update(|_, cx| browser.read(cx).omnibox.read(cx).value().is_empty()));
        modifiers(&browser, cx, true, false);
        held(cx);
        assert_eq!(input.starts.get(), 2);
        cx.deactivate_window();
        cx.run_until_parked();
        assert_eq!(input.stops.get(), 2, "losing the window must stop capture");
    }

    #[gpui_kit::test]
    fn edited_hotkey_holds_through_key_events_and_persists(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        cx.update(|cx| session::init(root.path(), cx));
        let (browser, cx) = open_with_tab(cx);
        let input = cx.update(|window, cx| {
            browser.update(cx, |b, cx| {
                b.set_voice_shortcut("cmd-l", window, cx);
                assert_eq!(b.speaker.preferences.dictation_hotkey, "left-cmd");
                b.set_voice_shortcut("ctrl-shift-space", window, cx);
                b.save(cx);
                fixture(b, window, cx)
            })
        });
        let key = Keystroke::parse("ctrl-shift-space").unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke: key.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        held(cx);
        assert_eq!(input.starts.get(), 1);
        cx.simulate_event(KeyDownEvent {
            keystroke: key.clone(),
            is_held: true,
            prefer_character_input: false,
        });
        assert_eq!(input.starts.get(), 1);
        cx.simulate_event(KeyUpEvent { keystroke: key });
        cx.run_until_parked();
        assert_eq!(input.stops.get(), 1);
        assert_eq!(
            cx.update(|_, cx| browser.read(cx).omnibox.read(cx).value().to_string()),
            "spoken words"
        );
        let replacement = cx.update(|window, cx| cx.new(|cx| Browser::new(window, cx)));
        assert_eq!(
            cx.update(|_, cx| replacement
                .read(cx)
                .speaker
                .preferences
                .dictation_hotkey
                .clone()),
            "ctrl-shift-space"
        );
    }
}
