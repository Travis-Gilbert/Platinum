//! The app's name and icon. A `.app` bundle names its icon in `Info.plist`; Platinum is run
//! as a bare binary, which the Dock shows with a generic icon, so the icon is
//! set at launch instead. The artwork is `assets/brand/icons/Platinum.icns`,
//! the same file a bundle would carry.
//!
//! SOURCING: AppKit's `NSApplication.applicationIconImage`, through
//! `objc2-app-kit`, as `glass.rs` and `share.rs` reach AppKit.

/// The app's name, for menus, titles and the folders it keeps its files in.
pub const NAME: &str = "Platinum";

/// The app's folder under `base` (the data or cache directory): `Platinum`,
/// taking over the `Frame` folder the app kept before it was renamed, so its
/// profile, index and downloaded model carry over. A `Frame` link is left in
/// its place, since builds of branches from before the rename still open it.
pub fn app_dir(base: &std::path::Path) -> std::path::PathBuf {
    let dir = base.join(NAME);
    let legacy = base.join("Frame");
    if !dir.exists() && legacy.is_dir() {
        let moved = std::fs::rename(&legacy, &dir)
            .and_then(|()| std::os::unix::fs::symlink(NAME, &legacy));
        if let Err(error) = moved {
            log::warn!(
                "could not move {} to {}: {error}",
                legacy.display(),
                dir.display()
            );
        }
    }
    dir
}

/// Show the Platinum icon in the Dock and the app switcher.
#[cfg(target_os = "macos")]
pub fn install() {
    use objc2::{AnyThread as _, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    const ICON: &[u8] = include_bytes!("../assets/brand/icons/Platinum.icns");

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(ICON);
    match NSImage::initWithData(NSImage::alloc(), &data) {
        // SAFETY: the main thread, with an image AppKit decoded.
        Some(image) => unsafe {
            NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image))
        },
        None => log::warn!("the app icon could not be decoded"),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install() {}
