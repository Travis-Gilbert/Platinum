//! Every measure and colour of the editor tab, in one place.
//!
//! The values re-express the relationships of the Zephium notes app, from
//! `docs/plans/evidence/notes-zephium-design-2026-10-31.md` sections 3 to 5
//! (Zephium is MPL-2.0; nothing here is taken from its code). The app keeps its
//! own background: only the relations are borrowed, so surfaces step by a few
//! percent of white (ink in light) and state is luminance, not a hue.

use gpui_kit::{Font, Hsla, Pixels, font, px, rgb};
use zorite_editor::SyntaxStyle;

/// The text column, centred in the tab.
pub const COLUMN_MAX_WIDTH: f32 = 760.;
/// Side padding of the column; the narrow one applies in a tab narrower than
/// the column's own width.
pub const SIDE_PADDING: f32 = 48.;
pub const SIDE_PADDING_NARROW: f32 = 24.;
/// Padding under the header, above the title.
pub const TOP_PADDING: f32 = 20.;
/// Room under the last line, as a share of the tab's height, so the last line
/// can be scrolled to the middle.
pub const BOTTOM_SPACE: f32 = 0.4;
/// Body text. The line height is the editor's own (1.45); the brief asks for
/// 1.62, which is a change to `vendor/zorite-editor`, not made here.
pub const TEXT_SIZE: f32 = 15.;

/// The note's name above its text: 28px, weight 700, line height 1.2 and
/// 0.35em below.
pub const TITLE_SIZE: f32 = 28.;
pub const TITLE_LINE_HEIGHT: f32 = 1.2;
pub const TITLE_GAP: f32 = TITLE_SIZE * 0.35;

/// The slim header: 48px high with 12px at the sides, the name at 13px and
/// weight 600 in the middle, the state at 12px and faint on the left.
pub const HEADER_HEIGHT: f32 = 48.;
pub const HEADER_PADDING: f32 = 12.;
pub const NAME_SIZE: f32 = 13.;
pub const STATE_SIZE: f32 = 12.;
/// Faint text is the text colour at this opacity.
pub const FAINT: f32 = 0.44;

/// How far from the tab's edge the caret is kept when typing scrolls it into
/// view.
pub const CARET_MARGIN: Pixels = px(48.);

/// The colour of a state that needs a person: soft lemon for a conflict,
/// soft rose for a failure.
pub fn conflict(dark: bool) -> Hsla {
    pick(dark, 0xebdbaf, 0x927224)
}

pub fn failure(dark: bool) -> Hsla {
    pick(dark, 0xf3bcc9, 0xa74f64)
}

/// The live-preview colours of the markdown the editor draws.
pub fn syntax(dark: bool) -> SyntaxStyle {
    SyntaxStyle {
        block_label: None,
        block_label_gen: 0,
        block_ref_count: None,
        marker: pick(dark, 0x7a7a7c, 0x98989a),
        code: pick(dark, 0xf5e4e6, 0x372629),
        code_bg: pick(dark, 0x2c2c2f, 0xe0e0e2),
        link: pick(dark, 0xf4f4f6, 0x1c1c1f),
        tag: pick(dark, 0xb4d0f5, 0x4474b1),
        quote: pick(dark, 0xd8d8db, 0x55555c),
        // Sky, mint, lilac, lemon and rose, for note, tip, important, warning
        // and caution.
        alert_note: pick(dark, 0x8db9f2, 0x4f8fe0),
        alert_tip: pick(dark, 0x88d0b3, 0x2f9c77),
        alert_important: pick(dark, 0xb7a6f0, 0x7e67d6),
        alert_warning: pick(dark, 0xe6cb80, 0xb88c14),
        alert_caution: pick(dark, 0xee9aae, 0xd35d7a),
        alert_icons: None,
        rule: pick(dark, 0x38383a, 0xd5d5d7),
        mark_bg: pick(dark, 0x34312b, 0xeae4da),
        popover_bg: pick(dark, 0x3d3d44, 0xffffff),
        popover_border: pick(dark, 0x38383a, 0xd5d5d7),
        popover_fg: pick(dark, 0xf4f4f6, 0x1c1c1f),
        // A row under the pointer is the text colour at 7.5 percent.
        popover_hover: pick(dark, 0xf4f4f6, 0x1c1c1f).opacity(0.075),
        popover_divider: pick(dark, 0xf4f4f6, 0x1c1c1f).opacity(0.13),
        popover_danger: failure(dark),
        mono: mono(),
        property_icon: None,
    }
}

fn mono() -> Font {
    font("Menlo")
}

fn pick(dark: bool, dark_value: u32, light_value: u32) -> Hsla {
    rgb(if dark { dark_value } else { light_value }).into()
}
