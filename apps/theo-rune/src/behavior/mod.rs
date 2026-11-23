//! Authored behavior: Luau source an author names and revises, run by the
//! session for as long as the session wants it, across turns and forms.
//!
//! The catalog is the record: every revision of every behavior with what
//! became of it. It is owned by the session alone; `store` persists what it
//! decides, and `host` runs what it says is active. A revision that fails to
//! start is kept, marked failed, and the one before it keeps running.

pub mod host;
pub mod run;
pub mod store;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The runtime every revision here targets, recorded with it for replay.
pub const RUNTIME: &str = "luau (rune-luau @ anomalyco/rune 63ad0209)";

/// A behavior's name: the author's choice, any lowercase words joined by
/// `-`, `_` or `.`, so it is also a file name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Name(String);

impl Name {
    pub fn parse(name: &str) -> Result<Name, String> {
        let valid = !name.is_empty()
            && name.len() <= 64
            && !name.starts_with('.')
            && name.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.')
            });
        if valid {
            Ok(Name(name.to_owned()))
        } else {
            Err(format!(
                "\"{name}\" is not a behavior name: use lowercase letters, digits, '-', '_' or '.'"
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Status {
    /// Written, not yet started.
    Pending,
    /// The one running.
    Active,
    /// Ran, then a later revision took over.
    Superseded,
    /// Did not start; the reason, as the runtime gave it.
    Failed(String),
    /// Stopped by its author; nothing of it runs.
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    pub number: u32,
    /// Who wrote it: `agent`, `person`, a file's path.
    pub by: String,
    pub runtime: String,
    pub status: Status,
    #[serde(skip)]
    pub source: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    behaviors: BTreeMap<Name, Vec<Revision>>,
}

impl Catalog {
    /// Records a new revision of `name` and returns its number.
    pub fn author(&mut self, name: &Name, source: &str, by: &str) -> u32 {
        let revisions = self.behaviors.entry(name.clone()).or_default();
        let number = revisions.last().map_or(1, |r| r.number + 1);
        revisions.push(Revision {
            number,
            by: by.to_owned(),
            runtime: RUNTIME.to_owned(),
            status: Status::Pending,
            source: source.to_owned(),
        });
        number
    }

    /// Records whether revision `number` started. Starting makes it the
    /// active one and the previous active one superseded; failing changes
    /// nothing else.
    pub fn outcome(&mut self, name: &Name, number: u32, started: Result<(), String>) {
        let Some(revisions) = self.behaviors.get_mut(name) else {
            return;
        };
        if started.is_ok() {
            for r in revisions.iter_mut().filter(|r| r.status == Status::Active) {
                r.status = Status::Superseded;
            }
        }
        if let Some(r) = revisions.iter_mut().find(|r| r.number == number) {
            r.status = match started {
                Ok(()) => Status::Active,
                Err(reason) => Status::Failed(reason),
            };
        }
    }

    pub fn stop(&mut self, name: &Name) {
        if let Some(r) = self.active_mut(name) {
            r.status = Status::Stopped;
        }
    }

    pub fn active(&self, name: &Name) -> Option<&Revision> {
        self.behaviors
            .get(name)?
            .iter()
            .rev()
            .find(|r| r.status == Status::Active)
    }

    fn active_mut(&mut self, name: &Name) -> Option<&mut Revision> {
        self.behaviors
            .get_mut(name)?
            .iter_mut()
            .rev()
            .find(|r| r.status == Status::Active)
    }

    pub fn revision(&self, name: &Name, number: u32) -> Option<&Revision> {
        self.behaviors
            .get(name)?
            .iter()
            .find(|r| r.number == number)
    }

    pub fn revisions(&self, name: &Name) -> &[Revision] {
        self.behaviors.get(name).map_or(&[], Vec::as_slice)
    }

    /// Every behavior with an active revision: what a restart starts again.
    pub fn running(&self) -> impl Iterator<Item = (&Name, &Revision)> {
        self.behaviors
            .iter()
            .filter_map(|(name, _)| self.active(name).map(|r| (name, r)))
    }

    pub fn names(&self) -> impl Iterator<Item = &Name> {
        self.behaviors.keys()
    }

    /// Puts back a behavior's history as the store read it.
    pub(crate) fn restore(&mut self, name: Name, revisions: Vec<Revision>) {
        self.behaviors.insert(name, revisions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wave() -> Name {
        Name::parse("wave").unwrap()
    }

    #[test]
    fn revising_supersedes_and_a_failed_revision_leaves_the_last_good_one_active() {
        let mut catalog = Catalog::default();
        let one = catalog.author(&wave(), "v1", "agent");
        catalog.outcome(&wave(), one, Ok(()));
        let two = catalog.author(&wave(), "v2", "agent");
        catalog.outcome(&wave(), two, Ok(()));
        let three = catalog.author(&wave(), "v3 (broken", "agent");
        catalog.outcome(&wave(), three, Err("syntax error".into()));

        assert_eq!((one, two, three), (1, 2, 3));
        assert_eq!(catalog.active(&wave()).unwrap().source, "v2");
        let statuses: Vec<_> = catalog
            .revisions(&wave())
            .iter()
            .map(|r| r.status.clone())
            .collect();
        assert_eq!(
            statuses,
            [
                Status::Superseded,
                Status::Active,
                Status::Failed("syntax error".into())
            ]
        );
        assert_eq!(catalog.running().count(), 1);
    }

    #[test]
    fn a_stopped_behavior_does_not_run_again() {
        let mut catalog = Catalog::default();
        let one = catalog.author(&wave(), "v1", "agent");
        catalog.outcome(&wave(), one, Ok(()));
        catalog.stop(&wave());
        assert!(catalog.active(&wave()).is_none());
        assert_eq!(catalog.running().count(), 0);
        assert_eq!(catalog.revisions(&wave()).len(), 1, "its history stays");
    }

    #[test]
    fn names_are_the_authors_but_must_be_file_names() {
        for good in ["wave", "lean-in", "tea.time", "v2_glow"] {
            assert!(Name::parse(good).is_ok(), "{good}");
        }
        for bad in ["", "Wave", "../x", ".hidden", "two words", "a/b"] {
            assert!(Name::parse(bad).is_err(), "{bad}");
        }
    }
}
