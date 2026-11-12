//! Whole scenarios, act by act, against what web Theo did with the same seed
//! on the same clock: the channels, the head, what leaves its sides, what it
//! says of itself, the shapes and the grains.
mod common;

use common::{differs, fixture, list, num};
use theo::engine::{Edge, Expression, Face, FaceOptions, How, Liveness, PlainStage};
use theo::json::Json;
use theo::layers::Layer;
use theo::particles::GRAIN_STRIDE;
use theo::player::Invocation;

/// On Linux x86-64 every number is the web's to the last bit. The slack is for a platform whose `sin`, `exp` and `powf` round the last place differently, which the grains then carry through thousands of steps.
const NEAR: f64 = 1e-9;
const GRAIN: f64 = 1e-4;

fn gaze(act: &Json) -> Option<(f64, f64)> {
    let gaze = act.get("gaze")?;
    Some((gaze.num_at("x")?, gaze.num_at("y")?))
}

fn named(invocation: Option<Invocation>) -> Json {
    invocation
        .and_then(|invocation| invocation.name)
        .map_or(Json::Null, Json::Str)
}

fn how(written: &Json) -> How {
    let mut how = How::by(written.str_at("owner").unwrap_or(""));
    how.layer = written.str_at("layer").and_then(Layer::parse);
    how.seed = written.num_at("seed").map(|seed| seed as u32);
    how.intensity = written.num_at("intensity");
    how.keep = written.get("keep").and_then(Json::bool).unwrap_or(false);
    how.strict = written.get("strict").and_then(Json::bool).unwrap_or(false);
    if let Some(args) = written.get("args").and_then(Json::obj) {
        how.args = args
            .iter()
            .map(|(name, value)| (name.clone(), value.num().expect("a number")))
            .collect();
    }
    how
}

fn expression(written: &Json) -> Expression {
    Expression {
        energy: written.num_at("energy"),
        breath: written.num_at("breath"),
        tilt: written.num_at("tilt"),
        hue: written.num_at("hue"),
        gaze: written
            .get("gaze")
            .and_then(Json::list)
            .map(|gaze| (gaze[0].num().unwrap(), gaze[1].num().unwrap())),
    }
}

fn object(fields: Vec<(&str, Json)>) -> Json {
    Json::Obj(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn numbers(values: impl IntoIterator<Item = f64>) -> Json {
    Json::List(values.into_iter().map(Json::Num).collect())
}

/// Does one act, and gives what the web would have recorded as its result.
fn act(face: &mut Face, now: &mut f64, act: &Json) -> Option<Json> {
    let number = |key: &str| num(act, key);
    match act.str_at("act").expect("an act") {
        "run" => {
            let mut t = 0.0;
            while t < number("ms") {
                *now += 8.0;
                face.set_now(*now);
                face.advance(0.008);
                t += 8.0;
            }
            None
        }
        "notice" => Some(named(face.notice(
            act.str_at("kind").unwrap(),
            act.num_at("weight").unwrap_or(1.0),
        ))),
        "liveness" => {
            face.set_liveness(Liveness::parse(act.str_at("to").unwrap()).expect("a liveness"));
            None
        }
        "feed" => {
            for (name, value) in act.get("signals").and_then(Json::obj).unwrap() {
                face.feed(name, value.num().unwrap());
            }
            None
        }
        "still" => {
            face.set_still(act.get("on").and_then(Json::bool).unwrap());
            None
        }
        "poke" => Some(named(face.poke(number("x"), number("y")))),
        "perform" => {
            let how = act.get("how").map(how).unwrap_or_default();
            Some(match face.perform(act.get("input").unwrap(), how) {
                Ok(performed) => object(vec![
                    ("ok", Json::Bool(true)),
                    (
                        "notes",
                        Json::List(performed.notes.into_iter().map(Json::Str).collect()),
                    ),
                ]),
                Err(refusal) => object(vec![
                    ("ok", Json::Bool(false)),
                    ("path", Json::Str(refusal.path)),
                    ("reason", Json::Str(refusal.reason)),
                ]),
            })
        }
        "pointer" => {
            face.follow_pointer(gaze(act));
            None
        }
        "watch" => {
            face.watch(gaze(act));
            None
        }
        "heading" => {
            face.heading(gaze(act));
            None
        }
        "grab" => {
            face.grab();
            None
        }
        "carry" => {
            face.carry(number("dx"), number("dy"));
            None
        }
        "drop" => Some(named(face.drop_down())),
        "toss" => Some(named(face.toss(number("force")))),
        "sway" => {
            face.sway(number("dx"), number("dy"), number("seconds"));
            None
        }
        "bump" => Some(named(face.bump(
            Edge::parse(act.str_at("side").unwrap()).expect("a side"),
            number("force"),
            act.str_at("against"),
        ))),
        "express" => {
            let written = act
                .get("expression")
                .filter(|written| written.obj().is_some());
            face.express(written.map(expression).as_ref());
            None
        }
        "tint" => {
            face.tint(number("hue"));
            None
        }
        "unform" => {
            face.unform();
            None
        }
        other => panic!("no act {other}"),
    }
}

/// The face as the web's snapshot writes it, in two parts: what is exact to `NEAR`, and the grains.
fn snapshot(face: &Face) -> (Json, Json) {
    let channels = Json::Obj(
        face.rig
            .channels
            .names()
            .into_iter()
            .map(|name| (name.to_owned(), Json::Num(face.rig.read(name))))
            .collect(),
    );
    let head = face.head();
    let emitting = face.emitting();
    let extent = face.field.extent();
    let near = object(vec![
        ("channels", channels),
        (
            "head",
            object(vec![
                ("tilt", Json::Num(head.tilt)),
                ("scale", Json::Num(head.scale)),
                ("dx", Json::Num(head.dx)),
                ("dy", Json::Num(head.dy)),
                ("squash", Json::Num(head.squash)),
            ]),
        ),
        (
            "emitting",
            object(vec![
                ("left", Json::Num(emitting.left)),
                ("right", Json::Num(emitting.right)),
            ]),
        ),
        ("state", face.state()),
        (
            "shapes",
            Json::List(
                face.shapes_now()
                    .iter()
                    .map(|shape| {
                        object(vec![
                            ("x", Json::Num(shape.x)),
                            ("y", Json::Num(shape.y)),
                            ("w", Json::Num(shape.w)),
                            ("h", Json::Num(shape.h)),
                            ("spun", Json::Num(shape.spun)),
                            ("stirred", Json::Num(shape.stirred)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]);
    let buffer = &face.buffer;
    let grains = object(vec![
        (
            "extent",
            object(vec![
                ("minX", Json::Num(extent.min_x)),
                ("maxX", Json::Num(extent.max_x)),
                ("minY", Json::Num(extent.min_y)),
                ("maxY", Json::Num(extent.max_y)),
            ]),
        ),
        (
            "first",
            numbers(buffer[..GRAIN_STRIDE].iter().map(|value| f64::from(*value))),
        ),
        (
            "grains",
            numbers((0..face.field.count()).step_by(7).flat_map(|index| {
                let at = index * GRAIN_STRIDE;
                [at, at + 1, at + 2, at + 7].map(|place| f64::from(buffer[place]))
            })),
        ),
    ]);
    (near, grains)
}

fn part(after: &Json, keys: &[&str]) -> Json {
    Json::Obj(
        keys.iter()
            .map(|key| {
                let value = after.get(key).unwrap_or_else(|| panic!("no {key}"));
                ((*key).to_owned(), value.clone())
            })
            .collect(),
    )
}

#[test]
fn a_scenario_goes_as_it_goes_on_the_web() {
    let traces = fixture("traces");
    let mut steps = 0;
    for trace in list(&traces, "traces") {
        let name = trace.str_at("name").unwrap();
        let mut now = num(trace, "start");
        let mut face = Face::new(FaceOptions {
            seed: num(trace, "seed") as u32,
            now,
            ..FaceOptions::default()
        });
        face.attach(Some(Box::new(PlainStage)));
        for (index, step) in list(trace, "steps").iter().enumerate() {
            let asked = step.get("act").unwrap();
            let at = format!("{name} step {index} ({asked})");
            let result = act(&mut face, &mut now, asked);
            match (result, step.get("result")) {
                (Some(ours), Some(theirs)) => {
                    assert_eq!(differs(&ours, theirs, 0.0, "result"), None, "{at}")
                }
                (None, None) => {}
                (ours, theirs) => panic!("{at}: result {ours:?}, not {theirs:?}"),
            }
            let after = step.get("after").unwrap();
            let (near, grains) = snapshot(&face);
            let theirs = part(after, &["channels", "head", "emitting", "state", "shapes"]);
            assert_eq!(differs(&near, &theirs, NEAR, "after"), None, "{at}");
            let theirs = part(after, &["extent", "first", "grains"]);
            assert_eq!(differs(&grains, &theirs, GRAIN, "after"), None, "{at}");
            steps += 1;
        }
    }
    assert_eq!(steps, 145);
}
