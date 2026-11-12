//! What Theo needs from macOS that GPUI does not give: which application and
//! window it was called over, handing focus back to it, the reduce-motion
//! setting, where the pointer is, placing the body's window, and hearing a
//! right click anywhere on the desktop.
//!
//! SOURCING: AppKit and Core Graphics through `objc2-app-kit` and
//! `objc2-core-graphics`; the listen-only event tap is Core Graphics' own
//! `CGEventTapCreate`. Nothing here is a reimplementation of either.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
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
    kCGWindowBounds, kCGWindowLayer, kCGWindowNumber, kCGWindowOwnerPID,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSPoint, NSRect, NSSize, NSString};
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

/// A window as the window server lists it: its number, owner, layer and
/// bounds (in its own global coordinates, y down).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Listed {
    number: i64,
    pid: i64,
    layer: i64,
    bounds: Rect,
}

/// The windows on screen, front to back. Reading numbers and bounds needs no
/// permission; titles would.
fn on_screen() -> Vec<Listed> {
    let Some(list) = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return Vec::new();
    };
    // SAFETY: CFArray and NSArray are toll-free bridged, and the window list
    // is an array of dictionaries keyed by strings.
    let windows: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { &*(CFRetained::as_ptr(&list).as_ptr() as *const _) };
    // SAFETY: the keys are Core Graphics' own constant strings, bridged.
    let (owner, layer, id, bounds) = unsafe {
        (
            &*(kCGWindowOwnerPID as *const _ as *const NSString),
            &*(kCGWindowLayer as *const _ as *const NSString),
            &*(kCGWindowNumber as *const _ as *const NSString),
            &*(kCGWindowBounds as *const _ as *const NSString),
        )
    };
    let number = |dict: &NSDictionary<NSString, AnyObject>, key: &NSString| {
        dict.objectForKey(key)
            .and_then(|value| value.downcast::<NSNumber>().ok())
            .map(|value| value.as_f64())
    };
    windows
        .iter()
        .filter_map(|window| {
            let frame = window
                .objectForKey(bounds)
                .and_then(|value| value.downcast::<NSDictionary>().ok())?;
            // SAFETY: a window's bounds are a dictionary of numbers keyed by
            // the strings X, Y, Width and Height.
            let frame: &NSDictionary<NSString, AnyObject> =
                unsafe { &*(Retained::as_ptr(&frame) as *const _) };
            let side = |key: &str| number(frame, &NSString::from_str(key));
            Some(Listed {
                number: number(&window, id)? as i64,
                pid: number(&window, owner)? as i64,
                layer: number(&window, layer)? as i64,
                bounds: Rect {
                    x: side("X")?,
                    y: side("Y")?,
                    w: side("Width")?,
                    h: side("Height")?,
                },
            })
        })
        .collect()
}

/// The frontmost ordinary window (layer 0) of `pid`.
fn front_window(pid: i32) -> Option<u32> {
    on_screen()
        .into_iter()
        .find(|window| window.pid == i64::from(pid) && window.layer == 0)
        .and_then(|window| u32::try_from(window.number).ok())
}

/// The owner of the ordinary window directly beneath window `ours`: the
/// first one after it, front to back, that it overlaps.
fn owner_beneath(ours: i64, windows: &[Listed]) -> Option<i64> {
    let at = windows.iter().position(|window| window.number == ours)?;
    let over = windows[at].bounds;
    windows[at + 1..]
        .iter()
        .find(|window| window.layer == 0 && window.bounds.overlaps(over))
        .map(|window| window.pid)
}

/// Whether the application in front now is the one Theo sits over: the
/// owner of the window directly beneath Theo's, or the Finder where only the
/// desktop is beneath.
pub fn front_is_beneath(window: &Window) -> bool {
    let (Some(ns), Some(front)) = (
        ns_window(window),
        NSWorkspace::sharedWorkspace().frontmostApplication(),
    ) else {
        return false;
    };
    match owner_beneath(ns.windowNumber() as i64, &on_screen()) {
        Some(pid) => pid == i64::from(front.processIdentifier()),
        None => front
            .bundleIdentifier()
            .is_some_and(|id| id.to_string() == "com.apple.finder"),
    }
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

pub(super) fn ns_window(window: &Window) -> Option<Retained<objc2_app_kit::NSWindow>> {
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
pub(super) fn after_update(
    window: &Window,
    cx: &App,
    change: impl FnOnce(&objc2_app_kit::NSWindow) + 'static,
) {
    if let Some(ns) = ns_window(window) {
        cx.foreground_executor()
            .spawn(async move { change(&ns) })
            .detach();
    }
}

/// The pointer in the window's own points, from its top left, y down.
pub fn pointer_in(window: &Window) -> Option<(f64, f64)> {
    let frame = ns_window(window)?.frame();
    let at: NSPoint = NSEvent::mouseLocation();
    Some((
        at.x - frame.origin.x,
        frame.origin.y + frame.size.height - at.y,
    ))
}

// The menu beside the face ---------------------------------------------------

/// Which side of the face the menu opened on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    /// Under the face, as a context menu opens under the pointer.
    #[default]
    Below,
    /// Over it, when the screen has no room underneath.
    Above,
}

/// How the window holding the face and its open menu is laid out: the side
/// the menu took and how far in from the window's left edge the face sits.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MenuForm {
    pub side: Side,
    pub face_x: f64,
}

/// A rectangle in AppKit's screen coordinates, y up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl From<NSRect> for Rect {
    fn from(rect: NSRect) -> Self {
        Self {
            x: rect.origin.x,
            y: rect.origin.y,
            w: rect.size.width,
            h: rect.size.height,
        }
    }
}

impl Rect {
    /// Whether the two share any area; touching edges do not count.
    fn overlaps(self, other: Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }
}

impl From<Rect> for NSRect {
    fn from(rect: Rect) -> Self {
        NSRect::new(NSPoint::new(rect.x, rect.y), NSSize::new(rect.w, rect.h))
    }
}

/// The window that holds `face` and a menu, `size` in all, without moving
/// the face: the menu goes under it when the screen has room there, over it
/// otherwise, and is kept on the screen sideways.
pub fn menu_frame(face: Rect, screen: Rect, size: (f64, f64)) -> (Rect, MenuForm) {
    let extra = (size.1 - face.h).max(0.0);
    let side = if face.y - screen.y >= extra {
        Side::Below
    } else {
        Side::Above
    };
    let centred = face.x + face.w / 2.0 - size.0 / 2.0;
    let x = centred.clamp(screen.x, (screen.x + screen.w - size.0).max(screen.x));
    let y = match side {
        Side::Below => face.y - extra,
        Side::Above => face.y,
    };
    let frame = Rect {
        x,
        y,
        w: size.0,
        h: size.1,
    };
    (
        frame,
        MenuForm {
            side,
            face_x: face.x - x,
        },
    )
}

/// The face's own rectangle inside a window laid out as `form`.
pub fn face_in(frame: Rect, form: MenuForm, face: (f64, f64)) -> Rect {
    Rect {
        x: frame.x + form.face_x,
        y: match form.side {
            Side::Below => frame.y + frame.h - face.1,
            Side::Above => frame.y,
        },
        w: face.0,
        h: face.1,
    }
}

/// Grows the face's window to hold the menu, `size` in all, and records in
/// `form` how it was laid out. Decided where the window changes, after the
/// GPUI update asking for it, so it reads the face where it really is.
pub fn open_menu(window: &Window, size: (f64, f64), form: Rc<Cell<Option<MenuForm>>>, cx: &App) {
    after_update(window, cx, move |ns| {
        if form.get().is_some() {
            return;
        }
        let Some(screen) = visible_frame(ns) else {
            return;
        };
        let (frame, laid) = menu_frame(ns.frame().into(), screen.into(), size);
        form.set(Some(laid));
        ns.setFrame_display_animate(frame.into(), true, false);
    });
}

/// Shrinks the window back to the face alone, where the face is now.
pub fn close_menu(window: &Window, face: (f64, f64), form: Rc<Cell<Option<MenuForm>>>, cx: &App) {
    after_update(window, cx, move |ns| {
        let Some(laid) = form.take() else {
            return;
        };
        let rect = face_in(ns.frame().into(), laid, face);
        ns.setFrame_display_animate(rect.into(), true, false);
    });
}

/// The screen's visible frame the window is on, in AppKit's coordinates.
pub(super) fn visible_frame(window: &objc2_app_kit::NSWindow) -> Option<NSRect> {
    Some(window.screen()?.visibleFrame())
}

/// Where the window's bottom-left corner sits as a fraction of its screen's
/// visible frame (0..1 each way, from the bottom left), so a place survives
/// the window growing taller.
/// While the menu is open (`form`), the spot is the face's, `face` in size.
pub fn spot_of(window: &Window, form: Option<(MenuForm, (f64, f64))>) -> Option<(f64, f64)> {
    let window = ns_window(window)?;
    let screen = visible_frame(&window)?;
    let frame: Rect = match form {
        Some((form, face)) => face_in(window.frame().into(), form, face),
        None => window.frame().into(),
    };
    Some((
        (frame.x - screen.origin.x) / (screen.size.width - frame.w).max(1.0),
        (frame.y - screen.origin.y) / (screen.size.height - frame.h).max(1.0),
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

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1440.0,
        h: 900.0,
    };
    const FACE: (f64, f64) = (264.0, 132.0);
    const OPEN: (f64, f64) = (344.0, 572.0);

    fn face_at(x: f64, y: f64) -> Rect {
        Rect {
            x,
            y,
            w: FACE.0,
            h: FACE.1,
        }
    }

    #[test]
    fn the_menu_opens_under_the_face_without_moving_it() {
        let face = face_at(600.0, 700.0);
        let (frame, form) = menu_frame(face, SCREEN, OPEN);
        assert_eq!(form.side, Side::Below);
        assert_eq!(face_in(frame, form, FACE), face);
        assert_eq!(frame.x + frame.w / 2.0, face.x + face.w / 2.0);
    }

    #[test]
    fn the_menu_opens_over_the_face_at_the_bottom_of_the_screen() {
        let face = face_at(600.0, 36.0);
        let (frame, form) = menu_frame(face, SCREEN, OPEN);
        assert_eq!(form.side, Side::Above);
        assert_eq!(face_in(frame, form, FACE), face);
    }

    #[test]
    fn the_menu_stays_on_the_screen_beside_an_edge() {
        let face = face_at(SCREEN.w - FACE.0, 700.0);
        let (frame, form) = menu_frame(face, SCREEN, OPEN);
        assert_eq!(frame.x + frame.w, SCREEN.w);
        assert_eq!(face_in(frame, form, FACE), face);
    }

    fn listed(number: i64, pid: i64, layer: i64, x: f64, y: f64, w: f64, h: f64) -> Listed {
        Listed {
            number,
            pid,
            layer,
            bounds: Rect { x, y, w, h },
        }
    }

    #[test]
    fn the_app_beneath_is_the_first_ordinary_window_theo_overlaps() {
        let theo = listed(9, 1, 101, 100.0, 100.0, 300.0, 300.0);
        let windows = [
            // In front of Theo: not beneath it.
            listed(1, 50, 0, 0.0, 0.0, 800.0, 800.0),
            theo,
            // The menu bar and a window elsewhere on screen are passed over.
            listed(2, 60, 24, 0.0, 0.0, 1440.0, 24.0),
            listed(3, 70, 0, 900.0, 100.0, 200.0, 200.0),
            listed(4, 80, 0, 50.0, 50.0, 600.0, 600.0),
            listed(5, 90, 0, 0.0, 0.0, 1440.0, 900.0),
        ];
        assert_eq!(owner_beneath(9, &windows), Some(80));
    }

    #[test]
    fn theo_over_its_own_app_is_over_that_app() {
        let windows = [
            listed(9, 1, 101, 100.0, 100.0, 300.0, 300.0),
            listed(4, 1, 0, 0.0, 0.0, 1440.0, 900.0),
        ];
        assert_eq!(owner_beneath(9, &windows), Some(1));
    }

    #[test]
    fn nothing_beneath_over_the_desktop_or_when_theo_is_not_listed() {
        let windows = [
            listed(9, 1, 101, 100.0, 100.0, 300.0, 300.0),
            // Touching edges only.
            listed(4, 80, 0, 400.0, 100.0, 300.0, 300.0),
        ];
        assert_eq!(owner_beneath(9, &windows), None);
        assert_eq!(owner_beneath(7, &windows), None);
    }
}
