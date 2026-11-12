//! What Theo is, as data: its identity, its channels, its signals, its book
//! and its rules. The file is written from web Theo's own modules by
//! `fixtures/generate.ts`, so nothing here is restated by hand.
use std::sync::OnceLock;

use crate::json::Json;

const DATA: &str = include_str!("../data/builtin.json");

/// The params every feature has, in the order the web declares them. The
/// data must agree; `Shape` is filled by place in this list.
pub const PARAMS: [&str; 31] = [
    "x", "y", "w", "h", "round", "r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "lid", "lidTilt",
    "lidBow", "low", "lowTilt", "lowBow", "hollow", "rings", "slats", "mesh", "globe", "spin",
    "stir", "loose", "turn", "scale", "alpha", "share",
];
pub const PARAM_COUNT: usize = PARAMS.len();

pub fn param_name(index: usize) -> &'static str {
    PARAMS[index]
}

pub fn param_index(name: &str) -> Option<usize> {
    (0..PARAM_COUNT).find(|&index| param_name(index) == name)
}

/// One number the face can be asked to hold: its bounds, where it rests, what it is.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelSpec {
    pub min: f64,
    pub max: f64,
    pub rest: f64,
    pub note: &'static str,
}

/// A number the host measures as it happens, that a program can tie a channel to.
#[derive(Clone, Debug, PartialEq)]
pub struct SignalSpec {
    pub name: &'static str,
    pub min: f64,
    pub max: f64,
    pub note: &'static str,
    /// It holds for a whole run or longer, so it is followed with motion reduced too.
    pub steady: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Centre,
}

impl Side {
    pub fn parse(text: &str) -> Option<Side> {
        match text {
            "left" => Some(Side::Left),
            "right" => Some(Side::Right),
            "centre" => Some(Side::Centre),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
            Side::Centre => "centre",
        }
    }

    /// -1 left of the centre line, 1 right, 0 on it.
    pub fn sign(self) -> f64 {
        match self {
            Side::Left => -1.0,
            Side::Right => 1.0,
            Side::Centre => 0.0,
        }
    }
}

/// A feature as the face holds it: where it is and what it rests as.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureDef {
    pub id: String,
    pub side: Side,
    /// Rest values that differ from the params' own, by param place.
    pub rest: Vec<(usize, f64)>,
}

/// What Theo is before anything asks it to be otherwise.
#[derive(Clone, Debug)]
pub struct Identity {
    pub width: f64,
    pub height: f64,
    pub grains: usize,
    pub grain: f64,
    pub hue: f64,
    pub chroma: f64,
    pub lightness: f64,
    pub features: Vec<FeatureDef>,
}

pub struct Tables {
    /// The web commit the data was written from.
    pub commit: &'static str,
    pub identity: Identity,
    pub feature_params: Vec<ChannelSpec>,
    pub face_channels: Vec<(&'static str, ChannelSpec)>,
    pub signals: Vec<SignalSpec>,
    /// The built-in performances, as the web writes them.
    pub book: Vec<Json>,
    /// The built-in rules, as the web writes them.
    pub rules: Vec<Json>,
}

fn leak(text: &str) -> &'static str {
    Box::leak(text.to_owned().into_boxed_str())
}

fn spec(value: &Json) -> ChannelSpec {
    ChannelSpec {
        min: value.num_at("min").expect("a channel has a least"),
        max: value.num_at("max").expect("a channel has a most"),
        rest: value.num_at("rest").expect("a channel has a rest"),
        note: leak(value.str_at("note").unwrap_or("")),
    }
}

fn load() -> Tables {
    let data = Json::parse(DATA).expect("data/builtin.json is JSON");
    let at = |key: &str| data.get(key).unwrap_or_else(|| panic!("data has {key}"));
    let identity = at("identity");
    let number = |key: &str| identity.num_at(key).expect("identity number");
    let features = identity
        .get("features")
        .and_then(Json::list)
        .expect("identity features")
        .iter()
        .map(|feature| FeatureDef {
            id: feature.str_at("id").expect("feature id").to_owned(),
            side: feature
                .str_at("side")
                .and_then(Side::parse)
                .expect("feature side"),
            rest: Vec::new(),
        })
        .collect();
    let params = at("featureParams").obj().expect("feature params");
    assert_eq!(params.len(), PARAM_COUNT, "the params the port knows");
    let feature_params = params
        .iter()
        .enumerate()
        .map(|(index, (name, value))| {
            assert_eq!(name, param_name(index), "params in the web's order");
            spec(value)
        })
        .collect();
    Tables {
        commit: leak(
            at("source")
                .str_at("commit")
                .expect("the commit the data is from"),
        ),
        identity: Identity {
            width: number("width"),
            height: number("height"),
            grains: number("grains") as usize,
            grain: number("grain"),
            hue: number("hue"),
            chroma: number("chroma"),
            lightness: number("lightness"),
            features,
        },
        feature_params,
        face_channels: at("faceChannels")
            .obj()
            .expect("face channels")
            .iter()
            .map(|(name, value)| (leak(name), spec(value)))
            .collect(),
        signals: at("signals")
            .obj()
            .expect("signals")
            .iter()
            .map(|(name, value)| SignalSpec {
                name: leak(name),
                min: value.num_at("min").expect("signal least"),
                max: value.num_at("max").expect("signal most"),
                note: leak(value.str_at("note").unwrap_or("")),
                steady: value.get("steady").and_then(Json::bool).unwrap_or(false),
            })
            .collect(),
        book: at("book").list().expect("book").to_vec(),
        rules: at("rules").list().expect("rules").to_vec(),
    }
}

/// The tables, read once.
pub fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(load)
}
