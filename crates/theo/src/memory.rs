//! What the face remembers: how often each kind of thing has happened over
//! seconds, minutes and days, the last few events in order, and a mood. The
//! same event read against a different past gives a different reading, which
//! is what makes a reaction depend on what came before.
use std::collections::HashMap;

use crate::channels::clamp;
use crate::json::Json;
use crate::ordered::Ordered;
use crate::random::round;

/// What keeps things between visits: a file in the app; a map in tests.
pub trait Shelf {
    fn get_item(&self, key: &str) -> Option<String>;
    /// Keeps a value. Failing to is not an error to the face: it is kept for this visit only.
    fn set_item(&mut self, key: &str, value: &str);
}

/// Something that happened: the person did it, a run did it, the environment did, or it happened to the figure's own body.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceEvent {
    /// A dotted kind: `person.poke`, `run.tool.fail`, `env.resize`, `body.bump.left`.
    pub kind: String,
    /// When, in milliseconds.
    pub at: f64,
    /// How much it counts.
    pub weight: f64,
}

/// What an event is like against what came before it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    /// How many of its kind lately, decayed over seconds.
    pub fast: f64,
    /// How many of its kind lately, decayed over minutes.
    pub slow: f64,
    /// How many of its kind in a row with only short gaps, this one included.
    pub streak: f64,
    /// Milliseconds since the last of its kind; infinite for the first.
    pub gap: f64,
    /// 0..1: how ordinary this kind is on this device over past days.
    pub habit: f64,
    /// 0..1: 1 the first time, falling as it becomes frequent or habitual.
    pub novelty: f64,
}

/// How the face feels, slowly: each drifts back to rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mood {
    /// 0 torpid to 1 wound up.
    pub arousal: f64,
    /// -1 sour to 1 pleased.
    pub valence: f64,
    /// 0 idle to 1 absorbed.
    pub focus: f64,
}

pub const MOOD_REST: Mood = Mood {
    arousal: 0.25,
    valence: 0.1,
    focus: 0.15,
};

const FAST_HALF: f64 = 8_000.0;
const SLOW_HALF: f64 = 300_000.0;
const MOOD_HALF: f64 = 45_000.0;
const STREAK_GAP: f64 = 2_600.0;
const DAY: f64 = 86_400_000.0;
/// How many events are remembered in order.
pub const RECENT: usize = 24;
/// How many kinds are counted at once, in each of the three places a kind is counted: lately, today, and
/// what is usual. The web counts every kind it is ever told; a host that names kinds freely would grow
/// that without end, in memory and on the shelf. So:
/// - lately: the kind longest unseen makes room;
/// - today: kinds past the bound are counted together under `OVERFLOW`, so no event goes uncounted;
/// - usual: the most usual are kept, and the rest read as new;
/// - a shelf that holds more, however it came to, is brought inside the same bounds when read.
pub const KINDS: usize = 512;
/// Where today's count goes for kinds past the bound. It is no kind a host names, so it is habit to nothing.
pub const OVERFLOW: &str = "*";
const SHELF_KEY: &str = "theorem-ui:theo:days";

/// What each kind of event does to the mood: arousal, valence, focus. The first prefix that matches wins.
const NUDGES: [(&str, f64, f64, f64); 20] = [
    ("person.poke", 0.16, 0.03, 0.0),
    ("person.pet", -0.04, 0.14, 0.0),
    ("person.drop", 0.1, 0.0, 0.0),
    ("person.throw", 0.18, 0.02, 0.0),
    ("person.send", 0.1, 0.0, 0.12),
    ("person.type", 0.0, 0.0, 0.02),
    ("person.return", 0.12, 0.05, 0.0),
    ("run.start", 0.08, 0.0, 0.22),
    ("run.tool.fail", 0.14, -0.18, 0.0),
    ("run.tool", 0.0, 0.0, 0.04),
    ("run.error", 0.12, -0.25, 0.0),
    ("run.ask", 0.1, 0.0, 0.0),
    ("run.done", -0.06, 0.2, -0.2),
    ("env.offline", 0.1, -0.15, 0.0),
    ("env.online", 0.0, 0.12, 0.0),
    ("env.", 0.05, 0.0, 0.0),
    ("sound.loud", 0.1, 0.0, 0.0),
    ("sound.start", 0.06, 0.0, 0.05),
    ("body.bump", 0.14, -0.05, 0.0),
    ("body.meet", 0.12, 0.03, 0.0),
];

#[derive(Clone, Copy, Debug)]
struct KindMemory {
    fast: f64,
    slow: f64,
    last: f64,
    streak: f64,
}

/// Counts per kind for today, and what is usual per day.
#[derive(Clone, Debug, Default)]
struct Days {
    day: f64,
    today: Ordered<f64>,
    usual: Ordered<f64>,
}

fn decay(value: f64, elapsed: f64, half: f64) -> f64 {
    value * 0.5_f64.powf(elapsed.max(0.0) / half)
}

fn counts(value: Option<&Json>) -> Option<Ordered<f64>> {
    let mut out = Ordered::new();
    for (kind, count) in value?.obj()? {
        out.set(kind, count.num()?);
    }
    Some(out)
}

/// Today's counts inside the bound: the first `KINDS` kinds as they are, the rest together.
fn bounded_today(counts: Ordered<f64>) -> Ordered<f64> {
    if counts.len() <= KINDS {
        return counts;
    }
    let mut out = Ordered::new();
    let mut over = 0.0;
    for (kind, count) in counts.iter() {
        if kind != OVERFLOW && out.len() < KINDS {
            out.set(kind, *count);
        } else {
            over += count;
        }
    }
    out.set(OVERFLOW, over);
    out
}

/// What is usual inside the bound: the `KINDS` most usual, in the order they were in.
fn bounded_usual(counts: Ordered<f64>) -> Ordered<f64> {
    if counts.len() <= KINDS {
        return counts;
    }
    let mut values: Vec<f64> = counts.values().copied().collect();
    values.sort_by(|a, b| b.total_cmp(a));
    let least = values[KINDS - 1];
    let mut out = Ordered::new();
    // Those above the least kept go in whatever; those equal to it, first come.
    let above = values.iter().filter(|value| **value > least).count();
    let mut equal = KINDS - above;
    for (kind, count) in counts.iter() {
        if *count > least {
            out.set(kind, *count);
        } else if *count == least && equal > 0 {
            equal -= 1;
            out.set(kind, *count);
        }
    }
    out
}

fn written(counts: &Ordered<f64>) -> Json {
    Json::Obj(
        counts
            .iter()
            .map(|(kind, count)| (kind.to_owned(), Json::Num(*count)))
            .collect(),
    )
}

#[derive(Clone, Debug)]
pub struct Memory {
    kinds: HashMap<String, KindMemory>,
    /// The last `RECENT` events, oldest first.
    pub recent: Vec<FaceEvent>,
    pub mood: Mood,
    mood_at: f64,
    days: Days,
    saved_at: f64,
}

impl Memory {
    pub fn new(shelf: Option<&dyn Shelf>) -> Self {
        let mut days = Days::default();
        // No past is a fine past.
        if let Some(saved) = shelf
            .and_then(|shelf| shelf.get_item(SHELF_KEY))
            .and_then(|text| Json::parse(&text).ok())
        {
            if let (Some(day), Some(today), Some(usual)) = (
                saved.num_at("day"),
                counts(saved.get("today")),
                counts(saved.get("usual")),
            ) {
                days = Days {
                    day,
                    today: bounded_today(today),
                    usual: bounded_usual(usual),
                };
            }
        }
        Self {
            kinds: HashMap::new(),
            recent: Vec::new(),
            mood: MOOD_REST,
            mood_at: 0.0,
            days,
            saved_at: 0.0,
        }
    }

    /// Takes an event in and says what it is like against the past.
    pub fn note(&mut self, event: &FaceEvent, mut shelf: Option<&mut dyn Shelf>) -> Reading {
        let weight = event.weight;
        let known = self.kinds.get(&event.kind).copied();
        let gap = known.map_or(f64::INFINITY, |known| event.at - known.last);
        let mut memory = known.unwrap_or(KindMemory {
            fast: 0.0,
            slow: 0.0,
            last: event.at,
            streak: 0.0,
        });
        let elapsed = if gap.is_infinite() { 0.0 } else { gap };
        memory.fast = decay(memory.fast, elapsed, FAST_HALF) + weight;
        memory.slow = decay(memory.slow, elapsed, SLOW_HALF) + weight;
        memory.streak = if gap <= STREAK_GAP {
            memory.streak + 1.0
        } else {
            1.0
        };
        memory.last = event.at;
        if known.is_none() && self.kinds.len() >= KINDS {
            if let Some(oldest) = self
                .kinds
                .iter()
                // Kinds seen at the same moment go in the order of their names, so the same events always forget the same one.
                .min_by(|a, b| a.1.last.total_cmp(&b.1.last).then_with(|| a.0.cmp(b.0)))
                .map(|(kind, _)| kind.clone())
            {
                self.kinds.remove(&oldest);
            }
        }
        self.kinds.insert(event.kind.clone(), memory);

        self.recent.push(event.clone());
        if self.recent.len() > RECENT {
            self.recent.remove(0);
        }

        self.roll(event.at, &mut shelf);
        let habit = self.habit(&event.kind);
        let counted = if self.days.today.has(&event.kind) || self.days.today.len() < KINDS {
            event.kind.as_str()
        } else {
            OVERFLOW
        };
        let today = self.days.today.get(counted).copied().unwrap_or(0.0);
        self.days.today.set(counted, today + weight);
        if event.at - self.saved_at > 30_000.0 {
            self.save(event.at, &mut shelf);
        }

        self.settle(event.at);
        if let Some((_, arousal, valence, focus)) = NUDGES
            .iter()
            .find(|(prefix, ..)| event.kind.starts_with(prefix))
        {
            // A habitual thing moves the mood less.
            let scale = weight * (1.0 - 0.6 * habit);
            self.mood.arousal = clamp(self.mood.arousal + arousal * scale, 0.0, 1.0);
            self.mood.valence = clamp(self.mood.valence + valence * scale, -1.0, 1.0);
            self.mood.focus = clamp(self.mood.focus + focus * scale, 0.0, 1.0);
        }

        Reading {
            fast: memory.fast,
            slow: memory.slow,
            streak: memory.streak,
            gap,
            habit,
            novelty: clamp(
                1.0 / (1.0 + (memory.slow - weight) * 0.6 + habit * 3.0),
                0.0,
                1.0,
            ),
        }
    }

    /// Lets the mood drift toward rest up to `now`.
    pub fn settle(&mut self, now: f64) -> Mood {
        if self.mood_at == 0.0 {
            self.mood_at = now;
        }
        let keep = 0.5_f64.powf((now - self.mood_at).max(0.0) / MOOD_HALF);
        self.mood_at = now;
        self.mood.arousal = MOOD_REST.arousal + (self.mood.arousal - MOOD_REST.arousal) * keep;
        self.mood.valence = MOOD_REST.valence + (self.mood.valence - MOOD_REST.valence) * keep;
        self.mood.focus = MOOD_REST.focus + (self.mood.focus - MOOD_REST.focus) * keep;
        self.mood
    }

    /// 0..1: how ordinary a kind is here, from its usual count per day.
    pub fn habit(&self, kind: &str) -> f64 {
        let usual = self.days.usual.get(kind).copied().unwrap_or(0.0);
        usual / (usual + 20.0)
    }

    /// Whether the last events, newest last, end with these kinds inside a span. `run.*` matches a prefix.
    pub fn ends_with(&self, kinds: &[String], within: Option<f64>) -> bool {
        if kinds.is_empty() || kinds.len() > self.recent.len() {
            return false;
        }
        let tail = &self.recent[self.recent.len() - kinds.len()..];
        let (first, last) = (&tail[0], &tail[tail.len() - 1]);
        if last.at - first.at > within.unwrap_or(f64::INFINITY) {
            return false;
        }
        kinds
            .iter()
            .zip(tail)
            .all(|(kind, event)| matches(kind, &event.kind))
    }

    /// A new day folds yesterday's counts into what is usual.
    fn roll(&mut self, now: f64, shelf: &mut Option<&mut dyn Shelf>) {
        let day = (now / DAY).floor();
        if self.days.day == 0.0 {
            self.days.day = day;
        }
        if day == self.days.day {
            return;
        }
        let mut usual = Ordered::new();
        let kinds = self.days.usual.keys().chain(
            self.days
                .today
                .keys()
                .filter(|kind| !self.days.usual.has(kind)),
        );
        for kind in kinds.filter(|kind| *kind != OVERFLOW) {
            let value = self.days.usual.get(kind).copied().unwrap_or(0.0) * 0.7
                + self.days.today.get(kind).copied().unwrap_or(0.0) * 0.3;
            if value >= 0.05 {
                usual.set(kind, round(value * 100.0) / 100.0);
            }
        }
        self.days = Days {
            day,
            today: Ordered::new(),
            usual: bounded_usual(usual),
        };
        self.save(now, shelf);
    }

    fn save(&mut self, now: f64, shelf: &mut Option<&mut dyn Shelf>) {
        self.saved_at = now;
        if let Some(shelf) = shelf {
            let days = Json::Obj(vec![
                ("day".to_owned(), Json::Num(self.days.day)),
                ("today".to_owned(), written(&self.days.today)),
                ("usual".to_owned(), written(&self.days.usual)),
            ]);
            shelf.set_item(SHELF_KEY, &days.to_string());
        }
    }
}

/// Whether an event kind is the one a rule names; a trailing `*` matches any ending.
pub fn matches(pattern: &str, kind: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => kind.starts_with(prefix),
        None => pattern == kind,
    }
}
