//! The headless core of Frame's notes app: parses notes, resolves links and decides what the index contains.
mod canvas;
mod frontmatter;
mod ids;
mod model;
mod offsets;
mod parse;
mod resolve;
mod settings;

pub use canvas::{CanvasLink, canvas_links};
pub use ids::{IdStore, is_note_id};
pub use model::*;
pub use offsets::{Offsets, Span};
pub use parse::parse as parse_note;
pub use resolve::{FileIndex, FileKind, Target, dir, heading_fragment, heading_key, join_relative, locate, name_key, normalize, stem};
pub use settings::{LinkFormat, NewFileLocation, VaultSettings, is_hidden};
