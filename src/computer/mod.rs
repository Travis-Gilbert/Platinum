//! Theo's observation and action broker for this Mac: semantic snapshots and
//! captures of a window, targets bound to the observation they came from,
//! serialized native input under a control generation, verification by
//! reading the application back, receipts, and the person's takeover.
//!
//! The executor is Cua Driver, in process (`cua.rs`); the comparison that
//! chose it over agent-desktop is recorded in
//! `docs/plans/theo-desktop-handoff.md`. The broker owns the loop around it:
//!
//! - **Observe**: `get_window_state` of one window (pid and window number),
//!   with a capture of that window alone, so Theo's own windows are never in
//!   it. Each observation gets the broker's own increasing id and becomes the
//!   window's latest.
//! - **Act**: a request names an element of an observation and carries its
//!   control generation. It is refused, with nothing sent, when the
//!   generation was revoked, when the observation is no longer the window's
//!   latest, or when the person has used the keyboard or a mouse button since
//!   the broker took control. The same checks run while the call is in
//!   flight, which is then cancelled.
//! - **Verify**: typing and setting a value are read back from a fresh
//!   observation; a press is verified by an element it should reveal, when
//!   the step names one.
//! - **Receipts**: every action ends verified, delivered, refused or
//!   uncertain, and is appended to `receipts.jsonl` in the broker's folder,
//!   with the capture it acted on and the target outlined on it.
//!
//! Dispatch is one action at a time for the whole desktop: competing callers
//! queue on the dispatch lock and each re-checks its generation there.

pub mod contract;
mod cua;
mod input;
pub mod task;

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use resvg::tiny_skia;
use serde_json::{Value, json};

use contract::{
    Act, ActRequest, Capture, Element, Observation, Outcome, Query, Receipt, Refusal, Reply,
    Resolved, Target, outcome, resolve,
};
use cua::{CallError, Cua};

/// Most elements walked for one observation.
const MOST_ELEMENTS: u32 = 400;
/// The tree walk's budget for one observation, in milliseconds.
const WALK_MS: u32 = 5000;

pub struct Broker {
    folder: PathBuf,
    executor: OnceLock<Result<Cua, String>>,
    generation: AtomicU64,
    /// Hardware input counts when the current generation took control.
    since: Mutex<input::Mark>,
    /// The last generation Theo's Stop ended, which nothing resumes.
    stopped: AtomicU64,
    observations: AtomicU64,
    actions: AtomicU64,
    latest: Mutex<HashMap<(i32, u32), Observation>>,
    dispatch: Mutex<()>,
}

static BROKER: OnceLock<Arc<Broker>> = OnceLock::new();

/// Makes the process's broker, keeping its receipts and captures in
/// `folder`. The executor loads on first use.
pub fn init(folder: PathBuf) {
    let _ = BROKER.set(Arc::new(Broker::new(folder)));
}

/// The process's broker, once `init` has run.
pub fn broker() -> Option<Arc<Broker>> {
    BROKER.get().cloned()
}

/// Stops dispatch for whoever holds control now: Theo's Stop.
pub fn stop() {
    if let Some(broker) = broker() {
        let generation = broker.generation();
        broker.stopped.fetch_max(generation, Ordering::SeqCst);
        broker.revoke(generation);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

impl Broker {
    fn new(folder: PathBuf) -> Broker {
        Broker {
            folder,
            executor: OnceLock::new(),
            generation: AtomicU64::new(1),
            since: Mutex::new(input::Mark::now()),
            stopped: AtomicU64::new(0),
            observations: AtomicU64::new(0),
            actions: AtomicU64::new(0),
            latest: Mutex::new(HashMap::new()),
            dispatch: Mutex::new(()),
        }
    }

    fn executor(&self) -> Result<&Cua, Refusal> {
        self.executor
            .get_or_init(|| {
                let path = cua::library_path();
                Cua::load(&path).map_err(|error| format!("{error:#}"))
            })
            .as_ref()
            .map_err(|why| Refusal::Unavailable(why.clone()))
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Takes the desktop's input for a new generation, from this moment: any
    /// key or mouse button the person presses after it revokes it.
    pub fn take_control(&self) -> u64 {
        let _dispatching = self.dispatch.lock().unwrap();
        *self.since.lock().unwrap() = input::Mark::now();
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// No further dispatch for `generation`. An action already sent may
    /// still complete; its receipt says so.
    pub fn revoke(&self, generation: u64) {
        let _ = self.generation.compare_exchange(
            generation,
            generation + 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }

    /// Whether Theo's Stop ended `generation`, rather than the person's input.
    pub fn stopped(&self, generation: u64) -> bool {
        self.stopped.load(Ordering::SeqCst) >= generation
    }

    /// Whether `generation` may still dispatch. A key or button press since
    /// control was taken revokes it here, at the moment of the write.
    fn may_dispatch(&self, generation: u64) -> bool {
        if self.generation() != generation {
            return false;
        }
        if self.since.lock().unwrap().person_acted() {
            self.revoke(generation);
            log::info!("computer: the person took over; generation {generation} revoked");
            return false;
        }
        true
    }

    pub fn windows(&self, app: &str) -> Result<Vec<cua::Window>, Refusal> {
        let result = self
            .executor()?
            .call("list_windows", &json!({ "app": app }), &|| false)
            .map_err(|error| Refusal::Executor(format!("{error:?}")))?;
        Ok(cua::windows(&result))
    }

    /// A fresh observation of `target`'s window, with a capture of that
    /// window when `capture` is set and Screen Recording allows it.
    pub fn observe(&self, target: &Target, capture: bool) -> Result<Observation, Refusal> {
        let id = self.observations.fetch_add(1, Ordering::SeqCst) + 1;
        let picture = self.folder.join(format!("o{id}.png"));
        let mut arguments = json!({
            "pid": target.pid,
            "window_id": target.window,
            "tree_format": "elements",
            "include_screenshot": capture,
            "max_elements": MOST_ELEMENTS,
            "timeout_ms": WALK_MS,
            "max_image_dimension": 0,
        });
        if capture {
            let _ = std::fs::create_dir_all(&self.folder);
            // Ids restart with the process, so a picture by this name may be
            // an earlier run's.
            let _ = std::fs::remove_file(&picture);
            arguments["screenshot_out_file"] = Value::from(picture.to_string_lossy().into_owned());
        }
        let read = |arguments: &Value| {
            let result = self
                .executor()?
                .call("get_window_state", arguments, &|| false)
                .map_err(|error| Refusal::Executor(format!("{error:?}")))?;
            cua::observation(&result, id, target.clone(), now_ms()).map_err(Refusal::Executor)
        };
        let mut seen = match read(&arguments) {
            // A picture is evidence, not what the broker acts on: when the
            // capture is refused, the element tree is still the observation.
            // Cua refuses with "capture binding is invalid" when it cannot
            // bind the picture for pixel clicks (seen on a display above the
            // main one), yet the picture it wrote is the window, and is kept.
            Err(Refusal::Executor(why)) if capture => {
                log::warn!("computer: reading the elements alone: {why}");
                arguments["include_screenshot"] = Value::from(false);
                arguments
                    .as_object_mut()
                    .map(|arguments| arguments.remove("screenshot_out_file"));
                let mut seen = read(&arguments)?;
                seen.capture_missing = Some(why.chars().take(300).collect());
                seen
            }
            seen => seen?,
        };
        if capture {
            seen.capture = read_capture(&picture, seen.window_bounds);
            if seen.capture.is_some() {
                seen.capture_missing = None;
            } else if seen.capture_missing.is_none() {
                seen.capture_missing = Some("no picture was written".into());
            }
        }
        self.latest
            .lock()
            .unwrap()
            .insert((target.pid, target.window), seen.clone());
        Ok(seen)
    }

    /// The window's latest observation.
    pub fn latest(&self, target: &Target) -> Option<Observation> {
        self.latest
            .lock()
            .unwrap()
            .get(&(target.pid, target.window))
            .cloned()
    }

    pub fn next_action(&self) -> u64 {
        self.actions.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// One action, never retried here. `expect` names an element a press
    /// should leave in the window, to verify it by.
    pub fn act(&self, target: &Target, request: &ActRequest, expect: Option<&Query>) -> Receipt {
        let started = Instant::now();
        let _dispatching = self.dispatch.lock().unwrap();
        let seen = self.latest(target);
        let element = seen
            .as_ref()
            .filter(|seen| seen.id == request.observation)
            .and_then(|seen| {
                seen.elements
                    .iter()
                    .find(|element| element.index == request.element)
            })
            .cloned();
        let receipt =
            |element: Option<Element>, outcome: Outcome, verified_by: Option<u64>| Receipt {
                action: request.action,
                generation: request.generation,
                observation: request.observation,
                target: target.clone(),
                element,
                act: request.act.clone(),
                outcome,
                verified_by,
                at_ms: now_ms(),
                elapsed_ms: started.elapsed().as_millis() as u64,
            };
        let refused = |why: Refusal| receipt(None, Outcome::Refused(why), None);
        let finished = if !self.may_dispatch(request.generation) {
            refused(Refusal::Revoked)
        } else if seen
            .as_ref()
            .is_none_or(|seen| seen.id != request.observation)
        {
            refused(Refusal::Stale)
        } else if let Some(element) = element {
            let reply = match self.executor() {
                Ok(executor) => self.send(executor, &element, request),
                Err(why) => Reply::Declined {
                    stale: false,
                    why: format!("{why:?}"),
                },
            };
            let (read_back, verified_by) = match &reply {
                Reply::Declined { .. } => (None, None),
                _ => self.read_back(target, &element, &request.act, expect),
            };
            receipt(Some(element), outcome(&reply, read_back), verified_by)
        } else {
            refused(Refusal::NoElement)
        };
        self.record(&finished, seen.as_ref());
        finished
    }

    fn send(&self, executor: &Cua, element: &Element, request: &ActRequest) -> Reply {
        let (tool, arguments) = match &request.act {
            Act::Press => ("click", json!({ "element_token": element.token })),
            Act::Type { text } => (
                "type_text",
                json!({ "element_token": element.token, "text": text, "delivery_mode": "background" }),
            ),
            Act::SetValue { value } => (
                "set_value",
                json!({ "element_token": element.token, "value": value }),
            ),
        };
        let generation = request.generation;
        match executor.call(tool, &arguments, &|| !self.may_dispatch(generation)) {
            Ok(result) => cua::reply(&result),
            Err(CallError::Rejected(why)) => Reply::Declined { stale: false, why },
            Err(CallError::Cancelled) => {
                Reply::Failed("cancelled while it was being delivered".into())
            }
            Err(CallError::Failed(why)) => Reply::Failed(why),
        }
    }

    /// Reads the application back after an action: whether it shows the
    /// effect, and the observation that showed it.
    fn read_back(
        &self,
        target: &Target,
        element: &Element,
        act: &Act,
        expect: Option<&Query>,
    ) -> (Option<bool>, Option<u64>) {
        let Ok(after) = self.observe(target, false) else {
            return (None, None);
        };
        let same = Query {
            role: Some(element.role.clone()),
            label: Some(element.label.clone()),
        };
        let now = match resolve(&after, &same) {
            Resolved::One(found) => Some(found),
            _ => after
                .elements
                .iter()
                .find(|found| found.index == element.index && found.role == element.role),
        };
        let shown = match act {
            Act::Type { text } => now.map(|found| {
                found
                    .value
                    .as_deref()
                    .unwrap_or_default()
                    .contains(text.trim())
            }),
            Act::SetValue { value } => {
                now.map(|found| found.value.as_deref().unwrap_or_default().trim() == value.trim())
            }
            Act::Press => expect.map(|expect| matches!(resolve(&after, expect), Resolved::One(_))),
        };
        (shown, Some(after.id))
    }

    /// Appends the receipt, and keeps the capture the action was aimed at
    /// with its target outlined: the same element the executor was given.
    fn record(&self, receipt: &Receipt, seen: Option<&Observation>) {
        log::info!(
            "computer: action {} on {:?} {:?}: {:?}",
            receipt.action,
            receipt
                .element
                .as_ref()
                .map(|element| (&element.role, &element.label)),
            receipt.act,
            receipt.outcome
        );
        if std::fs::create_dir_all(&self.folder).is_err() {
            return;
        }
        if let (Some(seen), Some(element)) = (seen, receipt.element.as_ref()) {
            outline(
                seen,
                element,
                &self.folder.join(format!("a{}.png", receipt.action)),
            );
        }
        let line = match serde_json::to_string(receipt) {
            Ok(line) => line,
            Err(error) => return log::error!("computer: receipt: {error}"),
        };
        let written = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.folder.join("receipts.jsonl"))
            .and_then(|mut file| writeln!(file, "{line}"));
        if let Err(error) = written {
            log::error!("computer: receipts: {error}");
        }
    }
}

fn read_capture(path: &Path, window: Option<contract::Rect>) -> Option<Capture> {
    let png = std::fs::read(path).ok()?;
    let pixmap = tiny_skia::Pixmap::decode_png(&png).ok()?;
    let scale = window.map_or(1.0, |window| f64::from(pixmap.width()) / window.w.max(1.0));
    Some(Capture {
        width: pixmap.width(),
        height: pixmap.height(),
        scale,
        png,
    })
}

/// A passive annotation: the target's bounds outlined on the capture it was
/// chosen from, through the one points-to-pixels transform.
fn outline(seen: &Observation, element: &Element, out: &Path) {
    let (Some(capture), Some(window), Some(bounds)) =
        (&seen.capture, seen.window_bounds, element.bounds)
    else {
        return;
    };
    let Ok(mut pixmap) = tiny_skia::Pixmap::decode_png(&capture.png) else {
        return;
    };
    let at = bounds.in_capture(window, capture.scale);
    let Some(rect) = tiny_skia::Rect::from_xywh(at.x as f32, at.y as f32, at.w as f32, at.h as f32)
    else {
        return;
    };
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 59, 48, 255);
    paint.anti_alias = true;
    let stroke = tiny_skia::Stroke {
        width: (2.0 * capture.scale) as f32,
        ..Default::default()
    };
    pixmap.stroke_path(
        &tiny_skia::PathBuilder::from_rect(rect),
        &paint,
        &stroke,
        tiny_skia::Transform::identity(),
        None,
    );
    if let Ok(png) = pixmap.encode_png() {
        let _ = std::fs::write(out, png);
    }
}
