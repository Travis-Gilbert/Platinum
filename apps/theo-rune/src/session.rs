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
use theo::engine::FaceOptions;

use crate::face::FaceFrame;
use crate::preferences::Saved;
use crate::presentation::{
    Cause, Effect, Form, Frame, Outcome, PRESENCE_SIZE, Presentation, Request, WORKSPACE_SIZE,
};
use crate::surfaces::{SurfaceRef, Surfaces};

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
}

impl AppSession {
    /// A session restored from what was saved. `home` is the presence's
    /// frame when none was saved, `workspace` likewise.
    pub fn new(saved: Saved, home: Frame, workspace: Frame, now: f64) -> Self {
        let mut companion = Companion::new(FaceOptions {
            now,
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
        Self {
            id: SessionId::fresh(),
            companion,
            presentation,
            surfaces,
            still: false,
        }
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
        Self::new(Saved::default(), home, workspace, 0.0)
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
        let restored = AppSession::new(saved, home, ws, 0.0);
        assert!(restored.presentation.workspace_interactive());
        assert_eq!(restored.surfaces.front().unwrap().edit.text, "unsent");
        assert_ne!(restored.id(), session.id(), "a new run is a new session");
    }
}
