//! Theo's launcher on the desk: one window, called from any application, to
//! find something on this Mac or in Theorem, act on it, or ask Theo.
//!
//! The window is `launcher_view::Launcher`; what it finds and which row is
//! selected is `launcher::Session`. This file is the host between them and
//! the Mac: it opens and closes the window, hands in the applications, the
//! commands and Theorem's conversations, and does what a chosen row says.
//!
//! Whose data is whose:
//!
//! - **The conversation** is the driver's, the one Theo's face and the Agent
//!   place already share. The launcher shows it and sends to it; it keeps no
//!   copy. "Open in Workspace" is therefore the same conversation.
//! - **What was typed and found** is kept here between showings (`Kept`), so
//!   hiding the window loses neither the rows nor an unsent message. Hiding
//!   stops nothing: Stop is its own action, routed as Theo's is.
//! - **Applications** are read from the application folders off the main
//!   thread each time the launcher is shown. A few hundred rows, replaced
//!   whole.
//! - **Files** are asked of Spotlight per query by the view, and cancelled
//!   there when the query changes. Showing a file here adds nothing to
//!   Theorem: only "Ask Theo About This" attaches one, to one message.
//!
//! Not here: passages from Theorem's own file index and a search inside
//! conversations. Platinum's pinned Theorem transport has no route for
//! either, so neither lane is offered rather than drawn empty.
//!
//! The file is `summon.rs` so the `launcher` crate keeps its name in here.
//!
//! SOURCING: `mod.rs`'s own window (`open`, `hide`, `give_back`) for when a
//! window may be opened, closed and handed the keyboard.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::component::Root;
use gpui_kit::{
    Action, AnyWindowHandle, App, AppContext as _, Bounds, ClipboardItem, Context, Entity,
    Styled as _, Subscription, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
    actions, point, px, size,
};
use launcher::spotlight::Spotlight;
use launcher::{Chosen, Item, ItemId, Kind, Lane, Target, Verb, apps, words};
use launcher_view::{Attachment, Event, Kept, Launcher, SIZE, Talk, Turn, Who};
use theorem_surface_contracts::composer::{AttachmentState, SendAffordance, SendBlock};
use theorem_surface_contracts::intent::ComposerIntent;
use theorem_surface_contracts::thread::{ThreadActor, ThreadPart};
use theoremweb_agent_runtime::ChatThreadSummary;

use super::{Desk, mac};
use crate::actions::{
    FocusAddressBar, NewTab, OpenPlace, OpenTyped, ShowBookmarks, ShowDownloads, ShowHistory,
};
use crate::theorem::{Connection, TheoremDriver};

actions!(
    theo,
    [
        /// Show Theo's launcher over the application in front, or hide it.
        SummonTheo,
        /// Whether Platinum stays running, for Theo, once its window is closed.
        KeepRunning,
    ]
);

/// The most of a conversation the launcher draws: its latest turns. A reply
/// arriving word by word rebuilds this much and no more; the whole
/// conversation is in the workspace.
const TURNS: usize = 40;
/// The conversations shown before anything is typed.
const RECENT: usize = 5;

/// What the desk keeps for the launcher.
#[derive(Default)]
pub(super) struct Summoned {
    window: Option<(AnyWindowHandle, Entity<Launcher>)>,
    /// A window is being made, once the update asking for it ends.
    opening: bool,
    /// The application the launcher was last called over, to hand the
    /// keyboard back to. None when it was called over Platinum itself.
    over: Option<i32>,
    /// What was typed and found, and an unsent message, between showings.
    kept: Kept,
    /// The applications on this Mac, as last read.
    apps: Vec<Item>,
    /// What the open window was last told, so a reply arriving word by word
    /// does not rebuild rows that have not changed.
    threads: Option<Vec<ChatThreadSummary>>,
    connection: Option<Connection>,
    _watching: Vec<Subscription>,
}

impl Summoned {
    /// The launcher's window, while it is open.
    pub(super) fn window(&self) -> Option<AnyWindowHandle> {
        self.window.as_ref().map(|(handle, _)| *handle)
    }
}

// The commands -----------------------------------------------------------------

/// A command the launcher can run.
struct Command {
    name: &'static str,
    /// What it does, said beside it.
    says: &'static str,
    /// Other words it answers to.
    words: &'static [&'static str],
    /// Shown before anything is typed.
    pinned: bool,
    run: Run,
}

#[derive(Clone, Copy)]
enum Run {
    /// Asked of the browser's window, which is opened first if it is closed.
    Workspace(fn() -> Box<dyn Action>),
    /// Done here, and the launcher stays to show it.
    Stay(fn(&mut Desk, &mut Context<Desk>)),
    /// Done here, and the launcher closes.
    Leave(fn(&mut Desk, &mut Context<Desk>)),
}

fn new_tab() -> Box<dyn Action> {
    Box::new(NewTab)
}
fn open_location() -> Box<dyn Action> {
    Box::new(FocusAddressBar)
}
fn show_history() -> Box<dyn Action> {
    Box::new(ShowHistory)
}
fn show_bookmarks() -> Box<dyn Action> {
    Box::new(ShowBookmarks)
}
fn show_downloads() -> Box<dyn Action> {
    Box::new(ShowDownloads)
}
fn agent_place() -> Box<dyn Action> {
    Box::new(OpenPlace {
        place: crate::places::Place::Agent,
    })
}

const NEW_CONVERSATION: &str = "New Conversation";
const USE_DEMO: &str = "Use Theo's Demo Authority";

const COMMANDS: &[Command] = &[
    Command {
        name: NEW_CONVERSATION,
        says: "Starts a new conversation with Theo, here",
        words: &["chat", "thread", "ask"],
        pinned: true,
        run: Run::Stay(|desk, cx| {
            desk.driver
                .update(cx, |driver, cx| driver.new_conversation(cx));
            desk.turn_to_talk(cx);
        }),
    },
    Command {
        name: "Open Theo's Workspace",
        says: "Opens the Agent place in Platinum, on the conversation shown here",
        words: &["agent", "theorem", "chat"],
        pinned: true,
        run: Run::Workspace(agent_place),
    },
    Command {
        name: "New Tab",
        says: "Opens a new tab in Platinum",
        words: &["browser", "window"],
        pinned: true,
        run: Run::Workspace(new_tab),
    },
    Command {
        name: "Open Location",
        says: "Puts the caret in Platinum's address bar",
        words: &["address", "url", "go"],
        pinned: false,
        run: Run::Workspace(open_location),
    },
    Command {
        name: "Show History",
        says: "Opens Platinum's history",
        words: &["visited", "pages"],
        pinned: false,
        run: Run::Workspace(show_history),
    },
    Command {
        name: "Show Bookmarks",
        says: "Opens Platinum's bookmarks",
        words: &["favourites", "favorites", "saved"],
        pinned: false,
        run: Run::Workspace(show_bookmarks),
    },
    Command {
        name: "Show Downloads",
        says: "Opens Platinum's downloads",
        words: &["files", "saved"],
        pinned: false,
        run: Run::Workspace(show_downloads),
    },
    Command {
        name: "Show or Hide Theo",
        says: "Shows Theo's face over the desktop, or hides it",
        words: &["companion", "face"],
        pinned: false,
        run: Run::Leave(|desk, cx| desk.toggle(cx)),
    },
    Command {
        name: "Stop Theo's Run",
        says: "Stops what Theo is doing in the conversation shown here",
        words: &["cancel", "halt"],
        pinned: false,
        run: Run::Stay(|desk, cx| desk.stop(cx)),
    },
    Command {
        name: USE_DEMO,
        says: "Theorem isn't set up on this Mac: talk to the demo authority instead",
        words: &["theorem", "set up", "connect"],
        pinned: true,
        run: Run::Stay(|desk, cx| desk.driver.update(cx, |driver, cx| driver.use_demo(cx))),
    },
    Command {
        name: "Quit Platinum",
        says: "Closes Platinum, and Theo with it",
        words: &["exit", "close"],
        pinned: false,
        run: Run::Leave(|_, cx| cx.defer(|cx| cx.quit())),
    },
];

/// The commands as rows: those that can be run as things stand.
fn commands(connection: Connection, cx: &App) -> Vec<Item> {
    let workspace = crate::has_workspace(cx);
    COMMANDS
        .iter()
        .filter(|command| match command.run {
            Run::Workspace(_) => workspace,
            _ => command.name != USE_DEMO || connection == Connection::NotConfigured,
        })
        .map(|command| {
            Item::new(
                ItemId::new("command", command.name),
                Kind::Command,
                command.name,
                Target::Command(command.name.to_owned()),
            )
            .detail(command.says)
            .keywords(command.words.iter().copied())
            .pinned(command.pinned)
        })
        .collect()
}

// The conversation ---------------------------------------------------------------

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64)
}

/// Theorem's conversations as rows, the latest first.
fn conversations(driver: &TheoremDriver) -> Vec<Item> {
    let projection = driver.projection();
    let now = now_ms();
    let mut threads: Vec<&ChatThreadSummary> = projection.threads.iter().collect();
    threads.sort_by_key(|thread| std::cmp::Reverse(thread.updated_at_ms));
    threads
        .into_iter()
        .enumerate()
        .map(|(at, thread)| {
            let title = match thread.title.trim() {
                "" => "Untitled conversation",
                title => title,
            };
            let touched = words::ago(now, thread.updated_at_ms);
            let mut item = Item::new(
                ItemId::new("conversation", &thread.thread_id),
                Kind::Conversation,
                title,
                Target::Conversation(thread.thread_id.clone()),
            )
            .detail(touched.clone())
            .fact("Last touched", touched)
            .pinned(at < RECENT);
            if let Some(project) = projection
                .projects
                .iter()
                .find(|project| project.project_id == thread.project_id)
            {
                item = item.fact("Project", project.title.clone());
            }
            item
        })
        .collect()
}

/// Why a message cannot be sent now, when it cannot.
fn refusing(driver: &TheoremDriver) -> Option<String> {
    if driver.connection() == Connection::NotConfigured {
        return Some(format!(
            "Theorem isn't set up on this Mac. \"{USE_DEMO}\" is among the commands."
        ));
    }
    let projection = driver.projection();
    if projection.phase.is_refusal() {
        return Some(
            projection
                .notice
                .clone()
                .unwrap_or_else(|| projection.phase.label().to_owned()),
        );
    }
    let composer = &projection.composer;
    // Asked with something to say, so an empty field is never the reason.
    match composer.affordance("?") {
        SendAffordance::Send | SendAffordance::Blocked(SendBlock::Empty) => None,
        SendAffordance::Blocked(SendBlock::Refused) => Some(composer.refusal.trim().to_owned()),
        SendAffordance::Blocked(block) => Some(format!("{}.", block.reason())),
    }
}

/// The conversation as the launcher draws it.
fn talk(driver: &TheoremDriver) -> Talk {
    let projection = driver.projection();
    let thread = &projection.thread;
    let title = match thread.title.trim() {
        "" if thread.messages.is_empty() => None,
        "" => Some("Untitled conversation".to_owned()),
        title => Some(title.to_owned()),
    };
    let from = thread.messages.len().saturating_sub(TURNS);
    let turns = thread.messages[from..]
        .iter()
        .filter_map(|message| {
            let who = match message.actor {
                ThreadActor::User => Who::Person,
                ThreadActor::Theorem => Who::Theo,
                _ => return None,
            };
            let text = message
                .parts
                .iter()
                .filter_map(|part| match part {
                    ThreadPart::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(Turn { who, text })
        })
        .collect();
    let attachments = projection
        .composer
        .attachments
        .iter()
        .map(|attachment| Attachment {
            id: attachment.attachment_id.clone(),
            name: match &attachment.state {
                AttachmentState::Ready => attachment.name.clone(),
                AttachmentState::Failed { .. } => format!("{} (failed)", attachment.name),
                _ => format!("{} (adding…)", attachment.name),
            },
        })
        .collect();
    Talk {
        title,
        turns,
        attachments,
        working: thread.run_state.is_live(),
        refusing: refusing(driver),
    }
}

// The desk's side ------------------------------------------------------------------

impl Desk {
    /// Shows the launcher over the application in front, or hides it.
    pub(super) fn summon(&mut self, cx: &mut Context<Self>) {
        if let Some((handle, _)) = &self.summoned.window {
            if cx.windows().contains(handle) {
                self.dismiss_launcher(true, cx);
                return;
            }
            // Closed some other way: only a window that is gone is forgotten.
            self.summoned.window = None;
        }
        if self.summoned.opening {
            return;
        }
        // Asked each time: called over Platinum itself, closing the window
        // is enough, and an application from an earlier call is not brought
        // forward in its place.
        self.summoned.over = mac::frontmost().map(|front| front.pid);
        self.summoned.opening = true;
        self.read_apps(cx);
        // Made once this update has ended, as Theo's own window is.
        let desk = cx.entity();
        cx.defer(move |cx| open(desk, cx));
    }

    /// Reads the application folders off the main thread and hands what is
    /// there to the launcher, replacing what it had.
    fn read_apps(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |desk, cx| {
            let found = cx
                .background_spawn(async move {
                    let home = dirs::home_dir();
                    apps::scan(&apps::folders(home.as_deref()), SystemTime::now())
                })
                .await;
            let _ = desk.update(cx, |desk, cx| {
                if found == desk.summoned.apps {
                    return;
                }
                desk.summoned.apps = found.clone();
                if let Some((_, view)) = &desk.summoned.window {
                    view.update(cx, |view, cx| view.set_standing(Lane::Apps, found, cx));
                }
            });
        })
        .detach();
    }

    /// Takes the window `open` made: listens to it, and hands it what it
    /// shows.
    fn adopt(
        &mut self,
        handle: AnyWindowHandle,
        view: Entity<Launcher>,
        left: Subscription,
        cx: &mut Context<Self>,
    ) {
        let chosen = cx.subscribe(&view, |desk, _, event: &Event, cx| {
            desk.launcher_event(event, cx)
        });
        let apps = self.summoned.apps.clone();
        let files: Vec<_> = dirs::home_dir().into_iter().collect();
        view.update(cx, |view, cx| {
            if !apps.is_empty() {
                view.set_standing(Lane::Apps, apps, cx);
            }
            view.add_source(Lane::Files, Arc::new(Spotlight::new(files)), cx);
        });
        self.summoned.window = Some((handle, view));
        self.summoned.threads = None;
        self.summoned.connection = None;
        self.summoned._watching = vec![chosen, left];
        self.feed(cx);
    }

    /// Tells the open launcher how Theorem stands: the conversations, the
    /// commands that can be run, and the conversation shown. Called when the
    /// window opens and whenever the driver changes.
    pub(super) fn feed(&mut self, cx: &mut Context<Self>) {
        let Some((_, view)) = self.summoned.window.clone() else {
            return;
        };
        let (threads, commands, connection, talk) = {
            let driver = self.driver.read(cx);
            let known = &driver.projection().threads;
            let threads = (self.summoned.threads.as_ref() != Some(known))
                .then(|| (known.clone(), conversations(driver)));
            let connection = driver.connection();
            let commands =
                (self.summoned.connection != Some(connection)).then(|| commands(connection, cx));
            (threads, commands, connection, talk(driver))
        };
        view.update(cx, |view, cx| {
            if let Some((_, rows)) = &threads {
                view.set_standing(Lane::Conversations, rows.clone(), cx);
            }
            if let Some(rows) = commands {
                view.set_standing(Lane::Commands, rows, cx);
            }
            view.set_talk(talk, cx);
        });
        if let Some((known, _)) = threads {
            self.summoned.threads = Some(known);
        }
        self.summoned.connection = Some(connection);
    }

    /// Closes the launcher's window and keeps what it held. Whatever Theo is
    /// doing goes on. `give_back` hands the keyboard to the application the
    /// launcher was called over: not wanted when the row chosen brings
    /// something else forward.
    pub(super) fn dismiss_launcher(&mut self, give_back: bool, cx: &mut Context<Self>) {
        let Some((handle, view)) = self.summoned.window.take() else {
            return;
        };
        self.summoned.kept = view.read(cx).keep();
        self.summoned._watching.clear();
        // Closed once this update has ended: closing a window runs the app's
        // window-closed callbacks, which read the desk.
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        });
        if give_back
            && let Some(pid) = self.summoned.over
            && mac::we_are_active()
        {
            mac::return_to(pid);
        }
        cx.notify();
    }

    fn launcher_event(&mut self, event: &Event, cx: &mut Context<Self>) {
        match event {
            Event::Chosen(chosen) => self.chosen(chosen, cx),
            Event::Send(text) => self.say(text, cx),
            Event::Stop => self.stop(cx),
            Event::NewConversation => {
                self.driver
                    .update(cx, |driver, cx| driver.new_conversation(cx));
            }
            Event::RemoveAttachment(id) => {
                let attachment_id = id.clone();
                self.driver.update(cx, |driver, cx| {
                    driver.composer_intent(ComposerIntent::RemoveAttachment { attachment_id }, cx)
                });
            }
            Event::OpenWorkspace => {
                // The Agent place shows the driver's conversation: the same one.
                self.dismiss_launcher(false, cx);
                self.ask_workspace(agent_place(), cx);
            }
            Event::Dismiss => self.dismiss_launcher(true, cx),
        }
    }

    /// Does what the chosen row says.
    fn chosen(&mut self, chosen: &Chosen, cx: &mut Context<Self>) {
        let Chosen { item, verb } = chosen;
        match (verb, &item.target) {
            (Verb::Open, Target::Path(path)) => {
                self.dismiss_launcher(false, cx);
                cx.open_with_system(path);
            }
            (Verb::Reveal, Target::Path(path)) => {
                self.dismiss_launcher(false, cx);
                cx.reveal_path(path);
            }
            (Verb::CopyPath, Target::Path(path)) => {
                cx.write_to_clipboard(ClipboardItem::new_string(path.display().to_string()));
                self.dismiss_launcher(true, cx);
            }
            (Verb::CopyAddress, Target::Address(address)) => {
                cx.write_to_clipboard(ClipboardItem::new_string(address.clone()));
                self.dismiss_launcher(true, cx);
            }
            // Attached to the message being written, and to nothing else.
            (Verb::AskAbout, Target::Path(path)) => {
                let paths = vec![path.clone()];
                self.driver
                    .update(cx, |driver, cx| driver.attach_paths(paths, cx));
            }
            (Verb::Resume, Target::Conversation(id)) => {
                self.driver
                    .update(cx, |driver, cx| driver.select_thread(id, cx));
            }
            (Verb::Run, Target::Command(name)) => self.run(name, cx),
            (Verb::Open, Target::Address(address)) => {
                self.dismiss_launcher(false, cx);
                if crate::has_workspace(cx) {
                    self.ask_workspace(
                        Box::new(OpenTyped {
                            text: address.clone(),
                            search: false,
                        }),
                        cx,
                    );
                } else {
                    // The notes app has no tabs: the person's browser opens it.
                    cx.open_url(address);
                }
            }
            (Verb::Search, Target::WebSearch(query)) => {
                if !crate::has_workspace(cx) {
                    log::warn!("theo: no browser window in this launch to search the web in");
                    return;
                }
                self.dismiss_launcher(false, cx);
                self.ask_workspace(
                    Box::new(OpenTyped {
                        text: query.clone(),
                        search: true,
                    }),
                    cx,
                );
            }
            (verb, target) => log::warn!("theo: nothing does {verb:?} to {target:?}"),
        }
    }

    fn run(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(command) = COMMANDS.iter().find(|command| command.name == name) else {
            log::warn!("theo: no command is named {name}");
            return;
        };
        match command.run {
            Run::Workspace(action) => {
                self.dismiss_launcher(false, cx);
                self.ask_workspace(action(), cx);
            }
            Run::Stay(run) => run(self, cx),
            Run::Leave(run) => {
                self.dismiss_launcher(true, cx);
                run(self, cx);
            }
        }
    }

    /// Hands `action` to the browser's window once this update has ended:
    /// opening or activating a window runs callbacks that read the desk.
    fn ask_workspace(&self, action: Box<dyn Action>, cx: &mut Context<Self>) {
        cx.defer(move |cx| {
            if !crate::ask_workspace(Some(action), cx) {
                log::warn!("theo: there is no browser window to ask");
            }
        });
    }

    /// Sends what was written to the conversation shown, as the Agent
    /// place's composer does, when the composer says it can be sent. A
    /// message it will not take goes back to the field.
    fn say(&mut self, text: &str, cx: &mut Context<Self>) {
        let (can, quote) = {
            let composer = &self.driver.read(cx).projection().composer;
            (composer.affordance(text).is_send(), composer.quote.clone())
        };
        if !can {
            let Some((handle, view)) = self.summoned.window.clone() else {
                // The window has gone: the message waits for the next one.
                self.summoned.kept.draft = text.to_owned();
                return;
            };
            let text = text.to_owned();
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    view.update(cx, |view, cx| view.put_back(&text, window, cx))
                });
            });
            return;
        }
        let text = text.to_owned();
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

    /// Turns the open launcher to the conversation.
    fn turn_to_talk(&self, cx: &mut Context<Self>) {
        let Some((handle, view)) = self.summoned.window.clone() else {
            return;
        };
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                view.update(cx, |view, cx| view.show_talk(window, cx))
            });
        });
    }

    pub(super) fn set_keep_running(&mut self, cx: &mut Context<Self>) {
        self.settings.resident = !self.settings.resident;
        self.save();
        // The menus read the desk for their check, so they are made again
        // once this update has ended.
        cx.defer(crate::install_menus);
        cx.notify();
    }
}

/// Opens the launcher's window, unless it was dismissed while waiting.
fn open(desk: Entity<Desk>, cx: &mut App) {
    let kept = {
        let shown = desk.read(cx);
        if !shown.summoned.opening || shown.summoned.window.is_some() {
            desk.update(cx, |desk, _| desk.summoned.opening = false);
            return;
        }
        shown.summoned.kept.clone()
    };
    let wanted = size(px(SIZE.0), px(SIZE.1));
    // Centred, a little above the middle, where the eye already is.
    let origin = cx
        .primary_display()
        .map_or(point(px(0.), px(0.)), |display| {
            let visible = display.visible_bounds();
            point(
                visible.origin.x + (visible.size.width - wanted.width) / 2.,
                visible.origin.y + (visible.size.height - wanted.height) * 0.3,
            )
        });
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, wanted))),
        titlebar: None,
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: true,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..WindowOptions::default()
    };
    let made = Rc::new(Cell::new(None));
    let slot = made.clone();
    let owner = desk.downgrade();
    let opened = cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| Launcher::new(kept, window, cx));
        mac::unframe_keeping_shadow(window, cx);
        window.set_window_title("Theo");
        window.activate_window();
        // Choosing another window or application closes it, as a menu does.
        // Not before it has once been the active window, or it would close
        // as it opens.
        let was_active = Cell::new(false);
        let left = view.update(cx, |view, cx| {
            view.focus(window, cx);
            cx.observe_window_activation(window, move |_, window, cx| {
                if window.is_window_active() {
                    was_active.set(true);
                } else if was_active.replace(false) {
                    // After this update: the desk reads the view to keep it.
                    let owner = owner.clone();
                    cx.defer(move |cx| {
                        let _ = owner.update(cx, |desk, cx| desk.dismiss_launcher(false, cx));
                    });
                }
            })
        });
        slot.set(Some((view.clone(), left)));
        cx.new(|cx| Root::new(view, window, cx).bg(gpui_kit::transparent_black()))
    });
    let made = made.take();
    desk.update(cx, |desk, cx| {
        desk.summoned.opening = false;
        match (opened, made) {
            (Ok(handle), Some((view, left))) => desk.adopt(handle.into(), view, left, cx),
            (Ok(_), None) => log::error!("theo: the launcher's window opened without a view"),
            (Err(error), _) => log::error!("theo: could not open the launcher: {error:#}"),
        }
        cx.notify();
    });
}
