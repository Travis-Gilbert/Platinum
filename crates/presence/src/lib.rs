//! Which of the things going on Theo shows.
//!
//! A desktop host has several activities at once: a run thinking, sound
//! playing, a capture being taken. The face has one form. This crate owns
//! the set of activities as the host knows it, chooses the one whose form is
//! shown, brings the earlier one back when a brief one ends, and refuses an
//! update or a stop that is older than what it already knows.
//!
//! It decides; it does not draw, and it does not own the work. The durable
//! record of a task is the runtime's, the visual program is `theo`'s, and
//! what is shown here is never a receipt that work completed. A host feeds
//! it updates in order, reads `primary` after each, and applies the change
//! to the face.
//!
//! Everything it keeps is bounded: at most `LIVE` activities, and the
//! generations of the last `ENDED` that ended.
use std::collections::VecDeque;

/// The most activities held at once. One more is refused, not dropped in silence.
pub const LIVE: usize = 64;
/// How many ended activities are remembered, so a late update for one is still known to be late.
pub const ENDED: usize = 256;

/// Where an activity is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Starting,
    Active,
    AwaitingUser,
    Stopping,
    Completed,
    Failed,
    /// The host has lost sight of it. It is still held, and still shown, until it is known to have ended.
    Unknown,
}

impl Phase {
    /// Whether the activity is over.
    pub fn ended(self) -> bool {
        matches!(self, Phase::Completed | Phase::Failed)
    }

    pub fn name(self) -> &'static str {
        match self {
            Phase::Starting => "starting",
            Phase::Active => "active",
            Phase::AwaitingUser => "awaiting_user",
            Phase::Stopping => "stopping",
            Phase::Completed => "completed",
            Phase::Failed => "failed",
            Phase::Unknown => "unknown",
        }
    }

    pub fn parse(name: &str) -> Option<Phase> {
        [
            Phase::Starting,
            Phase::Active,
            Phase::AwaitingUser,
            Phase::Stopping,
            Phase::Completed,
            Phase::Failed,
            Phase::Unknown,
        ]
        .into_iter()
        .find(|phase| phase.name() == name)
    }
}

/// How an activity stands against the others when one form must be chosen.
/// Among those that stand equally, the one begun last is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Standing {
    /// Shown only when nothing else is: something that goes on in the background.
    Ambient,
    #[default]
    Ordinary,
    /// Brief, and shown over the rest while it lasts: a capture, a confirmation.
    Brief,
}

/// A measurement an activity reports, with what it is a measurement of.
#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    pub name: String,
    pub value: f64,
    pub unit: String,
    /// Where the number came from: the playback clock, a provider's count.
    pub source: String,
    /// When it was measured, in the host's milliseconds.
    pub at: f64,
    /// How long it stays worth showing, in milliseconds.
    pub fresh_for: f64,
}

impl Signal {
    /// Whether the measurement is still worth showing. One that is not is not shown as if it were.
    pub fn fresh(&self, now: f64) -> bool {
        now >= self.at && now - self.at <= self.fresh_for
    }
}

/// One thing going on, as the host knows it. Enough to draw it again from nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    pub id: String,
    pub parent: Option<String>,
    pub conversation: Option<String>,
    pub session: Option<String>,
    pub agent: Option<String>,
    pub task: Option<String>,
    pub environment: Option<String>,
    pub surface: Option<String>,
    /// Which run of this activity this is: the second track played under the same id is a later generation.
    pub generation: u64,
    /// Which account of that run this is. A later one replaces an earlier.
    pub revision: u64,
    pub phase: Phase,
    pub standing: Standing,
    /// The form it would like. The host arbitrates; it is a preference.
    pub preferred_form: Option<String>,
    pub target: Option<String>,
    pub observation: Option<String>,
    pub signals: Vec<Signal>,
    /// What the person may do about it now.
    pub available_actions: Vec<String>,
    /// The verified result, or the explicit uncertainty, once there is one.
    pub outcome: Option<String>,
}

impl Activity {
    pub fn new(id: &str, generation: u64) -> Activity {
        Activity {
            id: id.to_owned(),
            parent: None,
            conversation: None,
            session: None,
            agent: None,
            task: None,
            environment: None,
            surface: None,
            generation,
            revision: 0,
            phase: Phase::Starting,
            standing: Standing::default(),
            preferred_form: None,
            target: None,
            observation: None,
            signals: Vec::new(),
            available_actions: Vec::new(),
            outcome: None,
        }
    }
}

/// What came of an update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    /// Taken. `changed` says whether what is shown is now different.
    Taken { changed: bool },
    /// Older than what is already known, or about a run that has ended: nothing changed.
    Stale,
    /// There are already `LIVE` activities: nothing changed, and the host should say so.
    Full,
}

/// What the face should be showing: whose form, and which run of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub activity: String,
    pub generation: u64,
    pub form: String,
}

#[derive(Clone, Debug)]
struct Held {
    activity: Activity,
    /// The order it began in, among all that ever began here.
    began: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Presence {
    live: Vec<Held>,
    /// The last generation of each activity that ended, oldest first.
    ended: VecDeque<(String, u64)>,
    began: u64,
}

impl Presence {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes an account of an activity: its beginning, a change, or its end.
    pub fn apply(&mut self, update: Activity) -> Applied {
        let before = self.primary();
        let at = self
            .live
            .iter()
            .position(|held| held.activity.id == update.id);
        match at {
            Some(at) => {
                let known = &self.live[at].activity;
                let newer =
                    (update.generation, update.revision) > (known.generation, known.revision);
                if !newer {
                    return Applied::Stale;
                }
                if update.phase.ended() {
                    let ended = self.live.remove(at);
                    // An end of an older run does not end this one, and was refused above; this is this run's or a later one's.
                    self.remember(ended.activity.id, update.generation);
                } else if update.generation > known.generation {
                    // A new run under the same id begins now, and is shown as the latest begun.
                    self.began += 1;
                    self.live[at] = Held {
                        activity: update,
                        began: self.began,
                    };
                } else {
                    self.live[at].activity = update;
                }
            }
            None => {
                let late = self
                    .ended
                    .iter()
                    .any(|(id, generation)| *id == update.id && update.generation <= *generation);
                if late {
                    return Applied::Stale;
                }
                if update.phase.ended() {
                    // It began and ended unseen. It is remembered as ended, so nothing older can begin it.
                    self.remember(update.id, update.generation);
                    return Applied::Taken { changed: false };
                }
                if self.live.len() >= LIVE {
                    return Applied::Full;
                }
                self.began += 1;
                self.live.push(Held {
                    activity: update,
                    began: self.began,
                });
            }
        }
        Applied::Taken {
            changed: self.primary() != before,
        }
    }

    /// Ends one run of an activity and no other: a stop for a run that a later one has replaced does nothing.
    /// It is a stop of what is shown and held here. Stopping the work is the owner's, and has its own outcome.
    pub fn end(&mut self, id: &str, generation: u64) -> Applied {
        let Some(known) = self.get(id) else {
            return Applied::Stale;
        };
        if known.generation != generation {
            return Applied::Stale;
        }
        let mut update = known.clone();
        update.revision = update.revision.saturating_add(1);
        update.phase = Phase::Completed;
        self.apply(update)
    }

    fn remember(&mut self, id: String, generation: u64) {
        self.ended.retain(|(known, _)| *known != id);
        if self.ended.len() >= ENDED {
            self.ended.pop_front();
        }
        self.ended.push_back((id, generation));
    }

    pub fn get(&self, id: &str) -> Option<&Activity> {
        self.live
            .iter()
            .find(|held| held.activity.id == id)
            .map(|held| &held.activity)
    }

    /// Every activity that has not ended, in the order they began.
    pub fn activities(&self) -> impl Iterator<Item = &Activity> {
        self.live.iter().map(|held| &held.activity)
    }

    /// The activity whose form is shown: of those that ask for a form, the one that stands highest, and of those the one begun last.
    pub fn primary(&self) -> Option<Shown> {
        self.live
            .iter()
            .filter(|held| held.activity.preferred_form.is_some())
            .max_by_key(|held| (held.activity.standing, held.began))
            .map(|held| Shown {
                activity: held.activity.id.clone(),
                generation: held.activity.generation,
                form: held.activity.preferred_form.clone().unwrap_or_default(),
            })
    }

    /// Everything needed to show the same thing again: the activities, in the order they began.
    pub fn snapshot(&self) -> Vec<Activity> {
        self.activities().cloned().collect()
    }

    /// Begins again from a snapshot, as a view does when it is made anew or a connection comes back.
    /// What was held is replaced. Activities past `LIVE`, and ones that have ended, are left out; how many were left out is returned.
    pub fn restore(&mut self, snapshot: Vec<Activity>) -> usize {
        self.live.clear();
        let mut left_out = 0;
        for activity in snapshot {
            let known = self.live.iter().any(|held| held.activity.id == activity.id);
            if activity.phase.ended() || known || self.live.len() >= LIVE {
                left_out += 1;
                continue;
            }
            self.began += 1;
            self.live.push(Held {
                activity,
                began: self.began,
            });
        }
        left_out
    }
}
