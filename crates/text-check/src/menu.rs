//! What a person can do about one flagged span, and what each choice is
//! allowed to do.
//!
//! A [`Menu`] is built when the person asks (a right-click on a flagged word),
//! from a result for the text as it was. By the time they choose, the text may
//! have changed. [`Menu::resolve`] therefore takes the field as it is *now*
//! and returns an [`Action`] only if the edit is provably still the one that
//! was offered: same revision, same text under the range, no composition in
//! progress. Nothing here ever changes text; an [`Action::Edit`] is a
//! description the host performs through its own editor, so it is one
//! ordinary undo step.

use std::ops::Range;

use theorem_text_check::{
    CheckOutcome, CurrentText, EditRefusal, OffsetUnit, ProfileError, ProposedEdit, RevisionId,
    SPELLING_RULE, Suggestion, TextDiagnostic, rules, validate_edit,
};

use crate::service::Service;

/// How many replacements a menu offers. The dictionary can offer dozens; a
/// menu of dozens is not a menu.
pub const MAX_REPLACEMENTS: usize = 6;

/// What a row does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Choice {
    /// Replace the span with suggestion `n`.
    Replace(usize),
    /// Teach the personal dictionary this word.
    AddWord,
    /// Stop reporting this kind of problem.
    IgnoreRule,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    pub label: String,
    pub choice: Choice,
}

/// What a chosen row has the host do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    /// Perform this edit through the field's own editor.
    Edit(ProposedEdit),
    AddWord(String),
    IgnoreRule(String),
}

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum Refusal {
    /// The field is no longer what the menu was built from.
    #[error("{0}")]
    Edit(#[from] EditRefusal),
    /// Not a row of this menu.
    #[error("the menu has no row {0}")]
    NoSuchRow(usize),
}

/// The menu for one flagged span.
#[derive(Clone, Debug)]
pub struct Menu {
    revision: RevisionId,
    diagnostic: TextDiagnostic,
    suggestions: Vec<Suggestion>,
    rows: Vec<Row>,
}

impl Menu {
    /// The menu for diagnostic `id` of `outcome`, with the dictionary's
    /// replacements for a misspelling (computed now, for this one word).
    pub fn new(service: &Service, outcome: &CheckOutcome, id: &str) -> Result<Self, EditRefusal> {
        let mut suggestions = service.suggestions(outcome, id)?;
        suggestions.truncate(MAX_REPLACEMENTS);
        let diagnostic = outcome
            .diagnostics
            .iter()
            .find(|candidate| candidate.id == id)
            .cloned()
            .ok_or_else(|| EditRefusal::UnknownDiagnostic(id.to_owned()))?;

        let mut rows: Vec<Row> = suggestions
            .iter()
            .enumerate()
            .map(|(index, suggestion)| Row {
                label: suggestion.label.clone(),
                choice: Choice::Replace(index),
            })
            .collect();
        if diagnostic.rule == SPELLING_RULE {
            rows.push(Row {
                label: "Add to Dictionary".to_owned(),
                choice: Choice::AddWord,
            });
        }
        let summary = rules()
            .into_iter()
            .find(|rule| rule.id == diagnostic.rule)
            .map_or_else(|| diagnostic.rule.clone(), |rule| rule.summary);
        rows.push(Row {
            label: format!("Ignore: {summary}"),
            choice: Choice::IgnoreRule,
        });
        Ok(Self {
            revision: outcome.revision.clone(),
            diagnostic,
            suggestions,
            rows,
        })
    }

    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// What is wrong, in a sentence.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.diagnostic.message
    }

    /// The flagged text.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.diagnostic.text
    }

    #[must_use]
    pub fn byte_range(&self) -> Range<usize> {
        self.diagnostic.byte_range.clone()
    }

    #[must_use]
    pub fn utf16_range(&self) -> Range<usize> {
        self.diagnostic.utf16_range.clone()
    }

    #[must_use]
    pub fn rule(&self) -> &str {
        &self.diagnostic.rule
    }

    /// The revision of the text this menu was built from.
    #[must_use]
    pub fn revision(&self) -> &RevisionId {
        &self.revision
    }

    /// What choosing row `index` does, given the field as it is *now*.
    ///
    /// A replacement is returned only if [`validate_edit`] still holds: no
    /// composition in progress, the revision unchanged, and the flagged text
    /// still exactly where it was. Anything else is a [`Refusal`] and the
    /// field is left alone. Adding a word or ignoring a rule changes no text,
    /// so it needs no such proof.
    pub fn resolve(&self, index: usize, now: &CurrentText<'_>) -> Result<Action, Refusal> {
        let row = self.rows.get(index).ok_or(Refusal::NoSuchRow(index))?;
        match row.choice {
            Choice::Replace(n) => {
                let edit = &self.suggestions[n].edit;
                validate_edit(edit, now)?;
                Ok(Action::Edit(edit.clone()))
            }
            Choice::AddWord => Ok(Action::AddWord(self.diagnostic.text.clone())),
            Choice::IgnoreRule => Ok(Action::IgnoreRule(self.diagnostic.rule.clone())),
        }
    }
}

impl Action {
    /// Carry out the part that is not an edit: a word for the personal
    /// dictionary, a rule switched off. An edit is handed back for the host to
    /// perform through its editor.
    pub fn run(self, service: &Service) -> Result<Option<ProposedEdit>, ProfileError> {
        match self {
            Self::Edit(edit) => Ok(Some(edit)),
            Self::AddWord(word) => service.add_word(&word).map(|()| None),
            Self::IgnoreRule(rule) => service.suppress_rule(&rule).map(|()| None),
        }
    }
}

/// The diagnostic a menu should be built for: one the caret or selection
/// touches, else the one nearest to it. `caret` is counted in `unit`.
#[must_use]
pub fn nearest<'a>(
    outcome: &'a CheckOutcome,
    caret: &Range<usize>,
    unit: OffsetUnit,
) -> Option<&'a TextDiagnostic> {
    outcome.diagnostics.iter().min_by_key(|diagnostic| {
        let span = match unit {
            OffsetUnit::Utf8 => &diagnostic.byte_range,
            OffsetUnit::Utf16 => &diagnostic.utf16_range,
        };
        if caret.end < span.start {
            span.start - caret.end
        } else {
            caret.start.saturating_sub(span.end)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{check_note, revision_of};
    use theorem_text_check::{CheckStatus, ProfileStore};

    fn service() -> (Service, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::open(ProfileStore::at(dir.path().join("profile.json"))).unwrap();
        (service, dir)
    }

    fn now<'a>(revision: &'a RevisionId, text: &'a str) -> CurrentText<'a> {
        CurrentText {
            revision,
            text,
            composing: false,
        }
    }

    #[test]
    fn a_spelling_menu_offers_replacements_a_word_and_a_rule() {
        let (service, _dir) = service();
        let text = "This has teh typo.";
        let outcome = check_note(&service, text, None, None);
        let id = outcome.diagnostics[0].id.clone();
        let menu = Menu::new(&service, &outcome, &id).unwrap();
        let labels: Vec<_> = menu.rows().iter().map(|r| r.label.as_str()).collect();
        assert!(labels.contains(&"the"), "{labels:?}");
        assert!(labels.contains(&"Add to Dictionary"));
        assert!(labels.last().unwrap().starts_with("Ignore: "));
        assert!(menu.rows().len() <= MAX_REPLACEMENTS + 2);
        assert_eq!(menu.subject(), "teh");
    }

    #[test]
    fn choosing_a_replacement_returns_an_edit_only_while_the_field_is_unchanged() {
        let (service, _dir) = service();
        let text = "This has teh typo.";
        let outcome = check_note(&service, text, None, None);
        let menu = Menu::new(&service, &outcome, &outcome.diagnostics[0].id).unwrap();
        let the = menu.rows().iter().position(|r| r.label == "the").unwrap();
        let revision = revision_of(text);

        let Action::Edit(edit) = menu.resolve(the, &now(&revision, text)).unwrap() else {
            panic!("a replacement resolves to an edit");
        };
        assert_eq!((edit.expected.as_str(), edit.replacement.as_str()), ("teh", "the"));

        // The person typed before choosing: refused, nothing to apply.
        let typed = "Oh. This has teh typo.";
        let moved = revision_of(typed);
        assert!(matches!(
            menu.resolve(the, &now(&moved, typed)),
            Err(Refusal::Edit(EditRefusal::StaleRevision { .. }))
        ));
        // The revision was reused for different text: the text has the last word.
        let swapped = "This has foo typo.";
        assert!(matches!(
            menu.resolve(the, &now(&revision, swapped)),
            Err(Refusal::Edit(EditRefusal::TextChanged { .. }))
        ));
        // An input-method composition is active.
        let composing = CurrentText { composing: true, ..now(&revision, text) };
        assert!(matches!(
            menu.resolve(the, &composing),
            Err(Refusal::Edit(EditRefusal::CompositionActive))
        ));
        assert!(matches!(menu.resolve(99, &now(&revision, text)), Err(Refusal::NoSuchRow(99))));
    }

    #[test]
    fn adding_a_word_persists_and_silences_it_for_the_next_check() {
        let (service, dir) = service();
        let text = "The frobnicator works.";
        let outcome = check_note(&service, text, None, None);
        assert_eq!(outcome.diagnostics.len(), 1, "{:?}", outcome.diagnostics);
        let menu = Menu::new(&service, &outcome, &outcome.diagnostics[0].id).unwrap();
        let add = menu.rows().iter().position(|r| r.choice == Choice::AddWord).unwrap();
        let revision = revision_of(text);
        let action = menu.resolve(add, &now(&revision, text)).unwrap();
        assert_eq!(action.run(&service).unwrap(), None);

        let again = check_note(&service, text, None, None);
        assert!(matches!(again.status, CheckStatus::Checked));
        assert!(again.diagnostics.is_empty(), "{:?}", again.diagnostics);

        // A new process opening the same file still knows the word.
        let reopened = Service::open(ProfileStore::at(dir.path().join("profile.json"))).unwrap();
        assert!(check_note(&reopened, text, None, None).diagnostics.is_empty());
    }

    #[test]
    fn ignoring_a_rule_switches_that_rule_off() {
        let (service, _dir) = service();
        let text = "This has teh typo.";
        let outcome = check_note(&service, text, None, None);
        let menu = Menu::new(&service, &outcome, &outcome.diagnostics[0].id).unwrap();
        let ignore = menu.rows().iter().position(|r| r.choice == Choice::IgnoreRule).unwrap();
        let revision = revision_of(text);
        menu.resolve(ignore, &now(&revision, text)).unwrap().run(&service).unwrap();
        assert!(check_note(&service, text, None, None).diagnostics.is_empty());
    }

    #[test]
    fn nearest_prefers_the_span_the_caret_touches() {
        let (service, _dir) = service();
        let text = "teh first and then teh second";
        let outcome = check_note(&service, text, None, None);
        assert_eq!(outcome.diagnostics.len(), 2);
        let second = text.rfind("teh").unwrap();
        let hit = nearest(&outcome, &(second + 1..second + 1), OffsetUnit::Utf8).unwrap();
        assert_eq!(hit.byte_range.start, second);
        let hit = nearest(&outcome, &(0..0), OffsetUnit::Utf8).unwrap();
        assert_eq!(hit.byte_range.start, 0);
    }
}
