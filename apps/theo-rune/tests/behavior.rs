//! Stage 2: behavior an author writes runs without rebuilding the app,
//! draws geometry the face's rig cannot, answers a capability's signal,
//! survives a broken revision, and can be run again from a capture.
//!
//! Everything here is the real Luau host and the real face engine.

use std::collections::BTreeMap;

use companion::Companion;
use rune::Tx;
use theo::engine::{FaceOptions, How};
use theo_desktop::behavior::run::{Behaviors, replay};
use theo_desktop::behavior::{Catalog, Name, Status};
use theo_desktop::signals::{Owner, Registry};
use theo_desktop::surfaces::Surfaces;

const FRAME_MS: f64 = 1000.0 / 60.0;

struct Run {
    companion: Companion,
    surfaces: Surfaces,
    behaviors: Behaviors,
    tx: Tx,
    now: f64,
}

impl Run {
    fn new(seed: u32) -> Self {
        let mut companion = Companion::new(FaceOptions {
            seed: 7,
            ..FaceOptions::default()
        });
        companion.show(companion::Scope::default());
        Self {
            companion,
            surfaces: Surfaces::default(),
            behaviors: Behaviors::new(seed, Catalog::default()),
            tx: Tx::default(),
            now: 0.0,
        }
    }

    fn frames(&mut self, count: usize, signals: &BTreeMap<String, f64>) {
        for _ in 0..count {
            self.now += FRAME_MS;
            self.behaviors.frame(
                self.now,
                signals.clone(),
                &mut self.companion,
                &mut self.surfaces,
                &mut self.tx,
            );
            self.companion.frame(self.now);
        }
    }

    fn author(&mut self, name: &str, source: &str) -> Result<u32, String> {
        self.behaviors
            .author(&Name::parse(name).unwrap(), source, "agent")
    }
}

fn none() -> BTreeMap<String, f64> {
    BTreeMap::new()
}

const BECKON: &str = r##"
local theo = require("@theo/host")
-- A performance the built-in book does not have, written now.
theo.define("beckon-wave", {
    { op = "to", set = { ["face.tilt"] = 6 }, over = 120 },
    { op = "use", name = "blink" },
}, "A beckon, authored at run time.")
-- A form the rig's superellipse features cannot describe: a five-point star.
local function star(points, outer, inner)
    local pts = {}
    for i = 0, points * 2 - 1 do
        local r = if i % 2 == 0 then outer else inner
        local a = i * math.pi / points
        table.insert(pts, { r * math.sin(a), -r * math.cos(a) })
    end
    return pts
end
theo.draw({ { y = -70, rotate = 0.1, paths = { { points = star(5, 16, 7), fill = "#ffd166" } } } })
task.spawn(function()
    theo.perform("beckon-wave")
end)
return {}
"##;

#[test]
fn an_authored_behavior_defines_a_new_performance_and_draws_a_new_form() {
    let mut run = Run::new(1);
    assert_eq!(run.author("beckon", BECKON), Ok(1));
    run.frames(3, &none());

    assert!(run.behaviors.notes.is_empty(), "{:?}", run.behaviors.notes);
    let drawings: Vec<_> = run.behaviors.drawings().collect();
    assert_eq!(drawings.len(), 1);
    assert_eq!(drawings[0].0.as_str(), "beckon");
    assert_eq!(
        drawings[0].1.groups[0].paths[0].points.len(),
        10,
        "a star's ten corners"
    );
    // The definition reached the face's own book, by the agent.
    assert!(
        run.companion
            .face
            .perform_named("beckon-wave", How::by("test"))
            .is_ok()
    );
}

const COUNTER_V1: &str = r##"
local theo = require("@theo/host")
local count = 0
task.spawn(function()
    while true do
        count += 1
        task.wait(0.01)
    end
end)
theo.draw({ { paths = { { points = { {0, 0}, {10, 0}, {5, 8} }, fill = "#88ccff" } } } })
return {
    saveHotState = function() return { count = count } end,
}
"##;

const COUNTER_V3: &str = r##"
local theo = require("@theo/host")
return {
    loadHotState = function(state)
        -- What the earlier revision counted carries into this one.
        theo.tint(state.count)
    end,
}
"##;

#[test]
fn a_broken_revision_changes_nothing_and_a_good_one_keeps_the_state() {
    let mut run = Run::new(1);
    let counter = Name::parse("counter").unwrap();
    run.author("counter", COUNTER_V1).unwrap();
    run.frames(30, &none());

    let broken = run.author("counter", "local theo = require(\"@theo/host\"\nreturn {");
    assert!(broken.is_err(), "a syntax error is reported");
    run.frames(5, &none());
    assert_eq!(
        run.behaviors.catalog.active(&counter).unwrap().number,
        1,
        "revision 1 still runs"
    );
    assert!(matches!(
        run.behaviors.catalog.revisions(&counter)[1].status,
        Status::Failed(_)
    ));
    assert_eq!(
        run.behaviors.drawings().count(),
        1,
        "its drawing is untouched"
    );

    assert_eq!(run.author("counter", COUNTER_V3), Ok(3));
    run.frames(2, &none());
    let hue = run.companion.face.hue;
    assert!(
        hue >= 20.0,
        "the count carried over into revision 3: hue {hue}"
    );
}

const LEAN_IN: &str = r##"
local theo = require("@theo/host")
theo.changed:Connect(function(name, value)
    if name == "composer.typing" and value ~= nil and value > 3 then
        theo.tint(300)
    elseif name == "composer.typing" and value == nil then
        theo.tint(40)
    end
end)
return {}
"##;

#[test]
fn a_new_capability_signal_drives_authored_behavior_and_stale_reads_as_absent() {
    let mut registry = Registry::default();
    let composer = Owner("composer".into());
    registry
        .register("composer.typing", &composer, 500.0)
        .unwrap();

    let mut run = Run::new(1);
    run.author("lean-in", LEAN_IN).unwrap();
    run.frames(1, &registry.current(run.now));

    registry
        .set("composer.typing", &composer, 5.0, run.now)
        .unwrap();
    run.frames(1, &registry.current(run.now));
    assert_eq!(
        run.companion.face.hue, 300.0,
        "the behavior answered the measurement"
    );

    // Nobody measures for longer than the signal's lifetime.
    for _ in 0..40 {
        let current = registry.current(run.now);
        run.frames(1, &current);
    }
    assert_eq!(
        run.companion.face.hue, 40.0,
        "a stale value reaches the behavior as gone"
    );
}

const RESTLESS: &str = r##"
local theo = require("@theo/host")
local t = 0
theo.frame:Connect(function(now, dt)
    t += 1
    local jitter = theo.random(-4, 4) + math.random() * 2
    local typing = theo.signal("composer.typing") or 0
    theo.draw({ { x = jitter, rotate = t * 0.05, scale = 1 + typing / 10,
        paths = { { points = { {-6, -6}, {6, -6}, {0, 8} }, stroke = "#ffffff", width = 2 } } } })
    if t % 20 == 0 then
        theo.perform({ { op = "to", set = { ["face.tilt"] = theo.random(-5, 5) }, over = 100 } })
    end
end)
return {}
"##;

fn recorded(seed: u32) -> theo_desktop::behavior::run::Capture {
    let mut run = Run::new(seed);
    run.behaviors.start_capture(0.0, 7);
    run.author("restless", RESTLESS).unwrap();
    let mut signals = none();
    for i in 0..90 {
        if i == 30 {
            signals.insert("composer.typing".into(), 6.0);
        }
        if i == 60 {
            signals.clear();
        }
        run.frames(1, &signals);
    }
    run.behaviors.take_capture().unwrap()
}

#[test]
fn a_capture_replays_the_same_run_and_a_new_seed_varies_it() {
    let capture = recorded(11);
    assert_eq!(capture.frames.len(), 90);
    assert!(matches!(
        capture.frames[0].inputs[0],
        theo_desktop::behavior::run::Input::Author { .. }
    ));

    let first = replay(&capture);
    let again = replay(&capture);
    assert!(first.notes.is_empty(), "{:?}", first.notes);
    assert_eq!(first, again, "the same start and inputs give the same run");

    // The capture is durable: written and read back, it replays the same.
    let json = serde_json::to_string(&capture).unwrap();
    let read: theo_desktop::behavior::run::Capture = serde_json::from_str(&json).unwrap();
    assert_eq!(replay(&read), first);

    let varied = replay(&recorded(12));
    assert_ne!(
        varied.frames, first.frames,
        "a fresh seed is fresh variation"
    );
}

#[test]
fn a_restart_starts_the_behaviors_that_were_running() {
    let beckon = Name::parse("beckon").unwrap();
    let mut catalog = Catalog::default();
    let one = catalog.author(&beckon, BECKON, "agent");
    catalog.outcome(&beckon, one, Ok(()));
    let behaviors = Behaviors::new(1, catalog);
    assert_eq!(behaviors.running().collect::<Vec<_>>(), [&beckon]);
}
