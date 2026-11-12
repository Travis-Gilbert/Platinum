//! The signals as they stand: what the host last measured, inside its bounds.
//! They are measured, never claimed: the sound ones from audio the host is
//! itself playing (never a microphone), the run ones from the run's own
//! reports.
use crate::ordered::Ordered;
use crate::tables::{SignalSpec, tables};

pub fn signal(name: &str) -> Option<&'static SignalSpec> {
    tables().signals.iter().find(|spec| spec.name == name)
}

pub fn is_signal(name: &str) -> bool {
    signal(name).is_some()
}

#[derive(Clone, Debug, Default)]
pub struct Signals {
    values: Ordered<f64>,
}

impl Signals {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes one measurement. A name that is no signal is passed over, and a value that is no number reads as the least.
    pub fn feed(&mut self, name: &str, value: f64) {
        let Some(spec) = signal(name) else { return };
        let kept = if value.is_finite() {
            value.max(spec.min).min(spec.max)
        } else {
            spec.min
        };
        self.values.set(name, kept);
    }

    pub fn read(&self, name: &str) -> f64 {
        self.values.get(name).copied().unwrap_or(0.0)
    }

    /// Every signal that is away from nothing, rounded: what is being measured, in numbers.
    pub fn away(&self) -> Vec<(String, f64)> {
        self.values
            .iter()
            .filter(|(_, value)| value.abs() > 0.01)
            .map(|(name, value)| (name.to_owned(), crate::random::round(value * 100.0) / 100.0))
            .collect()
    }
}
