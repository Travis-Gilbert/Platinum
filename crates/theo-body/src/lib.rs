//! Theo's body without a renderer or a window.
//!
//! Extracted from Platinum PR #25 (`src/theo_desk/shape.rs` and `morph.rs`
//! at 4b4ce4b) so that every host of Theo, the GPUI desk and the Rune
//! application alike, moves the body by the same numbers. The value types
//! the GPUI host lent it (`mac::Rect`, `delegate::Playback`) are defined here.
pub mod morph;
pub mod shape;

/// A rectangle on a display, in points, y up, as AppKit lays out frames.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// What a surface's media session says it is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Playback {
    /// Nothing has said: no media, or a surface that never reported.
    #[default]
    None,
    Playing,
    Paused,
}
