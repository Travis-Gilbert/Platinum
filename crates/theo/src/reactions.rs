//! Chooses what to do about each event. Every rule that fits is a candidate,
//! weighted down if it fired lately, and doing nothing is always a candidate
//! too; the draw is seeded, so a run can be replayed. A rule that says what
//! ends it gives the face a form rather than a passing reaction.
use std::collections::HashMap;

use crate::book::{Book, By, clip, is_name, length};
use crate::channels::{Channels, clamp};
use crate::json::Json;
use crate::memory::{FaceEvent, Memory, Mood, Reading, Shelf, matches};
use crate::program::{Context, Refusal, compile};
use crate::random::Random;
use crate::tables::tables;

/// A number the face reads off its memory and mood when deciding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gauge {
    Streak,
    Fast,
    Slow,
    Novelty,
    Habit,
    Arousal,
    Valence,
    Focus,
}

pub const GAUGES: [Gauge; 8] = [
    Gauge::Streak,
    Gauge::Fast,
    Gauge::Slow,
    Gauge::Novelty,
    Gauge::Habit,
    Gauge::Arousal,
    Gauge::Valence,
    Gauge::Focus,
];

impl Gauge {
    pub fn name(self) -> &'static str {
        match self {
            Gauge::Streak => "streak",
            Gauge::Fast => "fast",
            Gauge::Slow => "slow",
            Gauge::Novelty => "novelty",
            Gauge::Habit => "habit",
            Gauge::Arousal => "arousal",
            Gauge::Valence => "valence",
            Gauge::Focus => "focus",
        }
    }

    pub fn parse(name: &str) -> Option<Gauge> {
        GAUGES.into_iter().find(|gauge| gauge.name() == name)
    }

    fn read(self, reading: &Reading, mood: &Mood) -> f64 {
        match self {
            Gauge::Streak => reading.streak,
            Gauge::Fast => reading.fast,
            Gauge::Slow => reading.slow,
            Gauge::Novelty => reading.novelty,
            Gauge::Habit => reading.habit,
            Gauge::Arousal => mood.arousal,
            Gauge::Valence => mood.valence,
            Gauge::Focus => mood.focus,
        }
    }
}

/// One thing the face may do about one kind of event.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub id: String,
    /// The event kind, or the last few kinds in order ending with it. A trailing `*` matches any ending.
    pub on: Vec<String>,
    /// Whether `on` was written as one kind rather than a list.
    pub single: bool,
    /// With a sequence: the longest it may have taken, in milliseconds.
    pub within: Option<f64>,
    /// Each gauge named must lie within its bounds, inclusive.
    pub when: Vec<(Gauge, f64, f64)>,
    /// A performance from the book by name (a string), or a program.
    pub does: Json,
    /// The event kind that ends it. With this the rule gives the face a form.
    pub until: Option<String>,
    pub intensity: Option<f64>,
    pub weight: Option<f64>,
    /// Milliseconds over which it grows likely again after firing.
    pub cooldown: Option<f64>,
    pub note: Option<String>,
    pub by: By,
}

impl Rule {
    /// A rule as it is kept; `None` for something that is not one.
    pub fn read(value: &Json) -> Option<Rule> {
        let on = value.get("on")?;
        let (on, single) = match on {
            Json::Str(kind) => (vec![kind.clone()], true),
            Json::List(kinds) => (
                kinds
                    .iter()
                    .map(|kind| kind.str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()?,
                false,
            ),
            _ => return None,
        };
        let mut when = Vec::new();
        for (name, range) in value.get("when").and_then(Json::obj).unwrap_or(&[]) {
            let range = range.list()?;
            when.push((
                Gauge::parse(name)?,
                range.first()?.num()?,
                range.get(1)?.num()?,
            ));
        }
        Some(Rule {
            id: value.str_at("id")?.to_owned(),
            on,
            single,
            within: value.num_at("within"),
            when,
            does: value.get("do")?.clone(),
            until: value.str_at("until").map(str::to_owned),
            intensity: value.num_at("intensity"),
            weight: value.num_at("weight"),
            cooldown: value.num_at("cooldown"),
            note: value.str_at("note").map(str::to_owned),
            by: value.str_at("by").and_then(By::parse).unwrap_or(By::Theo),
        })
    }

    pub fn to_json(&self) -> Json {
        let text = |text: &str| Json::Str(text.to_owned());
        let mut fields = vec![("id".to_owned(), text(&self.id))];
        fields.push((
            "on".to_owned(),
            if self.single {
                text(&self.on[0])
            } else {
                Json::List(self.on.iter().map(|kind| text(kind)).collect())
            },
        ));
        let mut number = |key: &str, value: Option<f64>| {
            if let Some(value) = value {
                fields.push((key.to_owned(), Json::Num(value)));
            }
        };
        number("within", self.within);
        if !self.when.is_empty() {
            fields.push((
                "when".to_owned(),
                Json::Obj(
                    self.when
                        .iter()
                        .map(|(gauge, low, high)| {
                            (
                                gauge.name().to_owned(),
                                Json::List(vec![Json::Num(*low), Json::Num(*high)]),
                            )
                        })
                        .collect(),
                ),
            ));
        }
        fields.push(("do".to_owned(), self.does.clone()));
        let mut number = |key: &str, value: Option<f64>| {
            if let Some(value) = value {
                fields.push((key.to_owned(), Json::Num(value)));
            }
        };
        number("intensity", self.intensity);
        number("weight", self.weight);
        number("cooldown", self.cooldown);
        if let Some(note) = &self.note {
            fields.push(("note".to_owned(), text(note)));
        }
        if let Some(until) = &self.until {
            fields.push(("until".to_owned(), text(until)));
        }
        fields.push(("by".to_owned(), text(self.by.name())));
        Json::Obj(fields)
    }

    /// The book name the rule plays, when it names one.
    pub fn named(&self) -> Option<&str> {
        self.does.str()
    }
}

/// What the face decided to do about an event.
#[derive(Clone, Debug, PartialEq)]
pub struct Reaction {
    /// The rule that was drawn.
    pub rule: String,
    /// What ends it, for a form.
    pub until: Option<String>,
    /// What to perform: a book name (a string) or a program.
    pub does: Json,
    pub intensity: f64,
}

const SHELF_KEY: &str = "theorem-ui:theo:rules";
/// The most rules that may be added.
pub const ADDED: usize = 32;

/// An event kind as a rule may name it: `person.poke`, `run.*`.
fn is_kind(text: &str) -> bool {
    is_name(text.strip_suffix('*').unwrap_or(text), 1, 39, '.')
}

/// The rules that fit an event as read against memory and mood, in the order they stand.
fn fitting<'a>(
    defaults: &'a [Rule],
    added: &'a [Rule],
    event: &'a FaceEvent,
    reading: &'a Reading,
    mood: &'a Mood,
    memory: &'a Memory,
) -> impl Iterator<Item = &'a Rule> {
    defaults
        .iter()
        .filter(|rule| !added.iter().any(|item| item.id == rule.id))
        .chain(added)
        .filter(move |rule| {
            let Some(last) = rule.on.last() else {
                return false;
            };
            matches(last, &event.kind)
                && (rule.on.len() < 2 || memory.ends_with(&rule.on, rule.within))
                && rule.when.iter().all(|(gauge, low, high)| {
                    let value = gauge.read(reading, mood);
                    value >= *low && value <= *high
                })
        })
}

#[derive(Clone, Debug)]
pub struct Reactions {
    defaults: Vec<Rule>,
    added: Vec<Rule>,
    /// When each rule last fired, by id. Only rules in force are kept.
    fired: HashMap<String, f64>,
    last_rule: Option<String>,
    random: Random,
}

impl Reactions {
    pub fn new(random: Random, shelf: Option<&dyn Shelf>) -> Self {
        // No saved rules is no added rules.
        let saved = shelf
            .and_then(|shelf| shelf.get_item(SHELF_KEY))
            .and_then(|text| Json::parse(&text).ok());
        Self {
            defaults: tables().rules.iter().filter_map(Rule::read).collect(),
            added: saved
                .as_ref()
                .and_then(Json::list)
                .unwrap_or(&[])
                .iter()
                .filter_map(Rule::read)
                .filter(|rule| is_name(&rule.id, 1, 39, '-'))
                .take(ADDED)
                .collect(),
            fired: HashMap::new(),
            last_rule: None,
            random,
        }
    }

    /// Every rule in force: added ones replace built-in ones of the same id.
    pub fn rules(&self) -> impl Iterator<Item = &Rule> {
        self.defaults
            .iter()
            .filter(|rule| !self.added.iter().any(|item| item.id == rule.id))
            .chain(&self.added)
    }

    /// Adds or replaces a rule after checking it; a refusal says where and why.
    pub fn define(
        &mut self,
        input: &Json,
        by: By,
        channels: &Channels,
        book: &Book,
        shelf: Option<&mut dyn Shelf>,
    ) -> Result<Rule, Refusal> {
        let refuse = |path: &str, reason: String| {
            Err(Refusal {
                path: path.to_owned(),
                reason,
            })
        };
        if input.obj().is_none() {
            return refuse("rule", "A rule is {id, on, do, ...}.".to_owned());
        }
        let Some(id) = input.str_at("id").filter(|id| is_name(id, 1, 39, '-')) else {
            return refuse(
                "rule.id",
                "id is 2 to 40 lowercase letters, digits or hyphens.".to_owned(),
            );
        };
        let (on, single) = match input.get("on") {
            Some(Json::Str(kind)) => (vec![kind.as_str()], true),
            Some(Json::List(kinds)) => (kinds.iter().filter_map(Json::str).collect(), false),
            _ => (Vec::new(), false),
        };
        let listed = input
            .get("on")
            .and_then(Json::list)
            .map_or(1, <[Json]>::len);
        if on.is_empty()
            || on.len() > 6
            || on.len() != listed
            || !on.iter().all(|kind| is_kind(kind))
        {
            return refuse(
                "rule.on",
                "on is an event kind such as \"person.poke\", or up to six in order; a trailing * matches any ending."
                    .to_owned(),
            );
        }
        let until = match input.get("until") {
            None => None,
            Some(Json::Str(until)) if is_kind(until) => Some(until.clone()),
            Some(_) => {
                return refuse(
                    "rule.until",
                    "until is the event kind that ends it, such as \"sound.stop\".".to_owned(),
                );
            }
        };
        let mut when = Vec::new();
        if let Some(given) = input.get("when") {
            let Some(given) = given.obj() else {
                return refuse("rule.when", "when is {gauge: [low, high], ...}.".to_owned());
            };
            for (name, range) in given {
                let path = format!("rule.when.{name}");
                let Some(gauge) = Gauge::parse(name) else {
                    let gauges: Vec<&str> = GAUGES.iter().map(|gauge| gauge.name()).collect();
                    return refuse(
                        &path,
                        format!("No gauge \"{name}\". The gauges are {}.", gauges.join(", ")),
                    );
                };
                match range.list() {
                    Some([Json::Num(low), Json::Num(high)]) => when.push((gauge, *low, *high)),
                    _ => return refuse(&path, "A gauge is bounded as [low, high].".to_owned()),
                }
            }
        }
        let does = input.get("do").cloned().unwrap_or(Json::Null);
        match does.str() {
            Some(name) => {
                if book.get(name).is_none() {
                    return refuse(
                        "rule.do",
                        format!("The book has no performance \"{name}\"."),
                    );
                }
            }
            None => {
                let lookup = |reference: &str| book.lookup(reference);
                let mut context = Context::new(channels);
                context.lookup = Some(&lookup);
                if let Err(refusal) = compile(&does, &context) {
                    return refuse(&format!("rule.do.{}", refusal.path), refusal.reason);
                }
                if length(&does.to_string()) > 4000 {
                    return refuse(
                        "rule.do",
                        "A rule\u{2019}s own program is at most 4000 characters; save it to the book and name it."
                            .to_owned(),
                    );
                }
            }
        }
        let replaces = self.added.iter().position(|item| item.id == id);
        if replaces.is_none() && self.added.len() >= ADDED {
            return refuse(
                "rule",
                format!("At most {ADDED} rules may be added; remove one first."),
            );
        }
        let number = |key: &str, low: f64, high: f64| {
            input
                .num_at(key)
                .filter(|value| value.is_finite())
                .map(|value| clamp(value, low, high))
        };
        let made = Rule {
            id: id.to_owned(),
            on: on.into_iter().map(str::to_owned).collect(),
            single,
            within: number("within", 0.0, 3_600_000.0),
            when,
            does,
            until,
            intensity: number("intensity", 0.0, 2.0),
            weight: number("weight", 0.0, 20.0),
            cooldown: number("cooldown", 0.0, 3_600_000.0),
            note: input.str_at("note").map(|note| clip(note, 200)),
            by,
        };
        match replaces {
            Some(index) => self.added[index] = made.clone(),
            None => self.added.push(made.clone()),
        }
        self.save(shelf);
        Ok(made)
    }

    /// Removes an added rule; a built-in one it replaced comes back.
    pub fn remove(&mut self, id: &str, shelf: Option<&mut dyn Shelf>) -> bool {
        let Some(index) = self.added.iter().position(|item| item.id == id) else {
            return false;
        };
        self.added.remove(index);
        if !self.defaults.iter().any(|rule| rule.id == id) {
            self.fired.remove(id);
        }
        self.save(shelf);
        true
    }

    fn save(&self, shelf: Option<&mut dyn Shelf>) {
        if let Some(shelf) = shelf {
            let added = Json::List(self.added.iter().map(Rule::to_json).collect());
            shelf.set_item(SHELF_KEY, &added.to_string());
        }
    }

    /// The form the face takes for something that has begun and will last: one
    /// of the fitting rules that say what ends it, drawn by weight. Doing
    /// nothing is not a candidate here. A form is how the face shows a thing
    /// is going on, so it is not left out by chance.
    pub fn form(
        &mut self,
        event: &FaceEvent,
        reading: &Reading,
        mood: &Mood,
        memory: &Memory,
        book: &Book,
    ) -> Option<Reaction> {
        let fitting: Vec<&Rule> =
            fitting(&self.defaults, &self.added, event, reading, mood, memory)
                .filter(|rule| rule.until.is_some())
                .collect();
        let weights: Vec<f64> = fitting
            .iter()
            .map(|rule| rule.weight.unwrap_or(1.0))
            .collect();
        let chosen = fitting[self.random.weighted(&weights)?];
        if chosen.named().is_some_and(|name| book.get(name).is_none()) {
            return None;
        }
        Some(Reaction {
            rule: chosen.id.clone(),
            until: chosen.until.clone(),
            does: chosen.does.clone(),
            intensity: clamp(chosen.intensity.unwrap_or(1.0), 0.3, 1.6),
        })
    }

    /// Decides. `None` is a decision too: sometimes the face lets a thing
    /// pass, and more often the more habitual the thing is.
    pub fn react(
        &mut self,
        event: &FaceEvent,
        reading: &Reading,
        mood: &Mood,
        memory: &Memory,
        book: &Book,
    ) -> Option<Reaction> {
        let fitting: Vec<&Rule> =
            fitting(&self.defaults, &self.added, event, reading, mood, memory)
                .filter(|rule| rule.until.is_none())
                .collect();
        if fitting.is_empty() {
            return None;
        }
        let mut weights: Vec<f64> = fitting
            .iter()
            .map(|rule| {
                let since = event.at
                    - self
                        .fired
                        .get(&rule.id)
                        .copied()
                        .unwrap_or(f64::NEG_INFINITY);
                let cooldown = rule.cooldown.unwrap_or(4_000.0);
                let rested = if cooldown > 0.0 {
                    1.0 - (-since / cooldown).exp()
                } else {
                    1.0
                };
                let repeat = if self.last_rule.as_deref() == Some(&rule.id) {
                    0.35
                } else {
                    1.0
                };
                rule.weight.unwrap_or(1.0) * rested * repeat
            })
            .collect();
        let total: f64 = weights.iter().sum();
        // Doing nothing weighs more for a habitual, familiar thing, and never for a first.
        weights.push(total * (0.08 + 0.5 * reading.habit) * (1.0 - reading.novelty * 0.9));
        let chosen = *fitting.get(self.random.weighted(&weights)?)?;
        if chosen.named().is_some_and(|name| book.get(name).is_none()) {
            return None;
        }
        let reaction = Reaction {
            rule: chosen.id.clone(),
            until: None,
            does: chosen.does.clone(),
            intensity: clamp(
                chosen.intensity.unwrap_or(1.0)
                    * (0.6 + 0.55 * reading.novelty)
                    * (0.85 + 0.4 * mood.arousal),
                0.3,
                1.6,
            ),
        };
        self.fired.insert(reaction.rule.clone(), event.at);
        self.last_rule = Some(reaction.rule.clone());
        Some(reaction)
    }
}
