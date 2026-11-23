//! The persistent shell's journey, driven the way a person drives it:
//! press the face, wait for the body to change, write, press Escape, and
//! come back to the same Theo with the same words still there.
//!
//! This runs the real view and the real poll in Rune's in-process runtime;
//! no window exists, so the presentation's window effects queue unapplied.

use rune::{Env, Key, KeyEvent, Mods, Runtime};
use theo_desktop::Theo;
use theo_desktop::app::{COMPOSER_KEY, FACE_KEY};
use theo_desktop::presentation::{Form, WORKSPACE_SIZE};
use theo_desktop::session::new_conversation;
use theo_desktop::views;

const FRAME: f64 = 1.0 / 60.0;

struct Journey {
    rt: Runtime<Theo>,
    t: f64,
}

impl Journey {
    fn new() -> Self {
        let rt = Runtime::new(
            Theo::headless(),
            views::view,
            WORKSPACE_SIZE,
            Env::new(true, 2.0),
        );
        let mut journey = Self { rt, t: 0.0 };
        journey.run(0.05);
        journey
    }

    /// Moves time on by `seconds`, polling as the application loop does.
    fn run(&mut self, seconds: f64) {
        let frames = (seconds / FRAME).ceil() as usize;
        for _ in 0..frames {
            self.t += FRAME;
            let t = self.t;
            self.rt.update_if(|s, tx| s.poll(t, tx));
            self.rt.frame(FRAME);
        }
    }

    fn press(&mut self, key: &str) {
        let (x, y) = self
            .rt
            .find(key)
            .unwrap_or_else(|| panic!("no element {key}"));
        self.rt.pointer_down(x, y);
        self.rt.pointer_up(x, y);
        self.rt.frame(FRAME);
    }

    fn key(&mut self, key: Key) {
        self.rt.key(KeyEvent {
            key,
            mods: Mods::default(),
        });
        self.rt.frame(FRAME);
    }

    fn theo(&self) -> &Theo {
        &self.rt.state
    }
}

#[test]
fn the_face_opens_into_the_workspace_and_comes_back_the_same_theo() {
    let mut journey = Journey::new();
    let session = journey.theo().session.id();
    assert_eq!(journey.theo().session.form(), Form::Presence);
    assert!(
        journey.rt.find(FACE_KEY).is_some(),
        "the presence shows the face"
    );

    journey.press(FACE_KEY);
    assert!(
        !journey.theo().session.presentation.workspace_interactive(),
        "the workspace takes no input while the body is still changing"
    );
    journey.run(0.7);
    assert_eq!(journey.theo().session.form(), Form::Workspace);
    assert!(journey.theo().session.presentation.workspace_interactive());

    journey.key(Key::Text("half a thought".into()));
    let draft = &journey
        .theo()
        .session
        .surfaces
        .get(&new_conversation())
        .unwrap()
        .edit
        .text;
    assert_eq!(
        draft, "half a thought",
        "the settled workspace has the keyboard in its composer"
    );

    journey.key(Key::Escape);
    journey.run(0.6);
    assert_eq!(journey.theo().session.form(), Form::Presence);
    assert!(
        journey.rt.find(COMPOSER_KEY).is_none(),
        "the workspace is gone"
    );
    assert!(journey.rt.find(FACE_KEY).is_some(), "the face is back");

    assert_eq!(
        journey.theo().session.id(),
        session,
        "the same session throughout"
    );
    let draft = &journey
        .theo()
        .session
        .surfaces
        .get(&new_conversation())
        .unwrap()
        .edit
        .text;
    assert_eq!(
        draft, "half a thought",
        "nothing written is lost by changing form"
    );
}

#[test]
fn reduced_motion_makes_the_same_journey_without_the_change() {
    let mut journey = Journey::new();
    journey.rt.set_system_reduced_motion(true);
    journey.run(FRAME);
    journey.press(FACE_KEY);
    journey.run(2.0 * FRAME);
    assert!(journey.theo().session.presentation.workspace_interactive());
    journey.key(Key::Escape);
    journey.run(2.0 * FRAME);
    assert_eq!(journey.theo().session.form(), Form::Presence);
}

#[test]
fn pressing_the_face_while_it_changes_reaches_nothing_beneath() {
    let mut journey = Journey::new();
    journey.press(FACE_KEY);
    journey.run(0.1);
    // Mid-change the inert layer covers the shell: a press is swallowed,
    // and in particular the bar's way back cannot be taken half-open.
    if let Some((x, y)) = journey.rt.find("bar.collapse") {
        journey.rt.pointer_down(x, y);
        journey.rt.pointer_up(x, y);
    }
    journey.run(0.7);
    assert_eq!(journey.theo().session.form(), Form::Workspace);
}
