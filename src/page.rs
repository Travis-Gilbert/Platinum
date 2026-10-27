//! One tab's web page: a Servo web view drawn inside GPUI's scene, with
//! GPUI's input routed into it.
//!
//! SOURCING: hand-rolled embedder layer. No crate joins Servo and GPUI; the
//! event translation follows servoshell (`headed_window.rs`), and the frames
//! are GPUI's `surface()` element over `scene::SceneRenderingContext`.
//! Adapted from Theorem UI's desktop host (`desktop/src/page.rs`): the native
//! child view is gone, since the chrome must draw over pages, and the page
//! is one of many tabs.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::anyhow;
use dpi::PhysicalSize;
use euclid::Scale;
use gpui_kit::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
use gpui_kit::component::{ActiveTheme as _, Theme as ChromeTheme};
use gpui_kit::{
    App, AppContext as _, Bounds, Context, Corners, CursorStyle, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, Hsla, InteractiveElement as _,
    IntoElement, KeyUpEvent, Keystroke, ModifiersChangedEvent, MouseButton as GpuiButton,
    MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection, ObjectFit,
    ParentElement as _, PinchEvent, Pixels, Point, Render, RenderImage, Rgba, ScrollDelta,
    ScrollWheelEvent, SharedString, Styled as _, Subscription, UTF16Selection, Window, canvas, div,
    point, px, size, surface,
};
use servo::{
    ClipboardAction, CompositionEvent, CompositionState, ContextMenuAction, Cursor, DevicePoint,
    Image, ImeEvent, InputEvent, InputEventId, InputEventResult, InputMethodControl, JSValue,
    KeyState, LoadStatus, MouseButton as ServoButton, MouseButtonAction, MouseButtonEvent,
    MouseLeftViewportEvent, MouseMoveEvent as ServoMouseMove, PixelFormat, RenderingContext as _,
    Theme, WebView, WebViewBuilder, WheelDelta, WheelEvent, WheelMode,
};
use url::Url;

use crate::controls::Controls;
use crate::delegate::{Delegate, NewWebView, ServoEvent};
use crate::engine::Engine;
use crate::keys;
use crate::scene::SceneRenderingContext;
use crate::search::{self, VisitedPage};
use crate::site::{self, SiteInfo};

/// The key context the Edit menu's shortcuts bind in.
pub const CONTEXT: &str = "Page";

/// The current document's address, type and rendered markup, read together so
/// they always describe the same document: for the local search index, and
/// to notice Servo's placeholder for a file it cannot show.
// A login form is never serialized into local search/model context. This also
// excludes a site's own reflected password attributes after a fill.
const READ_DOCUMENT: &str = include_str!("scripts/read_document.js");

/// The site panel's reading of the current document: its metadata and the
/// fonts in use, as a JSON string.
const SITE_PANEL: &str = include_str!("scripts/site-panel.js");

/// servoshell's line height for wheels that scroll by lines.
const LINE_HEIGHT: f64 = 38.0;

/// Browser shortcuts the page never sees, so no site can take them.
const RESERVED: &[&str] = &[
    "cmd-k",
    "cmd-d",
    "cmd-y",
    "cmd-shift-p",
    "cmd-t",
    "cmd-w",
    "cmd-l",
    "cmd-n",
    "cmd-q",
    "cmd-r",
    "cmd-[",
    "cmd-]",
    "cmd-shift-[",
    "cmd-shift-]",
    "ctrl-tab",
    "ctrl-shift-tab",
    "ctrl-cmd-c",
    "cmd-1",
    "cmd-2",
    "cmd-3",
    "cmd-4",
    "cmd-5",
    "cmd-6",
    "cmd-7",
    "cmd-8",
    "cmd-9",
];

/// What the browser has to act on for this page.
pub enum PageEvent {
    /// The title, address, load state or history changed.
    Changed,
    Visited,
    PasswordFilled(bool),
    FormFilled(bool),
    PasswordRead(frame_profile::credentials::Login),
    PasswordUnavailable,
    /// The page opened new windows; `Page::take_opened` hands them over.
    Opened,
    /// The page's own context menu, which Frame shows as its search menu.
    ContextMenu(PageMenu),
    /// The page closed itself.
    Closed,
    /// A file to save rather than show: a link to one, or a page that
    /// turned out to be one.
    Download(Url),
}

/// The page's context menu as data: Servo's entries and where it was asked
/// for. The responder stays with the page until `select_context_action` or
/// `dismiss_context_menu` answers it.
#[derive(Clone)]
pub struct PageMenu {
    pub position: Point<Pixels>,
    pub items: Vec<PageMenuItem>,
    /// The link under the pointer, if any.
    pub link: Option<Url>,
    /// The image under the pointer, if any.
    pub image: Option<Url>,
}

#[derive(Clone)]
pub struct PageMenuItem {
    pub label: SharedString,
    pub action: ContextMenuAction,
    pub enabled: bool,
}

pub struct Page {
    focus_handle: FocusHandle,
    pub(crate) webview: WebView,
    delegate: Rc<Delegate>,
    context: Rc<SceneRenderingContext>,
    /// The page element's last layout, in window points, and the scale it
    /// was placed at.
    pub(crate) bounds: Option<Bounds<Pixels>>,
    pub(crate) scale: f32,
    cursor: CursorStyle,
    /// Keys down now, so another key-down for one of them is a repeat.
    held: HashSet<String>,
    modifiers: gpui_kit::Modifiers,
    /// Shortcuts sent to the page, kept until it says whether it used them;
    /// the ones it did not use go back through GPUI's key bindings.
    shortcuts: HashMap<InputEventId, Keystroke>,
    replaying: Rc<Cell<bool>>,
    pressed: Vec<GpuiButton>,
    pub(crate) last_pointer: Option<Point<Pixels>>,
    /// Text the input method is composing, as last marked.
    marked: Option<String>,
    pub(crate) input_method: Option<InputMethodControl>,
    pub(crate) controls: Controls,
    opened: Vec<NewWebView>,
    /// The page's icon and the host it came with. Servo keeps an icon until
    /// another arrives, so a site without one would show the last site's.
    favicon: Option<(String, Arc<RenderImage>)>,
    /// The color along the top of the page, which the toolbar takes.
    tint: Option<Hsla>,
    /// What the site panel shows, as of the last `read_site`.
    site: Option<SiteInfo>,
    /// The corners of the card the page sits in, which round its picture
    /// (`Browser::content`).
    pub(crate) corners: Corners<Pixels>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PageEvent> for Page {}

impl Page {
    /// A new web view loading `url`, sized to the window until it is laid out.
    pub fn open(url: Url, window: &mut Window, cx: &mut App) -> anyhow::Result<Entity<Self>> {
        let scale = window.scale_factor();
        let viewport = window.viewport_size();
        let size = PhysicalSize::new(
            (f32::from(viewport.width) * scale).round().max(1.) as u32,
            (f32::from(viewport.height) * scale).round().max(1.) as u32,
        );
        let context = Rc::new(
            SceneRenderingContext::new(size)
                .map_err(|error| anyhow!("rendering context: {error:?}"))?,
        );
        context
            .make_current()
            .map_err(|error| anyhow!("rendering context: {error:?}"))?;
        let delegate = Delegate::new();
        let webview = WebViewBuilder::new(&Engine::servo(cx), context.clone())
            .url(url)
            .hidpi_scale_factor(Scale::new(scale))
            .delegate(delegate.clone())
            .build();
        Ok(Self::adopt(
            NewWebView {
                webview,
                delegate,
                context,
            },
            window,
            cx,
        ))
    }

    /// A web view Servo already built, for a window the page opened.
    pub fn adopt(new: NewWebView, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let NewWebView {
            webview,
            delegate,
            context,
        } = new;
        webview.notify_theme_change(servo_theme(cx));
        cx.new(|cx: &mut Context<Self>| {
            let focus_handle = cx.focus_handle();
            let replaying = Rc::new(Cell::new(false));
            let subscriptions = vec![
                cx.on_focus(&focus_handle, window, |page, _, _| {
                    page.webview.set_focused(true)
                }),
                cx.on_blur(&focus_handle, window, |page, _, _| {
                    page.release_buttons();
                    page.webview.set_focused(false);
                }),
                cx.observe_global::<ChromeTheme>(|page, cx| {
                    page.webview.notify_theme_change(servo_theme(cx));
                }),
                Self::intercept_keys(replaying.clone(), window, cx),
            ];
            Self::drain_after_spin(window, cx);
            Self {
                focus_handle,
                scale: window.scale_factor(),
                webview,
                delegate,
                context,
                bounds: None,
                cursor: CursorStyle::Arrow,
                held: HashSet::new(),
                modifiers: gpui_kit::Modifiers::default(),
                shortcuts: HashMap::new(),
                replaying,
                pressed: Vec::new(),
                last_pointer: None,
                marked: None,
                input_method: None,
                controls: Controls::default(),
                opened: Vec::new(),
                favicon: None,
                tint: None,
                site: None,
                corners: Corners::default(),
                _subscriptions: subscriptions,
            }
        })
    }

    // What the chrome shows.

    pub fn title(&self) -> SharedString {
        if let Some(title) = self.webview.page_title().filter(|t| !t.trim().is_empty()) {
            return title.into();
        }
        match self.webview.url() {
            Some(url) => url.host_str().unwrap_or(url.as_str()).to_owned().into(),
            None => "Loading".into(),
        }
    }

    pub fn url(&self) -> Option<Url> {
        self.webview.url()
    }

    /// The site's icon, while the page is still on the site it came from.
    pub fn favicon(&self) -> Option<Arc<RenderImage>> {
        let (host, icon) = self.favicon.as_ref()?;
        let url = self.webview.url()?;
        (url.host_str() == Some(host.as_str())).then(|| icon.clone())
    }

    /// The color along the top of the page.
    pub fn tint(&self) -> Option<Hsla> {
        self.tint
    }

    /// What the site panel shows, while the page is still on the document
    /// `read_site` read it from.
    pub fn site(&self) -> Option<&SiteInfo> {
        let info = self.site.as_ref()?;
        let current = self.webview.url()?;
        let mut read_from = info.url.clone()?;
        read_from.set_fragment(current.fragment());
        (read_from == current).then_some(info)
    }

    pub fn is_loading(&self) -> bool {
        self.webview.load_status() != LoadStatus::Complete
    }

    /// The link under the pointer.
    pub fn status(&self) -> Option<String> {
        self.delegate.status()
    }

    pub fn can_go_back(&self) -> bool {
        self.webview.can_go_back()
    }

    pub fn can_go_forward(&self) -> bool {
        self.webview.can_go_forward()
    }

    // What the chrome asks of it.

    pub fn load(&mut self, url: Url) {
        self.webview.load(url);
    }

    pub fn back(&mut self) {
        if self.webview.can_go_back() {
            self.webview.go_back(1);
        }
    }

    pub fn forward(&mut self) {
        if self.webview.can_go_forward() {
            self.webview.go_forward(1);
        }
    }

    pub fn reload(&mut self) {
        self.webview.reload();
    }

    /// Background tabs stop painting and running timers at full rate.
    pub fn set_visible(&mut self, visible: bool) {
        if visible {
            self.webview.show();
        } else {
            self.release_buttons();
            self.webview.hide();
        }
    }

    pub fn take_opened(&mut self) -> Vec<NewWebView> {
        std::mem::take(&mut self.opened)
    }

    /// Send an input event to the page.
    pub(crate) fn send(&self, event: InputEvent) -> InputEventId {
        self.webview.notify_input_event(event)
    }

    /// Act on what Servo queued during its last turn of the event loop.
    fn drain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for event in self.delegate.take_events() {
            match event {
                ServoEvent::Frame => {
                    self.read_tint(cx);
                    cx.notify();
                }
                ServoEvent::Favicon => self.read_favicon(cx),
                ServoEvent::Cursor(cursor) => {
                    self.cursor = cursor_style(cursor);
                    cx.notify();
                }
                ServoEvent::InputHandled(id, result) => self.input_handled(id, result, window, cx),
                ServoEvent::ShowControl(control) => self.show_control(control, window, cx),
                ServoEvent::HideControl(id) => self.hide_control(id, cx),
                ServoEvent::Changed => {
                    cx.emit(PageEvent::Changed);
                    cx.notify();
                }
                ServoEvent::Loaded => {
                    self.read_document();
                    cx.emit(PageEvent::Visited);
                }
                ServoEvent::FormFilled(filled) => cx.emit(PageEvent::FormFilled(filled)),
                ServoEvent::PasswordFilled(filled) => cx.emit(PageEvent::PasswordFilled(filled)),
                ServoEvent::PasswordRead(login) => match login {
                    Some(login) => cx.emit(PageEvent::PasswordRead(login)),
                    None => cx.emit(PageEvent::PasswordUnavailable),
                },
                ServoEvent::Document(page) => search::capture(page, cx),
                ServoEvent::Site(info) => {
                    self.site = Some(info);
                    cx.emit(PageEvent::Changed);
                    cx.notify();
                }
                ServoEvent::Crashed(reason) => self.crashed(&reason, window, cx),
                ServoEvent::OpenExternally(url) => cx.open_url(url.as_str()),
                ServoEvent::NewWebView(new) => {
                    self.opened.push(new);
                    cx.emit(PageEvent::Opened);
                }
                ServoEvent::Closed => cx.emit(PageEvent::Closed),
                ServoEvent::Download(url) => cx.emit(PageEvent::Download(url)),
            }
        }
    }

    pub fn fill_form_info(&self, script: &str) {
        let delegate = self.delegate.clone();
        self.webview.evaluate_javascript(script, move |r| {
            delegate.push(ServoEvent::FormFilled(matches!(
                r,
                Ok(JSValue::Boolean(true))
            )))
        });
    }
    pub fn fill_password(&self, script: &str) {
        let delegate = self.delegate.clone();
        self.webview.evaluate_javascript(script, move |result| {
            delegate.push(ServoEvent::PasswordFilled(matches!(
                result,
                Ok(JSValue::Boolean(true))
            )));
        });
    }

    pub fn read_password(&self) -> anyhow::Result<()> {
        let url = self.url().ok_or_else(|| anyhow!("No login page"))?;
        let script = frame_profile::credentials::read_script(url.as_str())?;
        let expected = url.origin().ascii_serialization();
        let delegate = self.delegate.clone();
        self.webview.evaluate_javascript(script, move |result| {
            let login = (|| {
                let JSValue::Array(values) = result.ok()? else {
                    return None;
                };
                let [
                    JSValue::String(origin),
                    JSValue::String(username),
                    JSValue::String(password),
                ] = <[JSValue; 3]>::try_from(values).ok()?
                else {
                    return None;
                };
                if origin != expected || password.is_empty() {
                    return None;
                }
                Some(frame_profile::credentials::Login {
                    origin,
                    username,
                    password: zeroize::Zeroizing::new(password),
                })
            })();
            delegate.push(ServoEvent::PasswordRead(login));
        });
        Ok(())
    }

    /// Read the finished document back: for the local search index, or, when
    /// it is Servo's placeholder for a file it cannot show, to save the file.
    /// The answer arrives on a later turn of Servo's loop and is queued like
    /// its events.
    fn read_document(&self) {
        if !self
            .webview
            .url()
            .is_some_and(|url| search::capturable(&url))
        {
            return;
        }
        let delegate = self.delegate.clone();
        self.webview
            .evaluate_javascript(READ_DOCUMENT, move |result| match result {
                Ok(value) => match read_back(value) {
                    Some(ReadBack::Page(page)) => delegate.document(page),
                    Some(ReadBack::Unshown(url)) => delegate.download(url),
                    None => {}
                },
                Err(error) => log::debug!("could not read the page back: {error:?}"),
            });
    }

    /// Read what the site panel shows: the page's metadata and fonts, from
    /// the document, and its palette, from the newest frame. Only when the
    /// panel opens, never on a load, and none of it goes to the search index
    /// or the model. The answer arrives like Servo's events and is kept for
    /// `site`.
    pub fn read_site(&self) {
        let palette = self.context.with_pixels(site::palette).unwrap_or_default();
        let delegate = self.delegate.clone();
        self.webview
            .evaluate_javascript(SITE_PANEL, move |result| match result {
                Ok(JSValue::String(answer)) => match SiteInfo::from_answer(&answer, palette) {
                    Some(info) => delegate.site(info),
                    None => log::debug!("the site panel's answer was not an object"),
                },
                other => log::debug!("could not read the site: {other:?}"),
            });
    }

    /// Take the color along the top of the page from each frame, as the page
    /// loads, restyles or scrolls under a header that is not fixed.
    fn read_tint(&mut self, cx: &mut Context<Self>) {
        let tint = self.context.top_color().map(|[r, g, b]| {
            Hsla::from(Rgba {
                r: f32::from(r) / 255.,
                g: f32::from(g) / 255.,
                b: f32::from(b) / 255.,
                a: 1.,
            })
        });
        if tint.is_some() && tint != self.tint {
            self.tint = tint;
            cx.emit(PageEvent::Changed);
        }
    }

    /// Keep the icon Servo found, with the host it belongs to.
    fn read_favicon(&mut self, cx: &mut Context<Self>) {
        let host = self
            .webview
            .url()
            .and_then(|url| url.host_str().map(str::to_owned));
        let icon = self.webview.favicon().and_then(|icon| favicon_image(&icon));
        self.favicon = host.zip(icon);
        cx.emit(PageEvent::Changed);
    }

    /// Servo calls the delegate from inside `spin_event_loop`; the queued
    /// events are handled here, after it returns, with the window in hand.
    fn drain_after_spin(window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.entity().downgrade();
        let window_handle = Window::window_handle(window);
        Engine::after_spin(
            move |cx| {
                let Some(page) = page.upgrade() else {
                    return false;
                };
                let _ = window_handle.update(cx, |_, window, cx| {
                    page.update(cx, |page, cx| page.drain(window, cx));
                });
                true
            },
            cx,
        );
    }

    fn crashed(&mut self, reason: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.controls.begin_dialog() {
            return;
        }
        let answer = window.prompt(
            gpui_kit::PromptLevel::Critical,
            "This page stopped working.",
            Some(reason),
            &["Reload"],
            cx,
        );
        cx.spawn(async move |page, cx| {
            let _ = answer.await;
            let _ = page.update(cx, |page, _| {
                page.controls.end_dialog();
                page.webview.reload();
            });
        })
        .detach();
    }

    /// The page's context menu, for the browser to show as its search menu.
    pub(crate) fn context_menu_requested(&mut self, menu: PageMenu, cx: &mut Context<Self>) {
        cx.emit(PageEvent::ContextMenu(menu));
    }

    // Keyboard.

    /// Keys reach the page before GPUI's key bindings, so the page can use
    /// any shortcut but the browser's own; `input_handled` hands back the
    /// ones it did not use.
    fn intercept_keys(
        replaying: Rc<Cell<bool>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let page = cx.entity().downgrade();
        let window_handle = Window::window_handle(window);
        cx.intercept_keystrokes(move |event, window, cx| {
            if replaying.get() || Window::window_handle(window) != window_handle {
                return;
            }
            if RESERVED.contains(&event.keystroke.unparse().as_str()) {
                return;
            }
            let Some(page) = page.upgrade() else {
                return;
            };
            if page.update(cx, |page, _| page.key_down(&event.keystroke, window)) {
                cx.stop_propagation();
            }
        })
    }

    fn key_down(&mut self, keystroke: &Keystroke, window: &Window) -> bool {
        if !self.focus_handle.is_focused(window) {
            return false;
        }
        self.controls.dismiss_menus();
        let shortcut = keystroke.modifiers.platform || keystroke.modifiers.control;
        // macOS sends no key-up for a key pressed with Command, so those
        // never count as held.
        let repeat = !shortcut && !self.held.insert(keystroke.key.clone());
        let event = keys::keyboard_event(keystroke, KeyState::Down, repeat);
        let id = self.send(InputEvent::Keyboard(event));
        if shortcut {
            self.shortcuts.insert(id, keystroke.clone());
        }
        true
    }

    fn key_up(&mut self, event: &KeyUpEvent, window: &mut Window, _: &mut Context<Self>) {
        if !self.focus_handle.is_focused(window) {
            return;
        }
        self.held.remove(&event.keystroke.key);
        let event = keys::keyboard_event(&event.keystroke, KeyState::Up, false);
        self.send(InputEvent::Keyboard(event));
    }

    fn modifiers_changed(&mut self, event: &ModifiersChangedEvent, window: &mut Window) {
        if !self.focus_handle.is_focused(window) {
            return;
        }
        for event in keys::modifier_events(&self.modifiers, &event.modifiers) {
            self.send(InputEvent::Keyboard(event));
        }
        self.modifiers = event.modifiers;
    }

    fn input_handled(
        &mut self,
        id: InputEventId,
        result: InputEventResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(keystroke) = self.shortcuts.remove(&id) else {
            return;
        };
        if result.intersects(InputEventResult::Consumed | InputEventResult::DefaultPrevented) {
            return;
        }
        let replaying = self.replaying.clone();
        window.defer(cx, move |window, cx| {
            replaying.set(true);
            window.dispatch_keystroke(keystroke, cx);
            replaying.set(false);
        });
    }

    /// The Edit menu. Servo has editing actions for the clipboard; the rest
    /// reach the page as the shortcut they stand for.
    fn editing(&mut self, action: ClipboardAction) {
        // Servo runs the clipboard shortcuts on keydown itself, so a replayed
        // Cmd-C, Cmd-X or Cmd-V has already happened. A page that handles
        // the paste event leaves the keydown unconsumed, and repeating it here
        // would paste twice.
        if self.replaying.get() {
            return;
        }
        self.send(InputEvent::EditingAction(action));
    }

    fn shortcut(&mut self, key: &str, shift: bool) {
        // A replayed shortcut already went to the page and was not used.
        if self.replaying.get() {
            return;
        }
        let keystroke = Keystroke {
            modifiers: gpui_kit::Modifiers {
                platform: true,
                shift,
                ..Default::default()
            },
            key: key.into(),
            key_char: None,
        };
        for state in [KeyState::Down, KeyState::Up] {
            self.send(InputEvent::Keyboard(keys::keyboard_event(
                &keystroke, state, false,
            )));
        }
    }

    // Pointer.

    fn device_point(&self, position: Point<Pixels>) -> Option<DevicePoint> {
        let origin = self.bounds?.origin;
        Some(DevicePoint::new(
            f32::from(position.x - origin.x) * self.scale,
            f32::from(position.y - origin.y) * self.scale,
        ))
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(point) = self.device_point(event.position) else {
            return;
        };
        self.controls.dismiss_menus();
        window.focus(&self.focus_handle, cx);
        if !self.pressed.contains(&event.button) {
            self.pressed.push(event.button);
        }
        self.last_pointer = Some(event.position);
        self.send(InputEvent::MouseButton(MouseButtonEvent::new(
            MouseButtonAction::Down,
            servo_button(event.button),
            point.into(),
        )));
        cx.stop_propagation();
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.pressed.iter().position(|b| *b == event.button) else {
            return;
        };
        self.pressed.remove(index);
        if let Some(point) = self.device_point(event.position) {
            self.send(InputEvent::MouseButton(MouseButtonEvent::new(
                MouseButtonAction::Up,
                servo_button(event.button),
                point.into(),
            )));
        }
        cx.stop_propagation();
    }

    /// Buttons still down when the page loses focus would stay down in the
    /// page; release them where the pointer was last seen.
    fn release_buttons(&mut self) {
        let Some(point) = self.last_pointer.and_then(|p| self.device_point(p)) else {
            self.pressed.clear();
            return;
        };
        for button in std::mem::take(&mut self.pressed) {
            self.send(InputEvent::MouseButton(MouseButtonEvent::new(
                MouseButtonAction::Up,
                servo_button(button),
                point.into(),
            )));
        }
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(point) = self.device_point(event.position) {
            self.last_pointer = Some(event.position);
            self.send(InputEvent::MouseMove(ServoMouseMove::new(point.into())));
            cx.stop_propagation();
        }
    }

    fn mouse_exit(&mut self, _: &MouseExitEvent, _: &mut Window, _: &mut Context<Self>) {
        self.send(InputEvent::MouseLeftViewport(
            MouseLeftViewportEvent::default(),
        ));
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(point) = self.device_point(event.position) else {
            return;
        };
        let (x, y) = match event.delta {
            ScrollDelta::Pixels(delta) => (
                f64::from(f32::from(delta.x) * self.scale),
                f64::from(f32::from(delta.y) * self.scale),
            ),
            ScrollDelta::Lines(delta) => (
                f64::from(delta.x) * LINE_HEIGHT,
                f64::from(delta.y) * LINE_HEIGHT,
            ),
        };
        let delta = WheelDelta {
            x,
            y,
            z: 0.0,
            mode: WheelMode::DeltaPixel,
        };
        self.send(InputEvent::Wheel(WheelEvent::new(delta, point.into())));
        cx.stop_propagation();
    }

    fn pinch(&mut self, event: &PinchEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(center) = self.device_point(event.position) {
            self.webview.adjust_pinch_zoom(1.0 + event.delta, center);
            cx.stop_propagation();
        }
    }

    /// Size the web view to where the page was laid out.
    fn place(&mut self, bounds: Bounds<Pixels>, scale: f32) {
        if self.bounds == Some(bounds) && self.scale == scale {
            return;
        }
        if self.scale != scale {
            self.webview.set_hidpi_scale_factor(Scale::new(scale));
        }
        self.webview.resize(PhysicalSize::new(
            (f32::from(bounds.size.width) * scale).round().max(1.0) as u32,
            (f32::from(bounds.size.height) * scale).round().max(1.0) as u32,
        ));
        self.bounds = Some(bounds);
        self.scale = scale;
    }

    // Input method.

    fn compose(&mut self, state: CompositionState, data: &str) {
        self.send(InputEvent::Ime(ImeEvent::Composition(CompositionEvent {
            state,
            data: data.to_owned(),
        })));
    }

    fn commit(&mut self, text: &str) {
        if self.marked.take().is_none() {
            self.compose(CompositionState::Start, "");
        }
        self.compose(CompositionState::End, text);
    }
}

/// Pages follow the chrome's light or dark mode, not the system's.
/// What a `READ_DOCUMENT` evaluation found.
enum ReadBack {
    /// An HTML page, for the search index.
    Page(VisitedPage),
    /// Servo's placeholder for a type it cannot show: the address is a file.
    Unshown(Url),
}

fn read_back(value: JSValue) -> Option<ReadBack> {
    let JSValue::Array(values) = value else {
        return None;
    };
    let [
        JSValue::String(url),
        JSValue::String(content_type),
        JSValue::String(html),
    ] = <[JSValue; 3]>::try_from(values).ok()?
    else {
        return None;
    };
    let url = Url::parse(&url).ok()?;
    if is_unknown_content(&html) {
        return Some(ReadBack::Unshown(url));
    }
    if !matches!(content_type.as_str(), "text/html" | "application/xhtml+xml") || html.is_empty() {
        return None;
    }
    Some(ReadBack::Page(VisitedPage { url, html }))
}

/// Servo's whole document for a type it cannot render
/// (`servoparser::load_document`): "Unknown content type (application/zip)."
fn is_unknown_content(html: &str) -> bool {
    let Some(body) = html
        .split_once("<body>")
        .and_then(|(_, rest)| rest.split_once("</body>"))
        .map(|(body, _)| body.trim())
    else {
        return false;
    };
    body.strip_prefix("<p>Unknown content type (")
        .and_then(|rest| rest.strip_suffix(").</p>"))
        .is_some_and(|mime| !mime.contains('<'))
}

fn servo_theme(cx: &App) -> Theme {
    if cx.theme().mode.is_dark() {
        Theme::Dark
    } else {
        Theme::Light
    }
}

fn servo_button(button: GpuiButton) -> ServoButton {
    match button {
        GpuiButton::Left => ServoButton::Primary,
        GpuiButton::Middle => ServoButton::Auxiliary,
        GpuiButton::Right => ServoButton::Secondary,
        GpuiButton::Navigate(NavigationDirection::Back) => ServoButton::Back,
        GpuiButton::Navigate(NavigationDirection::Forward) => ServoButton::Forward,
    }
}

/// The page's CSS cursor as the closest macOS cursor.
fn cursor_style(cursor: Cursor) -> CursorStyle {
    match cursor {
        Cursor::Pointer => CursorStyle::PointingHand,
        Cursor::Text => CursorStyle::IBeam,
        Cursor::VerticalText => CursorStyle::IBeamCursorForVerticalLayout,
        Cursor::Crosshair | Cursor::Cell => CursorStyle::Crosshair,
        Cursor::ContextMenu => CursorStyle::ContextualMenu,
        Cursor::Alias => CursorStyle::DragLink,
        Cursor::Copy => CursorStyle::DragCopy,
        Cursor::NoDrop | Cursor::NotAllowed => CursorStyle::OperationNotAllowed,
        Cursor::Grab | Cursor::Move | Cursor::AllScroll => CursorStyle::OpenHand,
        Cursor::Grabbing => CursorStyle::ClosedHand,
        Cursor::EResize => CursorStyle::ResizeRight,
        Cursor::WResize => CursorStyle::ResizeLeft,
        Cursor::EwResize | Cursor::ColResize => CursorStyle::ResizeColumn,
        Cursor::NResize => CursorStyle::ResizeUp,
        Cursor::SResize => CursorStyle::ResizeDown,
        Cursor::NsResize | Cursor::RowResize => CursorStyle::ResizeRow,
        Cursor::NeResize | Cursor::SwResize | Cursor::NeswResize => {
            CursorStyle::ResizeUpRightDownLeft
        }
        Cursor::NwResize | Cursor::SeResize | Cursor::NwseResize => {
            CursorStyle::ResizeUpLeftDownRight
        }
        Cursor::None
        | Cursor::Default
        | Cursor::Help
        | Cursor::Progress
        | Cursor::Wait
        | Cursor::ZoomIn
        | Cursor::ZoomOut => CursorStyle::Arrow,
    }
}

impl Focusable for Page {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = cx.entity();
        let placed = page.downgrade();
        let focus_handle = self.focus_handle.clone();
        let frame = self.context.frame();
        div()
            .id("page")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .cursor(self.cursor)
            .on_any_mouse_down(cx.listener(Self::mouse_down))
            .capture_any_mouse_up(cx.listener(Self::mouse_up))
            .on_mouse_up_out(GpuiButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(GpuiButton::Right, cx.listener(Self::mouse_up))
            .on_mouse_up_out(GpuiButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_exit(cx.listener(Self::mouse_exit))
            .on_scroll_wheel(cx.listener(Self::scroll))
            .on_pinch(cx.listener(Self::pinch))
            .on_key_up(cx.listener(Self::key_up))
            .on_modifiers_changed(
                cx.listener(|page, event, window, _| page.modifiers_changed(event, window)),
            )
            .on_action(cx.listener(|page, _: &Copy, _, _| page.editing(ClipboardAction::Copy)))
            .on_action(cx.listener(|page, _: &Cut, _, _| page.editing(ClipboardAction::Cut)))
            .on_action(cx.listener(|page, _: &Paste, _, _| page.editing(ClipboardAction::Paste)))
            .on_action(cx.listener(|page, _: &SelectAll, _, _| page.shortcut("a", false)))
            .on_action(cx.listener(|page, _: &Undo, _, _| page.shortcut("z", false)))
            .on_action(cx.listener(|page, _: &Redo, _, _| page.shortcut("z", true)))
            .on_action(cx.listener(Self::pick_option))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let scale = window.scale_factor();
                        let _ = placed.update(cx, |page, _| page.place(bounds, scale));
                    },
                    move |bounds, (), window, cx| {
                        // The input method talks to the focused page through
                        // this handler, registered anew each frame.
                        window.handle_input(
                            &focus_handle,
                            ElementInputHandler::new(bounds, page),
                            cx,
                        );
                    },
                )
                .size_full(),
            )
            .children(frame.map(|frame| {
                div().absolute().inset_0().child(
                    surface(frame)
                        .object_fit(ObjectFit::Fill)
                        .size_full()
                        .rounded_tl(self.corners.top_left)
                        .rounded_tr(self.corners.top_right)
                        .rounded_br(self.corners.bottom_right)
                        .rounded_bl(self.corners.bottom_left),
                )
            }))
    }
}

impl EntityInputHandler for Page {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let utf16: Vec<u16> = self.marked.as_deref()?.encode_utf16().collect();
        let start = range.start.min(utf16.len());
        let end = range.end.clamp(start, utf16.len());
        *adjusted = Some(start..end);
        Some(String::from_utf16_lossy(&utf16[start..end]))
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self
            .marked
            .as_deref()
            .map_or(0, |text| text.encode_utf16().count());
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked
            .as_deref()
            .map(|text| 0..text.encode_utf16().count())
    }

    /// AppKit unmarks by keeping the marked text, so it is committed.
    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        if let Some(text) = self.marked.clone() {
            self.commit(&text);
        }
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.commit(text);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        if text.is_empty() {
            // The person deleted the whole composition.
            if self.marked.take().is_some() {
                self.compose(CompositionState::End, "");
            }
            return;
        }
        if self.marked.is_none() {
            self.compose(CompositionState::Start, "");
        }
        self.compose(CompositionState::Update, text);
        self.marked = Some(text.to_owned());
    }

    /// Where the candidate window goes: under the focused field, as Servo
    /// reports it, else under the pointer.
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        element: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if let Some(control) = &self.input_method {
            let rect = control.position();
            let origin = point(
                element.origin.x + px(rect.min.x as f32 / self.scale),
                element.origin.y + px(rect.min.y as f32 / self.scale),
            );
            let extent = size(
                px(rect.width() as f32 / self.scale),
                px(rect.height() as f32 / self.scale),
            );
            return Some(Bounds::new(origin, extent));
        }
        let pointer = self.last_pointer?;
        Some(Bounds::new(pointer, size(px(1.), px(16.))))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// A Servo image as the BGRA frame GPUI draws. Servo hands over the icon's
/// pixels in whichever of its formats it decoded to.
fn favicon_image(icon: &Image) -> Option<Arc<RenderImage>> {
    let pixels = (icon.width * icon.height) as usize;
    let data = icon.data();
    let mut bgra = Vec::with_capacity(pixels * 4);
    match icon.format {
        PixelFormat::K8 => {
            for &k in data.get(..pixels)? {
                bgra.extend([k, k, k, 255]);
            }
        }
        PixelFormat::KA8 => {
            for p in data.get(..pixels * 2)?.chunks_exact(2) {
                bgra.extend([p[0], p[0], p[0], p[1]]);
            }
        }
        PixelFormat::RGB8 => {
            for p in data.get(..pixels * 3)?.chunks_exact(3) {
                bgra.extend([p[2], p[1], p[0], 255]);
            }
        }
        PixelFormat::RGBA8 => {
            for p in data.get(..pixels * 4)?.chunks_exact(4) {
                bgra.extend([p[2], p[1], p[0], p[3]]);
            }
        }
        PixelFormat::BGRA8 => bgra.extend_from_slice(data.get(..pixels * 4)?),
    }
    let frame = image::RgbaImage::from_raw(icon.width, icon.height, bgra)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(frame)])))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(url: &str, content_type: &str, html: &str) -> JSValue {
        JSValue::Array(
            [url, content_type, html]
                .map(|value| JSValue::String(value.to_owned()))
                .into(),
        )
    }

    fn visited_page(value: JSValue) -> Option<VisitedPage> {
        match read_back(value)? {
            ReadBack::Page(page) => Some(page),
            ReadBack::Unshown(_) => None,
        }
    }

    #[test]
    fn an_html_document_is_read_as_a_visited_page() {
        let page =
            visited_page(document("https://example.com/a", "text/html", "<p>a</p>")).unwrap();
        assert_eq!(page.url.as_str(), "https://example.com/a");
        assert_eq!(page.html, "<p>a</p>");
    }

    #[test]
    fn other_documents_are_not_read() {
        assert!(
            visited_page(document("https://example.com/a.png", "image/png", "<img>")).is_none()
        );
        assert!(visited_page(document("https://example.com/a", "text/html", "")).is_none());
        assert!(visited_page(document("not a url", "text/html", "<p>a</p>")).is_none());
        assert!(visited_page(JSValue::Null).is_none());
        assert!(visited_page(JSValue::Array(vec![JSValue::Null; 3])).is_none());
    }

    #[test]
    fn servos_placeholder_for_a_file_is_a_download() {
        let placeholder = "<html><head></head><body><p>Unknown content type \
            (application/x-apple-diskimage).</p></body></html>";
        for content_type in ["application/x-apple-diskimage", "text/html"] {
            let read = read_back(document(
                "https://example.com/get?id=1",
                content_type,
                placeholder,
            ));
            assert!(
                matches!(read, Some(ReadBack::Unshown(url)) if url.as_str() == "https://example.com/get?id=1")
            );
        }
        // A page that only mentions it is a page.
        let page = "<html><head></head><body><h1>Errors</h1>\
            <p>Unknown content type (text/x-foo).</p></body></html>";
        assert!(matches!(
            read_back(document("https://example.com/", "text/html", page)),
            Some(ReadBack::Page(_))
        ));
    }
}
