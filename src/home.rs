//! Home: what Frame shows before any page, the front door of the native
//! workspace (`docs/vision/native-workspace.md`). It lists the places beneath
//! the pages and the pages and searches to get back to. A new tab shows it
//! under its own address bar; with every tab closed, the workspace shows it
//! with a search field of its own.
//!
//! It is part of the chrome, not a page: it has no surface of its own and
//! sits on the window's frame, or on a tab's pane of it, never on a page's
//! card. Its places are a launcher's row of tiles, a little above the middle,
//! with the keys to know along the foot.
//!
//! Its parts differ by shape, weight and words, never by color alone: places
//! are tiles with an icon in a well, recent items are quiet rows with an icon
//! and a word for their kind.
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
    Action, AnyElement, App, ElementId, FontWeight, IntoElement, ParentElement, RenderOnce,
    SharedString, Styled, Window, div, prelude::FluentBuilder as _, rems,
};
use url::Url;

use crate::actions::{FocusAddressBar, NewTab, OpenPalette};
use crate::icons;
use crate::places::Place;
use crate::recents::{self, RecentPage, RecentSearch};

/// What the person picked on Home. The host decides where it opens.
#[derive(Clone, Debug, PartialEq)]
pub enum HomeEvent {
    Place(Place),
    /// A recent page; in a new tab when `new_tab` (⌘-click).
    Page { url: Url, new_tab: bool },
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

impl Home {
    fn places(&self, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, well, border, radius) = (
            theme.muted_foreground,
            theme.background,
            theme.border,
            theme.radius_lg,
        );
        v_flex()
            .items_center()
            .gap_3()
            .child(
                h_flex()
                    .justify_center()
                    .flex_wrap()
                    .gap_2()
                    .children(Place::ALL.map(|place| {
                        let handler = self.on_event.clone();
                        Button::new(SharedString::from(format!("home-place-{}", place.key())))
                            .ghost()
                            .w(rems(6.))
                            .h(rems(6.5))
                            .accessibility_label(format!("{}, not built yet", place.label()))
                            .tooltip(place.summary())
                            .child(
                                v_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .size(rems(3.25))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(radius)
                                            .border_1()
                                            .border_color(border)
                                            .bg(well)
                                            .shadow_xs()
                                            .child(Icon::new(place.icon()).large()),
                                    )
                                    .child(div().text_sm().child(place.label())),
                            )
                            .on_click(move |_, window, cx| {
                                emit(&handler, HomeEvent::Place(place), window, cx)
                            })
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Not built yet: each place shows what it will hold."),
            )
    }

    fn page_row(&self, page: &RecentPage, cx: &App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let host = page.url.host_str().unwrap_or_default();
        let host = host.strip_prefix("www.").unwrap_or(host).to_string();
        let title = if page.title.is_empty() {
            host.clone()
        } else {
            page.title.clone()
        };
        let url = page.url.clone();
        let handler = self.on_event.clone();
        Button::new(SharedString::from(format!("recent-page:{}", page.url)))
            .ghost()
            .w_full()
            .justify_start()
            .accessibility_label(format!("{title}, {host}"))
            .tooltip(page.url.to_string())
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(Icon::new(IconName::Globe).small().text_color(muted))
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .child(div().flex_shrink_0().text_xs().text_color(muted).child(host)),
            )
            .on_click(move |event, window, cx| {
                let new_tab = event.modifiers().secondary();
                emit(
                    &handler,
                    HomeEvent::Page {
                        url: url.clone(),
                        new_tab,
                    },
                    window,
                    cx,
                )
            })
    }

    fn search_row(&self, search: &RecentSearch, cx: &App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let RecentSearch { query, ask } = search.clone();
        let icon = if ask {
            icons::ask_on()
        } else {
            Icon::new(IconName::Search)
        };
        let handler = self.on_event.clone();
        Button::new(SharedString::from(format!(
            "recent-search:{}:{query}",
            if ask { "ask" } else { "search" }
        )))
        .ghost()
        .w_full()
        .justify_start()
        .accessibility_label(if ask {
            format!("Ask {query}")
        } else {
            format!("Search {query}")
        })
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(icon.small().text_color(muted))
                .child(div().flex_1().min_w_0().truncate().child(query.clone()))
                .when(ask, |this| {
                    this.child(div().flex_shrink_0().text_xs().text_color(muted).child("Ask"))
                }),
        )
        .on_click(move |event, window, cx| {
            let new_tab = event.modifiers().secondary();
            emit(
                &handler,
                HomeEvent::Search {
                    query: query.clone(),
                    ask,
                    new_tab,
                },
                window,
                cx,
            )
        })
    }

    fn recent(&self, cx: &App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let pages = recents::pages(cx);
        let searches = recents::searches(cx);
        let empty = pages.is_empty() && searches.is_empty();
        let column = |title: &'static str, rows: Vec<AnyElement>| {
            v_flex()
                .flex_1()
                .min_w(rems(14.))
                .gap_1()
                .child(div().text_xs().text_color(muted).pb_1().child(title))
                .children(rows)
        };
        let page_rows: Vec<AnyElement> = pages
            .iter()
            .map(|page| self.page_row(page, cx).into_any_element())
            .collect();
        let search_rows: Vec<AnyElement> = searches
            .iter()
            .map(|search| self.search_row(search, cx).into_any_element())
            .collect();
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .child(heading("Recent"))
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
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("Pages you open and searches you run appear here."),
                )
            })
            .when(!empty, |this| {
                this.child(
                    h_flex()
                        .items_start()
                        .flex_wrap()
                        .gap_6()
                        .when(!page_rows.is_empty(), |this| {
                            this.child(column("Pages", page_rows))
                        })
                        .when(!search_rows.is_empty(), |this| {
                            this.child(column("Searches", search_rows))
                        }),
                )
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
        let word = |text: &'static str| div().font_weight(FontWeight::MEDIUM).child(text).into_any_element();
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
            .child(hint(word("Drag a tab"), "To a page's edge, for two side by side"))
    }
}

impl RenderOnce for Home {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let places = self.places(cx).into_any_element();
        let recent = self.recent(cx).into_any_element();
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
                    .max_w(rems(40.))
                    .gap(rems(2.5))
                    .when_some(self.field, |this, field| this.child(field))
                    .child(places)
                    .child(recent),
            )
            .child(div().flex_1().min_h(rems(2.)))
            .child(shortcuts)
    }
}
