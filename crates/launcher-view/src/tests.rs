//! The launcher in a headless window: what typing, the arrows, Tab, Enter
//! and Escape do, and what the host is told.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gpui_kit::component::Root;
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext, px, size};
use launcher::{Cancel, Item, ItemId, Kind, Lane, Mode, Scope, Session, Source, Target, Verb};

use crate::{Event, Kept, Launcher, REST, SIZE, Talk, Turn, Who};

fn app(name: &str) -> Item {
    let path = format!("/Applications/{name}.app");
    Item::new(
        ItemId::new("app", &path),
        Kind::App,
        name,
        Target::Path(PathBuf::from(path)),
    )
}

fn command(name: &str) -> Item {
    Item::new(
        ItemId::new("command", name),
        Kind::Command,
        name,
        Target::Command(name.to_owned()),
    )
    .pinned(true)
}

fn conversation(id: &str, title: &str) -> Item {
    Item::new(
        ItemId::new("conversation", id),
        Kind::Conversation,
        title,
        Target::Conversation(id.to_owned()),
    )
    .pinned(true)
}

/// Finds one file named after the query, and notes each query it was
/// asked.
#[derive(Default)]
struct Shelf {
    asked: Mutex<Vec<String>>,
    broken: bool,
}

impl Source for Shelf {
    fn find(&self, query: &str, _: &Cancel) -> Result<Vec<Item>, String> {
        self.asked.lock().unwrap().push(query.to_owned());
        if self.broken {
            return Err("Spotlight is off".to_owned());
        }
        let path = format!("/Users/someone/{query} notes.txt");
        Ok(vec![Item::new(
            ItemId::new("file", &path),
            Kind::File,
            format!("{query} notes.txt"),
            Target::Path(PathBuf::from(path)),
        )])
    }
}

fn standing() -> Session {
    let mut session = Session::new();
    session.set_standing(
        Lane::Apps,
        vec![app("Lumio"), app("Lumio Dev"), app("Mail")],
    );
    session.set_standing(Lane::Commands, vec![command("New Tab")]);
    session.set_standing(
        Lane::Conversations,
        vec![conversation("t1", "Launcher lifecycle")],
    );
    session
}

type Told = Rc<RefCell<Vec<Event>>>;

fn open(kept: Kept, cx: &mut TestAppContext) -> (Entity<Launcher>, Told, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::init(cx);
    });
    let mut made = None;
    let window = cx.open_window(size(px(SIZE.0), px(SIZE.1)), |window, cx| {
        let launcher = cx.new(|cx| Launcher::new(kept, window, cx));
        made = Some(launcher.clone());
        Root::new(launcher, window, cx)
    });
    let launcher = made.expect("launcher view");
    let told = Told::default();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    let log = told.clone();
    cx.update(|window, cx| {
        cx.subscribe(&launcher, move |_, event: &Event, _| {
            log.borrow_mut().push(event.clone());
        })
        .detach();
        // As the person's window is: GPUI tells focus listeners nothing
        // while the window is in the background.
        window.activate_window();
        launcher.update(cx, |launcher, cx| launcher.focus(window, cx));
    });
    cx.run_until_parked();
    (launcher, told, cx)
}

fn start(cx: &mut TestAppContext) -> (Entity<Launcher>, Told, &mut VisualTestContext) {
    open(
        Kept {
            session: standing(),
            draft: String::new(),
        },
        cx,
    )
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn typed(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Lets a rested query's searches run.
fn rest(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(REST * 2);
    cx.run_until_parked();
}

fn add_shelf(launcher: &Entity<Launcher>, shelf: Arc<Shelf>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        launcher.update(cx, |launcher, cx| {
            launcher.add_source(Lane::Files, shelf, cx);
        })
    });
}

fn set_talk(launcher: &Entity<Launcher>, talk: Talk, cx: &mut VisualTestContext) {
    cx.update(|_, cx| launcher.update(cx, |launcher, cx| launcher.set_talk(talk, cx)));
    cx.run_until_parked();
}

fn titles(launcher: &Entity<Launcher>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        launcher
            .read(cx)
            .session()
            .items()
            .map(|item| item.title.clone())
            .collect()
    })
}

fn selected(launcher: &Entity<Launcher>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        launcher
            .read(cx)
            .session()
            .selected()
            .map(|item| item.title.clone())
    })
}

fn field(launcher: &Entity<Launcher>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| launcher.read(cx).field.read(cx).value().to_string())
}

fn mode(launcher: &Entity<Launcher>, cx: &mut VisualTestContext) -> Mode {
    cx.update(|_, cx| launcher.read(cx).session().mode())
}

fn scope(launcher: &Entity<Launcher>, cx: &mut VisualTestContext) -> Scope {
    cx.update(|_, cx| launcher.read(cx).session().scope())
}

fn looking(launcher: &Entity<Launcher>, cx: &mut VisualTestContext) -> Option<Lane> {
    cx.update(|_, cx| launcher.read(cx).session().looking())
}

#[gpui_kit::test]
fn typing_finds_and_enter_opens_the_selected_row(cx: &mut TestAppContext) {
    let (launcher, told, cx) = start(cx);
    // Untyped, only what the host pinned is shown.
    assert_eq!(titles(&launcher, cx), ["New Tab", "Launcher lifecycle"]);

    typed(cx, "lumio");
    assert_eq!(selected(&launcher, cx).as_deref(), Some("Lumio"));
    keys(cx, "down");
    assert_eq!(selected(&launcher, cx).as_deref(), Some("Lumio Dev"));
    keys(cx, "enter");

    let told = told.borrow();
    let [Event::Chosen(chosen)] = told.as_slice() else {
        panic!("one thing chosen, got {told:?}");
    };
    assert_eq!(chosen.verb, Verb::Open);
    assert_eq!(
        chosen.item.target,
        Target::Path(PathBuf::from("/Applications/Lumio Dev.app"))
    );
    // Looking something up started no conversation.
    assert_eq!(mode(&launcher, cx), Mode::Find);
}

#[gpui_kit::test]
fn tab_narrows_the_results_and_types_nothing(cx: &mut TestAppContext) {
    let (launcher, _, cx) = start(cx);
    typed(cx, "l");
    keys(cx, "tab");
    assert_eq!(field(&launcher, cx), "l");
    assert_eq!(scope(&launcher, cx), Scope::Apps);
    assert_eq!(titles(&launcher, cx), ["Lumio", "Lumio Dev", "Mail"]);
    keys(cx, "shift-tab");
    assert_eq!(scope(&launcher, cx), Scope::All);
}

#[gpui_kit::test]
fn asking_sends_what_was_typed_and_turns_to_the_conversation(cx: &mut TestAppContext) {
    let (launcher, told, cx) = start(cx);
    typed(cx, "why did the build fail?");
    // It reads as a question, so asking leads; it is still its own row.
    assert_eq!(
        selected(&launcher, cx).as_deref(),
        Some("why did the build fail?")
    );
    keys(cx, "enter");

    assert_eq!(
        told.borrow().as_slice(),
        [Event::Send("why did the build fail?".to_owned())]
    );
    assert_eq!(mode(&launcher, cx), Mode::Talk);
    assert_eq!(field(&launcher, cx), "");

    // The next message goes to the same conversation, from the same field.
    typed(cx, "and how do I fix it");
    keys(cx, "enter");
    assert_eq!(
        told.borrow().last(),
        Some(&Event::Send("and how do I fix it".to_owned()))
    );
    assert_eq!(field(&launcher, cx), "");
}

#[gpui_kit::test]
fn a_name_that_finds_nothing_does_not_ask_by_itself(cx: &mut TestAppContext) {
    let (launcher, told, cx) = start(cx);
    typed(cx, "zzyzx");
    // The web row leads; asking is offered beneath it, not chosen.
    let first = cx.update(|_, cx| launcher.read(cx).session().selected().map(|item| item.kind));
    assert_eq!(first, Some(Kind::WebSearch));
    keys(cx, "enter");
    let told = told.borrow();
    assert!(
        matches!(told.as_slice(), [Event::Chosen(chosen)] if chosen.verb == Verb::Search),
        "{told:?}"
    );
}

#[gpui_kit::test]
fn escape_steps_back_one_thing_at_a_time(cx: &mut TestAppContext) {
    let (launcher, told, cx) = start(cx);
    typed(cx, "lifecycle");
    assert_eq!(
        selected(&launcher, cx).as_deref(),
        Some("Launcher lifecycle")
    );
    keys(cx, "enter");
    // Resuming is the host's to do; the window turns to the conversation.
    assert!(matches!(
        told.borrow().as_slice(),
        [Event::Chosen(chosen)] if chosen.verb == Verb::Resume
    ));
    assert_eq!(mode(&launcher, cx), Mode::Talk);
    typed(cx, "half a thought");

    keys(cx, "escape");
    assert_eq!(mode(&launcher, cx), Mode::Find);
    // The results are where they were left.
    assert_eq!(field(&launcher, cx), "lifecycle");
    assert_eq!(
        selected(&launcher, cx).as_deref(),
        Some("Launcher lifecycle")
    );

    keys(cx, "escape");
    assert_eq!(field(&launcher, cx), "");
    assert_eq!(told.borrow().len(), 1);

    keys(cx, "escape");
    assert_eq!(told.borrow().last(), Some(&Event::Dismiss));

    // The unsent message was kept through all of it.
    let kept = cx.update(|_, cx| launcher.read(cx).keep());
    assert_eq!(kept.draft, "half a thought");
}

#[gpui_kit::test]
fn a_rested_query_is_asked_once_and_an_abandoned_one_never(cx: &mut TestAppContext) {
    let (launcher, _, cx) = start(cx);
    let shelf = Arc::new(Shelf::default());
    add_shelf(&launcher, shelf.clone(), cx);
    typed(cx, "lu");
    // Typed on before the first query rested: it is given up unasked.
    typed(cx, "m");
    assert_eq!(looking(&launcher, cx), Some(Lane::Files));
    rest(cx);

    assert_eq!(shelf.asked.lock().unwrap().as_slice(), ["lum"]);
    assert!(titles(&launcher, cx).contains(&"lum notes.txt".to_owned()));
    // The first row was selected before the files arrived, and still is.
    assert_eq!(selected(&launcher, cx).as_deref(), Some("Lumio"));
    assert_eq!(looking(&launcher, cx), None);
}

#[gpui_kit::test]
fn a_lane_that_cannot_look_says_so_and_the_rest_still_answer(cx: &mut TestAppContext) {
    let (launcher, told, cx) = start(cx);
    let shelf = Arc::new(Shelf {
        broken: true,
        ..Shelf::default()
    });
    add_shelf(&launcher, shelf, cx);
    typed(cx, "mail");
    rest(cx);

    let unavailable = cx.update(|_, cx| {
        launcher
            .read(cx)
            .session()
            .unavailable()
            .into_iter()
            .map(|(lane, why)| (lane, why.to_owned()))
            .collect::<Vec<_>>()
    });
    assert_eq!(unavailable, [(Lane::Files, "Spotlight is off".to_owned())]);
    assert_eq!(selected(&launcher, cx).as_deref(), Some("Mail"));
    // The failed lookup asked nothing of Theo.
    assert!(told.borrow().is_empty());
}

#[gpui_kit::test]
fn command_enter_attaches_the_file_and_turns_to_the_conversation(cx: &mut TestAppContext) {
    let (launcher, told, cx) = start(cx);
    add_shelf(&launcher, Arc::new(Shelf::default()), cx);
    typed(cx, "budget");
    rest(cx);
    keys(cx, "tab tab");
    assert_eq!(scope(&launcher, cx), Scope::Files);
    assert_eq!(selected(&launcher, cx).as_deref(), Some("budget notes.txt"));
    keys(cx, "secondary-enter");

    assert!(
        matches!(
            told.borrow().as_slice(),
            [Event::Chosen(chosen)] if chosen.verb == Verb::AskAbout
        ),
        "{:?}",
        told.borrow()
    );
    assert_eq!(mode(&launcher, cx), Mode::Talk);
    assert_eq!(field(&launcher, cx), "");
}

#[gpui_kit::test]
fn a_question_that_cannot_be_sent_waits_in_the_field(cx: &mut TestAppContext) {
    let (launcher, told, cx) = start(cx);
    set_talk(
        &launcher,
        Talk {
            refusing: Some("Theorem isn't set up on this Mac.".to_owned()),
            ..Talk::default()
        },
        cx,
    );
    typed(cx, "what changed today?");
    keys(cx, "enter");

    assert!(told.borrow().is_empty());
    assert_eq!(mode(&launcher, cx), Mode::Talk);
    assert_eq!(field(&launcher, cx), "what changed today?");
    // Enter does not send it either, and does not lose it.
    keys(cx, "enter");
    assert!(told.borrow().is_empty());
    assert_eq!(field(&launcher, cx), "what changed today?");
}

#[gpui_kit::test]
fn the_ask_row_says_which_conversation_a_question_goes_to(cx: &mut TestAppContext) {
    let (launcher, _, cx) = start(cx);
    typed(cx, "is this right?");
    let detail = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            launcher
                .read(cx)
                .session()
                .selected()
                .map(|item| item.detail.clone())
        })
    };
    assert_eq!(detail(cx).as_deref(), Some("Starts a new conversation"));
    set_talk(
        &launcher,
        Talk {
            title: Some("Launcher lifecycle".to_owned()),
            turns: vec![Turn {
                who: Who::Theo,
                text: "It stays resident.".to_owned(),
            }],
            ..Talk::default()
        },
        cx,
    );
    assert_eq!(
        detail(cx).as_deref(),
        Some("Continues “Launcher lifecycle”")
    );
}

#[gpui_kit::test]
fn what_was_kept_comes_back_in_a_new_window(cx: &mut TestAppContext) {
    let (launcher, _, cx) = start(cx);
    typed(cx, "lumio");
    keys(cx, "down");
    let kept = cx.update(|_, cx| launcher.read(cx).keep());
    assert_eq!(kept.session.query(), "lumio");

    let mut session = kept.session.clone();
    session.talk();
    let (again, _, cx) = open(
        Kept {
            session,
            draft: "not sent yet".to_owned(),
        },
        cx,
    );
    // Talking: the field holds the draft, and the results wait behind it.
    assert_eq!(field(&again, cx), "not sent yet");
    keys(cx, "escape");
    assert_eq!(field(&again, cx), "lumio");
    assert_eq!(selected(&again, cx).as_deref(), Some("Lumio Dev"));
}

#[gpui_kit::test]
fn a_message_the_host_could_not_send_comes_back_to_the_field(cx: &mut TestAppContext) {
    let (launcher, told, cx) = start(cx);
    typed(cx, "what changed today?");
    keys(cx, "enter");
    assert_eq!(
        told.borrow().as_slice(),
        [Event::Send("what changed today?".to_owned())]
    );
    assert_eq!(field(&launcher, cx), "");

    // The person went on typing before the host found it could not send.
    typed(cx, "and why");
    cx.update(|window, cx| {
        launcher.update(cx, |launcher, cx| {
            launcher.put_back("what changed today?", window, cx)
        })
    });
    cx.run_until_parked();

    assert_eq!(field(&launcher, cx), "what changed today? and why");
    let kept = cx.update(|_, cx| launcher.read(cx).keep());
    assert_eq!(kept.draft, "what changed today? and why");
    // Nothing more was sent by handing it back.
    assert_eq!(told.borrow().len(), 1);
}
