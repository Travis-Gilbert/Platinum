//! One native field: a note, a chat draft, a mail body, an annotation.
//!
//! The host owns the text and an editor; this owns what was last found in it.
//! The one rule is that a result describes a single state of the text. The
//! state is named by [`revision_of`], a digest of the text itself, so no host
//! has to remember to bump a counter and a result for yesterday's text cannot
//! be mistaken for today's.

use std::ops::Range;

use theorem_text_check::{CheckOutcome, CheckRequest, CheckStatus, FieldPolicy, RevisionId};

use crate::{markdown, service::Service};

/// The revision of `text`: its length and a 32-bit FNV-1a digest of its UTF-16
/// code units, as `"<units>:<hash>"`.
///
/// Web pages count in UTF-16 and cannot cheaply share a Rust value, so the
/// page-side script computes the same digest over the same units
/// (`probe.js`), and a test pins the two to each other. A digest cannot
/// collide with a *different* range's text unnoticed: every edit also checks
/// that the text it replaces is still there.
#[must_use]
pub fn revision_of(text: &str) -> RevisionId {
    let mut hash: u32 = 0x811c_9dc5;
    let mut units = 0usize;
    for unit in text.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(0x0100_0193);
        units += 1;
    }
    RevisionId(format!("{units}:{hash:08x}"))
}

/// Check a markdown note.
///
/// Only prose is judged (see [`markdown::prose_view`]); `selection` and
/// `composition` are byte ranges of `text`, which is what the editor reports.
/// Every diagnostic and edit in the result is verified to name the very bytes
/// of `text` it points at, so nothing in it can touch a character the checker
/// did not read.
#[must_use]
pub fn check_note(
    service: &Service,
    text: &str,
    selection: Option<Range<usize>>,
    composition: Option<Range<usize>>,
) -> CheckOutcome {
    check_note_in(service, text, selection, composition, &FieldPolicy::default())
}

/// [`check_note`] for a field with its own policy: a protected field, one the
/// editor has switched checking off for, or one in another language.
#[must_use]
pub fn check_note_in(
    service: &Service,
    text: &str,
    selection: Option<Range<usize>>,
    composition: Option<Range<usize>>,
    policy: &FieldPolicy,
) -> CheckOutcome {
    let mut request = CheckRequest::new(revision_of(text), markdown::prose_view(text));
    request.selection = selection;
    request.composition = composition;
    request.field = policy.clone();
    let mut outcome = service.check(&request);
    anchor(&mut outcome, text);
    outcome
}

/// Drop whatever does not name the same bytes in `source` as in the text that
/// was checked: a range spanning blanked markup, say.
fn anchor(outcome: &mut CheckOutcome, source: &str) {
    outcome
        .diagnostics
        .retain(|d| source.get(d.byte_range.clone()) == Some(d.text.as_str()));
    for diagnostic in &mut outcome.diagnostics {
        diagnostic
            .suggestions
            .retain(|s| source.get(s.edit.byte_range.clone()) == Some(s.edit.expected.as_str()));
    }
}

/// What was last found in one field.
#[derive(Debug, Default)]
pub struct Field {
    outcome: Option<CheckOutcome>,
}

impl Field {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep `outcome` if it describes `text` exactly as it is now. A check
    /// takes time; the person keeps typing; an answer that arrives for text
    /// that is gone is dropped here, never shown and never applied.
    pub fn accept(&mut self, outcome: CheckOutcome, text: &str) -> bool {
        if outcome.revision != revision_of(text) {
            return false;
        }
        self.outcome = Some(outcome);
        true
    }

    /// What was found, if it still describes `text`. Underlines are drawn from
    /// this, so a result that has gone stale draws nothing rather than
    /// underlining the wrong word.
    #[must_use]
    pub fn outcome_for(&self, text: &str) -> Option<&CheckOutcome> {
        self.outcome
            .as_ref()
            .filter(|outcome| outcome.revision == revision_of(text))
    }

    /// Whether the last check looked at the text at all.
    #[must_use]
    pub fn checked(&self) -> bool {
        self.outcome
            .as_ref()
            .is_some_and(|o| matches!(o.status, CheckStatus::Checked))
    }

    pub fn clear(&mut self) {
        self.outcome = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theorem_text_check::ProfileStore;

    fn service() -> (Service, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::open(ProfileStore::at(dir.path().join("profile.json"))).unwrap();
        (service, dir)
    }

    #[test]
    fn revision_is_content_and_changes_with_any_edit() {
        assert_eq!(revision_of("teh"), revision_of("teh"));
        assert_ne!(revision_of("teh"), revision_of("the"));
        assert_ne!(revision_of("a"), revision_of("a "));
        // Length is part of it: same hash input cannot hide a trailing change.
        assert!(revision_of("a\u{1f600}").0.starts_with("3:"));
    }

    #[test]
    fn revision_known_vectors_are_pinned() {
        // The page-side digest in probe.js must produce these same strings.
        assert_eq!(revision_of("").0, "0:811c9dc5");
        assert_eq!(revision_of("teh").0, "3:bb01f12e");
    }

    #[test]
    fn a_misspelling_in_prose_is_found_and_one_in_code_is_not() {
        let (service, _dir) = service();
        let text = "This has teh typo.\n\n```\nlet teh = 1;\n```\n";
        let outcome = check_note(&service, text, None, None);
        assert!(matches!(outcome.status, CheckStatus::Checked));
        let found: Vec<_> = outcome.diagnostics.iter().map(|d| (d.rule.as_str(), d.text.as_str())).collect();
        assert_eq!(found, [("spelling", "teh")], "{found:?}");
        let at = text.find("teh").unwrap();
        assert_eq!(outcome.diagnostics[0].byte_range, at..at + 3);
    }

    #[test]
    fn a_result_for_other_text_is_not_accepted_or_shown() {
        let (service, _dir) = service();
        let before = "This has teh typo.";
        let outcome = check_note(&service, before, None, None);
        let mut field = Field::new();
        assert!(field.accept(outcome.clone(), before));
        assert!(field.outcome_for(before).is_some());
        // The person typed: the old result describes nothing now.
        assert!(field.outcome_for("This has teh typo!").is_none());
        let mut other = Field::new();
        assert!(!other.accept(outcome, "This has the typo."));
        assert!(other.outcome_for("This has the typo.").is_none());
    }
}
