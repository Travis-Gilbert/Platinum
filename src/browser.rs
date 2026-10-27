//! The browser window's one view. Tabs live on GPUI Kit's Dock
//! (`browser/dock.rs`): each tab group is a row of tabs over a rounded card
//! that holds the tab's toolbar and content, framed by the window's chrome,
//! as Dia draws it. Dragging a tab reorders it, moves it to another group, or
//! drops it on a card's edge to split the window. Also here: the address bar
//! and its Ask button, Share, the downloads list, the appearance button,
//! compact mode, and the navigator.
//!
//! A tab holds at most one page and one results page. Results sit before the
//! page's own history: Back from a page with nowhere to go shows the results
//! again, and a result opens in a fresh page so that Back returns to them.
//!
//! The dock decides where tabs are and which one each group shows; the
//! browser keeps `tabs` in the dock's order and decides which shown tab is
//! the active one, the one the address bar and the shortcuts act on.
//!
//! SOURCING: GPUI Kit's base `DockArea` for the tab layout, drag and drop
//! and splits; GPUI Kit's `TitleBar`, folder `TabBar`/`Tab`, `Toolbar`,
//! `Button`, `Input`, `Kbd`, `Spinner`, `Popover` and `Progress`; the navigator is GPUI Kit's `Command` (`navigator.rs`).

mod dock;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::base::dock::{
    DockArea, DockEvent, DockLayout, DragPanel, NodeId, PanelId, TabGroupContext,
};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::progress::{Progress, ProgressCircle};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::tab::{Tab as TabItem, TabBar};
use gpui_kit::component::toolbar::Toolbar;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, TitleBar, WindowExt as _,
    h_flex, v_flex,
};
use gpui_kit::{
    Anchor, AnyElement, App, AppContext as _, Bounds, ClickEvent, Context, Corners, Div, ElementId,
    Empty, Entity, FocusHandle, Focusable, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, ParentElement as _, Pixels, Render, RenderImage, Rgba,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, img,
    point, prelude::FluentBuilder as _, px, size, transparent_black,
};
use url::Url;

use self::dock::{DRAG_PREVIEW, FRAME, FrameDock, Group, TabPanel, TabPreview};
use crate::actions::{
    ActivateTab, BookmarkPage, CloseTab, FillForm, FillPassword, FocusAddressBar, GoBack,
    GoForward, ImportBrowserData, NewTab, NextTab, OpenPalette, PreviousTab, Quit, Reload,
    SavePassword, SetAppearance, SharePage, ShowBookmarks, ShowDownloads, ShowHistory,
    ShowProfiles, ToggleAsk, ToggleCompact,
};
use crate::address::{Target, classify};
use crate::delegate::{self, Verdict};
use crate::downloads::{self, Download, DownloadState, Downloads, Request};
use crate::engine::Engine;
use crate::glass;
use crate::icons;
use crate::navigator::{self, Navigator, Pick, Placement, TabEntry};
use crate::page::{Page, PageEvent, PageMenu};
use crate::profile_ui::{ActiveProfile, ProfileEvent, ProfilePanel, Section};
use crate::serp::{Serp, SerpEvent, SerpMode};
use crate::share;
use crate::theme::{self, Appearance};
use frame_profile::{
    Session, SessionTab,
    credentials::{self, Keychain},
};

/// The height of a row of tabs, the same as the title bar's.
const TAB_ROW: f32 = 36.;

#[derive(Clone, Copy, PartialEq)]
enum Showing {
    Start,
    Page,
    Serp,
}

struct Tab {
    id: u64,
    /// The tab as the dock holds it.
    panel: Entity<TabPanel>,
    page: Option<Entity<Page>>,
    serp: Option<Entity<Serp>>,
    showing: Showing,
    /// Where the address bar asked the page to go, shown until it gets there.
    requested: Option<Url>,
    /// Whether the page was last told it is on screen.
    page_shown: bool,
    /// A page opened the tab, for a link with a target or `window.open`.
    opened: bool,
    _page_events: Option<Subscription>,
    _serp_events: Option<Subscription>,
}

impl Tab {
    fn start(id: u64, panel: Entity<TabPanel>) -> Self {
        Self {
            id,
            panel,
            page: None,
            serp: None,
            showing: Showing::Start,
            requested: None,
            page_shown: false,
            opened: false,
            _page_events: None,
            _serp_events: None,
        }
    }

    fn panel_id(&self) -> PanelId {
        PanelId::from(self.panel.entity_id())
    }

    fn title(&self, cx: &App) -> SharedString {
        match self.showing {
            Showing::Start => "New Tab".into(),
            Showing::Page => self
                .page
                .as_ref()
                .map_or("New Tab".into(), |page| page.read(cx).title()),
            Showing::Serp => self
                .serp
                .as_ref()
                .map_or("Search".into(), |serp| serp.read(cx).title()),
        }
    }

    /// Where the page is, or is going: the address being loaded until it
    /// arrives.
    fn url(&self, cx: &App) -> Option<Url> {
        let page = self.page.as_ref()?.read(cx);
        match (&self.requested, page.is_loading()) {
            (Some(requested), true) => Some(requested.clone()),
            _ => page.url(),
        }
    }

    /// What the address bar holds once the person starts typing in it.
    fn address(&self, cx: &App) -> SharedString {
        match self.showing {
            Showing::Start => SharedString::default(),
            Showing::Page => self
                .url(cx)
                .map_or(SharedString::default(), |url| url.to_string().into()),
            Showing::Serp => self.serp.as_ref().map_or(SharedString::default(), |serp| {
                serp.read(cx).query().clone()
            }),
        }
    }

    /// What the address bar shows otherwise: the site and the page's title,
    /// "github.com / Pull requests", or the query.
    fn summary(&self, cx: &App) -> SharedString {
        if self.showing != Showing::Page {
            return self.address(cx);
        }
        let Some(url) = self.url(cx) else {
            return SharedString::default();
        };
        let Some(host) = url.host_str() else {
            return url.to_string().into();
        };
        let host = host.strip_prefix("www.").unwrap_or(host);
        let title = self.title(cx);
        if self.loading(cx)
            || title.trim().is_empty()
            || title.as_ref() == url.host_str().unwrap_or(host)
        {
            host.to_owned().into()
        } else {
            format!("{host} / {title}").into()
        }
    }

    fn loading(&self, cx: &App) -> bool {
        self.showing == Showing::Page
            && self
                .page
                .as_ref()
                .is_some_and(|page| page.read(cx).is_loading())
    }

    /// The color along the top of the page the tab shows.
    fn tint(&self, cx: &App) -> Option<Hsla> {
        if self.showing != Showing::Page {
            return None;
        }
        self.page.as_ref()?.read(cx).tint()
    }

    /// The site's icon, while the tab shows the site.
    fn favicon(&self, cx: &App) -> Option<Arc<RenderImage>> {
        if self.showing != Showing::Page {
            return None;
        }
        self.page.as_ref()?.read(cx).favicon()
    }

    fn icon(&self) -> Icon {
        match self.showing {
            Showing::Start => icons::page(),
            Showing::Page => Icon::new(IconName::Globe),
            Showing::Serp => Icon::new(IconName::Search),
        }
    }
}

pub struct Browser {
    canvas: Option<Entity<crate::canvas::SpatialCanvas>>,
    focus_handle: FocusHandle,
    /// In the dock's order: group by group, left to right, top to bottom.
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    dock: Entity<DockArea>,
    /// The tab each group shows, and the groups whose tab rows run along the
    /// top of the window. Read off the dock after every change: the dock
    /// cannot be read while it draws.
    displayed: Vec<PanelId>,
    top_groups: Vec<NodeId>,
    omnibox: Entity<InputState>,
    suggestion_state: Entity<CommandState>,
    suggestions: Vec<frame_profile::LibraryEntry>,
    address_anchor: Rc<RefCell<Option<Bounds<Pixels>>>>,
    /// The person is typing in the address bar; leave its text alone.
    editing: bool,
    /// The address bar asks (RustyWeb's synthesis) instead of searching.
    ask: bool,
    compact: bool,
    /// In compact mode, the chrome is showing over the page.
    revealed: bool,
    navigator: Option<(Navigator, Subscription)>,
    downloads: Entity<Downloads>,
    /// The tab whose toolbar shows the downloads list.
    downloads_open: Option<u64>,
    /// Where each tab's Share button was last drawn, for the share sheet.
    share_anchors: Rc<RefCell<HashMap<u64, Bounds<Pixels>>>>,
    window_title: SharedString,
    last_session: Option<Session>,
    restoring: bool,
    profile_panel: Option<Entity<ProfilePanel>>,
    profile_events: Option<Subscription>,
    _quit_events: Subscription,
    _omnibox_events: Subscription,
    _dock_events: Subscription,
    _downloads_events: Subscription,
    _appearance_events: Subscription,
}

impl Browser {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let omnibox = cx.new(|cx| InputState::new(window, cx).placeholder("Search..."));
        let omnibox_events = cx.subscribe_in(&omnibox, window, Self::omnibox_event);
        let this = cx.weak_entity();
        let dock = cx.new(|cx| {
            DockArea::new("frame-tabs", None, window, cx)
                .with_renderer(Rc::new(FrameDock::new(this.clone())))
        });
        let panel = cx.new(|cx| TabPanel::new(this, window, cx));
        dock.update(cx, |dock, cx| {
            dock.set_center(DockLayout::tabs().panel(panel.clone()), window, cx);
        });
        let dock_events = cx.subscribe_in(&dock, window, Self::dock_event);
        let downloads = cx.new(|_| Downloads::new(Downloads::default_folder()));
        let downloads_events = cx.observe(&downloads, |_, _, cx| cx.notify());
        // Match System follows the system as it changes.
        let appearance_events =
            cx.observe_window_appearance(window, |_, window, cx| theme::system_changed(window, cx));
        let quit_events = cx.on_app_quit(|this, cx| {
            this.persist_session(cx);
            async {}
        });
        let mut browser = Self {
            canvas: None,
            focus_handle: cx.focus_handle(),
            tabs: vec![Tab::start(0, panel)],
            active: 0,
            next_id: 1,
            dock,
            displayed: Vec::new(),
            top_groups: Vec::new(),
            omnibox,
            suggestion_state: cx.new(|cx| CommandState::new(window, cx)),
            suggestions: Vec::new(),
            address_anchor: Rc::default(),
            editing: false,
            ask: false,
            compact: false,
            revealed: false,
            navigator: None,
            downloads,
            downloads_open: None,
            share_anchors: Rc::default(),
            window_title: SharedString::default(),
            last_session: None,
            restoring: true,
            profile_panel: None,
            profile_events: None,
            _quit_events: quit_events,
            _omnibox_events: omnibox_events,
            _dock_events: dock_events,
            _downloads_events: downloads_events,
            _appearance_events: appearance_events,
        };
        browser.sync_from_dock(cx);
        browser.restore_session(window, cx);
        browser.restoring = false;
        browser.focus_content(window, cx);
        browser
    }

    fn restore_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = cx.try_global::<ActiveProfile>().map(|p| p.0.clone()) else {
            return;
        };
        let Ok(session) = store.session() else {
            return;
        };
        for (ix, tab) in session.tabs.iter().enumerate() {
            if ix > 0 {
                let next = self.add_tab(self.active, false, window, cx);
                self.active = next;
            }
            if let Some(url) = tab.url.as_deref().and_then(frame_profile::web_url) {
                // A restored URL is a page, never an implicit download.
                if delegate::verdict(&url) == Verdict::Allow {
                    self.go(url, false, false, window, cx);
                }
            } else if let Some(query) = &tab.query {
                self.search(query.clone(), SerpMode::Search, false, window, cx);
            }
        }
        // Reuse GPUI's own dock persistence for split axes, sizes and each
        // group's selected tab. Tab records follow the same traversal order.
        if let Some(layout) = session
            .layout
            .and_then(|v| serde_json::from_value::<gpui_kit::base::dock::DockAreaState>(v).ok())
        {
            fn panel_count(state: &gpui_kit::base::dock::PanelState) -> Option<usize> {
                if state.panel_name == "FrameTab" {
                    return state.children.is_empty().then_some(1);
                }
                if !matches!(
                    state.info,
                    gpui_kit::base::dock::PanelInfo::Tabs { .. }
                        | gpui_kit::base::dock::PanelInfo::Stack { .. }
                ) {
                    return None;
                }
                state.children.iter().map(panel_count).sum()
            }
            if layout.left_dock.is_none()
                && layout.right_dock.is_none()
                && layout.bottom_dock.is_none()
                && panel_count(&layout.center) == Some(self.tabs.len())
            {
                let panels = Rc::new(RefCell::new(
                    self.tabs
                        .iter()
                        .map(|t| t.panel.clone())
                        .collect::<std::collections::VecDeque<_>>(),
                ));
                gpui_kit::base::dock::register_panel(cx, "FrameTab", move |_, _, _| {
                    Arc::new(
                        panels
                            .borrow_mut()
                            .pop_front()
                            .expect("validated saved panel count"),
                    )
                });
                if self
                    .dock
                    .update(cx, |dock, cx| dock.load(layout, window, cx))
                    .is_ok()
                {
                    self.sync_from_dock(cx);
                }
            }
        }
        if !self.tabs.is_empty() {
            self.activate(session.active.min(self.tabs.len() - 1), window, cx);
        }
    }

    fn persist_session(&mut self, cx: &mut Context<Self>) {
        if self.restoring {
            return;
        }
        let Some(store) = cx.try_global::<ActiveProfile>().map(|p| p.0.clone()) else {
            return;
        };
        let tabs = self
            .tabs
            .iter()
            .map(|tab| SessionTab {
                url: if tab.showing == Showing::Page {
                    tab.url(cx)
                        .filter(|u| frame_profile::web_url(u.as_str()).is_some())
                        .map(|u| u.to_string())
                } else {
                    None
                },
                query: if tab.showing == Showing::Serp {
                    Some(tab.address(cx).to_string())
                } else {
                    None
                },
            })
            .collect();
        let session = Session {
            layout: serde_json::to_value(self.dock.read(cx).dump(cx)).ok(),
            tabs,
            active: self.active,
        };
        if self.last_session.as_ref() != Some(&session) {
            match store.save_session(&session) {
                Ok(()) => self.last_session = Some(session),
                Err(_) => log::warn!("Could not preserve the browser session"),
            }
        }
    }

    fn show_profile_panel(
        &mut self,
        section: Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_navigator(window, cx);
        let Some(store) = cx.try_global::<ActiveProfile>().map(|p| p.0.clone()) else {
            return;
        };
        let panel = cx.new(|cx| ProfilePanel::new(store, section, window, cx));
        self.profile_events =
            Some(
                cx.subscribe_in(&panel, window, |this, _, event, window, cx| {
                    let ProfileEvent::Open(url) = event;
                    window.close_sheet(cx);
                    this.go(url.clone(), false, false, window, cx);
                }),
            );
        self.profile_panel = Some(panel.clone());
        window.open_sheet(cx, move |sheet, _, _| {
            sheet
                .title("Browser data")
                .size(px(520.))
                .child(panel.clone())
        });
    }

    fn bookmark_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(store) = cx.try_global::<ActiveProfile>() {
            if let Some(url) = self.tabs[self.active].url(cx) {
                let result = store
                    .0
                    .bookmark(url.as_str(), &self.tabs[self.active].title(cx));
                window.push_notification(
                    if result.is_ok() {
                        Notification::success("Bookmark saved")
                    } else {
                        Notification::error("Could not save bookmark")
                    },
                    cx,
                );
            }
        }
    }

    fn choose_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_navigator(window, cx);
        let Some(store) = cx.try_global::<ActiveProfile>().map(|p| p.0.clone()) else {
            return;
        };
        let Some(page) = self.tabs[self.active]
            .page
            .clone()
            .filter(|_| self.tabs[self.active].showing == Showing::Page)
        else {
            return;
        };
        let Some(url) = page
            .read(cx)
            .url()
            .filter(|u| credentials::fill_allowed(u.as_str()))
        else {
            window.push_notification(Notification::error("Open a secure login page first"), cx);
            return;
        };
        let accounts = credentials::accounts(&store, url.as_str()).unwrap_or_default();
        if accounts.is_empty() {
            window.push_notification(Notification::info("No saved passwords for this site. Import passwords from File > Import Browser Data."),cx);
            return;
        }
        let browser = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let mut list = v_flex().gap_2().child(url.origin().ascii_serialization());
            for (ix, username) in accounts.iter().enumerate() {
                let username = username.clone();
                let url = url.clone();
                let store = store.clone();
                let page = page.clone();
                let browser = browser.clone();
                list = list.child(
                    Button::new(("login", ix))
                        .label(if username.is_empty() {
                            "Account without username".into()
                        } else {
                            username.clone()
                        })
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let _ = browser.update(cx, |_, cx| {
                                let username = username.clone();
                                let store = store.clone();
                                let page = page.clone();
                                let url = url.clone();
                                let expected = url.clone();
                                let task = cx.background_spawn(async move {
                                    credentials::password(
                                        &Keychain::for_profile(&store),
                                        url.as_str(),
                                        &username,
                                    )
                                    .and_then(|password| {
                                        credentials::fill_script(url.as_str(), &username, &password)
                                    })
                                });
                                cx.spawn_in(window, async move |_, cx| {
                                    let result = task.await;
                                    let _ = cx.update(|window, cx| match result {
                                        Ok(script) => page.update(cx, |page, _| {
                                            if page
                                                .url()
                                                .is_some_and(|u| u.origin() == expected.origin())
                                            {
                                                page.fill_password(&script);
                                            }
                                        }),
                                        Err(_) => window.push_notification(
                                            Notification::error(
                                                "Could not access this password in Keychain",
                                            ),
                                            cx,
                                        ),
                                    });
                                })
                                .detach();
                            });
                        }),
                );
            }
            dialog.title("Choose an account").child(list)
        });
    }

    fn fill_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = cx.try_global::<ActiveProfile>().map(|p| p.0.clone()) else {
            return;
        };
        let Some(page) = self.tabs[self.active]
            .page
            .clone()
            .filter(|_| self.tabs[self.active].showing == Showing::Page)
        else {
            return;
        };
        let Some(url) = page
            .read(cx)
            .url()
            .filter(|u| credentials::fill_allowed(u.as_str()))
        else {
            return;
        };
        let fields = store.form_values().unwrap_or_default();
        if fields.is_empty() {
            window.push_notification(
                Notification::info("No basic form information imported for this profile"),
                cx,
            );
            return;
        }
        window.open_dialog(cx, move |dialog, _, _| {
            let mut list = v_flex().gap_2().child(url.origin().ascii_serialization());
            for (ix, (name, value)) in fields.iter().enumerate() {
                let name = name.clone();
                let value = value.clone();
                let url = url.clone();
                let page = page.clone();
                list = list.child(
                    Button::new(("form-value", ix))
                        .label(format!("{name} · {value}"))
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            if let Ok(script) =
                                credentials::form_script(url.as_str(), &name, &value)
                            {
                                page.update(cx, |page, _| page.fill_form_info(&script));
                            }
                        }),
                );
            }
            dialog.title("Choose form information").child(list)
        });
    }

    fn save_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = self.tabs[self.active]
            .page
            .as_ref()
            .filter(|_| self.tabs[self.active].showing == Showing::Page)
        {
            let result = page.update(cx, |page, _| page.read_password());
            if result.is_err() {
                window.push_notification(Notification::error("Open a secure login form first"), cx);
            }
        }
    }

    fn new_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn tab_index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    fn panel_index(&self, panel: PanelId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.panel_id() == panel)
    }

    // Tabs.

    /// Take the dock's layout: the tabs' order, the tab each group shows and
    /// the groups along the top. The active tab stays active while its group
    /// shows it; otherwise the tab its group now shows takes over. Returns
    /// whether the active tab changed.
    fn sync_from_dock(&mut self, cx: &mut Context<Self>) -> bool {
        let (groups, top_groups) = {
            let dock = self.dock.read(cx);
            (dock::groups(dock), dock::top_row(dock))
        };
        let order: Vec<PanelId> = groups
            .iter()
            .flat_map(|group| group.panels.iter().copied())
            .collect();
        self.displayed = groups.iter().filter_map(Group::displayed).collect();
        self.top_groups = top_groups;

        let was_active = self.tabs.get(self.active).map(|tab| tab.id);
        self.tabs.sort_by_key(|tab| {
            let panel = tab.panel_id();
            order
                .iter()
                .position(|held| *held == panel)
                .unwrap_or(usize::MAX)
        });
        let mut active = was_active.and_then(|id| self.tab_index(id)).unwrap_or(0);
        let panel = self.tabs[active].panel_id();
        if !self.displayed.contains(&panel)
            && let Some(shown) = groups
                .iter()
                .find(|group| group.panels.contains(&panel))
                .and_then(Group::displayed)
            && let Some(index) = self.panel_index(shown)
        {
            active = index;
        }
        self.active = active;
        Some(self.tabs[active].id) != was_active
    }

    fn dock_event(
        &mut self,
        _: &Entity<DockArea>,
        event: &DockEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let DockEvent::LayoutChanged = event else {
            return;
        };
        if self.sync_from_dock(cx) {
            self.editing = false;
            self.focus_content(window, cx);
        }
        self.apply_visibility(cx);
        self.sync_chrome(window, cx);
    }

    /// A new start tab in the group of the tab at `anchor`: right after it,
    /// or at the end of the group. Returns its index.
    fn add_tab(
        &mut self,
        anchor: usize,
        after: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let id = self.new_id();
        let browser = cx.weak_entity();
        let panel = cx.new(|cx| TabPanel::new(browser, window, cx));
        let anchor = self.tabs[anchor].panel_id();
        self.tabs.push(Tab::start(id, panel.clone()));
        self.dock.update(cx, |dock, cx| {
            dock::place(dock, panel, anchor, after, window, cx)
        });
        self.sync_from_dock(cx);
        self.tab_index(id).unwrap_or(self.tabs.len() - 1)
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_tab_beside(self.active, window, cx);
    }

    /// A new tab at the end of the group of the tab at `anchor`, ready for an
    /// address.
    fn new_tab_beside(&mut self, anchor: usize, window: &mut Window, cx: &mut Context<Self>) {
        let index = self.add_tab(anchor, false, window, cx);
        self.activate(index, window, cx);
        self.focus_address_bar(window, cx);
    }

    fn insert_tab_after_active(&mut self, window: &mut Window, cx: &mut Context<Self>) -> usize {
        self.add_tab(self.active, true, window, cx)
    }

    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let (id, panel) = (tab.id, tab.panel_id());
        self.close_navigator(window, cx);
        self.dock
            .update(cx, |dock, cx| dock.select_panel(panel, window, cx));
        self.sync_from_dock(cx);
        self.active = self.tab_index(id).unwrap_or(self.active);
        self.editing = false;
        self.apply_visibility(cx);
        self.sync_chrome(window, cx);
        self.focus_content(window, cx);
    }

    /// Make the tab `id` the active one, for a control on its own card or
    /// tab, if it is not already.
    fn activate_id(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.tab_index(id)
            && index != self.active
        {
            self.activate(index, window, cx);
        }
    }

    /// A right-click landed in a tab's card.
    fn panel_pointed(&mut self, panel: PanelId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.panel_index(panel)
            && index != self.active
        {
            self.activate(index, window, cx);
        }
    }

    /// Focus arrived in a tab's card: on the card itself (`itself`), from a
    /// click on nothing focusable in it, or on something inside it, a page
    /// say. The tab becomes the active one, and a click on the card passes
    /// the focus on to its content.
    fn panel_focused(
        &mut self,
        panel: PanelId,
        itself: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.panel_index(panel) else {
            return;
        };
        if index != self.active && self.displayed.contains(&panel) {
            self.active = index;
            self.editing = false;
            self.sync_chrome(window, cx);
        }
        if itself && index == self.active {
            self.focus_content(window, cx);
        }
    }

    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.close_navigator(window, cx);
        if self.tabs.len() == 1 {
            let id = self.new_id();
            let panel = self.tabs[0].panel.clone();
            self.tabs[0] = Tab::start(id, panel);
        } else {
            let active_id = self.tabs[self.active].id;
            let closing = self.tabs[index].panel_id();
            let group_ix = dock::groups(self.dock.read(cx))
                .iter()
                .position(|group| group.panels.contains(&closing))
                .unwrap_or(0);
            let tab = self.tabs.remove(index);
            self.active = self.tab_index(active_id).unwrap_or(0);
            self.dock.update(cx, |dock, cx| {
                dock.remove_panel(tab.panel.clone(), window, cx)
            });
            self.sync_from_dock(cx);
            // Closing the active tab passes it to the tab its group shows
            // next, or, when the group went with it, to the group in its place.
            if tab.id == active_id {
                let groups = dock::groups(self.dock.read(cx));
                let next = groups
                    .get(group_ix.min(groups.len().saturating_sub(1)))
                    .and_then(Group::displayed)
                    .and_then(|panel| self.panel_index(panel));
                if let Some(next) = next {
                    self.active = next;
                }
            }
        }
        self.editing = false;
        self.apply_visibility(cx);
        self.sync_chrome(window, cx);
        self.focus_content(window, cx);
    }

    /// Only the pages on screen, one per group, run at full rate; the rest
    /// are hidden from Servo.
    fn apply_visibility(&mut self, cx: &mut Context<Self>) {
        for tab in &mut self.tabs {
            let shown = tab.showing == Showing::Page && self.displayed.contains(&tab.panel_id());
            if let Some(page) = &tab.page
                && shown != tab.page_shown
            {
                page.update(cx, |page, _| page.set_visible(shown));
                tab.page_shown = shown;
            }
        }
    }

    fn focus_content(&self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &self.tabs[self.active];
        match tab.showing {
            Showing::Page if let Some(page) = &tab.page => {
                window.focus(&page.focus_handle(cx), cx);
            }
            Showing::Serp if let Some(serp) = &tab.serp => {
                window.focus(&serp.focus_handle(cx), cx);
            }
            // With the chrome hidden the address bar is not drawn; the window
            // keeps focus so that shortcuts still reach the browser.
            _ if self.compact && !self.revealed => window.focus(&self.focus_handle, cx),
            _ => self.omnibox.update(cx, |input, cx| input.focus(window, cx)),
        }
    }

    // Going places.

    /// Load `url` in the active tab, or in a new one after it. A fresh page
    /// starts its own history, so Back from it returns to the results. A file
    /// is downloaded instead: Servo checks only the navigations pages start.
    fn go(
        &mut self,
        url: Url,
        new_tab: bool,
        fresh: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.suggestions.clear();
        if delegate::verdict(&url) == Verdict::Download {
            self.download(url, None, window, cx);
            return;
        }
        let index = if new_tab {
            self.insert_tab_after_active(window, cx)
        } else {
            self.active
        };
        let reuse = !fresh && self.tabs[index].showing == Showing::Page;
        match &self.tabs[index].page {
            Some(page) if reuse => page.update(cx, |page, _| page.load(url.clone())),
            _ => match Page::open(url.clone(), window, cx) {
                Ok(page) => self.set_page(index, page, window, cx),
                Err(error) => {
                    log::error!("opening {url}: {error:#}");
                    window.push_notification(
                        Notification::error(format!("{error:#}"))
                            .title("Frame could not open that page"),
                        cx,
                    );
                    return;
                }
            },
        }
        let tab = &mut self.tabs[index];
        tab.requested = Some(url);
        tab.showing = Showing::Page;
        self.activate(index, window, cx);
    }

    fn search(
        &mut self,
        query: String,
        mode: SerpMode,
        new_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = if new_tab {
            self.insert_tab_after_active(window, cx)
        } else {
            self.active
        };
        let serp = cx.new(|cx| Serp::new(query, mode, window, cx));
        let id = self.tabs[index].id;
        let events = cx.subscribe_in(
            &serp,
            window,
            move |browser, _, event: &SerpEvent, window, cx| {
                let SerpEvent::Open { url, new_tab } = event;
                if *new_tab {
                    browser.go(url.clone(), true, true, window, cx);
                } else if let Some(index) = browser.tab_index(id) {
                    browser.active = index;
                    browser.go(url.clone(), false, true, window, cx);
                }
            },
        );
        let tab = &mut self.tabs[index];
        tab.serp = Some(serp);
        tab._serp_events = Some(events);
        tab.showing = Showing::Serp;
        self.activate(index, window, cx);
    }

    fn set_page(
        &mut self,
        index: usize,
        page: Entity<Page>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let events = cx.subscribe_in(&page, window, Self::page_event);
        let tab = &mut self.tabs[index];
        tab.page = Some(page);
        tab.page_shown = true;
        tab._page_events = Some(events);
    }

    fn page_event(
        &mut self,
        page: &Entity<Page>,
        event: &PageEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.page.as_ref() == Some(page))
        else {
            return;
        };
        match event {
            PageEvent::FormFilled(filled) => {
                window.push_notification(
                    if *filled {
                        Notification::success("Form field filled")
                    } else {
                        Notification::info("No matching visible field was found on this page")
                    },
                    cx,
                );
            }
            PageEvent::PasswordFilled(filled) => {
                window.push_notification(
                    if *filled {
                        Notification::success("Login details filled. Review the form and sign in.")
                    } else {
                        Notification::error("No visible, same-origin login form was found")
                    },
                    cx,
                );
            }
            PageEvent::PasswordRead(login) => {
                let Some(store) = cx.try_global::<ActiveProfile>().map(|p| p.0.clone()) else {
                    return;
                };
                let login = login.clone();
                window.open_alert_dialog(cx, move |dialog, _, _| {
                    let store = store.clone();
                    let login = login.clone();
                    dialog
                        .title("Save or update password?")
                        .description(format!("{} · {}", login.origin, login.username))
                        .child("The password will be stored in macOS Keychain for this profile.")
                        .button_props(
                            gpui_kit::component::dialog::DialogButtonProps::default()
                                .ok_text("Save password")
                                .show_cancel(true),
                        )
                        .on_ok(move |_, window, cx| {
                            let store = store.clone();
                            let login = login.clone();
                            let task = cx.background_spawn(async move {
                                credentials::save(
                                    &store,
                                    &Keychain::for_profile(&store),
                                    &login.origin,
                                    &login.username,
                                    &login.password,
                                )
                            });
                            window
                                .spawn(cx, async move |cx| {
                                    let result = task.await;
                                    let _ = cx.update(|window, cx| {
                                        window.push_notification(
                                            if result.is_ok() {
                                                Notification::success("Password saved")
                                            } else {
                                                Notification::error(
                                                    "Could not save password in Keychain",
                                                )
                                            },
                                            cx,
                                        )
                                    });
                                })
                                .detach();
                            true
                        })
                });
            }
            PageEvent::PasswordUnavailable => {
                window.push_notification(Notification::info("Enter a password in a visible login form, then choose Save or Update Password"),cx);
            }
            PageEvent::Visited => {
                if let Some(store) = cx.try_global::<ActiveProfile>() {
                    if let Some(url) = page.read(cx).url() {
                        let _ = store.0.visit(url.as_str(), &page.read(cx).title());
                    }
                }
            }
            PageEvent::Changed => {
                if !page.read(cx).is_loading() {
                    self.tabs[index].requested = None;
                }
                self.sync_chrome(window, cx);
            }
            PageEvent::Opened => {
                // Each opens beside the one before it, and the last one shows.
                let opened = page.update(cx, |page, _| page.take_opened());
                let mut anchor = index;
                let mut last = None;
                for new in opened {
                    let page = Page::adopt(new, window, cx);
                    let at = self.add_tab(anchor, true, window, cx);
                    self.set_page(at, page, window, cx);
                    self.tabs[at].showing = Showing::Page;
                    self.tabs[at].opened = true;
                    last = Some(self.tabs[at].id);
                    anchor = at;
                }
                if let Some(id) = last {
                    self.activate_id(id, window, cx);
                }
            }
            PageEvent::RevealCanvas => {},
            PageEvent::ContextMenu(menu) => {
                let tab = &self.tabs[index];
                if tab.showing == Showing::Page && self.displayed.contains(&tab.panel_id()) {
                    self.activate_id(tab.id, window, cx);
                    let entry = Some((page.clone(), menu.clone()));
                    self.open_navigator(Placement::Pointer(menu.position), entry, window, cx);
                } else {
                    page.update(cx, |page, _| page.dismiss_context_menu());
                }
            }
            PageEvent::Closed => self.close_tab(index, window, cx),
            PageEvent::Download(url) => {
                let referrer = page.read(cx).url();
                self.leave_download(index, url, window, cx);
                self.download(url.clone(), referrer, window, cx);
            }
        }
    }

    /// A page that went to a file is left on Servo's placeholder for it, or,
    /// in a tab opened for the file, on nothing at all: take it back to where
    /// it was, or close it.
    fn leave_download(
        &mut self,
        index: usize,
        url: &Url,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = &mut self.tabs[index];
        let Some(page) = tab.page.clone() else {
            return;
        };
        let (at, can_back) = {
            let page = page.read(cx);
            (page.url(), page.can_go_back())
        };
        let on_file = at.as_ref() == Some(url);
        let blank = at.as_ref().is_none_or(|at| at.scheme() == "about");
        if !on_file && !blank {
            // The page never left; Frame stopped it before it went.
            return;
        }
        if on_file && can_back {
            page.update(cx, |page, _| page.back());
        } else if tab.opened {
            self.close_tab(index, window, cx);
        } else {
            tab.page = None;
            tab._page_events = None;
            tab.page_shown = false;
            tab.requested = None;
            tab.showing = if tab.serp.is_some() {
                Showing::Serp
            } else {
                Showing::Start
            };
            if index == self.active {
                self.after_switch(window, cx);
            } else {
                cx.notify();
            }
        }
    }

    /// Save `url` to the Downloads folder, with the cookies Servo holds for
    /// it, and show the list.
    fn download(
        &mut self,
        url: Url,
        referrer: Option<Url>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = downloads::file_name(&url, None, None);
        let request = Request {
            cookies: Engine::cookie_header(&url, cx),
            user_agent: Some(Engine::user_agent()),
            referrer: referrer.filter(|referrer| *referrer != url),
            url,
        };
        self.downloads
            .update(cx, |downloads, cx| downloads.start(request, cx));
        if self.compact && !self.revealed {
            window.push_notification(
                Notification::info(format!("{name} is saving to your Downloads folder."))
                    .title("Download started"),
                cx,
            );
        } else {
            self.downloads_open = Some(self.tabs[self.active].id);
        }
        cx.notify();
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[self.active];
        match tab.showing {
            Showing::Page => {
                let Some(page) = tab.page.clone() else { return };
                if page.read(cx).can_go_back() {
                    page.update(cx, |page, _| page.back());
                } else if tab.serp.is_some() {
                    tab.showing = Showing::Serp;
                    self.after_switch(window, cx);
                }
            }
            Showing::Serp | Showing::Start => {}
        }
    }

    fn forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[self.active];
        match tab.showing {
            Showing::Page => {
                if let Some(page) = &tab.page {
                    page.update(cx, |page, _| page.forward());
                }
            }
            Showing::Serp if tab.page.is_some() => {
                tab.showing = Showing::Page;
                self.after_switch(window, cx);
            }
            Showing::Serp | Showing::Start => {}
        }
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let tab = &self.tabs[self.active];
        match (tab.showing, &tab.page, &tab.serp) {
            (Showing::Page, Some(page), _) => page.update(cx, |page, _| page.reload()),
            (Showing::Serp, _, Some(serp)) => serp.update(cx, |serp, cx| serp.reload(cx)),
            _ => {}
        }
    }

    /// The active tab now shows something else.
    fn after_switch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_visibility(cx);
        self.sync_chrome(window, cx);
        self.focus_content(window, cx);
    }

    // The address bar.

    fn omnibox_event(
        &mut self,
        omnibox: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter { secondary, .. } => self.submit(*secondary, window, cx),
            InputEvent::Focus => {
                // Typing starts from the whole address, not the summary.
                self.editing = true;
                let address = self.tabs[self.active].address(cx);
                let omnibox = omnibox.clone();
                window.defer(cx, move |window, cx| {
                    omnibox.update(cx, |input, cx| {
                        if input.value() != address {
                            input.set_value(address, window, cx);
                        }
                        // After the click that focused it has placed the caret.
                        input.select_all(window, cx);
                    });
                });
                cx.notify();
            }
            InputEvent::Blur => {
                self.editing = false;
                if !self.chrome_in_use() {
                    self.revealed = false;
                }
                self.sync_chrome(window, cx);
            }
            InputEvent::Change => {
                let query = omnibox.read(cx).value();
                self.suggestions.clear();
                if !self.ask && !query.trim().is_empty() {
                    if let Some(store) = cx.try_global::<ActiveProfile>() {
                        self.suggestions = store.0.suggestions(&query, true, 4).unwrap_or_default();
                        for entry in store.0.suggestions(&query, false, 6).unwrap_or_default() {
                            if !self.suggestions.iter().any(|e| e.url == entry.url) {
                                self.suggestions.push(entry);
                            }
                        }
                    }
                }
                cx.notify();
            }
        }
    }

    fn submit(&mut self, new_tab: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.suggestions.clear();
        let text = self.omnibox.read(cx).value().to_string();
        match classify(&text) {
            None => {}
            Some(Target::Go(url)) => {
                let fresh = self.tabs[self.active].showing != Showing::Page;
                self.go(url, new_tab, fresh, window, cx);
            }
            Some(Target::Search(query)) => {
                let mode = if self.ask {
                    SerpMode::Ask
                } else {
                    SerpMode::Search
                };
                self.search(query, mode, new_tab, window, cx);
            }
        }
    }

    fn focus_address_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.compact {
            self.revealed = true;
        }
        self.omnibox.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    /// Escape in the address bar puts the address back and returns to the page.
    fn escape_address_bar(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        self.suggestions.clear();
        self.editing = false;
        self.sync_chrome(window, cx);
        if self.tabs[self.active].showing != Showing::Start {
            self.focus_content(window, cx);
        }
    }

    /// The Ask button: the address bar asks RustyWeb for an answer instead of
    /// searching, until it is turned off.
    fn toggle_ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask = !self.ask;
        let placeholder = if self.ask { "Ask..." } else { "Search..." };
        self.omnibox.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
        if self.ask {
            self.focus_address_bar(window, cx);
        }
        cx.notify();
    }

    /// The address bar's text and the window's title follow the active tab.
    fn sync_chrome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.persist_session(cx);
        let tab = &self.tabs[self.active];
        if !self.editing {
            let summary = tab.summary(cx);
            if self.omnibox.read(cx).value() != summary {
                self.omnibox
                    .update(cx, |input, cx| input.set_value(summary, window, cx));
            }
        }
        let title = tab.title(cx);
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }
        cx.notify();
    }

    // The navigator.

    fn open_navigator(
        &mut self,
        placement: Placement,
        page: Option<(Entity<Page>, PageMenu)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_navigator(window, cx);
        let restore_focus = window.focused(cx);
        let state = cx.new(|cx| CommandState::new(window, cx));
        // The list is built from the query, so the view redraws as it changes.
        let observer = cx.observe(&state, |_, _, cx| cx.notify());
        state.update(cx, |state, cx| state.focus(window, cx));
        self.navigator = Some((
            Navigator {
                state,
                placement,
                page,
                restore_focus,
            },
            observer,
        ));
        cx.notify();
    }

    fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((navigator, _)) = &self.navigator
            && matches!(navigator.placement, Placement::Palette)
        {
            self.close_navigator(window, cx);
        } else {
            self.open_navigator(Placement::Palette, None, window, cx);
        }
    }

    fn close_navigator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((navigator, _)) = self.navigator.take() else {
            return;
        };
        // Answers the page's menu if nothing on it was chosen.
        if let Some((page, _)) = &navigator.page {
            page.update(cx, |page, _| page.dismiss_context_menu());
        }
        // Focus goes back only if nothing chosen moved it elsewhere.
        if navigator.state.focus_handle(cx).is_focused(window) {
            match navigator.restore_focus {
                Some(handle) => window.focus(&handle, cx),
                None => self.focus_content(window, cx),
            }
        }
        // A pick that put the cursor in the address bar keeps the chrome out;
        // `editing` only follows once the input reports its focus.
        if self.compact
            && !self.omnibox.focus_handle(cx).is_focused(window)
            && self.downloads_open.is_none()
        {
            self.revealed = false;
        }
        cx.notify();
    }

    fn picked(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        match pick {
            Pick::Page(action) => {
                if let Some((page, _)) = self.navigator.as_ref().and_then(|(n, _)| n.page.clone()) {
                    page.update(cx, |page, _| page.select_context_action(action));
                }
            }
            Pick::SwitchTab(id) => {
                if let Some(index) = self.tab_index(id) {
                    self.activate(index, window, cx);
                }
            }
            Pick::Command => {}
            Pick::Go(url) => {
                let fresh = self.tabs[self.active].showing != Showing::Page;
                self.go(url, false, fresh, window, cx);
            }
            Pick::Search(query) => self.search(query, SerpMode::Search, false, window, cx),
            Pick::Ask(query) => self.search(query, SerpMode::Ask, false, window, cx),
            Pick::Download(url) => {
                let referrer = self
                    .navigator
                    .as_ref()
                    .and_then(|(navigator, _)| navigator.page.as_ref())
                    .and_then(|(page, _)| page.read(cx).url());
                self.download(url, referrer, window, cx);
            }
        }
        self.close_navigator(window, cx);
    }

    /// Pages answer right-clicks themselves (`PageEvent::ContextMenu`); this
    /// is the start surface and the results.
    fn right_click(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.open_navigator(Placement::Pointer(event.position), None, window, cx);
        // Otherwise this view's own focus-on-click runs next and takes the
        // cursor out of the navigator's search field.
        window.prevent_default();
    }

    // Commands.

    fn toggle_compact(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.compact = !self.compact;
        self.revealed = false;
        if self.compact && self.omnibox.focus_handle(cx).is_focused(window) {
            self.editing = false;
            self.focus_content(window, cx);
        }
        cx.notify();
    }

    /// Something in compact mode's chrome is in use, so it stays out.
    fn chrome_in_use(&self) -> bool {
        self.editing || self.navigator.is_some() || self.downloads_open.is_some()
    }

    fn toggle_downloads(&mut self, cx: &mut Context<Self>) {
        if self.downloads_open.is_some() {
            self.set_downloads_open(None, cx);
        } else {
            // The list hangs off the toolbar, which compact mode hides.
            if self.compact {
                self.revealed = true;
            }
            self.set_downloads_open(Some(self.tabs[self.active].id), cx);
        }
    }

    fn set_downloads_open(&mut self, open: Option<u64>, cx: &mut Context<Self>) {
        self.downloads_open = open;
        if self.compact && !self.chrome_in_use() {
            self.revealed = false;
        }
        cx.notify();
    }

    /// The web page the tab shows, if it can be shared.
    fn share_url(&self, index: usize, cx: &App) -> Option<Url> {
        let tab = &self.tabs[index];
        if !share::SUPPORTED || tab.showing != Showing::Page {
            return None;
        }
        let url = tab.page.as_ref()?.read(cx).url()?;
        matches!(url.scheme(), "http" | "https").then_some(url)
    }

    /// The share sheet, below the tab's Share button, or below the top of
    /// the window when the button is not on screen.
    fn share_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        let Some(url) = self.share_url(index, cx) else {
            return;
        };
        self.activate_id(id, window, cx);
        let anchor = self.share_anchors.borrow().get(&id).copied();
        let anchor = anchor.unwrap_or_else(|| {
            let width = window.viewport_size().width;
            Bounds::new(point(width * 0.5, px(0.)), size(px(1.), px(TAB_ROW)))
        });
        share::share(&url, anchor, window, cx);
    }

    fn activate_numbered(
        &mut self,
        action: &ActivateTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // ⌘9 is always the last tab.
        let index = if action.index >= 8 {
            self.tabs.len() - 1
        } else {
            action.index
        };
        self.activate(index, window, cx);
    }

    fn cycle(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.tabs.len() as isize;
        let index = (self.active as isize + step).rem_euclid(count) as usize;
        self.activate(index, window, cx);
    }

    // Drawing.

    /// The site's icon, else the tab's own, or a spinner while its page loads.
    fn tab_prefix(tab: &Tab, cx: &App) -> AnyElement {
        if tab.loading(cx) {
            Spinner::new().into_any_element()
        } else if let Some(favicon) = tab.favicon(cx) {
            img(favicon).flex_shrink_0().size_4().into_any_element()
        } else {
            tab.icon().into_any_element()
        }
    }

    /// The color of a tab's toolbar, and of the tab while it is selected: the
    /// color along the top of its page, as Dia does; else the card's.
    fn chrome_fill(tab: &Tab, cx: &App) -> Hsla {
        tab.tint(cx).unwrap_or(cx.theme().background)
    }

    /// The color of text and icons on a tab's toolbar when the theme's would
    /// not read there: a light page under the dark theme, or a dark page under
    /// the light one. The theme's background is the color furthest from its
    /// text, so it reads where the text would not.
    fn chrome_ink(tab: &Tab, cx: &App) -> Option<Hsla> {
        Self::background_ink(glass::seen(Self::chrome_fill(tab, cx), cx), cx)
    }

    fn background_ink(background: Hsla, cx: &App) -> Option<Hsla> {
        let Rgba { r, g, b, .. } = background.to_rgb();
        let light = 0.299 * r + 0.587 * g + 0.114 * b > 0.5;
        let theme = cx.theme();
        (light == theme.is_dark()).then_some(theme.background)
    }

    /// A chrome button: in contrasting `ink` when needed, else the theme's ghost.
    fn inked(button: Button, ink: Option<Hsla>, cx: &App) -> Button {
        match ink {
            Some(ink) => button.custom(
                ButtonCustomVariant::new(cx)
                    .foreground(ink)
                    .hover(ink.opacity(0.1))
                    .active(ink.opacity(0.16)),
            ),
            None => button.ghost(),
        }
    }

    /// A row of folder tabs. They share the row, up to a width, and the
    /// selected one rises out of the toolbar below it.
    fn tab_bar(id: impl Into<ElementId>) -> TabBar {
        TabBar::new(id)
            .folder()
            .flex_1()
            .min_w_0()
            .menu(true)
            .max_width(px(240.))
            .bg(transparent_black())
    }

    /// A tab's close button, in `ink` when the selected tab has the page's color.
    fn close_button(id: u64, ink: Option<Hsla>, cx: &mut Context<Self>) -> Button {
        Self::inked(Button::new(("close-tab", id)), ink, cx)
            .icon(IconName::Close)
            .xsmall()
            .tooltip_with_action("Close Tab", &CloseTab, None)
            .on_click(cx.listener(move |browser, _, window, cx| {
                if let Some(index) = browser.tab_index(id) {
                    browser.close_tab(index, window, cx);
                }
                cx.stop_propagation();
            }))
    }

    /// The window-drag area in a row of tabs: over the traffic lights at the
    /// start of the top-left row, and the rest of every top row.
    fn drag_strip() -> TitleBar {
        TitleBar::new().border_b_0().bg(transparent_black())
    }

    /// A tab group's row of tabs (`FrameDock::render_tab_bar`). The dock
    /// moves a dragged tab; dropping it on a tab puts it there, and on the
    /// empty end of the row, last.
    fn render_tab_row(
        &mut self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.compact {
            return Empty.into_any_element();
        }
        let node = group.node();
        let top = self.top_groups.contains(&node);
        let corner = self.top_groups.first() == Some(&node) && !window.is_fullscreen();
        let droppable = group.is_droppable();
        let draggable = group.is_draggable();
        let count = group.panels().len();

        let items: Vec<TabItem> = group
            .panels()
            .iter()
            .enumerate()
            .filter_map(|(ix, view)| {
                let tab = &self.tabs[self.panel_index(view.panel_id(cx))?];
                let id = tab.id;
                let title = tab.title(cx);
                let drag = draggable.then(|| group.drag_panel(ix, cx)).flatten();
                let ink = Self::chrome_ink(tab, cx).filter(|_| ix == group.active_ix());
                let item = TabItem::new()
                    .debug_selector(move || format!("tab-{id}"))
                    .flex_1()
                    .selected_bg(Self::chrome_fill(tab, cx))
                    .when_some(ink, |item, ink| item.selected_fg(ink))
                    .label(title.clone())
                    .prefix(Self::tab_prefix(tab, cx))
                    .suffix(Self::close_button(id, ink, cx))
                    .on_click(cx.listener(move |browser, _, window, cx| {
                        browser.activate_id(id, window, cx)
                    }))
                    .when_some(drag, |item, drag| {
                        item.on_drag(drag, move |drag, offset, _, cx| {
                            cx.stop_propagation();
                            drag.set_drag_offset(offset);
                            drag.set_preview_size(DRAG_PREVIEW);
                            cx.new(|_| TabPreview {
                                title: title.clone(),
                            })
                        })
                    })
                    .when(droppable, |item| {
                        let group = group.clone();
                        item.drag_over::<DragPanel>(|style, _, _, cx| {
                            style
                                .rounded_l_none()
                                .border_l_2()
                                .border_r_0()
                                .border_color(cx.theme().drag_border)
                        })
                        .on_drop(move |drag: &DragPanel, window, cx| {
                            group.drop_panel(drag.clone(), Some(ix), true, window, cx);
                        })
                    });
                Some(item)
            })
            .collect();

        // A new tab opens at the end of this group, beside the tab it shows.
        // The button sits at the end of the row, past the room to drag in.
        let shown = group.active_panel().map(|view| view.panel_id(cx));
        let new_tab = Button::new(("new-tab", node.as_u64()))
            .debug_selector(move || format!("new-tab-{}", node.as_u64()))
            .icon(IconName::Plus)
            .ghost()
            .small()
            .tooltip_with_action("New Tab", &NewTab, None)
            .on_click(cx.listener(move |browser, _, window, cx| {
                let anchor = shown.and_then(|panel| browser.panel_index(panel));
                browser.new_tab_beside(anchor.unwrap_or(browser.active), window, cx);
            }));
        let space = h_flex()
            .id(("tab-space", node.as_u64()))
            .h_full()
            .flex_grow_1()
            .min_w_16()
            .items_center()
            .when(top, |space| {
                space.child(div().flex_1().h_full().child(Self::drag_strip().pl_0()))
            })
            .when(droppable, |space| {
                let group = group.clone();
                space
                    .drag_over::<DragPanel>(|style, _, _, cx| style.bg(cx.theme().drop_target))
                    .on_drop(move |drag: &DragPanel, window, cx| {
                        // Past its own group's last tab, a tab lands last; one
                        // from another group joins this one in the background.
                        let ix = (drag.source() == group.node()).then(|| count - 1);
                        group.drop_panel(drag.clone(), ix, false, window, cx);
                    })
            });

        h_flex()
            .id(("tab-row", node.as_u64()))
            .flex_shrink_0()
            .h(px(TAB_ROW))
            .items_center()
            .pr_1()
            .when(!corner, |row| row.pl_1())
            // Clicks in the row leave the focus where it is.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .when(corner, |row| row.child(Self::drag_strip()))
            .child(
                Self::tab_bar(("tabs", node.as_u64()))
                    .selected_index(group.active_ix())
                    .last_empty_space(space)
                    .children(items),
            )
            .child(new_tab)
            .into_any_element()
    }

    /// A tab's card (`TabPanel`): its toolbar, unless compact, over its content.
    fn render_card(
        &mut self,
        panel: PanelId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(index) = self.panel_index(panel) else {
            return Empty.into_any_element();
        };
        let tab = &self.tabs[index];
        let home = tab.showing == Showing::Start;
        let id = tab.id;
        // A page's color runs on into the toolbar; anything else keeps a rule.
        let tinted = tab.tint(cx).is_some();
        let fill = Self::chrome_fill(tab, cx);
        let (border, radius) = (cx.theme().border, cx.theme().radius_lg);
        v_flex()
            .size_full()
            .when(!self.compact, |card| {
                card.child(
                    self.toolbar(index, cx)
                        .rounded_t(radius)
                        .bg(fill)
                        .when(!tinted, |toolbar| toolbar.border_b_1().border_color(border)),
                )
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.content(index, window, cx))
                    .children(self.status_bubble(index, cx)),
            )
            .when(home, |card| card.child(self.home_status_bar(id, cx)))
            .into_any_element()
    }

    /// Back, Forward and Reload, the address bar and the Ask button. Each acts
    /// on its own tab, which a click makes the active one.
    fn toolbar(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let tab = &self.tabs[index];
        let id = tab.id;
        let (can_back, can_forward) = match (tab.showing, &tab.page) {
            (Showing::Page, Some(page)) => {
                let page = page.read(cx);
                (
                    page.can_go_back() || tab.serp.is_some(),
                    page.can_go_forward(),
                )
            }
            (Showing::Serp, page) => (false, page.is_some()),
            _ => (false, false),
        };
        let can_reload = tab.showing != Showing::Start;
        let ink = Self::chrome_ink(tab, cx);
        h_flex()
            .flex_shrink_0()
            .gap_1()
            .px_2()
            .py_1()
            .child(
                Toolbar::new(("navigation", id))
                    .flex_1()
                    .min_w_0()
                    .child(
                        Self::inked(Button::new(("back", id)), ink, cx)
                            .icon(IconName::ArrowLeft)
                            .disabled(!can_back)
                            .tooltip_with_action("Back", &GoBack, None)
                            .on_click(cx.listener(move |browser, _, window, cx| {
                                browser.activate_id(id, window, cx);
                                browser.back(window, cx);
                            })),
                    )
                    .child(
                        Self::inked(Button::new(("forward", id)), ink, cx)
                            .icon(IconName::ArrowRight)
                            .disabled(!can_forward)
                            .tooltip_with_action("Forward", &GoForward, None)
                            .on_click(cx.listener(move |browser, _, window, cx| {
                                browser.activate_id(id, window, cx);
                                browser.forward(window, cx);
                            })),
                    )
                    .child(
                        Self::inked(Button::new(("reload", id)), ink, cx)
                            .icon(IconName::RefreshCw)
                            .disabled(!can_reload)
                            .tooltip_with_action("Reload", &Reload, None)
                            .on_click(cx.listener(move |browser, _, window, cx| {
                                browser.activate_id(id, window, cx);
                                browser.reload(cx);
                            })),
                    )
                    .content(self.address(index, ink, cx)),
            )
            .child(self.ask_button(index, ink, cx))
            .child(self.share_button(index, ink, cx))
            .children(self.downloads_button(id, ink, cx))
            .child(self.appearance_button(id, ink, cx))
    }

    /// The active tab's address bar. Another tab's card shows its summary in
    /// the same place; a click there moves the address bar to it.
    fn address(&self, index: usize, ink: Option<Hsla>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (text, muted) = ink.map_or((theme.foreground, theme.muted_foreground), |ink| {
            (ink, ink.opacity(0.6))
        });
        if index == self.active {
            let prefix = if self.ask {
                icons::ask().small()
            } else {
                Icon::new(IconName::Search).small()
            };
            // On a page's color the field is drawn here, in that color's ink:
            // the theme's own field would be a dark box on a light page.
            let editing = self.editing;
            let anchor = self.address_anchor.clone();
            return div()
                .on_children_prepainted(move |bounds, _, _| {
                    *anchor.borrow_mut() = bounds.first().copied();
                })
                .debug_selector(|| "address-bar".into())
                .flex_1()
                .min_w_0()
                .on_action(cx.listener(Self::escape_address_bar))
                .when_some(ink, |field, ink| {
                    field
                        .text_color(ink)
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(if editing {
                            ink.opacity(0.3)
                        } else {
                            transparent_black()
                        })
                        .when(editing, |field| field.bg(ink.opacity(0.06)))
                })
                .child(
                    Input::new(&self.omnibox)
                        .id("omnibox")
                        .prefix(prefix.when_some(ink, |icon, ink| icon.text_color(ink)))
                        .small()
                        .appearance(editing && ink.is_none())
                        .when_some(ink, |input, ink| input.foreground(ink)),
                )
                .into_any_element();
        }
        let tab = &self.tabs[index];
        let id = tab.id;
        let summary = tab.summary(cx);
        let (text, color) = if summary.is_empty() {
            (
                SharedString::from(if self.ask { "Ask..." } else { "Search..." }),
                muted,
            )
        } else {
            (summary, text)
        };
        h_flex()
            .id(("address", id))
            .flex_1()
            .min_w_0()
            .h_7()
            .px_2()
            .gap_2()
            .cursor_text()
            .text_sm()
            .text_color(color)
            .child(Icon::new(IconName::Search).small().text_color(muted))
            .child(div().min_w_0().truncate().child(text))
            .on_click(cx.listener(move |browser, _, window, cx| {
                browser.activate_id(id, window, cx);
                browser.focus_address_bar(window, cx);
            }))
            .into_any_element()
    }

    /// Filled while the address bar asks, outlined while it searches. An icon
    /// alone, at the toolbar's 24 px row; Button draws it at three quarters of
    /// that, larger than the 14 px of a small button's icon.
    fn ask_button(&self, index: usize, ink: Option<Hsla>, cx: &mut Context<Self>) -> Button {
        let id = self.tabs[index].id;
        let button = Button::new(("ask", id))
            .debug_selector(move || format!("ask-{id}"))
            .with_size(px(24.))
            .accessibility_label("Ask AI")
            .toggled(self.ask)
            .tooltip_with_action(
                if self.ask { "Turn Off Ask" } else { "Ask AI" },
                &ToggleAsk,
                None,
            )
            .on_click(cx.listener(move |browser, _, window, cx| {
                browser.activate_id(id, window, cx);
                browser.toggle_ask(window, cx);
            }));
        if self.ask {
            let (on, text) = match ink {
                Some(ink) => (ink, Self::chrome_fill(&self.tabs[index], cx)),
                None => (cx.theme().foreground, cx.theme().background),
            };
            button.icon(icons::ask_on()).custom(
                ButtonCustomVariant::new(cx)
                    .color(on)
                    .foreground(text)
                    .hover(on.opacity(0.9))
                    .active(on.opacity(0.8)),
            )
        } else if ink.is_some() {
            Self::inked(button, ink, cx).icon(icons::ask())
        } else {
            button.icon(icons::ask()).outline()
        }
    }

    /// The system's share sheet for the tab's page. AppKit asks for it on
    /// mouse-down, so a pointer opens it there; Space or Return on the
    /// focused button opens it on the click.
    fn share_button(
        &self,
        index: usize,
        ink: Option<Hsla>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = self.tabs[index].id;
        let anchors = self.share_anchors.clone();
        div()
            .on_children_prepainted(move |bounds, _, _| {
                if let Some(bounds) = bounds.first() {
                    anchors.borrow_mut().insert(id, *bounds);
                }
            })
            .child(
                Self::inked(Button::new(("share", id)), ink, cx)
                    .icon(icons::share())
                    .small()
                    .disabled(self.share_url(index, cx).is_none())
                    .tooltip_with_action("Share", &SharePage, None)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |browser, _: &MouseDownEvent, window, cx| {
                            browser.share_tab(id, window, cx)
                        }),
                    )
                    .on_click(cx.listener(move |browser, event: &ClickEvent, window, cx| {
                        if event.is_keyboard() {
                            browser.share_tab(id, window, cx);
                        }
                    })),
            )
    }

    /// Shown once there is a download: a ring of the running downloads'
    /// progress, or the downloads glyph, over the list.
    fn downloads_button(
        &self,
        id: u64,
        ink: Option<Hsla>,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let open = self.downloads_open == Some(id);
        let list = self.downloads.read(cx);
        if list.is_empty() && !open {
            return None;
        }
        let button = Self::inked(Button::new(("downloads", id)), ink, cx)
            .small()
            .tooltip_with_action("Downloads", &ShowDownloads, None);
        let button = match list.running() {
            Some(percent) => button.icon(
                ProgressCircle::new(("downloads-progress", id))
                    .value(percent.unwrap_or_default())
                    .loading(percent.is_none()),
            ),
            None => button.icon(icons::downloads()),
        };
        let browser = cx.weak_entity();
        let downloads = self.downloads.clone();
        Some(
            Popover::new(("downloads-list", id))
                .anchor(Anchor::TopRight)
                .open(open)
                .on_open_change(move |open, _, cx| {
                    let _ = browser.update(cx, |browser, cx| {
                        browser.set_downloads_open(open.then_some(id), cx)
                    });
                })
                .trigger(button)
                .content(move |_, _, cx| Self::downloads_list(&downloads, cx)),
        )
    }

    fn downloads_list(downloads: &Entity<Downloads>, cx: &App) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let list = downloads.read(cx);
        let clearable = list.items().iter().any(|download| !download.is_running());
        let header = h_flex()
            .justify_between()
            .items_center()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Downloads"),
            )
            .child(
                Button::new("clear-downloads")
                    .ghost()
                    .xsmall()
                    .label("Clear")
                    .disabled(!clearable)
                    .on_click({
                        let downloads = downloads.clone();
                        move |_, _, cx| downloads.update(cx, |downloads, cx| downloads.clear(cx))
                    }),
            );
        let body = if list.is_empty() {
            div()
                .py_4()
                .text_center()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("No downloads")
                .into_any_element()
        } else {
            v_flex()
                .id("download-rows")
                .max_h(px(360.))
                .overflow_y_scroll()
                .gap_1()
                .children(
                    list.items()
                        .iter()
                        .map(|download| Self::download_row(download, downloads, cx)),
                )
                .into_any_element()
        };
        v_flex().w(px(320.)).gap_2().child(header).child(body)
    }

    /// A download: its name, its progress while it runs, and Cancel, or Show
    /// in Finder once it is saved. Clicking a saved one opens it.
    fn download_row(
        download: &Download,
        downloads: &Entity<Downloads>,
        cx: &App,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let id = download.id;
        let finished = download.state == DownloadState::Finished;
        let status_color = match download.state {
            DownloadState::Failed(_) => theme.danger,
            _ => theme.muted_foreground,
        };
        let action = if download.is_running() {
            let downloads = downloads.clone();
            Some(
                Button::new(("cancel-download", id))
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip("Cancel")
                    .on_click(move |_, _, cx| {
                        downloads.update(cx, |downloads, cx| downloads.cancel(id, cx));
                        cx.stop_propagation();
                    }),
            )
        } else if finished {
            let downloads = downloads.clone();
            Some(
                Button::new(("reveal-download", id))
                    .icon(IconName::FolderOpen)
                    .ghost()
                    .xsmall()
                    .tooltip("Show in Finder")
                    .on_click(move |_, _, cx| {
                        downloads.read(cx).reveal(id, cx);
                        cx.stop_propagation();
                    }),
            )
        } else {
            None
        };
        h_flex()
            .id(("download", id))
            .gap_2()
            .px_2()
            .py_1p5()
            .rounded(theme.radius)
            .items_center()
            .when(finished, |row| {
                let (downloads, hover) = (downloads.clone(), theme.accent);
                row.cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .on_click(move |_, _, cx| downloads.read(cx).open(id, cx))
            })
            .child(
                Icon::new(IconName::File)
                    .small()
                    .text_color(theme.muted_foreground),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().text_sm().truncate().child(download.name.clone()))
                    .when(download.is_running(), |info| {
                        let percent = download.percent();
                        info.child(
                            Progress::new(("download-progress", id))
                                .value(percent.unwrap_or_default())
                                .loading(percent.is_none()),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(status_color)
                            .truncate()
                            .child(download.status()),
                    ),
            )
            .children(action)
    }

    /// Switches between Light and Dark, from whichever shows now. Match
    /// System is in the View menu and the palette.
    fn appearance_button(
        &self,
        id: u64,
        ink: Option<Hsla>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let next = if cx.theme().is_dark() {
            Appearance::Light
        } else {
            Appearance::Dark
        };
        Self::inked(Button::new(("appearance", id)), ink, cx)
            .debug_selector(move || format!("appearance-{id}"))
            .icon(theme::appearance(cx).icon())
            .small()
            .tooltip(next.command())
            .on_click(move |_, window, cx| {
                window.dispatch_action(Box::new(SetAppearance { appearance: next }), cx)
            })
    }

    /// Compact mode's chrome, over the page while the pointer is at the top
    /// of the window or the address bar is in use: every tab in one row, and
    /// the active tab's toolbar.
    fn compact_chrome(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let tabs: Vec<TabItem> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let ink = Self::chrome_ink(tab, cx).filter(|_| index == self.active);
                TabItem::new()
                    .flex_1()
                    .selected_bg(Self::chrome_fill(tab, cx))
                    .when_some(ink, |item, ink| item.selected_fg(ink))
                    .label(tab.title(cx))
                    .prefix(Self::tab_prefix(tab, cx))
                    .suffix(Self::close_button(tab.id, ink, cx))
            })
            .collect();
        v_flex()
            .id("compact-chrome")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bg(theme.title_bar)
            .occlude()
            .shadow_lg()
            .on_hover(cx.listener(|browser, hovered: &bool, _, cx| {
                if !*hovered && !browser.chrome_in_use() {
                    browser.revealed = false;
                    cx.notify();
                }
            }))
            .child(
                h_flex()
                    .h(px(TAB_ROW))
                    .items_center()
                    .pr_1()
                    .when(window.is_fullscreen(), |row| row.pl_1())
                    .when(!window.is_fullscreen(), |row| row.child(Self::drag_strip()))
                    .child(
                        Self::tab_bar("compact-tabs")
                            .selected_index(self.active)
                            .on_click(cx.listener(|browser, index: &usize, window, cx| {
                                browser.activate(*index, window, cx)
                            }))
                            .children(tabs),
                    )
                    .child(
                        Button::new("compact-new-tab")
                            .icon(IconName::Plus)
                            .ghost()
                            .small()
                            .tooltip_with_action("New Tab", &NewTab, None)
                            .on_click(
                                cx.listener(|browser, _, window, cx| browser.new_tab(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .mx(px(FRAME))
                    .mb(px(FRAME))
                    .rounded(theme.radius_lg)
                    .bg(Self::chrome_fill(&self.tabs[self.active], cx))
                    .child(self.toolbar(self.active, cx)),
            )
    }

    fn start_surface(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let hint = |keys: AnyElement, text: &'static str| {
            h_flex()
                .gap_3()
                .child(h_flex().w(px(96.)).justify_end().child(keys))
                .child(text)
        };
        let binding = |action: &dyn gpui_kit::Action| -> AnyElement {
            match Kbd::binding_for_action(action, None, window) {
                Some(kbd) => kbd.into_any_element(),
                None => div().into_any_element(),
            }
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_6()
            .text_color(theme.muted_foreground)
            .child(icons::page().large().text_color(theme.muted_foreground))
            .child(
                v_flex()
                    .gap_2()
                    .text_sm()
                    .child(hint(
                        binding(&OpenPalette),
                        "Search tabs, commands and the web",
                    ))
                    .child(hint(
                        binding(&FocusAddressBar),
                        "Type an address or a search",
                    ))
                    .child(hint(
                        div().text_xs().child("Right-click").into_any_element(),
                        "The same search, wherever the pointer is",
                    ))
                    .child(hint(
                        div().text_xs().child("Drag a tab").into_any_element(),
                        "Onto a page's edge to see two side by side",
                    )),
            )
            .into_any_element()
    }

    fn home_status_bar(&self, id: u64, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(("home-status-bar", id))
            .debug_selector(move || format!("home-status-bar-{id}"))
            .flex_shrink_0()
            .w_full()
            .child(
                StatusBar::new()
                    .w_full()
                    .rounded_b(cx.theme().radius_lg)
                    .left(
                        Self::inked(
                            Button::new(("import-browser-data", id)),
                            Self::background_ink(cx.theme().status_bar, cx),
                            cx,
                        )
                        .debug_selector(move || format!("import-browser-data-{id}"))
                        .xsmall()
                        .icon(icons::import_browser_data())
                        .accessibility_label("Import browser data")
                        .tooltip("Import browser data…")
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.activate_id(id, window, cx);
                                this.show_profile_panel(Section::Import, window, cx);
                            },
                        )),
                    ),
            )
    }

    /// What a tab's card shows under its toolbar. The card paints the
    /// background, rounded, behind it; only a page's own picture has to be
    /// rounded to the card's corners, its top ones while there is no toolbar.
    fn content(&self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.tabs[index];
        match tab.showing {
            Showing::Page if let Some(page) = &tab.page => {
                let radius = cx.theme().radius_lg;
                let top = if self.compact { radius } else { px(0.) };
                let corners = Corners {
                    top_left: top,
                    top_right: top,
                    bottom_right: radius,
                    bottom_left: radius,
                };
                page.update(cx, |page, _| page.corners = corners);
                page.clone().into_any_element()
            }
            Showing::Serp if let Some(serp) = &tab.serp => serp.clone().into_any_element(),
            _ => self.start_surface(window, cx),
        }
    }

    /// The link under the pointer, over the page's corner.
    fn status_bubble(&self, index: usize, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let tab = &self.tabs[index];
        if tab.showing != Showing::Page {
            return None;
        }
        let status = tab.page.as_ref()?.read(cx).status()?;
        let theme = cx.theme();
        Some(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .m_1()
                .max_w(px(480.))
                .px_2()
                .py_0p5()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_xs()
                .text_color(theme.muted_foreground)
                .truncate()
                .child(status),
        )
    }
}

impl Focusable for Browser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The Share buttons drawn this frame record where they are.
        self.share_anchors.borrow_mut().clear();
        let theme = cx.theme().clone();
        let browser = cx.entity().downgrade();
        let navigator = self.navigator.as_ref().map(|(navigator, _)| {
            let tabs: Vec<TabEntry> = self
                .tabs
                .iter()
                .enumerate()
                .map(|(index, tab)| TabEntry {
                    id: tab.id,
                    title: tab.title(cx),
                    address: tab.address(cx),
                    icon: tab.icon(),
                    active: index == self.active,
                })
                .collect();
            let state = navigator::BrowserState {
                tabs: &tabs,
                compact: self.compact,
                appearance: theme::appearance(cx),
                ask: self.ask,
                shareable: self.share_url(self.active, cx).is_some(),
            };
            let on_pick = {
                let browser = browser.clone();
                move |pick, window: &mut Window, cx: &mut App| {
                    let _ = browser.update(cx, |browser, cx| browser.picked(pick, window, cx));
                }
            };
            let on_close = {
                let browser = browser.clone();
                move |window: &mut Window, cx: &mut App| {
                    let _ = browser.update(cx, |browser, cx| browser.close_navigator(window, cx));
                }
            };
            navigator.render(&state, on_pick, on_close, cx)
        });

        let suggestions = if self.suggestions.is_empty()
            || self.navigator.is_some()
            || self.address_anchor.borrow().is_none()
        {
            None
        } else {
            let mut group = CommandGroup::new().label("Bookmarks and history · ↓ to choose");
            for entry in &self.suggestions {
                group = group.item(
                    CommandItem::new()
                        .label(if entry.title.is_empty() {
                            entry.url.clone()
                        } else {
                            entry.title.clone()
                        })
                        .keywords([entry.url.clone()]),
                );
            }
            let anchor = self.address_anchor.borrow().unwrap();
            let entries = self.suggestions.clone();
            let browser = cx.weak_entity();
            Some(
                gpui_kit::deferred(
                    gpui_kit::anchored()
                        .position(anchor.bottom_left())
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            div()
                                .w(anchor.size.width.max(px(240.)))
                                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                    this.suggestions.clear();
                                    cx.notify();
                                }))
                                .occlude()
                                .bg(theme.popover)
                                .text_color(theme.popover_foreground)
                                .rounded(theme.radius_lg)
                                .shadow_lg()
                                .child(
                                    Command::new(&self.suggestion_state)
                                        .searchable(false)
                                        .filterable(false)
                                        .group(group)
                                        .max_h(px(320.))
                                        .on_confirm(move |ix, window, cx| {
                                            if let Some(url) = entries
                                                .get(ix.row)
                                                .and_then(|e| Url::parse(&e.url).ok())
                                            {
                                                let _ = browser.update(cx, |this, cx| {
                                                    this.go(url, false, false, window, cx)
                                                });
                                            }
                                        })
                                        .on_cancel({
                                            let browser = cx.weak_entity();
                                            move |window, cx| {
                                                let _ = browser.update(cx, |this, cx| {
                                                    this.suggestions.clear();
                                                    this.focus_address_bar(window, cx);
                                                });
                                            }
                                        }),
                                ),
                        ),
                )
                .with_priority(2),
            )
        };

        div()
            .id("browser")
            .key_context("Browser")
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(glass::frame(cx))
            .text_color(theme.foreground)
            .on_action(cx.listener(|browser, _: &ImportBrowserData, window, cx| {
                browser.show_profile_panel(Section::Import, window, cx)
            }))
            .on_action(cx.listener(|browser, _: &ShowProfiles, window, cx| {
                browser.show_profile_panel(Section::Profiles, window, cx)
            }))
            .on_action(cx.listener(|browser, _: &ShowBookmarks, window, cx| {
                browser.show_profile_panel(Section::Bookmarks, window, cx)
            }))
            .on_action(cx.listener(|browser, _: &ShowHistory, window, cx| {
                browser.show_profile_panel(Section::History, window, cx)
            }))
            .on_action(cx.listener(|browser, _: &BookmarkPage, window, cx| {
                browser.bookmark_page(window, cx)
            }))
            .on_action(cx.listener(|browser, _: &FillPassword, window, cx| {
                browser.choose_password(window, cx)
            }))
            .on_action(
                cx.listener(|browser, _: &FillForm, window, cx| browser.fill_form(window, cx)),
            )
            .on_action(cx.listener(|browser, _: &SavePassword, window, cx| {
                browser.save_password(window, cx)
            }))
            .on_action(cx.listener(|browser, _: &NewTab, window, cx| browser.new_tab(window, cx)))
            .on_action(cx.listener(|browser, _: &CloseTab, window, cx| {
                browser.close_tab(browser.active, window, cx)
            }))
            .on_action(cx.listener(|browser, _: &NextTab, window, cx| browser.cycle(1, window, cx)))
            .on_action(
                cx.listener(|browser, _: &PreviousTab, window, cx| browser.cycle(-1, window, cx)),
            )
            .on_action(cx.listener(Self::activate_numbered))
            .on_action(cx.listener(|browser, _: &Reload, _, cx| browser.reload(cx)))
            .on_action(cx.listener(|browser, _: &GoBack, window, cx| browser.back(window, cx)))
            .on_action(
                cx.listener(|browser, _: &GoForward, window, cx| browser.forward(window, cx)),
            )
            .on_action(cx.listener(|browser, _: &FocusAddressBar, window, cx| {
                browser.focus_address_bar(window, cx)
            }))
            .on_action(cx.listener(|browser, _: &OpenPalette, window, cx| {
                browser.toggle_palette(window, cx)
            }))
            .on_action(cx.listener(|browser, _: &ToggleCompact, window, cx| {
                browser.toggle_compact(window, cx)
            }))
            .on_action(
                cx.listener(|browser, _: &ToggleAsk, window, cx| browser.toggle_ask(window, cx)),
            )
            .on_action(
                cx.listener(|browser, _: &ShowDownloads, _, cx| browser.toggle_downloads(cx)),
            )
            .on_action(cx.listener(|browser, _: &SharePage, window, cx| {
                browser.share_tab(browser.tabs[browser.active].id, window, cx)
            }))
            .on_action(cx.listener(|browser, _: &crate::actions::ShowCanvas, window, cx| {
                if let Some(board) = browser.canvas.take() {
                    board.update(cx, |board, cx| board.detach(cx));
                    browser.focus_content(window, cx);
                } else if let Some(page) = browser.tabs[browser.active].page.clone() {
                    let board = cx.new(|cx| crate::canvas::SpatialCanvas::new(page, cx));
                    window.focus(&board.focus_handle(cx), cx);
                    browser.canvas = Some(board);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
            .capture_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "down"
                        && !this.suggestions.is_empty()
                        && this.omnibox.focus_handle(cx).is_focused(window)
                    {
                        this.suggestion_state
                            .update(cx, |state, cx| state.focus(window, cx));
                        cx.stop_propagation();
                    }
                }),
            )
            .on_mouse_down(MouseButton::Right, cx.listener(Self::right_click))
            .child(div().flex_1().min_h_0().child(match &self.canvas {
                Some(board) => board.clone().into_any_element(),
                None => self.dock.clone().into_any_element(),
            }))
            .when(self.compact, |this| {
                this.child(
                    // Pointing at the top edge brings the chrome back over the page.
                    div()
                        .id("reveal")
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(FRAME * 2.))
                        .on_hover(cx.listener(|browser, hovered: &bool, _, cx| {
                            if *hovered && !browser.revealed {
                                browser.revealed = true;
                                cx.notify();
                            }
                        })),
                )
                .when(self.revealed, |this| {
                    this.child(self.compact_chrome(window, cx))
                })
            })
            .children(navigator)
            .children(suggestions)
    }
}

#[cfg(test)]
mod tests;
