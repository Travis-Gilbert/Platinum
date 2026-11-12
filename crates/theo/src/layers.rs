//! The face's numbers in motion. Each channel is a spring following the
//! highest layer that holds it, or its rest value when none does. Modifiers
//! (a blink, a breath, a wave) are laid over the spring for one frame at a
//! time, so they never fight a hold and never stick.
use crate::channels::{Channels, clamp};
use crate::ordered::Ordered;
use crate::random::round;
use crate::tables::FeatureDef;

/// Who may hold a channel, lowest first. A higher layer's hold wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    Mood,
    Form,
    Reaction,
    Performance,
    Hand,
}

pub const LAYERS: [Layer; 5] = [
    Layer::Mood,
    Layer::Form,
    Layer::Reaction,
    Layer::Performance,
    Layer::Hand,
];

impl Layer {
    pub fn slot(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Layer::Mood => "mood",
            Layer::Form => "form",
            Layer::Reaction => "reaction",
            Layer::Performance => "performance",
            Layer::Hand => "hand",
        }
    }

    pub fn parse(name: &str) -> Option<Layer> {
        LAYERS.into_iter().find(|layer| layer.name() == name)
    }
}

/// How a channel should get where it is told to go.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Manner {
    /// About how long the move takes, in milliseconds.
    pub over: Option<f64>,
    /// 0 arrives without overshoot; toward 1 it overshoots and rings.
    pub bounce: Option<f64>,
}

impl Manner {
    pub fn over(over: f64) -> Manner {
        Manner {
            over: Some(over),
            bounce: None,
        }
    }
}

#[derive(Clone, Debug)]
struct ChannelState {
    value: f64,
    velocity: f64,
    /// One slot per layer; `None` is not held.
    holds: [Option<f64>; 5],
    omega: f64,
    zeta: f64,
    /// Added this frame, after the spring.
    add: f64,
    /// Pulled toward `pull_to` by this much this frame, after `add`.
    pull: f64,
    pull_to: f64,
}

const DEFAULT_OVER: f64 = 260.0;
/// The stiffest a spring may be and stay stable at the engine's step.
const MAX_OMEGA: f64 = 70.0;
/// A move asked to take less than this is made at once.
const SNAP_MS: f64 = 40.0;

fn omega_of(over: f64) -> f64 {
    // `Math.max(SNAP_MS, over)` with an `over` that is no number is no number, and so is the least of it and 70.
    MAX_OMEGA.min(5.5 / (SNAP_MS.max(over) / 1000.0))
}

fn zeta_of(bounce: f64) -> f64 {
    1.0 - 0.78 * clamp(bounce, 0.0, 1.0)
}

fn flows(name: &str) -> bool {
    name.ends_with(".share") || name.ends_with(".alpha")
}

#[derive(Clone, Debug)]
pub struct Rig {
    pub channels: Channels,
    state: Ordered<ChannelState>,
    retiring: Vec<String>,
    /// Set when nothing should move for effect: springs become quick and exact.
    pub still: bool,
}

impl Rig {
    pub fn new(features: &[FeatureDef]) -> Self {
        let mut rig = Self {
            channels: Channels::new(features),
            state: Ordered::new(),
            retiring: Vec::new(),
            still: false,
        };
        let names: Vec<String> = rig.channels.names().map(str::to_owned).collect();
        for name in names {
            rig.ensure(&name, None);
        }
        rig
    }

    fn ensure(&mut self, name: &str, start: Option<f64>) {
        if self.state.has(name) {
            return;
        }
        let Some(spec) = self.channels.spec(name) else {
            return;
        };
        let state = ChannelState {
            value: start.unwrap_or(spec.rest),
            velocity: 0.0,
            holds: [None; 5],
            omega: omega_of(DEFAULT_OVER),
            zeta: 1.0,
            add: 0.0,
            pull: 0.0,
            pull_to: 0.0,
        };
        self.state.set(name, state);
    }

    /// Adds a feature. Its matter flows in: it starts with no share and no light.
    pub fn add_feature(&mut self, def: &FeatureDef) -> Result<(), String> {
        let had = self.channels.feature(&def.id).is_some();
        self.channels.add_feature(def)?;
        self.retiring.retain(|id| *id != def.id);
        if had {
            return Ok(());
        }
        let prefix = format!("{}.", def.id);
        let names: Vec<String> = self
            .channels
            .names()
            .filter(|name| name.starts_with(&prefix))
            .map(str::to_owned)
            .collect();
        for name in names {
            let start = flows(&name).then_some(0.0);
            self.ensure(&name, start);
        }
        Ok(())
    }

    /// Lets a feature's matter flow back out; it is gone once it has none.
    pub fn retire_feature(&mut self, id: &str) {
        if self.channels.feature(id).is_some() && !self.is_retiring(id) {
            self.retiring.push(id.to_owned());
        }
    }

    pub fn is_retiring(&self, id: &str) -> bool {
        self.retiring.iter().any(|retiring| retiring == id)
    }

    /// Has a layer hold a channel at a value. Returns the value held, clamped; `None` for no such channel.
    pub fn hold(&mut self, layer: Layer, name: &str, value: f64, manner: Manner) -> Option<f64> {
        let spec = self.channels.spec(name)?;
        if !value.is_finite() {
            return None;
        }
        let held = clamp(value, spec.min, spec.max);
        let state = self.state.get_mut(name)?;
        state.holds[layer.slot()] = Some(held);
        state.omega = omega_of(manner.over.unwrap_or(DEFAULT_OVER));
        state.zeta = zeta_of(manner.bounce.unwrap_or(0.0));
        // Matter flows in and out of a feature; everything else may be placed at once.
        if let Some(over) = manner.over
            && over < SNAP_MS
            && !flows(name)
        {
            let target = self.target(name);
            if let Some(state) = self.state.get_mut(name) {
                state.value = target;
                state.velocity = 0.0;
            }
        }
        Some(held)
    }

    /// A layer lets go of some channels, or of all it holds.
    pub fn release(&mut self, layer: Layer, names: Option<&[String]>, manner: Manner) {
        let slot = layer.slot();
        let let_go = |state: &mut ChannelState| {
            if state.holds[slot].is_none() {
                return;
            }
            state.holds[slot] = None;
            state.omega = omega_of(manner.over.unwrap_or(340.0));
            state.zeta = zeta_of(manner.bounce.unwrap_or(0.0));
        };
        match names {
            Some(names) => {
                for name in names {
                    if let Some(state) = self.state.get_mut(name) {
                        let_go(state);
                    }
                }
            }
            None => self.state.values_mut().for_each(let_go),
        }
    }

    /// The channels a layer holds now.
    pub fn held(&self, layer: Layer) -> Vec<String> {
        self.state
            .iter()
            .filter(|(_, state)| state.holds[layer.slot()].is_some())
            .map(|(name, _)| name.to_owned())
            .collect()
    }

    fn target_of(&self, name: &str, state: &ChannelState) -> f64 {
        let Some(spec) = self.channels.spec(name) else {
            return 0.0;
        };
        if flows(name)
            && let Some((feature, _)) = name.rsplit_once('.')
            && self.is_retiring(feature)
        {
            return 0.0;
        }
        state
            .holds
            .iter()
            .rev()
            .find_map(|held| *held)
            .unwrap_or(spec.rest)
    }

    /// Where a channel is headed: the top hold, or rest.
    pub fn target(&self, name: &str) -> f64 {
        match self.state.get(name) {
            Some(state) => self.target_of(name, state),
            None => 0.0,
        }
    }

    /// Adds to a channel for this frame only.
    pub fn offset(&mut self, name: &str, amount: f64) {
        if let Some(state) = self.state.get_mut(name) {
            state.add += amount;
        }
    }

    /// Pulls a channel toward a value for this frame only; 1 puts it there.
    pub fn toward(&mut self, name: &str, to: f64, amount: f64) {
        if let Some(state) = self.state.get_mut(name) {
            if amount <= state.pull {
                return;
            }
            state.pull = clamp(amount, 0.0, 1.0);
            state.pull_to = to;
        }
    }

    /// Clears the frame's modifiers. Call before the things that set them.
    pub fn begin_frame(&mut self) {
        for state in self.state.values_mut() {
            state.add = 0.0;
            state.pull = 0.0;
        }
    }

    /// Moves every spring on by `dt` seconds. Keep `dt` small and fixed.
    pub fn step(&mut self, dt: f64) {
        let targets: Vec<f64> = self
            .state
            .iter()
            .map(|(name, state)| self.target_of(name, state))
            .collect();
        let rate = 1.0 - (-dt * 14.0).exp();
        for (state, target) in self.state.values_mut().zip(targets) {
            if self.still {
                // Reduced motion: no overshoot, and quick.
                state.value += (target - state.value) * rate;
                state.velocity = 0.0;
                continue;
            }
            let force = -state.omega * state.omega * (state.value - target)
                - 2.0 * state.zeta * state.omega * state.velocity;
            state.velocity += force * dt;
            state.value += state.velocity * dt;
        }
        for id in self.retiring.clone() {
            if self
                .state
                .get(&format!("{id}.share"))
                .is_some_and(|share| share.value > 0.02)
            {
                continue;
            }
            self.retiring.retain(|retiring| *retiring != id);
            self.channels.remove_feature(&id);
            let prefix = format!("{id}.");
            let gone: Vec<String> = self
                .state
                .keys()
                // Only its own: `brow` going must not take `brow.left`'s with it.
                .filter(|name| {
                    name.strip_prefix(&prefix)
                        .is_some_and(|param| !param.contains('.'))
                })
                .map(str::to_owned)
                .collect();
            for name in gone {
                self.state.remove(&name);
            }
        }
    }

    /// Puts every spring at its target at once.
    pub fn settle(&mut self) {
        let targets: Vec<f64> = self
            .state
            .iter()
            .map(|(name, state)| self.target_of(name, state))
            .collect();
        for (state, target) in self.state.values_mut().zip(targets) {
            state.value = target;
            state.velocity = 0.0;
        }
    }

    /// A channel's value now, with this frame's modifiers, inside its bounds.
    pub fn read(&self, name: &str) -> f64 {
        let (Some(state), Some(spec)) = (self.state.get(name), self.channels.spec(name)) else {
            return 0.0;
        };
        let mut value = state.value + state.add;
        if state.pull > 0.0 {
            value += (state.pull_to - value) * state.pull;
        }
        clamp(value, spec.min, spec.max)
    }

    /// Whether any spring is still far from where it is going.
    pub fn moving(&self, epsilon: f64) -> bool {
        self.state.iter().any(|(name, state)| {
            let Some(spec) = self.channels.spec(name) else {
                return false;
            };
            let span = spec.max - spec.min;
            (state.value - self.target_of(name, state)).abs() > span * epsilon
                || state.velocity.abs() > span * epsilon * 4.0
        })
    }

    /// Every channel that is away from rest, by name, rounded: what the face is doing in numbers.
    pub fn away(&self) -> Vec<(String, f64)> {
        self.state
            .iter()
            .filter_map(|(name, state)| {
                let spec = self.channels.spec(name)?;
                let span = spec.max - spec.min;
                ((state.value - spec.rest).abs() > span * 0.01)
                    .then(|| (name.to_owned(), round(state.value * 1000.0) / 1000.0))
            })
            .collect()
    }
}
