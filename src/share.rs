//! Sharing a page through the system's share sheet: Mail, Messages, AirDrop,
//! Notes and the rest, as Safari offers them.
//!
//! SOURCING: AppKit's `NSSharingServicePicker`, through `objc2-app-kit`,
//! shown the way GPUI Kit shows its native menus (`native_menu/macos.rs`):
//! from a foreground task, so that GPUI is not borrowed while AppKit runs,
//! after which the window is woken to paint again.

use gpui_kit::{App, Bounds, Pixels, Window};
use url::Url;

/// Whether this platform has a share sheet.
pub const SUPPORTED: bool = cfg!(target_os = "macos");

/// Show the share sheet for `url` below `anchor`, in window coordinates.
#[cfg(target_os = "macos")]
pub fn share(url: &Url, anchor: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let Some(view) = macos::ns_view(window) else {
        return;
    };
    // GPUI's `AnyWindowHandle`, not the `raw_window_handle` trait method.
    let handle = Window::window_handle(window);
    let url = url.to_string();
    cx.spawn(async move |cx| {
        macos::show(view, &url, anchor);
        let _ = cx.update(move |app| {
            let _ = handle.update(app, |_, window, _| window.refresh());
        });
    })
    .detach();
}

#[cfg(not(target_os = "macos"))]
pub fn share(_: &Url, _: Bounds<Pixels>, _: &mut Window, _: &mut App) {}

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::RefCell;

    use gpui_kit::{Bounds, Pixels, Window};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{AnyThread as _, MainThreadMarker};
    use objc2_app_kit::{NSSharingServicePicker, NSView};
    use objc2_foundation::{NSArray, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    thread_local! {
        /// The picker last shown, kept alive while it is on screen.
        static SHOWN: RefCell<Option<Retained<NSSharingServicePicker>>> =
            const { RefCell::new(None) };
    }

    /// The window's AppKit view, as an address the foreground task can hold.
    pub fn ns_view(window: &Window) -> Option<usize> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return None;
        };
        Some(handle.ns_view.as_ptr() as usize)
    }

    pub fn show(view: usize, url: &str, anchor: Bounds<Pixels>) {
        if MainThreadMarker::new().is_none() {
            return;
        }
        let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) else {
            return;
        };
        // SAFETY: the address came from the window's AppKit handle, and the
        // window outlives this synchronous call.
        let view: &NSView = unsafe { &*(view as *const NSView) };
        let item: &AnyObject = &url;
        let items = NSArray::from_slice(&[item]);
        // SAFETY: an NSURL is one of the item types the picker takes.
        let picker = unsafe {
            NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), &items)
        };
        // GPUI's origin is the top left; AppKit's is the bottom left.
        let height = view.bounds().size.height;
        let pixels = |value: Pixels| f64::from(f32::from(value));
        let rect = NSRect::new(
            NSPoint::new(
                pixels(anchor.origin.x),
                height - pixels(anchor.origin.y + anchor.size.height),
            ),
            NSSize::new(pixels(anchor.size.width), pixels(anchor.size.height)),
        );
        picker.showRelativeToRect_ofView_preferredEdge(rect, view, NSRectEdge::MinY);
        SHOWN.with(|shown| *shown.borrow_mut() = Some(picker));
    }
}
