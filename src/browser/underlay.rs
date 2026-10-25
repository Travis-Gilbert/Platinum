//! The native workspace beneath the pages (`docs/vision/native-workspace.md`):
//! what the window shows while no tab is open, at launch and after the last
//! tab closes. It has routes of its own, Home, results and the places, with
//! their own Back and Forward, and a search field where the address bar
//! would be. Opening a page or a result, or a new tab, brings the tabs back.
//!
//! While it shows, the browser keeps one fresh tab behind it, the tab it
//! stands for: a page opened from here loads there.
//!
//! Home and the places are part of the chrome and sit on the window's frame
//! itself. A results page is a document, so it sits on a flat, opaque card
//! with a border, where a page's card is raised on a shadow: the workspace and
//! the web differ by texture as well as color.
//!
//! SOURCING: GPUI Kit's `Input`, `Button` and `TitleBar`; Home (`home.rs`),
//! the results page (`serp.rs`) and the places (`places.rs`).

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px, rems,
};
use url::Url;

use super::dock::FRAME;
use super::{Browser, TAB_ROW};
use crate::actions::{GoBack, GoForward, NewTab, ToggleAsk};
use crate::address::{Target, classify};
use crate::delegate::{self, Verdict};
use crate::home::{Home, HomeEvent};
use crate::icons;
use crate::places::{Place, PlaceView};
use crate::recents;
use crate::serp::{Serp, SerpMode};

pub(super) struct Underlay {
    pub(super) shown: bool,
    pub(super) route: Route,
    back: Vec<Route>,
    forward: Vec<Route>,
    pub(super) field: Entity<InputState>,
    _field_events: Subscription,
}

#[derive(Clone)]
pub(super) enum Route {
    Home,
    /// A results page, and the subscription that hands its picks to the
    /// browser; both move to the tab when a result is opened.
    Results(Entity<Serp>, Rc<Subscription>),
    Place(Place),
}

impl Route {
    fn same(&self, other: &Route) -> bool {
        match (self, other) {
            (Route::Home, Route::Home) => true,
            (Route::Results(a, _), Route::Results(b, _)) => a == b,
            (Route::Place(a), Route::Place(b)) => a == b,
            _ => false,
        }
    }
}

const SEARCH_PLACEHOLDER: &str = "Search or type an address...";
const ASK_PLACEHOLDER: &str = "Ask...";

impl Underlay {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Browser>) -> Self {
        let field = cx.new(|cx| InputState::new(window, cx).placeholder(SEARCH_PLACEHOLDER));
        let field_events = cx.subscribe_in(&field, window, Browser::underlay_field_event);
        Self {
            shown: true,
            route: Route::Home,
            back: Vec::new(),
            forward: Vec::new(),
            field,
            _field_events: field_events,
        }
    }

    pub(super) fn title(&self, cx: &gpui_kit::App) -> SharedString {
        match &self.route {
            Route::Home => "Home".into(),
            Route::Results(serp, _) => serp.read(cx).title(),
            Route::Place(place) => place.label().into(),
        }
    }

    pub(super) fn set_placeholder(&self, ask: bool, window: &mut Window, cx: &mut gpui_kit::App) {
        let placeholder = if ask { ASK_PLACEHOLDER } else { SEARCH_PLACEHOLDER };
        self.field
            .update(cx, |field, cx| field.set_placeholder(placeholder, window, cx));
    }
}

impl Browser {
    /// No tab is open: the workspace shows, at Home. The tail of whatever
    /// closed the tab syncs the chrome and moves the focus.
    pub(super) fn reset_underlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let underlay = &mut self.underlay;
        underlay.shown = true;
        underlay.route = Route::Home;
        underlay.back.clear();
        underlay.forward.clear();
        underlay
            .field
            .update(cx, |field, cx| field.set_value("", window, cx));
        cx.notify();
    }

    /// Back to the tabs, on the one the workspace stands for.
    pub(super) fn hide_underlay(&mut self, cx: &mut Context<Self>) {
        let underlay = &mut self.underlay;
        underlay.shown = false;
        underlay.route = Route::Home;
        underlay.back.clear();
        underlay.forward.clear();
        self.active = 0;
        cx.notify();
    }

    /// ⌘T: the tab behind the workspace, ready for an address.
    pub(super) fn reveal_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_underlay(cx);
        self.activate(0, window, cx);
        self.focus_address_bar(window, cx);
    }

    pub(super) fn underlay_go(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if self.underlay.route.same(&route) {
            self.focus_content(window, cx);
            return;
        }
        let left = std::mem::replace(&mut self.underlay.route, route);
        self.underlay.back.push(left);
        self.underlay.forward.clear();
        self.underlay_arrived(window, cx);
    }

    pub(super) fn underlay_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(route) = self.underlay.back.pop() else {
            return;
        };
        let left = std::mem::replace(&mut self.underlay.route, route);
        self.underlay.forward.push(left);
        self.underlay_arrived(window, cx);
    }

    pub(super) fn underlay_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(route) = self.underlay.forward.pop() else {
            return;
        };
        let left = std::mem::replace(&mut self.underlay.route, route);
        self.underlay.back.push(left);
        self.underlay_arrived(window, cx);
    }

    pub(super) fn underlay_reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Route::Results(serp, _) = &self.underlay.route {
            serp.update(cx, |serp, cx| serp.reload(window, cx));
        }
    }

    /// The field holds the route's query, the title follows the route, and
    /// the keyboard goes to it.
    fn underlay_arrived(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = match &self.underlay.route {
            Route::Results(serp, _) => serp.read(cx).query().clone(),
            Route::Home | Route::Place(_) => SharedString::default(),
        };
        self.underlay.field.update(cx, |field, cx| {
            if field.value() != text {
                field.set_value(text, window, cx);
            }
        });
        self.sync_chrome(window, cx);
        self.focus_content(window, cx);
    }

    pub(super) fn focus_underlay(&self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.underlay.route {
            Route::Home => self
                .underlay
                .field
                .update(cx, |field, cx| field.focus(window, cx)),
            Route::Results(serp, _) => window.focus(&serp.focus_handle(cx), cx),
            Route::Place(_) => window.focus(&self.focus_handle, cx),
        }
    }

    pub(super) fn focus_underlay_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.underlay.field.update(cx, |field, cx| {
            field.focus(window, cx);
            field.select_all(window, cx);
        });
        cx.notify();
    }

    pub(super) fn underlay_search(
        &mut self,
        query: String,
        mode: SerpMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        recents::record_search(&query, mode == SerpMode::Ask, cx);
        let (serp, events) = self.new_serp(query, mode, window, cx);
        self.underlay_go(Route::Results(serp, events), window, cx);
    }

    /// Open `url` in the tab the workspace stands for. A file is downloaded
    /// and the workspace stays.
    pub(super) fn underlay_open(&mut self, url: Url, window: &mut Window, cx: &mut Context<Self>) {
        if delegate::verdict(&url) == Verdict::Download {
            self.download(url, None, window, cx);
            return;
        }
        self.hide_underlay(cx);
        self.go(url, false, true, window, cx);
    }

    /// A result was opened from the workspace's results. They become the
    /// tab's, so Back from the page returns to them. True when `serp` is
    /// the workspace's.
    pub(super) fn underlay_opened(
        &mut self,
        serp: &Entity<Serp>,
        url: &Url,
        new_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Route::Results(shown, events) = &self.underlay.route else {
            return false;
        };
        if shown != serp {
            return false;
        }
        let (serp, events) = (shown.clone(), events.clone());
        self.hide_underlay(cx);
        let tab = &mut self.tabs[0];
        tab.serp = Some(serp);
        tab._serp_events = Some(events);
        tab.showing = super::Showing::Serp;
        self.go(url.clone(), new_tab, true, window, cx);
        true
    }

    fn underlay_field_event(
        &mut self,
        field: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let InputEvent::PressEnter { .. } = event else {
            return;
        };
        let text = field.read(cx).value().to_string();
        match classify(&text) {
            None => {}
            Some(Target::Go(url)) => self.underlay_open(url, window, cx),
            Some(Target::Search(query)) => {
                let mode = if self.ask {
                    SerpMode::Ask
                } else {
                    SerpMode::Search
                };
                self.underlay_search(query, mode, window, cx);
            }
        }
    }

    /// Escape in the field returns to what the route shows.
    fn underlay_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.underlay.route, Route::Home) {
            self.focus_content(window, cx);
        }
    }

    fn underlay_home_event(&mut self, event: &HomeEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            HomeEvent::Place(place) => self.underlay_go(Route::Place(*place), window, cx),
            HomeEvent::Page { url, .. } => self.underlay_open(url.clone(), window, cx),
            HomeEvent::Search { query, ask, .. } => {
                let mode = if *ask {
                    SerpMode::Ask
                } else {
                    SerpMode::Search
                };
                self.underlay_search(query.clone(), mode, window, cx);
            }
        }
    }

    /// Home's field: large, with Ask beside it, labelled so its state reads
    /// in words as well as fill.
    fn home_field(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let prefix = if self.ask {
            icons::ask()
        } else {
            Icon::new(IconName::Search)
        };
        let ask = Button::new("underlay-ask")
            .debug_selector(|| "underlay-ask".into())
            .large()
            .label("Ask")
            .accessibility_label("Ask AI")
            .toggled(self.ask)
            .tooltip_with_action(
                if self.ask { "Turn Off Ask" } else { "Ask AI" },
                &ToggleAsk,
                None,
            )
            .on_click(cx.listener(|browser, _, window, cx| browser.toggle_ask(window, cx)));
        let ask = if self.ask {
            let (on, text) = (cx.theme().foreground, cx.theme().background);
            ask.icon(icons::ask_on()).custom(
                ButtonCustomVariant::new(cx)
                    .color(on)
                    .foreground(text)
                    .hover(on.opacity(0.9))
                    .active(on.opacity(0.8)),
            )
        } else {
            ask.icon(icons::ask()).outline()
        };
        h_flex()
            .gap_2()
            .child(
                div()
                    .debug_selector(|| "underlay-field".into())
                    .flex_1()
                    .min_w_0()
                    .on_action(cx.listener(Self::underlay_escape))
                    .child(
                        Input::new(&self.underlay.field)
                            .id("underlay-field")
                            .prefix(prefix.small())
                            .large(),
                    ),
            )
            .child(ask)
    }

    /// The field in the header, on the routes that are not Home.
    fn header_field(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let prefix = if self.ask {
            icons::ask()
        } else {
            Icon::new(IconName::Search)
        };
        h_flex()
            .gap_1()
            .w(rems(30.))
            .min_w_0()
            .child(
                div()
                    .debug_selector(|| "underlay-field".into())
                    .flex_1()
                    .min_w_0()
                    .on_action(cx.listener(Self::underlay_escape))
                    .child(
                        Input::new(&self.underlay.field)
                            .id("underlay-field")
                            .prefix(prefix.small())
                            .small(),
                    ),
            )
            .child(self.ask_button(0, None, cx))
    }

    pub(super) fn render_underlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let id = self.tabs[0].id;
        let route = self.underlay.route.clone();
        let home = matches!(route, Route::Home);
        let document = matches!(route, Route::Results(..));
        let (can_back, can_forward) = (
            !self.underlay.back.is_empty(),
            !self.underlay.forward.is_empty(),
        );

        let header = h_flex()
            .id("underlay-header")
            .flex_shrink_0()
            .h(px(TAB_ROW))
            .items_center()
            .gap_1()
            .pr_1()
            .when(window.is_fullscreen(), |row| row.pl_1())
            .when(!window.is_fullscreen(), |row| row.child(Self::drag_strip()))
            .child(
                Button::new("underlay-back")
                    .ghost()
                    .small()
                    .icon(IconName::ArrowLeft)
                    .disabled(!can_back)
                    .tooltip_with_action("Back", &GoBack, None)
                    .on_click(cx.listener(|browser, _, window, cx| browser.underlay_back(window, cx))),
            )
            .child(
                Button::new("underlay-forward")
                    .ghost()
                    .small()
                    .icon(IconName::ArrowRight)
                    .disabled(!can_forward)
                    .tooltip_with_action("Forward", &GoForward, None)
                    .on_click(
                        cx.listener(|browser, _, window, cx| browser.underlay_forward(window, cx)),
                    ),
            )
            .child(
                Button::new("underlay-home")
                    .ghost()
                    .small()
                    .icon(gpui_kit::assets::IconName::House)
                    .tooltip("Home")
                    .accessibility_label("Home")
                    .on_click(cx.listener(|browser, _, window, cx| {
                        browser.underlay_go(Route::Home, window, cx)
                    })),
            )
            .when(!home, |row| row.child(self.header_field(cx)))
            .child(div().flex_1().h_full().child(Self::drag_strip().pl_0()))
            .children(self.downloads_button(id, None, cx))
            .child(self.appearance_button(id, None, cx))
            .child(
                Button::new("underlay-new-tab")
                    .ghost()
                    .small()
                    .icon(IconName::Plus)
                    .tooltip_with_action("New Tab", &NewTab, None)
                    .on_click(cx.listener(|browser, _, window, cx| browser.reveal_tab(window, cx))),
            );

        let body = match route {
            Route::Home => {
                let browser = cx.weak_entity();
                Home::new("underlay-home")
                    .field(self.home_field(cx))
                    .on_event(move |event, window, cx| {
                        let _ = browser.update(cx, |browser, cx| {
                            browser.underlay_home_event(event, window, cx)
                        });
                    })
                    .into_any_element()
            }
            Route::Results(serp, _) => serp.into_any_element(),
            Route::Place(place) => PlaceView::new(place).into_any_element(),
        };

        v_flex()
            .debug_selector(|| "underlay".into())
            .size_full()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .mx(px(FRAME))
                    .mb(px(FRAME))
                    .when(document, |card| {
                        card.rounded(theme.radius_lg)
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.background)
                    })
                    .overflow_hidden()
                    .child(body),
            )
            .into_any_element()
    }
}
