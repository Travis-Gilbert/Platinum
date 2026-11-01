//! Platinum's side of shared text checking (SPEC-PLATINUM-MASTER section 13).
//!
//! Theorem's `theorem-text-check` decides what is wrong with a piece of text.
//! This crate is what Platinum puts around it so that native notes and web
//! page fields can use it without either being able to corrupt text:
//!
//! * [`Service`]: one checker and one personal profile (words added, rules
//!   switched off), shared by every field.
//! * [`markdown`]: a note is markdown, so code, links, HTML and front matter
//!   are blanked before checking and nothing inside them is ever flagged.
//! * [`Menu`]: the rows a person sees for a flagged span, and what each does.
//!   A correction is refused unless the field is provably still what was
//!   checked.
//! * [`Field`]: the state of one native field: the last outcome, accepted only
//!   if it still describes the text.
//! * [`web`]: the protocol with an editable field in a page (`<input>`,
//!   `<textarea>`, `contenteditable`), which reads nothing it must not and
//!   re-verifies in the page before changing a character.
//!
//! No part of this crate rewrites text. It returns descriptions of edits that
//! the host performs through its own editor, so each is one ordinary undo step.

pub mod field;
pub mod markdown;
pub mod menu;
pub mod service;
pub mod web;

pub use field::{Field, check_note, check_note_in, revision_of};
pub use theorem_text_check::{
    CheckOutcome, CheckRequest, CheckStatus, CurrentText, FieldKind, FieldPolicy, PersonalProfile,
    ProfileStore, RevisionId,
};
pub use menu::{Action, Choice, Menu, Refusal, Row, nearest};
pub use service::{Service, install, shared};
