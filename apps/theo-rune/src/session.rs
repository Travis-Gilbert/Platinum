//! The one Theo session: everything the presence, the conversation and the
//! workspace show, owned once, above any window or view.
//!
//! A change of form changes which views observe this; it never copies it,
//! recreates the companion, or restarts what it holds. Durable resources
//! (threads, documents, runs) stay owned by Theorem's authority; the session
//! keeps their references, the client's projections of them, and the
//! editors open on them.

use std::time::{SystemTime, UNIX_EPOCH};

use companion::Companion;
use rune::Tx;
use theo::engine::FaceOptions;

use crate::behavior::run::Behaviors;
use crate::behavior::{Catalog, Name};
use crate::face::FaceFrame;
use crate::preferences::Saved;
use crate::presentation::{
    Cause, Effect, Form, Frame, Outcome, PRESENCE_SIZE, Presentation, Request, WORKSPACE_SIZE,
};
use crate::signals::{Owner, Registry};
use crate::surfaces::{SurfaceRef, Surfaces};

/// How fast the person is writing in the composer, in characters a second,
/// measured while the workspace shows and absent otherwise.
pub const TYPING: &str = "composer.typing";
/// How far the body has changed toward the workspace, 0 to 1.
pub const FORM: &str = "theo.form";
const TYPING_LIFETIME_MS: f64 = 1_500.0;
/// How quickly the typing rate follows what was just typed.
const TYPING_EASE: f64 = 0.15;

/// The session's identity, fixed when it begins. Views and windows come and
/// go; this does not change until the application ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

impl SessionId {
    fn fresh() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        Self(nanos ^ (u64::from(std::process::id()) << 32))
    }
}

/// The conversation the composer writes into before a thread exists.
pub fn new_conversation() -> SurfaceRef {
    SurfaceRef::new("conversation", "new")
}

pub struct AppSession {
    id: SessionId,
    pub companion: Companion,
    pub presentation: Presentation,
    pub surfaces: Surfaces,
    /// Reduced motion, as the system last reported it.
    pub still: bool,
    /// Authored behaviors, running for as long as the session.
    pub behaviors: Behaviors,
    /// What capabilities measure for behaviors.
    pub signals: Registry,
    face_seed: u32,
    typing: Typing,
}

/// The composer's length and rate as the last frame saw them.
#[derive(Default)]
struct Typing {
    length: usize,
    rate: f64,
    at: f64,
}

fn composer() -> Owner {
    Owner("composer".into())
}

fn theo_owner() -> Owner {
    Owner("theo".into())
}

impl AppSession {
    /// A session restored from what was saved. `home` is the presence's
    /// frame when none was saved, `workspace` likewise.
    /// `seed` gives the face and the behaviors their variation; `catalog`
    /// is every behavior kept so far, whose active ones start again.
    pub fn new(
        saved: Saved,
        home: Frame,
        workspace: Frame,
        now: f64,
        seed: u32,
        catalog: Catalog,
    ) -> Self {
        let mut companion = Companion::new(FaceOptions {
            now,
            seed,
            ..FaceOptions::default()
        });
        companion.show(companion::Scope::default());
        let presentation = Presentation::new(
            saved.form,
            saved.presence.unwrap_or(home),
            saved.workspace.unwrap_or(workspace),
        );
        let mut surfaces = Surfaces::default();
        surfaces.open(new_conversation());
        if let Some(session) = surfaces.get_mut(&new_conversation()) {
            // A new conversation has nothing to load.
            session.loaded("", None);
        }
        for draft in saved.drafts {
            surfaces.recover(draft);
        }
        surfaces.bring_to_front(&new_conversation());
        let mut signals = Registry::default();
        // Both names are namespaced and owned here; neither can be taken.
        let _ = signals.register(TYPING, &composer(), TYPING_LIFETIME_MS);
        let _ = signals.register(FORM, &theo_owner(), f64::INFINITY);
        Self {
            id: SessionId::fresh(),
            companion,
            presentation,
            surfaces,
            still: false,
            behaviors: Behaviors::new(seed, catalog),
            signals,
            face_seed: seed,
            typing: Typing::default(),
        }
    }

    /// A seed of its own for a new session, so each run varies.
    pub fn fresh_seed() -> u32 {
        let id = SessionId::fresh().0;
        (id ^ (id >> 32)) as u32
    }

    /// A session at default frames, for tests and headless runs.
    pub fn headless() -> Self {
        let home = Frame {
            x: 0,
            y: 0,
            w: PRESENCE_SIZE.0,
            h: PRESENCE_SIZE.1,
        };
        let workspace = Frame {
            x: 0,
            y: 0,
            w: WORKSPACE_SIZE.0,
            h: WORKSPACE_SIZE.1,
        };
        Self::new(
            Saved::default(),
            home,
            workspace,
            0.0,
            1,
            Catalog::default(),
        )
    }

    pub fn id(&self) -> SessionId {
        self.id
    }

    /// Asks for another presentation, with who asked.
    pub fn request(&mut self, request: Request, cause: Cause, now: f64) -> Outcome {
        self.presentation.step(request, cause, now, self.still)
    }

    /// Moves time on: the body's change between forms.
    pub fn tick(&mut self, now: f64) -> Vec<Effect> {
        self.presentation.tick(now)
    }

    /// Defines or revises an authored behavior. The route every author
    /// takes: the agent, the person, a file in the behaviors folder.
    pub fn author(&mut self, name: &Name, source: &str, by: &str) -> Result<u32, String> {
        self.behaviors.author(name, source, by)
    }

    pub fn stop_behavior(&mut self, name: &Name) {
        self.behaviors.stop(name, &mut self.companion);
    }

    /// Records from here on, for replay.
    pub fn start_capture(&mut self, now: f64) {
        self.behaviors.start_capture(now, self.face_seed);
    }

    /// One frame of authored behavior: the session's own measurements
    /// taken, then every behavior run against the signals as they stand.
    pub fn behave(&mut self, now: f64, tx: &mut Tx) {
        self.measure(now);
        let signals = self.signals.current(now);
        self.behaviors
            .frame(now, signals, &mut self.companion, &mut self.surfaces, tx);
    }

    fn measure(&mut self, now: f64) {
        let form = if self.presentation.shows_workspace() {
            self.presentation.progress(now)
        } else {
            0.0
        };
        let _ = self.signals.set(FORM, &theo_owner(), form, now);
        if !self.presentation.workspace_interactive() {
            // No composer to measure: the reading goes stale on its own.
            self.typing = Typing::default();
            return;
        }
        let length = self
            .surfaces
            .front()
            .map_or(0, |front| front.edit.text.chars().count());
        let seconds = ((now - self.typing.at) / 1000.0).max(1e-3);
        let typed = if self.typing.at > 0.0 {
            length.abs_diff(self.typing.length) as f64 / seconds
        } else {
            0.0
        };
        self.typing.rate += (typed - self.typing.rate) * TYPING_EASE;
        self.typing.length = length;
        self.typing.at = now;
        let _ = self.signals.set(TYPING, &composer(), self.typing.rate, now);
    }

    /// The face as it is at `now`.
    pub fn face(&mut self, now: f64, dark: bool) -> FaceFrame {
        self.companion.set_still(self.still);
        FaceFrame::of(&mut self.companion, now, dark)
    }

    /// What to remember: the form, both frames and every unsaved draft.
    pub fn saved(&self) -> Saved {
        Saved {
            form: self.presentation.form(),
            presence: Some(self.presentation.presence_frame()),
            workspace: Some(self.presentation.workspace_frame()),
            drafts: self.surfaces.drafts(),
        }
    }

    pub fn form(&self) -> Form {
        self.presentation.form()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::Draft;

    #[test]
    fn changing_form_keeps_the_session_its_companion_and_its_drafts() {
        let mut session = AppSession::headless();
        let id = session.id();
        session.request(Request::Summon, Cause::User, 0.0).unwrap();
        session
            .surfaces
            .front_mut()
            .unwrap()
            .edit
            .insert("half a thought");
        // Something only this face has: a hue no new face starts with.
        session.companion.face.tint(123.0);
        session.companion.face.notice("greeted", 1.0);
        session.request(Request::Expand, Cause::User, 10.0).unwrap();
        for t in (10..600).step_by(16) {
            session.tick(t as f64);
            session.face(t as f64, true);
        }
        session
            .request(Request::Collapse, Cause::User, 700.0)
            .unwrap();
        session.tick(1_100.0);
        assert_eq!(session.id(), id);
        assert_eq!(session.form(), Form::Presence);
        assert_eq!(
            session.surfaces.get(&new_conversation()).unwrap().edit.text,
            "half a thought"
        );
        assert_eq!(
            session.companion.face.hue, 123.0,
            "the same face, not a new one"
        );
    }

    #[test]
    fn a_restart_restores_the_form_frames_and_drafts() {
        let mut session = AppSession::headless();
        session.request(Request::Expand, Cause::User, 0.0).unwrap();
        session.surfaces.front_mut().unwrap().edit.insert("unsent");
        let saved = session.saved();
        assert_eq!(saved.form, Form::Workspace);
        assert_eq!(
            saved.drafts,
            vec![Draft {
                reference: new_conversation(),
                text: "unsent".into()
            }]
        );
        let home = saved.presence.unwrap();
        let ws = saved.workspace.unwrap();
        let restored = AppSession::new(saved, home, ws, 0.0, 1, Catalog::default());
        assert!(restored.presentation.workspace_interactive());
        assert_eq!(restored.surfaces.front().unwrap().edit.text, "unsent");
        assert_ne!(restored.id(), session.id(), "a new run is a new session");
    }

    #[test]
    fn typing_is_measured_only_while_the_composer_can_take_it() {
        let mut session = AppSession::headless();
        let mut tx = Tx::default();
        session.request(Request::Summon, Cause::User, 0.0).unwrap();
        session.behave(16.0, &mut tx);
        assert_eq!(
            session.signals.read(TYPING, 16.0),
            None,
            "no composer in the presence"
        );
        assert_eq!(session.signals.read(FORM, 16.0), Some(0.0));

        session.still = true;
        session.request(Request::Expand, Cause::User, 20.0).unwrap();
        session.tick(20.0);
        session.behave(32.0, &mut tx);
        session
            .surfaces
            .front_mut()
            .unwrap()
            .edit
            .insert("hello there");
        session.behave(132.0, &mut tx);
        let rate = session.signals.read(TYPING, 132.0).unwrap();
        assert!(
            rate > 1.0,
            "eleven characters in a tenth of a second reads as typing: {rate}"
        );
        assert_eq!(session.signals.read(FORM, 132.0), Some(1.0));
    }
}
