//! Frame's own icons, drawn in the chrome's text color like GPUI Kit's: the
//! SVGs are masks, so their lighter fill (`fill-opacity`) gives each one a
//! duotone in the current theme.
//!
//! The Ask glyphs are Streamline's "design file AI"; the rest are redrawn
//! from the Streamline Ultimate Duotone set (`sd-card-download` from its CC BY
//! 4.0 free subset). See the README's credits.
//!
//! The canvas glyphs (`assets/icons/canvas-*.svg`) are Lucide shapes (ISC
//! licence, Copyright (c) Lucide Icons and Contributors, with Feather's MIT
//! notice for its portions), redrawn at the stroke width of the icons above
//! with a lighter fill added, so they read as one set with them.

use gpui_kit::component::Icon;

const ASK: &[u8] = include_bytes!("../assets/icons/ask.svg");
const ASK_ON: &[u8] = include_bytes!("../assets/icons/ask-on.svg");
const PAGE: &[u8] = include_bytes!("../assets/icons/page.svg");
const SHARE: &[u8] = include_bytes!("../assets/icons/share.svg");
// The supplied PNG is embedded unchanged; GPUI tints its alpha mask.
const IMPORT_BROWSER_DATA: &[u8] = include_bytes!("../assets/icons/import-browser-data.svg");
const DOWNLOADS: &[u8] = include_bytes!("../assets/icons/downloads.svg");
const LIGHT: &[u8] = include_bytes!("../assets/icons/theme-light.svg");
const DARK: &[u8] = include_bytes!("../assets/icons/theme-dark.svg");
const SYSTEM: &[u8] = include_bytes!("../assets/icons/theme-system.svg");
const CANVAS_SELECT: &[u8] = include_bytes!("../assets/icons/canvas-select.svg");
const CANVAS_PAN: &[u8] = include_bytes!("../assets/icons/canvas-pan.svg");
const CANVAS_LINK: &[u8] = include_bytes!("../assets/icons/canvas-link.svg");
const CANVAS_NOTE: &[u8] = include_bytes!("../assets/icons/canvas-note.svg");
const CANVAS_INSERT: &[u8] = include_bytes!("../assets/icons/canvas-insert.svg");
const CANVAS_FIND: &[u8] = include_bytes!("../assets/icons/canvas-find.svg");
const CANVAS_FIT_ALL: &[u8] = include_bytes!("../assets/icons/canvas-fit-all.svg");
const CANVAS_EXPAND: &[u8] = include_bytes!("../assets/icons/canvas-expand.svg");
const CANVAS_SPLIT: &[u8] = include_bytes!("../assets/icons/canvas-split.svg");
const CANVAS_GRID: &[u8] = include_bytes!("../assets/icons/canvas-grid.svg");
const CANVAS_PILE: &[u8] = include_bytes!("../assets/icons/canvas-pile.svg");
const CANVAS_ANNOTATE: &[u8] = include_bytes!("../assets/icons/canvas-annotate.svg");
const CANVAS_RELATED: &[u8] = include_bytes!("../assets/icons/canvas-related.svg");
const CANVAS_REMOVE: &[u8] = include_bytes!("../assets/icons/canvas-remove.svg");
const CANVAS_LAYERS: &[u8] = include_bytes!("../assets/icons/canvas-layers.svg");
const CANVAS_APP: &[u8] = include_bytes!("../assets/icons/canvas-app.svg");
const CANVAS_FILE_IMAGE: &[u8] = include_bytes!("../assets/icons/canvas-file-image.svg");

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

pub fn import_browser_data() -> Icon {
    Icon::default().data(IMPORT_BROWSER_DATA)
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

/// Select, the pointer tool.
pub fn canvas_select() -> Icon {
    Icon::default().data(CANVAS_SELECT)
}

/// Pan, the hand tool.
pub fn canvas_pan() -> Icon {
    Icon::default().data(CANVAS_PAN)
}

/// Link, the connector tool.
pub fn canvas_link() -> Icon {
    Icon::default().data(CANVAS_LINK)
}

/// Note, the tool and the page it places.
pub fn canvas_note() -> Icon {
    Icon::default().data(CANVAS_NOTE)
}

/// Insert, which opens its menu.
pub fn canvas_insert() -> Icon {
    Icon::default().data(CANVAS_INSERT)
}

/// Find, which takes the omnibar.
pub fn canvas_find() -> Icon {
    Icon::default().data(CANVAS_FIND)
}

/// Fit all: the four corners of the view.
pub fn canvas_fit_all() -> Icon {
    Icon::default().data(CANVAS_FIT_ALL)
}

/// Expand a focused card until its page fills the window.
pub fn canvas_expand() -> Icon {
    Icon::default().data(CANVAS_EXPAND)
}

/// Open the selection side by side.
pub fn canvas_split() -> Icon {
    Icon::default().data(CANVAS_SPLIT)
}

/// Arrange the selection as a grid.
pub fn canvas_grid() -> Icon {
    Icon::default().data(CANVAS_GRID)
}

/// Arrange the selection as a pile.
pub fn canvas_pile() -> Icon {
    Icon::default().data(CANVAS_PILE)
}

/// Annotate a card.
pub fn canvas_annotate() -> Icon {
    Icon::default().data(CANVAS_ANNOTATE)
}

/// Find related material.
pub fn canvas_related() -> Icon {
    Icon::default().data(CANVAS_RELATED)
}

/// Remove from the canvas.
pub fn canvas_remove() -> Icon {
    Icon::default().data(CANVAS_REMOVE)
}

/// Layers: the trail, semantic edges and annotations.
pub fn canvas_layers() -> Icon {
    Icon::default().data(CANVAS_LAYERS)
}

/// An app card, in the Insert menu.
pub fn canvas_app() -> Icon {
    Icon::default().data(CANVAS_APP)
}

/// A file or image card, in the Insert menu.
pub fn canvas_file_image() -> Icon {
    Icon::default().data(CANVAS_FILE_IMAGE)
}
