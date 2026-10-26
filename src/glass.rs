//! Liquid Glass under the window's chrome. The window is transparent and an
//! AppKit glass view lies behind GPUI's one view, so wherever the chrome
//! leaves its frame partly clear, the glass, and the desktop through it, show.
//! Pages, their toolbars and the selected tab stay opaque, so a page reads
//! exactly as it did.
//!
//! The glass takes the chrome's appearance, not the system's: Platinum in Light
//! on a Dark system gets light glass, which dark glass under the light frame
//! would otherwise muddy into a flat grey.
//!
//! Without the glass (before macOS 26, or with Reduce Transparency on) the
//! window stays opaque and the frame keeps the theme's solid color.
//!
//! SOURCING: AppKit's `NSGlassEffectView`, through `objc2-app-kit`, added
//! under GPUI's view the way GPUI adds its own blur view
//! (`set_background_appearance` with `Blurred`, in `gpui-pre-macos`), on a
//! window GPUI makes `Transparent`. Its appearance is AppKit's own
//! `NSAppearanceCustomization`.

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::{App, Global, Hsla, Window, WindowBackgroundAppearance};

/// How much of the dark frame's color lies over the glass.
const DARK_TINT: f32 = 0.55;
/// The same for the light frame. Light glass is already pale, so the light
/// frame's color over it at the dark tint all but hides it.
const LIGHT_TINT: f32 = 0.3;

/// Present once a window has glass under it.
struct Glass {
    #[cfg(target_os = "macos")]
    view: objc2::rc::Retained<objc2_app_kit::NSGlassEffectView>,
}

impl Global for Glass {}

/// Put glass under `window`, if this system has it and wants it.
pub fn install(window: &mut Window, cx: &mut App) {
    #[cfg(target_os = "macos")]
    if let Some(view) = macos::install(window) {
        window.set_background_appearance(WindowBackgroundAppearance::Transparent);
        macos::set_dark(&view, cx.theme().is_dark());
        cx.set_global(Glass { view });
        cx.observe_global::<Theme>(|cx| {
            let dark = cx.theme().is_dark();
            macos::set_dark(&cx.global::<Glass>().view, dark);
        })
        .detach();
    }
}

/// The color of the window's frame: the theme's title bar, let through to
/// the glass where there is glass.
pub fn frame(cx: &App) -> Hsla {
    let theme = cx.theme();
    let color = theme.title_bar;
    if cx.has_global::<Glass>() {
        let tint = if theme.is_dark() {
            DARK_TINT
        } else {
            LIGHT_TINT
        };
        color.opacity(tint)
    } else {
        color
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use gpui_kit::Window;
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2::runtime::AnyClass;
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization as _, NSAppearanceNameAqua,
        NSAppearanceNameDarkAqua, NSAutoresizingMaskOptions, NSGlassEffectView, NSView,
        NSWindowOrderingMode, NSWorkspace,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// Adds a glass view filling the window, under GPUI's view. None when
    /// there is no glass to add.
    pub fn install(window: &Window) -> Option<Retained<NSGlassEffectView>> {
        let mtm = MainThreadMarker::new()?;
        if AnyClass::get(c"NSGlassEffectView").is_none()
            || NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()
        {
            return None;
        }
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return None;
        };
        // SAFETY: the handle's view is GPUI's, alive as long as the window,
        // and this is the main thread, where AppKit's views are used.
        let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
        let content = unsafe { view.superview() }?;
        let glass = NSGlassEffectView::initWithFrame(mtm.alloc(), content.bounds());
        glass.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        // The window's own corners round it.
        glass.setCornerRadius(0.);
        content.addSubview_positioned_relativeTo(&glass, NSWindowOrderingMode::Below, Some(view));
        Some(glass)
    }

    /// Give the glass the dark or the light appearance. Only the glass view
    /// takes it, so the window still reports the system's appearance, which
    /// Match System follows.
    pub fn set_dark(glass: &NSGlassEffectView, dark: bool) {
        // SAFETY: AppKit's appearance names are static strings it defines.
        let name = unsafe {
            if dark {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            }
        };
        glass.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
    }
}
