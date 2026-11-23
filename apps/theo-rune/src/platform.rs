//! The native desktop host: the one window, how it is made, and the few
//! operating-system calls Rune does not make for Theo.
//!
//! The window is a transparent, borderless, key-taking panel on every
//! Space: the presence floats over other windows without activating Theo,
//! and the workspace in the same window takes the keyboard. Rune's own
//! window calls move and size it; giving focus back to the application
//! Theo was summoned over is AppKit's `NSRunningApplication`, as the GPUI
//! desk does it (`src/theo_desk/mac.rs`).

use std::sync::{Arc, OnceLock};

use rune::prefs::Prefs;
use rune::{
    App, AppSpec, Display, MacWindowOptions, RunOptions, WindowHandle, WindowOptions, WindowSpec,
};

use crate::app::Theo;
use crate::behavior::store::{self, Library};
use crate::preferences::{self, APP_ID, Writer};
use crate::presentation::{
    Frame, Insets, PRESENCE_SIZE, presence_home, work_area, workspace_around,
};
use crate::session::AppSession;
use crate::views;

/// Summons or dismisses Theo. The GPUI desk's default is the same chord;
/// while both run, the second to register is told it is taken.
pub const TOGGLE: &str = "ctrl+alt+t";

/// NSWindowCollectionBehavior: on every Space, and beside full-screen apps.
const ALL_SPACES: usize = (1 << 0) | (1 << 8);

/// The part of the primary display Theo may cover, as presentation frames
/// are measured: the display less the menu bar and the Dock.
fn display_frame(display: &Display) -> (Frame, f64) {
    let scale = display.scale.max(1.0);
    let frame = Frame {
        x: display.position.0,
        y: display.position.1,
        w: (f64::from(display.size.0) / scale) as f32,
        h: (f64::from(display.size.1) / scale) as f32,
    };
    (
        work_area(frame, primary_insets((frame.w, frame.h)), scale),
        scale,
    )
}

/// Starts Theo and runs until it quits.
pub fn run() {
    rune::run_windows_with(RunOptions { accessory: true }, |displays| {
        let primary = displays
            .iter()
            .find(|d| d.primary)
            .or(displays.first())
            .cloned();
        let prefs = Prefs::open(APP_ID);
        let saved = preferences::load(&prefs);
        let (display, scale) = primary.as_ref().map_or(
            (
                Frame {
                    x: 0,
                    y: 0,
                    w: 1440.0,
                    h: 900.0,
                },
                2.0,
            ),
            display_frame,
        );
        let home = presence_home(display, scale);
        let workspace = workspace_around(saved.presence.unwrap_or(home), display, scale);
        let behaviors = store::default_dir();
        let catalog = store::load(&behaviors);
        let session = AppSession::new(
            saved,
            home,
            workspace,
            0.0,
            AppSession::fresh_seed(),
            catalog,
        );
        let first = session.presentation.frame();
        let floats = !session.presentation.shows_workspace();

        let handle: Arc<OnceLock<WindowHandle>> = Arc::new(OnceLock::new());
        let kept = handle.clone();
        let spec = AppSpec {
            min_size: Some(PRESENCE_SIZE),
            poll: Some(|s: &mut Theo, t, tx| s.poll(t, tx)),
            // Named states for `--snap --scene`, so a headless run can show each form.
            scenes: &[
                ("presence", |_| {}),
                ("workspace", |s| {
                    s.settle_in(crate::presentation::Request::Expand)
                }),
                ("opening", |s| s.begin(crate::presentation::Request::Expand)),
                // The presence with every behavior in the behaviors folder running.
                ("authored", |s| s.author_folder()),
            ],
            ..AppSpec::new("Theo", (first.w, first.h), Theo::headless, views::view)
        };
        let app = App::new(spec);
        let app_handle = app.handle();
        let app = app
            .init(move || {
                Theo::new(
                    session,
                    Some(app_handle),
                    handle,
                    Some(Writer::spawn(prefs)),
                    Some(Library::open(behaviors)),
                )
            })
            .keep_running()
            .global_shortcut(TOGGLE, Theo::toggle)
            .on_window(move |window| {
                let _ = kept.set(window.clone());
            });
        let options = WindowOptions {
            transparent: true,
            borderless: true,
            always_on_top: floats,
            position: Some((first.x, first.y)),
            // The workspace's least size is the person's; the window must
            // still shrink to the presence.
            min_size: Some(PRESENCE_SIZE),
            resizable: true,
            ..WindowOptions::default()
        }
        .macos(
            MacWindowOptions::default()
                .nonactivating_panel(true)
                .shadow(false)
                .collection_behavior(ALL_SPACES),
        );
        vec![WindowSpec::from_app("theo", app).configured_options(options)]
    });
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSApplicationActivationOptions, NSRunningApplication, NSScreen, NSWorkspace,
    };

    use crate::presentation::Insets;

    /// The menu bar and Dock on the primary display: the first screen, which
    /// AppKit measures from its bottom-left, so each edge is the gap between
    /// its whole frame and its visible frame. None unless that screen is
    /// `size` in points, so a virtual display is never given this Mac's Dock.
    pub fn primary_insets(size: (f32, f32)) -> Option<Insets> {
        let screen = NSScreen::screens(MainThreadMarker::new()?).firstObject()?;
        let (whole, visible) = (screen.frame(), screen.visibleFrame());
        if (whole.size.width as f32, whole.size.height as f32) != size {
            return None;
        }
        let edge = |gap: f64| gap.max(0.0) as f32;
        Some(Insets {
            left: edge(visible.origin.x - whole.origin.x),
            bottom: edge(visible.origin.y - whole.origin.y),
            right: edge(
                (whole.origin.x + whole.size.width) - (visible.origin.x + visible.size.width),
            ),
            top: edge(
                (whole.origin.y + whole.size.height) - (visible.origin.y + visible.size.height),
            ),
        })
    }

    pub fn frontmost_other() -> Option<i32> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let pid = app.processIdentifier();
        (pid != std::process::id() as i32).then_some(pid)
    }

    pub fn return_to(pid: i32) {
        if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
            app.activateWithOptions(NSApplicationActivationOptions::empty());
        }
    }
}

/// What the system keeps along the edges of the primary display, `size` in
/// points; none where it cannot be read.
fn primary_insets(size: (f32, f32)) -> Insets {
    #[cfg(target_os = "macos")]
    return mac::primary_insets(size).unwrap_or_default();
    #[cfg(not(target_os = "macos"))]
    {
        let _ = size;
        Insets::default()
    }
}

/// The application in front, unless it is Theo.
pub fn frontmost_other() -> Option<i32> {
    #[cfg(target_os = "macos")]
    return mac::frontmost_other();
    #[cfg(not(target_os = "macos"))]
    None
}

/// Gives the keyboard back to the application with `pid`.
pub fn return_to(pid: i32) {
    #[cfg(target_os = "macos")]
    mac::return_to(pid);
    #[cfg(not(target_os = "macos"))]
    let _ = pid;
}
