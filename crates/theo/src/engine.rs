//! Theo: one face. Everything that moves it goes through here, whoever asks:
//! the run's liveness, the agent's programs, the reaction rules, the person's
//! hand. It draws nothing; `advance` fills a buffer of grains that a renderer
//! draws, and `head` says how the visor sits.
//!
//! The face owns its rig, matter, book, memory, rules, player and signals,
//! and reads the time from the host: `set_now` before anything that happens,
//! `advance` for each frame.
use crate::autonomy::Autonomy;
use crate::book::{Book, By, Entry};
use crate::channels::clamp;
use crate::feature::Shape;
use crate::json::Json;
use crate::layers::{Layer, Manner, Rig};
use crate::memory::{FaceEvent, Memory, Shelf, matches};
use crate::particles::{Field, GRAIN_STRIDE, Matter, oklch};
use crate::player::{Ending, Invocation, NoStage, Player, Scene, Stage};
use crate::program::{Context, Place, Refusal, Thing, compile};
use crate::random::{Random, round};
use crate::reactions::{Reactions, Rule};
use crate::signals::Signals;
use crate::tables::{Identity, PARAM_COUNT, PARAMS, Side, param_index, tables};

/// What the agent is doing, from run truth and nothing the agent says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    Idle,
    Attend,
    Work,
    Ask,
    Done,
}

impl Liveness {
    pub fn name(self) -> &'static str {
        match self {
            Liveness::Idle => "idle",
            Liveness::Attend => "attend",
            Liveness::Work => "work",
            Liveness::Ask => "ask",
            Liveness::Done => "done",
        }
    }

    pub fn parse(name: &str) -> Option<Liveness> {
        [
            Liveness::Idle,
            Liveness::Attend,
            Liveness::Work,
            Liveness::Ask,
            Liveness::Done,
        ]
        .into_iter()
        .find(|liveness| liveness.name() == name)
    }

    /// The mood performance a liveness settles into. Idle and done settle into none.
    fn mood(self) -> Option<&'static str> {
        match self {
            Liveness::Attend => Some("attend"),
            Liveness::Work => Some("focus"),
            Liveness::Ask => Some("ask"),
            Liveness::Idle | Liveness::Done => None,
        }
    }
}

/// The manner the agent authored for its turn. Every field is optional.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Expression {
    pub energy: Option<f64>,
    pub breath: Option<f64>,
    pub tilt: Option<f64>,
    pub gaze: Option<(f64, f64)>,
    pub hue: Option<f64>,
}

/// How the head sits, for whatever draws the visor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Head {
    pub tilt: f64,
    pub scale: f64,
    pub dx: f64,
    pub dy: f64,
    pub squash: f64,
}

/// How much leaves each side of the face, 0 to 1, for whoever draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emitting {
    pub left: f64,
    pub right: f64,
}

/// The side of the figure that struck something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    pub fn name(self) -> &'static str {
        match self {
            Edge::Left => "left",
            Edge::Right => "right",
            Edge::Top => "top",
            Edge::Bottom => "bottom",
        }
    }

    pub fn parse(name: &str) -> Option<Edge> {
        [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom]
            .into_iter()
            .find(|edge| edge.name() == name)
    }
}

/// The numbers a person may set for how the face moves inside its holder.
/// (The web's other settings are for the figure's place in the window, which
/// is not this crate's.)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feel {
    /// How much the face leans into travel and looks ahead of it: 1 is as designed. 0 to 2.5.
    pub lean: f64,
    /// How far the visor trails inside its holder when the speed changes: 1 is as designed. 0 to 2.5.
    pub trail: f64,
    /// How quickly the face gets over a knock, per second. 2 to 20.
    pub recover: f64,
}

impl Default for Feel {
    fn default() -> Self {
        Self {
            lean: 1.0,
            trail: 1.0,
            recover: 7.5,
        }
    }
}

impl Feel {
    /// The same feel with each number brought inside its bounds; one that is no number is as shipped.
    pub fn bounded(self) -> Feel {
        let rest = Feel::default();
        let within = |value: f64, min: f64, max: f64, rest: f64| {
            if value.is_finite() {
                value.max(min).min(max)
            } else {
                rest
            }
        };
        Feel {
            lean: within(self.lean, 0.0, 2.5, rest.lean),
            trail: within(self.trail, 0.0, 2.5, rest.trail),
            recover: within(self.recover, 2.0, 20.0, rest.recover),
        }
    }
}

/// How a performance is asked for.
#[derive(Clone, Debug, Default)]
pub struct How {
    /// The layer it plays on; the performance layer unless said.
    pub layer: Option<Layer>,
    pub owner: String,
    pub args: Vec<(String, f64)>,
    pub intensity: Option<f64>,
    pub seed: Option<u32>,
    /// A thing the program names that the host does not show refuses it.
    pub strict: bool,
    /// What it sets stays when it ends, whether or not the program says so.
    pub keep: bool,
}

impl How {
    pub fn by(owner: &str) -> How {
        How {
            owner: owner.to_owned(),
            ..How::default()
        }
    }

    fn on(layer: Layer, owner: &str) -> How {
        How {
            layer: Some(layer),
            ..How::by(owner)
        }
    }
}

/// What a request to perform came to when it was sound.
#[derive(Clone, Debug, PartialEq)]
pub struct Performed {
    pub invocation: Invocation,
    /// What the checker changed rather than refused.
    pub notes: Vec<String>,
}

pub struct FaceOptions {
    pub identity: Option<Identity>,
    pub seed: u32,
    /// Where definitions and the days baseline are kept.
    pub shelf: Option<Box<dyn Shelf>>,
    /// The host's clock as the face begins, in milliseconds.
    pub now: f64,
}

impl Default for FaceOptions {
    fn default() -> Self {
        Self {
            identity: None,
            seed: 20_261_111,
            shelf: None,
            now: 0.0,
        }
    }
}

/// Idle this long, the face sleeps.
pub const SLEEP_AFTER_MS: f64 = 90_000.0;
/// With motion reduced, what leaves the face's sides does not follow the sound: it is this much while a signal it is tied to is over this, and nothing otherwise.
const STEADY_OVER: f64 = 0.05;
const STEADY_AMOUNT: f64 = 0.6;
const STEP: f64 = 1.0 / 120.0;
/// The longest one `frame` moves the face on, in milliseconds: a host that was away does not make it catch up.
const LONGEST_FRAME: f64 = 100.0;
/// How far a poke reaches into the matter, in face units.
const REACH: f64 = 14.0;

/// What the figure's motion in its window does to the face. Speeds are face units per second.
mod sway {
    /// Motion faster than this reads as this.
    pub const FASTEST: f64 = 2400.0;
    /// How quickly the face takes up a change of motion, and how slowly the visor catches up with it, per second.
    pub const QUICK: f64 = 14.0;
    pub const SLOW: f64 = 5.0;
    /// Degrees of lean per unit of sideways speed, before the person's setting.
    pub const LEAN: f64 = 0.011;
    /// Gaze per unit of speed.
    pub const AHEAD: f64 = 0.0004;
    /// Face units of trailing per unit of change in speed, before the person's setting.
    pub const TRAIL: f64 = 0.006;
}

/// Something the eyes are drawn to, and how much.
#[derive(Clone, Copy, Debug, Default)]
struct Draw {
    x: f64,
    y: f64,
    weight: f64,
    target: f64,
}

impl Draw {
    fn set(&mut self, gaze: Option<(f64, f64)>) {
        match gaze {
            None => self.target = 0.0,
            Some((x, y)) => {
                self.x = clamp(x, -1.0, 1.0);
                self.y = clamp(y, -1.0, 1.0);
                self.target = 1.0;
            }
        }
    }
}

/// How the figure is moving in its window, in face units per second: as told this frame, smoothed, and smoothed slowly.
#[derive(Clone, Copy, Debug, Default)]
struct Motion {
    x: f64,
    y: f64,
    fresh: bool,
    vx: f64,
    vy: f64,
    slow_x: f64,
    slow_y: f64,
}

/// A knock that is dying away: which way it threw the head, and which eye it closed on.
#[derive(Clone, Copy, Debug, Default)]
struct Knock {
    x: f64,
    y: f64,
    squash: f64,
    eye: f64,
    amount: f64,
}

/// One feature's channel names, kept so a frame builds none.
#[derive(Clone, Debug)]
struct Named {
    id: String,
    side: Side,
    eye: bool,
    /// One per param, in `tables::PARAMS` order.
    channels: Vec<String>,
}

/// The places of the params the face itself lays things over.
#[derive(Clone, Copy, Debug)]
struct Lids {
    lid: usize,
    low: usize,
    lid_tilt: usize,
    low_bow: usize,
}

/// The shelf as the parts that keep things take it.
fn kept(shelf: &mut Option<Box<dyn Shelf>>) -> Option<&mut dyn Shelf> {
    match shelf {
        Some(shelf) => Some(&mut **shelf),
        None => None,
    }
}

/// The player's stage: the host's, with bursts sent to the face's own matter.
struct Inner<'a> {
    host: &'a mut dyn Stage,
    field: &'a mut Field,
}

impl Stage for Inner<'_> {
    fn gaze_at(&self, place: &Place) -> Option<(f64, f64)> {
        self.host.gaze_at(place)
    }
    fn go(&mut self, place: &Place, size: Option<f64>, over: f64) {
        self.host.go(place, size, over);
    }
    fn burst(&mut self, x: f64, y: f64, strength: f64) {
        self.field.burst(x, y, strength, REACH);
    }
}

/// Runs `$body` with `$scene`, the scene the player plays in, borrowed from the face's own parts.
macro_rules! with_scene {
    ($face:ident, |$scene:ident| $body:expr) => {{
        let mut inner = Inner {
            host: &mut *$face.stage,
            field: &mut $face.field,
        };
        let mut $scene = Scene {
            rig: &mut $face.rig,
            stage: &mut inner,
            signals: &$face.signals,
            now: $face.now,
        };
        $body
    }};
}

pub struct Face {
    pub identity: Identity,
    /// The hue of the matter, in OKLCH degrees: the identity's own until the host gives its theme's. `matter.hue` turns from here.
    pub hue: f64,
    pub rig: Rig,
    pub field: Field,
    pub book: Book,
    pub memory: Memory,
    pub reactions: Reactions,
    pub player: Player,
    /// The grains as last painted, `GRAIN_STRIDE` numbers each.
    pub buffer: Vec<f32>,
    /// What the host measures as it happens, for programs that follow it.
    pub signals: Signals,
    /// How the face moves inside its holder, as the person set it.
    pub feel: Feel,
    pub asleep: bool,
    autonomy: Autonomy,
    shelf: Option<Box<dyn Shelf>>,
    stage: Box<dyn Stage>,
    now: f64,
    shapes: Vec<Shape>,
    names: Vec<Named>,
    lids: Lids,
    matter: Matter,
    time: f64,
    last: Option<f64>,
    liveness: Liveness,
    idle_since: f64,
    expressed: Vec<String>,
    follow: Draw,
    /// Where the figure is about to go, as a gaze: the eyes get there first.
    bound: Draw,
    /// Something the host shows that moved and has its eye.
    seen: Draw,
    motion: Motion,
    knock: Knock,
    seeds: u32,
    /// The event that ends the form the face has taken, while it has one.
    form_until: Option<String>,
}

impl Face {
    pub fn new(options: FaceOptions) -> Self {
        let identity = options
            .identity
            .unwrap_or_else(|| tables().identity.clone());
        let seed = options.seed;
        let rig = Rig::new(&identity.features);
        let field = Field::new(&identity, seed);
        let shelf = options.shelf;
        let book = Book::new(shelf.as_deref());
        let memory = Memory::new(shelf.as_deref());
        let reactions = Reactions::new(Random::new(seed ^ 0x9e37_79b9), shelf.as_deref());
        let player = Player::new(&rig);
        let place = |name: &str| param_index(name).expect("a param the face lays things over");
        let mut face = Self {
            hue: identity.hue,
            buffer: vec![0.0; field.count() * GRAIN_STRIDE],
            identity,
            rig,
            field,
            book,
            memory,
            reactions,
            player,
            signals: Signals::new(),
            feel: Feel::default(),
            asleep: false,
            autonomy: Autonomy::new(Random::new(seed ^ 0x85eb_ca6b)),
            shelf,
            stage: Box::new(NoStage),
            now: options.now,
            shapes: Vec::new(),
            names: Vec::new(),
            lids: Lids {
                lid: place("lid"),
                low: place("low"),
                lid_tilt: place("lidTilt"),
                low_bow: place("lowBow"),
            },
            matter: Matter {
                cohesion: 0.0,
                jitter: 0.0,
                drift: 0.0,
                size: 1.0,
                glow: 1.0,
                hue: 0.0,
            },
            time: 0.0,
            last: None,
            liveness: Liveness::Idle,
            idle_since: options.now,
            expressed: Vec::new(),
            follow: Draw::default(),
            bound: Draw::default(),
            seen: Draw::default(),
            motion: Motion::default(),
            knock: Knock::default(),
            seeds: seed,
            form_until: None,
        };
        face.advance(0.0);
        face
    }

    /// Says where the face is shown, so it can look at things and move.
    pub fn attach(&mut self, stage: Option<Box<dyn Stage>>) {
        self.stage = stage.unwrap_or_else(|| Box::new(NoStage));
    }

    /// Tells the face the time, in milliseconds. Events are dated by it.
    pub fn set_now(&mut self, now: f64) {
        self.now = now;
    }

    pub fn now(&self) -> f64 {
        self.now
    }

    /// No motion for effect: the face still changes, without flourish.
    pub fn set_still(&mut self, still: bool) {
        self.rig.still = still;
    }

    pub fn still(&self) -> bool {
        self.rig.still
    }

    pub fn liveness(&self) -> Liveness {
        self.liveness
    }

    // Performing ------------------------------------------------------------

    /// Checks a program and, if it is sound, plays it on a layer. The input
    /// may be a book name (a string), a program, a list of steps or one step.
    pub fn perform(&mut self, input: &Json, how: How) -> Result<Performed, Refusal> {
        let named = match input.str() {
            Some(name) => {
                let entry = self.book.get(name).ok_or_else(|| Refusal {
                    path: "use".to_owned(),
                    reason: format!("The book has no performance \"{name}\"."),
                })?;
                Some((entry.reference(), entry.program.clone()))
            }
            None => None,
        };
        let seed = how.seed.unwrap_or_else(|| self.next_seed());
        let checked = {
            let book = &self.book;
            let stage = &*self.stage;
            let lookup = |reference: &str| book.lookup(reference);
            let thing = |thing: &Thing| stage.thing(thing);
            let mut context = Context::new(&self.rig.channels);
            context.lookup = Some(&lookup);
            context.seed = Some(seed);
            context.args = how.args;
            context.intensity = how.intensity;
            if stage.knows_things() {
                context.thing = Some(&thing);
            }
            context.strict = how.strict;
            compile(
                named.as_ref().map_or(input, |(_, program)| program),
                &context,
            )
        };
        let mut score = checked?;
        let layer = how.layer.unwrap_or(Layer::Performance);
        if layer != Layer::Mood {
            self.rouse();
        }
        let notes = score.notes.clone();
        score.keep |= how.keep;
        let name = named.as_ref().map(|(name, _)| name.as_str());
        let invocation = with_scene!(self, |scene| self
            .player
            .play(&mut scene, score, layer, &how.owner, name));
        Ok(Performed { invocation, notes })
    }

    /// Plays a performance from the book by name.
    pub fn perform_named(&mut self, name: &str, how: How) -> Result<Performed, Refusal> {
        self.perform(&Json::Str(name.to_owned()), how)
    }

    /// How an invocation ended; `None` while it still plays.
    pub fn ending(&self, id: &str) -> Option<Ending> {
        self.player.ending(id)
    }

    fn next_seed(&mut self) -> u32 {
        self.seeds = self
            .seeds
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.seeds
    }

    fn stop(&mut self, layer: Layer) {
        with_scene!(self, |scene| self.player.stop(
            &mut scene,
            layer,
            Ending::Stopped
        ));
    }

    /// What the host measured this frame, by signal name: how loud the sound
    /// it is playing is, and the like. Programs that follow a signal move with it.
    pub fn feed(&mut self, name: &str, value: f64) {
        self.signals.feed(name, value);
    }

    // Definitions -----------------------------------------------------------

    /// Adds a performance to the book, or a new version of one.
    pub fn define(
        &mut self,
        name: &str,
        program: &Json,
        note: &str,
        by: By,
    ) -> Result<Entry, Refusal> {
        self.book.define(
            name,
            program,
            note,
            by,
            &self.rig.channels,
            kept(&mut self.shelf),
        )
    }

    /// Removes an added performance, or one version of it.
    pub fn forget(&mut self, reference: &str) -> Result<(), String> {
        self.book.remove(reference, kept(&mut self.shelf))
    }

    /// Adds or replaces a reaction rule.
    pub fn define_rule(&mut self, input: &Json, by: By) -> Result<Rule, Refusal> {
        self.reactions.define(
            input,
            by,
            &self.rig.channels,
            &self.book,
            kept(&mut self.shelf),
        )
    }

    /// Removes an added rule; a built-in one it replaced comes back.
    pub fn forget_rule(&mut self, id: &str) -> bool {
        self.reactions.remove(id, kept(&mut self.shelf))
    }

    // Events ----------------------------------------------------------------

    /// Something happened. The face remembers it and may react; what it does
    /// depends on what else has happened, so the same event twice is not
    /// promised the same face.
    pub fn notice(&mut self, kind: &str, weight: f64) -> Option<Invocation> {
        let event = FaceEvent {
            kind: kind.to_owned(),
            at: self.now,
            weight,
        };
        let reading = self.memory.note(&event, kept(&mut self.shelf));
        if kind.starts_with("person.") {
            self.idle_since = event.at;
        }
        // What a form was waiting for has come.
        if self
            .form_until
            .as_deref()
            .is_some_and(|until| matches(until, kind))
        {
            self.unform();
        }
        // Something that will last has begun: the face takes a form for it, beneath whatever else it does, asleep or not.
        let lasting = self.reactions.form(
            &event,
            &reading,
            &self.memory.mood,
            &self.memory,
            &self.book,
        );
        if let Some(lasting) = lasting {
            let how = How {
                intensity: Some(lasting.intensity),
                keep: true,
                ..How::on(Layer::Form, &format!("rule:{}", lasting.rule))
            };
            if let Ok(performed) = self.perform(&lasting.does, how) {
                self.form_until = lasting.until;
                return Some(performed.invocation);
            }
        }
        if self.asleep {
            // Only the person or a run wakes it; the window resizing does not, nor its own body settling.
            if !kind.starts_with("env.") && !kind.starts_with("body.") {
                self.rouse();
            }
            return None;
        }
        // A deliberate performance is not talked over.
        if self.player.active(Layer::Performance).is_some()
            || self.player.active(Layer::Hand).is_some()
        {
            return None;
        }
        let reaction = self.reactions.react(
            &event,
            &reading,
            &self.memory.mood,
            &self.memory,
            &self.book,
        )?;
        let how = How {
            intensity: Some(reaction.intensity),
            ..How::on(Layer::Reaction, &format!("rule:{}", reaction.rule))
        };
        self.perform(&reaction.does, how)
            .ok()
            .map(|performed| performed.invocation)
    }

    /// Lets go of the form the face has taken, if it has one: what it was a form for is over.
    pub fn unform(&mut self) {
        self.form_until = None;
        if self.player.holding(Layer::Form).is_none() {
            return;
        }
        self.stop(Layer::Form);
        self.idle_since = self.now;
    }

    /// Follows the run: each liveness settles the face into its own manner.
    pub fn set_liveness(&mut self, liveness: Liveness) {
        if liveness == self.liveness {
            return;
        }
        let before = self.liveness;
        self.liveness = liveness;
        if liveness == Liveness::Idle {
            self.idle_since = self.now;
        } else {
            self.rouse();
        }
        match liveness.mood() {
            Some(mood) => {
                let _ = self.perform_named(mood, How::on(Layer::Mood, "liveness"));
            }
            None if !self.asleep => self.stop(Layer::Mood),
            None => {}
        }
        if liveness == Liveness::Work && before != Liveness::Ask {
            self.notice("run.start", 1.0);
        }
        if liveness == Liveness::Ask {
            self.notice("run.ask", 1.0);
        }
        if liveness == Liveness::Done {
            self.notice("run.done", 1.0);
        }
        // Working has begun, or begun again after a question, or is over whatever came next: what lasts for as long
        // as the work does starts and ends here. Said last, so the events a rule waits for in order stay next to each other.
        if liveness == Liveness::Work {
            self.notice("run.work", 1.0);
        }
        if before == Liveness::Work {
            self.notice("run.rest", 1.0);
        }
    }

    /// Whether a run is at work now.
    pub fn working(&self) -> bool {
        self.liveness == Liveness::Work
    }

    /// The manner the agent authored for its turn, laid on the mood layer.
    pub fn express(&mut self, expression: Option<&Expression>) {
        let expressed = std::mem::take(&mut self.expressed);
        self.rig
            .release(Layer::Mood, Some(&expressed), Manner::default());
        let Some(expression) = expression else {
            return;
        };
        let hue = self.hue;
        let mut hold = |channel: &str, value: f64| {
            if self
                .rig
                .hold(Layer::Mood, channel, value, Manner::over(500.0))
                .is_some()
            {
                self.expressed.push(channel.to_owned());
            }
        };
        if let Some(tilt) = expression.tilt {
            hold("face.tilt", tilt * 14.0);
        }
        if let Some((x, y)) = expression.gaze {
            hold("gaze.x", x);
            hold("gaze.y", y);
        }
        if let Some(energy) = expression.energy {
            hold("matter.jitter", 0.03 + 0.22 * clamp(energy, 0.0, 1.0));
            hold("auto.saccade", 0.5 + 1.6 * clamp(energy, 0.0, 1.0));
        }
        if let Some(breath) = expression.breath {
            hold("auto.breath", breath);
        }
        if let Some(turned) = expression.hue {
            hold("matter.hue", ((turned - hue + 540.0) % 360.0) - 180.0);
        }
    }

    /// Wakes the face if it sleeps.
    pub fn rouse(&mut self) {
        self.idle_since = self.now;
        if !self.asleep {
            return;
        }
        self.asleep = false;
        self.stop(Layer::Mood);
        if let Some(mood) = self.liveness.mood() {
            let _ = self.perform_named(mood, How::on(Layer::Mood, "liveness"));
        }
        let _ = self.perform_named("wake", How::on(Layer::Reaction, "liveness"));
    }

    // The person's hand ------------------------------------------------------

    /// The pointer is near: the eyes follow it. A gaze, -1..1; `None` lets go.
    pub fn follow_pointer(&mut self, gaze: Option<(f64, f64)>) {
        self.follow.set(gaze);
    }

    /// Something the host shows is moving and worth a look. The eyes go with
    /// it, over the pointer but not over where the figure itself is bound. A
    /// gaze, -1..1; `None` when it has stopped.
    pub fn watch(&mut self, gaze: Option<(f64, f64)>) {
        self.seen.set(gaze);
    }

    /// A tap at a point of the face, in face units: the eye on that side flinches, whatever else the face makes of it.
    pub fn poke(&mut self, x: f64, y: f64) -> Option<Invocation> {
        self.field.burst(x, y, 70.0, REACH);
        self.flinch(if x.abs() < 4.0 { 0.0 } else { x.signum() }, 0.7);
        self.notice("person.poke", 1.0)
    }

    /// Picked up: the eyes go wide while it is carried.
    pub fn grab(&mut self) {
        self.rouse();
        self.stop(Layer::Hand);
        let manner = Manner {
            over: Some(140.0),
            bounce: Some(0.4),
        };
        for (channel, value) in [
            ("eyes.h", 15.5),
            ("eyes.round", 2.8),
            ("face.squash", -0.06),
            ("auto.blink", 0.0),
        ] {
            for name in self.rig.channels.expand(channel) {
                self.rig.hold(Layer::Hand, &name, value, manner);
            }
        }
    }

    /// Carried by this much, in face units per frame: it leans into the motion and its matter lags.
    pub fn carry(&mut self, dx: f64, dy: f64) {
        self.field.shove(dx * 0.6, dy * 0.6);
        self.rig.hold(
            Layer::Hand,
            "face.tilt",
            clamp(dx * 2.2, -24.0, 24.0),
            Manner::over(120.0),
        );
    }

    fn open_hand(&mut self) {
        let manner = Manner {
            over: Some(420.0),
            bounce: Some(0.5),
        };
        self.rig.release(Layer::Hand, None, manner);
    }

    /// Put down.
    pub fn drop_down(&mut self) -> Option<Invocation> {
        self.open_hand();
        self.notice("person.drop", 1.0)
    }

    /// Let go while moving: thrown. `force` is 0 for barely to 1 for as hard as a throw gets.
    pub fn toss(&mut self, force: f64) -> Option<Invocation> {
        self.open_hand();
        self.notice("person.throw", 0.6 + 0.8 * clamp(force, 0.0, 1.0))
    }

    // The body ----------------------------------------------------------------
    // What the figure's motion in its window does to the face. None of it is
    // evidence of a run: liveness comes from the runs alone (`set_liveness`).

    /// The figure moved this far over `seconds`, in face units: the matter
    /// lags, the head leans into the motion, the visor trails a change of
    /// pace and the eyes look a little where it is going.
    pub fn sway(&mut self, dx: f64, dy: f64, seconds: f64) {
        if self.rig.still || !(seconds > 0.0) {
            return;
        }
        self.field
            .shove(clamp(dx, -6.0, 6.0) * 0.35, clamp(dy, -6.0, 6.0) * 0.35);
        self.motion.x = clamp(dx / seconds, -sway::FASTEST, sway::FASTEST);
        self.motion.y = clamp(dy / seconds, -sway::FASTEST, sway::FASTEST);
        self.motion.fresh = true;
    }

    /// The figure is about to go somewhere: the eyes turn that way first. A gaze, -1..1; `None` when it has arrived.
    pub fn heading(&mut self, gaze: Option<(f64, f64)>) {
        self.bound.set(gaze);
    }

    /// The figure struck something on this side of it: an edge of its window,
    /// or something named by `against`. `force` is 0 for a touch to 1 for as
    /// hard as it can be thrown. The head is knocked back from what it met and
    /// the eye on that side shuts for a moment; what else the face makes of it
    /// is the rules', and they tell an edge (`body.bump.<side>`) from a thing
    /// (`body.meet.<against>`).
    pub fn bump(&mut self, side: Edge, force: f64, against: Option<&str>) -> Option<Invocation> {
        let hard = clamp(force, 0.0, 1.0);
        let across = match side {
            Edge::Left => -1.0,
            Edge::Right => 1.0,
            _ => 0.0,
        };
        let down = match side {
            Edge::Top => -1.0,
            Edge::Bottom => 1.0,
            _ => 0.0,
        };
        self.field.burst(
            (across * self.identity.width) / 2.0,
            (down * self.identity.height) / 2.0,
            50.0 + 110.0 * hard,
            26.0,
        );
        if !self.rig.still {
            self.knock.x = -across * (2.0 + 5.0 * hard);
            self.knock.y = -down * (2.0 + 4.0 * hard);
            // Flattened against the edge it met.
            self.knock.squash = (if across == 0.0 { 1.0 } else { -1.0 }) * (0.05 + 0.11 * hard);
            self.flinch(across, 0.5 + 0.5 * hard);
        }
        let kind = match against {
            Some(against) => format!("body.meet.{against}"),
            None => format!("body.bump.{}", side.name()),
        };
        self.notice(&kind, 0.6 + 0.8 * hard)
    }

    /// One eye shuts for a moment, or both a little when it came from neither side.
    fn flinch(&mut self, eye: f64, amount: f64) {
        if self.rig.still {
            return;
        }
        self.knock.eye = eye;
        self.knock.amount = self.knock.amount.max(clamp(amount, 0.0, 1.0));
    }

    /// Lays this frame's share of the figure's motion over the rig.
    fn carry_motion(&mut self, dt: f64) {
        let feel = self.feel.bounded();
        let (lids, asleep) = (self.lids, self.asleep);
        let Self {
            rig,
            motion,
            knock,
            bound,
            signals,
            names,
            ..
        } = self;
        let quick = 1.0 - (-dt * sway::QUICK).exp();
        motion.vx += ((if motion.fresh { motion.x } else { 0.0 }) - motion.vx) * quick;
        motion.vy += ((if motion.fresh { motion.y } else { 0.0 }) - motion.vy) * quick;
        signals.feed("body.speed", motion.vx.hypot(motion.vy) / sway::FASTEST);
        let slow = 1.0 - (-dt * sway::SLOW).exp();
        motion.slow_x += (motion.vx - motion.slow_x) * slow;
        motion.slow_y += (motion.vy - motion.slow_y) * slow;
        bound.weight += (bound.target - bound.weight) * (1.0 - (-dt * 10.0).exp());
        let fade = (-dt * feel.recover).exp();
        knock.x *= fade;
        knock.y *= fade;
        knock.squash *= fade;
        knock.amount *= fade;
        if rig.still {
            return;
        }
        if motion.vx.abs() + motion.vy.abs() > 0.5 {
            rig.offset(
                "face.tilt",
                clamp(motion.vx * sway::LEAN * feel.lean, -16.0, 16.0),
            );
            rig.offset(
                "gaze.x",
                clamp(motion.vx * sway::AHEAD * feel.lean, -0.3, 0.3),
            );
            rig.offset(
                "gaze.y",
                clamp(motion.vy * sway::AHEAD * feel.lean, -0.3, 0.3),
            );
        }
        // Speeding up, the visor is left a little behind; slowing, it runs a little on.
        let lag_x = clamp(
            (motion.slow_x - motion.vx) * sway::TRAIL * feel.trail,
            -5.0,
            5.0,
        );
        let lag_y = clamp(
            (motion.slow_y - motion.vy) * sway::TRAIL * feel.trail,
            -5.0,
            5.0,
        );
        if lag_x.abs() + lag_y.abs() + knock.x.abs() + knock.y.abs() > 0.02 {
            rig.offset("face.dx", lag_x + knock.x);
            rig.offset("face.dy", lag_y + knock.y);
        }
        if knock.squash.abs() > 0.002 {
            rig.offset("face.squash", knock.squash);
        }
        if knock.amount > 0.01 {
            for feature in names.iter().filter(|feature| feature.eye) {
                // The eye it came at shuts most; the other barely.
                let share = if knock.eye == 0.0 {
                    0.45
                } else if feature.side.sign() == knock.eye {
                    1.0
                } else {
                    0.15
                };
                rig.offset(&feature.channels[lids.lid], 0.7 * knock.amount * share);
                rig.offset(&feature.channels[lids.low], 0.25 * knock.amount * share);
            }
        }
        if bound.weight > 0.01 && !asleep {
            rig.offset("gaze.x", bound.x * 0.8 * bound.weight);
            rig.offset("gaze.y", bound.y * 0.8 * bound.weight);
        }
    }

    // Frames ----------------------------------------------------------------

    /// Advances to `now` (milliseconds) and paints the buffer. The first call moves nothing on.
    pub fn frame(&mut self, now: f64) -> &[f32] {
        let elapsed = self
            .last
            .map_or(0.0, |last| (now - last).max(0.0).min(LONGEST_FRAME));
        self.last = Some(now);
        self.now = now;
        self.advance(elapsed / 1000.0);
        &self.buffer
    }

    /// Advances by a span of seconds in fixed steps, whatever the frame rate, and paints the buffer.
    pub fn advance(&mut self, seconds: f64) {
        let steps = (seconds / STEP - 1e-6).ceil().max(1.0);
        let dt = seconds / steps;
        for _ in 0..steps as u32 {
            self.tick(dt);
        }
        // Motion is told afresh each frame; untold, the figure is not moving.
        self.motion.fresh = false;
        self.paint();
    }

    /// Gives the matter the hue of the host's theme, and shows it at once.
    pub fn tint(&mut self, hue: f64) {
        self.hue = hue;
        self.paint();
    }

    /// Writes the grains as they stand into the buffer, in the hue the face has now.
    fn paint(&mut self) {
        let colour = oklch(
            self.identity.lightness,
            self.identity.chroma,
            self.hue + self.matter.hue,
        );
        self.field
            .write(&mut self.buffer, &self.matter, self.identity.grain, colour);
    }

    /// Keeps the channel names of the features as they stand.
    fn know_features(&mut self) {
        let features = || self.rig.channels.features();
        if features()
            .map(|feature| feature.id.as_str())
            .eq(self.names.iter().map(|named| named.id.as_str()))
        {
            return;
        }
        self.names = features()
            .map(|feature| Named {
                id: feature.id.clone(),
                side: feature.side,
                eye: feature.id.starts_with("eye"),
                channels: PARAMS
                    .iter()
                    .map(|param| format!("{}.{param}", feature.id))
                    .collect(),
            })
            .collect();
    }

    fn tick(&mut self, dt: f64) {
        let now = self.now;
        if !self.asleep
            && self.liveness == Liveness::Idle
            && now - self.idle_since > SLEEP_AFTER_MS
            && self.player.active(Layer::Performance).is_none()
            && self.player.holding(Layer::Form).is_none()
        {
            self.asleep = true;
            let _ = self.perform_named("sleep", How::on(Layer::Mood, "liveness"));
        }
        self.time += dt;
        self.rig.begin_frame();
        with_scene!(self, |scene| self.player.tick(&mut scene, dt * 1000.0));
        self.autonomy.tick(&mut self.rig, dt);
        self.know_features();
        self.colour_by_mood(now);
        self.follow.weight += (self.follow.target - self.follow.weight) * (1.0 - (-dt * 7.0).exp());
        self.seen.weight += (self.seen.target - self.seen.weight) * (1.0 - (-dt * 9.0).exp());
        // Bound for somewhere, the eyes are on where it is going; watching something, on that; only otherwise on the pointer.
        let watching = self.seen.weight * (1.0 - self.bound.weight);
        if watching > 0.01 && !self.asleep {
            self.rig.offset("gaze.x", self.seen.x * 0.8 * watching);
            self.rig.offset("gaze.y", self.seen.y * 0.8 * watching);
        }
        let free = self.follow.weight * (1.0 - self.bound.weight) * (1.0 - self.seen.weight);
        if free > 0.01 && !self.asleep {
            self.rig.offset("gaze.x", self.follow.x * 0.7 * free);
            self.rig.offset("gaze.y", self.follow.y * 0.7 * free);
        }
        self.carry_motion(dt);
        self.rig.step(dt);
        self.know_features();
        self.pose();
        self.field
            .step(&mut self.shapes, &self.matter, dt, self.time);
    }

    /// The mood shows faintly all the time: a pleased face smiles a little, a wound-up one trembles.
    fn colour_by_mood(&mut self, now: f64) {
        if self.rig.still || self.asleep {
            return;
        }
        let mood = self.memory.settle(now);
        // In a form the eyes are something else: a smile on a loudspeaker is a dent in it.
        let formed = self.player.holding(Layer::Form).is_some();
        let pleased = (mood.valence - 0.2).max(0.0);
        let sour = (-mood.valence).max(0.0);
        if !formed {
            for feature in self.names.iter().filter(|feature| feature.eye) {
                self.rig
                    .offset(&feature.channels[self.lids.low_bow], pleased * 0.35);
                self.rig
                    .offset(&feature.channels[self.lids.lid_tilt], sour * 0.35);
            }
        }
        self.rig
            .offset("matter.jitter", (mood.arousal - 0.3).max(0.0) * 0.12);
        self.rig.offset("auto.blink", (mood.arousal - 0.25) * 0.8);
    }

    /// Reads the rig into shapes and matter, with the gaze turned into where the features sit.
    fn pose(&mut self) {
        let Self {
            rig,
            shapes,
            names,
            matter,
            ..
        } = self;
        // A shape is its place in the list, as on the web: what a globe has turned stays with the place, not the feature.
        shapes.truncate(names.len());
        while shapes.len() < names.len() {
            shapes.push(Shape::rest(Side::Centre));
        }
        let gaze_x = rig.read("gaze.x");
        let gaze_y = rig.read("gaze.y");
        for (shape, feature) in shapes.iter_mut().zip(names.iter()) {
            for param in 0..PARAM_COUNT {
                shape.set(param, rig.read(&feature.channels[param]));
            }
            shape.side = feature.side.sign();
            let depth = if feature.eye { 1.0 } else { 0.7 };
            let side = if shape.side == 0.0 { 1.0 } else { shape.side };
            // Looking sideways slides the features across and narrows the far one a little.
            shape.x += side * gaze_x * 8.0 * depth;
            shape.y += gaze_y * 6.0 * depth;
            if shape.side != 0.0 {
                shape.w *= 1.0 - 0.1 * gaze_x * shape.side;
            }
            shape.h *= 1.0 - 0.06 * gaze_y.abs();
            // With motion reduced a globe is a ball of dots that does not turn.
            if rig.still {
                shape.spin = 0.0;
                shape.stir = 0.0;
            }
        }
        matter.cohesion = rig.read("matter.cohesion");
        matter.jitter = if rig.still {
            0.0
        } else {
            rig.read("matter.jitter")
        };
        matter.drift = if rig.still {
            0.0
        } else {
            rig.read("matter.drift")
        };
        matter.size = rig.read("matter.size");
        matter.glow = rig.read("matter.glow");
        matter.hue = rig.read("matter.hue");
    }

    /// The feature shapes as they are now, for a still mark and for tests.
    pub fn shapes_now(&self) -> &[Shape] {
        &self.shapes
    }

    pub fn head(&self) -> Head {
        Head {
            tilt: self.rig.read("face.tilt"),
            scale: self.rig.read("face.scale"),
            dx: self.rig.read("face.dx"),
            dy: self.rig.read("face.dy"),
            squash: self.rig.read("face.squash"),
        }
    }

    /// How much leaves each side of the face: it is outside the visor, so it is no matter of the face's.
    pub fn emitting(&self) -> Emitting {
        if self.rig.still {
            // With motion reduced nothing follows the sound. That something is sounding is still shown: steadily, there or not.
            let steady = |channel: &str| {
                if self
                    .player
                    .tied(channel)
                    .into_iter()
                    .any(|signal| self.signals.read(signal) > STEADY_OVER)
                {
                    STEADY_AMOUNT
                } else {
                    0.0
                }
            };
            return Emitting {
                left: steady("emit.left"),
                right: steady("emit.right"),
            };
        }
        Emitting {
            left: self.rig.read("emit.left"),
            right: self.rig.read("emit.right"),
        }
    }

    /// What the face is doing, in a few words, for a status a screen reader gets.
    pub fn words(&self) -> &str {
        if self.asleep {
            return "asleep";
        }
        let active = self
            .player
            .active(Layer::Hand)
            .or_else(|| self.player.active(Layer::Performance))
            .or_else(|| self.player.active(Layer::Reaction))
            .or_else(|| self.player.holding(Layer::Form));
        let name = active
            .and_then(|invocation| invocation.name.as_deref())
            .and_then(|name| name.split('@').next())
            .filter(|name| !name.is_empty());
        if let Some(name) = name {
            return name;
        }
        match self.liveness {
            Liveness::Idle => "at rest",
            liveness => liveness.mood().unwrap_or(liveness.name()),
        }
    }

    /// What the face is doing and has lately done, as data a model can read.
    pub fn state(&self) -> Json {
        let text = |text: &str| Json::Str(text.to_owned());
        let numbers = |pairs: Vec<(String, f64)>| {
            Json::Obj(
                pairs
                    .into_iter()
                    .map(|(name, value)| (name, Json::Num(value)))
                    .collect(),
            )
        };
        let hundredth = |value: f64| Json::Num(round(value * 100.0) / 100.0);
        let mut fields = vec![
            ("liveness".to_owned(), text(self.liveness.name())),
            ("asleep".to_owned(), Json::Bool(self.asleep)),
            ("doing".to_owned(), text(self.words())),
        ];
        let form = self
            .player
            .holding(Layer::Form)
            .and_then(|form| form.name.as_deref());
        if let (Some(name), Some(until)) = (form, self.form_until.as_deref()) {
            fields.push((
                "form".to_owned(),
                Json::Obj(vec![
                    ("name".to_owned(), text(name)),
                    ("until".to_owned(), text(until)),
                ]),
            ));
        }
        let signals = self.signals.away();
        if !signals.is_empty() {
            fields.push(("signals".to_owned(), numbers(signals)));
        }
        fields.push((
            "features".to_owned(),
            Json::List(
                self.rig
                    .channels
                    .features()
                    .map(|feature| {
                        Json::Obj(vec![
                            ("id".to_owned(), text(&feature.id)),
                            ("side".to_owned(), text(feature.side.name())),
                        ])
                    })
                    .collect(),
            ),
        ));
        fields.push(("away".to_owned(), numbers(self.rig.away())));
        fields.push((
            "mood".to_owned(),
            Json::Obj(vec![
                ("arousal".to_owned(), hundredth(self.memory.mood.arousal)),
                ("valence".to_owned(), hundredth(self.memory.mood.valence)),
                ("focus".to_owned(), hundredth(self.memory.mood.focus)),
            ]),
        ));
        let recent = &self.memory.recent;
        fields.push((
            "recentEvents".to_owned(),
            Json::List(
                recent[recent.len().saturating_sub(8)..]
                    .iter()
                    .map(|event| text(&event.kind))
                    .collect(),
            ),
        ));
        let history = &self.player.history;
        fields.push((
            "performances".to_owned(),
            Json::List(
                history[history.len().saturating_sub(6)..]
                    .iter()
                    .map(|invocation| {
                        let mut fields = vec![("id".to_owned(), text(&invocation.id))];
                        if let Some(name) = &invocation.name {
                            fields.push(("name".to_owned(), text(name)));
                        }
                        fields.push(("owner".to_owned(), text(&invocation.owner)));
                        fields.push(("layer".to_owned(), text(invocation.layer.name())));
                        fields.push(("ms".to_owned(), Json::Num(round(invocation.ms))));
                        fields.push((
                            "ending".to_owned(),
                            text(invocation.ending.map_or("running", Ending::name)),
                        ));
                        Json::Obj(fields)
                    })
                    .collect(),
            ),
        ));
        Json::Obj(fields)
    }
}

/// A place as a gaze when nothing the host shows can say better.
pub fn plain_gaze(place: &Place) -> Option<(f64, f64)> {
    match place {
        Place::Thing(_) => None,
        Place::Spot { x, y } => Some((*x, *y)),
        Place::Named("centre" | "person") => Some((0.0, 0.0)),
        Place::Named("composer") => Some((0.15, 0.75)),
        Place::Named("text") => Some((0.7, 0.1)),
        Place::Named("artifact") => Some((0.6, -0.3)),
        Place::Named(_) => None,
    }
}

/// A stage that knows only the plain places: enough for a face shown on its own.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlainStage;

impl Stage for PlainStage {
    fn gaze_at(&self, place: &Place) -> Option<(f64, f64)> {
        plain_gaze(place)
    }
    fn go(&mut self, _place: &Place, _size: Option<f64>, _over: f64) {}
    fn burst(&mut self, _x: f64, _y: f64, _strength: f64) {}
}
