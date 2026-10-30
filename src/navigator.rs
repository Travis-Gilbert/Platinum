//! Platinum's navigator: one searchable list shown two ways. Right-click puts it
//! at the pointer as a menu whose first row is the search field, already
//! focused, so the person can just start typing; ⌘K shows the same list as a
//! command palette at the top of the window, whether or not the address bar
//! is on screen.
//!
//! The list is local first: the page's own context menu entries, the open
//! tabs (Switch), Frame's commands, Home and the places beneath the pages,
//! and only then the web (RustyWeb search, and Ask, which is the one explicit
//! AI entry).
//!
//! SOURCING: GPUI Kit's `Command` (search field, groups, keyboard selection,
//! filtering, keybinding hints) inside GPUI's `anchored` / `deferred`
//! overlay. It is drawn inside the browser view rather than as a Root dialog
//! so that the commands it dispatches reach the browser's action handlers.

use std::rc::Rc;

use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, IconName, IndexPath};
use gpui_kit::{
    Action, AnyElement, App, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Point, SharedString, Styled as _, Window, anchored, deferred, div,
    px,
};
use servo::ContextMenuAction;
use url::Url;

use crate::actions::{
    BookmarkPage, CloseTab, FillPassword, FocusAddressBar, GoBack, GoForward, ImportBrowserData,
    NewTab, PinTab, Quit, Reload, SavePassword, SetAppearance, SharePage, ShowBookmarks,
    ShowConsole, ShowDownloads, ShowHistory, ShowProfiles, ToggleAsk, ToggleCompact, TogglePlaces,
    ToggleSidebar,
};
use crate::address::{Target, classify};
use crate::icons;
use crate::page::{Page, PageMenu};
use crate::places::Place;
use crate::theme::Appearance;

/// Where the navigator is on screen.
#[derive(Clone, Copy)]
pub enum Placement {
    /// A menu at the pointer, from a right-click.
    Pointer(Point<Pixels>),
    /// The ⌘K palette.
    Palette,
}

/// What choosing an entry does, beyond any action the entry dispatched.
#[derive(Clone)]
pub enum Pick {
    Page(ContextMenuAction),
    SwitchTab(u64),
    /// A Platinum command; its action already ran.
    Command,
    Go(Url),
    Search(String),
    Ask(String),
    /// Save the link or image under the pointer.
    Download(Url),
    Home,
    Place(Place),
    Tab(TabAction),
}

/// Actions on a tab or pinned site, keyed by stable identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAction {
    Close(u64),
    /// The tab and every tab under it in the sidebar.
    CloseBranch(u64),
    Fold(u64),
    Unfold(u64),
    Rename(u64),
    Pin(u64),
    Unpin(u64),
}

impl TabAction {
    fn label(self) -> &'static str {
        match self {
            Self::Close(_) => "Close Tab",
            Self::CloseBranch(_) => "Close Branch",
            Self::Fold(_) => "Collapse Branch",
            Self::Unfold(_) => "Expand Branch",
            Self::Rename(_) => "Rename...",
            Self::Pin(_) => "Pin Tab",
            Self::Unpin(_) => "Unpin Site",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Close(_) | Self::CloseBranch(_) => Icon::new(IconName::Close),
            Self::Fold(_) => Icon::new(IconName::ChevronRight),
            Self::Unfold(_) => Icon::new(IconName::ChevronDown),
            Self::Rename(_) => icons::rename(),
            Self::Pin(_) => icons::pin(),
            Self::Unpin(_) => icons::unpin(),
        }
    }
}

/// The entries for the tab or pinned site under the pointer.
pub struct TabMenu {
    /// "Tab" or "Pinned Site".
    pub label: SharedString,
    pub actions: Vec<TabAction>,
}
/// One open tab, as the navigator lists it.
pub struct TabEntry {
    pub id: u64,
    pub title: SharedString,
    pub address: SharedString,
    pub icon: Icon,
    pub active: bool,
}

/// What the browser knows that the list needs.
pub struct BrowserState<'a> {
    pub tabs: &'a [TabEntry],
    pub compact: bool,
    pub appearance: Appearance,
    /// The address bar asks instead of searching.
    pub ask: bool,
    /// The active tab shows a web page that can be shared.
    pub shareable: bool,
    pub sidebar: bool,
    /// The places bump stands over the tabs and the workspace too.
    pub places_pinned: bool,
    /// The active tab shows a site that can be pinned.
    pub pinnable: bool,
}

pub struct Navigator {
    pub state: Entity<CommandState>,
    pub placement: Placement,
    /// The page whose context menu this is, with its entries; the page holds
    /// the responder until the navigator answers it.
    pub page: Option<(Entity<Page>, PageMenu)>,
    /// The tab or pinned site whose context menu this is.
    pub tab: Option<TabMenu>,
    /// Where focus goes back to when the navigator closes.
    pub restore_focus: Option<FocusHandle>,
}

impl Navigator {
    /// The groups for the current query, and what each entry does, in the
    /// same order, so a confirmed `IndexPath` finds its `Pick`.
    pub fn entries(
        &self,
        query: &str,
        browser: &BrowserState,
    ) -> (Vec<CommandGroup>, Vec<Vec<Pick>>) {
        let query = query.trim();
        let typing = !query.is_empty();
        let mut groups = Vec::new();
        let mut picks = Vec::new();

        if let Some((_, menu)) = &self.page {
            let mut group = CommandGroup::new().label("Page");
            let mut group_picks = Vec::new();
            for item in &menu.items {
                let mut row = CommandItem::new()
                    .label(item.label.clone())
                    .disabled(!item.enabled);
                if let Some(icon) = page_icon(item.action) {
                    row = row.icon(icon);
                }
                group = group.item(row);
                group_picks.push(Pick::Page(item.action));
            }
            let downloads = [
                ("Download Linked File", &menu.link),
                ("Download Image", &menu.image),
            ];
            for (label, url) in downloads {
                if let Some(url) = url.as_ref().filter(|url| downloadable(url)) {
                    group = group.item(CommandItem::new().label(label).icon(icons::downloads()));
                    group_picks.push(Pick::Download(url.clone()));
                }
            }
            groups.push(group);
            picks.push(group_picks);
        }
        if let Some(menu) = &self.tab {
            let mut group = CommandGroup::new().label(menu.label.clone());
            for action in &menu.actions {
                group = group.item(CommandItem::new().label(action.label()).icon(action.icon()));
            }
            groups.push(group);
            picks.push(menu.actions.iter().copied().map(Pick::Tab).collect());
        }

        let pointer = matches!(self.placement, Placement::Pointer(_));
        // The right-click menu leads with what can be done: its tabs show only
        // once the person types. The palette keeps them, as a switcher.
        let mut tabs = None;
        if !browser.tabs.is_empty() && (typing || !pointer) {
            let mut group = CommandGroup::new().label("Tabs");
            let mut group_picks = Vec::new();
            for tab in browser.tabs {
                group = group.item(
                    CommandItem::new()
                        .label(tab.title.clone())
                        .icon(tab.icon.clone())
                        .keywords([tab.address.clone()])
                        .checked(tab.active),
                );
                group_picks.push(Pick::SwitchTab(tab.id));
            }
            tabs = Some((group, group_picks));
        }
        // Filtering keeps the groups' order and Enter takes the first match.
        // Untyped, the palette's tabs come first as a switcher; typed, a query
        // that names a command means the command ("new tab" over a start tab
        // titled "New Tab"), and one that only matches tabs still finds them.
        if !typing && let Some((group, group_picks)) = tabs.take() {
            groups.push(group);
            picks.push(group_picks);
        }

        // (label, icon, action, shown untyped in the right-click menu, checked)
        let mut commands: Vec<(SharedString, Icon, Box<dyn Action>, bool, bool)> = vec![
            (
                "New Tab".into(),
                icons::page(),
                Box::new(NewTab),
                true,
                false,
            ),
            (
                if browser.compact {
                    "Show Toolbar"
                } else {
                    "Compact Mode"
                }
                .into(),
                Icon::new(if browser.compact {
                    IconName::Maximize
                } else {
                    IconName::Minimize
                }),
                Box::new(ToggleCompact),
                true,
                false,
            ),
            (
                if browser.sidebar {
                    "Hide Sidebar"
                } else {
                    "Show Sidebar"
                }
                .into(),
                Icon::new(IconName::PanelLeft),
                Box::new(ToggleSidebar),
                true,
                false,
            ),
            (
                if browser.places_pinned {
                    "Show Places Only in Places"
                } else {
                    "Always Show Places"
                }
                .into(),
                icons::apps(),
                Box::new(TogglePlaces),
                true,
                false,
            ),
        ];
        // A tab's own menu pins that tab, which may not be the active one.
        if browser.pinnable && self.tab.is_none() {
            commands.push((
                "Pin Tab".into(),
                icons::pin(),
                Box::new(PinTab),
                !pointer,
                false,
            ));
        }
        if browser.shareable {
            commands.push((
                "Share Page".into(),
                icons::share(),
                Box::new(SharePage),
                true,
                false,
            ));
        }
        commands.push((
            "Downloads".into(),
            icons::downloads(),
            Box::new(ShowDownloads),
            !pointer,
            false,
        ));
        commands.push((
            "Console".into(),
            icons::page(),
            Box::new(ShowConsole),
            !pointer,
            false,
        ));
        for (label, action) in [
            (
                "Import browser data…",
                Box::new(ImportBrowserData) as Box<dyn Action>,
            ),
            ("Profiles…", Box::new(ShowProfiles)),
            ("History…", Box::new(ShowHistory)),
            ("Bookmarks…", Box::new(ShowBookmarks)),
            ("Bookmark this page", Box::new(BookmarkPage)),
            ("Fill saved password…", Box::new(FillPassword)),
            ("Save or update password…", Box::new(SavePassword)),
        ] {
            commands.push((
                label.into(),
                Icon::new(IconName::Globe),
                action,
                !pointer,
                false,
            ));
        }
        for appearance in Appearance::ALL {
            commands.push((
                appearance.command(),
                appearance.icon(),
                Box::new(SetAppearance { appearance }),
                !pointer,
                appearance == browser.appearance,
            ));
        }
        commands.extend([
            (
                if browser.ask {
                    "Turn Off Ask"
                } else {
                    "Turn On Ask"
                }
                .into(),
                icons::ask(),
                Box::new(ToggleAsk) as Box<dyn Action>,
                !pointer,
                false,
            ),
            (
                "Address Bar".into(),
                Icon::new(IconName::Globe),
                Box::new(FocusAddressBar),
                !pointer,
                false,
            ),
            (
                "Back".into(),
                Icon::new(IconName::ArrowLeft),
                Box::new(GoBack),
                !pointer,
                false,
            ),
            (
                "Forward".into(),
                Icon::new(IconName::ArrowRight),
                Box::new(GoForward),
                !pointer,
                false,
            ),
            (
                "Reload".into(),
                Icon::new(IconName::RefreshCw),
                Box::new(Reload),
                !pointer,
                false,
            ),
            (
                "Close Tab".into(),
                Icon::new(IconName::Close),
                Box::new(CloseTab),
                !pointer,
                false,
            ),
            (
                format!("Quit {}", crate::brand::NAME).into(),
                Icon::new(IconName::WindowClose),
                Box::new(Quit),
                !pointer,
                false,
            ),
        ]);
        // The right-click menu stays short until the person types; then every
        // command is reachable.
        commands.retain(|(_, _, _, shown, _)| *shown || typing);
        let mut group = CommandGroup::new().label(crate::brand::NAME);
        let mut group_picks = Vec::new();
        for (label, icon, action, _, checked) in commands {
            group = group.item(
                CommandItem::new()
                    .label(label)
                    .icon(icon)
                    .action(action)
                    .checked(checked),
            );
            group_picks.push(Pick::Command);
        }
        groups.push(group);
        picks.push(group_picks);
        if let Some((group, group_picks)) = tabs {
            groups.push(group);
            picks.push(group_picks);
        }

        // Out of the short right-click menu until the person types. Before
        // the web, so "mail" opens Mail.
        if !pointer || typing {
            let mut group = CommandGroup::new()
                .label("Places")
                .item(CommandItem::new().label("Home").icon(icons::home()));
            let mut group_picks = vec![Pick::Home];
            for place in Place::ALL {
                group = group.item(CommandItem::new().label(place.label()).icon(place.icon()));
                group_picks.push(Pick::Place(place));
            }
            groups.push(group);
            picks.push(group_picks);
        }

        if typing {
            let mut group = CommandGroup::new().label("Web");
            let mut group_picks = Vec::new();
            if let Some(Target::Go(url)) = classify(query) {
                group = group.item(
                    CommandItem::new()
                        .label(format!("Open {url}"))
                        .icon(IconName::ExternalLink)
                        .keywords([query.to_owned()]),
                );
                group_picks.push(Pick::Go(url));
            }
            group = group.item(
                CommandItem::new()
                    .label(format!("Search the web for “{query}”"))
                    .icon(IconName::Search)
                    .keywords([query.to_owned()]),
            );
            group_picks.push(Pick::Search(query.to_owned()));
            group = group.item(
                CommandItem::new()
                    .label(format!("Ask “{query}”"))
                    .icon(icons::ask())
                    .keywords([query.to_owned()]),
            );
            group_picks.push(Pick::Ask(query.to_owned()));
            groups.push(group);
            picks.push(group_picks);
        }

        (groups, picks)
    }

    /// The navigator, placed. `on_pick` runs for a confirmed entry and
    /// `on_close` for Escape or a click outside.
    pub fn render(
        &self,
        browser: &BrowserState,
        on_pick: impl Fn(Pick, &mut Window, &mut App) + 'static,
        on_close: impl Fn(&mut Window, &mut App) + 'static,
        cx: &App,
    ) -> AnyElement {
        let query = self.state.read(cx).query(cx);
        let (groups, picks) = self.entries(&query, browser);
        let picks = Rc::new(picks);
        let on_close = Rc::new(on_close);
        let theme = cx.theme();

        let mut command = Command::new(&self.state)
            .placeholder(match self.placement {
                Placement::Pointer(_) => "Search tabs, commands and the web...",
                Placement::Palette => "Search tabs, commands and the web, or type an address...",
            })
            .on_confirm({
                let picks = picks.clone();
                move |index: IndexPath, window, cx| {
                    if let Some(pick) = picks.get(index.section).and_then(|g| g.get(index.row)) {
                        on_pick(pick.clone(), window, cx);
                    }
                }
            })
            .on_cancel({
                let on_close = on_close.clone();
                move |window, cx| {
                    let on_close = on_close.clone();
                    window.defer(cx, move |window, cx| on_close(window, cx));
                }
            })
            .empty(|_, _, _| div().p_3().text_sm().child("Nothing matches."));
        for group in groups {
            command = command.group(group);
        }

        let panel = div()
            .id("navigator")
            .debug_selector(|| "navigator".into())
            .occlude()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .rounded(theme.radius_lg)
            .shadow_lg()
            .on_mouse_down_out(move |_, window, cx| on_close(window, cx));

        match self.placement {
            Placement::Pointer(position) => deferred(
                anchored()
                    .position(position)
                    .snap_to_window_with_margin(px(8.))
                    .child(panel.w(px(280.)).child(command.max_h(px(360.)))),
            )
            .with_priority(1)
            .into_any_element(),
            Placement::Palette => deferred(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .justify_center()
                    .pt(px(72.))
                    .child(panel.w(px(560.)).h_auto().child(command.max_h(px(420.)))),
            )
            .with_priority(1)
            .into_any_element(),
        }
    }
}

/// Platinum fetches downloads itself, over the network.
fn downloadable(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

/// The closest bundled icon for a page menu entry, if there is one.
fn page_icon(action: ContextMenuAction) -> Option<IconName> {
    Some(match action {
        ContextMenuAction::GoBack => IconName::ArrowLeft,
        ContextMenuAction::GoForward => IconName::ArrowRight,
        ContextMenuAction::Reload => IconName::RefreshCw,
        ContextMenuAction::CopyLink
        | ContextMenuAction::CopyImageLink
        | ContextMenuAction::Copy => IconName::Copy,
        ContextMenuAction::OpenLinkInNewWebView | ContextMenuAction::OpenImageInNewView => {
            IconName::ExternalLink
        }
        ContextMenuAction::Cut | ContextMenuAction::Paste | ContextMenuAction::SelectAll => {
            return None;
        }
    })
}
