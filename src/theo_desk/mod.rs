//! Theo on the desktop: the companion in a small window of its own that
//! floats over every application, a menu to talk to the selected agent
//! through, and the keys and clicks that call it.
//!
//! One `Desk` per process holds the `Companion`, the clock, and the Theorem
//! driver it shares with the browser's Agent place, so Theo and the place
//! show one conversation. Theo's window is only a view of the desk: hiding
//! it closes the window, and showing it again opens a new one onto the same
//! face, the same run and the same draft. Hiding never touches the run; Stop
//! is its own action, routed to the driver that owns the run.
//!
//! Talking: a right click on Theo, or the talk key, opens a menu over it
//! with a field to type into straight away. The same menu then carries
//! Theorem's reply, and Stop while a run goes on. There is no standing text
//! box and no window around Theo: only the face, on a transparent window.
//!
//! Focus: shown by a key or a double right click, Theo floats over the
//! application in front without taking focus from it. Only the menu takes
//! the keyboard, and closing it hands the keyboard back to the application
//! it was called over.
//!
//! SOURCING: `companion` and `theo` (this repository's port of web Theo);
//! GPUI Kit's `Root`, `PopupMenu` and `Input`; `global-hotkey` for keys that
//! work while another application is in front; AppKit and Core Graphics in
//! `mac.rs`.

pub mod activity;
mod body;
mod mac;

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use companion::{Calling, Companion, Scope};
use gpui_kit::component::input::{
    Escape, InlineToken, Input, InputContent, InputEvent, InputState, InputToken, Textarea,
    TextareaState,
};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, IconName, Root, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, DismissEvent, Entity, FocusHandle,
    Focusable as _, Global, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    Pixels, Render, Styled as _, Subscription, Window,
    WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, actions,
    canvas, div, point, px, size,
};
use serde::{Deserialize, Serialize};
use theo::engine::{Expression, FaceOptions, Liveness};
use theo::player::Stage;
use theo::program::Place;
use theorem_surface_contracts::intent::ComposerIntent;
use theorem_surface_contracts::thread::{ThreadActor, ThreadPart};
use theoremweb_agent_runtime::ChatHostPhase;

use self::activity::{Runs, activity_id};
use self::body::{BODY_SIZE, BodyFrame};
use crate::theorem::{Connection, DriverEvent, TheoremDriver};

actions!(
    theo,
    [
        /// Show Theo over the application in front, or hide it.
        ToggleTheo,
        /// Open Theo's composer to talk to the selected agent.
        TalkToTheo,
        /// Hide Theo. Whatever it is doing goes on.
        HideTheo,
        /// Stop the run Theo shows.
        StopTheo,
        /// Whether two right clicks in one place call Theo.
        CallWithDoubleRightClick,
    ]
);

/// The menu's width and the room beside Theo it may take, in points. The
/// menu keeps its width; its reply grows with what Theorem says, from one
/// line to `REPLY_ROWS`, and scrolls past that.
const MENU_WIDTH: f32 = 320.0;
const MENU_ROOM: f32 = 440.0;
const REPLY_ROWS: usize = 12;
/// Clear space either side of the menu for its shadow, in points.
const MENU_MARGIN: f32 = 12.0;
/// Theo's window while the menu is open: the menu and its margins (wider
/// than the face) by the face and the room beside it, in points.
const OPEN_SIZE: (f64, f64) = (
    MENU_WIDTH as f64 + 2.0 * MENU_MARGIN as f64,
    MENU_ROOM as f64 + BODY_SIZE.1,
);
/// How far from the plate the pointer is still followed, in points.
const FOLLOW_WITHIN: f64 = 480.0;
/// The pointer at this distance turns the gaze all the way, in points.
const GAZE_REACH: f64 = 240.0;
/// How often the reduce-motion setting is read again, in milliseconds.
const STILL_EVERY_MS: f64 = 1_000.0;

/// What the person set, kept as `theo.json` in the profile folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Settings {
    /// Shows or hides Theo from any application.
    #[serde(default = "Settings::default_toggle")]
    toggle: String,
    /// Opens the composer from any application.
    #[serde(default = "Settings::default_talk")]
    talk: String,
    #[serde(default)]
    double_right_click: bool,
    /// Where on its screen Theo rests, as `mac::spot_of` measures it.
    #[serde(default)]
    spot: Option<(f64, f64)>,
}

impl Settings {
    fn default_toggle() -> String {
        "control+alt+KeyT".to_owned()
    }
    fn default_talk() -> String {
        "control+alt+KeyC".to_owned()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            toggle: Self::default_toggle(),
            talk: Self::default_talk(),
            double_right_click: false,
            spot: None,
        }
    }
}

/// The bottom right of the screen, where Theo rests until moved.
const HOME: (f64, f64) = (0.98, 0.04);

/// What the face's programs can ask of the desktop, shared between the
/// desk and the adapter the face holds. The face asks during its frame,
/// when no window is at hand, so a move is noted and made after the frame.
#[derive(Debug, Default)]
struct StageState {
    pointer: Option<(f64, f64)>,
    composer: bool,
    /// The composer opened over the face rather than under it.
    composer_above: bool,
    rest: Option<(f64, f64)>,
    go: Option<((f64, f64), bool)>,
    still: bool,
}

struct DeskStage(Rc<RefCell<StageState>>);

impl Stage for DeskStage {
    fn gaze_at(&self, place: &Place) -> Option<(f64, f64)> {
        let state = self.0.borrow();
        match place {
            Place::Named("pointer") => state.pointer,
            // The composer opens under the body, or over it at the bottom
            // of the screen.
            Place::Named("composer") => state
                .composer
                .then_some((0.0, if state.composer_above { -0.8 } else { 0.8 })),
            Place::Named("home") => Some((0.0, 0.0)),
            _ => theo::engine::plain_gaze(place),
        }
    }

    fn go(&mut self, place: &Place, _size: Option<f64>, over: f64) {
        let mut state = self.0.borrow_mut();
        let spot = match place {
            Place::Spot { x, y } => (*x, 1.0 - *y),
            Place::Named("home") => state.rest.unwrap_or(HOME),
            Place::Named("centre") => (0.5, 0.5),
            _ => return,
        };
        let animate = over > 0.0 && !state.still;
        state.go = Some((spot, animate));
    }

    // The face kicks its own matter; there is nothing on the desktop to kick.
    fn burst(&mut self, _x: f64, _y: f64, _strength: f64) {}
}

pub struct Desk {
    companion: Companion,
    epoch: Instant,
    runs: Runs,
    driver: Entity<TheoremDriver>,
    settings: Settings,
    path: Option<PathBuf>,
    window: Option<(AnyWindowHandle, Entity<Body>)>,
    /// How the window was laid out when it grew to hold the menu, once it
    /// has; set where the window changes (see `mac::open_menu`).
    menu_form: Rc<Cell<Option<mac::MenuForm>>>,
    /// The window has been asked to grow for the menu and not yet to shrink.
    menu_grown: bool,
    /// A window is being made for Theo, once the update asking for it ends.
    opening: bool,
    /// The application and window Theo was last called over.
    front: Option<mac::Front>,
    liveness: Liveness,
    expression: Option<Expression>,
    phase: ChatHostPhase,
    stage: Rc<RefCell<StageState>>,
    still_read_at: f64,
    /// The keys that call Theo from anywhere, and their ids.
    hotkeys: Option<(global_hotkey::GlobalHotKeyManager, u32, u32)>,
    /// Why a key or the double right click cannot call Theo, when one cannot.
    unheard: Vec<String>,
    right_clicks: bool,
    _subscriptions: Vec<Subscription>,
}

struct DeskGlobal(Entity<Desk>);

impl Global for DeskGlobal {}

/// The process's desk, made the first time it is asked for.
fn desk(cx: &mut App) -> Entity<Desk> {
    if let Some(desk) = cx.try_global::<DeskGlobal>() {
        return desk.0.clone();
    }
    let desk = cx.new(|cx| Desk::new(None, cx));
    cx.set_global(DeskGlobal(desk.clone()));
    desk
}

/// Theorem's driver for this process: the one Theo talks through. The
/// browser's Agent place takes the same one, so both show one conversation.
pub fn driver(cx: &mut App) -> Entity<TheoremDriver> {
    desk(cx).read(cx).driver.clone()
}

/// Whether `window` is Theo's, which does not keep Platinum open alone.
pub fn is_theo(window: AnyWindowHandle, cx: &App) -> bool {
    cx.try_global::<DeskGlobal>()
        .and_then(|desk| desk.0.read(cx).window.as_ref().map(|(shown, _)| *shown))
        == Some(window)
}

/// Makes the desk with the settings at `path`, and registers its actions.
pub fn init(path: PathBuf, cx: &mut App) {
    let desk = cx.new(|cx| Desk::new(Some(path), cx));
    cx.set_global(DeskGlobal(desk));
    cx.on_action(|_: &ToggleTheo, cx| with_desk(cx, |desk, cx| desk.toggle(cx)));
    cx.on_action(|_: &TalkToTheo, cx| with_desk(cx, |desk, cx| desk.talk(cx)));
    cx.on_action(|_: &HideTheo, cx| with_desk(cx, |desk, cx| desk.hide(cx)));
    cx.on_action(|_: &StopTheo, cx| with_desk(cx, |desk, cx| desk.stop(cx)));
    cx.on_action(|_: &CallWithDoubleRightClick, cx| {
        with_desk(cx, |desk, cx| desk.set_double_right_click(cx))
    });
}

fn with_desk(cx: &mut App, f: impl FnOnce(&mut Desk, &mut Context<Desk>)) {
    let desk = desk(cx);
    desk.update(cx, f);
}

/// The Theo menu's items, with the double right click's check as it stands.
pub fn menu(cx: &App) -> gpui_kit::Menu {
    use gpui_kit::MenuItem;
    let double = cx
        .try_global::<DeskGlobal>()
        .is_some_and(|desk| desk.0.read(cx).settings.double_right_click);
    gpui_kit::Menu::new("Theo").items([
        MenuItem::action("Show or Hide Theo", ToggleTheo),
        MenuItem::action("Talk to Theo", TalkToTheo),
        MenuItem::action("Stop Theo's Run", StopTheo),
        MenuItem::separator(),
        MenuItem::action(
            "Call Theo with Double Right-Click",
            CallWithDoubleRightClick,
        )
        .checked(double),
    ])
}

fn load(path: &Path) -> Settings {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The pointer as a gaze, while it is near enough to follow.
fn gaze_of(pointer: (f64, f64)) -> Option<(f64, f64)> {
    (pointer.0.hypot(pointer.1) <= FOLLOW_WITHIN).then(|| {
        (
            (pointer.0 / GAZE_REACH).clamp(-1.0, 1.0),
            (pointer.1 / GAZE_REACH).clamp(-1.0, 1.0),
        )
    })
}

/// Theorem's latest message as the reply shows it: its words, with each
/// citation, attachment and artifact as one inline token.
fn reply_content(parts: &[ThreadPart]) -> Option<InputContent> {
    let mut text = String::new();
    let mut tokens: Vec<(Range<usize>, InlineToken)> = Vec::new();
    let mut after_token = false;
    for part in parts {
        let (id, name) = match part {
            ThreadPart::Text { text: words, .. } | ThreadPart::Code { source: words, .. } => {
                if words.is_empty() {
                    continue;
                }
                if !text.is_empty() && !text.ends_with('\n') && (after_token || !words.starts_with(char::is_whitespace)) {
                    text.push('\n');
                }
                text.push_str(words);
                after_token = false;
                continue;
            }
            ThreadPart::Citation {
                source_id, label, ..
            } => (format!("citation:{source_id}"), label.clone()),
            ThreadPart::Attachment { part_id, name, .. } => {
                (format!("attachment:{part_id}"), name.clone())
            }
            ThreadPart::Artifact {
                artifact_id,
                artifact_kind,
                fallback_text,
                ..
            } => (
                format!("artifact:{artifact_id}"),
                fallback_text
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .unwrap_or(artifact_kind)
                    .to_owned(),
            ),
            _ => continue,
        };
        let name = token_text(&name, &id);
        if !text.is_empty() && !text.ends_with(char::is_whitespace) {
            text.push(' ');
        }
        let range = text.len()..text.len() + name.len();
        text.push_str(&name);
        tokens.push((range, InlineToken::new(id, name)));
        after_token = true;
    }
    if text.trim().is_empty() {
        return None;
    }
    let mut content = InputContent::new(text);
    for (range, token) in tokens {
        content = match content.clone().with_token(range, token) {
            Ok(content) => content,
            Err(error) => {
                log::warn!("theo: a reply token was refused: {error:?}");
                content
            }
        };
    }
    Some(content)
}

/// A token's text: one line with no control characters, never empty.
fn token_text(name: &str, id: &str) -> String {
    let words = name
        .split(|c: char| c.is_whitespace() || c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if !words.is_empty() {
        return words;
    }
    match id.split_once(':').map(|(kind, _)| kind) {
        Some("citation") => "Source",
        Some("attachment") => "Attachment",
        _ => "Artifact",
    }
    .to_owned()
}

/// The mark a reply token wears for what it refers to.
fn token_icon(id: &str) -> IconName {
    if id.starts_with("citation:") {
        IconName::BookOpen
    } else if id.starts_with("attachment:") {
        IconName::File
    } else {
        IconName::Frame
    }
}

impl Desk {
    fn new(path: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let settings = path.as_deref().map(load).unwrap_or_default();
        let driver = cx.new(TheoremDriver::new);
        let stage = Rc::new(RefCell::new(StageState {
            rest: settings.spot,
            ..StageState::default()
        }));
        let mut companion = Companion::new(FaceOptions::default());
        companion
            .face
            .attach(Some(Box::new(DeskStage(stage.clone()))));
        companion.set_calling(Calling {
            double_right_click: settings.double_right_click,
            ..Calling::default()
        });
        let subscriptions = vec![
            cx.observe(&driver, |desk, _, cx| desk.follow(cx)),
            // Pages Theorem asks for open in the person's browser when the
            // Agent place is not there to take them.
            cx.subscribe(&driver, |_, _, event: &DriverEvent, cx| {
                if let DriverEvent::OpenUrl(url) = event {
                    cx.open_url(url.as_str());
                }
            }),
        ];
        let mut desk = Self {
            companion,
            epoch: Instant::now(),
            runs: Runs::default(),
            driver,
            settings,
            path,
            window: None,
            menu_form: Rc::new(Cell::new(None)),
            menu_grown: false,
            opening: false,
            front: None,
            liveness: Liveness::Idle,
            expression: None,
            phase: ChatHostPhase::default(),
            stage,
            still_read_at: f64::NEG_INFINITY,
            hotkeys: None,
            unheard: Vec::new(),
            right_clicks: false,
            _subscriptions: subscriptions,
        };
        desk.listen_for_keys(cx);
        if desk.settings.double_right_click {
            desk.listen_for_right_clicks(cx);
        }
        desk.follow(cx);
        desk
    }

    fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64() * 1_000.0
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Ok(bytes) = serde_json::to_vec_pretty(&self.settings)
            && let Err(error) = std::fs::write(path, bytes)
        {
            log::warn!("theo: could not save {}: {error}", path.display());
        }
    }

    // The keys and the clicks ----------------------------------------------

    fn listen_for_keys(&mut self, cx: &mut Context<Self>) {
        use global_hotkey::hotkey::HotKey;
        use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => manager,
            Err(error) => {
                self.unheard.push(format!("keys: {error}"));
                return;
            }
        };
        let mut register = |text: &str| -> Option<u32> {
            let key: HotKey = match text.parse() {
                Ok(key) => key,
                Err(error) => {
                    self.unheard.push(format!("{text}: {error}"));
                    return None;
                }
            };
            match manager.register(key) {
                Ok(()) => Some(key.id()),
                Err(error) => {
                    self.unheard.push(format!("{text}: {error}"));
                    None
                }
            }
        };
        let toggle = register(&self.settings.toggle.clone()).unwrap_or(0);
        let talk = register(&self.settings.talk.clone()).unwrap_or(0);
        let (sender, mut receiver) = futures::channel::mpsc::unbounded();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state() == HotKeyState::Pressed {
                let _ = sender.unbounded_send(event.id());
            }
        }));
        self.hotkeys = Some((manager, toggle, talk));
        cx.spawn(async move |desk, cx| {
            use futures::StreamExt as _;
            while let Some(id) = receiver.next().await {
                let alive = desk.update(cx, |desk, cx| {
                    let Some((_, toggle, talk)) = desk.hotkeys else {
                        return;
                    };
                    if id == toggle {
                        desk.toggle(cx);
                    } else if id == talk {
                        desk.talk(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn listen_for_right_clicks(&mut self, cx: &mut Context<Self>) {
        if self.right_clicks {
            return;
        }
        let mut receiver = match mac::hear_right_clicks() {
            Ok(receiver) => receiver,
            Err(deaf) => {
                self.unheard.push(match deaf {
                    mac::Deaf::NotPermitted => "Double right-click needs Input Monitoring for \
                        Platinum (System Settings, Privacy & Security), then a relaunch."
                        .to_owned(),
                    mac::Deaf::Refused => {
                        "Double right-click: macOS refused the event tap.".to_owned()
                    }
                });
                return;
            }
        };
        self.right_clicks = true;
        cx.spawn(async move |desk, cx| {
            use futures::StreamExt as _;
            while let Some(click) = receiver.next().await {
                let alive = desk.update(cx, |desk, cx| {
                    let now = desk.now();
                    if desk.companion.right_click(now, click.x, click.y) {
                        desk.toggle(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn set_double_right_click(&mut self, cx: &mut Context<Self>) {
        self.settings.double_right_click = !self.settings.double_right_click;
        self.companion.set_calling(Calling {
            double_right_click: self.settings.double_right_click,
            ..self.companion.calling()
        });
        if self.settings.double_right_click {
            self.listen_for_right_clicks(cx);
        }
        self.save();
        // The menus read the desk for their check, so they are made again
        // once this update has ended.
        cx.defer(crate::install_menus);
        cx.notify();
    }

    // Showing and hiding ---------------------------------------------------

    /// Where Theo is being called from, as it stands now.
    fn capture(&mut self, cx: &App) -> Scope {
        if let Some(front) = mac::frontmost() {
            self.front = Some(front);
        }
        let front = self.front.as_ref();
        Scope {
            conversation: self.driver.read(cx).projection().selected_thread_id.clone(),
            environment: Some("desktop".to_owned()),
            surface: front.and_then(|front| front.bundle.clone().or(front.name.clone())),
            target: front.and_then(|front| front.window.map(|window| format!("window:{window}"))),
        }
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.companion.visible() {
            self.hide(cx);
        } else {
            let scope = self.capture(cx);
            self.companion.show(scope);
            self.mount(cx);
        }
    }

    fn talk(&mut self, cx: &mut Context<Self>) {
        if self.companion.composer_open() {
            if let Some((handle, body)) = self.window.clone() {
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, cx| {
                        body.update(cx, |body, cx| body.focus(window, cx))
                    });
                });
            }
            return;
        }
        let scope = self.capture(cx);
        self.companion.show(scope);
        self.companion.open_composer();
        self.stage.borrow_mut().composer = true;
        self.mount(cx);
    }

    fn close_composer(&mut self, cx: &mut Context<Self>) {
        self.companion.close_composer();
        self.stage.borrow_mut().composer = false;
        self.give_back();
        self.mount(cx);
    }

    fn hide(&mut self, cx: &mut Context<Self>) {
        let had_keys = self.companion.composer_open();
        self.companion.hide();
        self.stage.borrow_mut().composer = false;
        if let Some((handle, _)) = self.forget_window() {
            // Closed once this update has ended: closing a window runs the
            // app's window-closed callbacks, which read the desk.
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            });
        }
        if had_keys {
            self.give_back();
        }
        cx.notify();
    }

    /// Lets go of Theo's window, and of how it was laid out for the menu.
    fn forget_window(&mut self) -> Option<(AnyWindowHandle, Entity<Body>)> {
        self.menu_grown = false;
        self.menu_form.set(None);
        self.window.take()
    }

    /// Hands the keyboard back to the application Theo was called over, if
    /// Platinum took it.
    fn give_back(&self) {
        if let Some(front) = &self.front
            && mac::we_are_active()
        {
            mac::return_to(front.pid);
        }
    }

    /// Mounts the face alone; the window grows around it for the menu.
    fn mount(&mut self, cx: &mut Context<Self>) {
        let keys = self.companion.composer_open();
        if let Some((handle, body)) = self.window.clone() {
            // Asked from inside Theo's own window (a right click, Escape, the
            // menu closing), the window cannot be updated until that update
            // ends, so the fitting waits for it. Only a window that is gone
            // is forgotten: taking a busy one for gone opened a second Theo.
            if cx.windows().contains(&handle) {
                let rest = self.settings.spot.unwrap_or(HOME);
                let grow = keys && !self.menu_grown;
                let shrink = !keys && self.menu_grown;
                self.menu_grown = keys;
                let form = self.menu_form.clone();
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, cx| {
                        if grow {
                            mac::open_menu(window, OPEN_SIZE, form, cx);
                        } else if shrink {
                            mac::close_menu(window, BODY_SIZE, form, cx);
                        } else if !keys {
                            mac::resize_at(window, BODY_SIZE, rest, cx);
                        }
                        if keys {
                            window.activate_window();
                            body.update(cx, |body, cx| body.focus(window, cx));
                        }
                    });
                });
                cx.notify();
                return;
            }
            self.forget_window();
        }
        if self.opening {
            return;
        }
        // The window is made once this update has ended: its body reads the
        // desk as it is made and as it first draws.
        self.opening = true;
        let desk = cx.entity();
        cx.defer(move |cx| open(desk, cx));
    }

    // The run --------------------------------------------------------------

    /// Follows the driver: the selected conversation's run, its manner, and
    /// a snapshot of what is still going whenever the authority is ready again.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let now = self.now();
        let projection = self.driver.read(cx).projection();
        let phase = projection.phase;
        let thread = projection.thread.clone();
        if phase == ChatHostPhase::Ready && self.phase != ChatHostPhase::Ready {
            // Connected, or connected again: what the companion holds is
            // replaced by what is going on now, then updates follow in order.
            self.runs.observe(&thread, now);
            self.companion.restore(self.runs.snapshot());
        } else if let Some(activity) = self.runs.observe(&thread, now) {
            self.companion.update(activity);
        }
        self.phase = phase;
        let expression = activity::expression(&thread);
        if expression != self.expression {
            self.companion.express(expression.as_ref());
            self.expression = expression;
        }
        self.settle_liveness(now, cx);
        cx.notify();
    }

    fn settle_liveness(&mut self, now: f64, cx: &App) {
        let state = self.driver.read(cx).projection().thread.run_state;
        let liveness = self
            .runs
            .liveness(state, self.companion.composer_open(), now);
        if liveness != self.liveness {
            self.liveness = liveness;
            self.companion.set_liveness(liveness);
        }
    }

    /// Asks the run's owner to stop the run Theo shows. Nothing is ended
    /// here, and a stop for a run that has since been replaced is refused.
    fn stop(&mut self, cx: &mut Context<Self>) {
        let thread = &self.driver.read(cx).projection().thread;
        if !thread.run_state.is_live() {
            return;
        }
        let selected = activity_id(&thread.thread_id);
        let routed = match self.companion.stop() {
            Some(request) => {
                request.activity == selected
                    && self.runs.is_current(&request.activity, request.generation)
            }
            // A run waiting on the person has no form, so the face shows no
            // primary activity; the stop is still for the selected run.
            None => true,
        };
        if routed {
            self.driver.update(cx, |driver, cx| driver.stop(cx));
        } else {
            log::info!("theo: refused a stop for a run that is no longer the selected one");
        }
    }

    /// One frame: the clock, stillness, the pointer, the face's own moves.
    /// `menu` is where the open menu was drawn last, in window points.
    fn frame(&mut self, window: &mut Window, menu: Option<Bounds<Pixels>>, cx: &App) -> BodyFrame {
        let now = self.now();
        if now - self.still_read_at >= STILL_EVERY_MS {
            self.still_read_at = now;
            let still = mac::reduce_motion();
            if still != self.stage.borrow().still {
                self.stage.borrow_mut().still = still;
                self.companion.set_still(still);
            }
        }
        // The pointer in window points, and from the face's centre, wherever
        // the face sits in the window.
        let pointer = mac::pointer_in(window);
        let form = self.menu_form.get();
        let viewport = window.viewport_size();
        let (width, height) = (f64::from(viewport.width), f64::from(viewport.height));
        let centre = match form {
            None => (width / 2.0, height / 2.0),
            Some(form) => (
                form.face_x + BODY_SIZE.0 / 2.0,
                match form.side {
                    mac::Side::Below => BODY_SIZE.1 / 2.0,
                    mac::Side::Above => height - BODY_SIZE.1 / 2.0,
                },
            ),
        };
        let from_face = pointer.map(|(x, y)| (x - centre.0, y - centre.1));
        let gaze = from_face.and_then(gaze_of);
        {
            let mut stage = self.stage.borrow_mut();
            stage.pointer = gaze;
            stage.composer_above = form.is_some_and(|form| form.side == mac::Side::Above);
        }
        self.companion.face.follow_pointer(gaze);
        self.settle_liveness(now, cx);
        let grains = self.companion.frame(now).to_vec();
        // The open menu holds Theo where it is; a move waits for it to close.
        let go = if form.is_none() {
            self.stage.borrow_mut().go.take()
        } else {
            None
        };
        if let Some((spot, animate)) = go {
            mac::place(window, spot, animate, cx);
        }
        // Only the face and the menu take clicks: everywhere else on the
        // window lets them through to what is beneath.
        let open = self.companion.composer_open();
        let over_face = from_face
            .is_some_and(|(x, y)| x.abs() <= body::VISOR_PX / 2.0 + 8.0 && y.abs() <= 48.0);
        let over_menu = open
            && pointer
                .zip(menu)
                .is_some_and(|((x, y), menu)| menu.contains(&point(px(x as f32), px(y as f32))));
        mac::pass_clicks(window, !(over_face || over_menu));
        let face = &self.companion.face;
        BodyFrame {
            grains,
            head: face.head(),
            emitting: face.emitting(),
            width: f64::from(face.identity.width),
            height: f64::from(face.identity.height),
            hue: face.hue,
            still: self.stage.borrow().still,
            now,
            dark: matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        }
    }

    fn words(&self) -> String {
        self.companion.face.words().to_owned()
    }

    /// The person moved Theo: rest there from now on.
    fn rest_here(&mut self, window: &Window) {
        let form = self.menu_form.get().map(|form| (form, BODY_SIZE));
        if let Some(spot) = mac::spot_of(window, form) {
            self.settings.spot = Some(spot);
            self.stage.borrow_mut().rest = Some(spot);
            self.save();
        }
    }
}

/// Opens Theo's window for what the desk shows now, unless it was hidden
/// while waiting.
fn open(desk: Entity<Desk>, cx: &mut App) {
    let (keys, rest, draft) = {
        let shown = desk.read(cx);
        if !shown.companion.visible() || shown.window.is_some() {
            desk.update(cx, |desk, _| desk.opening = false);
            return;
        }
        (
            shown.companion.composer_open(),
            shown.settings.spot.unwrap_or(HOME),
            shown.driver.read(cx).projection().composer.draft.clone(),
        )
    };
    let wanted = size(px(BODY_SIZE.0 as f32), px(BODY_SIZE.1 as f32));
    let origin = cx
        .primary_display()
        .map_or(point(px(0.), px(0.)), |display| {
            let visible = display.visible_bounds();
            point(
                visible.right() - wanted.width - px(16.),
                visible.bottom() - wanted.height - px(16.),
            )
        });
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, wanted))),
        titlebar: None,
        focus: keys,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: true,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..WindowOptions::default()
    };
    let made = Rc::new(RefCell::new(None));
    let slot = made.clone();
    let owner = desk.clone();
    let form = desk.read(cx).menu_form.clone();
    let opened = cx.open_window(options, |window, cx| {
        let body = cx.new(|cx| Body::new(owner, draft, window, cx));
        mac::unframe(window, cx);
        mac::place(window, rest, false, cx);
        if keys {
            // Placed first, then grown around where the face was put.
            mac::open_menu(window, OPEN_SIZE, form, cx);
            window.activate_window();
            body.update(cx, |body, cx| body.focus(window, cx));
        }
        *slot.borrow_mut() = Some(body.clone());
        cx.new(|cx| Root::new(body, window, cx).bg(gpui_kit::transparent_black()))
    });
    let body = made.borrow_mut().take();
    desk.update(cx, |desk, cx| {
        desk.opening = false;
        match (opened, body) {
            (Ok(handle), Some(body)) => {
                desk.window = Some((handle.into(), body));
                desk.menu_grown = keys;
            }
            (Ok(_), None) => log::error!("theo: its window opened without a body"),
            (Err(error), _) => log::error!("theo: could not open its window: {error:#}"),
        }
        cx.notify();
    });
}

// The window -----------------------------------------------------------------

/// What the menu shows that changes its items. The reply is read as the
/// menu draws, so a reply arriving word by word does not rebuild it.
#[derive(Clone, Debug, PartialEq)]
struct MenuContent {
    title: String,
    live: bool,
    not_configured: bool,
    notice: Option<String>,
}

/// What the menu's items act on.
#[derive(Clone)]
struct MenuParts {
    desk: Entity<Desk>,
    driver: Entity<TheoremDriver>,
    ask: Entity<InputState>,
    reply: Entity<TextareaState>,
    /// The menu's dismissal is for an item that keeps it open, or for a
    /// right click that opens it again.
    keep_open: Rc<Cell<bool>>,
}

/// Theo's window: the face, and the menu above it while it is open.
pub struct Body {
    desk: Entity<Desk>,
    driver: Entity<TheoremDriver>,
    ask: Entity<InputState>,
    /// Theorem's reply, read-only, growing with what it says.
    reply: Entity<TextareaState>,
    /// What the reply was last given, so it is set only when that changes.
    reply_shown: Option<InputContent>,
    focus_handle: FocusHandle,
    title: String,
    /// The open menu, what it was built from, and its dismissal.
    menu: Option<(Entity<PopupMenu>, MenuContent, Subscription)>,
    /// Where the menu was drawn last, in window points.
    menu_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// See `MenuParts::keep_open`.
    keep_open: Rc<Cell<bool>>,
    /// Whether the window was active while the menu was open.
    was_active: bool,
    /// Where a press on the face began, until it is a click or a drag.
    pressed: Option<gpui_kit::Point<gpui_kit::Pixels>>,
    /// The person is dragging Theo somewhere.
    dragging: bool,
    _subscriptions: Vec<Subscription>,
}

impl Body {
    fn new(desk: Entity<Desk>, draft: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ask = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("Ask Theo");
            // A remounted menu starts from the draft the driver kept.
            state.set_value(draft, window, cx);
            state
        });
        let reply = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, REPLY_ROWS));
        let subscriptions = vec![
            cx.observe(&desk, |_, _, cx| cx.notify()),
            cx.subscribe_in(&ask, window, |body, _, event, window, cx| {
                body.ask_event(event, window, cx)
            }),
            // Choosing another application closes the menu, as a menu does.
            cx.observe_window_activation(window, |body, window, cx| {
                let active = window.is_window_active();
                if active {
                    body.was_active = true;
                } else if body.was_active && body.menu.is_some() {
                    body.was_active = false;
                    body.desk.update(cx, |desk, cx| desk.close_composer(cx));
                }
            }),
        ];
        Self {
            driver: desk.read(cx).driver.clone(),
            desk,
            ask,
            reply,
            reply_shown: None,
            focus_handle: cx.focus_handle(),
            title: String::new(),
            menu: None,
            menu_bounds: Rc::new(Cell::new(None)),
            keep_open: Rc::new(Cell::new(false)),
            was_active: false,
            pressed: None,
            dragging: false,
            _subscriptions: subscriptions,
        }
    }

    fn focus(&self, window: &mut Window, cx: &mut App) {
        let handle = self.ask.focus_handle(cx);
        window.focus(&handle, cx);
    }

    fn ask_event(&mut self, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::Change => {
                let text = self.ask.read(cx).value().to_string();
                self.driver.update(cx, |driver, cx| {
                    driver.composer_intent(ComposerIntent::DraftChanged { text }, cx)
                });
            }
            InputEvent::PressEnter { shift: false, .. } => self.submit(window, cx),
            _ => {}
        }
    }

    /// Sends what was typed to the selected agent, as the Agent place's
    /// composer does, when the composer says it can be sent.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.ask.read(cx).value().to_string();
        let driver = self.driver.clone();
        let composer = &driver.read(cx).projection().composer;
        if !composer.affordance(&text).is_send() {
            return;
        }
        let quote = composer.quote.clone();
        self.ask.update(cx, |ask, cx| ask.set_value("", window, cx));
        driver.update(cx, |driver, cx| {
            driver.composer_intent(
                ComposerIntent::Submit {
                    text,
                    quote,
                    steer: false,
                    context: Vec::new(),
                },
                cx,
            )
        });
    }

    fn menu_content(&self, cx: &App) -> MenuContent {
        let desk = self.desk.read(cx);
        let driver = desk.driver.read(cx);
        let projection = driver.projection();
        MenuContent {
            title: match projection.thread.title.as_str() {
                "" => "New conversation".to_owned(),
                title => title.to_owned(),
            },
            live: projection.thread.run_state.is_live(),
            not_configured: driver.connection() == Connection::NotConfigured,
            notice: projection
                .notice
                .clone()
                .or_else(|| desk.unheard.first().cloned()),
        }
    }

    fn parts(&self) -> MenuParts {
        MenuParts {
            desk: self.desk.clone(),
            driver: self.driver.clone(),
            ask: self.ask.clone(),
            reply: self.reply.clone(),
            keep_open: self.keep_open.clone(),
        }
    }

    /// Opens the menu, or builds it again when what it shows has changed.
    fn sync_menu(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !open {
            if self.menu.take().is_some() {
                self.menu_bounds.set(None);
            }
            return;
        }
        let content = self.menu_content(cx);
        let parts = self.parts();
        if let Some((menu, built, _)) = &mut self.menu {
            if *built != content {
                *built = content.clone();
                menu.update(cx, |menu, cx| {
                    menu.rebuild(window, cx, |menu, _, _| parts.fill(menu, &content))
                });
            }
            return;
        }
        let menu = PopupMenu::build(window, cx, |menu, _, _| parts.fill(menu, &content));
        let dismissed = cx.subscribe_in(&menu, window, |_, _, _: &DismissEvent, window, cx| {
            // Settled after the click that dismissed it: an item or a right
            // click on the face may keep it open.
            cx.defer_in(window, |body, window, cx| body.dismissed(window, cx));
        });
        self.menu = Some((menu, content, dismissed));
        self.focus(window, cx);
    }

    /// Gives the reply Theorem's latest message, when that has changed.
    fn sync_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let driver = self.driver.read(cx);
        let thread = &driver.projection().thread;
        let latest = thread
            .messages
            .iter()
            .rev()
            .find(|message| message.actor == ThreadActor::Theorem);
        let content = latest
            .and_then(|message| reply_content(&message.parts))
            .unwrap_or_else(|| {
                InputContent::new(if driver.connection() == Connection::NotConfigured {
                    "Theorem isn't set up on this Mac."
                } else {
                    "Say what you need."
                })
            });
        if self.reply_shown.as_ref() == Some(&content) {
            return;
        }
        self.reply_shown = Some(content.clone());
        self.reply
            .update(cx, |reply, cx| reply.set_value(content, window, cx));
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.keep_open.replace(false) {
            if self.menu.is_some() {
                self.focus(window, cx);
            }
            return;
        }
        self.desk.update(cx, |desk, cx| desk.close_composer(cx));
    }
}

impl MenuParts {
    /// The menu's items for `content`: the conversation, Theorem's reply,
    /// a notice if there is one, the field, then what can be done.
    fn fill(&self, menu: PopupMenu, content: &MenuContent) -> PopupMenu {
        let driver = self.driver.clone();
        let mut menu = menu
            .min_w(px(MENU_WIDTH))
            .max_w(px(MENU_WIDTH))
            .label(content.title.clone())
            .item(
                // Disabled as a menu item, so selecting in the reply does
                // not close the menu.
                PopupMenuItem::element({
                    let reply = self.reply.clone();
                    move |_, _| {
                        div().flex_1().min_w_0().child(
                            Textarea::new(&reply)
                                .readonly(true)
                                .bordered(false)
                                .appearance(false)
                                .token(|token, _, _| {
                                    InputToken::new(token).icon(token_icon(token.token().id()))
                                }),
                        )
                    }
                })
                .disabled(true),
            );
        if let Some(notice) = content.notice.clone() {
            menu = menu.item(
                PopupMenuItem::element(move |_, cx| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(notice.clone())
                })
                .disabled(true),
            );
        }
        let ask = self.ask.clone();
        menu = menu.separator().item(
            // Disabled as a menu item, so a click in the field does not
            // close the menu; the field itself takes the keys.
            PopupMenuItem::element(move |_, _| div().flex_1().py_1().child(Input::new(&ask)))
                .disabled(true),
        );
        menu = menu.separator();
        if !content.not_configured {
            // Theo shares the Agent place's selection, which may be any
            // conversation in the account; a fresh one is always a click away.
            let keep = self.keep_open.clone();
            let driver = driver.clone();
            menu = menu.item(PopupMenuItem::new("New Conversation").on_click(
                move |_, _, cx| {
                    keep.set(true);
                    driver.update(cx, |driver, cx| driver.new_conversation(cx));
                },
            ));
        }
        if content.live {
            let desk = self.desk.clone();
            let keep = self.keep_open.clone();
            menu = menu.item(PopupMenuItem::new("Stop").on_click(move |_, _, cx| {
                keep.set(true);
                desk.update(cx, |desk, cx| desk.stop(cx));
            }));
        }
        if content.not_configured {
            let keep = self.keep_open.clone();
            menu = menu.item(PopupMenuItem::new("Use the Demo Authority").on_click(
                move |_, _, cx| {
                    keep.set(true);
                    driver.update(cx, |driver, cx| driver.use_demo(cx));
                },
            ));
        }
        menu
    }
}

impl Render for Body {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu_bounds = self.menu_bounds.get();
        let (frame, words, open) = self.desk.update(cx, |desk, cx| {
            (
                desk.frame(window, menu_bounds, cx),
                desk.words(),
                desk.companion.composer_open(),
            )
        });
        self.sync_reply(window, cx);
        self.sync_menu(open, window, cx);
        // Assistive technology reads the window's title: what Theo is doing.
        let title = format!("Theo: {words}");
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        window.request_animation_frame();
        // A drag is AppKit's until the button comes up; then Theo rests there.
        if self.dragging && !mac::left_button_down() {
            self.dragging = false;
            self.desk.update(cx, |desk, _| desk.rest_here(window));
        }
        let measured = self.menu_bounds.clone();
        // The menu shows once the window has grown to hold it, under the face
        // or over it as the window was laid out; the face never moves.
        let form = self.desk.read(cx).menu_form.get();
        let menu = form.and(self.menu.as_ref()).map(|(menu, _, _)| {
            h_flex().w_full().justify_center().child(
                div().relative().child(menu.clone()).child(
                    canvas(
                        move |bounds, _, _| measured.set(Some(bounds)),
                        |_, _, _, _| {},
                    )
                    // Pinned to the menu's corner: left at its static
                    // place it would sit after the menu, a menu's height off.
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                ),
            )
        });
        let face = div()
            .pl(px(form.map_or(0.0, |form| form.face_x.max(0.0)) as f32))
            .child(
                div()
                    .id("theo-body")
                    .w(px(BODY_SIZE.0 as f32))
                    .h(px(BODY_SIZE.1 as f32))
                    .child(body::body(frame))
                    // A right click on the face opens the menu to talk in.
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|body, _: &gpui_kit::MouseDownEvent, _, cx| {
                            // An open menu is dismissed by this very click
                            // before it reaches the face: keep it open.
                            if body.menu.is_some() {
                                body.keep_open.set(true);
                            }
                            body.desk.update(cx, |desk, cx| desk.talk(cx));
                        }),
                    )
                    // Dragging the face moves Theo, and where it is let go is
                    // where it rests.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|body, event: &gpui_kit::MouseDownEvent, _, _| {
                            body.pressed = Some(event.position);
                        }),
                    )
                    .on_mouse_move(cx.listener(
                        |body, event: &gpui_kit::MouseMoveEvent, window, _| {
                            let Some(from) = body.pressed else {
                                return;
                            };
                            let moved = event.position - from;
                            if event.dragging()
                                && f32::from(moved.x).hypot(f32::from(moved.y)) > 3.0
                            {
                                body.pressed = None;
                                body.dragging = true;
                                window.start_window_move();
                            }
                        },
                    ))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|body, _: &gpui_kit::MouseUpEvent, _, _| {
                            body.pressed = None;
                        }),
                    ),
            );
        v_flex()
            .id("theo")
            .track_focus(&self.focus_handle)
            .size_full()
            .on_action(cx.listener(|body, _: &Escape, _, cx| {
                body.desk.update(cx, |desk, cx| desk.close_composer(cx))
            }))
            .map(|column| match form {
                None => column.items_center().justify_center().child(face),
                Some(form) if form.side == mac::Side::Below => column
                    .child(face)
                    .when_some(menu, |column, menu| column.child(menu.pt_1())),
                Some(_) => column
                    .justify_end()
                    .when_some(menu, |column, menu| column.child(menu.pb_1()))
                    .child(face),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> ThreadPart {
        ThreadPart::Text {
            part_id: "p".to_owned(),
            text: text.to_owned(),
            status: Default::default(),
        }
    }

    #[test]
    fn a_reply_reads_its_words_and_tokens_in_order() {
        let content = reply_content(&[
            words("Two and two make four."),
            ThreadPart::Citation {
                part_id: "c".to_owned(),
                source_id: "s1".to_owned(),
                label: "Peano\narithmetic".to_owned(),
            },
            ThreadPart::Artifact {
                part_id: "a".to_owned(),
                artifact_id: "art".to_owned(),
                artifact_revision: "1".to_owned(),
                artifact_kind: "table".to_owned(),
                renderer_id: "r".to_owned(),
                fallback_text: "\n  Sums  \nrows".to_owned(),
            },
            words("That is all."),
        ])
        .expect("a reply");
        assert_eq!(
            content.text().as_ref(),
            "Two and two make four. Peano arithmetic Sums\nThat is all."
        );
        let tokens = content
            .tokens()
            .iter()
            .map(|span| {
                (
                    span.token().id().to_string(),
                    content.text()[span.range()].to_owned(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            tokens,
            [
                ("citation:s1".to_owned(), "Peano arithmetic".to_owned()),
                ("artifact:art".to_owned(), "Sums".to_owned()),
            ]
        );
    }

    #[test]
    fn a_token_without_a_name_is_still_named() {
        let content = reply_content(&[ThreadPart::Attachment {
            part_id: "f".to_owned(),
            name: "\t".to_owned(),
            media_type: "text/plain".to_owned(),
            state: "ready".to_owned(),
        }])
        .expect("a reply");
        assert_eq!(content.text().as_ref(), "Attachment");
        assert_eq!(content.tokens().len(), 1);
    }

    #[test]
    fn a_reply_with_nothing_to_read_is_none() {
        assert!(reply_content(&[words("  ")]).is_none());
        assert!(reply_content(&[]).is_none());
    }
}
