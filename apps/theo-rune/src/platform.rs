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
use crate::preferences::{self, APP_ID, Writer};
use crate::presentation::{Frame, PRESENCE_SIZE, presence_home, workspace_around};
use crate::session::AppSession;
use crate::views;

/// Summons or dismisses Theo. The GPUI desk's default is the same chord;
/// while both run, the second to register is told it is taken.
pub const TOGGLE: &str = "ctrl+alt+t";

/// NSWindowCollectionBehavior: on every Space, and beside full-screen apps.
const ALL_SPACES: usize = (1 << 0) | (1 << 8);

/// The display's frame, as presentation frames are measured.
fn display_frame(display: &Display) -> (Frame, f64) {
    let scale = display.scale.max(1.0);
    let frame = Frame {
        x: display.position.0,
        y: display.position.1,
        w: (f64::from(display.size.0) / scale) as f32,
        h: (f64::from(display.size.1) / scale) as f32,
    };
    (frame, scale)
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
        let session = AppSession::new(saved, home, workspace, 0.0);
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
            ],
            ..AppSpec::new("Theo", (first.w, first.h), Theo::headless, views::view)
        };
        let app = App::new(spec)
            .init(move || Theo::new(session, handle, Some(Writer::spawn(prefs))))
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
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

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
