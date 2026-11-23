//! What Theo learns from the Theorem driver: the selected conversation's run
//! as an activity, the face's liveness, and the manner the agent wrote.
//!
//! The driver's projection is the source of truth; this only follows it.
//! Each conversation's runs are counted here so that a second run in the same
//! conversation is a later generation, and a stop for the first cannot end
//! the second. Liveness follows web Theo's `livenessOf`: asking, then
//! working, then a short hold on done, then attending, then idle.

use presence::{Activity, Phase};
use theo::engine::{Expression, Liveness};
use theorem_surface_contracts::thread::{ThreadDocument, ThreadRunState};

/// The form a run takes while it works. The book defines it.
pub const RUN_FORM: &str = "orb";
/// How long a finished run shows as done, as web Theo holds it.
pub const DONE_HOLD_MS: f64 = 1_500.0;
/// Conversations remembered, so a run that comes back is a later generation.
const REMEMBERED: usize = 256;

/// The activity id a conversation's runs go by.
pub fn activity_id(thread_id: &str) -> String {
    format!("theorem:{thread_id}")
}

/// One conversation's runs, as they were last seen.
#[derive(Clone, Debug)]
struct Seen {
    thread_id: String,
    generation: u64,
    revision: u64,
    state: ThreadRunState,
}

/// Every conversation's runs Theo has seen, and when the last one finished.
#[derive(Debug, Default)]
pub struct Runs {
    /// In the order they were first seen.
    seen: Vec<Seen>,
    done_until: f64,
}

fn phase_of(state: ThreadRunState) -> Phase {
    match state {
        ThreadRunState::Streaming => Phase::Active,
        ThreadRunState::RequiresApproval => Phase::AwaitingUser,
        ThreadRunState::Idle | ThreadRunState::Aborted => Phase::Completed,
        // The driver shows a run as interrupted once it knows nothing will
        // settle it: it has ended, without a result.
        ThreadRunState::Failed | ThreadRunState::Interrupted => Phase::Failed,
    }
}

fn outcome_of(state: ThreadRunState) -> Option<&'static str> {
    match state {
        ThreadRunState::Idle => Some("completed"),
        ThreadRunState::Aborted => Some("stopped"),
        ThreadRunState::Failed => Some("failed"),
        ThreadRunState::Interrupted => Some("interrupted: the authority lost the run"),
        ThreadRunState::Streaming | ThreadRunState::RequiresApproval => None,
    }
}

impl Seen {
    fn activity(&self) -> Activity {
        let phase = phase_of(self.state);
        let mut activity = Activity::new(&activity_id(&self.thread_id), self.generation);
        activity.revision = self.revision;
        activity.phase = phase;
        activity.conversation = Some(self.thread_id.clone());
        activity.agent = Some("theorem".to_owned());
        // While it asks, the face asks; a form would cover it.
        activity.preferred_form = (phase == Phase::Active).then(|| RUN_FORM.to_owned());
        if self.state.is_live() {
            activity.available_actions = vec!["stop".to_owned()];
        }
        activity.outcome = outcome_of(self.state).map(str::to_owned);
        activity
    }
}

impl Runs {
    /// The update the conversation's run state comes to, if it changed.
    pub fn observe(&mut self, thread: &ThreadDocument, now: f64) -> Option<Activity> {
        if thread.thread_id.is_empty() {
            return None;
        }
        let state = thread.run_state;
        let Some(seen) = self
            .seen
            .iter_mut()
            .find(|seen| seen.thread_id == thread.thread_id)
        else {
            self.remember(Seen {
                thread_id: thread.thread_id.clone(),
                // A conversation first seen at rest has had no run here yet.
                generation: u64::from(state.is_live()),
                revision: 0,
                state,
            });
            return state
                .is_live()
                .then(|| self.seen.last().map(Seen::activity))
                .flatten();
        };
        if seen.state == state {
            return None;
        }
        let was_live = seen.state.is_live();
        if !was_live && !state.is_live() {
            // One ending replacing another, as a settled conversation is
            // adopted: nothing began, and what ended stays ended.
            seen.state = state;
            return None;
        }
        if was_live {
            seen.revision += 1;
        } else {
            seen.generation += 1;
            seen.revision = 0;
        }
        seen.state = state;
        let activity = seen.activity();
        if was_live && !state.is_live() {
            self.done_until = now + DONE_HOLD_MS;
        }
        Some(activity)
    }

    fn remember(&mut self, seen: Seen) {
        if self.seen.len() >= REMEMBERED
            && let Some(at) = self.seen.iter().position(|seen| !seen.state.is_live())
        {
            self.seen.remove(at);
        }
        self.seen.push(seen);
    }

    /// The runs still going, in the order they were seen: what a view made
    /// anew starts from.
    pub fn snapshot(&self) -> Vec<Activity> {
        self.seen
            .iter()
            .filter(|seen| seen.state.is_live())
            .map(Seen::activity)
            .collect()
    }

    /// Whether `generation` of the conversation's runs is the one going now:
    /// a stop for any other is refused.
    pub fn is_current(&self, activity: &str, generation: u64) -> bool {
        self.seen.iter().any(|seen| {
            activity_id(&seen.thread_id) == activity
                && seen.generation == generation
                && seen.state.is_live()
        })
    }

    /// The face's liveness for the selected conversation at `now`.
    pub fn liveness(&self, state: ThreadRunState, attending: bool, now: f64) -> Liveness {
        match state {
            ThreadRunState::RequiresApproval => Liveness::Ask,
            ThreadRunState::Streaming => Liveness::Work,
            _ if now < self.done_until => Liveness::Done,
            _ if attending => Liveness::Attend,
            _ => Liveness::Idle,
        }
    }
}

/// The part of the agent's authored manner the face takes, while its turn
/// runs; web Theo takes the same subset. The rest of the wire expression is
/// the sprite's (pose, frame) or the field's (chroma, density, size, calm,
/// tightness, churn, lattice) and the face has no use for it.
pub fn expression(thread: &ThreadDocument) -> Option<Expression> {
    if !thread.run_state.is_live() {
        return None;
    }
    let wire = thread.figure_expression.as_ref()?;
    Some(Expression {
        energy: wire.energy.map(f64::from),
        breath: wire.breath.map(f64::from),
        tilt: Some(f64::from(wire.tilt)),
        gaze: Some((f64::from(wire.gaze[0]), f64::from(wire.gaze[1]))),
        hue: wire.hue.map(f64::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(id: &str, state: ThreadRunState) -> ThreadDocument {
        let mut thread = ThreadDocument::default();
        thread.thread_id = id.to_owned();
        thread.run_state = state;
        thread
    }

    #[test]
    fn a_run_begins_works_and_ends_as_one_generation() {
        let mut runs = Runs::default();
        assert!(
            runs.observe(&thread("a", ThreadRunState::Idle), 0.0)
                .is_none()
        );
        let begun = runs
            .observe(&thread("a", ThreadRunState::Streaming), 10.0)
            .unwrap();
        assert_eq!(begun.id, "theorem:a");
        assert_eq!((begun.generation, begun.revision), (1, 0));
        assert_eq!(begun.phase, Phase::Active);
        assert_eq!(begun.preferred_form.as_deref(), Some(RUN_FORM));
        assert!(runs.is_current("theorem:a", 1));

        let asking = runs
            .observe(&thread("a", ThreadRunState::RequiresApproval), 20.0)
            .unwrap();
        assert_eq!((asking.generation, asking.revision), (1, 1));
        assert_eq!(asking.phase, Phase::AwaitingUser);
        assert_eq!(asking.preferred_form, None);

        let ended = runs
            .observe(&thread("a", ThreadRunState::Aborted), 30.0)
            .unwrap();
        assert_eq!((ended.generation, ended.revision), (1, 2));
        assert_eq!(ended.phase, Phase::Completed);
        assert_eq!(ended.outcome.as_deref(), Some("stopped"));
        assert!(!runs.is_current("theorem:a", 1));
        // Settling the stopped conversation begins nothing.
        assert!(
            runs.observe(&thread("a", ThreadRunState::Idle), 40.0)
                .is_none()
        );
    }

    #[test]
    fn the_next_run_is_a_later_generation_and_an_old_stop_is_refused() {
        let mut runs = Runs::default();
        runs.observe(&thread("a", ThreadRunState::Streaming), 0.0);
        runs.observe(&thread("a", ThreadRunState::Idle), 1.0);
        let second = runs
            .observe(&thread("a", ThreadRunState::Streaming), 2.0)
            .unwrap();
        assert_eq!(second.generation, 2);
        assert!(!runs.is_current("theorem:a", 1));
        assert!(runs.is_current("theorem:a", 2));
    }

    #[test]
    fn the_snapshot_holds_only_runs_still_going() {
        let mut runs = Runs::default();
        runs.observe(&thread("a", ThreadRunState::Streaming), 0.0);
        runs.observe(&thread("b", ThreadRunState::Streaming), 0.0);
        runs.observe(&thread("b", ThreadRunState::Failed), 1.0);
        let snapshot = runs.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].id, "theorem:a");
    }

    #[test]
    fn liveness_asks_works_holds_done_then_attends() {
        let mut runs = Runs::default();
        assert_eq!(
            runs.liveness(ThreadRunState::RequiresApproval, true, 0.0),
            Liveness::Ask
        );
        assert_eq!(
            runs.liveness(ThreadRunState::Streaming, true, 0.0),
            Liveness::Work
        );
        runs.observe(&thread("a", ThreadRunState::Streaming), 0.0);
        runs.observe(&thread("a", ThreadRunState::Idle), 100.0);
        assert_eq!(
            runs.liveness(ThreadRunState::Idle, true, 100.0 + DONE_HOLD_MS - 1.0),
            Liveness::Done
        );
        assert_eq!(
            runs.liveness(ThreadRunState::Idle, true, 100.0 + DONE_HOLD_MS),
            Liveness::Attend
        );
        assert_eq!(
            runs.liveness(ThreadRunState::Idle, false, 100.0 + DONE_HOLD_MS),
            Liveness::Idle
        );
    }

    #[test]
    fn the_face_takes_the_manner_only_while_the_turn_runs() {
        let mut live = thread("a", ThreadRunState::Streaming);
        assert_eq!(expression(&live), None);
        let mut wire = agpui_contract::field::Expression::NEUTRAL;
        wire.hue = Some(40.0);
        wire.energy = Some(0.5);
        wire.tilt = -0.25;
        wire.gaze = [0.5, -1.0];
        live.figure_expression = Some(wire);
        assert_eq!(
            expression(&live),
            Some(Expression {
                energy: Some(0.5),
                breath: None,
                tilt: Some(-0.25),
                gaze: Some((0.5, -1.0)),
                hue: Some(40.0),
            })
        );
        live.run_state = ThreadRunState::Idle;
        assert_eq!(expression(&live), None);
    }
}
