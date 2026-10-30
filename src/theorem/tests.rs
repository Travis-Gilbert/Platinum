//! Headless UI integration tests of the workbench against Theorem's
//! deterministic demo authority (`docs/plans/theorem-workbench.md`,
//! Verification). Each drives the window the way a person does, with clicks
//! and keys, and checks what was drawn and what the driver now holds.

use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::{
    AppContext as _, Bounds, Context, Entity, Focusable as _, KeyBinding, Modifiers, Pixels,
    TestAppContext, VisualTestContext, px, size,
};
use theorem_surface_contracts::thread::{ThreadActor, ThreadRunState};
use theoremweb_agent_runtime::{ChatAuthorityKind, ChatHostPhase, DeterministicDemoAuthority};

use super::Workbench;
use super::driver::{Connection, TheoremDriver};
use super::starters::Role;
use super::transcript::{Block, blocks};
use super::workbench::{CONTEXT, NewConversation, NextConversation, PreviousConversation, StopRun};

fn open(
    driver: impl FnOnce(&mut Context<TheoremDriver>) -> TheoremDriver + 'static,
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.bind_keys([
            KeyBinding::new("cmd-n", NewConversation, Some(CONTEXT)),
            KeyBinding::new("cmd-.", StopRun, Some(CONTEXT)),
            KeyBinding::new("alt-up", PreviousConversation, Some(CONTEXT)),
            KeyBinding::new("alt-down", NextConversation, Some(CONTEXT)),
        ]);
    });
    let mut workbench = None;
    let window = cx.open_window(size(px(1100.), px(760.)), |window, cx| {
        let driver = cx.new(driver);
        let view = cx.new(|cx| Workbench::with_driver(driver, window, cx));
        workbench = Some(view.clone());
        Root::new(view, window, cx)
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (workbench.expect("workbench view"), cx)
}

fn demo(
    authority: DeterministicDemoAuthority,
    cx: &mut TestAppContext,
) -> (
    Entity<Workbench>,
    Rc<DeterministicDemoAuthority>,
    &mut VisualTestContext,
) {
    let authority = Rc::new(authority);
    let transport = authority.clone();
    let (workbench, cx) = open(
        move |cx| {
            TheoremDriver::with_transport(ChatAuthorityKind::DeterministicDemo, transport, cx)
        },
        cx,
    );
    (workbench, authority, cx)
}

/// The element drawn with `debug_selector(name)`, if it was.
fn bounds(cx: &mut VisualTestContext, name: impl Into<String>) -> Option<Bounds<Pixels>> {
    // `debug_bounds` takes a static name; these few live as long as the test.
    let name: &'static str = Box::leak(name.into().into_boxed_str());
    cx.debug_bounds(name)
}

fn drawn(cx: &mut VisualTestContext, name: impl Into<String>) -> bool {
    bounds(cx, name).is_some()
}

fn click(cx: &mut VisualTestContext, name: impl Into<String>) {
    let name = name.into();
    let at = bounds(cx, name.clone())
        .unwrap_or_else(|| panic!("{name} drawn"))
        .center();
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn run_state(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> ThreadRunState {
    cx.update(|_, cx| {
        workbench
            .read(cx)
            .driver()
            .read(cx)
            .projection()
            .thread
            .run_state
    })
}

fn selected_thread(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        workbench
            .read(cx)
            .driver()
            .read(cx)
            .projection()
            .selected_thread_id
            .clone()
    })
}

#[gpui_kit::test]
fn without_configuration_the_card_says_what_to_set_and_offers_the_demo(cx: &mut TestAppContext) {
    let (workbench, cx) = open(|cx| TheoremDriver::from_configuration(false, cx), cx);
    assert!(drawn(cx, "theorem-navigator"));
    assert!(drawn(cx, "theorem-card"));
    assert!(drawn(cx, "theorem-try-demo"));
    assert!(!drawn(cx, "theorem-composer"), "nothing to send to");
    // Opening the place asks for the composer; with none drawn, the
    // workbench holds the keyboard rather than a field nobody can see.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.focus_composer(window, cx));
        assert!(workbench.read(cx).focus_handle(cx).is_focused(window));
        assert!(
            !workbench
                .read(cx)
                .composer
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        );
    });

    click(cx, "theorem-try-demo");
    let threads = cx.update(|_, cx| {
        let driver = workbench.read(cx).driver().read(cx);
        assert_eq!(driver.connection(), Connection::Demo);
        assert_eq!(driver.projection().phase, ChatHostPhase::Ready);
        driver
            .projection()
            .threads
            .iter()
            .map(|thread| thread.thread_id.clone())
            .collect::<Vec<_>>()
    });
    assert!(!threads.is_empty());
    let bump = bounds(cx, "places-bump").expect("the places bump drawn");
    for thread in threads {
        let row = bounds(cx, format!("theorem-thread:{thread}"))
            .unwrap_or_else(|| panic!("{thread} listed"));
        assert!(
            row.left() >= bump.right(),
            "{thread} clear of the bump on the window's edge: {row:?} beside {bump:?}"
        );
    }
    assert!(drawn(cx, "theorem-composer"));
    assert!(!drawn(cx, "theorem-try-demo"));
    cx.update(|window, cx| {
        assert!(
            workbench
                .read(cx)
                .composer
                .read(cx)
                .focus_handle(cx)
                .is_focused(window),
            "once hydrated, the runtime's request for the composer is handed on"
        )
    });
}

#[gpui_kit::test]
fn a_starter_fills_the_composer_and_says_how_to_start(cx: &mut TestAppContext) {
    let (workbench, _, cx) = demo(DeterministicDemoAuthority::new(), cx);
    click(cx, "theorem-thread:demo-thread-parity");
    assert!(drawn(cx, "theorem-starters"), "nothing said yet");
    assert!(drawn(cx, "theorem-starter-review-change"));
    click(cx, "theorem-starter-review-change");
    let (text, focused) = cx.update(|window, cx| {
        let composer = workbench.read(cx).composer.read(cx);
        (
            composer.value().to_string(),
            composer.focus_handle(cx).is_focused(window),
        )
    });
    assert!(text.starts_with("Review this change"), "{text:?}");
    assert!(focused, "ready to add what matters, or Return");

    // Another role offers its own starters.
    cx.update(|_, cx| workbench.update(cx, |this, cx| this.set_role(Role::Founder, cx)));
    cx.run_until_parked();
    assert!(drawn(cx, "theorem-starter-pressure-test"));
    assert!(!drawn(cx, "theorem-starter-review-change"));
}

#[gpui_kit::test]
fn opening_a_conversation_shows_its_transcript_with_work_folded(cx: &mut TestAppContext) {
    let (workbench, _, cx) = demo(DeterministicDemoAuthority::new(), cx);
    click(cx, "theorem-thread:demo-thread-parity");
    assert_eq!(
        selected_thread(&workbench, cx).as_deref(),
        Some("demo-thread-parity")
    );
    assert!(
        drawn(cx, "theorem-empty"),
        "a conversation with nothing said yet"
    );

    click(cx, "theorem-thread:demo-thread-controller");
    assert_eq!(
        selected_thread(&workbench, cx).as_deref(),
        Some("demo-thread-controller")
    );
    assert!(drawn(cx, "theorem-transcript"));
    let (rows, answer, group) = cx.update(|_, cx| {
        let this = workbench.read(cx);
        let thread = &this.driver().read(cx).projection().thread;
        let rows = this.transcript.read(cx).item_count();
        assert_eq!(rows, thread.messages.len());
        let (answer, group) = thread
            .messages
            .iter()
            .filter(|message| message.actor == ThreadActor::Theorem)
            .find_map(|message| {
                blocks(&message.parts)
                    .into_iter()
                    .find_map(|block| match block {
                        Block::Activity(parts) => Some((
                            message.message_id.clone(),
                            format!("group:{}", parts[0].part_id()),
                        )),
                        _ => None,
                    })
            })
            .expect("the demo's answer shows its work");
        (rows, answer, group)
    });
    assert!(rows > 0);
    for message in cx.update(|_, cx| {
        workbench
            .read(cx)
            .driver()
            .read(cx)
            .projection()
            .thread
            .messages
            .iter()
            .map(|message| message.message_id.clone())
            .collect::<Vec<_>>()
    }) {
        assert!(
            drawn(cx, format!("theorem-message:{message}")),
            "{message} drawn"
        );
    }

    // The work is one folded summary until the reader opens it.
    let folded = bounds(cx, format!("theorem-message:{answer}")).expect("answer drawn");
    assert!(drawn(cx, format!("theorem-activity:{group}")));
    click(cx, format!("theorem-activity:{group}"));
    cx.update(|_, cx| assert!(workbench.read(cx).expanded.contains(&group)));
    let opened = bounds(cx, format!("theorem-message:{answer}")).expect("answer drawn");
    assert!(
        opened.size.height > folded.size.height,
        "opening the work makes room for its rows: {folded:?} then {opened:?}"
    );
    click(cx, format!("theorem-activity:{group}"));
    cx.update(|_, cx| assert!(!workbench.read(cx).expanded.contains(&group)));
}

#[gpui_kit::test]
fn sending_streams_the_answer_into_the_card_and_escape_stops_it(cx: &mut TestAppContext) {
    let (workbench, authority, cx) = demo(DeterministicDemoAuthority::new().holding_runs(), cx);
    click(cx, "theorem-composer");
    let before = selected_thread(&workbench, cx);
    keys(cx, "cmd-n");
    assert_ne!(
        selected_thread(&workbench, cx),
        before,
        "a new conversation"
    );
    cx.update(|window, cx| {
        assert!(
            workbench
                .read(cx)
                .driver()
                .read(cx)
                .projection()
                .thread
                .messages
                .is_empty()
        );
        assert!(
            workbench
                .read(cx)
                .composer
                .read(cx)
                .focus_handle(cx)
                .is_focused(window),
            "a new conversation puts the keyboard in the composer"
        )
    });

    cx.simulate_input("Platinum workbench probe");
    keys(cx, "enter");
    assert_eq!(run_state(&workbench, cx), ThreadRunState::Streaming);
    assert!(
        drawn(cx, "theorem-card-stop"),
        "the header offers Stop while it runs"
    );
    let reply = cx.update(|_, cx| {
        let projection = workbench.read(cx).driver().read(cx).projection();
        assert!(
            projection.composer.draft.is_empty(),
            "the sent text left the draft"
        );
        assert!(
            workbench.read(cx).composer.read(cx).value().is_empty(),
            "the sent text left the field"
        );
        projection
            .thread
            .messages
            .iter()
            .rfind(|message| message.actor == ThreadActor::Theorem)
            .map(|message| message.message_id.clone())
            .expect("the answer is streaming")
    });
    assert!(drawn(cx, format!("theorem-message:{reply}")));

    // Escape in an idle field has nothing of its own to dismiss, so it stops
    // the run. The keystroke parks the executor, so the local stop and the
    // authority's settled conversation land together; the driver's own test
    // holds the local half apart.
    cx.simulate_keystrokes("escape");
    assert_ne!(run_state(&workbench, cx), ThreadRunState::Streaming);
    assert!(!drawn(cx, "theorem-card-stop"));
    assert!(drawn(cx, "theorem-composer-send"));

    // Releasing afterwards revives nothing: the authority already ended it.
    authority.release_runs();
    cx.run_until_parked();
    assert_ne!(run_state(&workbench, cx), ThreadRunState::Streaming);
}

#[gpui_kit::test]
fn command_period_in_the_field_stops_the_run(cx: &mut TestAppContext) {
    // The field's own ⌘. belongs to the code editor's actions, which a
    // plain textarea does not handle, so the key reaches Stop.
    let (workbench, _authority, cx) = demo(DeterministicDemoAuthority::new().holding_runs(), cx);
    click(cx, "theorem-composer");
    cx.simulate_input("Hold this run");
    keys(cx, "enter");
    assert_eq!(run_state(&workbench, cx), ThreadRunState::Streaming);
    keys(cx, "cmd-.");
    assert_ne!(run_state(&workbench, cx), ThreadRunState::Streaming);
    assert!(drawn(cx, "theorem-composer-send"));
}

#[gpui_kit::test]
fn a_finished_run_settles_and_the_answer_stays(cx: &mut TestAppContext) {
    let (workbench, authority, cx) = demo(DeterministicDemoAuthority::new().holding_runs(), cx);
    click(cx, "theorem-composer");
    cx.simulate_input("Settle this one");
    click(cx, "theorem-composer-send");
    assert_eq!(run_state(&workbench, cx), ThreadRunState::Streaming);
    authority.release_runs();
    cx.run_until_parked();
    assert_eq!(run_state(&workbench, cx), ThreadRunState::Idle);
    assert!(!drawn(cx, "theorem-card-stop"));
    let answered = cx.update(|_, cx| {
        workbench
            .read(cx)
            .driver()
            .read(cx)
            .projection()
            .thread
            .messages
            .iter()
            .any(|message| {
                message.actor == ThreadActor::Theorem
                    && message.actions.copy_text.contains("Settle this one")
            })
    });
    assert!(answered, "the settled answer is the one the authority kept");
}

#[gpui_kit::test]
fn option_arrows_walk_the_conversations_in_navigator_order(cx: &mut TestAppContext) {
    let (workbench, _, cx) = demo(DeterministicDemoAuthority::new(), cx);
    click(cx, "theorem-thread:demo-thread-controller");
    click(cx, "theorem-composer");
    keys(cx, "alt-down");
    assert_eq!(
        selected_thread(&workbench, cx).as_deref(),
        Some("demo-thread-parity")
    );
    keys(cx, "alt-down");
    assert_eq!(
        selected_thread(&workbench, cx).as_deref(),
        Some("demo-thread-geometry")
    );
    // The last stays the last.
    keys(cx, "alt-down");
    assert_eq!(
        selected_thread(&workbench, cx).as_deref(),
        Some("demo-thread-geometry")
    );
    keys(cx, "alt-up");
    assert_eq!(
        selected_thread(&workbench, cx).as_deref(),
        Some("demo-thread-parity")
    );
}

#[gpui_kit::test]
fn stop_without_a_run_leaves_the_key_to_others(cx: &mut TestAppContext) {
    let (workbench, _, cx) = demo(DeterministicDemoAuthority::new(), cx);
    let notice = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            workbench
                .read(cx)
                .driver()
                .read(cx)
                .projection()
                .notice
                .clone()
        })
    };
    let before = (selected_thread(&workbench, cx), notice(cx));
    click(cx, "theorem-composer");
    keys(cx, "cmd-.");
    assert_eq!(run_state(&workbench, cx), ThreadRunState::Idle);
    assert_eq!(
        (selected_thread(&workbench, cx), notice(cx)),
        before,
        "no refusal for a stop that was never asked for"
    );
}

#[gpui_kit::test]
fn the_composer_widens_in_use_and_shows_what_the_send_will_use(cx: &mut TestAppContext) {
    let (_workbench, _, cx) = demo(DeterministicDemoAuthority::new(), cx);
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    let at_rest = bounds(cx, "theorem-composer").expect("composer drawn");
    assert!(
        !drawn(cx, "theorem-composer-context"),
        "at rest the composer is only its field"
    );

    click(cx, "theorem-composer");
    let in_use = bounds(cx, "theorem-composer").expect("composer drawn");
    assert!(
        in_use.size.width > at_rest.size.width,
        "focused, it widens to the column: {at_rest:?} then {in_use:?}"
    );
    assert!(
        drawn(cx, "theorem-composer-context-project"),
        "and names the project the send goes to"
    );
    assert_eq!(
        in_use.bottom(),
        at_rest.bottom(),
        "its base stays on the card's foot"
    );

    // Written in, it stays in use with the keyboard elsewhere, and grows
    // upward with the text.
    cx.simulate_input("First line");
    keys(cx, "shift-enter");
    cx.simulate_input("Second line");
    cx.run_until_parked();
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    let written = bounds(cx, "theorem-composer").expect("composer drawn");
    assert_eq!(written.size.width, in_use.size.width, "still in use");
    assert!(
        written.size.height > in_use.size.height && written.bottom() == in_use.bottom(),
        "a second line raises its top, not its base: {in_use:?} then {written:?}"
    );
}
