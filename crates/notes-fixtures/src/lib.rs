//! Test corpus and Obsidian reference harness for the notes app.
pub mod generate;
pub mod launch;
mod normalize;
pub mod obsidian;
pub mod reference;

/// The one Obsidian version the reference is recorded against.
pub const OBSIDIAN_VERSION: &str = "1.14.5";
