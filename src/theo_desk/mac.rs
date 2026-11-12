//! What Theo needs from macOS that GPUI does not give: which application and
//! window it was called over, handing focus back to it, the reduce-motion
//! setting, where the pointer is, placing the body's window, and hearing a
//! right click anywhere on the desktop.
//!
//! SOURCING: AppKit and Core Graphics through `objc2-app-kit` and
//! `objc2-core-graphics`; the listen-only event tap is Core Graphics' own
//! `CGEventTapCreate`. Nothing here is a reimplementation of either.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::OnceLock;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::{App, Window};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSApplicationActivationOptions, NSEvent, NSRunningApplication, NSView, NSWorkspace,
};
use objc2_core_foundation::{CFMachPort, CFRetained, CFRunLoop, kCFRunLoopCommonModes};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventSourceStateID, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType, CGPreflightListenEventAccess,
    CGRequestListenEventAccess, CGWindowListCopyWindowInfo, CGWindowListOption, kCGNullWindowID,
    kCGWindowLayer, kCGWindowNumber, kCGWindowOwnerPID,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSPoint, NSRect, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// The application in front when Theo was called, and its frontmost window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Front {
    pub pid: i32,
    pub bundle: Option<String>,
    pub name: Option<String>,
    /// The window server's number for its frontmost ordinary window.
    pub window: Option<u32>,
}

/// The application in front now, unless it is this one.
pub fn frontmost() -> Option<Front> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let pid = app.processIdentifier();
    if pid == std::process::id() as i32 {
        return None;
    }
    Some(Front {
        pid,
        bundle: app.bundleIdentifier().map(|id| id.to_string()),
        name: app.localizedName().map(|name| name.to_string()),
        window: front_window(pid),
    })
}

/// The frontmost ordinary window (layer 0) of `pid`, as the window server
/// lists them front to back. Reading numbers needs no permission; titles would.
fn front_window(pid: i32) -> Option<u32> {
    let list = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        kCGNullWindowID,
    )?;
    // SAFETY: CFArray and NSArray are toll-free bridged, and the window list
    // is an array of dictionaries keyed by strings.
    let windows: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { &*(CFRetained::as_ptr(&list).as_ptr() as *const _) };
    let number = |window: &NSDictionary<NSString, AnyObject>, key: &NSString| {
        window
            .objectForKey(key)
            .and_then(|value| value.downcast::<NSNumber>().ok())
            .map(|value| value.as_i64())
    };
    // SAFETY: the keys are Core Graphics' own constant strings, bridged.
    let (owner, layer, id) = unsafe {
        (
            &*(kCGWindowOwnerPID as *const _ as *const NSString),
            &*(kCGWindowLayer as *const _ as *const NSString),
            &*(kCGWindowNumber as *const _ as *const NSString),
        )
    };
    windows.iter().find_map(|window| {
        (number(&window, owner) == Some(i64::from(pid)) && number(&window, layer) == Some(0))
            .then(|| number(&window, id))
            .flatten()
            .and_then(|id| u32::try_from(id).ok())
    })
}

/// Hands focus back to the application Theo was called over.
pub fn return_to(pid: i32) {
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        app.activateWithOptions(NSApplicationActivationOptions::empty());
    }
}

/// Whether this process is the active application.
pub fn we_are_active() -> bool {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .is_some_and(|app| app.processIdentifier() == std::process::id() as i32)
}

/// The system's reduce-motion setting.
pub fn reduce_motion() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

fn ns_window(window: &Window) -> Option<Retained<objc2_app_kit::NSWindow>> {
    MainThreadMarker::new()?;
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: the handle is the live window's AppKit view.
    let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
    view.window()
}

/// Takes away the frame macOS draws around a window. With no titlebar GPUI
/// still makes a titled window, so AppKit draws its rounded rim and a shadow
/// around the whole rectangle even where nothing is painted. Borderless, the
/// window is only what it paints; GPUI's panel class can still become key,
/// so the menu still takes the keyboard.
pub fn unframe(window: &Window, cx: &App) {
    use objc2_app_kit::NSWindowStyleMask;
    after_update(window, cx, |ns| {
        let panel = ns.styleMask() & NSWindowStyleMask::NonactivatingPanel;
        // Changing the style swaps AppKit's frame view, which can leave the
        // window itself as first responder: keys would then stop at the
        // window and never reach GPUI's view, though a field still shows its
        // caret. Give the responder back.
        let responder = ns.firstResponder();
        ns.setStyleMask(NSWindowStyleMask::Borderless | panel);
        ns.setHasShadow(false);
        let kept = ns.firstResponder() == responder;
        if !kept {
            let restored = ns.makeFirstResponder(responder.as_deref());
            log::info!("theo: unframe moved the first responder; restored: {restored}");
        }
    });
}

/// Changes the native window once the GPUI update asking for it has ended.
/// AppKit answers a new frame or style at once, telling GPUI the window's new
/// size while GPUI is still updating that window; GPUI cannot take it then,
/// the size is lost, and the window goes on drawing at the old one. GPUI's
/// own `resize` waits the same way, on the foreground executor.
fn after_update(window: &Window, cx: &App, change: impl FnOnce(&objc2_app_kit::NSWindow) + 'static) {
    if let Some(ns) = ns_window(window) {
        cx.foreground_executor()
            .spawn(async move { change(&ns) })
            .detach();
    }
}

/// The pointer relative to the window's centre, in points, y down.
pub fn pointer_from(window: &Window) -> Option<(f64, f64)> {
    let frame = ns_window(window)?.frame();
    let at: NSPoint = NSEvent::mouseLocation();
    let centre = (
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    );
    Some((at.x - centre.0, centre.1 - at.y))
}

/// The screen's visible frame the window is on, in AppKit's coordinates.
fn visible_frame(window: &objc2_app_kit::NSWindow) -> Option<NSRect> {
    Some(window.screen()?.visibleFrame())
}

/// Where the window's bottom-left corner sits as a fraction of its screen's
/// visible frame (0..1 each way, from the bottom left), so a place survives
/// the window growing taller.
pub fn spot_of(window: &Window) -> Option<(f64, f64)> {
    let window = ns_window(window)?;
    let screen = visible_frame(&window)?;
    let frame = window.frame();
    Some((
        (frame.origin.x - screen.origin.x) / (screen.size.width - frame.size.width).max(1.0),
        (frame.origin.y - screen.origin.y) / (screen.size.height - frame.size.height).max(1.0),
    ))
}

/// Puts the window at a spot (as `spot_of` gives it), easing there over
/// AppKit's own animation when `animate`.
pub fn place(window: &Window, spot: (f64, f64), animate: bool, cx: &App) {
    after_update(window, cx, move |ns| {
        let size = ns.frame().size;
        fit(ns, (size.width, size.height), spot, animate);
    });
}

/// Gives the window a new size and puts it at a spot, in one move, on the
/// screen it is on now: resizing first could carry it onto a neighbouring
/// screen before it is placed.
pub fn resize_at(window: &Window, size: (f64, f64), spot: (f64, f64), cx: &App) {
    after_update(window, cx, move |ns| fit(ns, size, spot, false));
}

fn fit(ns: &objc2_app_kit::NSWindow, size: (f64, f64), spot: (f64, f64), animate: bool) {
    let Some(screen) = visible_frame(ns) else {
        return;
    };
    let mut frame = ns.frame();
    frame.size.width = size.0;
    frame.size.height = size.1;
    let room = (
        (screen.size.width - frame.size.width).max(0.0),
        (screen.size.height - frame.size.height).max(0.0),
    );
    frame.origin.x = screen.origin.x + room.0 * spot.0.clamp(0.0, 1.0);
    frame.origin.y = screen.origin.y + room.1 * spot.1.clamp(0.0, 1.0);
    ns.setFrame_display_animate(frame, true, animate);
}

// A right click anywhere -----------------------------------------------------

/// A right click the person made with the mouse, in global points, y down.
#[derive(Clone, Copy, Debug)]
pub struct RightClick {
    pub x: f64,
    pub y: f64,
}

/// Why the desktop's right clicks cannot be heard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Deaf {
    /// Input Monitoring is not granted to this application. macOS has been
    /// asked to offer it; the setting takes effect after a relaunch.
    NotPermitted,
    /// The tap could not be made for another reason.
    Refused,
}

static CLICKS: OnceLock<UnboundedSender<RightClick>> = OnceLock::new();
static TAP: OnceLock<usize> = OnceLock::new();

/// Starts hearing right clicks on the desktop, once per process. The tap only
/// listens: every click goes on to where it was going, the first of two
/// included, so a double right click also opens whatever menu the first one
/// would have. Clicks that a program posted rather than the hardware are not
/// passed on.
pub fn hear_right_clicks() -> Result<UnboundedReceiver<RightClick>, Deaf> {
    if CLICKS.get().is_some() {
        return Err(Deaf::Refused);
    }
    if !CGPreflightListenEventAccess() {
        CGRequestListenEventAccess();
        return Err(Deaf::NotPermitted);
    }
    let (sender, receiver) = unbounded();
    let (ready, made) = std::sync::mpsc::channel();
    let _ = CLICKS.set(sender);
    std::thread::Builder::new()
        .name("theo-right-clicks".into())
        .spawn(move || {
            // SAFETY: the callback matches the tap's signature and reads only
            // the event it is given; it takes no user data.
            let tap = unsafe {
                CGEvent::tap_create(
                    CGEventTapLocation::SessionEventTap,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::ListenOnly,
                    1 << CGEventType::RightMouseDown.0,
                    Some(heard),
                    std::ptr::null_mut(),
                )
            };
            let Some(tap) = tap else {
                let _ = ready.send(false);
                return;
            };
            let source = CFMachPort::new_run_loop_source(None, Some(&tap), 0);
            let Some(run_loop) = CFRunLoop::current() else {
                let _ = ready.send(false);
                return;
            };
            // SAFETY: the mode is Core Foundation's own constant.
            run_loop.add_source(source.as_deref(), unsafe { kCFRunLoopCommonModes });
            let _ = TAP.set(CFRetained::as_ptr(&tap).as_ptr() as usize);
            CGEvent::tap_enable(&tap, true);
            let _ = ready.send(true);
            CFRunLoop::run();
        })
        .map_err(|_| Deaf::Refused)?;
    match made.recv() {
        Ok(true) => Ok(receiver),
        _ => Err(Deaf::Refused),
    }
}

unsafe extern "C-unwind" fn heard(
    _proxy: CGEventTapProxy,
    kind: CGEventType,
    event: NonNull<CGEvent>,
    _user: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: Core Graphics hands the tap a live event for the call.
    let seen = unsafe { event.as_ref() };
    if kind == CGEventType::TapDisabledByTimeout || kind == CGEventType::TapDisabledByUserInput {
        if let Some(&tap) = TAP.get() {
            // SAFETY: the tap is kept alive by its run loop for the process.
            CGEvent::tap_enable(unsafe { &*(tap as *const CFMachPort) }, true);
        }
        return event.as_ptr();
    }
    let hardware = CGEvent::integer_value_field(Some(seen), CGEventField::EventSourceStateID)
        == i64::from(CGEventSourceStateID::HIDSystemState.0);
    if kind == CGEventType::RightMouseDown && hardware {
        let at = CGEvent::location(Some(seen));
        if let Some(clicks) = CLICKS.get() {
            let _ = clicks.unbounded_send(RightClick { x: at.x, y: at.y });
        }
    }
    event.as_ptr()
}

/// Whether the window lets clicks through to what is beneath it.
pub fn pass_clicks(window: &Window, pass: bool) {
    if let Some(ns) = ns_window(window)
        && ns.ignoresMouseEvents() != pass
    {
        ns.setIgnoresMouseEvents(pass);
    }
}

/// Whether the left mouse button is held now, wherever the pointer is.
pub fn left_button_down() -> bool {
    NSEvent::pressedMouseButtons() & 1 == 1
}
