//! The Rune application's state: the session, the clock, and the native
//! window the presentation's effects are applied to.
//!
//! Rune gives each window its own state; Theo has one window, so this is
//! the one place the session lives. Handlers and the poll take requests to
//! the session, which answers with effects; `apply` is the only code that
//! turns them into window calls.

use std::cell::Cell;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rune::{AppHandle, TextEdit, Tx, WindowHandle};

use crate::behavior::store::Library;
use crate::face::FaceFrame;
use crate::platform;
use crate::preferences::Writer;
use crate::presentation::{Cause, Effect, Form, Frame, Request};
use crate::session::AppSession;
use crate::surfaces::SurfaceRef;

/// The keys of the element each form focuses.
pub const FACE_KEY: &str = "theo.face";
pub const COMPOSER_KEY: &str = "theo.composer";

/// How often a settled window's frame is read back, to keep where the
/// person moved it.
const PLACEMENT_EVERY_MS: f64 = 400.0;
/// How often drafts are offered to the preferences writer.
const DRAFTS_EVERY_MS: f64 = 1_000.0;

pub struct Theo {
    pub session: AppSession,
    /// The poll clock, in milliseconds.
    pub now: f64,
    pub face: FaceFrame,
    /// What the view last read from the system, for the poll.
    pub dark: Cell<bool>,
    pub still: Cell<bool>,
    /// The application's own handle: shows, hides, focuses and moves the
    /// window on every host, before the native window exists and headless.
    app: Option<AppHandle<Theo>>,
    /// The native window, once made: only its level and its frame read back
    /// need it.
    window: Arc<OnceLock<WindowHandle>>,
    /// A level decided before the native window existed.
    float: Option<bool>,
    writer: Option<Writer>,
    /// Where behaviors are kept and authored from.
    library: Option<Library>,
    /// The application in front when Theo was summoned, to give focus back to.
    returns_to: Option<i32>,
    placement_read_at: f64,
    drafts_offered_at: f64,
    /// Effects decided before the first poll, or with no application to
    /// apply them to (tests and named scenes).
    pending: Vec<Effect>,
    /// Edits aimed at a surface that has since closed land here.
    scratch: TextEdit,
    /// The last thing worth saying aloud: a refusal, a shortcut not taken.
    pub status: String,
}

impl Theo {
    pub fn new(
        mut session: AppSession,
        app: Option<AppHandle<Theo>>,
        window: Arc<OnceLock<WindowHandle>>,
        writer: Option<Writer>,
        library: Option<Library>,
    ) -> Self {
        let pending = session
            .request(Request::Summon, Cause::Restore, 0.0)
            .unwrap_or_default();
        let face = session.face(0.0, true);
        Self {
            session,
            now: 0.0,
            face,
            dark: Cell::new(true),
            still: Cell::new(false),
            app,
            window,
            float: None,
            writer,
            library,
            returns_to: None,
            placement_read_at: 0.0,
            drafts_offered_at: 0.0,
            pending,
            scratch: TextEdit::default(),
            status: String::new(),
        }
    }

    /// A Theo with no native window or preferences file, for tests and
    /// headless snapshots.
    pub fn headless() -> Self {
        Self::new(
            AppSession::headless(),
            None,
            Arc::new(OnceLock::new()),
            None,
            None,
        )
    }

    /// Starts a change of form on the person's behalf with no window to
    /// apply it to, for named scenes and tests.
    pub fn begin(&mut self, request: Request) {
        if let Ok(effects) = self.session.request(request, Cause::User, self.now) {
            self.pending.extend(effects);
        }
    }

    /// The same, with motion reduced, so the change has settled at once.
    pub fn settle_in(&mut self, request: Request) {
        let still = self.session.still;
        self.session.still = true;
        self.begin(request);
        self.pending.extend(self.session.tick(self.now));
        self.session.still = still;
    }

    /// Asks the session for another presentation on the person's behalf.
    pub fn request(&mut self, request: Request, cause: Cause, tx: &mut Tx) {
        if request == Request::Summon && cause != Cause::Background {
            self.returns_to = platform::frontmost_other();
        }
        // The view may have read a new setting since the last poll.
        self.session.still = self.still.get();
        match self.session.request(request, cause, self.now) {
            Ok(effects) => self.apply(effects, tx),
            Err(refusal) => self.status = format!("{refusal:?}"),
        }
    }

    /// The global shortcut: summon Theo, or dismiss it when it is in front.
    pub fn toggle(&mut self, tx: &mut Tx) {
        let request = if self.session.presentation.visible() {
            Request::Dismiss
        } else {
            Request::Summon
        };
        self.request(request, Cause::User, tx);
    }

    pub fn expand(&mut self, tx: &mut Tx) {
        self.request(Request::Expand, Cause::User, tx);
    }

    pub fn collapse(&mut self, tx: &mut Tx) {
        self.request(Request::Collapse, Cause::User, tx);
    }

    /// The editor of `reference`'s session, wherever a view finds it.
    pub fn edit_of(&mut self, reference: &SurfaceRef) -> &mut TextEdit {
        match self.session.surfaces.get_mut(reference) {
            Some(session) => &mut session.edit,
            None => &mut self.scratch,
        }
    }

    /// Carries out the presentation's effects, in order.
    pub fn apply(&mut self, effects: Vec<Effect>, tx: &mut Tx) {
        let Some(app) = self.app.clone() else {
            // Focus needs no window; the rest waits for an application.
            for effect in &effects {
                if let Effect::Focus(form) = effect {
                    focus(*form, tx);
                }
            }
            self.pending.extend(effects);
            return;
        };
        for effect in effects {
            match effect {
                Effect::Place(frame) => {
                    tx.resize_window_instant((frame.w, frame.h));
                    app.set_position((frame.x, frame.y));
                }
                Effect::Float(on) => {
                    self.float = Some(on);
                    self.keep_level();
                }
                Effect::Show { activate } => {
                    app.show();
                    if activate {
                        app.focus();
                    }
                }
                Effect::Hide => app.hide(),
                Effect::Focus(form) => focus(form, tx),
                Effect::ReturnFocus => {
                    if let Some(pid) = self.returns_to.take() {
                        platform::return_to(pid);
                    }
                }
                Effect::Persist => self.persist(),
            }
        }
    }

    /// Gives the native window the level last decided, once it exists.
    fn keep_level(&mut self) {
        if let Some(window) = self.window.get()
            && let Some(on) = self.float.take()
        {
            window.set_always_on_top(on);
        }
    }

    fn persist(&mut self) {
        if let Some(writer) = &mut self.writer {
            writer.store(self.session.saved());
        }
    }

    /// One frame: the clock, the system's settings, the body's change, the
    /// face, and where the person has moved the window.
    pub fn poll(&mut self, t: f64, tx: &mut Tx) -> bool {
        self.now = t * 1000.0;
        self.session.still = self.still.get();
        if self.app.is_some() && !self.pending.is_empty() {
            let pending = std::mem::take(&mut self.pending);
            self.apply(pending, tx);
        }
        self.keep_level();
        let settled = self.session.tick(self.now);
        if !settled.is_empty() {
            self.apply(settled, tx);
        }
        self.author_from_inbox();
        self.session.behave(self.now, tx);
        self.keep_behaviors();
        self.face = self.session.face(self.now, self.dark.get());
        self.read_placement();
        if self.now - self.drafts_offered_at >= DRAFTS_EVERY_MS {
            self.drafts_offered_at = self.now;
            self.persist();
        }
        if self.session.presentation.visible() {
            tx.wake_after(Duration::ZERO);
        }
        true
    }

    /// Defines or revises what authors left in the behaviors folder.
    fn author_from_inbox(&mut self) {
        let Some(library) = &self.library else { return };
        let authored: Vec<_> = library.inbox.try_iter().collect();
        for a in authored {
            if let Err(reason) = self.session.author(&a.name, &a.source, &a.by) {
                self.status = format!("{} did not start: {reason}", a.name.as_str());
            }
        }
    }

    /// Authors everything in the behaviors folder at once, for the
    /// `authored` scene: a headless run shows what a folder holds.
    pub fn author_folder(&mut self) {
        let dir = crate::behavior::store::default_dir();
        for a in crate::behavior::store::scan(&dir, &mut Default::default()) {
            if let Err(reason) = self.session.author(&a.name, &a.source, &a.by) {
                eprintln!("theo: {} did not start: {reason}", a.name.as_str());
            }
        }
    }

    /// Hands every behavior whose record changed to the folder's writer.
    fn keep_behaviors(&mut self) {
        let changed = self.session.behaviors.take_changed();
        if let Some(library) = &self.library {
            for name in changed {
                library.writer.store(&name, &self.session.behaviors.catalog);
            }
        }
    }

    /// Keeps a settled form's frame where the person moved or resized it.
    fn read_placement(&mut self) {
        if self.now - self.placement_read_at < PLACEMENT_EVERY_MS {
            return;
        }
        self.placement_read_at = self.now;
        let Some(placement) = self.window.get().and_then(WindowHandle::placement) else {
            return;
        };
        let scale = placement.scale.max(1.0);
        let frame = Frame {
            x: placement.position.0,
            y: placement.position.1,
            w: (f64::from(placement.content_size.0) / scale) as f32,
            h: (f64::from(placement.content_size.1) / scale) as f32,
        };
        let known = self.session.presentation.frame();
        if same_frame(frame, known) || !self.session.presentation.visible() {
            return;
        }
        let _ = self
            .session
            .request(Request::Moved(frame), Cause::User, self.now);
        self.persist();
    }
}

fn focus(form: Form, tx: &mut Tx) {
    match form {
        Form::Presence => tx.focus(FACE_KEY, false),
        Form::Workspace => tx.focus(COMPOSER_KEY, false),
    }
}

/// Frames read back from the window server round their sizes; a point's
/// difference is no move.
fn same_frame(a: Frame, b: Frame) -> bool {
    a.x == b.x && a.y == b.y && (a.w - b.w).abs() < 1.0 && (a.h - b.h).abs() < 1.0
}
