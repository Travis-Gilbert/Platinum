//! What the face keeps: added performances and rules, their versions, and
//! that they come back from a shelf. The web's fixtures do not cover these;
//! what is checked here is this crate's own account of the web's behaviour,
//! read from its source, not a comparison with it.
use std::collections::HashMap;

use theo::book::By;
use theo::engine::{Face, FaceOptions, How};
use theo::json::Json;
use theo::memory::{FaceEvent, KINDS, Memory, Shelf};

#[derive(Clone, Default)]
struct Map(HashMap<String, String>);

impl Shelf for Map {
    fn get_item(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_item(&mut self, key: &str, value: &str) {
        self.0.insert(key.to_owned(), value.to_owned());
    }
}

/// A shelf two faces can share, as a file is shared between two visits.
#[derive(Clone, Default)]
struct Shared(std::rc::Rc<std::cell::RefCell<Map>>);

impl Shelf for Shared {
    fn get_item(&self, key: &str) -> Option<String> {
        self.0.borrow().get_item(key)
    }
    fn set_item(&mut self, key: &str, value: &str) {
        self.0.borrow_mut().set_item(key, value);
    }
}

fn face(shelf: &Shared) -> Face {
    Face::new(FaceOptions {
        shelf: Some(Box::new(shelf.clone())),
        now: 1_700_000_000_000.0,
        ..FaceOptions::default()
    })
}

fn json(text: &str) -> Json {
    Json::parse(text).expect("sound JSON")
}

#[test]
fn a_definition_is_versioned_pinned_and_kept() {
    let shelf = Shared::default();
    let mut theo = face(&shelf);
    let dip = json(r#"[{"op":"to","set":{"face.tilt":6},"over":120},{"op":"use","name":"blink"}]"#);
    let first = theo.define("dip", &dip, "A dip.", By::Agent).unwrap();
    assert_eq!(first.reference(), "dip@1");
    // What it uses is written down by version.
    assert!(
        first.program.to_string().contains("\"blink@1\""),
        "{}",
        first.program
    );
    let second = theo.define("dip", &dip, "A dip.", By::Agent).unwrap();
    assert_eq!(second.reference(), "dip@2");

    let twice = json(r#"[{"op":"use","name":"dip"}]"#);
    assert_eq!(
        theo.define("twice", &twice, "", By::Person)
            .unwrap()
            .program
            .to_string()
            .contains("dip@2"),
        true
    );
    // A version something else uses stays; one nothing uses goes.
    assert!(theo.forget("dip@2").unwrap_err().contains("twice@1"));
    assert!(theo.forget("dip@1").is_ok());
    // The face's own performances stay.
    assert!(theo.forget("blink").is_err());

    let refused = theo.define("Dip", &dip, "", By::Agent).unwrap_err();
    assert_eq!(refused.path, "name");
    let refused = theo
        .define(
            "bad",
            &json(r#"[{"op":"to","set":{"no.such":1}}]"#),
            "",
            By::Agent,
        )
        .unwrap_err();
    assert!(!refused.reason.is_empty());

    // Another visit finds them.
    let mut later = face(&shelf);
    assert_eq!(
        later
            .book
            .get("dip")
            .map(|entry| entry.reference())
            .as_deref(),
        Some("dip@2")
    );
    assert!(later.book.get("dip@1").is_none());
    let performed = later.perform_named("twice", How::by("agent")).unwrap();
    assert_eq!(performed.invocation.name.as_deref(), Some("twice@1"));
}

#[test]
fn a_rule_is_added_kept_and_removed() {
    let shelf = Shared::default();
    let mut theo = face(&shelf);
    let rule = json(r#"{"id":"hum-nod","on":"env.hum","do":"blink","weight":5}"#);
    let added = theo.define_rule(&rule, By::Agent).unwrap();
    assert_eq!(added.id, "hum-nod");
    assert_eq!(
        theo.notice("env.hum", 1.0)
            .and_then(|played| played.name)
            .as_deref(),
        Some("blink@1")
    );
    // A rule that names what the book has not is refused.
    let refused = theo
        .define_rule(
            &json(r#"{"id":"hum-two","on":"env.hum","do":"no-such"}"#),
            By::Agent,
        )
        .unwrap_err();
    assert!(!refused.reason.is_empty());

    let mut later = face(&shelf);
    assert!(later.reactions.rules().any(|rule| rule.id == "hum-nod"));
    assert!(later.forget_rule("hum-nod"));
    assert!(!later.forget_rule("hum-nod"));
    assert!(
        !face(&shelf)
            .reactions
            .rules()
            .any(|rule| rule.id == "hum-nod")
    );
}

#[test]
fn the_kinds_counted_are_bounded() {
    let mut memory = Memory::new(None);
    let mut at = 1_700_000_000_000.0;
    let mut note = |memory: &mut Memory, kind: String| {
        at += 10.0;
        memory.note(
            &FaceEvent {
                kind,
                at,
                weight: 1.0,
            },
            None,
        )
    };
    note(&mut memory, "env.first".to_owned());
    for index in 0..KINDS {
        note(&mut memory, format!("env.kind{index}"));
    }
    // The one longest unseen made room, so it reads as new again; one seen since does not.
    assert!(note(&mut memory, "env.first".to_owned()).gap.is_infinite());
    assert!(
        note(&mut memory, format!("env.kind{}", KINDS - 1))
            .gap
            .is_finite()
    );
}
