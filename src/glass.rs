//! Liquid Glass under the window's chrome. The window is transparent and an
//! AppKit glass view lies behind GPUI's one view, so wherever the chrome
//! leaves its frame clear, the glass, and the desktop through it, show: the
//! tab row, the frame around the cards, Home and the places. A page's toolbar
//! and its selected tab take the page's color as a wash over the glass, as
//! Frame's own panes take theirs, so the glass stays with the chrome while a
//! page is up. Pages and a results page stay opaque.
//!
//! It is AppKit's clear glass, the kind that lets what is behind it through
//! sharply enough to see; window-sized regular glass frosts it to a flat grey.
//! Clear glass needs dimming under text, so the glass leans to the theme's
//! frame color, as its own tint rather than under a layer of paint that would
//! flatten it, by as much as keeps Home's text readable over a white window or
//! a black one behind Frame. It takes the chrome's appearance, not the
//! system's: Frame in Light on a Dark system gets light glass.
//!
//! Without the glass (before macOS 26, or with Reduce Transparency or
//! Increase Contrast on) the window stays opaque and the frame keeps the
//! theme's solid color.
//!
//! SOURCING: AppKit's `NSGlassEffectView`, through `objc2-app-kit`, added
//! under GPUI's view the way GPUI adds its own blur view
//! (`set_background_appearance` with `Blurred`, in `gpui-pre-macos`), on a
//! window GPUI makes `Transparent`. Its tint is its own `tintColor`, its
//! appearance AppKit's `NSAppearanceCustomization`.

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::{App, Global, Hsla, Window, WindowBackgroundAppearance};

/// How strongly the dark glass leans to the frame's color: enough that the
/// theme's text stays readable with a white window behind.
const DARK_TINT: f32 = 0.65;
/// The same for light glass, with a black window behind.
const LIGHT_TINT: f32 = 0.6;
/// How much of the page's color lies over the frame on Frame's own surfaces
/// in a tab's card, and of a page's color over its toolbar.
const PANE: f32 = 0.5;

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
        macos::style(&view, cx.theme());
        cx.set_global(Glass { view });
        cx.observe_global::<Theme>(|cx| macos::style(&cx.global::<Glass>().view, cx.theme()))
            .detach();
    }
}

/// The color of the window's frame: the theme's title bar, or nothing where
/// the glass, tinted to it, shows instead.
pub fn frame(cx: &App) -> Hsla {
    if cx.has_global::<Glass>() {
        gpui_kit::transparent_black()
    } else {
        cx.theme().title_bar
    }
}

/// The fill of Frame's own surfaces in a tab's card, Home and the places:
/// the page's color, half let through, over the frame. They read as part of
/// the chrome, glass and all, not as a page, while the selected tab still
/// rises out of a color of its own.
pub fn pane(cx: &App) -> Hsla {
    cx.theme().background.opacity(PANE)
}

/// A page's color on its toolbar and selected tab: half let through, like a
/// pane, over the glass; the color itself where there is no glass.
pub fn wash(color: Hsla, cx: &App) -> Hsla {
    if cx.has_global::<Glass>() {
        color.opacity(PANE)
    } else {
        color
    }
}

/// What a fill over the frame comes to, for the color of text on it. The
/// glass is tinted to the frame's color enough to stand for it here.
pub fn seen(fill: Hsla, cx: &App) -> Hsla {
    cx.theme().title_bar.blend(fill)
}

#[cfg(target_os = "macos")]
mod macos {
    use gpui_kit::component::Theme;
    use gpui_kit::{Rgba, Window};
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2::runtime::AnyClass;
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization as _, NSAppearanceNameAqua,
        NSAppearanceNameDarkAqua, NSAutoresizingMaskOptions, NSColor, NSGlassEffectView,
        NSGlassEffectViewStyle, NSView, NSWindowOrderingMode, NSWorkspace,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// Adds a glass view filling the window, under GPUI's view. None when
    /// there is no glass to add.
    pub fn install(window: &Window) -> Option<Retained<NSGlassEffectView>> {
        let mtm = MainThreadMarker::new()?;
        let workspace = NSWorkspace::sharedWorkspace();
        if AnyClass::get(c"NSGlassEffectView").is_none()
            || workspace.accessibilityDisplayShouldReduceTransparency()
            || workspace.accessibilityDisplayShouldIncreaseContrast()
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
        // Only its content view is sure to be inside the glass; GPUI's view
        // stays above it, where AppKit keeps the window's own views.
        glass.setContentView(Some(&NSView::new(mtm)));
        glass.setStyle(NSGlassEffectViewStyle::Clear);
        // The window's own corners round it.
        glass.setCornerRadius(0.);
        content.addSubview_positioned_relativeTo(&glass, NSWindowOrderingMode::Below, Some(view));
        Some(glass)
    }

    /// Give the glass the theme's appearance and lean it to the frame's
    /// color. Only the glass view takes the appearance, so the window still
    /// reports the system's, which Match System follows.
    pub fn style(glass: &NSGlassEffectView, theme: &Theme) {
        // SAFETY: AppKit's appearance names are static strings it defines.
        let name = unsafe {
            if theme.is_dark() {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            }
        };
        glass.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
        let Rgba { r, g, b, .. } = theme.title_bar.to_rgb();
        let alpha = if theme.is_dark() {
            super::DARK_TINT
        } else {
            super::LIGHT_TINT
        };
        let tint =
            NSColor::colorWithSRGBRed_green_blue_alpha(r.into(), g.into(), b.into(), alpha.into());
        glass.setTintColor(Some(&tint));
    }
}
