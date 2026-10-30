//! The place the window shows (`Browser::place`), over the tabs or the
//! workspace, which it leaves as they were. No tab row or sidebar stands
//! over a place: its card holds it, and the places bump (`PlacesBump`), at
//! the card's leading edge or, in the Agent place, the window's, moves
//! between the places and back out to what is beneath. With Always Show
//! Places on, the bump stands over the tabs and the workspace as well.
//!
//! SOURCING: the bump and the card's shape are `bump.rs`; the places,
//! `places.rs`; the Agent place is Theorem's workbench (`theorem/`).

use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::{
    AnyElement, AppContext as _, Context, IntoElement, ParentElement as _, Styled as _, Window,
    div, px,
};
use url::Url;

use super::{Browser, FRAME, Origin, TAB_ROW, underlay};
use crate::bump::{self, Slot};
use crate::places::{Exit, Place, PlaceView, PlacesBump};
use crate::theorem::{DriverEvent, Workbench};

impl Browser {
    /// Show a place over the tabs or the workspace.
    pub(super) fn open_place(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        if place == Place::Agent && self.theorem.is_none() {
            self.open_theorem(window, cx);
        }
        let exit = self.exit();
        if let Some((workbench, _)) = &self.theorem {
            workbench.update(cx, |workbench, cx| workbench.set_exit(exit, cx));
        }
        self.close_navigator(window, cx);
        self.place = Some(place);
        self.sync_chrome(window, cx);
        self.focus_place(place, window, cx);
    }

    /// Where the bump's way out goes: the workspace's Home while the
    /// workspace is beneath, else the tabs.
    fn exit(&self) -> Exit {
        if self.underlay.shown {
            Exit::Home
        } else {
            Exit::Tabs
        }
    }

    /// Leave the place for what is beneath it, without moving the focus.
    /// Whether a place was showing.
    pub(super) fn leave_place(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.place.take().is_none() {
            return false;
        }
        self.sync_chrome(window, cx);
        true
    }

    /// Leave the place, and focus what is beneath it (Back, ⌘W).
    pub(super) fn close_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.leave_place(window, cx) {
            self.focus_content(window, cx);
        }
    }

    /// The bump's way out: the tabs, or the workspace's Home.
    pub(super) fn exit_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let exit = self.exit();
        self.close_place(window, cx);
        if exit == Exit::Home {
            self.underlay_go(underlay::Route::Home, window, cx);
        }
    }

    pub(super) fn toggle_places(&mut self, cx: &mut Context<Self>) {
        self.places_pinned = !self.places_pinned;
        self.schedule_save(cx);
        cx.notify();
    }

    /// The Agent's composer, or the window, which holds the other places'
    /// keyboard.
    pub(super) fn focus_place(
        &mut self,
        place: Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (place, &self.theorem) {
            (Place::Agent, Some((workbench, _))) => {
                workbench.update(cx, |workbench, cx| workbench.focus_composer(window, cx))
            }
            _ => window.focus(&self.focus_handle, cx),
        }
    }

    /// Make Theorem's workbench, which connects to the configured authority.
    /// The pages it asks for, a sign-in page or a cited source, open in a
    /// tab.
    fn open_theorem(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let workbench = cx.new(|cx| Workbench::new(window, cx));
        let driver = workbench.read(cx).driver().clone();
        let events = cx.subscribe_in(
            &driver,
            window,
            |browser, _, event: &DriverEvent, window, cx| {
                let DriverEvent::OpenUrl(url) = event;
                browser.theorem_open(url.clone(), window, cx);
            },
        );
        self.theorem = Some((workbench, events));
    }

    /// A page Theorem asks for: in a new tab, or in the tab the workspace
    /// stands for. Either way the tabs come forward to show it.
    fn theorem_open(&mut self, url: Url, window: &mut Window, cx: &mut Context<Self>) {
        let new_tab = (!self.underlay.shown).then_some(Origin::Root);
        self.go(url, new_tab, true, window, cx);
    }

    /// The place: the Agent's workbench, which draws its own card, or a
    /// place's card under the window's drag strip.
    pub(super) fn render_place(&mut self, place: Place, cx: &mut Context<Self>) -> AnyElement {
        if place == Place::Agent
            && let Some((workbench, _)) = &self.theorem
        {
            return workbench.clone().into_any_element();
        }
        let slot = Slot::default();
        v_flex()
            .size_full()
            .child(Self::drag_strip().h(px(TAB_ROW)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .mx(px(FRAME))
                    .mb(px(FRAME))
                    .text_color(cx.theme().foreground)
                    .child(bump::card(Some(slot.clone()), None, cx))
                    .child(PlaceView::new(place))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left_0()
                            .flex()
                            .items_center()
                            .child(PlacesBump::new(Some(place)).exit(self.exit()).slot(slot)),
                    ),
            )
            .into_any_element()
    }

    /// Always Show Places, while no place shows: the bump at the leading
    /// edge of the tabs or the workspace. On the workspace's frame it fades
    /// in from the frame; over a page, which it cannot be cut into, it is
    /// solid.
    pub(super) fn render_pinned_places(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if !self.places_pinned || self.place.is_some() || self.compact {
            return None;
        }
        let fill = if self.underlay.shown && self.canvas.is_none() {
            bump::leading_fill(cx)
        } else {
            cx.theme().sidebar_border.into()
        };
        Some(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .flex()
                .items_center()
                .child(PlacesBump::new(None).fill(fill))
                .into_any_element(),
        )
    }
}
