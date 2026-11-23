//! The session's behaviors: what the catalog says is active, running in the
//! host, and what they asked for applied to Theo's face, drawings and
//! surfaces. Their lifetime is the session's, not a reply's: they keep
//! running between turns and through every change of form.
//!
//! A capture records what a run started from and every input it was given,
//! so the same local behavior can be run again from it with those inputs
//! held fixed (see [`replay`]).

use std::collections::BTreeMap;

use companion::Companion;
use rune::Tx;
use serde::{Deserialize, Serialize};
use theo::book::By;
use theo::engine::{FaceOptions, How};

use super::host::{Command, Host};
use super::{Catalog, Name, RUNTIME};
use crate::drawing::Drawing;
use crate::surfaces::{SurfaceRef, Surfaces};

/// How many refusals are kept for an author to read.
const NOTES_KEPT: usize = 32;

pub struct Behaviors {
    pub catalog: Catalog,
    host: Host,
    drawings: BTreeMap<Name, Drawing>,
    /// A script's performance handles, to the face's invocation ids.
    handles: BTreeMap<(Name, u32), String>,
    /// What the host or the face refused, newest last.
    pub notes: Vec<(Name, String)>,
    /// Behaviors whose record changed since the store last saw them.
    changed: Vec<Name>,
    capture: Option<Capture>,
}

impl Behaviors {
    /// Starts every behavior the catalog says was running. One that no
    /// longer starts is marked failed; the rest run regardless.
    pub fn new(seed: u32, catalog: Catalog) -> Self {
        let mut behaviors = Self {
            catalog,
            host: Host::new(seed),
            drawings: BTreeMap::new(),
            handles: BTreeMap::new(),
            notes: Vec::new(),
            changed: Vec::new(),
            capture: None,
        };
        let running: Vec<(Name, u32, String)> = behaviors
            .catalog
            .running()
            .map(|(name, r)| (name.clone(), r.number, r.source.clone()))
            .collect();
        for (name, number, source) in running {
            if let Err(reason) = behaviors.host.start(&name, number, &source) {
                behaviors.catalog.outcome(&name, number, Err(reason));
                behaviors.changed.push(name);
            }
        }
        behaviors
    }

    pub fn seed(&self) -> u32 {
        self.host.seed()
    }

    /// Defines `name`, or revises it, from `source`. The same source as the
    /// latest revision is no new revision. Returns the revision's number, or
    /// why it did not start, in which case the previous one still runs.
    pub fn author(&mut self, name: &Name, source: &str, by: &str) -> Result<u32, String> {
        if let Some(latest) = self.catalog.revisions(name).last()
            && latest.source == source
        {
            return match &latest.status {
                super::Status::Failed(reason) => Err(reason.clone()),
                super::Status::Stopped | super::Status::Superseded => {
                    self.restart(name, latest.number, source)
                }
                _ => Ok(latest.number),
            };
        }
        if let Some(capture) = &mut self.capture {
            capture.input(Input::Author {
                name: name.clone(),
                source: source.to_owned(),
                by: by.to_owned(),
            });
        }
        let number = self.catalog.author(name, source, by);
        self.restart(name, number, source)
    }

    fn restart(&mut self, name: &Name, number: u32, source: &str) -> Result<u32, String> {
        let started = self.host.start(name, number, source);
        self.catalog.outcome(name, number, started.clone());
        self.changed.push(name.clone());
        started.map(|()| number)
    }

    /// Stops `name`: its performances end and its drawing goes.
    pub fn stop(&mut self, name: &Name, companion: &mut Companion) {
        if let Some(capture) = &mut self.capture {
            capture.input(Input::Stop { name: name.clone() });
        }
        self.host.stop(name);
        self.catalog.stop(name);
        self.drawings.remove(name);
        let ended: Vec<_> = self
            .handles
            .keys()
            .filter(|(n, _)| n == name)
            .cloned()
            .collect();
        for key in ended {
            if let Some(id) = self.handles.remove(&key) {
                companion.face.cancel(&id);
            }
        }
        self.changed.push(name.clone());
    }

    /// One frame: the behaviors run against the signals as they stand at
    /// `now_ms`, and what they asked is applied.
    pub fn frame(
        &mut self,
        now_ms: f64,
        signals: BTreeMap<String, f64>,
        companion: &mut Companion,
        surfaces: &mut Surfaces,
        tx: &mut Tx,
    ) {
        if let Some(capture) = &mut self.capture {
            capture.frame(now_ms, &signals);
        }
        for (name, value) in &signals {
            if theo::signals::is_signal(name) {
                companion.face.feed(name, *value);
            }
        }
        for command in self.host.poll(now_ms / 1000.0, signals, tx) {
            self.apply(command, companion, surfaces);
        }
    }

    fn apply(&mut self, command: Command, companion: &mut Companion, surfaces: &mut Surfaces) {
        let face = &mut companion.face;
        match command {
            Command::Perform {
                behavior,
                handle,
                program,
                intensity,
            } => {
                let how = How {
                    intensity,
                    ..How::by(&format!("behavior:{}", behavior.as_str()))
                };
                match face.perform(&program, how) {
                    Ok(performed) => {
                        self.handles
                            .insert((behavior, handle), performed.invocation.id);
                    }
                    Err(refusal) => self.note(
                        behavior,
                        format!("perform: {}: {}", refusal.path, refusal.reason),
                    ),
                }
            }
            Command::Define {
                behavior,
                name,
                program,
                note,
            } => {
                if let Err(refusal) = face.define(&name, &program, &note, By::Agent) {
                    self.note(
                        behavior,
                        format!("define {name}: {}: {}", refusal.path, refusal.reason),
                    );
                }
            }
            Command::Cancel { behavior, handle } => {
                if let Some(id) = self.handles.remove(&(behavior, handle)) {
                    face.cancel(&id);
                }
            }
            Command::Notice { kind, weight } => {
                face.notice(&kind, weight);
            }
            Command::Tint(hue) => face.tint(hue),
            Command::Draw { behavior, drawing } => {
                if drawing.is_empty() {
                    self.drawings.remove(&behavior);
                } else {
                    self.drawings.insert(behavior, drawing);
                }
            }
            Command::Open { kind, id } => {
                surfaces.open(SurfaceRef::new(kind, id));
            }
            Command::Refused { behavior, reason } => self.note(behavior, reason),
        }
    }

    fn note(&mut self, behavior: Name, reason: String) {
        self.notes.push((behavior, reason));
        let over = self.notes.len().saturating_sub(NOTES_KEPT);
        self.notes.drain(..over);
    }

    /// What the running behaviors draw, in name order.
    pub fn drawings(&self) -> impl Iterator<Item = (&Name, &Drawing)> {
        self.drawings.iter()
    }

    pub fn running(&self) -> impl Iterator<Item = &Name> {
        self.host.running()
    }

    pub fn errors(&self, name: &Name) -> Vec<String> {
        self.host.errors(name)
    }

    /// The behaviors to save since this was last asked.
    pub fn take_changed(&mut self) -> Vec<Name> {
        let mut changed = std::mem::take(&mut self.changed);
        changed.sort();
        changed.dedup();
        changed
    }

    /// Begins recording from this point. Behaviors already running are
    /// recorded as the inputs that started them.
    pub fn start_capture(&mut self, origin_ms: f64, face_seed: u32) {
        let mut capture = Capture {
            format: 1,
            engine: Engine::current(),
            seed: self.seed(),
            face_seed,
            origin_ms,
            frames: Vec::new(),
            pending: Vec::new(),
        };
        for (name, r) in self.catalog.running() {
            capture.input(Input::Author {
                name: name.clone(),
                source: r.source.clone(),
                by: r.by.clone(),
            });
        }
        self.capture = Some(capture);
    }

    pub fn take_capture(&mut self) -> Option<Capture> {
        self.capture.take()
    }
}

/// What a run depended on that is not in its inputs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Engine {
    pub app: String,
    pub runtime: String,
    pub face: String,
}

impl Engine {
    pub fn current() -> Self {
        Self {
            app: concat!("theo-desktop ", env!("CARGO_PKG_VERSION")).to_owned(),
            runtime: RUNTIME.to_owned(),
            face: "theo (crates/theo, same revision as the app)".to_owned(),
        }
    }
}

/// An input a run was given between two frames.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Input {
    Author {
        name: Name,
        source: String,
        by: String,
    },
    Stop {
        name: Name,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapturedFrame {
    pub at_ms: f64,
    pub inputs: Vec<Input>,
    /// Signals that changed since the frame before; `None` went stale.
    pub signals: BTreeMap<String, Option<f64>>,
}

/// A reproducible run: its starting point (a defined reset state: a new
/// face from `face_seed` at `origin_ms`, behaviors seeded from `seed`), the
/// exact sources it was given, and every frame's inputs and signals.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Capture {
    pub format: u32,
    pub engine: Engine,
    pub seed: u32,
    pub face_seed: u32,
    pub origin_ms: f64,
    pub frames: Vec<CapturedFrame>,
    #[serde(skip)]
    pending: Vec<Input>,
}

impl Capture {
    fn input(&mut self, input: Input) {
        self.pending.push(input);
    }

    fn frame(&mut self, at_ms: f64, signals: &BTreeMap<String, f64>) {
        let before = self.signals_at(self.frames.len());
        let mut changed: BTreeMap<String, Option<f64>> = signals
            .iter()
            .filter(|(k, v)| before.get(*k) != Some(*v))
            .map(|(k, v)| (k.clone(), Some(*v)))
            .collect();
        changed.extend(
            before
                .keys()
                .filter(|k| !signals.contains_key(*k))
                .map(|k| (k.clone(), None)),
        );
        self.frames.push(CapturedFrame {
            at_ms,
            inputs: std::mem::take(&mut self.pending),
            signals: changed,
        });
    }

    /// The signals as they stood before frame `index`.
    fn signals_at(&self, index: usize) -> BTreeMap<String, f64> {
        let mut signals = BTreeMap::new();
        for frame in &self.frames[..index] {
            for (k, v) in &frame.signals {
                match v {
                    Some(v) => signals.insert(k.clone(), *v),
                    None => signals.remove(k),
                };
            }
        }
        signals
    }
}

/// What a replay produced, to compare runs by: each frame's face channels
/// and drawings, and what was refused.
#[derive(Debug, PartialEq)]
pub struct Trace {
    pub frames: Vec<String>,
    pub notes: Vec<(Name, String)>,
}

/// Runs a capture again from its starting point with its inputs held fixed.
pub fn replay(capture: &Capture) -> Trace {
    let mut companion = Companion::new(FaceOptions {
        seed: capture.face_seed,
        now: capture.origin_ms,
        ..FaceOptions::default()
    });
    companion.show(companion::Scope::default());
    let mut surfaces = Surfaces::default();
    let mut behaviors = Behaviors::new(capture.seed, Catalog::default());
    let mut tx = Tx::default();
    let mut frames = Vec::with_capacity(capture.frames.len());
    for (i, frame) in capture.frames.iter().enumerate() {
        for input in &frame.inputs {
            match input {
                Input::Author { name, source, by } => {
                    let _ = behaviors.author(name, source, by);
                }
                Input::Stop { name } => behaviors.stop(name, &mut companion),
            }
        }
        let mut signals = capture.signals_at(i);
        for (k, v) in &frame.signals {
            match v {
                Some(v) => signals.insert(k.clone(), *v),
                None => signals.remove(k),
            };
        }
        behaviors.frame(frame.at_ms, signals, &mut companion, &mut surfaces, &mut tx);
        companion.frame(frame.at_ms);
        let drawings: Vec<_> = behaviors.drawings().collect();
        frames.push(format!("{:?} {drawings:?}", companion.face.state()));
    }
    Trace {
        frames,
        notes: behaviors.notes,
    }
}
