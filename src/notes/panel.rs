//! The notes reading panel: a dock panel, like a browser tab's `TabPanel`
//! without the browser's chrome, that shows one note of a vault in a Servo web
//! view.
//!
//! Ownership, by the data-oriented rules:
//! - the vault's text and revision belong to the vault's worker thread;
//! - what the page shows, the callout folds and the notice for the person are
//!   the `Session`'s (`host::session`), the one owner of the conversation with
//!   the page;
//! - the web view's frames, input and focus are the `Page`'s, the same entity
//!   a browser tab uses;
//! - this panel owns only what joins them to the dock: the focus handle, the
//!   subscriptions, and what it draws when there is no page to show.
//!
//! The panel never decides where the web opens: a link to the web reaches its
//! owner as `ReadingEvent::OpenWeb`, since the owner knows whether there is a
//! browser to open it in.
//!
//! SOURCING: `Page` for the web view's drawing and input, `Session` for the
//! conversation with the page, GPUI Kit's `Panel` for the dock.

use std::rc::Rc;

use anyhow::anyhow;
use dpi::PhysicalSize;
use euclid::Scale;
use gpui_kit::base::dock::{Panel, PanelEvent, PanelState};
use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Corners, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Subscription, Window, div, px,
};
use servo::{RenderingContext as _, WebViewBuilder};
use url::Url;

use super::host::service::{Service, Vaults};
use super::host::session::{Session, SessionEvent, Status, Wiring};
use super::host::shell::Chrome;
use crate::delegate::{Delegate, NewWebView};
use crate::engine::Engine;
use crate::page::Page;
use crate::scene::SceneRenderingContext;

/// What the owner of a reading panel has to act on.
pub enum ReadingEvent {
    /// A link to the web was followed (`http`, `https` or `mailto`).
    OpenWeb(Url),
}

pub struct ReadingPanel {
    session: Entity<Session>,
    page: Entity<Page>,
    vaults: Vaults,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ReadingEvent> for ReadingPanel {}
impl EventEmitter<PanelEvent> for ReadingPanel {}

impl ReadingPanel {
    /// A panel on `vault`, showing `first`, a note's path in the vault. With
    /// none it shows nothing until `show` names one. `vaults` is the registry
    /// `vault` came from: the pages it serves take the app's theme from it.
    ///
    /// The first note is a construction argument because Servo drops a page
    /// asked for before its web view's first page is up (see
    /// `Wiring::first_page`); `show` is for every note after it.
    pub fn new(
        vaults: &Vaults,
        vault: Service,
        first: Option<&str>,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<Entity<Self>> {
        // Before the first page is served, so it carries the theme.
        vaults.set_chrome(Chrome::from_theme(cx.theme()));
        let (page, session) = Self::web_view(vault, first, window, cx)?;
        let vaults = vaults.clone();
        Ok(cx.new(|cx: &mut Context<Self>| {
            let focus_handle = cx.focus_handle();
            let subscriptions = vec![
                // A click that lands on nothing focusable focuses the panel;
                // the page takes it from there, as it does in a browser tab.
                cx.on_focus(&focus_handle, window, |panel, window, cx| {
                    window.focus(&panel.page.focus_handle(cx), cx);
                }),
                cx.subscribe(&session, |_, _, event: &SessionEvent, cx| match event {
                    SessionEvent::OpenWeb(url) => cx.emit(ReadingEvent::OpenWeb(url.clone())),
                }),
                // The status and the notice are the session's.
                cx.observe(&session, |_, _, cx| cx.notify()),
                cx.observe_global::<Theme>(|panel, cx| panel.retheme(cx)),
            ];
            let mut panel = Self {
                session,
                page,
                vaults,
                focus_handle,
                _subscriptions: subscriptions,
            };
            panel.round_page(cx);
            panel
        }))
    }

    /// The web view of the page and the session that talks to it. The page is
    /// a browser tab's `Page`; its web view is built with the session's
    /// delegate, which answers the page's messages and passes the rest on.
    fn web_view(
        vault: Service,
        first: Option<&str>,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<(Entity<Page>, Entity<Session>)> {
        let scale = window.scale_factor();
        let viewport = window.viewport_size();
        let size = PhysicalSize::new(
            (f32::from(viewport.width) * scale).round().max(1.) as u32,
            (f32::from(viewport.height) * scale).round().max(1.) as u32,
        );
        let context = Rc::new(
            SceneRenderingContext::for_window(size)
                .map_err(|error| anyhow!("rendering context: {error:?}"))?,
        );
        context
            .make_current()
            .map_err(|error| anyhow!("rendering context: {error:?}"))?;
        let delegate = Delegate::new();
        let mut wiring = Wiring::new(vault.host(), delegate.clone());
        let mut builder = WebViewBuilder::new(&Engine::servo(cx), context.clone())
            .hidpi_scale_factor(Scale::new(scale))
            .delegate(wiring.delegate());
        if let Some(first) = first {
            builder = builder.url(wiring.first_page(first)?);
        }
        let webview = builder.build();
        let page = Page::adopt(
            NewWebView {
                webview: webview.clone(),
                delegate,
                context,
            },
            window,
            cx,
        );
        page.update(cx, |page, _| page.set_visible(true));
        let session = Session::start(wiring, webview, vault, cx);
        Ok((page, session))
    }

    /// Opens `path`, or the vault's first note when none is named. Not for
    /// the panel's first note, which `new` takes.
    pub fn show(&self, path: Option<String>, fragment: Option<String>, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, cx| session.show(path, fragment, cx));
    }

    pub fn session(&self) -> &Entity<Session> {
        &self.session
    }

    /// The note's name, for a title.
    pub fn title(&self, cx: &App) -> SharedString {
        self.session.read(cx).title().into()
    }

    /// The app's theme changed: pages served from now on take it, and the
    /// page on screen is given it.
    fn retheme(&mut self, cx: &mut Context<Self>) {
        let chrome = Chrome::from_theme(cx.theme());
        self.vaults.set_chrome(chrome);
        self.session
            .update(cx, |session, _| session.restyle(chrome));
        self.round_page(cx);
        cx.notify();
    }

    /// The page's picture takes the corners of the card it sits in.
    fn round_page(&mut self, cx: &mut Context<Self>) {
        let radius = cx.theme().radius_lg;
        self.page
            .update(cx, |page, _| page.corners = Corners::all(radius));
    }
}

/// What the panel says in place of a page, if there is no page to show.
fn message(status: &Status) -> Option<String> {
    match status {
        Status::Showing => None,
        Status::Opening => Some("Opening the note".to_owned()),
        Status::Empty => Some("This vault has no notes yet.".to_owned()),
        Status::Unavailable(reason) => Some(format!("This vault could not be read: {reason}")),
    }
}

impl Panel for ReadingPanel {
    fn panel_name(&self) -> &'static str {
        "NotesReading"
    }

    fn zoomable(&self, _: &App) -> bool {
        false
    }

    fn dump(&self, _: &App) -> PanelState {
        PanelState::new(self.panel_name())
    }
}

impl Focusable for ReadingPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ReadingPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let session = self.session.read(cx);
        let theme = cx.theme();
        let body: AnyElement = match message(session.status()) {
            None => self.page.clone().into_any_element(),
            Some(text) => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p_6()
                .text_color(theme.muted_foreground)
                .child(text)
                .into_any_element(),
        };
        let notice = session.notice().map(|notice| {
            div()
                .id("notes-notice")
                .px_4()
                .py_2()
                .border_t_1()
                .border_color(theme.border)
                .bg(theme.sidebar)
                .text_sm()
                .text_color(theme.foreground)
                .child(notice.to_owned())
        });
        div()
            .id("notes-reading")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(div().flex_1().min_h_0().relative().child(body))
            .children(notice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_page_that_is_showing_is_drawn() {
        assert_eq!(message(&Status::Showing), None);
        for status in [
            Status::Opening,
            Status::Empty,
            Status::Unavailable("busy".into()),
        ] {
            assert!(message(&status).is_some(), "{status:?}");
        }
    }

    #[test]
    fn an_unreadable_vault_says_why() {
        let text = message(&Status::Unavailable("the vault is busy".into())).unwrap();
        assert!(text.contains("the vault is busy"));
    }
}
