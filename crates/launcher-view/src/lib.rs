//! Theo's launcher as a GPUI view.
//!
//! One window, one field. What is typed is looked up among applications,
//! files, commands and conversations, and the selected row says what Enter
//! will do: open, run, resume, or ask Theo. Asking turns the same window
//! into the conversation; Escape turns it back.
//!
//! What this crate owns is what is drawn and which key does what. It keeps
//! a [`launcher::Session`] (the rows and the selection) and runs the asked
//! lanes' searches off the main thread, cancelling one as soon as the query
//! it was for has changed. It does not open files, send messages or know
//! where a conversation is kept: it emits an [`Event`] and the host acts.
//!
//! The host hands in:
//!
//! - standing items per lane ([`Launcher::set_standing`]);
//! - a [`launcher::Source`] per asked lane ([`Launcher::add_source`]);
//! - the conversation as it stands ([`Launcher::set_talk`]).
//!
//! and keeps what should outlive the window ([`Launcher::keep`]): the query,
//! the rows, the selection and an unsent draft, handed back to
//! [`Launcher::new`] when the window is shown again.
//!
//! SOURCING: GPUI Kit's `Input`, `Button`, `Icon` and theme; the key context
//! and bindings follow GPUI Kit's own `Command` component.

extern crate gpui_kit as gpui;

mod mark;
mod preview;
mod rows;
mod talk;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window,
    actions, div, px,
};
use launcher::{Cancel, Chosen, Escaped, Item, ItemId, Lane, Mode, Scope, Session, Source, Verb};

pub use talk::{Attachment, Talk, Turn, Who};

actions!(
    theo_launcher,
    [
        /// Select the row above, around the top.
        SelectUp,
        /// Select the row below, around the bottom.
        SelectDown,
        /// Narrow to the next kind of result.
        NextScope,
        /// Narrow to the previous kind of result.
        PreviousScope,
        /// Step back once: out of the conversation, then out of what was
        /// typed, then out of the launcher.
        StepBack,
        /// Attach the selected file to the conversation and talk about it.
        AskAbout,
        /// Show the selected item in Finder.
        Reveal,
        /// Copy the selected item's path.
        CopyPath,
        /// Start a new conversation.
        NewConversation,
        /// Carry the conversation on in the full workspace.
        OpenWorkspace,
        /// Stop what Theo is doing.
        StopTheo,
    ]
);

/// The key context of the launcher's window.
pub const CONTEXT: &str = "TheoLauncher";
/// The same, with the field focused. A binding here outranks the field's
/// own for the same key, so Tab narrows instead of indenting.
const FIELD: &str = "TheoLauncher > Input";

/// How long a query rests before an asked lane is troubled for it, so a
/// word typed quickly starts one search and not one per letter.
const REST: Duration = Duration::from_millis(90);

/// The launcher's size, for the host's window.
pub const SIZE: (f32, f32) = (780., 500.);

/// Binds the launcher's keys. Call once, after `gpui_kit::init`.
pub fn init(cx: &mut App) {
    let mut keys = Vec::new();
    for context in [CONTEXT, FIELD] {
        let context = Some(context);
        keys.extend([
            KeyBinding::new("up", SelectUp, context),
            KeyBinding::new("down", SelectDown, context),
            KeyBinding::new("tab", NextScope, context),
            KeyBinding::new("shift-tab", PreviousScope, context),
            KeyBinding::new("escape", StepBack, context),
            KeyBinding::new("secondary-enter", AskAbout, context),
            KeyBinding::new("secondary-r", Reveal, context),
            KeyBinding::new("secondary-shift-c", CopyPath, context),
            KeyBinding::new("secondary-n", NewConversation, context),
            KeyBinding::new("secondary-o", OpenWorkspace, context),
            KeyBinding::new("secondary-.", StopTheo, context),
        ]);
    }
    cx.bind_keys(keys);
}

/// What the person asked for. The host does it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Do this with that: open the file, run the command, resume the
    /// conversation, attach the file. Asking Theo what was typed is `Send`.
    Chosen(Chosen),
    /// Send this to the conversation shown, or to a new one if none is.
    Send(String),
    Stop,
    NewConversation,
    /// Take this attachment off the message being written, by its id.
    RemoveAttachment(String),
    /// Carry the same conversation on in the full workspace.
    OpenWorkspace,
    /// Close the window. What was typed and found is the host's to keep.
    Dismiss,
}

/// What outlives the window: the rows as they stood and an unsent draft.
#[derive(Clone, Debug, Default)]
pub struct Kept {
    pub session: Session,
    pub draft: String,
}

/// One search of an asked lane, for one query.
struct Search {
    cancel: Cancel,
    _task: Task<()>,
}

pub struct Launcher {
    session: Session,
    talk: Talk,
    /// The message being written while the field is for talking. While it
    /// is for finding, the field holds the query and this waits.
    draft: String,
    field: Entity<InputState>,
    focus: FocusHandle,
    sources: Vec<(Lane, Arc<dyn Source>)>,
    searches: Vec<Search>,
    rows: ScrollHandle,
    turns: ScrollHandle,
    _field: Subscription,
}

impl EventEmitter<Event> for Launcher {}

impl Focusable for Launcher {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.read(cx).focus_handle(cx)
    }
}

impl Launcher {
    pub fn new(kept: Kept, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let Kept { session, draft } = kept;
        let shown = match session.mode() {
            Mode::Find => session.query().to_owned(),
            Mode::Talk => draft.clone(),
        };
        let placeholder = placeholder(session.mode());
        let field = cx.new(|cx| {
            let mut field = InputState::new(window, cx).placeholder(placeholder);
            if !shown.is_empty() {
                field.set_value(shown, window, cx);
            }
            field
        });
        let _field = cx.subscribe_in(&field, window, Self::field_event);
        Self {
            session,
            talk: Talk::default(),
            draft,
            field,
            focus: cx.focus_handle(),
            sources: Vec::new(),
            searches: Vec::new(),
            rows: ScrollHandle::new(),
            turns: ScrollHandle::new(),
            _field,
        }
    }

    // What the host hands in ------------------------------------------------

    /// Replaces a lane's standing items.
    pub fn set_standing(&mut self, lane: Lane, items: Vec<Item>, cx: &mut Context<Self>) {
        self.session.set_standing(lane, items);
        cx.notify();
    }

    /// Gives `lane` a source to ask for each query. A query already in the
    /// field is asked of it now.
    pub fn add_source(&mut self, lane: Lane, source: Arc<dyn Source>, cx: &mut Context<Self>) {
        self.sources.retain(|(known, _)| *known != lane);
        self.sources.push((lane, source));
        self.session.expect(lane);
        self.search(cx);
    }

    /// Says how the conversation stands: its title, what was said, what is
    /// attached, and whether Theo is at work.
    pub fn set_talk(&mut self, talk: Talk, cx: &mut Context<Self>) {
        if talk == self.talk {
            return;
        }
        self.session.set_conversation(talk.title.clone());
        if talk.turns != self.talk.turns {
            self.turns.scroll_to_bottom();
        }
        self.talk = talk;
        cx.notify();
    }

    /// What should outlive the window. Searches still running are given up;
    /// their lanes are asked again when the window is shown.
    pub fn keep(&self) -> Kept {
        Kept {
            session: self.session.clone(),
            draft: self.draft.clone(),
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn talk(&self) -> &Talk {
        &self.talk
    }

    /// Puts the caret in the field.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.field.update(cx, |field, cx| field.focus(window, cx));
    }

    /// Turns the field over to talking, as choosing "Ask Theo" does. For a
    /// host that summons the launcher straight into the conversation.
    pub fn show_talk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.enter_talk(window, cx);
    }

    /// Hands back a message the host could not send after all, so it waits
    /// in the field instead of being lost. Whatever was typed since follows
    /// it.
    pub fn put_back(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let draft = match self.draft.trim() {
            "" => text.to_owned(),
            since => format!("{text} {since}"),
        };
        self.draft = draft.clone();
        if self.session.mode() == Mode::Talk {
            self.write_field(&draft, window, cx);
        }
        cx.notify();
    }

    // The field ---------------------------------------------------------------

    fn field_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => self.typed(cx),
            InputEvent::PressEnter { secondary, .. } => self.entered(*secondary, window, cx),
            _ => {}
        }
    }

    /// Reads the field. Writing the field from here lands in this too, and
    /// changes nothing: the text is already what the session or draft holds.
    fn typed(&mut self, cx: &mut Context<Self>) {
        let text = self.field.read(cx).value().to_string();
        match self.session.mode() {
            Mode::Talk => self.draft = text,
            Mode::Find => {
                if self.session.set_query(&text) {
                    self.rows.scroll_to_item(0);
                    self.search(cx);
                }
            }
        }
        cx.notify();
    }

    fn write_field(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = placeholder(self.session.mode());
        let text = text.to_owned();
        self.field.update(cx, |field, cx| {
            field.set_placeholder(placeholder, window, cx);
            field.set_value(text, window, cx);
            field.focus(window, cx);
        });
    }

    /// Gives up the searches for the last query and asks the lanes that
    /// have not answered this one.
    fn search(&mut self, cx: &mut Context<Self>) {
        for search in self.searches.drain(..) {
            search.cancel.cancel();
        }
        let generation = self.session.generation();
        let query = self.session.query().trim().to_owned();
        for lane in self.session.lanes_to_ask() {
            let Some(source) = self
                .sources
                .iter()
                .find(|(known, _)| *known == lane)
                .map(|(_, source)| source.clone())
            else {
                continue;
            };
            let cancel = Cancel::new();
            let finder = cancel.clone();
            let query = query.clone();
            let task = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(REST).await;
                if finder.is_cancelled() {
                    return;
                }
                let found = cx
                    .background_spawn(async move { source.find(&query, &finder) })
                    .await;
                // A refusal means the query moved on while this looked.
                let _ = this.update(cx, |this, cx| {
                    if this.session.deliver(lane, generation, found).is_ok() {
                        cx.notify();
                    }
                });
            });
            self.searches.push(Search {
                cancel,
                _task: task,
            });
        }
    }

    // What the keys do --------------------------------------------------------

    fn entered(&mut self, secondary: bool, window: &mut Window, cx: &mut Context<Self>) {
        match self.session.mode() {
            Mode::Talk => self.send(window, cx),
            Mode::Find => {
                let chosen = if secondary {
                    self.session.choose(Verb::AskAbout)
                } else {
                    self.session.confirm()
                };
                if let Some(chosen) = chosen {
                    self.act(chosen, window, cx);
                }
            }
        }
    }

    /// Does what was chosen, as far as the launcher itself goes: asking
    /// turns the window to the conversation; everything else is the host's.
    fn act(&mut self, chosen: Chosen, window: &mut Window, cx: &mut Context<Self>) {
        match chosen.verb {
            Verb::Ask => {
                let question = chosen.item.title.clone();
                // The question leaves the field for the conversation; what
                // it found is no longer what the window is about.
                self.session.set_query("");
                self.search(cx);
                if self.talk.refusing.is_some() {
                    // Not sent: it waits in the field, where it can be seen.
                    self.draft = question;
                    self.enter_talk(window, cx);
                } else {
                    self.draft.clear();
                    self.enter_talk(window, cx);
                    cx.emit(Event::Send(question));
                }
            }
            Verb::AskAbout | Verb::Resume => {
                cx.emit(Event::Chosen(chosen));
                self.enter_talk(window, cx);
            }
            _ => cx.emit(Event::Chosen(chosen)),
        }
    }

    fn enter_talk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session.talk();
        let draft = self.draft.clone();
        self.write_field(&draft, window, cx);
        self.turns.scroll_to_bottom();
        cx.notify();
    }

    fn leave_talk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session.find();
        let query = self.session.query().to_owned();
        self.write_field(&query, window, cx);
        cx.notify();
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.draft.trim().to_owned();
        if text.is_empty() || self.talk.refusing.is_some() {
            return;
        }
        self.draft.clear();
        self.write_field("", window, cx);
        cx.emit(Event::Send(text));
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.step(-1, cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.step(1, cx);
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        if self.session.mode() != Mode::Find {
            return;
        }
        self.session.move_selection(by);
        if let Some(at) = self.selected_child() {
            self.rows.scroll_to_item(at);
        }
        cx.notify();
    }

    /// Where the selected row sits among the list's children, headings
    /// counted, for scrolling it into view.
    fn selected_child(&self) -> Option<usize> {
        let selected = &self.session.selected()?.id;
        let mut at = 0;
        for section in self.session.sections() {
            at += 1;
            for item in &section.items {
                if &item.id == selected {
                    return Some(at);
                }
                at += 1;
            }
        }
        None
    }

    fn next_scope(&mut self, _: &NextScope, _: &mut Window, cx: &mut Context<Self>) {
        self.narrow(|session| session.next_scope(), cx);
    }

    fn previous_scope(&mut self, _: &PreviousScope, _: &mut Window, cx: &mut Context<Self>) {
        self.narrow(|session| session.previous_scope(), cx);
    }

    fn narrow(&mut self, change: impl FnOnce(&mut Session), cx: &mut Context<Self>) {
        if self.session.mode() != Mode::Find {
            return;
        }
        change(&mut self.session);
        // A lane that was out of sight for this query may be in it now.
        self.search(cx);
        self.rows.scroll_to_item(0);
        cx.notify();
    }

    fn step_back(&mut self, _: &StepBack, window: &mut Window, cx: &mut Context<Self>) {
        match self.session.escape() {
            Escaped::LeftTalk => {
                // `escape` already turned the session back to finding.
                self.leave_talk(window, cx);
            }
            Escaped::Cleared => {
                self.search(cx);
                self.write_field("", window, cx);
                cx.notify();
            }
            Escaped::Dismiss => cx.emit(Event::Dismiss),
        }
    }

    fn ask_about(&mut self, _: &AskAbout, window: &mut Window, cx: &mut Context<Self>) {
        self.choose(Verb::AskAbout, window, cx);
    }

    fn reveal(&mut self, _: &Reveal, window: &mut Window, cx: &mut Context<Self>) {
        self.choose(Verb::Reveal, window, cx);
    }

    fn copy_path(&mut self, _: &CopyPath, window: &mut Window, cx: &mut Context<Self>) {
        self.choose(Verb::CopyPath, window, cx);
    }

    /// Does `verb` with the selected row, when it can be done with it.
    fn choose(&mut self, verb: Verb, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.mode() != Mode::Find {
            return;
        }
        if let Some(chosen) = self.session.choose(verb) {
            self.act(chosen, window, cx);
        }
    }

    fn new_conversation(&mut self, _: &NewConversation, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(Event::NewConversation);
    }

    fn open_workspace(&mut self, _: &OpenWorkspace, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(Event::OpenWorkspace);
    }

    fn stop(&mut self, _: &StopTheo, _: &mut Window, cx: &mut Context<Self>) {
        if self.talk.working {
            cx.emit(Event::Stop);
        }
    }

    /// A row was clicked: once selects it, twice does what Enter does.
    fn clicked(&mut self, id: &ItemId, twice: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.session.select(id);
        if twice && let Some(chosen) = self.session.confirm() {
            self.act(chosen, window, cx);
        }
        self.focus(window, cx);
        cx.notify();
    }

    // What is drawn -----------------------------------------------------------

    fn header(&self, cx: &Context<Self>) -> Div {
        let talking = self.session.mode() == Mode::Talk;
        h_flex()
            .flex_none()
            .h(px(60.))
            .px(px(18.))
            .gap(px(12.))
            .child(mark::mark(self.talk.working, cx))
            .when(talking, |header| header.child(pill("Ask Theo", true, cx)))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.field)
                        .appearance(false)
                        .bordered(false)
                        .focus_bordered(false)
                        .text_size(px(19.)),
                ),
            )
    }

    fn scopes(&self, cx: &mut Context<Self>) -> Div {
        let current = self.session.scope();
        h_flex()
            .flex_none()
            .px(px(18.))
            .pb(px(12.))
            .gap(px(6.))
            .children(Scope::EVERY.into_iter().map(|scope| {
                pill(scope.label(), scope == current, cx)
                    .id(SharedString::from(format!("scope-{}", scope.label())))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.narrow(|session| session.set_scope(scope), cx);
                        this.focus(window, cx);
                    }))
            }))
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let border = theme.border;
        let talking = self.session.mode() == Mode::Talk;
        let hints: Vec<(&str, String)> = if talking {
            let mut hints = vec![("↵", "send".to_owned()), ("esc", "results".to_owned())];
            hints.push(("⌘O", "open in workspace".to_owned()));
            hints.push(("⌘N", "new conversation".to_owned()));
            hints
        } else {
            let mut hints = vec![("↑↓", "select".to_owned())];
            if let Some(chosen) = self.session.confirm() {
                hints.push(("↵", hint(chosen.verb).to_owned()));
            }
            hints.push(("⇥", "filter".to_owned()));
            if self.session.choose(Verb::AskAbout).is_some() {
                hints.push(("⌘↵", hint(Verb::AskAbout).to_owned()));
            }
            let leaves = if self.session.query().is_empty() {
                "close"
            } else {
                "clear"
            };
            hints.push(("esc", leaves.to_owned()));
            hints
        };
        h_flex()
            .flex_none()
            .h(px(36.))
            .px(px(18.))
            .gap(px(14.))
            .border_t_1()
            .border_color(border)
            .text_size(px(12.))
            .text_color(muted)
            .children(hints.into_iter().map(|(key, does)| {
                h_flex()
                    .gap(px(5.))
                    .child(keycap(key, cx))
                    .child(SharedString::from(does))
            }))
            .child(div().flex_1())
            .child(if talking {
                talk::status(self, cx)
            } else {
                rows::status(&self.session, cx)
            })
    }
}

impl Render for Launcher {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let talking = self.session.mode() == Mode::Talk;
        let (surface, ink, border) = {
            let theme = cx.theme();
            (theme.popover, theme.popover_foreground, theme.border)
        };
        v_flex()
            .id("theo-launcher")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .overflow_hidden()
            .rounded(px(14.))
            .border_1()
            .border_color(border)
            .bg(surface)
            .text_color(ink)
            .text_size(px(14.))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::next_scope))
            .on_action(cx.listener(Self::previous_scope))
            .on_action(cx.listener(Self::step_back))
            .on_action(cx.listener(Self::ask_about))
            .on_action(cx.listener(Self::reveal))
            .on_action(cx.listener(Self::copy_path))
            .on_action(cx.listener(Self::new_conversation))
            .on_action(cx.listener(Self::open_workspace))
            .on_action(cx.listener(Self::stop))
            .child(self.header(cx))
            .when(!talking, |column| column.child(self.scopes(cx)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(border)
                    .child(if talking {
                        talk::pane(self, cx)
                    } else {
                        h_flex()
                            .size_full()
                            .items_start()
                            .child(rows::list(self, cx))
                            .child(preview::pane(self, cx))
                    }),
            )
            .child(self.footer(cx))
    }
}

/// What a verb does, as the footer says it after its key.
fn hint(verb: Verb) -> &'static str {
    match verb {
        Verb::Open => "open",
        Verb::Reveal => "reveal in Finder",
        Verb::CopyPath => "copy path",
        Verb::CopyAddress => "copy address",
        Verb::AskAbout => "ask Theo about this",
        Verb::Resume => "resume",
        Verb::Run => "run",
        Verb::Ask => "ask Theo",
        Verb::Search => "search",
    }
}

fn placeholder(mode: Mode) -> &'static str {
    match mode {
        Mode::Find => "Search, ask, or run a command…",
        Mode::Talk => "Message Theo…",
    }
}

/// A rounded label: a scope, or the word that says the field is for
/// talking. The lit one is filled; the rest are outlined and quiet.
fn pill(label: &'static str, lit: bool, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex_none()
        .h(px(26.))
        .px(px(11.))
        .flex()
        .items_center()
        .rounded_full()
        .text_size(px(12.5))
        .border_1()
        .map(|pill| {
            if lit {
                pill.border_color(theme.foreground.opacity(0.14))
                    .bg(theme.foreground.opacity(0.14))
                    .text_color(theme.foreground)
            } else {
                pill.border_color(theme.border)
                    .text_color(theme.muted_foreground)
            }
        })
        .child(label)
}

/// A key, as a small cap.
fn keycap(key: &'static str, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex_none()
        .h(px(18.))
        .min_w(px(18.))
        .px(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.muted)
        .text_size(px(11.))
        .text_color(theme.foreground.opacity(0.8))
        .child(key)
}
