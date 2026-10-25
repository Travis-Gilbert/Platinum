//! Frame's own icons, drawn in the chrome's text color like GPUI Kit's: the
//! SVGs are masks, so their lighter fill (`fill-opacity`) gives each one a
//! duotone in the current theme.
//!
//! The Ask glyphs are Streamline's "design file AI"; the rest are redrawn
//! from the Streamline Ultimate Duotone set (`sd-card-download` from its CC BY
//! 4.0 free subset). See the README's credits.

use gpui_kit::component::Icon;

const ASK: &[u8] = include_bytes!("../assets/icons/ask.svg");
const ASK_ON: &[u8] = include_bytes!("../assets/icons/ask-on.svg");
const PAGE: &[u8] = include_bytes!("../assets/icons/page.svg");
const SHARE: &[u8] = include_bytes!("../assets/icons/share.svg");
const DOWNLOADS: &[u8] = include_bytes!("../assets/icons/downloads.svg");
const LIGHT: &[u8] = include_bytes!("../assets/icons/theme-light.svg");
const DARK: &[u8] = include_bytes!("../assets/icons/theme-dark.svg");
const SYSTEM: &[u8] = include_bytes!("../assets/icons/theme-system.svg");

/// Ask, outlined: the address bar searches.
pub fn ask() -> Icon {
    Icon::default().data(ASK)
}

/// Ask, filled: the address bar asks.
pub fn ask_on() -> Icon {
    Icon::default().data(ASK_ON)
}

/// A blank page: new tabs, before they go anywhere.
pub fn page() -> Icon {
    Icon::default().data(PAGE)
}

pub fn share() -> Icon {
    Icon::default().data(SHARE)
}

pub fn downloads() -> Icon {
    Icon::default().data(DOWNLOADS)
}

pub fn light() -> Icon {
    Icon::default().data(LIGHT)
}

pub fn dark() -> Icon {
    Icon::default().data(DARK)
}

/// Light or dark, as the system is.
pub fn system() -> Icon {
    Icon::default().data(SYSTEM)
}
