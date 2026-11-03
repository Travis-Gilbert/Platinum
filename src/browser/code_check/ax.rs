//! The window's accessibility tree, read in process from the answers AppKit's
//! `NSAccessibility` methods give for the window's content view. Asking is what an
//! assistive client does, so it also starts GPUI's AccessKit tree.
use gpui_kit::Window;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, msg_send, sel};
use objc2_foundation::{NSArray, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde_json::{Map, Value, json};

const DEPTH: usize = 40;
const NODES: usize = 4000;

/// The tree under the window's view, or `None` off the main thread or
/// without an AppKit view.
pub(super) fn walk(window: &Window) -> Option<Value> {
    MainThreadMarker::new()?;
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: the view is GPUI's, alive as long as the window, and this is
    // the main thread, where AppKit's views are used.
    let view: &AnyObject = unsafe { handle.ns_view.cast().as_ref() };
    // AccessKit's adapter subclasses the window's content view, which holds
    // GPUI's view; the tree hangs from the content view.
    // SAFETY: `window` and `contentView` take nothing and return an object
    // or nil.
    let window: Option<Retained<AnyObject>> = unsafe { msg_send![view, window] };
    let content: Option<Retained<AnyObject>> =
        window.and_then(|window| unsafe { msg_send![&*window, contentView] });
    let mut count = 0;
    Some(autoreleasepool(|_| {
        node(content.as_deref().unwrap_or(view), 0, &mut count)
    }))
}

fn node(object: &AnyObject, depth: usize, count: &mut usize) -> Value {
    *count += 1;
    let mut fields = Map::new();
    fields.insert(
        "class".into(),
        json!(object.class().name().to_string_lossy()),
    );
    for (name, selector) in [
        ("role", sel!(accessibilityRole)),
        ("label", sel!(accessibilityLabel)),
        ("title", sel!(accessibilityTitle)),
        ("identifier", sel!(accessibilityIdentifier)),
    ] {
        if let Some(text) = string(object, selector) {
            fields.insert(name.into(), json!(text));
        }
    }
    if responds(object, sel!(accessibilityValue)) {
        // SAFETY: `accessibilityValue` takes nothing and returns an object.
        let value: Option<Retained<AnyObject>> = unsafe { msg_send![object, accessibilityValue] };
        if let Some(value) = value {
            // SAFETY: every object answers `description` with a string.
            let text: Retained<NSString> = unsafe { msg_send![&*value, description] };
            fields.insert("value".into(), json!(text.to_string()));
        }
    }
    if depth < DEPTH && *count < NODES && responds(object, sel!(accessibilityChildren)) {
        // SAFETY: `accessibilityChildren` takes nothing and returns an array
        // of objects or nil.
        let children: Option<Retained<NSArray<AnyObject>>> =
            unsafe { msg_send![object, accessibilityChildren] };
        if let Some(children) = children {
            let children: Vec<Value> = children
                .iter()
                .map(|child| node(&child, depth + 1, count))
                .collect();
            if !children.is_empty() {
                fields.insert("children".into(), Value::Array(children));
            }
        }
    }
    Value::Object(fields)
}

fn responds(object: &AnyObject, selector: Sel) -> bool {
    object.class().responds_to(selector)
}

fn string(object: &AnyObject, selector: Sel) -> Option<String> {
    if !responds(object, selector) {
        return None;
    }
    // SAFETY: each selector passed here takes nothing and returns a string
    // or nil.
    let text: Option<Retained<NSString>> = unsafe { msg_send![object, performSelector: selector] };
    text.map(|text| text.to_string())
        .filter(|text| !text.is_empty())
}
