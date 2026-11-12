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

const DAYS: &str = "theorem-ui:theo:days";

fn saved_days(shelf: &Map) -> (usize, f64, usize) {
    let saved = json(&shelf.get_item(DAYS).expect("days were saved"));
    let today = saved.get("today").and_then(Json::obj).unwrap();
    let usual = saved.get("usual").and_then(Json::obj).unwrap();
    let counted = today.iter().map(|(_, count)| count.num().unwrap()).sum();
    (today.len(), counted, usual.len())
}

#[test]
fn what_is_saved_is_bounded_however_many_kinds_there_are() {
    let mut shelf = Map::default();
    let mut memory = Memory::new(None);
    let start = 1_700_000_000_000.0;
    let many = KINDS * 4;
    for index in 0..many {
        let event = FaceEvent {
            kind: format!("env.kind{index}"),
            at: start + index as f64 * 100.0,
            weight: 1.0,
        };
        memory.note(&event, Some(&mut shelf));
    }
    // Every event is still counted, the ones past the bound together.
    let (today, counted, _) = saved_days(&shelf);
    assert_eq!(today, KINDS + 1);
    assert!(
        counted > (many - 400) as f64 && counted <= many as f64,
        "{counted}"
    );

    // The next day, what is usual is bounded too, and the overflow is habit to nothing.
    let tomorrow = FaceEvent {
        kind: "env.kind0".to_owned(),
        at: start + 86_400_000.0 * 1.5,
        weight: 1.0,
    };
    memory.note(&tomorrow, Some(&mut shelf));
    let (today, _, usual) = saved_days(&shelf);
    assert!(today <= 1 && usual <= KINDS, "{today} {usual}");
    assert_eq!(memory.habit(theo::memory::OVERFLOW), 0.0);
    assert!(memory.habit("env.kind0") > 0.0);
}

#[test]
fn a_shelf_that_holds_too_much_is_bounded_when_read() {
    let counts = |most: usize| {
        let fields: Vec<String> = (0..most)
            .map(|index| format!("\"env.kind{index}\":{}", 1 + index % 7))
            .collect();
        format!("{{{}}}", fields.join(","))
    };
    let day = (1_700_000_000_000.0_f64 / 86_400_000.0).floor();
    let mut shelf = Map::default();
    let many = KINDS * 3;
    shelf.set_item(
        DAYS,
        &format!(
            "{{\"day\":{day},\"today\":{},\"usual\":{}}}",
            counts(many),
            counts(many)
        ),
    );
    let before: f64 = (0..many).map(|index| (1 + index % 7) as f64).sum();
    let mut memory = Memory::new(Some(&shelf));
    // The most usual were kept.
    assert!(memory.habit("env.kind6") > 0.0);
    let event = FaceEvent {
        kind: "env.new".to_owned(),
        at: 1_700_000_000_000.0,
        weight: 1.0,
    };
    memory.note(&event, Some(&mut shelf));
    let (today, counted, usual) = saved_days(&shelf);
    assert_eq!((today, usual), (KINDS + 1, KINDS));
    assert_eq!(counted, before + 1.0);
}

#[test]
fn a_stale_stop_ends_nothing_newer() {
    let mut theo = Face::new(FaceOptions::default());
    let first = theo
        .perform_named("dizzy", How::by("agent"))
        .unwrap()
        .invocation;
    let second = theo
        .perform_named("scan", How::by("agent"))
        .unwrap()
        .invocation;
    assert_ne!(first.id, second.id);
    // The first was replaced on its layer; stopping it now is late and does nothing.
    assert!(!theo.cancel(&first.id));
    assert_eq!(theo.ending(&second.id), None);
    assert!(theo.cancel(&second.id));
    assert_eq!(theo.ending(&second.id), Some(theo::player::Ending::Stopped));
    assert!(!theo.cancel(&second.id));
    assert!(!theo.cancel("p999"));

    // A form is let go of the same way, and only by its own id.
    let form = theo.notice("run.work", 1.0).expect("the orb");
    assert!(!theo.cancel(&second.id));
    assert!(theo.state().get("form").is_some());
    assert!(theo.cancel(&form.id));
    assert!(theo.state().get("form").is_none());
}

#[test]
fn on_the_desktop_sound_is_a_loudspeaker_and_never_a_mouth() {
    use theo::engine::Profile;
    let heard = |profile: Profile, seed: u32| {
        let mut theo = Face::new(FaceOptions {
            profile,
            seed,
            ..FaceOptions::default()
        });
        theo.notice("sound.start", 1.0).and_then(|form| form.name)
    };
    let mut mouths = 0;
    for seed in 0..200 {
        assert_eq!(
            heard(Profile::Desktop, seed).as_deref(),
            Some("boombox@1"),
            "seed {seed}"
        );
        if heard(Profile::Web, seed).as_deref() == Some("speak@1") {
            mouths += 1;
        }
    }
    // The web's own choice is untouched: about one time in four it is a mouth.
    assert!((20..90).contains(&mouths), "{mouths}");
}
