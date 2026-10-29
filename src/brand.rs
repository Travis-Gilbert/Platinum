//! The app's name and icon. A `.app` bundle names its icon in `Info.plist`; Platinum is run
//! as a bare binary, which the Dock shows with a generic icon, so the icon is
//! set at launch instead. The artwork is `assets/brand/icons/Platinum.icns`,
//! the same file a bundle would carry.
//!
//! SOURCING: AppKit's `NSApplication.applicationIconImage`, through
//! `objc2-app-kit`, as `glass.rs` and `share.rs` reach AppKit.

/// The app's name, for menus, titles and the folders it keeps its files in.
pub const NAME: &str = frame_profile::APP_NAME;

pub use frame_profile::app_dir;

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
