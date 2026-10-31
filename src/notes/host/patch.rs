//! What the reading page shows, and the patches that bring it up to date.
//!
//! The authority owns a note's text and its revision. The page shows a
//! derived rendering of it, and this tracks which one: the revision and
//! digest the page last confirmed. A patch is sent only when the new
//! rendering differs from what the page shows and is not older than it, one
//! patch is in flight at a time, and the answer to a patch or a render that
//! belongs to a page load that has since been replaced is dropped. The page
//! applies the same rule on its side (`reading.js`): it drops a patch older
//! than the revision it shows.
//!
//! Callout folds are view state the host keeps for the note, not part of the
//! document: the page reports each fold with a sequence number, and a patch
//! carries the folds back with the highest number the host has counted.

use std::collections::BTreeMap;

use serde::Serialize;

use super::ops::Rendered;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Shown {
    revision: u64,
    digest: String,
}

/// A note's callout folds, by the page's key for each callout.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folds {
    collapsed: BTreeMap<String, bool>,
    /// The highest fold the page numbered that has been counted.
    ack: u64,
}

impl Folds {
    /// Counts a fold the page reported. A fold numbered no higher than one
    /// already counted is a repeat and changes nothing.
    pub fn record(&mut self, key: String, collapsed: bool, seq: u64) -> bool {
        if seq <= self.ack {
            return false;
        }
        self.ack = seq;
        self.collapsed.insert(key, collapsed);
        true
    }

    pub fn is_empty(&self) -> bool {
        self.collapsed.is_empty()
    }

    /// A page is loading anew and numbers its folds from the start again.
    fn new_page(&mut self) {
        self.ack = 0;
    }

    /// Puts remembered folds on a page that has just loaded.
    pub fn restore_script(&self) -> Option<String> {
        (!self.is_empty()).then(|| {
            format!(
                "frameNotes.restore({})",
                serde_json::json!({ "folds": self.collapsed, "foldAck": self.ack })
            )
        })
    }
}

/// A rendering for the page to morph into.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Patch {
    pub revision: u64,
    pub digest: String,
    pub html: String,
    pub cssclasses: Vec<String>,
    pub folds: BTreeMap<String, bool>,
    #[serde(rename = "foldAck")]
    pub fold_ack: u64,
}

impl Patch {
    /// The script that applies it. Its value is whether the page applied it.
    pub fn script(&self) -> String {
        format!(
            "frameNotes.patch({})",
            serde_json::to_string(self).expect("a patch is plain data")
        )
    }
}

#[derive(Debug)]
enum Phase {
    Idle,
    Rendering,
    Sending(Shown),
}

/// What to do with a rendering.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Send(Patch),
    /// The page has it, or has something newer. `again` says another refresh
    /// was wanted meanwhile.
    Skip { again: bool },
    /// The page load this was rendered for is gone.
    Stale,
}

/// One page load's view of the host's side of the conversation.
#[derive(Debug)]
pub struct Tracker {
    /// Numbers the page loads, so late answers for an earlier one are dropped.
    epoch: u64,
    shown: Option<Shown>,
    phase: Phase,
    again: bool,
    folds: Folds,
}

impl Tracker {
    pub fn new(folds: Folds) -> Self {
        Self {
            epoch: 0,
            shown: None,
            phase: Phase::Idle,
            again: false,
            folds,
        }
    }

    /// The page is loading anew: nothing is known about what it shows until
    /// it says so.
    pub fn reset(&mut self) {
        self.epoch += 1;
        self.shown = None;
        self.phase = Phase::Idle;
        self.again = false;
        self.folds.new_page();
    }

    /// Another note's folds take the place of the current note's, which are
    /// returned to be kept. The page must be loading anew.
    pub fn swap_folds(&mut self, mut folds: Folds) -> Folds {
        folds.new_page();
        std::mem::replace(&mut self.folds, folds)
    }

    /// The page may not be showing what it said it shows (a change it made
    /// was refused): the next refresh sends its rendering even if it is the
    /// one the page confirmed.
    pub fn distrust(&mut self) {
        if let Some(shown) = &mut self.shown {
            shown.digest.clear();
        }
    }

    /// The page's script is running and shows this rendering.
    pub fn ready(&mut self, revision: u64, digest: String) {
        self.shown = Some(Shown { revision, digest });
    }

    pub fn shows(&self) -> Option<u64> {
        self.shown.as_ref().map(|shown| shown.revision)
    }

    pub fn folds(&self) -> &Folds {
        &self.folds
    }

    pub fn fold(&mut self, key: String, collapsed: bool, seq: u64) -> bool {
        self.folds.record(key, collapsed, seq)
    }

    /// A refresh is wanted. Returns the ticket to render under, or `None`
    /// when the page is not ready (its `ready` will ask again) or a refresh
    /// is already under way (which then runs once more when it ends).
    pub fn begin(&mut self) -> Option<u64> {
        if self.shown.is_none() {
            return None;
        }
        match self.phase {
            Phase::Idle => {
                self.phase = Phase::Rendering;
                Some(self.epoch)
            }
            Phase::Rendering | Phase::Sending(_) => {
                self.again = true;
                None
            }
        }
    }

    /// Decides what `rendered`, the answer to a refresh begun under `ticket`,
    /// asks of the page. `None` is a render that failed.
    pub fn decide(&mut self, ticket: u64, rendered: Option<&Rendered>) -> Decision {
        if ticket != self.epoch || !matches!(self.phase, Phase::Rendering) {
            return Decision::Stale;
        }
        let (Some(shown), Some(rendered)) = (&self.shown, rendered) else {
            return self.settle();
        };
        let newer = rendered.revision > shown.revision;
        let different = rendered.revision == shown.revision && rendered.digest != shown.digest;
        if !newer && !different {
            return self.settle();
        }
        self.phase = Phase::Sending(Shown {
            revision: rendered.revision,
            digest: rendered.digest.clone(),
        });
        Decision::Send(Patch {
            revision: rendered.revision,
            digest: rendered.digest.clone(),
            html: rendered.html.clone(),
            cssclasses: rendered.cssclasses.clone(),
            folds: self.folds.collapsed.clone(),
            fold_ack: self.folds.ack,
        })
    }

    fn settle(&mut self) -> Decision {
        self.phase = Phase::Idle;
        Decision::Skip {
            again: std::mem::take(&mut self.again),
        }
    }

    /// The page answered a patch. Returns whether another refresh was wanted
    /// meanwhile. A patch the page did not apply leaves what is shown as it
    /// was, so the next refresh sends it again.
    pub fn applied(&mut self, ticket: u64, applied: bool) -> bool {
        if ticket != self.epoch {
            return false;
        }
        if let Phase::Sending(sent) = std::mem::replace(&mut self.phase, Phase::Idle) {
            if applied {
                self.shown = Some(sent);
            }
        }
        std::mem::take(&mut self.again)
    }

    pub fn ticket(&self) -> u64 {
        self.epoch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(revision: u64, html: &str) -> Rendered {
        Rendered {
            revision,
            html: html.into(),
            cssclasses: vec!["wide".into()],
            digest: format!("d-{html}"),
        }
    }

    fn ready(revision: u64, html: &str) -> Tracker {
        let mut tracker = Tracker::new(Folds::default());
        tracker.ready(revision, rendered(revision, html).digest);
        tracker
    }

    fn sent(decision: Decision) -> Patch {
        match decision {
            Decision::Send(patch) => patch,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nothing_is_sent_until_the_page_says_what_it_shows() {
        let mut tracker = Tracker::new(Folds::default());
        assert_eq!(tracker.begin(), None);
        tracker.ready(3, "d".into());
        assert!(tracker.begin().is_some());
    }

    #[test]
    fn a_newer_rendering_is_sent_and_confirmed() {
        let mut tracker = ready(1, "a");
        let ticket = tracker.begin().unwrap();
        let patch = sent(tracker.decide(ticket, Some(&rendered(2, "b"))));
        assert_eq!((patch.revision, patch.html.as_str()), (2, "b"));
        assert_eq!(patch.cssclasses, ["wide"]);
        assert!(!tracker.applied(ticket, true));
        assert_eq!(tracker.shows(), Some(2));
    }

    #[test]
    fn what_the_page_has_or_what_is_older_is_not_sent() {
        let mut tracker = ready(5, "a");
        for same_or_older in [rendered(5, "a"), rendered(4, "z"), rendered(1, "a")] {
            let ticket = tracker.begin().unwrap();
            assert_eq!(
                tracker.decide(ticket, Some(&same_or_older)),
                Decision::Skip { again: false }
            );
        }
        assert_eq!(tracker.shows(), Some(5));
    }

    #[test]
    fn another_rendering_at_the_same_revision_is_sent() {
        // The text is the same; how it renders changed (a setting).
        let mut tracker = ready(5, "a");
        let ticket = tracker.begin().unwrap();
        assert!(matches!(
            tracker.decide(ticket, Some(&rendered(5, "b"))),
            Decision::Send(_)
        ));
    }

    #[test]
    fn one_refresh_runs_at_a_time_and_a_wanted_one_runs_after() {
        let mut tracker = ready(1, "a");
        let ticket = tracker.begin().unwrap();
        // Wanted while rendering, and again while sending.
        assert_eq!(tracker.begin(), None);
        sent(tracker.decide(ticket, Some(&rendered(2, "b"))));
        assert_eq!(tracker.begin(), None);
        assert!(tracker.applied(ticket, true));
        assert!(tracker.begin().is_some());
        // A refresh that finds nothing to send also reports what was wanted.
        let mut tracker = ready(1, "a");
        let ticket = tracker.begin().unwrap();
        assert_eq!(tracker.begin(), None);
        assert_eq!(
            tracker.decide(ticket, Some(&rendered(1, "a"))),
            Decision::Skip { again: true }
        );
        assert!(tracker.begin().is_some());
    }

    #[test]
    fn a_patch_the_page_did_not_apply_is_sent_again() {
        let mut tracker = ready(1, "a");
        let ticket = tracker.begin().unwrap();
        sent(tracker.decide(ticket, Some(&rendered(2, "b"))));
        tracker.applied(ticket, false);
        assert_eq!(tracker.shows(), Some(1));
        let ticket = tracker.begin().unwrap();
        assert!(matches!(
            tracker.decide(ticket, Some(&rendered(2, "b"))),
            Decision::Send(_)
        ));
    }

    #[test]
    fn a_failed_render_changes_nothing() {
        let mut tracker = ready(1, "a");
        let ticket = tracker.begin().unwrap();
        assert_eq!(tracker.decide(ticket, None), Decision::Skip { again: false });
        assert!(tracker.begin().is_some());
    }

    #[test]
    fn answers_for_an_earlier_page_load_are_dropped() {
        let mut tracker = ready(1, "a");
        let ticket = tracker.begin().unwrap();
        tracker.reset();
        tracker.ready(9, "d-z".into());
        assert_eq!(
            tracker.decide(ticket, Some(&rendered(2, "b"))),
            Decision::Stale
        );
        // A patch in flight for the earlier load does not confirm anything.
        let mut tracker = ready(1, "a");
        let ticket = tracker.begin().unwrap();
        sent(tracker.decide(ticket, Some(&rendered(2, "b"))));
        tracker.reset();
        tracker.ready(1, "d-a".into());
        assert!(!tracker.applied(ticket, true));
        assert_eq!(tracker.shows(), Some(1));
        assert!(tracker.begin().is_some());
    }

    #[test]
    fn folds_are_counted_once_and_travel_with_the_patch() {
        let mut tracker = ready(1, "a");
        assert!(tracker.fold("note|Title#0".into(), true, 1));
        assert!(!tracker.fold("note|Title#0".into(), false, 1));
        assert!(tracker.fold("tip|Other#0".into(), false, 2));
        let ticket = tracker.begin().unwrap();
        let patch = sent(tracker.decide(ticket, Some(&rendered(2, "b"))));
        assert_eq!(patch.fold_ack, 2);
        assert_eq!(patch.folds["note|Title#0"], true);
        assert_eq!(patch.folds["tip|Other#0"], false);
    }

    #[test]
    fn a_fresh_page_numbers_its_folds_from_one_again() {
        let mut tracker = ready(1, "a");
        assert!(tracker.fold("k".into(), true, 1));
        assert!(tracker.fold("k".into(), false, 2));
        tracker.reset();
        assert!(tracker.fold("k".into(), true, 1));
        assert_eq!(tracker.folds().restore_script().unwrap().matches("\"foldAck\":1").count(), 1);
    }

    #[test]
    fn another_notes_folds_replace_the_current_ones_and_are_returned() {
        let mut tracker = ready(1, "a");
        tracker.fold("k".into(), true, 3);
        let mut other = Folds::default();
        other.record("o".into(), true, 9);
        tracker.reset();
        let kept = tracker.swap_folds(other);
        assert!(!kept.is_empty());
        assert!(tracker.folds().restore_script().unwrap().contains("\"foldAck\":0"));
        assert!(tracker.fold("o".into(), false, 1));
    }

    #[test]
    fn a_distrusted_page_is_sent_what_it_already_confirmed() {
        let mut tracker = ready(4, "a");
        tracker.distrust();
        let ticket = tracker.begin().unwrap();
        let patch = sent(tracker.decide(ticket, Some(&rendered(4, "a"))));
        assert_eq!(patch.revision, 4);
        assert!(!tracker.applied(ticket, true));
        // Once it applies the patch it is trusted again.
        let ticket = tracker.begin().unwrap();
        assert_eq!(
            tracker.decide(ticket, Some(&rendered(4, "a"))),
            Decision::Skip { again: false }
        );
    }

    #[test]
    fn remembered_folds_are_restored_on_a_fresh_page() {
        let mut folds = Folds::default();
        assert_eq!(folds.restore_script(), None);
        folds.record("note|T#0".into(), true, 4);
        let script = folds.restore_script().unwrap();
        let json = script
            .strip_prefix("frameNotes.restore(")
            .and_then(|rest| rest.strip_suffix(')'))
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"foldAck": 4, "folds": {"note|T#0": true}})
        );
    }

    #[test]
    fn the_script_is_one_call_with_the_patch_as_json() {
        let mut tracker = ready(1, "a");
        let ticket = tracker.begin().unwrap();
        let patch = sent(tracker.decide(ticket, Some(&rendered(2, "<p>\"hi\"</p>\n"))));
        let script = patch.script();
        let json = script
            .strip_prefix("frameNotes.patch(")
            .and_then(|rest| rest.strip_suffix(')'))
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(value["revision"], 2);
        assert_eq!(value["html"], "<p>\"hi\"</p>\n");
        assert_eq!(value["foldAck"], 0);
        assert!(value["folds"].as_object().unwrap().is_empty());
    }
}
