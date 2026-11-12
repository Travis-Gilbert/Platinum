//! What the tests share: reading what web Theo wrote.
#![allow(dead_code)]
use theo::json::Json;

pub fn fixture(name: &str) -> Json {
    let path = format!("{}/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    Json::parse(&text).unwrap_or_else(|error| panic!("{path}: {error}"))
}

pub fn list<'a>(value: &'a Json, key: &str) -> &'a [Json] {
    value
        .get(key)
        .and_then(Json::list)
        .unwrap_or_else(|| panic!("no list at {key}"))
}

pub fn num(value: &Json, key: &str) -> f64 {
    value
        .num_at(key)
        .unwrap_or_else(|| panic!("no number at {key}"))
}

pub fn numbers(value: &Json) -> Vec<f64> {
    value
        .list()
        .expect("a list of numbers")
        .iter()
        .map(|item| item.num().expect("a number"))
        .collect()
}

/// Whether two numbers agree to `within`, taking no number to agree with no number.
pub fn close(a: f64, b: f64, within: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a == b || (a - b).abs() <= within
}

/// Where two values differ, if they do: objects by key whatever the order, numbers to `within`.
pub fn differs(ours: &Json, theirs: &Json, within: f64, path: &str) -> Option<String> {
    match (ours, theirs) {
        (Json::Num(a), Json::Num(b)) => {
            (!close(*a, *b, within)).then(|| format!("{path}: {a} is not {b}"))
        }
        (Json::List(a), Json::List(b)) => {
            if a.len() != b.len() {
                return Some(format!("{path}: {} items, not {}", a.len(), b.len()));
            }
            a.iter()
                .zip(b)
                .enumerate()
                .find_map(|(index, (a, b))| differs(a, b, within, &format!("{path}[{index}]")))
        }
        (Json::Obj(a), Json::Obj(b)) => {
            if let Some((key, _)) = a.iter().find(|(key, _)| theirs.get(key).is_none()) {
                return Some(format!("{path}: has {key}, which the web has not"));
            }
            b.iter().find_map(|(key, value)| match ours.get(key) {
                Some(mine) => differs(mine, value, within, &format!("{path}.{key}")),
                None => Some(format!("{path}: lacks {key}")),
            })
        }
        (a, b) => (a != b).then(|| format!("{path}: {a} is not {b}")),
    }
}
