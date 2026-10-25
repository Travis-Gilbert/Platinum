//! UI integration tests of the chrome in headless windows. Pages need Servo
//! and results need the network, so these cover what is Frame's own: the
//! workspace beneath the tabs, the places, the right-click search, the
//! palette, tab shortcuts, dragging tabs, the Ask button, the appearance
//! button, the downloads list and compact mode.

use gpui_kit::component::{Root, Theme};
use gpui_kit::{
    AppContext as _, Bounds, Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point,
    TestAppContext, VisualTestContext, point, px, size,
};

use super::{Browser, Showing};
use crate::downloads::DownloadState;
use crate::navigator::Placement;
use crate::places::Place;
use crate::theme::{self, Appearance};

/// Frame as it launches: on the workspace, no tab open.
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
fn ask_button_switches_the_address_bar_between_searching_and_asking(cx: &mut TestAppContext) {
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
    assert_eq!(state(cx), (false, "Search...".to_owned()));

    // An icon alone, square in the toolbar's row.
    let ask = bounds_of(cx, format!("ask-{id}"));
    assert_eq!(ask.size, size(px(24.), px(24.)));

    let button = ask.center();
    click(cx, button);
    assert_eq!(state(cx), (true, "Ask...".to_owned()));
    assert!(in_address_bar(&browser, cx), "ready for the question");

    click(cx, button);
    assert_eq!(state(cx), (false, "Search...".to_owned()));
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
fn tabs_sit_side_by_side_with_new_tab_at_the_end(cx: &mut TestAppContext) {
    let (browser, cx) = open_with_tab(cx);
    keys(cx, "cmd-t");
    let tabs: Vec<_> = ids(&browser, cx)
        .into_iter()
        .map(|id| bounds_of(cx, format!("tab-{id}")))
        .collect();
    assert!(
        tabs.iter().all(|tab| tab.size.width == px(240.)),
        "two tabs in a wide window are as wide as tabs get: {tabs:?}"
    );
    let gap = tabs[1].left() - tabs[0].right();
    assert!(gap <= px(4.), "{gap:?} between {tabs:?}");

    let row = cx.update(|_, cx| browser.read(cx).top_groups[0].as_u64());
    let new_tab = bounds_of(cx, format!("new-tab-{row}"));
    assert!(
        new_tab.right() > px(1000. - 48.),
        "New Tab at the right of the window: {new_tab:?}"
    );
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

#[gpui_kit::test]
fn places_open_from_the_palette_and_back_returns(cx: &mut TestAppContext) {
    let (browser, cx) = open(cx);
    keys(cx, "cmd-k");
    cx.simulate_input("mail");
    cx.run_until_parked();
    keys(cx, "enter");
    assert_eq!(route(&browser, cx), "Mail");
    keys(cx, "cmd-[");
    assert_eq!(route(&browser, cx), "Home");

    // In a tab, a place opens over the tab's Home, and Back returns to it.
    keys(cx, "cmd-t");
    keys(cx, "cmd-k");
    cx.simulate_input("library");
    cx.run_until_parked();
    keys(cx, "enter");
    assert!(showing(&browser, cx) == Showing::Place(Place::Library));
    assert_eq!(tabs(&browser, cx), (1, 0), "in the same tab");
    keys(cx, "cmd-[");
    assert!(showing(&browser, cx) == Showing::Start);
}
