//! Theo on the desktop, less the window.
//!
//! A host has a window, a pointer, a clock, runs and sound. This crate has
//! none of them. It holds the one `Face`, the `Presence` that says which
//! activity it stands for, and the small state a host would otherwise keep
//! in its view: whether Theo is shown, whether its composer is open, the
//! scope it was called from. The host tells it what happened and when, and
//! draws what `frame` gives back.
//!
//! What it will not do: end work. Hiding and collapsing change what is seen.
//! `stop` names the activity a stop is for and hands that to the host, whose
//! owner of the work ends it and reports the outcome as an update.
use presence::{Activity, Applied, Presence, Shown};
use theo::engine::{Expression, Face, FaceOptions, How, Liveness, Profile};
use theo::layers::Layer;

/// Where Theo was called from, kept so that what is said next goes to the same place.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub conversation: Option<String>,
    pub environment: Option<String>,
    pub surface: Option<String>,
    pub target: Option<String>,
}

/// How the pointer calls Theo, as the person set it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Calling {
    /// Whether two right clicks in one place call it.
    pub double_right_click: bool,
    /// The longest between the two, in milliseconds.
    pub within_ms: f64,
    /// The furthest apart the two may land, in points.
    pub slop: f64,
}

impl Default for Calling {
    fn default() -> Self {
        Self {
            double_right_click: false,
            within_ms: 400.0,
            slop: 6.0,
        }
    }
}

/// A stop the person asked for: which run of which activity it is for. The host gives it to the owner of that work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopRequest {
    pub activity: String,
    pub generation: u64,
}

/// The form the face has for the primary activity, or why it has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formed {
    pub shown: Shown,
    /// The face's invocation of it, while there is one.
    pub invocation: Option<String>,
    /// Why the face would not take the form: the book has none by that name, say.
    pub refused: Option<String>,
}

pub struct Companion {
    pub face: Face,
    presence: Presence,
    formed: Option<Formed>,
    visible: bool,
    composer: bool,
    scope: Scope,
    calling: Calling,
    /// The last right click that did not call Theo: when and where.
    clicked: Option<(f64, f64, f64)>,
}

impl Companion {
    /// A desktop Theo. Sound is a loudspeaker here, never a mouth.
    pub fn new(options: FaceOptions) -> Self {
        Self {
            face: Face::new(FaceOptions {
                profile: Profile::Desktop,
                ..options
            }),
            presence: Presence::new(),
            formed: None,
            visible: false,
            composer: false,
            scope: Scope::default(),
            calling: Calling::default(),
            clicked: None,
        }
    }

    // What is going on ------------------------------------------------------

    /// Takes an account of an activity and gives the face the form of whichever is now primary.
    pub fn update(&mut self, activity: Activity) -> Applied {
        let applied = self.presence.apply(activity);
        self.settle();
        applied
    }

    /// Takes a snapshot in place of everything held, as after a reconnection. Returns how many were left out.
    pub fn restore(&mut self, snapshot: Vec<Activity>) -> usize {
        let left_out = self.presence.restore(snapshot);
        self.settle();
        left_out
    }

    pub fn presence(&self) -> &Presence {
        &self.presence
    }

    /// The form the face has for the primary activity, or why not.
    pub fn formed(&self) -> Option<&Formed> {
        self.formed.as_ref()
    }

    /// Makes the face's form the primary activity's. Called after anything that may have changed either.
    fn settle(&mut self) {
        let primary = self.presence.primary();
        let held = self
            .face
            .player
            .holding(Layer::Form)
            .map(|held| held.id.clone());
        if let Some(formed) = &self.formed {
            let same = primary.as_ref() == Some(&formed.shown);
            // Refused once is refused until something changes; asked again each frame it would only be refused again.
            let standing = formed.refused.is_some() || formed.invocation == held;
            if same && standing {
                return;
            }
            if let Some(id) = &formed.invocation {
                self.face.cancel(id);
            }
            self.formed = None;
        }
        let Some(shown) = primary else {
            return;
        };
        let how = How {
            layer: Some(Layer::Form),
            keep: true,
            ..How::by(&format!("activity:{}", shown.activity))
        };
        let (invocation, refused) = match self.face.perform_named(&shown.form, how) {
            Ok(performed) => (Some(performed.invocation.id), None),
            Err(refusal) => (None, Some(refusal.reason)),
        };
        self.formed = Some(Formed {
            shown,
            invocation,
            refused,
        });
    }

    /// The person asked for what is shown to stop. Nothing is ended here: this says what the stop is for.
    pub fn stop(&self) -> Option<StopRequest> {
        self.presence.primary().map(|shown| StopRequest {
            activity: shown.activity,
            generation: shown.generation,
        })
    }

    // The run ---------------------------------------------------------------

    /// Follows a run's liveness, then puts the primary activity's form back if the face's own rules took it.
    pub fn set_liveness(&mut self, liveness: Liveness) {
        self.face.set_liveness(liveness);
        self.settle();
    }

    /// The manner the agent wrote for its turn, or none to let go of it.
    pub fn express(&mut self, expression: Option<&Expression>) {
        self.face.express(expression);
    }

    /// Reduced motion: what is shown is the same, and gets there without the travel.
    pub fn set_still(&mut self, still: bool) {
        self.face.set_still(still);
    }

    // Seen or not -----------------------------------------------------------

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn composer_open(&self) -> bool {
        self.composer
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    /// Shows Theo, called from `scope`. What it was called from before is replaced.
    pub fn show(&mut self, scope: Scope) {
        self.visible = true;
        self.scope = scope;
    }

    /// Hides Theo and its composer. Whatever is going on goes on, and is there when it is shown again.
    pub fn hide(&mut self) {
        self.visible = false;
        self.composer = false;
    }

    /// Opens the composer, showing Theo if it was hidden; the scope stays as it was.
    pub fn open_composer(&mut self) {
        self.visible = true;
        self.composer = true;
    }

    pub fn close_composer(&mut self) {
        self.composer = false;
    }

    pub fn calling(&self) -> Calling {
        self.calling
    }

    pub fn set_calling(&mut self, calling: Calling) {
        self.calling = calling;
        self.clicked = None;
    }

    /// A right click at a time and place. Says whether it was the second of two that call Theo;
    /// the host then shows or hides it. A third click soon after begins again.
    pub fn right_click(&mut self, at_ms: f64, x: f64, y: f64) -> bool {
        if !self.calling.double_right_click {
            return false;
        }
        let called = self.clicked.is_some_and(|(then, px, py)| {
            let elapsed = at_ms - then;
            (0.0..=self.calling.within_ms).contains(&elapsed)
                && (x - px).hypot(y - py) <= self.calling.slop
        });
        self.clicked = if called { None } else { Some((at_ms, x, y)) };
        called
    }

    // Frames ----------------------------------------------------------------

    /// Advances to `now` (milliseconds) and gives the grains to draw.
    ///
    /// The primary activity's measurements are fed to the face first, each only while it is fresh.
    /// One that has gone stale reads as nothing: the face is never moved by a number nobody measured.
    pub fn frame(&mut self, now: f64) -> &[f32] {
        self.face.set_now(now);
        self.settle();
        if let Some(activity) = self
            .presence
            .primary()
            .and_then(|shown| self.presence.get(&shown.activity))
        {
            for signal in &activity.signals {
                let value = if signal.fresh(now) { signal.value } else { 0.0 };
                self.face.feed(&signal.name, value);
            }
        }
        self.face.frame(now)
    }
}
