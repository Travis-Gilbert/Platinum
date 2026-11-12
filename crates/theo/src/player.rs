//! Runs checked performances on the rig, one per layer, and remembers how
//! each ended. A newer performance on a layer interrupts the one there; one
//! that ends and keeps what it held (a mood, a sleep, a form) holds until the
//! layer is stopped or played on again.
//!
//! The player owns no rig, stage, signals or clock: each call is handed the
//! `Scene` it plays in, so the face that owns them stays their one owner.
use std::f64::consts::PI;

use crate::layers::{Layer, Manner, Rig};
use crate::program::{Cue, CueKind, Place, Score, Thing, Wave};
use crate::signals::{Signals, signal};
use crate::tables::FeatureDef;

/// What the player asks of wherever the face is shown.
pub trait Stage {
    /// Where a place is, as a gaze in -1..1 per axis; `None` when it cannot be found.
    fn gaze_at(&self, place: &Place) -> Option<(f64, f64)>;
    /// Moves the face in its window.
    fn go(&mut self, place: &Place, size: Option<f64>, over: f64);
    /// Kicks the matter.
    fn burst(&mut self, x: f64, y: f64, strength: f64);
    /// Whether the host can say what a named thing is. Without it, things are not looked for.
    fn knows_things(&self) -> bool {
        false
    }
    /// What a thing a program names comes to in the host: what there is to say of it, or `None` when nothing has the name.
    fn thing(&self, _thing: &Thing) -> Option<Vec<String>> {
        None
    }
}

/// A stage that shows the face nowhere: nothing to look at, nowhere to go.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoStage;

impl Stage for NoStage {
    fn gaze_at(&self, _place: &Place) -> Option<(f64, f64)> {
        None
    }
    fn go(&mut self, _place: &Place, _size: Option<f64>, _over: f64) {}
    fn burst(&mut self, _x: f64, _y: f64, _strength: f64) {}
}

/// What a call to the player plays in.
pub struct Scene<'a> {
    pub rig: &'a mut Rig,
    pub stage: &'a mut dyn Stage,
    pub signals: &'a Signals,
    /// The host's clock, in milliseconds.
    pub now: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    Completed,
    Interrupted,
    Stopped,
}

impl Ending {
    pub fn name(self) -> &'static str {
        match self {
            Ending::Completed => "completed",
            Ending::Interrupted => "interrupted",
            Ending::Stopped => "stopped",
        }
    }
}

/// One time a performance was played: who asked, with what draw, and how it ended.
#[derive(Clone, Debug, PartialEq)]
pub struct Invocation {
    /// Stable for the life of the player: `p1`, `p2`, ...
    pub id: String,
    /// The book name and version it came from, or `None` for one written on the spot.
    pub name: Option<String>,
    pub owner: String,
    pub layer: Layer,
    pub seed: u32,
    pub ms: f64,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    pub ending: Option<Ending>,
}

#[derive(Clone, Debug)]
struct WaveCue {
    at: f64,
    channel: String,
    amp: f64,
    period: f64,
    until: f64,
    shape: Wave,
    phase: f64,
}

#[derive(Clone, Debug)]
struct FollowCue {
    at: f64,
    channel: String,
    signal: String,
    amp: f64,
    until: Option<f64>,
}

#[derive(Clone, Debug)]
struct Playing {
    invocation: Invocation,
    score: Score,
    elapsed: f64,
    next: usize,
    waves: Vec<WaveCue>,
    /// Channels tied to a signal, for as long as each says or as long as this holds.
    follows: Vec<FollowCue>,
    held: Vec<String>,
    /// Features it put on the face, to be taken away when it lets go.
    added: Vec<String>,
}

const TAU: f64 = PI * 2.0;
/// How many invocations are remembered.
pub const HISTORY: usize = 12;
/// How long a tie to a signal takes to come in and to go out, in milliseconds.
const FOLLOW_EDGE: f64 = 140.0;

/// Whether a signal holds still for as long as a run does.
fn steady(name: &str) -> bool {
    signal(name).is_some_and(|spec| spec.steady)
}

fn add(set: &mut Vec<String>, name: &str) {
    if !set.iter().any(|item| item == name) {
        set.push(name.to_owned());
    }
}

/// Runs scores on a rig, one per layer, and remembers how each ended.
#[derive(Clone, Debug)]
pub struct Player {
    /// At most one per layer, in the order they began.
    playing: Vec<Playing>,
    /// Performances that ended and kept what they held, at most one per layer.
    kept: Vec<Playing>,
    /// The last `HISTORY` invocations, oldest first.
    pub history: Vec<Invocation>,
    count: u64,
    /// The features the face came with: no performance adds them and none takes them away.
    own: Vec<String>,
    /// Who put each added feature there, by invocation id. It leaves when the last of them lets go.
    users: Vec<(String, Vec<String>)>,
}

impl Player {
    pub fn new(rig: &Rig) -> Self {
        Self {
            playing: Vec::new(),
            kept: Vec::new(),
            history: Vec::new(),
            count: 0,
            own: rig
                .channels
                .features()
                .map(|feature| feature.id.clone())
                .collect(),
            users: Vec::new(),
        }
    }

    /// Starts a score on a layer, ending whatever played there.
    pub fn play(
        &mut self,
        scene: &mut Scene<'_>,
        score: Score,
        layer: Layer,
        owner: &str,
        name: Option<&str>,
    ) -> Invocation {
        self.stop(scene, layer, Ending::Interrupted);
        self.count += 1;
        let invocation = Invocation {
            id: format!("p{}", self.count),
            name: name.filter(|name| !name.is_empty()).map(str::to_owned),
            owner: owner.to_owned(),
            layer,
            seed: score.seed,
            ms: score.ms,
            started_at: scene.now,
            ended_at: None,
            ending: None,
        };
        self.playing.push(Playing {
            invocation: invocation.clone(),
            score,
            elapsed: 0.0,
            next: 0,
            waves: Vec::new(),
            follows: Vec::new(),
            held: Vec::new(),
            added: Vec::new(),
        });
        self.history.push(invocation.clone());
        if self.history.len() > HISTORY {
            self.history.remove(0);
        }
        // Cues at 0 take effect before the first frame, so a start is never a blank.
        self.advance(scene, layer, 0.0);
        invocation
    }

    /// How an invocation ended; `None` while it still plays. One too old to be remembered reads as completed.
    pub fn ending(&self, id: &str) -> Option<Ending> {
        if self
            .playing
            .iter()
            .any(|playing| playing.invocation.id == id)
        {
            return None;
        }
        Some(
            self.history
                .iter()
                .find(|invocation| invocation.id == id)
                .and_then(|invocation| invocation.ending)
                .unwrap_or(Ending::Completed),
        )
    }

    /// What is playing on a layer now.
    pub fn active(&self, layer: Layer) -> Option<&Invocation> {
        self.playing
            .iter()
            .find(|playing| playing.invocation.layer == layer)
            .map(|playing| &playing.invocation)
    }

    /// What holds a layer: what plays there, or what ended there and kept its hold.
    pub fn holding(&self, layer: Layer) -> Option<&Invocation> {
        self.active(layer).or_else(|| {
            self.kept
                .iter()
                .find(|playing| playing.invocation.layer == layer)
                .map(|playing| &playing.invocation)
        })
    }

    /// The signals a channel is tied to by whatever plays or holds, on any layer.
    pub fn tied(&self, channel: &str) -> Vec<&str> {
        self.playing
            .iter()
            .chain(&self.kept)
            .flat_map(|playing| &playing.follows)
            .filter(|cue| cue.channel == channel)
            .map(|cue| cue.signal.as_str())
            .collect()
    }

    /// Ends what plays on a layer and lets go of everything held there, kept or not.
    pub fn stop(&mut self, scene: &mut Scene<'_>, layer: Layer, ending: Ending) {
        if let Some(index) = self
            .kept
            .iter()
            .position(|playing| playing.invocation.layer == layer)
        {
            let mut kept = self.kept.remove(index);
            self.let_go(scene, layer, &mut kept);
        }
        let Some(index) = self
            .playing
            .iter()
            .position(|playing| playing.invocation.layer == layer)
        else {
            return;
        };
        let playing = self.playing.remove(index);
        self.finish(scene, playing, ending, false);
    }

    fn let_go(&mut self, scene: &mut Scene<'_>, layer: Layer, playing: &mut Playing) {
        scene
            .rig
            .release(layer, Some(&playing.held), Manner::over(380.0));
        for id in playing.added.drain(..) {
            if let Some(at) = self.users.iter().position(|(feature, _)| *feature == id) {
                let users = &mut self.users[at].1;
                users.retain(|user| *user != playing.invocation.id);
                if !users.is_empty() {
                    continue;
                }
                self.users.remove(at);
            }
            scene.rig.retire_feature(&id);
        }
    }

    /// Ends a performance that has already been taken out of `playing`.
    fn finish(&mut self, scene: &mut Scene<'_>, mut playing: Playing, ending: Ending, keep: bool) {
        playing.invocation.ended_at = Some(scene.now);
        playing.invocation.ending = Some(ending);
        if let Some(past) = self
            .history
            .iter_mut()
            .find(|invocation| invocation.id == playing.invocation.id)
        {
            past.ended_at = playing.invocation.ended_at;
            past.ending = playing.invocation.ending;
        }
        if keep {
            self.kept.push(playing);
        } else {
            let layer = playing.invocation.layer;
            self.let_go(scene, layer, &mut playing);
        }
    }

    /// Moves every performance on by `ms` and lays this frame's waves over the rig.
    pub fn tick(&mut self, scene: &mut Scene<'_>, ms: f64) {
        // What ended and kept its hold keeps its ties too: a form follows the sound for as long as it is one.
        // One that ends in this tick is put after these, and has already followed.
        let kept = self.kept.len();
        let mut layers = [None; 5];
        for (slot, playing) in layers.iter_mut().zip(&self.playing) {
            *slot = Some(playing.invocation.layer);
        }
        for layer in layers.into_iter().flatten() {
            self.advance(scene, layer, ms);
        }
        for held in &mut self.kept[..kept] {
            held.elapsed += ms;
            follow(scene, held);
        }
    }

    fn advance(&mut self, scene: &mut Scene<'_>, layer: Layer, ms: f64) {
        let Some(index) = self
            .playing
            .iter()
            .position(|playing| playing.invocation.layer == layer)
        else {
            return;
        };
        // Taken out while it runs, so a cue may reach the others; put back where it was unless it ends.
        let mut playing = self.playing.remove(index);
        playing.elapsed += ms;
        while playing.next < playing.score.cues.len() {
            if playing.score.cues[playing.next].at > playing.elapsed {
                break;
            }
            let cue = playing.score.cues[playing.next].clone();
            playing.next += 1;
            self.fire(scene, layer, &mut playing, cue);
        }
        if !scene.rig.still {
            let elapsed = playing.elapsed;
            playing.waves.retain(|wave| elapsed < wave.until);
            // Newest first, as the web lays them: the order sums are made in is part of the number.
            for wave in playing.waves.iter().rev() {
                let into = elapsed - wave.at;
                let left = wave.until - elapsed;
                // Ease in and out over a fifth of the wave, so it never starts or stops with a jump.
                let edge = ((wave.until - wave.at) * 0.2).max(40.0);
                let envelope = (into / edge).min(left / edge).min(1.0);
                let turn = into / wave.period + wave.phase;
                let level = match wave.shape {
                    Wave::Sine => (turn * TAU).sin(),
                    Wave::Square => {
                        if (turn * TAU).sin() >= 0.0 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    Wave::Noise => (turn * TAU).sin() * 0.6 + (turn * TAU * 2.71 + 1.3).sin() * 0.4,
                };
                scene.rig.offset(&wave.channel, wave.amp * envelope * level);
            }
        }
        follow(scene, &mut playing);
        if playing.elapsed >= playing.score.ms && playing.next >= playing.score.cues.len() {
            let keep = playing.score.keep;
            self.finish(scene, playing, Ending::Completed, keep);
        } else {
            self.playing.insert(index, playing);
        }
    }

    fn fire(&mut self, scene: &mut Scene<'_>, layer: Layer, playing: &mut Playing, cue: Cue) {
        match cue.kind {
            CueKind::Hold {
                channel,
                value,
                relative,
                over,
                bounce,
            } => {
                let value = if relative {
                    scene.rig.target(&channel) + value
                } else {
                    value
                };
                let manner = Manner {
                    over: Some(over),
                    bounce: Some(bounce),
                };
                if scene.rig.hold(layer, &channel, value, manner).is_some() {
                    add(&mut playing.held, &channel);
                }
            }
            CueKind::Wave {
                channel,
                amp,
                period,
                until,
                shape,
                phase,
            } => playing.waves.push(WaveCue {
                at: cue.at,
                channel,
                amp,
                period,
                until,
                shape,
                phase,
            }),
            CueKind::Follow {
                channel,
                signal,
                amp,
                until,
            } => playing.follows.push(FollowCue {
                at: cue.at,
                channel,
                signal,
                amp,
                until,
            }),
            CueKind::Look { place, over } => {
                let Some((x, y)) = scene.stage.gaze_at(&place) else {
                    return;
                };
                for (channel, value) in [("gaze.x", x), ("gaze.y", y)] {
                    scene.rig.hold(layer, channel, value, Manner::over(over));
                    add(&mut playing.held, channel);
                }
            }
            CueKind::Feature { id, side, remove } => {
                if remove {
                    // Said outright, it goes for everyone who had it.
                    for user in self.playing.iter_mut().chain(&mut self.kept) {
                        user.added.retain(|added| *added != id);
                    }
                    playing.added.retain(|added| *added != id);
                    self.users.retain(|(feature, _)| *feature != id);
                    scene.rig.retire_feature(&id);
                    return;
                }
                let def = FeatureDef {
                    id: id.clone(),
                    side,
                    rest: Vec::new(),
                };
                if scene.rig.add_feature(&def).is_err() {
                    return;
                }
                // Whether the feature was already there or on its way out, this performance now has it too.
                if self.own.contains(&id) {
                    return;
                }
                add(&mut playing.added, &id);
                match self.users.iter_mut().find(|(feature, _)| *feature == id) {
                    Some((_, users)) => add(users, &playing.invocation.id),
                    None => self.users.push((id, vec![playing.invocation.id.clone()])),
                }
            }
            CueKind::Go { place, size, over } => scene.stage.go(&place, size, over),
            CueKind::Burst { x, y, strength } => scene.stage.burst(x, y, strength),
            CueKind::Release { channels, over } => {
                let names = channels.clone().unwrap_or_else(|| playing.held.clone());
                scene.rig.release(layer, Some(&names), Manner::over(over));
                playing.held.retain(|held| !names.contains(held));
                // Letting go of a channel ends what it followed too, eased out as a tie with `for` is.
                let until = playing.elapsed + FOLLOW_EDGE;
                for tie in &mut playing.follows {
                    if channels
                        .as_ref()
                        .is_some_and(|channels| !channels.contains(&tie.channel))
                    {
                        continue;
                    }
                    if tie.until.is_none_or(|due| due > until) {
                        tie.until = Some(until);
                    }
                }
            }
        }
    }
}

/// Lays this frame's share of each signal a performance follows over the rig.
fn follow(scene: &mut Scene<'_>, playing: &mut Playing) {
    let elapsed = playing.elapsed;
    playing
        .follows
        .retain(|cue| cue.until.is_none_or(|until| elapsed < until));
    for cue in playing.follows.iter().rev() {
        // With motion reduced nothing follows what moves, but a tie still ends when it is due to. What holds for a whole run is still shown.
        if scene.rig.still && !steady(&cue.signal) {
            continue;
        }
        let into = (elapsed - cue.at) / FOLLOW_EDGE;
        let left = cue
            .until
            .map_or(1.0, |until| (until - elapsed) / FOLLOW_EDGE);
        let envelope = into.min(left).min(1.0).max(0.0);
        scene.rig.offset(
            &cue.channel,
            cue.amp * envelope * scene.signals.read(&cue.signal),
        );
    }
}
