//! Runs authored behaviors: one sandboxed Luau VM each (rune-luau's
//! `Script`, with its time and memory limits), on the application's poll
//! clock, so an ongoing motion is local code and never a model call a frame.
//!
//! A script reaches Theo through `require("@theo/host")`. Every call there
//! is queued as a [`Command`] and the session applies it after the poll, so
//! no script ever holds the face, the window or a surface; the session stays
//! their one writer.
//!
//! ```luau
//! local theo = require("@theo/host")
//! theo.frame:Connect(function(now, dt) ... end)        -- every frame, on the poll clock
//! theo.changed:Connect(function(name, value) ... end)  -- a signal moved
//! theo.signal("composer.typing")                       -- a value, or nil if stale
//! theo.perform({ steps = { ... } }, { intensity = 0.5 }) -- a face program (the engine's JSON)
//! theo.perform("nod")                                  -- a book performance by name
//! theo.define("my-nod", { steps = { ... } }, "note")    -- a new book entry or version
//! theo.cancel(handle) theo.notice("person.wave", 1) theo.tint(200)
//! theo.draw({ { x = 0, y = -60, rotate = 0, scale = 1, paths = { { points = {{0,0},{8,4}}, fill = "#ffd166" } } } })
//! theo.open("note", "n1")                              -- an application intent
//! theo.random() theo.now()
//! ```
//!
//! `theo.frame` is the clock to move by when a run must replay: it carries
//! only the poll clock. rune-luau's `task.wait` adds the wall time since the
//! poll to its deadlines, so how often it wakes depends on the host's speed.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use rune::Tx;
use rune_luau::mlua::{self, Lua, Value};
use rune_luau::{Data, LogLevel, Script, ScriptOptions, Sources};
use theo::json::Json;
use theo::random::Random;

use super::Name;
use crate::drawing::Drawing;

/// How long a script may run before it is stopped at a safepoint.
const TIME_LIMIT: Duration = Duration::from_millis(50);
const MEMORY_LIMIT: usize = 32 * 1024 * 1024;

/// What a script asked of Theo, in the order it asked.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Perform {
        behavior: Name,
        handle: u32,
        program: Json,
        intensity: Option<f64>,
    },
    Define {
        behavior: Name,
        name: String,
        program: Json,
        note: String,
    },
    Cancel {
        behavior: Name,
        handle: u32,
    },
    Notice {
        kind: String,
        weight: f64,
    },
    Tint(f64),
    Draw {
        behavior: Name,
        drawing: Drawing,
    },
    Open {
        kind: String,
        id: String,
    },
    /// A call the host could not read; the reason, for the author.
    Refused {
        behavior: Name,
        reason: String,
    },
}

/// What scripts can read this frame: the poll clock and the signals.
#[derive(Default)]
struct World {
    now: Cell<f64>,
    signals: RefCell<BTreeMap<String, f64>>,
}

/// One behavior's link to the host, shared by every VM its revisions start.
struct Link {
    name: Name,
    outbox: Rc<RefCell<Vec<Command>>>,
    world: Rc<World>,
    random: RefCell<Random>,
    handles: Cell<u32>,
}

impl Link {
    fn push(&self, command: Command) {
        self.outbox.borrow_mut().push(command);
    }

    fn refuse(&self, reason: impl Into<String>) {
        self.push(Command::Refused {
            behavior: self.name.clone(),
            reason: reason.into(),
        });
    }
}

fn program_of(value: &Data) -> Result<Json, String> {
    match value {
        Data::String(name) => Ok(Json::Str(name.clone())),
        Data::Map(_) | Data::List(_) => Json::parse(&value.to_json().to_string()),
        _ => Err("a program is a book name or a table".into()),
    }
}

fn module(lua: &Lua, link: Rc<Link>, changed: Value, frame: Value) -> mlua::Result<mlua::Table> {
    let theo = lua.create_table()?;
    theo.raw_set("changed", changed)?;
    theo.raw_set("frame", frame)?;

    let l = link.clone();
    theo.raw_set(
        "perform",
        lua.create_function(move |_, (program, options): (Data, Option<Data>)| {
            let program = match program_of(&program) {
                Ok(p) => p,
                Err(e) => {
                    l.refuse(format!("perform: {e}"));
                    return Ok(None);
                }
            };
            let handle = l.handles.get() + 1;
            l.handles.set(handle);
            let intensity = options.as_ref().and_then(|o| o.get("intensity").as_f64());
            l.push(Command::Perform {
                behavior: l.name.clone(),
                handle,
                program,
                intensity,
            });
            Ok(Some(handle))
        })?,
    )?;

    let l = link.clone();
    theo.raw_set(
        "define",
        lua.create_function(
            move |_, (name, program, note): (String, Data, Option<String>)| {
                match program_of(&program) {
                    Ok(program) => l.push(Command::Define {
                        behavior: l.name.clone(),
                        name,
                        program,
                        note: note.unwrap_or_default(),
                    }),
                    Err(e) => l.refuse(format!("define {name}: {e}")),
                }
                Ok(())
            },
        )?,
    )?;

    let l = link.clone();
    theo.raw_set(
        "cancel",
        lua.create_function(move |_, handle: u32| {
            l.push(Command::Cancel {
                behavior: l.name.clone(),
                handle,
            });
            Ok(())
        })?,
    )?;

    let l = link.clone();
    theo.raw_set(
        "notice",
        lua.create_function(move |_, (kind, weight): (String, Option<f64>)| {
            l.push(Command::Notice {
                kind,
                weight: weight.unwrap_or(1.0),
            });
            Ok(())
        })?,
    )?;

    let l = link.clone();
    theo.raw_set(
        "tint",
        lua.create_function(move |_, hue: f64| {
            l.push(Command::Tint(hue));
            Ok(())
        })?,
    )?;

    let l = link.clone();
    theo.raw_set(
        "draw",
        lua.create_function(move |_, groups: Data| {
            match Drawing::decode(&groups) {
                Ok(drawing) => l.push(Command::Draw {
                    behavior: l.name.clone(),
                    drawing,
                }),
                Err(e) => l.refuse(format!("draw: {e}")),
            }
            Ok(())
        })?,
    )?;

    let l = link.clone();
    theo.raw_set(
        "open",
        lua.create_function(move |_, (kind, id): (String, String)| {
            l.push(Command::Open { kind, id });
            Ok(())
        })?,
    )?;

    let l = link.clone();
    theo.raw_set(
        "signal",
        lua.create_function(move |_, name: String| {
            Ok(l.world.signals.borrow().get(&name).copied())
        })?,
    )?;

    let l = link.clone();
    theo.raw_set(
        "now",
        lua.create_function(move |_, ()| Ok(l.world.now.get()))?,
    )?;

    let l = link;
    theo.raw_set(
        "random",
        lua.create_function(move |_, (low, high): (Option<f64>, Option<f64>)| {
            let mut random = l.random.borrow_mut();
            Ok(match (low, high) {
                (Some(low), Some(high)) => random.between(low, high),
                _ => random.next(),
            })
        })?,
    )?;
    Ok(theo)
}

struct Running {
    script: Script,
    revision: u32,
}

/// Every running behavior, by name.
pub struct Host {
    seed: u32,
    outbox: Rc<RefCell<Vec<Command>>>,
    world: Rc<World>,
    running: BTreeMap<Name, Running>,
}

impl Host {
    /// A host whose behaviors draw their randomness from `seed`: the same
    /// seed and inputs give the same run, a new seed fresh variation.
    pub fn new(seed: u32) -> Self {
        Self {
            seed,
            outbox: Rc::default(),
            world: Rc::default(),
            running: BTreeMap::new(),
        }
    }

    pub fn seed(&self) -> u32 {
        self.seed
    }

    fn options(&self, name: &Name, revision: u32) -> ScriptOptions {
        let seed = theo::random::seed_of(&format!("{}:{}:{revision}", self.seed, name.as_str()));
        let link = Rc::new(Link {
            name: name.clone(),
            outbox: self.outbox.clone(),
            world: self.world.clone(),
            random: RefCell::new(Random::new(seed)),
            handles: Cell::new(0),
        });
        let label = name.as_str().to_owned();
        ScriptOptions::new()
            .time_limit(TIME_LIMIT)
            .memory_limit(MEMORY_LIMIT)
            .on_log(move |level, line| {
                if level == LogLevel::Error {
                    eprintln!("theo behavior {label}: {line}");
                }
            })
            .setup(move |host| {
                // Luau's own `math.random` follows the seed too.
                host.lua().load(format!("math.randomseed({seed})")).exec()?;
                let changed = host.signal("changed")?;
                let frame = host.signal("frame")?;
                host.register_module(
                    "@theo/host",
                    module(host.lua(), link.clone(), changed, frame)?,
                )
            })
    }

    /// Starts revision `revision` of `name`, or revises the running one in
    /// place (its hot state carried over). A revision that fails to compile
    /// or start changes nothing and returns why.
    pub fn start(&mut self, name: &Name, revision: u32, source: &str) -> Result<(), String> {
        if let Some(running) = self.running.get_mut(name) {
            running
                .script
                .reload(Sources::single(source.to_owned()))
                .map_err(|e| e.message)?;
            running.revision = revision;
            return Ok(());
        }
        let script = Script::with_options(
            name.as_str(),
            Sources::single(source.to_owned()),
            self.options(name, revision),
        );
        if let Some(error) = script.error() {
            return Err(error.message);
        }
        self.running
            .insert(name.clone(), Running { script, revision });
        Ok(())
    }

    /// Stops `name`; says whether it was running.
    pub fn stop(&mut self, name: &Name) -> bool {
        self.running.remove(name).is_some()
    }

    pub fn revision(&self, name: &Name) -> Option<u32> {
        self.running.get(name).map(|r| r.revision)
    }

    pub fn running(&self) -> impl Iterator<Item = &Name> {
        self.running.keys()
    }

    /// One frame: the signals as they now stand, `changed` fired for each
    /// that moved, `frame` fired, every behavior's due work run, and what
    /// they asked for.
    pub fn poll(
        &mut self,
        now_s: f64,
        signals: BTreeMap<String, f64>,
        tx: &mut Tx,
    ) -> Vec<Command> {
        let dt = (now_s - self.world.now.get()).max(0.0);
        self.world.now.set(now_s);
        let moved: Vec<(String, Option<f64>)> = {
            let before = self.world.signals.borrow();
            let mut moved: Vec<_> = signals
                .iter()
                .filter(|(k, v)| before.get(*k) != Some(*v))
                .map(|(k, v)| (k.clone(), Some(*v)))
                .collect();
            moved.extend(
                before
                    .keys()
                    .filter(|k| !signals.contains_key(*k))
                    .map(|k| (k.clone(), None)),
            );
            moved
        };
        *self.world.signals.borrow_mut() = signals;
        for running in self.running.values() {
            for (name, value) in &moved {
                let value = value.map_or(Data::Nil, Data::Number);
                running
                    .script
                    .fire("changed", [Data::String(name.clone()), value]);
            }
            running
                .script
                .fire("frame", [Data::Number(now_s), Data::Number(dt)]);
            running.script.poll(now_s, tx);
        }
        std::mem::take(&mut *self.outbox.borrow_mut())
    }

    /// The errors a behavior's tasks raised since it started, newest last.
    pub fn errors(&self, name: &Name) -> Vec<String> {
        self.running.get(name).map_or_else(Vec::new, |r| {
            r.script
                .log()
                .into_iter()
                .filter(|line| line.level == LogLevel::Error)
                .map(|line| line.message)
                .collect()
        })
    }
}
