//! Frame's own icons, drawn in the chrome's text color like GPUI Kit's: the
//! SVGs are masks, so their lighter fill (`fill-opacity`) gives each one a
//! duotone in the current theme.
//!
//! The Ask glyphs are Streamline's "design file AI". Home and the appearance
//! glyphs are redrawn from the Streamline Ultimate Duotone set, as are the
//! page, share and downloads glyphs (`sd-card-download` from its CC BY 4.0
//! free subset). The places' glyphs are redrawn from Streamline Ultimate
//! Light, on its 48-point grid; All apps is drawn to match them. See the
//! README's credits.

use gpui_kit::component::Icon;

const ASK: &[u8] = include_bytes!("../assets/icons/ask.svg");
const ASK_ON: &[u8] = include_bytes!("../assets/icons/ask-on.svg");
const PAGE: &[u8] = include_bytes!("../assets/icons/page.svg");
const SHARE: &[u8] = include_bytes!("../assets/icons/share.svg");
const DOWNLOADS: &[u8] = include_bytes!("../assets/icons/downloads.svg");
const LIGHT: &[u8] = include_bytes!("../assets/icons/theme-light.svg");
const DARK: &[u8] = include_bytes!("../assets/icons/theme-dark.svg");
const SYSTEM: &[u8] = include_bytes!("../assets/icons/theme-system.svg");
const HOME: &[u8] = include_bytes!("../assets/icons/home.svg");
const MAIL: &[u8] = include_bytes!("../assets/icons/mail.svg");
const MEDIA: &[u8] = include_bytes!("../assets/icons/media.svg");
const NOTES: &[u8] = include_bytes!("../assets/icons/notes.svg");
const TERMINAL: &[u8] = include_bytes!("../assets/icons/terminal.svg");
const AGENT: &[u8] = include_bytes!("../assets/icons/agent.svg");
const APPS: &[u8] = include_bytes!("../assets/icons/apps.svg");

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

pub fn home() -> Icon {
    Icon::default().data(HOME)
}

pub fn mail() -> Icon {
    Icon::default().data(MAIL)
}

/// The library: saved pages, media and files.
pub fn media() -> Icon {
    Icon::default().data(MEDIA)
}

pub fn notes() -> Icon {
    Icon::default().data(NOTES)
}

pub fn terminal() -> Icon {
    Icon::default().data(TERMINAL)
}

pub fn agent() -> Icon {
    Icon::default().data(AGENT)
}

pub fn apps() -> Icon {
    Icon::default().data(APPS)
}
