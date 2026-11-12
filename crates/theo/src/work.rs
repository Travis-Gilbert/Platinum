//! What a run is doing, as numbers the face can follow: how busy it is with
//! tools, how hard it was asked to think, how full its context is. Counted
//! from what the run reports; nothing here guesses.
use crate::random::Random;

/// How much one tool call starting or ending adds, how much more a failed one does, and how long the pace takes to halve, in milliseconds.
const PUSH: f64 = 0.3;
const FAILED: f64 = 1.6;
const HALVES: f64 = 2400.0;

/// The reasoning effort levels, least first.
pub const EFFORT_LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// Where a reasoning effort level lies from least (0) to most (1). A level not listed, or none, is the middle.
pub fn effort_share(level: Option<&str>) -> f64 {
    match level.and_then(|level| EFFORT_LEVELS.iter().position(|known| *known == level)) {
        Some(index) => index as f64 / (EFFORT_LEVELS.len() - 1) as f64,
        None => 0.5,
    }
}

/// Up to this many tokens in its context a conversation is as composed as ever.
pub const COMPOSED_UNTIL: f64 = 400_000.0;
const WINDOW_UNKNOWN: f64 = 1_000_000.0;

/// How far past `COMPOSED_UNTIL` a conversation's context is, 0 to 1.
pub fn context_share(tokens: Option<f64>, window: Option<f64>) -> f64 {
    let Some(tokens) = tokens else { return 0.0 };
    if !(tokens > COMPOSED_UNTIL) {
        return 0.0;
    }
    let full = match window {
        Some(window) if window > 0.0 => window,
        _ => WINDOW_UNKNOWN,
    };
    if full <= COMPOSED_UNTIL {
        return 0.0;
    }
    ((tokens - COMPOSED_UNTIL) / (full - COMPOSED_UNTIL)).min(1.0)
}

/// The three run signals as they stand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measured {
    pub pace: f64,
    pub effort: f64,
    pub context: f64,
}

#[derive(Clone, Debug)]
pub struct Work {
    pace: f64,
    at: Option<f64>,
    /// The effort the run was sent with, 0 to 1.
    pub effort: f64,
    /// How full the conversation's context is past where it is composed, 0 to 1.
    pub context: f64,
    /// How hard each push lands is not the same twice.
    random: Random,
}

impl Work {
    pub fn new(random: Random) -> Self {
        Self {
            pace: 0.0,
            at: None,
            effort: 0.5,
            context: 0.0,
            random,
        }
    }

    fn settle(&mut self, now: f64) {
        if let Some(at) = self.at
            && now > at
        {
            self.pace *= 0.5f64.powf((now - at) / HALVES);
        }
        self.at = Some(now);
    }

    /// A tool call started or ended at `now`, in milliseconds.
    pub fn tool(&mut self, now: f64, failed: bool) {
        self.settle(now);
        let push = PUSH * (0.7 + 0.6 * self.random.next()) * if failed { FAILED } else { 1.0 };
        self.pace = (self.pace + push).min(1.0);
    }

    /// The signals as they stand at `now`. While nothing runs they are 0.
    pub fn measure(&mut self, now: f64, working: bool) -> Measured {
        self.settle(now);
        if !working {
            self.pace = 0.0;
        }
        Measured {
            pace: self.pace,
            effort: if working { self.effort } else { 0.0 },
            context: if working { self.context } else { 0.0 },
        }
    }
}
