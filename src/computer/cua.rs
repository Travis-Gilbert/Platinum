//! Cua Driver in this process, through its versioned C ABI
//! (`cua_driver_abi.h`, ABI 1.1, release cua-driver-rs-v0.47.23).
//!
//! SOURCING: the executor is Cua Driver itself; this file only binds its
//! exported C functions with `libloading`, the way Cua's own Rust SDK
//! (`cua-driver-sdk`, `src/abi.rs`) consumes the same ABI. The SDK crate is
//! not used directly because its workspace pins `zune-core = "=0.5.1"`, which
//! cannot resolve beside Platinum's locked `image` (zune-core 0.5.3), and it
//! is not published. `script/fetch-cua-driver` downloads the release's
//! `libcua_driver_sdk.dylib` and checks its digest.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context as _, bail};
use libloading::Library;
use serde_json::Value;

use super::contract::{Element, Observation, Rect, Reply, Target};

/// The release the broker was measured against.
pub const RELEASE: &str = "cua-driver-rs-0.47.23";

#[repr(C)]
struct Buffer {
    data: *mut u8,
    len: usize,
    capacity: usize,
}

impl Buffer {
    const EMPTY: Buffer = Buffer {
        data: std::ptr::null_mut(),
        len: 0,
        capacity: 0,
    };
}

type Status = i32;
const OK: Status = 0;
const CANCELLED: Status = 5;

type Completion = extern "C" fn(*mut c_void, Status, Buffer, Buffer);

struct Abi {
    compatible: unsafe extern "C" fn(u16, u16) -> bool,
    create: unsafe extern "C" fn(*const u8, usize, *mut *mut c_void, *mut Buffer) -> Status,
    destroy: unsafe extern "C" fn(*mut *mut c_void),
    #[allow(clippy::type_complexity)]
    invoke: unsafe extern "C" fn(
        *mut c_void,
        *const u8,
        usize,
        *const u8,
        usize,
        Completion,
        *mut c_void,
        *mut *mut c_void,
        *mut Buffer,
    ) -> Status,
    cancel: unsafe extern "C" fn(*mut c_void),
    release: unsafe extern "C" fn(*mut *mut c_void),
    free: unsafe extern "C" fn(*mut Buffer),
}

/// Where the driver library is: `FRAME_CUA_DRIVER`, else beside the app in
/// its bundle's Frameworks, else where `script/fetch-cua-driver` put it.
pub fn library_path() -> PathBuf {
    if let Some(path) = std::env::var_os("FRAME_CUA_DRIVER") {
        return path.into();
    }
    let bundled = std::env::current_exe().ok().and_then(|exe| {
        Some(
            exe.parent()?
                .parent()?
                .join("Frameworks/libcua_driver_sdk.dylib"),
        )
    });
    match bundled {
        Some(path) if path.exists() => path,
        _ => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("deps")
            .join(RELEASE)
            .join("libcua_driver_sdk.dylib"),
    }
}

/// One in-process Cua Driver runtime. Cua allows one per process.
pub struct Cua {
    abi: Abi,
    handle: *mut c_void,
    _library: Library,
}

// SAFETY: the handle is Cua's opaque runtime, which its ABI documents as
// usable from any thread; it is destroyed only in `Drop`.
unsafe impl Send for Cua {}
unsafe impl Sync for Cua {}

/// Why a call produced no tool result.
#[derive(Debug)]
pub enum CallError {
    /// Refused at the ABI before anything ran.
    Rejected(String),
    /// Cancelled while it ran: it may or may not have taken effect.
    Cancelled,
    /// Ran and failed at the ABI level.
    Failed(String),
}

impl Cua {
    pub fn load(path: &Path) -> anyhow::Result<Cua> {
        // SAFETY: loading runs the library's initializers; the file is the
        // pinned release `script/fetch-cua-driver` checked the digest of.
        let library = unsafe { Library::new(path) }
            .with_context(|| format!("loading Cua Driver from {}", path.display()))?;
        // SAFETY: the symbol types are the ones `cua_driver_abi.h` declares.
        let abi = unsafe {
            Abi {
                compatible: *library.get(b"cua_driver_abi_is_compatible_v1\0")?,
                create: *library.get(b"cua_driver_create_v1\0")?,
                destroy: *library.get(b"cua_driver_destroy_v1\0")?,
                invoke: *library.get(b"cua_driver_invoke_v1\0")?,
                cancel: *library.get(b"cua_driver_operation_cancel_v1\0")?,
                release: *library.get(b"cua_driver_operation_release_v1\0")?,
                free: *library.get(b"cua_driver_buffer_free_v1\0")?,
            }
        };
        // SAFETY: a plain query.
        if !unsafe { (abi.compatible)(1, 1) } {
            bail!(
                "the Cua Driver at {} does not speak ABI 1.1",
                path.display()
            );
        }
        let mut handle = std::ptr::null_mut();
        let mut error = Buffer::EMPTY;
        // SAFETY: empty options are allowed; both out pointers are valid.
        let status = unsafe { (abi.create)(std::ptr::null(), 0, &mut handle, &mut error) };
        let why = take(&abi, &mut error);
        if status != OK || handle.is_null() {
            bail!("Cua Driver did not start (status {status}): {why}");
        }
        Ok(Cua {
            abi,
            handle,
            _library: library,
        })
    }

    /// Runs one tool and waits for it. While it runs, `stop` is asked every
    /// few milliseconds; once it says so the call is cancelled.
    pub fn call(
        &self,
        tool: &str,
        arguments: &Value,
        stop: &dyn Fn() -> bool,
    ) -> Result<Value, CallError> {
        let arguments = arguments.to_string();
        let (sender, receiver) = mpsc::channel();
        let context = Box::into_raw(Box::new(Waiting {
            sender,
            free: self.abi.free,
        }))
        .cast::<c_void>();
        let mut operation = std::ptr::null_mut();
        let mut error = Buffer::EMPTY;
        // SAFETY: the buffers outlive the call, the callback matches the
        // ABI's completion type and owns `context` once the call succeeds.
        let status = unsafe {
            (self.abi.invoke)(
                self.handle,
                tool.as_ptr(),
                tool.len(),
                arguments.as_ptr(),
                arguments.len(),
                completed,
                context,
                &mut operation,
                &mut error,
            )
        };
        if status != OK {
            // SAFETY: the callback is never called when invocation fails.
            drop(unsafe { Box::from_raw(context.cast::<Waiting>()) });
            return Err(CallError::Rejected(take(&self.abi, &mut error)));
        }
        let mut cancelled = false;
        let done = loop {
            match receiver.recv_timeout(Duration::from_millis(15)) {
                Ok(done) => break done,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if !cancelled && stop() {
                        // SAFETY: the operation is live until released below.
                        unsafe { (self.abi.cancel)(operation) };
                        cancelled = true;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break (CANCELLED, String::new(), "completion was dropped".into());
                }
            }
        };
        // SAFETY: completion has run; the token is released exactly once.
        unsafe { (self.abi.release)(&mut operation) };
        match done {
            (OK, result, _) => {
                serde_json::from_str(&result).map_err(|error| CallError::Failed(error.to_string()))
            }
            (CANCELLED, _, _) => Err(CallError::Cancelled),
            (status, _, why) => Err(CallError::Failed(format!("status {status}: {why}"))),
        }
    }
}

impl Drop for Cua {
    fn drop(&mut self) {
        // SAFETY: the handle came from `create` and is not used after this.
        unsafe { (self.abi.destroy)(&mut self.handle) };
    }
}

struct Waiting {
    sender: mpsc::Sender<(Status, String, String)>,
    free: unsafe extern "C" fn(*mut Buffer),
}

extern "C" fn completed(
    context: *mut c_void,
    status: Status,
    mut result: Buffer,
    mut error: Buffer,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: Cua calls this exactly once with the context `call` boxed.
        let waiting = unsafe { Box::from_raw(context.cast::<Waiting>()) };
        let result = copy_and_free(waiting.free, &mut result);
        let error = copy_and_free(waiting.free, &mut error);
        let _ = waiting.sender.send((status, result, error));
    }));
}

fn take(abi: &Abi, buffer: &mut Buffer) -> String {
    copy_and_free(abi.free, buffer)
}

fn copy_and_free(free: unsafe extern "C" fn(*mut Buffer), buffer: &mut Buffer) -> String {
    let text = if buffer.data.is_null() {
        String::new()
    } else {
        // SAFETY: the ABI hands back `len` initialized bytes at `data`.
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(buffer.data, buffer.len) })
            .into_owned()
    };
    // SAFETY: caller-owned buffers are freed with the ABI's own function.
    unsafe { free(buffer) };
    text
}

/// The text parts of a tool result, joined.
pub fn text(result: &Value) -> String {
    result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|part| part["type"] == "text")
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A window from `list_windows`.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub target: Target,
    pub title: String,
    pub bounds: Option<Rect>,
    pub z_index: Option<i64>,
    pub on_screen: bool,
}

pub fn windows(result: &Value) -> Vec<Window> {
    result["structuredContent"]["windows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|window| {
            Some(Window {
                target: Target {
                    pid: window["pid"].as_i64()? as i32,
                    window: window["window_id"].as_u64()? as u32,
                    app: window["app_name"].as_str().map(str::to_owned),
                },
                title: window["title"].as_str().unwrap_or_default().to_owned(),
                bounds: rect(&window["bounds"]),
                z_index: window["z_index"].as_i64(),
                on_screen: window["is_on_screen"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

fn rect(value: &Value) -> Option<Rect> {
    // `list_windows` spells out width and height; element frames abbreviate.
    let number = |keys: &[&str]| keys.iter().find_map(|key| value[*key].as_f64());
    Some(Rect {
        x: number(&["x"])?,
        y: number(&["y"])?,
        w: number(&["width", "w"])?,
        h: number(&["height", "h"])?,
    })
}

/// The observation in a `get_window_state` result, or the executor's reason
/// it holds no elements (a locked screen, a missing permission).
pub fn observation(
    result: &Value,
    id: u64,
    target: Target,
    at_ms: u64,
) -> Result<Observation, String> {
    let state = &result["structuredContent"];
    let Some(snapshot) = state["snapshot_id"].as_str() else {
        let why = state["degraded_reason"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| text(result));
        return Err(why.chars().take(400).collect());
    };
    let elements: Vec<Element> = state["elements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|element| {
            Some(Element {
                index: element["element_index"].as_u64()? as usize,
                token: element["element_token"].as_str()?.to_owned(),
                role: element["role"].as_str().unwrap_or_default().to_owned(),
                subrole: element["subrole"].as_str().map(str::to_owned),
                label: element["label"]
                    .as_str()
                    .or_else(|| element["title"].as_str())
                    .unwrap_or_default()
                    .to_owned(),
                value: element["value"].as_str().map(str::to_owned),
                bounds: rect(&element["frame"]),
                actions: element["actions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|action| action.as_str().map(str::to_owned))
                    .collect(),
            })
        })
        .collect();
    let window_bounds = elements
        .iter()
        .find(|element| element.role == "AXWindow")
        .and_then(|window| window.bounds);
    Ok(Observation {
        id,
        target,
        snapshot: snapshot.to_owned(),
        window_bounds,
        complete: state["elements_complete"]
            .as_bool()
            .unwrap_or(!state["truncated"].as_bool().unwrap_or(false)),
        elements,
        captured_at_ms: at_ms,
        capture: None,
        capture_missing: state["screenshot_error"]["reason"]
            .as_str()
            .or_else(|| state["screenshot_error"]["code"].as_str())
            .map(|why| why.chars().take(300).collect()),
    })
}

/// What an action tool's result says, before the application is read back.
pub fn reply(result: &Value) -> Reply {
    let state = &result["structuredContent"];
    let said = || text(result).chars().take(300).collect::<String>();
    let effect = state["effect"].as_str();
    if effect == Some("refused") {
        let why = state["error"]["message"]
            .as_str()
            .or_else(|| state["error"]["code"].as_str());
        return Reply::Declined {
            stale: false,
            why: why.map(str::to_owned).unwrap_or_else(said),
        };
    }
    if result["isError"].as_bool().unwrap_or(false) {
        let said = said();
        // Cua checks a token before it sends anything; a stale or unknown
        // one, or a locked screen, means nothing reached the application.
        if said.contains("element_token is stale or unknown") {
            return Reply::Declined {
                stale: true,
                why: said,
            };
        }
        if said.contains("screen_locked") || said.contains("invalid arguments") {
            return Reply::Declined {
                stale: false,
                why: said,
            };
        }
        return Reply::Failed(said);
    }
    match effect {
        Some("confirmed") => Reply::Confirmed,
        Some("suspected_noop") => Reply::SuspectedNoop,
        _ => Reply::Delivered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Shapes recorded from Cua Driver 0.47.23 on macOS 27 during the
    // executor comparison (TextEdit fixture window, Finder, a dialog).

    fn snapshot() -> Value {
        json!({
            "content": [{"type": "text", "text": "window_id=5072 pid=71844 elements=3 snapshot_id=s00000001"}],
            "structuredContent": {
                "app_name": "TextEdit", "elements_complete": true, "pid": 71844, "snapshot_id": "s00000001",
                "truncated": false, "window_id": 5072,
                "screenshot_error": {"code": "px_capture_unavailable", "reason": "The user declined TCCs"},
                "elements": [
                    {"actions": ["AXRaise"], "element_index": 0, "element_token": "s00000001:0",
                     "frame": {"h": 439.0, "w": 673.0, "x": 253.0, "y": 151.0}, "label": "theo-probe.txt",
                     "role": "AXWindow", "subrole": "AXStandardWindow", "title": "theo-probe.txt"},
                    {"actions": ["AXShowMenu"], "element_index": 1, "element_token": "s00000001:1",
                     "frame": {"h": 384.0, "w": 656.0, "x": 253.0, "y": 183.0}, "label": "Theo probe document.",
                     "role": "AXTextArea", "value": "Theo probe document."},
                    {"actions": ["AXPress"], "element_index": 2, "element_token": "s00000001:2",
                     "frame": {"h": 16.0, "w": 16.0, "x": 261.0, "y": 159.0}, "role": "AXButton",
                     "subrole": "AXCloseButton"}
                ]
            }
        })
    }

    #[test]
    fn a_window_state_becomes_an_observation() {
        let target = Target {
            pid: 71844,
            window: 5072,
            app: Some("TextEdit".into()),
        };
        let seen = observation(&snapshot(), 7, target.clone(), 1000).unwrap();
        assert_eq!(seen.id, 7);
        assert_eq!(seen.target, target);
        assert_eq!(seen.snapshot, "s00000001");
        assert!(seen.complete);
        assert_eq!(
            seen.window_bounds,
            Some(Rect {
                x: 253.0,
                y: 151.0,
                w: 673.0,
                h: 439.0
            })
        );
        assert_eq!(seen.elements.len(), 3);
        assert_eq!(seen.elements[1].token, "s00000001:1");
        assert_eq!(
            seen.elements[1].value.as_deref(),
            Some("Theo probe document.")
        );
        assert_eq!(seen.elements[2].label, "");
        assert_eq!(
            seen.capture_missing.as_deref(),
            Some("The user declined TCCs")
        );
    }

    #[test]
    fn a_locked_screen_is_no_observation() {
        let locked = json!({"structuredContent": {"degraded": true, "element_count": 0,
            "degraded_reason": "screen_locked: window_id 5072 has no accessibility tree"}});
        let target = Target {
            pid: 1,
            window: 5072,
            app: None,
        };
        assert!(
            observation(&locked, 1, target, 0)
                .unwrap_err()
                .starts_with("screen_locked")
        );
    }

    #[test]
    fn replies_say_what_reached_the_application() {
        let typed = json!({"content": [{"type": "text", "text": "Inserted 15 char(s)"}],
            "structuredContent": {"effect": "confirmed", "route": "accessibility"}});
        assert_eq!(reply(&typed), Reply::Confirmed);
        let pressed =
            json!({"structuredContent": {"effect": "unverifiable", "route": "accessibility"}});
        assert_eq!(reply(&pressed), Reply::Delivered);
        let stale = json!({"isError": true, "content": [{"type": "text", "text":
            "element_token is stale or unknown; call get_window_state again to refresh"}]});
        assert!(matches!(reply(&stale), Reply::Declined { stale: true, .. }));
        let failed = json!({"isError": true, "content": [{"type": "text", "text":
            "AX action failed: AXUIElementPerformAction(AXOpen) returned -25205"}],
            "structuredContent": {"code": "tool_invocation_failed"}});
        assert!(matches!(reply(&failed), Reply::Failed(_)));
        let refused = json!({"structuredContent": {"effect": "refused", "route": "accessibility",
            "error": {"code": "background_refused", "message": "the app ignores background input"}}});
        assert_eq!(
            reply(&refused),
            Reply::Declined {
                stale: false,
                why: "the app ignores background input".into()
            }
        );
    }

    #[test]
    fn windows_carry_their_identity() {
        let listed = json!({"structuredContent": {"windows": [
            {"app_name": "TextEdit", "bounds": {"height": 439.0, "width": 673.0, "x": 300.0, "y": 180.0},
             "is_on_screen": true, "layer": 0, "pid": 71844, "title": "", "window_id": 5072, "z_index": 1}]}});
        let found = windows(&listed);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].target,
            Target {
                pid: 71844,
                window: 5072,
                app: Some("TextEdit".into())
            }
        );
        assert_eq!(
            found[0].bounds,
            Some(Rect {
                x: 300.0,
                y: 180.0,
                w: 673.0,
                h: 439.0
            })
        );
        assert_eq!(found[0].z_index, Some(1));
    }
}
