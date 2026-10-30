//! Home: what Frame shows before any page, the front door of the native
//! workspace (`docs/vision/native-workspace.md`). It shelves the places
//! beneath the pages and lists what to continue with. A new tab shows it
//! under its own address bar; with every tab closed, the workspace shows it
//! with a search field of its own, the widest thing on it.
//!
//! It is part of the chrome, not a page: it has no surface of its own and
//! sits on the window's frame, or on a tab's pane of it, never on a page's
//! card. Its places are a launcher's shelf, a little above the middle, with
//! the keys to know along the foot.
//!
//! Its parts differ by shape, weight and words, never by color alone: places
//! are icons on neutral tiles with their names, the things to continue are
//! rows with a title and one line under it.
//!
//! SOURCING: GPUI Kit's `Button`, `Icon` and `Kbd`; the record is
//! `recents.rs`.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    kbd::Kbd,
    scroll::ScrollableElement as _,
    v_flex,
};
use gpui_kit::{
    Action, AnyElement, App, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement, RenderOnce, SharedString, Styled, Window, div, prelude::FluentBuilder as _,
    rems,
};
use url::Url;

use crate::actions::{FocusAddressBar, NewTab, OpenPalette};
use crate::icons;
use crate::places::Place;
use crate::recents::{self, Recent, RecentSearch};

/// What the person picked on Home. The host decides where it opens.
#[derive(Clone, Debug, PartialEq)]
pub enum HomeEvent {
    Place(Place),
    /// A recent page; in a new tab when `new_tab` (⌘-click).
    Page {
        url: Url,
        new_tab: bool,
    },
    /// A recent search, run again.
    Search {
        query: String,
        ask: bool,
        new_tab: bool,
    },
}

type Handler = Rc<dyn Fn(&HomeEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Home {
    id: ElementId,
    field: Option<AnyElement>,
    on_event: Option<Handler>,
}

impl Home {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            field: None,
            on_event: None,
        }
    }

    /// A search field above everything else, for when no address bar is
    /// showing.
    pub fn field(mut self, field: impl IntoElement) -> Self {
        self.field = Some(field.into_any_element());
        self
    }

    pub fn on_event(
        mut self,
        handler: impl Fn(&HomeEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_event = Some(Rc::new(handler));
        self
    }
}

fn heading(text: &'static str) -> impl IntoElement {
    div()
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

fn emit(handler: &Option<Handler>, event: HomeEvent, window: &mut Window, cx: &mut App) {
    if let Some(handler) = handler {
        handler(&event, window, cx);
    }
}

/// How many things Continue lists.
const CONTINUE: usize = 6;

impl Home {
    /// The places, as a launcher's shelf: each an icon on a neutral tile,
    /// with its name under it.
    fn shelf(&self, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let (tile, radius) = (theme.secondary, theme.radius_lg);
        h_flex()
            .justify_center()
            .flex_wrap()
            .gap_2()
            .children(Place::ALL.map(|place| {
                let handler = self.on_event.clone();
                Button::new(SharedString::from(format!("home-place-{}", place.key())))
                    .debug_selector(move || format!("home-place-{}", place.key()))
                    .ghost()
                    .w(rems(5.5))
                    .h(rems(5.75))
                    .accessibility_label(if place.is_built() {
                        SharedString::from(place.label())
                    } else {
                        format!("{}, not built yet", place.label()).into()
                    })
                    .tooltip(place.summary())
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                div()
                                    .size(rems(3.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(radius)
                                    .bg(tile)
                                    .child(place.icon().size(rems(1.75))),
                            )
                            .child(div().text_sm().child(place.label())),
                    )
                    .on_click(move |_, window, cx| {
                        emit(&handler, HomeEvent::Place(place), window, cx)
                    })
            }))
    }

    /// One thing to continue: its icon, its title, and one line saying
    /// where it is or what kind it is.
    fn continue_row(&self, recent: Recent, cx: &App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let (id, icon, title, detail, label, event) = match recent {
            Recent::Page(page) => {
                let host = page.url.host_str().unwrap_or_default();
                let host = host.strip_prefix("www.").unwrap_or(host).to_string();
                let title = if page.title.is_empty() {
                    host.clone()
                } else {
                    page.title.clone()
                };
                (
                    format!("recent-page:{}", page.url),
                    Icon::new(IconName::Globe),
                    title.clone(),
                    host.clone(),
                    format!("{title}, {host}"),
                    HomeEvent::Page {
                        url: page.url.clone(),
                        new_tab: false,
                    },
                )
            }
            Recent::Search(RecentSearch { query, ask, .. }) => (
                format!(
                    "recent-search:{}:{query}",
                    if *ask { "ask" } else { "search" }
                ),
                if *ask {
                    icons::ask_on()
                } else {
                    Icon::new(IconName::Search)
                },
                query.clone(),
                if *ask { "Asked" } else { "Searched the web" }.to_string(),
                if *ask {
                    format!("Ask {query}")
                } else {
                    format!("Search {query}")
                },
                HomeEvent::Search {
                    query: query.clone(),
                    ask: *ask,
                    new_tab: false,
                },
            ),
        };
        let handler = self.on_event.clone();
        let selector = id.clone();
        Button::new(SharedString::from(id))
            .debug_selector(move || selector.clone())
            .ghost()
            .w_full()
            .h(rems(3.25))
            .justify_start()
            .accessibility_label(label)
            .when_some(
                match &event {
                    HomeEvent::Page { url, .. } => Some(url.to_string()),
                    _ => None,
                },
                |button, url| button.tooltip(url),
            )
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .child(icon.text_color(muted))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .items_start()
                            .child(div().w_full().truncate().child(title))
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(detail),
                            ),
                    ),
            )
            .on_click(move |click, window, cx| {
                // ⌘-click opens it in a new tab.
                let new_tab = click.modifiers().secondary();
                let event = match event.clone() {
                    HomeEvent::Page { url, .. } => HomeEvent::Page { url, new_tab },
                    HomeEvent::Search { query, ask, .. } => HomeEvent::Search {
                        query,
                        ask,
                        new_tab,
                    },
                    event => event,
                };
                emit(&handler, event, window, cx)
            })
    }

    /// The newest pages and searches together, to pick up again.
    fn continue_list(&self, cx: &App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let rows: Vec<AnyElement> = recents::latest(CONTINUE, cx)
            .into_iter()
            .map(|recent| self.continue_row(recent, cx).into_any_element())
            .collect();
        let empty = rows.is_empty();
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .child(heading("Continue"))
                    .when(!empty, |this| {
                        this.child(
                            Button::new("home-clear-recents")
                                .ghost()
                                .xsmall()
                                .label("Clear")
                                .tooltip("Clear recent pages and searches")
                                .on_click(|_, _, cx| recents::clear(cx)),
                        )
                    }),
            )
            .when(empty, |this| {
                this.child(
                    div().text_sm().text_color(muted).child(
                        "Pages you open and searches you run appear here, to pick up again.",
                    ),
                )
            })
            .when(!empty, |this| {
                this.child(div().grid().grid_cols(2).gap_x_4().gap_y_1().children(rows))
            })
    }

    /// The keys to know, in one line along the foot.
    fn shortcuts(&self, window: &mut Window, cx: &App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let binding = |action: &dyn Action, window: &mut Window| -> AnyElement {
            match Kbd::binding_for_action(action, None, window) {
                Some(kbd) => kbd.into_any_element(),
                None => div().into_any_element(),
            }
        };
        let hint = |keys: AnyElement, text: &'static str| h_flex().gap_2().child(keys).child(text);
        let word = |text: &'static str| {
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(text)
                .into_any_element()
        };
        h_flex()
            .flex_shrink_0()
            .flex_wrap()
            .justify_center()
            .gap_x_6()
            .gap_y_2()
            .py_4()
            .text_xs()
            .text_color(muted)
            .child(hint(binding(&NewTab, window), "New tab"))
            .child(hint(
                binding(&OpenPalette, window),
                "Search tabs, commands and the web",
            ))
            .child(hint(
                binding(&FocusAddressBar, window),
                "Type an address or a search",
            ))
            .child(hint(word("Right-click"), "The same search, anywhere"))
            .child(hint(
                word("Drag a tab"),
                "To a page's edge, for two side by side",
            ))
    }
}

impl RenderOnce for Home {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let shelf = self.shelf(cx).into_any_element();
        let continue_list = self.continue_list(cx).into_any_element();
        let shortcuts = self.shortcuts(window, cx).into_any_element();
        v_flex()
            .size_full()
            .items_center()
            .px_6()
            .text_color(cx.theme().foreground)
            .overflow_y_scrollbar()
            .id(self.id)
            .child(div().flex_1().min_h(rems(2.)))
            .child(
                v_flex()
                    .flex_shrink_0()
                    .w_full()
                    .max_w(rems(46.))
                    .gap(rems(2.5))
                    .when_some(self.field, |this, field| this.child(field))
                    .child(shelf)
                    .child(continue_list),
            )
            .child(div().flex_1().min_h(rems(2.)))
            .child(shortcuts)
    }
}
