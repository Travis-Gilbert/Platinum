//! Checks a program and lays it out in time. A program is plain JSON so a
//! model, a rule or a person can write one; nothing is run here. A wrong
//! program is refused with the place and the reason; a value out of bounds is
//! pulled in and noted instead.
use crate::channels::{Channels, clamp, is_feature_id};
use crate::json::{Json, js_number, quote};
use crate::random::{Random, round};
use crate::signals::is_signal;
use crate::tables::{FeatureDef, Side, tables};

/// Something particular the host shows, by the name the host gives it, and optionally one part of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Thing {
    /// `kind:id`, or `kind:last` and `kind:failed` for the newest of a kind.
    pub thing: String,
    /// The first place these words appear in it.
    pub text: Option<String>,
    /// A numbered line of it, where it has numbered lines.
    pub line: Option<f64>,
}

/// Where the face can be told to look or go.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    /// One of `PLACES`.
    Named(&'static str),
    Thing(Thing),
    /// A point: of the gaze for `look` (-1..1), of the window for `go` (0..1).
    Spot {
        x: f64,
        y: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    Sine,
    Square,
    Noise,
}

/// What happens at a moment of a performance.
#[derive(Clone, Debug, PartialEq)]
pub enum CueKind {
    Hold {
        channel: String,
        value: f64,
        relative: bool,
        over: f64,
        bounce: f64,
    },
    Wave {
        channel: String,
        amp: f64,
        period: f64,
        until: f64,
        shape: Wave,
        phase: f64,
    },
    Follow {
        channel: String,
        signal: String,
        amp: f64,
        /// When it lets go; `None` holds as long as the performance does.
        until: Option<f64>,
    },
    Look {
        place: Place,
        over: f64,
    },
    Feature {
        id: String,
        side: Side,
        remove: bool,
    },
    Go {
        place: Place,
        size: Option<f64>,
        over: f64,
    },
    Burst {
        x: f64,
        y: f64,
        strength: f64,
    },
    Release {
        channels: Option<Vec<String>>,
        over: f64,
    },
}

/// Something that happens at a moment of a performance, ready to run.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    /// Milliseconds from the start.
    pub at: f64,
    pub kind: CueKind,
}

/// A checked performance: its cues in order, how long it runs, what was adjusted.
#[derive(Clone, Debug, PartialEq)]
pub struct Score {
    pub cues: Vec<Cue>,
    pub ms: f64,
    pub keep: bool,
    pub seed: u32,
    /// Things the checker changed rather than refused, such as a value pulled into bounds.
    pub notes: Vec<String>,
    /// Features the performance adds to the face as it stood when checked.
    pub adds: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    /// Where in the program, such as `steps[2].set["eye.w"]`.
    pub path: String,
    pub reason: String,
}

/// A performance as written: steps, and whether what it set stays when it ends.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub steps: Vec<Json>,
    pub keep: bool,
}

/// What compiling needs from where it runs.
pub struct Context<'a> {
    pub channels: &'a Channels,
    /// Looks a named performance up in the book.
    pub lookup: Option<&'a dyn Fn(&str) -> Option<Program>>,
    pub seed: Option<u32>,
    pub args: Vec<(String, f64)>,
    /// Scales how far from rest the performance goes: 0 nothing, 1 as written, up to 2.
    pub intensity: Option<f64>,
    /// What a thing a step names comes to in the host: what there is to say about it, or `None` when nothing has the name. Left out, things are not looked for.
    pub thing: Option<&'a dyn Fn(&Thing) -> Option<Vec<String>>>,
    /// A thing that is not there refuses the program; otherwise it is a note.
    pub strict: bool,
}

impl<'a> Context<'a> {
    pub fn new(channels: &'a Channels) -> Self {
        Self {
            channels,
            lookup: None,
            seed: None,
            args: Vec::new(),
            intensity: None,
            thing: None,
            strict: false,
        }
    }
}

pub mod limits {
    /// The longest a performance may run, in milliseconds.
    pub const MS: f64 = 30_000.0;
    /// The most cues one may compile to.
    pub const CUES: usize = 600;
    pub const DEPTH: usize = 10;
    pub const REPEAT: f64 = 24.0;
    pub const USES: usize = 4;
    /// The most characters of `text` a thing is searched for.
    pub const TEXT: usize = 200;
}

pub const PLACES: [&str; 7] = [
    "person", "pointer", "centre", "composer", "text", "home", "artifact",
];

/// Every step there is, in the order the manual lists them.
pub const STEPS: [&str; 16] = [
    "to", "by", "wave", "follow", "wait", "seq", "par", "stagger", "repeat", "vary", "look",
    "feature", "go", "burst", "use", "release",
];

/// Accepts a program, a bare list of steps, or a single step.
pub fn as_program(input: &Json) -> Option<Program> {
    match input {
        Json::List(steps) => Some(Program {
            steps: steps.clone(),
            keep: false,
        }),
        Json::Obj(_) => {
            if let (Some(steps), None) = (input.get("steps").and_then(Json::list), input.get("op"))
            {
                return Some(Program {
                    steps: steps.to_vec(),
                    keep: input.get("keep") == Some(&Json::Bool(true)),
                });
            }
            input.str_at("op").map(|_| Program {
                steps: vec![input.clone()],
                keep: false,
            })
        }
        _ => None,
    }
}

struct Run<'a> {
    view: Channels,
    random: Random,
    cues: Vec<Cue>,
    notes: Vec<String>,
    adds: Vec<String>,
    lookup: Option<&'a dyn Fn(&str) -> Option<Program>>,
    thing: Option<&'a dyn Fn(&Thing) -> Option<Vec<String>>>,
    strict: bool,
}

#[derive(Clone)]
struct Frame {
    args: Vec<(String, f64)>,
    intensity: f64,
    depth: usize,
    uses: usize,
}

type Fails<T> = Result<T, Refusal>;

fn fail<T>(path: impl Into<String>, reason: impl Into<String>) -> Fails<T> {
    Err(Refusal {
        path: path.into(),
        reason: reason.into(),
    })
}

/// Checks a program and lays it out in time. Nothing is run.
pub fn compile(input: &Json, context: &Context<'_>) -> Result<Score, Refusal> {
    let Some(program) = as_program(input) else {
        return fail(
            "",
            "A program is {steps:[...]}, a list of steps, or one step.",
        );
    };
    let seed = context.seed.unwrap_or(1);
    // Features the program adds have channels from its first step on.
    let features: Vec<FeatureDef> = context.channels.features().cloned().collect();
    let mut run = Run {
        view: Channels::new(&features),
        random: Random::new(seed),
        cues: Vec::new(),
        notes: Vec::new(),
        adds: Vec::new(),
        lookup: context.lookup,
        thing: context.thing,
        strict: context.strict,
    };
    declare_features(&program.steps, "steps", &mut run, 0)?;
    let frame = Frame {
        args: context.args.clone(),
        intensity: clamp(context.intensity.unwrap_or(1.0), 0.0, 2.0),
        depth: 0,
        uses: 0,
    };
    let ms = sequence(&program.steps, "steps", 0.0, &mut run, &frame)?;
    if ms > limits::MS {
        return fail(
            "",
            format!(
                "It runs {} ms; the most is {}.",
                js_number(round(ms)),
                limits::MS
            ),
        );
    }
    // A stable sort: cues at one moment stay in the order they were written.
    run.cues.sort_by(|a, b| a.at.total_cmp(&b.at));
    Ok(Score {
        cues: run.cues,
        ms,
        keep: program.keep,
        seed,
        notes: run.notes,
        adds: run.adds,
    })
}

fn guess_side(id: &str) -> &'static str {
    if id.ends_with(".left") {
        "left"
    } else if id.ends_with(".right") {
        "right"
    } else {
        "centre"
    }
}

/// Finds every feature a program adds, so later steps may name its channels.
fn declare_features(steps: &[Json], path: &str, run: &mut Run<'_>, depth: usize) -> Fails<()> {
    if depth > limits::DEPTH {
        return Ok(());
    }
    for (index, step) in steps.iter().enumerate() {
        if step.obj().is_none() {
            continue;
        }
        let here = format!("{path}[{index}]");
        if step.str_at("op") == Some("feature") && step.get("remove") != Some(&Json::Bool(true)) {
            let Some(id) = step.str_at("id").filter(|id| is_feature_id(id)) else {
                return fail(
                    format!("{here}.id"),
                    "A feature id is a short lowercase name such as \"brow.left\".",
                );
            };
            let side = match step.get("side") {
                None | Some(Json::Null) => Side::parse(guess_side(id)),
                Some(side) => side.str().and_then(Side::parse),
            };
            let Some(side) = side else {
                return fail(
                    format!("{here}.side"),
                    "side is \"left\", \"right\" or \"centre\".",
                );
            };
            if run.view.feature(id).is_none() {
                let def = FeatureDef {
                    id: id.to_owned(),
                    side,
                    rest: Vec::new(),
                };
                if let Err(refusal) = run.view.add_feature(&def) {
                    return fail(here, refusal);
                }
                if !run.adds.iter().any(|added| added == id) {
                    run.adds.push(id.to_owned());
                }
            }
        }
        for key in ["steps", "of"] {
            if let Some(inner) = step.get(key).and_then(Json::list) {
                declare_features(inner, &format!("{here}.{key}"), run, depth + 1)?;
            }
        }
        if let Some(inner) = step.get("step").filter(|inner| inner.obj().is_some()) {
            declare_features(
                std::slice::from_ref(inner),
                &format!("{here}.step"),
                run,
                depth + 1,
            )?;
        }
    }
    Ok(())
}

fn sequence(steps: &[Json], path: &str, at: f64, run: &mut Run<'_>, frame: &Frame) -> Fails<f64> {
    let mut cursor = at;
    for (index, step) in steps.iter().enumerate() {
        cursor += place(step, &format!("{path}[{index}]"), cursor, run, frame)?;
    }
    Ok(cursor - at)
}

fn steps_of<'j>(value: Option<&'j Json>, path: &str, reason: &str) -> Fails<&'j [Json]> {
    match value.and_then(Json::list) {
        Some(steps) => Ok(steps),
        None => fail(path, reason),
    }
}

fn duration(value: Option<&Json>, path: &str, fallback: f64) -> Fails<f64> {
    match value {
        None => Ok(fallback),
        Some(Json::Num(value)) if value.is_finite() && *value >= 0.0 => Ok(value.min(limits::MS)),
        Some(_) => fail(path, "This is a time in milliseconds, 0 or more."),
    }
}

fn number(value: Option<&Json>, path: &str, run: &mut Run<'_>, frame: &Frame) -> Fails<f64> {
    match value {
        Some(Json::Num(value)) if value.is_finite() => return Ok(*value),
        Some(written @ Json::Obj(_)) => {
            if let Some([Json::Num(low), Json::Num(high)]) =
                written.get("rand").and_then(Json::list)
                && low.is_finite()
                && high.is_finite()
            {
                return Ok(run.random.between(*low, *high));
            }
            if let Some(arg) = written.str_at("arg") {
                // A later argument of the same name wins, as it does when two sets are spread into one.
                if let Some((_, given)) = frame
                    .args
                    .iter()
                    .rev()
                    .find(|(name, given)| name == arg && given.is_finite())
                {
                    return Ok(*given);
                }
                if let Some(or) = written.num_at("or").filter(|or| or.is_finite()) {
                    return Ok(or);
                }
                return fail(
                    path,
                    format!(
                        "This needs the argument \"{arg}\", which was not passed and has no \"or\"."
                    ),
                );
            }
        }
        _ => {}
    }
    fail(
        path,
        "This is a number, {rand:[low,high]} or {arg:\"name\", or:number}.",
    )
}

fn channels_of(name: &str, path: &str, run: &Run<'_>) -> Fails<Vec<String>> {
    let channels = run.view.expand(name);
    if channels.is_empty() {
        let near = run
            .view
            .nearest(name)
            .map(|near| format!(" Did you mean \"{near}\"?"))
            .unwrap_or_default();
        return fail(path, format!("No channel \"{name}\".{near}"));
    }
    Ok(channels)
}

/// A thing's name: a kind, a colon, and the rest (`^[a-z][a-z0-9-]{0,23}:\S{1,160}$`).
fn is_thing_name(name: &str) -> bool {
    let Some((kind, rest)) = name.split_once(':') else {
        return false;
    };
    let kind = kind.as_bytes();
    let rest_units = rest.encode_utf16().count();
    !kind.is_empty()
        && kind.len() <= 24
        && kind[0].is_ascii_lowercase()
        && kind
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        && (1..=160).contains(&rest_units)
        && !rest
            .chars()
            .any(|character| character.is_whitespace() || character == '\u{feff}')
}

fn thing_of(value: &Json, path: &str) -> Fails<Thing> {
    let Some(thing) = value.str_at("thing").filter(|name| is_thing_name(name)) else {
        return fail(
            format!("{path}.thing"),
            "thing is a name the page lists, as \"kind:id\": \"tool:failed\", \"artifact:last\".",
        );
    };
    let text = match value.get("text") {
        None => None,
        Some(Json::Str(text)) if !text.trim().is_empty() => {
            let units: Vec<u16> = text.trim().encode_utf16().take(limits::TEXT).collect();
            Some(String::from_utf16_lossy(&units))
        }
        Some(_) => {
            return fail(
                format!("{path}.text"),
                "text is the words to find in the thing.",
            );
        }
    };
    let line = match value.get("line") {
        None => None,
        Some(Json::Num(line)) if line.fract() == 0.0 && *line >= 1.0 => Some(*line),
        Some(_) => return fail(format!("{path}.line"), "line is a line number, from 1."),
    };
    Ok(Thing {
        thing: thing.to_owned(),
        text,
        line,
    })
}

/// A thing as the host has it now: refused or noted when it is not there, noted when it is not simply there.
fn on_page(thing: Thing, path: &str, run: &mut Run<'_>) -> Fails<Thing> {
    let Some(find) = run.thing else {
        return Ok(thing);
    };
    let note = |text: &str, notes: &mut Vec<String>| {
        let line = format!("{path}: {text}");
        if !notes.contains(&line) {
            notes.push(line);
        }
    };
    match find(&thing) {
        Some(said) => {
            for text in said {
                note(&text, &mut run.notes);
            }
        }
        None if run.strict => {
            return fail(
                format!("{path}.thing"),
                format!(
                    "Nothing on the page is named \"{}\". face.read with part \"page\" lists what is.",
                    thing.thing
                ),
            );
        }
        None => note(
            &format!(
                "nothing on the page is named \"{}\"; the face stays where it is.",
                thing.thing
            ),
            &mut run.notes,
        ),
    }
    Ok(thing)
}

fn place_of(
    value: Option<&Json>,
    path: &str,
    bounds: (f64, f64),
    run: &mut Run<'_>,
) -> Fails<Place> {
    if let Some(name) = value.and_then(Json::str)
        && let Some(known) = PLACES.iter().find(|place| **place == name)
    {
        return Ok(Place::Named(known));
    }
    if let Some(written @ Json::Obj(_)) = value {
        if written.get("thing").is_some() {
            let thing = thing_of(written, path)?;
            return on_page(thing, path, run).map(Place::Thing);
        }
        if let (Some(x), Some(y)) = (written.num_at("x"), written.num_at("y")) {
            return Ok(Place::Spot {
                x: clamp(x, bounds.0, bounds.1),
                y: clamp(y, bounds.0, bounds.1),
            });
        }
    }
    let places: Vec<String> = PLACES.iter().map(|place| format!("\"{place}\"")).collect();
    fail(
        path,
        format!(
            "This is one of {}, {{x, y}} or {{thing}}.",
            places.join(", ")
        ),
    )
}

fn push(run: &mut Run<'_>, at: f64, kind: CueKind, path: &str) -> Fails<()> {
    if run.cues.len() >= limits::CUES {
        return fail(
            path,
            format!("A performance has at most {} cues.", limits::CUES),
        );
    }
    run.cues.push(Cue { at, kind });
    Ok(())
}

/// Lays `set` out as holds: the body of `to` and `by`, and of a feature's own `set`, whose params are given their feature's name.
#[allow(clippy::too_many_arguments)]
fn holds(
    set: &[(String, Json)],
    prefix: Option<&str>,
    relative: bool,
    over: f64,
    bounce: f64,
    path: &str,
    at: f64,
    run: &mut Run<'_>,
    frame: &Frame,
) -> Fails<()> {
    for (written_name, raw) in set {
        let name = match prefix {
            Some(prefix) => format!("{prefix}.{written_name}"),
            None => written_name.clone(),
        };
        let here = format!("{path}.set[{}]", quote(&name));
        let written = number(Some(raw), &here, run, frame)?;
        for channel in channels_of(&name, &here, run)? {
            let Some(spec) = run.view.spec(&channel).cloned() else {
                continue;
            };
            let mut value = if relative {
                written * frame.intensity
            } else {
                spec.rest + (written - spec.rest) * frame.intensity
            };
            if !relative {
                let inside = clamp(value, spec.min, spec.max);
                if inside != value && frame.intensity == 1.0 {
                    run.notes.push(format!(
                        "{channel} {} is outside {}..{}; used {}.",
                        js_number(written),
                        js_number(spec.min),
                        js_number(spec.max),
                        js_number(inside)
                    ));
                }
                value = inside;
            }
            push(
                run,
                at,
                CueKind::Hold {
                    channel,
                    value,
                    relative,
                    over,
                    bounce,
                },
                &here,
            )?;
        }
    }
    Ok(())
}

/// Lays one step out at a time and returns how long it takes.
fn place(step: &Json, path: &str, at: f64, run: &mut Run<'_>, frame: &Frame) -> Fails<f64> {
    let Some(op) = step.obj().and_then(|_| step.str_at("op")) else {
        return fail(path, "A step is an object with an \"op\".");
    };
    if frame.depth > limits::DEPTH {
        return fail(path, "Steps are nested too deeply.");
    }
    let inner = Frame {
        depth: frame.depth + 1,
        ..frame.clone()
    };
    let get = |key: &str| step.get(key);
    match op {
        "to" | "by" => {
            let Some(set) = get("set").and_then(Json::obj) else {
                return fail(format!("{path}.set"), "set is {channel: value, ...}.");
            };
            let over = duration(get("over"), &format!("{path}.over"), 240.0)?;
            let bounce = clamp(step.num_at("bounce").unwrap_or(0.0), 0.0, 1.0);
            holds(set, None, op == "by", over, bounce, path, at, run, frame)?;
            Ok(over + duration(get("hold"), &format!("{path}.hold"), 0.0)?)
        }
        "wave" => {
            let Some(channel) = step.str_at("channel") else {
                return fail(format!("{path}.channel"), "channel is a channel name.");
            };
            let period = duration(get("period"), &format!("{path}.period"), 600.0)?;
            if period < 40.0 {
                return fail(format!("{path}.period"), "A wave period is at least 40 ms.");
            }
            let length = duration(get("for"), &format!("{path}.for"), period * 2.0)?;
            let shape = match get("shape") {
                None | Some(Json::Null) => Wave::Sine,
                Some(Json::Str(shape)) if shape == "sine" => Wave::Sine,
                Some(Json::Str(shape)) if shape == "square" => Wave::Square,
                Some(Json::Str(shape)) if shape == "noise" => Wave::Noise,
                Some(_) => {
                    return fail(
                        format!("{path}.shape"),
                        "shape is \"sine\", \"square\" or \"noise\".",
                    );
                }
            };
            let amp = number(get("amp"), &format!("{path}.amp"), run, frame)? * frame.intensity;
            let phase = run.random.next();
            for channel in channels_of(channel, &format!("{path}.channel"), run)? {
                push(
                    run,
                    at,
                    CueKind::Wave {
                        channel,
                        amp,
                        period,
                        until: at + length,
                        shape,
                        phase,
                    },
                    path,
                )?;
            }
            Ok(length)
        }
        "follow" => {
            let Some(channel) = step.str_at("channel") else {
                return fail(format!("{path}.channel"), "channel is a channel name.");
            };
            let Some(signal) = step.str_at("signal").filter(|signal| is_signal(signal)) else {
                let names: Vec<&str> = tables().signals.iter().map(|spec| spec.name).collect();
                return fail(
                    format!("{path}.signal"),
                    format!("signal is one of {}.", names.join(", ")),
                );
            };
            let length = match get("for") {
                None => None,
                given => Some(duration(given, &format!("{path}.for"), 0.0)?),
            };
            let amp = number(get("amp"), &format!("{path}.amp"), run, frame)? * frame.intensity;
            for channel in channels_of(channel, &format!("{path}.channel"), run)? {
                push(
                    run,
                    at,
                    CueKind::Follow {
                        channel,
                        signal: signal.to_owned(),
                        amp,
                        until: length.map(|length| at + length),
                    },
                    path,
                )?;
            }
            Ok(length.unwrap_or(0.0))
        }
        "wait" => Ok(number(get("ms"), &format!("{path}.ms"), run, frame)?
            .min(limits::MS)
            .max(0.0)),
        "seq" => {
            let path = format!("{path}.steps");
            let steps = steps_of(get("steps"), &path, "This must be a list of steps.")?;
            sequence(steps, &path, at, run, &inner)
        }
        "par" => {
            let steps = steps_of(
                get("steps"),
                &format!("{path}.steps"),
                "steps is a list of steps.",
            )?;
            let mut longest: f64 = 0.0;
            for (index, child) in steps.iter().enumerate() {
                longest = longest.max(place(
                    child,
                    &format!("{path}.steps[{index}]"),
                    at,
                    run,
                    &inner,
                )?);
            }
            Ok(longest)
        }
        "stagger" => {
            let steps = steps_of(
                get("steps"),
                &format!("{path}.steps"),
                "steps is a list of steps.",
            )?;
            let gap = duration(get("gap"), &format!("{path}.gap"), 80.0)?;
            let mut longest: f64 = 0.0;
            for (index, child) in steps.iter().enumerate() {
                let start = index as f64 * gap;
                longest = longest.max(
                    start
                        + place(
                            child,
                            &format!("{path}.steps[{index}]"),
                            at + start,
                            run,
                            &inner,
                        )?,
                );
            }
            Ok(longest)
        }
        "repeat" => {
            let Some(times) = step
                .num_at("times")
                .filter(|times| times.fract() == 0.0 && *times >= 1.0 && *times <= limits::REPEAT)
            else {
                return fail(
                    format!("{path}.times"),
                    format!("times is a whole number from 1 to {}.", limits::REPEAT),
                );
            };
            let mut cursor = at;
            for _ in 0..times as usize {
                // A step that is missing is refused where it would have been placed.
                cursor += place(
                    get("step").unwrap_or(&Json::Null),
                    &format!("{path}.step"),
                    cursor,
                    run,
                    &inner,
                )?;
            }
            Ok(cursor - at)
        }
        "vary" => {
            let Some(options) = get("of").and_then(Json::list).filter(|of| !of.is_empty()) else {
                return fail(format!("{path}.of"), "of is a list of one or more steps.");
            };
            let written = get("weights").and_then(Json::list).unwrap_or(&[]);
            let weights: Vec<f64> = (0..options.len())
                .map(|index| written.get(index).and_then(Json::num).unwrap_or(1.0))
                .collect();
            let chosen = run.random.weighted(&weights).unwrap_or(0);
            place(
                &options[chosen],
                &format!("{path}.of[{chosen}]"),
                at,
                run,
                &inner,
            )
        }
        "look" => {
            let over = duration(get("over"), &format!("{path}.over"), 200.0)?;
            let place = place_of(get("at"), &format!("{path}.at"), (-1.0, 1.0), run)?;
            push(run, at, CueKind::Look { place, over }, path)?;
            Ok(over + duration(get("hold"), &format!("{path}.hold"), 0.0)?)
        }
        "feature" => {
            let Some(id) = step.str_at("id") else {
                return fail(format!("{path}.id"), "id is the feature’s name.");
            };
            let Some(side) = run.view.feature(id).map(|known| known.side) else {
                return fail(
                    format!("{path}.id"),
                    format!("The face has no feature \"{id}\"."),
                );
            };
            let remove = get("remove") == Some(&Json::Bool(true));
            push(
                run,
                at,
                CueKind::Feature {
                    id: id.to_owned(),
                    side,
                    remove,
                },
                path,
            )?;
            if !remove && let Some(set) = get("set") {
                let Some(set) = set.obj() else {
                    return fail(format!("{path}.set"), "set is {param: value, ...}.");
                };
                holds(set, Some(id), false, 0.0, 0.0, path, at, run, &inner)?;
            }
            Ok(0.0)
        }
        "go" => {
            let over = duration(get("over"), &format!("{path}.over"), 620.0)?;
            let size = match get("size") {
                None => None,
                Some(Json::Num(size)) if size.is_finite() => Some(clamp(*size, 0.5, 4.0)),
                Some(_) => {
                    return fail(format!("{path}.size"), "size is a number from 0.5 to 4.");
                }
            };
            let place = place_of(get("to"), &format!("{path}.to"), (0.0, 1.0), run)?;
            push(run, at, CueKind::Go { place, size, over }, path)?;
            Ok(over)
        }
        "burst" => {
            let read = |key: &str| {
                step.num_at(key)
                    .filter(|value| value.is_finite())
                    .unwrap_or(0.0)
            };
            let strength = match get("strength") {
                None => 120.0,
                given => number(given, &format!("{path}.strength"), run, frame)?,
            };
            push(
                run,
                at,
                CueKind::Burst {
                    x: clamp(read("x"), -50.0, 50.0),
                    y: clamp(read("y"), -32.0, 32.0),
                    strength: clamp(strength, 0.0, 600.0) * frame.intensity,
                },
                path,
            )?;
            Ok(0.0)
        }
        "use" => {
            let Some(name) = step.str_at("name") else {
                return fail(format!("{path}.name"), "name is a performance in the book.");
            };
            if frame.uses >= limits::USES {
                return fail(path, "Performances are used inside each other too deeply.");
            }
            let Some(found) = run.lookup.and_then(|lookup| lookup(name)) else {
                return fail(
                    format!("{path}.name"),
                    format!("The book has no performance \"{name}\"."),
                );
            };
            let mut args = frame.args.clone();
            if let Some(passed) = get("args").and_then(Json::obj) {
                args.extend(
                    passed
                        .iter()
                        .filter_map(|(name, value)| Some((name.clone(), value.num()?))),
                );
            }
            let scale = step
                .num_at("intensity")
                .map_or(1.0, |scale| clamp(scale, 0.0, 2.0));
            let inside = format!("{path}<{name}>");
            declare_features(&found.steps, &inside, run, 0)?;
            sequence(
                &found.steps,
                &inside,
                at,
                run,
                &Frame {
                    args,
                    intensity: clamp(frame.intensity * scale, 0.0, 2.0),
                    depth: frame.depth + 1,
                    uses: frame.uses + 1,
                },
            )
        }
        "release" => {
            let over = duration(get("over"), &format!("{path}.over"), 340.0)?;
            let channels = match get("channels") {
                None => None,
                Some(Json::List(names)) => {
                    let mut channels = Vec::new();
                    for (index, name) in names.iter().enumerate() {
                        let here = format!("{path}.channels[{index}]");
                        let Some(name) = name.str() else {
                            return fail(here, "A channel name is text.");
                        };
                        channels.extend(channels_of(name, &here, run)?);
                    }
                    Some(channels)
                }
                Some(_) => {
                    return fail(
                        format!("{path}.channels"),
                        "channels is a list of channel names.",
                    );
                }
            };
            push(run, at, CueKind::Release { channels, over }, path)?;
            Ok(0.0)
        }
        other => fail(
            format!("{path}.op"),
            format!("No step \"{other}\". The steps are {}.", STEPS.join(", ")),
        ),
    }
}

fn entry(key: &str, value: impl Into<Json>) -> (String, Json) {
    (key.to_owned(), value.into())
}

impl Place {
    /// The place as a program writes it.
    pub fn to_json(&self) -> Json {
        match self {
            Place::Named(name) => Json::Str((*name).to_owned()),
            Place::Spot { x, y } => Json::Obj(vec![entry("x", *x), entry("y", *y)]),
            Place::Thing(thing) => {
                let mut entries = vec![entry("thing", thing.thing.as_str())];
                if let Some(text) = &thing.text {
                    entries.push(entry("text", text.as_str()));
                }
                if let Some(line) = thing.line {
                    entries.push(entry("line", line));
                }
                Json::Obj(entries)
            }
        }
    }
}

impl Cue {
    /// The cue as the web writes it, for comparing the two and for reading a score.
    pub fn to_json(&self) -> Json {
        let mut entries = vec![entry("at", self.at)];
        let texts = |names: &[String]| {
            Json::List(names.iter().map(|name| Json::Str(name.clone())).collect())
        };
        match &self.kind {
            CueKind::Hold {
                channel,
                value,
                relative,
                over,
                bounce,
            } => entries.extend([
                entry("kind", "hold"),
                entry("channel", channel.as_str()),
                entry("value", *value),
                ("relative".to_owned(), Json::Bool(*relative)),
                entry("over", *over),
                entry("bounce", *bounce),
            ]),
            CueKind::Wave {
                channel,
                amp,
                period,
                until,
                shape,
                phase,
            } => entries.extend([
                entry("kind", "wave"),
                entry("channel", channel.as_str()),
                entry("amp", *amp),
                entry("period", *period),
                entry("until", *until),
                entry(
                    "shape",
                    match shape {
                        Wave::Sine => "sine",
                        Wave::Square => "square",
                        Wave::Noise => "noise",
                    },
                ),
                entry("phase", *phase),
            ]),
            CueKind::Follow {
                channel,
                signal,
                amp,
                until,
            } => {
                entries.extend([
                    entry("kind", "follow"),
                    entry("channel", channel.as_str()),
                    entry("signal", signal.as_str()),
                    entry("amp", *amp),
                ]);
                if let Some(until) = until {
                    entries.push(entry("until", *until));
                }
            }
            CueKind::Look { place, over } => entries.extend([
                entry("kind", "look"),
                ("place".to_owned(), place.to_json()),
                entry("over", *over),
            ]),
            CueKind::Feature { id, side, remove } => entries.extend([
                entry("kind", "feature"),
                entry("id", id.as_str()),
                entry("side", side.name()),
                ("remove".to_owned(), Json::Bool(*remove)),
            ]),
            CueKind::Go { place, size, over } => {
                entries.extend([entry("kind", "go"), ("place".to_owned(), place.to_json())]);
                if let Some(size) = size {
                    entries.push(entry("size", *size));
                }
                entries.push(entry("over", *over));
            }
            CueKind::Burst { x, y, strength } => entries.extend([
                entry("kind", "burst"),
                entry("x", *x),
                entry("y", *y),
                entry("strength", *strength),
            ]),
            CueKind::Release { channels, over } => {
                entries.push(entry("kind", "release"));
                if let Some(channels) = channels {
                    entries.push(("channels".to_owned(), texts(channels)));
                }
                entries.push(entry("over", *over));
            }
        }
        Json::Obj(entries)
    }
}

impl Score {
    pub fn to_json(&self) -> Json {
        let texts = |names: &[String]| {
            Json::List(names.iter().map(|name| Json::Str(name.clone())).collect())
        };
        Json::Obj(vec![
            (
                "cues".to_owned(),
                Json::List(self.cues.iter().map(Cue::to_json).collect()),
            ),
            entry("ms", self.ms),
            ("keep".to_owned(), Json::Bool(self.keep)),
            entry("seed", f64::from(self.seed)),
            ("notes".to_owned(), texts(&self.notes)),
            ("adds".to_owned(), texts(&self.adds)),
        ])
    }
}
