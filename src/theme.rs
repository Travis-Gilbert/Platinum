//! The chrome's two themes: Asciinema, dark and the default, and GitHub
//! Sharp for light mode. Pages follow the chrome's mode (`page.rs`).
//!
//! The person picks an appearance: Light, Dark, or Match System, which
//! follows the system's setting as it changes.
//!
//! SOURCING: GPUI Kit's `ThemeRegistry` and `Theme` (including
//! `Theme::sync_system_appearance`); the theme files are in `themes/`,
//! translated to GPUI Kit's schema.

use gpui_kit::component::{Icon, Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Global, SharedString, Window};

use crate::icons;

const ASCIINEMA: &str = include_str!("../themes/asciinema.json");
const GITHUB_SHARP: &str = include_str!("../themes/github-sharp.json");

/// Which theme the chrome and pages use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    Light,
    #[default]
    Dark,
    /// Light or dark, as the system is.
    System,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::Light, Appearance::Dark, Appearance::System];

    pub fn label(self) -> &'static str {
        match self {
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
            Appearance::System => "Match System",
        }
    }

    /// As a command: "Light Appearance".
    pub fn command(self) -> SharedString {
        match self {
            Appearance::System => "Match System Appearance".into(),
            appearance => format!("{} Appearance", appearance.label()).into(),
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Appearance::Light => icons::light(),
            Appearance::Dark => icons::dark(),
            Appearance::System => icons::system(),
        }
    }
}

#[derive(Default)]
struct Chosen(Appearance);

impl Global for Chosen {}

pub fn init(cx: &mut App) {
    let registry = ThemeRegistry::global_mut(cx);
    for (name, source) in [("Asciinema", ASCIINEMA), ("GitHub Sharp", GITHUB_SHARP)] {
        if let Err(error) = registry.load_themes_from_str(source) {
            log::error!("theme {name}: {error:#}");
        }
    }
    let themes = ThemeRegistry::global(cx).themes();
    let dark = themes.get("Asciinema").cloned();
    let light = themes.get("GitHub Sharp").cloned();
    Theme::update(cx, |theme| {
        if let Some(dark) = dark {
            theme.dark_theme = dark;
        }
        if let Some(light) = light {
            theme.light_theme = light;
        }
    });
    cx.set_global(Chosen::default());
    Theme::change(ThemeMode::Dark, None, cx);
}

pub fn appearance(cx: &App) -> Appearance {
    cx.try_global::<Chosen>()
        .map_or_else(Appearance::default, |chosen| chosen.0)
}

pub fn set_appearance(appearance: Appearance, cx: &mut App) {
    cx.set_global(Chosen(appearance));
    match appearance {
        Appearance::Light => Theme::change(ThemeMode::Light, None, cx),
        Appearance::Dark => Theme::change(ThemeMode::Dark, None, cx),
        Appearance::System => Theme::sync_system_appearance(None, cx),
    }
}

/// The system's appearance changed under a window; Match System follows it.
pub fn system_changed(window: &mut Window, cx: &mut App) {
    if appearance(cx) == Appearance::System {
        Theme::sync_system_appearance(Some(window), cx);
    }
}
