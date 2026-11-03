//! The Agent place's view: the places bump on the window's leading edge, the
//! navigator on the window's frame beside it, and the work card, cut around
//! the composer (`docs/plans/theorem-workbench.md`, Layout).
//!
//! The driver owns every piece of conversation state; this view owns only
//! what drawing it needs: the navigator's width, which activity rows are open,
//! the composer's field, the transcript's scroll, the role the starters are
//! for and where the places bump leads out. `navigator.rs`, `transcript.rs`,
//! `starters.rs` and `composer.rs` draw its parts.
//!
//! SOURCING: GPUI Kit's `MessageScroller`, `Textarea` and `resize_handle`;
//! Theorem's chat host through `TheoremDriver`.

use std::collections::HashSet;

use gpui_kit::component::input::{Escape, InputEvent, TextareaState};
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{
    AppContext as _, Context, DragMoveEvent, Empty, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, Subscription,
    Window, actions, div, px,
};
use theorem_surface_contracts::intent::{ComposerIntent, ThreadIntent};
use theorem_surface_contracts::thread::ThreadRunState;

use super::driver::TheoremDriver;
use super::saved;
use super::starters::Role;
use crate::bump;
use crate::places::{Exit, Place, PlacesBump};

actions!(
    theorem,
    [
        /// Start a conversation in the selected project.
        NewConversation,
        /// Stop the run that is streaming.
        StopRun,
        /// Open the conversation above the selected one.
        PreviousConversation,
        /// Open the conversation below the selected one.
        NextConversation,
    ]
);

/// The key context the workbench's bindings belong to.
pub const CONTEXT: &str = "TheoremWorkbench";

/// The navigator's width, in rems: its default and the range it is dragged in.
pub(super) const NAVIGATOR_WIDTH: f32 = 15.;
const NAVIGATOR_MIN: f32 = 12.5;
const NAVIGATOR_MAX: f32 = 22.;

pub struct Workbench {
    driver: Entity<TheoremDriver>,
    pub(super) composer: Entity<TextareaState>,
    pub(super) transcript: Entity<MessageScrollerState>,
    focus_handle: FocusHandle,
    /// In rems.
    pub(super) navigator_width: f32,
    /// Activity groups and rows the reader opened, by their part's id.
    pub(super) expanded: HashSet<String>,
    /// Whose starters the empty conversation offers.
    pub(super) role: Role,
    /// Where the places bump's way out goes, as the browser last said.
    pub(super) exit: Exit,
    shown: Shown,
    board: Option<bump::Slot>,
    _subscriptions: [Subscription; 4],
}

/// What the transcript and the composer last took from the projection, so
/// a new projection changes only what changed.
#[derive(Default)]
struct Shown {
    thread_id: String,
    messages: usize,
    draft_revision: u64,
    focus_revision: u64,
}

impl Workbench {
    /// The workbench over this window's driver, which connects to the
    /// configured authority, if there is one.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let driver = cx.new(TheoremDriver::new);
        Self::with_driver(driver, window, cx)
    }

    pub fn with_driver(
        driver: Entity<TheoremDriver>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
                .submit_on_enter(true)
                .placeholder("Ask Theorem")
        });
        let transcript = cx.new(|cx| MessageScrollerState::new(0, cx));
        // The composer widens while it is in use, so its focus redraws it.
        let field = composer.focus_handle(cx);
        let subscriptions = [
            cx.observe_in(&driver, window, |this, _, window, cx| this.sync(window, cx)),
            cx.subscribe_in(&composer, window, |this, _, event, window, cx| {
                this.composer_event(event, window, cx)
            }),
            cx.on_focus_in(&field, window, |_, _, cx| cx.notify()),
            cx.on_focus_out(&field, window, |_, _, _, cx| cx.notify()),
        ];
        let mut this = Self {
            driver,
            composer,
            transcript,
            focus_handle: cx.focus_handle(),
            navigator_width: saved::navigator_width(cx).map_or(NAVIGATOR_WIDTH, |width| {
                width.clamp(NAVIGATOR_MIN, NAVIGATOR_MAX)
            }),
            expanded: HashSet::new(),
            role: Role::default(),
            exit: Exit::Tabs,
            shown: Shown::default(),
            board: None,
            _subscriptions: subscriptions,
        };
        this.sync(window, cx);
        this
    }

    pub fn set_board(&mut self, board: bool, cx: &mut Context<Self>) {
        if board != self.board.is_some() {
            self.board = board.then(bump::Slot::default);
            cx.notify();
        }
    }
    pub fn board_slot(&self) -> Option<bump::Slot> {
        self.board.clone()
    }
    pub fn attach_workspace(&mut self, text: String, cx: &mut Context<Self>) {
        let id = format!("workspace-{}", frame_profile::now());
        self.driver.update(cx, |driver, cx| {
            driver.composer_intent(
                ComposerIntent::AddPastedFile {
                    attachment_id: id,
                    name: "Selected workspace sources.md".into(),
                    media_type: "text/markdown".into(),
                    text,
                },
                cx,
            )
        });
    }

    pub fn driver(&self) -> &Entity<TheoremDriver> {
        &self.driver
    }

    /// Where the places bump's way out goes: what the place stands over.
    pub fn set_exit(&mut self, exit: Exit, cx: &mut Context<Self>) {
        if self.exit != exit {
            self.exit = exit;
            cx.notify();
        }
    }

    /// Put the keyboard in the composer, as opening the place does. While
    /// the card draws no composer it goes to the workbench, which hands it on
    /// when the runtime asks for the composer after hydrating.
    pub fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.body(cx).has_composer() {
            self.composer
                .update(cx, |composer, cx| composer.focus(window, cx));
        } else {
            window.focus(&self.focus_handle, cx);
        }
    }

    /// Bring the transcript and the composer up to the driver's projection.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let projection = self.driver.read(cx).projection();
        let thread = &projection.thread;
        let count = thread.messages.len();
        let switched = thread.thread_id != self.shown.thread_id;
        let thread_id = switched.then(|| thread.thread_id.clone());
        let composer = &projection.composer;
        let draft =
            (composer.draft_revision != self.shown.draft_revision).then(|| composer.draft.clone());
        let (draft_revision, focus_revision) = (composer.draft_revision, composer.focus_revision);

        if let Some(thread_id) = thread_id {
            self.shown.thread_id = thread_id;
            self.expanded.clear();
            self.transcript
                .update(cx, |state, cx| state.reset(count, cx));
        } else if count < self.shown.messages {
            self.transcript
                .update(cx, |state, cx| state.reset(count, cx));
        } else if count > self.shown.messages {
            let added = count - self.shown.messages;
            self.transcript.update(cx, |state, cx| {
                state.append(added, cx);
            });
        }
        self.shown.messages = count;

        if let Some(draft) = draft {
            self.shown.draft_revision = draft_revision;
            self.composer
                .update(cx, |composer, cx| composer.set_value(draft, window, cx));
        }
        // The runtime asks for the composer after hydrating and after a new
        // conversation. Only a reader already in the workbench gets it; one
        // typing an address elsewhere keeps their place.
        if focus_revision != self.shown.focus_revision {
            self.shown.focus_revision = focus_revision;
            if self.focus_handle.contains_focused(window, cx) {
                self.focus_composer(window, cx);
            }
        }
        cx.notify();
    }

    fn composer_event(&mut self, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::Change => {
                let text = self.composer.read(cx).value().to_string();
                self.driver.update(cx, |driver, cx| {
                    driver.composer_intent(ComposerIntent::DraftChanged { text }, cx)
                });
            }
            InputEvent::PressEnter { shift: false, .. } => self.submit(window, cx),
            _ => {}
        }
    }

    /// Send the composer's text, when the composer says it can be sent.
    /// When it cannot, the reason is drawn beneath the field.
    pub(super) fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().to_string();
        let composer = &self.driver.read(cx).projection().composer;
        if !composer.affordance(&text).is_send() {
            return;
        }
        let quote = composer.quote.clone();
        // The field empties as the message leaves, before the host answers,
        // which is the runtime's contract with its composer: a host that
        // refuses the send puts the text back through a new draft revision.
        self.composer
            .update(cx, |composer, cx| composer.set_value("", window, cx));
        self.driver.update(cx, |driver, cx| {
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

    pub(super) fn thread_intent(&mut self, intent: ThreadIntent, cx: &mut Context<Self>) {
        self.driver
            .update(cx, |driver, cx| driver.thread_intent(intent, cx));
    }

    pub(super) fn composer_intent(&mut self, intent: ComposerIntent, cx: &mut Context<Self>) {
        self.driver
            .update(cx, |driver, cx| driver.composer_intent(intent, cx));
    }

    pub(super) fn can_stop(&self, cx: &gpui_kit::App) -> bool {
        self.driver.read(cx).projection().composer.can_cancel()
    }

    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        self.driver.update(cx, |driver, cx| driver.stop(cx));
    }

    pub(super) fn new_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.driver
            .update(cx, |driver, cx| driver.new_conversation(cx));
        self.focus_composer(window, cx);
    }

    pub(super) fn select_thread(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        self.driver
            .update(cx, |driver, cx| driver.select_thread(thread_id, cx));
    }

    pub(super) fn select_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        self.driver
            .update(cx, |driver, cx| driver.select_project(project_id, cx));
    }

    /// Open or close an activity group or row, and let the transcript
    /// measure the row it is in again.
    pub(super) fn toggle(&mut self, key: String, row: usize, cx: &mut Context<Self>) {
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        self.transcript.update(cx, |state, cx| {
            state.remeasure_items(row..row + 1, cx);
        });
        cx.notify();
    }

    pub(super) fn set_navigator_width(&mut self, rems: f32, cx: &mut Context<Self>) {
        let rems = rems.clamp(NAVIGATOR_MIN, NAVIGATOR_MAX);
        if rems != self.navigator_width {
            self.navigator_width = rems;
            saved::set_navigator_width(rems, cx);
            cx.notify();
        }
    }

    /// The conversation `step` rows from the selected one, in the order the
    /// navigator lists them.
    fn neighbor(&self, step: isize, cx: &gpui_kit::App) -> Option<String> {
        let projection = self.driver.read(cx).projection();
        let rows = super::navigator::by_recency(projection);
        let selected = projection.selected_thread_id.as_deref();
        let index = match rows
            .iter()
            .position(|thread| Some(thread.thread_id.as_str()) == selected)
        {
            Some(index) => index.checked_add_signed(step)?,
            None if step > 0 => 0,
            None => rows.len().checked_sub(1)?,
        };
        rows.get(index).map(|thread| thread.thread_id.clone())
    }

    fn on_new_conversation(
        &mut self,
        _: &NewConversation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_conversation(window, cx);
    }

    fn on_stop(&mut self, _: &StopRun, _: &mut Window, cx: &mut Context<Self>) {
        if self.can_stop(cx) {
            self.stop(cx);
        } else {
            cx.propagate();
        }
    }

    /// Escape in the composer reaches here when the field has nothing of its
    /// own to dismiss.
    fn on_escape(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        if self.can_stop(cx) {
            self.stop(cx);
        } else {
            cx.propagate();
        }
    }

    fn on_previous(&mut self, _: &PreviousConversation, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(thread_id) = self.neighbor(-1, cx) {
            self.select_thread(&thread_id, cx);
        }
    }

    fn on_next(&mut self, _: &NextConversation, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(thread_id) = self.neighbor(1, cx) {
            self.select_thread(&thread_id, cx);
        }
    }

    /// The run state's word, when it is one the reader should see.
    pub(super) fn run_state_word(state: ThreadRunState) -> Option<&'static str> {
        match state {
            ThreadRunState::Idle => None,
            ThreadRunState::Streaming => Some("Running"),
            ThreadRunState::RequiresApproval => Some("Needs approval"),
            ThreadRunState::Failed => Some("Failed"),
            ThreadRunState::Aborted => Some("Stopped"),
            ThreadRunState::Interrupted => Some("Interrupted"),
        }
    }
}

impl Focusable for Workbench {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// The navigator's edge, while it is dragged. It draws nothing: the edge
/// itself shows the drag.
#[derive(Clone)]
pub(super) struct ResizeNavigator;

impl Render for ResizeNavigator {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(slot) = self.board.clone() {
            use gpui_kit::component::{
                Sizable as _,
                button::{Button, ButtonVariants as _},
                v_flex,
            };
            return v_flex()
                .id("theorem-board-composer")
                .debug_selector(|| "theorem-board-composer".into())
                .key_context(CONTEXT)
                .track_focus(&self.focus_handle)
                .on_action(cx.listener(Self::on_stop))
                .child(if self.body(cx).has_composer() {
                    self.render_composer(slot, window, cx)
                } else {
                    div()
                        .relative()
                        .p_3()
                        .child(slot.recorder())
                        .child(
                            Button::new("board-open-agent")
                                .small()
                                .ghost()
                                .label("Connect Theorem to ask about this workspace")
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(
                                        Box::new(crate::actions::OpenPlace {
                                            place: Place::Agent,
                                        }),
                                        cx,
                                    )
                                }),
                        )
                        .into_any_element()
                })
                .into_any_element();
        }
        h_flex()
            .id("theorem-workbench")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_new_conversation))
            .on_action(cx.listener(Self::on_stop))
            .on_action(cx.listener(Self::on_escape))
            .on_action(cx.listener(Self::on_previous))
            .on_action(cx.listener(Self::on_next))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<ResizeNavigator>, window, cx| {
                    let width =
                        event.event.position.x - event.bounds.left() - px(PlacesBump::WIDTH);
                    this.set_navigator_width(f32::from(width) / f32::from(window.rem_size()), cx);
                },
            ))
            .relative()
            .size_full()
            .min_h_0()
            .items_stretch()
            .text_color(cx.theme().foreground)
            .child(self.render_navigator(window, cx))
            .child(self.render_card(window, cx))
            // On the window's own edge, where the navigator leaves it room,
            // rising out of the frame as the composer does at the foot.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .flex()
                    .items_center()
                    .child(
                        PlacesBump::new(Some(Place::Agent))
                            .exit(self.exit)
                            .fill(bump::leading_fill(cx)),
                    ),
            )
            .into_any_element()
    }
}
