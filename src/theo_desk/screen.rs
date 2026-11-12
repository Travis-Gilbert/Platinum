//! Theo's screen: the face opens into a small browser of its own, and
//! closes back into the face without losing the page.
//!
//! The window grows to the screen's size at once, still showing the face
//! where it was, and the shell morphs in it: the dark plate stretches into
//! the bezel and the white eyes broaden into the content backing, with a
//! still of the page on it. The live page is shown, and takes input, only
//! once the screen has settled, at the size it will keep; it is never
//! resized frame by frame. Collapsing runs the same morph backwards and puts
//! the face back where it now sits in the screen.
//!
//! The page is Platinum's own `Page` over the profile's `Engine`, held by a
//! `morph::Session` from the first opening until Close. Collapsing hides it
//! and keeps it (address, history, scroll, what it plays), hiding Theo does
//! too, and a page whose window closed is moved to the next one
//! (`Page::rebind`). Close ends it. Pause and Play are the page's media
//! session's own actions; while it plays the face takes the boombox, fed the
//! level measured where the sound leaves (`meter`).
//!
//! SOURCING: `crate::page::Page` and its engine; GPUI Kit's `Button`,
//! `Input` and `Spinner` for the controls; GPUI's `surface` for the still;
//! AppKit, through `mac`, for the window's frame, level and resizing. The
//! geometry and the session are `morph`'s.

use std::time::Instant;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Disableable as _, Selectable as _, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AnyWindowHandle, AppContext as _, Context, Corners, Div, Entity, Focusable as _,
    InteractiveElement as _, IntoElement, MouseButton, ObjectFit, ParentElement as _, Rgba,
    Styled as _, Subscription, Window, div, px, surface,
};
use objc2_app_kit::NSWindowStyleMask;
use objc2_foundation::NSSize;
use servo::MediaSessionActionType;
use theo::particles::{GRAIN_STRIDE, oklch};
use url::Url;

use super::body::{self, BodyFrame, VISOR_PX};
use super::mac::{self, Rect};
use super::morph::{self, Area, Form, Playback, Session, Settled};
use super::shape::Fill;
use super::{Desk, meter};
use crate::address::{self, Target};
use crate::page::Page;

/// AppKit's window levels: Theo's own (a pop-up's, over everything) while
/// it is the face or pinned, an ordinary window's while the screen is not.
const POP_UP_LEVEL: isize = 101;
const NORMAL_LEVEL: isize = 0;

/// How the screen was opened, kept to put the face back.
#[derive(Clone, Copy, Debug)]
struct Opened {
    size: (f64, f64),
    /// Where the face sat in the screen's window.
    inside: Area,
}

/// The screen as the desk holds it.
#[derive(Default)]
pub(super) struct Host {
    session: Session<Entity<Page>>,
    /// The address to show once the page is made.
    wanted: Option<Url>,
    /// The window has been asked to grow into the screen and has not yet.
    growing: bool,
    asked: bool,
    /// Set where the window grew, so the morph starts from what AppKit did.
    grown: std::rc::Rc<std::cell::Cell<Option<Opened>>>,
    opened: Option<Opened>,
    /// Where the face is in the window while the morph runs, and the eyes
    /// it opened from, from the face's top left.
    inside: Area,
    eyes: Option<[Area; 2]>,
    /// Where the face's window goes once the screen has closed.
    back: Option<Rect>,
    /// The window the page is shown in.
    window: Option<AnyWindowHandle>,
    address: Option<(AnyWindowHandle, Entity<InputState>, Subscription)>,
    /// The address the field last showed.
    shown: Option<Url>,
    /// The size the person last left the screen at.
    size: Option<(f64, f64)>,
    /// The composer was asked for while the screen was open: it opens once
    /// the face is back.
    then_talk: bool,
}

/// What Theo's menu offers for the screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ScreenMenu {
    held: bool,
    playback: Playback,
}

impl Desk {
    /// Whether the window is the screen's, or becoming it.
    pub(super) fn screen_shows(&self) -> bool {
        self.screen.growing || self.screen.session.morph.shows_screen()
    }

    /// Whether the window takes every click: the settled screen does.
    pub(super) fn screen_takes_clicks(&self) -> bool {
        self.screen.session.morph.interactive()
    }

    /// Opens Theo's screen, onto `url` or onto the page it holds.
    pub(super) fn open_screen(&mut self, url: Option<Url>, cx: &mut Context<Self>) {
        meter::install();
        if let Some(url) = url {
            match self.screen.session.page() {
                Some(page) => page.update(cx, |page, _| page.load(url)),
                None => self.screen.wanted = Some(url),
            }
        }
        if self.screen.session.page().is_none() && self.screen.wanted.is_none() {
            return;
        }
        let now = self.now();
        let still = self.stage.borrow().still;
        match self.screen.session.morph.form() {
            // Closing: turn back from where it has got to.
            Form::Closing { .. } => {
                self.screen.session.morph.open(now, still);
                self.screen.back = None;
                cx.notify();
                return;
            }
            Form::Opening { .. } | Form::Screen => return,
            Form::Face if self.screen.growing => return,
            Form::Face => {}
        }
        // Grown as the window next draws, after the menu, if it was open,
        // has given the window back to the face alone.
        self.screen.growing = true;
        self.screen.asked = false;
        self.screen.then_talk = false;
        if self.companion.composer_open() {
            self.close_composer(cx);
        }
        self.appear(cx);
        cx.notify();
    }

    /// Closes the screen back into the face, keeping the page; `close` ends
    /// the page too once the face is back.
    pub(super) fn collapse_screen(&mut self, close: bool, cx: &mut Context<Self>) {
        let now = self.now();
        let still = self.stage.borrow().still;
        if self.screen.growing {
            self.screen.growing = false;
        }
        if self.screen.session.morph.form() == Form::Face {
            if close {
                self.end_screen();
            }
            return;
        }
        if let Some(page) = self.screen.session.page() {
            // The page shows as a still while the shell closes.
            page.update(cx, |page, _| page.set_visible(false));
        }
        if !matches!(self.screen.session.morph.form(), Form::Closing { .. }) {
            self.screen.back = None;
        }
        self.screen.session.morph.close(now, close, still);
        cx.notify();
    }

    /// The page goes, and with it what it plays.
    fn end_screen(&mut self) {
        self.screen.session.end();
        self.screen.wanted = None;
        self.screen.shown = None;
    }

    /// Asks the screen's page to pause or play.
    pub(super) fn screen_media(&mut self, action: MediaSessionActionType, cx: &mut Context<Self>) {
        if let Some(page) = self.screen.session.page() {
            page.read(cx).media(action);
        }
    }

    fn toggle_pin(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.screen.session.pinned = !self.screen.session.pinned;
        let level = if self.screen.session.pinned {
            POP_UP_LEVEL
        } else {
            NORMAL_LEVEL
        };
        mac::after_update(window, cx, move |ns| ns.setLevel(level));
        cx.notify();
    }

    /// Theo was hidden: its window closes. The page stays, out of sight and
    /// still playing, and the screen is collapsed when Theo comes back.
    pub(super) fn screen_hidden(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = self.screen.session.page() {
            page.update(cx, |page, _| page.set_visible(false));
        }
        self.screen.session.morph = morph::Morph::default();
        self.screen.growing = false;
        self.screen.opened = None;
        self.screen.back = None;
        self.screen.address = None;
        self.screen.then_talk = false;
    }

    /// The composer was asked for while the screen is open: the screen
    /// closes into the face first. Says whether it did.
    pub(super) fn screen_yields_to_talk(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.screen_shows() {
            return false;
        }
        self.collapse_screen(false, cx);
        self.screen.then_talk = true;
        true
    }

    pub(super) fn screen_menu(&self) -> ScreenMenu {
        ScreenMenu {
            held: self.screen.session.page().is_some(),
            playback: self.screen.session.playback(),
        }
    }

    /// The screen as the window draws it, or None while Theo is the face.
    pub(super) fn screen_view(
        &mut self,
        frame: &BodyFrame,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let now = self.now();
        self.follow_sound(cx);
        let handle = window.window_handle();
        if let Some(page) = self.screen.session.page().cloned()
            && self.screen.window != Some(handle)
        {
            page.update(cx, |page, cx| page.rebind(window, cx));
            self.screen.window = Some(handle);
        }
        if self.screen.growing && !self.screen.asked {
            self.screen.asked = true;
            self.grow(window, cx);
        }
        if let Some(opened) = self.screen.grown.take() {
            self.begin_opening(opened, frame, window, cx);
        }
        if let Some(settled) = self.screen.session.morph.tick(now) {
            self.settled(settled, window, cx);
        }
        if matches!(self.screen.session.morph.form(), Form::Closing { .. })
            && self.screen.back.is_none()
        {
            self.aim_back(window);
        }
        if !self.screen.session.morph.shows_screen() {
            return None;
        }
        Some(self.draw(frame, now, window, cx))
    }

    /// The face's sound on the face: the boombox while the page plays.
    fn follow_sound(&mut self, cx: &mut Context<Self>) {
        let generation = self.screen.session.generation();
        let playback = self
            .screen
            .session
            .page()
            .map(|page| page.read(cx).playback())
            .unwrap_or_default();
        self.screen.session.report(generation, playback);
        let epoch = self.epoch;
        let level = meter::level_now().map(|(level, heard): (f64, Instant)| {
            (
                level,
                heard.saturating_duration_since(epoch).as_secs_f64() * 1_000.0,
            )
        });
        if let Some(activity) = self.screen.session.sound(level) {
            self.companion.update(activity);
        }
    }

    /// Grows the face's window into the screen, centred on the face and
    /// kept on its screen.
    fn grow(&self, window: &Window, cx: &mut Context<Self>) {
        let grown = self.screen.grown.clone();
        let size = self.screen.size.unwrap_or(morph::SCREEN_SIZE);
        mac::after_update(window, cx, move |ns| {
            let Some(visible) = mac::visible_frame(ns) else {
                return;
            };
            let face: Rect = ns.frame().into();
            let screen = morph::screen_frame(face, visible.into(), size);
            ns.setFrame_display_animate(screen.into(), true, false);
            grown.set(Some(Opened {
                size: (screen.w, screen.h),
                inside: morph::face_in_window(screen, face),
            }));
        });
    }

    /// The window has grown: the morph begins, and the page is made if
    /// there is none, at the size it will be shown at.
    fn begin_opening(
        &mut self,
        opened: Opened,
        frame: &BodyFrame,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.screen.growing = false;
        self.screen.opened = Some(opened);
        self.screen.inside = opened.inside;
        let plate = plate_in(opened.inside, frame);
        let unit = VISOR_PX / frame.width;
        self.screen.eyes = Some(
            morph::eye_boxes(&frame.grains, GRAIN_STRIDE, unit, plate.centre())
                .unwrap_or_else(|| morph::default_eyes(plate))
                .map(|eye| Area {
                    x: eye.x - opened.inside.x,
                    y: eye.y - opened.inside.y,
                    ..eye
                }),
        );
        let wanted = self.screen.wanted.take();
        let made = self.screen.session.page_or(|| {
            let url = wanted?;
            match Page::open(url, window, cx) {
                Ok(page) => Some(page),
                Err(error) => {
                    log::error!("theo: its screen could not open a page: {error:#}");
                    None
                }
            }
        });
        if made && let Some(page) = self.screen.session.page() {
            page.update(cx, |page, _| {
                page.corners = Corners::all(px(morph::BACKING_RADIUS as f32));
                // Shown once the screen has settled.
                page.set_visible(false);
            });
            self.screen.window = Some(window.window_handle());
        }
        let now = self.now();
        if self.screen.session.page().is_none() {
            // Nothing to show: the face comes straight back.
            self.screen.back = None;
            self.screen.session.morph.open(now, true);
            self.screen.session.morph.close(now, true, true);
            return;
        }
        let still = self.stage.borrow().still;
        self.screen.session.morph.open(now, still);
    }

    /// Where the face goes back to, in the screen as it now is: where it sat
    /// in it when it opened, whatever the person has done to the screen since.
    fn aim_back(&mut self, window: &Window) {
        let Some(opened) = self.screen.opened else {
            return;
        };
        let Some(ns) = mac::ns_window(window) else {
            return;
        };
        let Some(visible) = mac::visible_frame(&ns) else {
            return;
        };
        let screen: Rect = ns.frame().into();
        let back = morph::face_frame(screen, opened.size, opened.inside, visible.into());
        self.screen.back = Some(back);
        self.screen.inside = morph::face_in_window(screen, back);
        self.screen.size = Some((screen.w, screen.h));
    }

    fn settled(&mut self, settled: Settled, window: &mut Window, cx: &mut Context<Self>) {
        match settled {
            Settled::Opened => {
                let pinned = self.screen.session.pinned;
                mac::after_update(window, cx, move |ns| {
                    ns.setStyleMask(ns.styleMask() | NSWindowStyleMask::Resizable);
                    ns.setContentMinSize(NSSize::new(morph::SCREEN_MIN.0, morph::SCREEN_MIN.1));
                    ns.setLevel(if pinned { POP_UP_LEVEL } else { NORMAL_LEVEL });
                });
                window.activate_window();
                if let Some(page) = self.screen.session.page().cloned() {
                    page.update(cx, |page, _| page.set_visible(true));
                    let focus = page.focus_handle(cx);
                    window.focus(&focus, cx);
                }
            }
            Settled::Collapsed | Settled::Closed => {
                if self.screen.back.is_none() {
                    self.aim_back(window);
                }
                if let Some(back) = self.screen.back.take() {
                    if let Some(visible) =
                        mac::ns_window(window).and_then(|ns| mac::visible_frame(&ns))
                    {
                        let spot = morph::spot(back, visible.into());
                        self.settings.spot = Some(spot);
                        self.stage.borrow_mut().rest = Some(spot);
                        self.save();
                    }
                    mac::after_update(window, cx, move |ns| {
                        ns.setStyleMask(ns.styleMask() & !NSWindowStyleMask::Resizable);
                        ns.setLevel(POP_UP_LEVEL);
                        ns.setFrame_display_animate(back.into(), true, false);
                    });
                }
                self.screen.opened = None;
                self.screen.address = None;
                if settled == Settled::Closed {
                    self.end_screen();
                }
                if std::mem::take(&mut self.screen.then_talk) {
                    self.talk(cx);
                } else if !self.companion.composer_open() {
                    self.give_back();
                }
            }
        }
        cx.notify();
    }

    /// Theo's body at this moment of the morph: the face's shape blended
    /// into the screen's, each part painted by what it is filled with. The
    /// settled screen shows the live page in place of its still.
    fn draw(
        &mut self,
        frame: &BodyFrame,
        now: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let viewport = window.viewport_size();
        let size = (f64::from(viewport.width), f64::from(viewport.height));
        let t = self.screen.session.morph.progress(now);
        let inside = self.screen.inside;
        let plate = plate_in(inside, frame);
        let eyes = self
            .screen
            .eyes
            .map(|eyes| {
                eyes.map(|eye| Area {
                    x: eye.x + inside.x,
                    y: eye.y + inside.y,
                    ..eye
                })
            })
            .unwrap_or_else(|| morph::default_eyes(plate));
        let shape = morph::opening().blend(
            &morph::face_shape(inside, plate, eyes),
            &morph::screen_shape(size),
            t,
        );
        let interactive = self.screen.session.morph.interactive();
        let [r, g, b] = oklch(0.17, 0.012, 255.0);
        let ink = Rgba {
            r: r as f32,
            g: g as f32,
            b: b as f32,
            a: 1.0,
        };
        let page = self.screen.session.page().cloned();
        let mut root = div().size_full().relative();
        for part in &shape.parts {
            if part.alpha <= 0.0 {
                continue;
            }
            let at = place(part.area).opacity(part.alpha as f32);
            let radius = px(part.radius as f32);
            let painted = match &part.fill {
                Fill::Ink => Some(at.rounded(radius).bg(ink).shadow_md()),
                Fill::Backing if !interactive => Some(at.rounded(radius).bg(gpui_kit::white())),
                Fill::Face => Some(at.child(body::body(frame.clone()))),
                // The settled screen shows the page itself.
                Fill::Still if interactive => page
                    .clone()
                    .map(|page| at.rounded(radius).bg(gpui_kit::white()).child(page)),
                Fill::Still => page
                    .as_ref()
                    .and_then(|page| page.read(cx).context.frame())
                    .map(|still| {
                        at.child(
                            surface(still)
                                .object_fit(ObjectFit::Cover)
                                .size_full()
                                .rounded(radius),
                        )
                    }),
                Fill::Controls(name) if name == morph::BAR => {
                    Some(at.child(self.controls(interactive, window, cx)))
                }
                Fill::Backing | Fill::Controls(_) => None,
            };
            if let Some(painted) = painted {
                root = root.child(painted);
            }
        }
        root.into_any_element()
    }

    /// The bar along the screen's top: back, the address, what is loading
    /// and playing, pin, collapse and close. Dragging it moves the screen.
    fn controls(
        &mut self,
        interactive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let desk = cx.entity();
        let address = self.address(window, cx);
        let page = self.screen.session.page().cloned();
        let (loading, can_go_back, url, playback) = page
            .as_ref()
            .map(|page| {
                let page = page.read(cx);
                (
                    page.is_loading(),
                    page.can_go_back(),
                    page.url(),
                    page.playback(),
                )
            })
            .unwrap_or_default();
        // The field follows the page unless the person is typing in it.
        let typing = address.read(cx).focus_handle(cx).is_focused(window);
        if !typing && url != self.screen.shown {
            let text = url.as_ref().map(Url::to_string).unwrap_or_default();
            address.update(cx, |field, cx| field.set_value(text, window, cx));
            self.screen.shown = url;
        }
        let pinned = self.screen.session.pinned;
        h_flex()
            .id("theo-screen-controls")
            .size_full()
            .gap_1()
            .px_1()
            .items_center()
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            .child(hold(
                Button::new("theo-screen-back")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ArrowLeft)
                    .accessibility_label("Back")
                    .tooltip("Back")
                    .disabled(!interactive || !can_go_back)
                    .on_click({
                        let page = page.clone();
                        move |_, _, cx| {
                            if let Some(page) = &page {
                                page.update(cx, |page, _| page.back());
                            }
                        }
                    }),
            ))
            .child(hold(Input::new(&address).xsmall()).flex_1().min_w_0())
            .when(loading, |bar| bar.child(Spinner::new().xsmall()))
            .when(playback != Playback::None, |bar| {
                let playing = playback == Playback::Playing;
                let desk = desk.clone();
                bar.child(hold(
                    Button::new("theo-screen-playback")
                        .ghost()
                        .xsmall()
                        .icon(if playing { Lucide::Pause } else { Lucide::Play })
                        .accessibility_label(if playing { "Pause" } else { "Play" })
                        .tooltip(if playing { "Pause" } else { "Play" })
                        .on_click(move |_, _, cx| {
                            let action = if playing {
                                MediaSessionActionType::Pause
                            } else {
                                MediaSessionActionType::Play
                            };
                            desk.update(cx, |desk, cx| desk.screen_media(action, cx));
                        }),
                ))
            })
            .child(hold(
                Button::new("theo-screen-pin")
                    .ghost()
                    .xsmall()
                    .icon(if pinned { Lucide::PinOff } else { Lucide::Pin })
                    .selected(pinned)
                    .accessibility_label(if pinned { "Unpin" } else { "Keep on Top" })
                    .tooltip(if pinned { "Unpin" } else { "Keep on Top" })
                    .on_click({
                        let desk = desk.clone();
                        move |_, window, cx| desk.update(cx, |desk, cx| desk.toggle_pin(window, cx))
                    }),
            ))
            .child(hold(
                Button::new("theo-screen-collapse")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Minimize2)
                    .accessibility_label("Back to Theo")
                    .tooltip("Back to Theo")
                    .on_click({
                        let desk = desk.clone();
                        move |_, _, cx| desk.update(cx, |desk, cx| desk.collapse_screen(false, cx))
                    }),
            ))
            .child(hold(
                Button::new("theo-screen-close")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::X)
                    .accessibility_label("Close Page")
                    .tooltip("Close Page")
                    .on_click(move |_, _, cx| {
                        desk.update(cx, |desk, cx| desk.collapse_screen(true, cx))
                    }),
            ))
    }

    /// The address field, made for the window it is shown in.
    fn address(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        let handle = window.window_handle();
        if let Some((shown_in, field, _)) = &self.screen.address
            && *shown_in == handle
        {
            return field.clone();
        }
        let field = cx.new(|cx| InputState::new(window, cx).placeholder("Address"));
        let entered = cx.subscribe(&field, |desk, field, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::PressEnter { .. }) {
                return;
            }
            let text = field.read(cx).value().to_string();
            match address::classify(&text) {
                Some(Target::Go(url)) => {
                    if let Some(page) = desk.screen.session.page() {
                        page.update(cx, |page, _| page.load(url));
                    }
                }
                // Platinum's search results are the browser window's own
                // view; the screen opens addresses only.
                Some(Target::Search(_)) | None => {}
            }
        });
        self.screen.shown = None;
        self.screen.address = Some((handle, field.clone(), entered));
        field
    }
}

/// A control on the bar: a press on it is the control's, not a move.
fn hold(control: impl IntoElement) -> gpui_kit::Div {
    div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(control)
}

/// A box placed at `area` in the window.
fn place(area: Area) -> gpui_kit::Div {
    div()
        .absolute()
        .left(px(area.x as f32))
        .top(px(area.y as f32))
        .w(px(area.w as f32))
        .h(px(area.h as f32))
}

/// The face's plate, for the face at `inside` in the window.
fn plate_in(inside: Area, frame: &BodyFrame) -> Area {
    let height = VISOR_PX * frame.height / frame.width;
    Area {
        x: inside.x + (inside.w - VISOR_PX) / 2.0,
        y: inside.y + (inside.h - height) / 2.0,
        w: VISOR_PX,
        h: height,
    }
}

/// The screen's actions in Theo's menu: show it again, pause or play what
/// it plays, close it.
pub(super) fn actions(mut row: Div, desk: &Entity<Desk>, content: &ScreenMenu) -> Div {
    if !content.held {
        return row;
    }
    let show = desk.clone();
    row = row.child(
        Button::new("theo-show-page")
            .label("Show Page")
            .icon(Lucide::Maximize2)
            .ghost()
            .small()
            .on_click(move |_, _, cx| show.update(cx, |desk, cx| desk.open_screen(None, cx))),
    );
    if content.playback != Playback::None {
        let playing = content.playback == Playback::Playing;
        let media = desk.clone();
        row = row.child(
            Button::new("theo-page-playback")
                .label(if playing { "Pause" } else { "Play" })
                .icon(if playing { Lucide::Pause } else { Lucide::Play })
                .ghost()
                .small()
                .on_click(move |_, _, cx| {
                    let action = if playing {
                        MediaSessionActionType::Pause
                    } else {
                        MediaSessionActionType::Play
                    };
                    media.update(cx, |desk, cx| desk.screen_media(action, cx));
                }),
        );
    }
    let close = desk.clone();
    row.child(
        Button::new("theo-close-page")
            .label("Close Page")
            .icon(Lucide::X)
            .ghost()
            .small()
            .on_click(move |_, _, cx| close.update(cx, |desk, cx| desk.collapse_screen(true, cx))),
    )
}

/// What typed into Theo's field opens in the screen rather than going to
/// the agent: a whole web address, and nothing else.
pub(super) fn address_typed(text: &str) -> Option<Url> {
    let text = text.trim();
    if text.contains(char::is_whitespace) {
        return None;
    }
    let url = Url::parse(text).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_whole_web_address_opens_the_screen() {
        assert_eq!(
            address_typed(" https://www.youtube.com/watch?v=x ").map(String::from),
            Some("https://www.youtube.com/watch?v=x".to_owned())
        );
        assert!(address_typed("youtube.com").is_none());
        assert!(address_typed("look at https://servo.org").is_none());
        assert!(address_typed("file:///etc/hosts").is_none());
        assert!(address_typed("ok.").is_none());
    }
}
