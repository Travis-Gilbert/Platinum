//! The headless core of Frame's notes app: parses notes, resolves links and decides what the index contains.
mod frontmatter;
mod model;
mod offsets;
mod parse;

pub use model::*;
pub use offsets::{Offsets, Span};
pub use parse::parse as parse_note;
