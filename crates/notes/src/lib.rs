//! The headless core of Frame's notes app: parses notes, resolves links and decides what the index contains.
mod authority;
mod canvas;
mod frontmatter;
mod ids;
mod index;
mod model;
mod offsets;
mod parse;
mod rename;
mod resolve;
mod settings;
mod vault;
mod wordcount;

pub use canvas::{CanvasLink, canvas_links};
pub use ids::{IdStore, is_note_id};
pub use index::{IndexedFile, IndexedLink, LinkMap, VaultIndex};
pub use model::*;
pub use offsets::{Offsets, Span};
pub use parse::parse as parse_note;
pub use rename::{DocumentEdits, RenamePlan, TextEdit, apply_text_edits};
pub use resolve::{
    FileIndex, FileKind, Target, dir, heading_fragment, heading_key, join_relative, locate,
    name_key, normalize, stem,
};
pub use settings::{LinkFormat, NewFileLocation, VaultSettings, is_hidden};
pub use theorem_ide_runtime::authority::{ReconcileConflictReason, ReconcileResult};
pub use vault::{Document, Vault};
