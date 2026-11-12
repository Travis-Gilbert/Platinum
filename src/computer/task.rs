//! A small task in an existing application, run through the broker: each
//! step finds its element in a fresh observation, acts, and is read back.
//! When the person takes the keyboard or mouse the run stops at the next
//! write; it resumes, if the task allows it, only after the person has been
//! idle, and only from a fresh observation. A journal beside the task file
//! records what each step started from, so a resumed step (in this run or a
//! later one) does only what has not landed.
//!
//! This is the verification entry point for Stage C (`--theo-task <file>`).
//! Agents reach the broker through Stage D's capability service instead.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::contract::{
    Act, ActRequest, Element, Outcome, Query, Receipt, Refusal, Resolved, Target, pieces,
    remaining, resolve,
};
use super::{Broker, input};

/// The most characters typed in one dispatch, so a takeover stops typing
/// within a word or two.
const PIECE: usize = 8;
/// Dispatches in a row that change nothing before a step gives up.
const MOST_ATTEMPTS: u32 = 3;
/// How long a run waits for the person to be idle before giving up.
const MOST_WAIT: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, Deserialize)]
pub struct Task {
    pub target: Aim,
    pub steps: Vec<Step>,
    /// Resume after a takeover once the person has used neither keyboard nor
    /// mouse button for this many seconds. Absent: a takeover ends the run.
    #[serde(default)]
    pub resume_after_idle_s: Option<u64>,
}

/// Which window the task works in.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Aim {
    Window {
        pid: i32,
        window: u32,
    },
    App {
        app: String,
        /// A part of the window title, when the app has several windows.
        #[serde(default)]
        title: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub struct Step {
    pub find: Query,
    pub act: Act,
    /// An element that shows the step is done: checked before acting, so a
    /// resumed run does not repeat it, and after a press, to verify it.
    #[serde(default)]
    pub until: Option<Query>,
}

/// What a step had done when the run last touched it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub done: bool,
    /// The field's value before this step first typed into it.
    pub before: Option<String>,
    /// A press was sent; without `until` it is never sent again.
    pub pressed: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Journal {
    steps: Vec<Progress>,
}

/// What the run should do next for a step, from its element as seen now.
#[derive(Debug, PartialEq)]
pub enum Next {
    Done,
    Send(Act),
    /// The field no longer reads as this step's typing left it.
    Diverged,
    /// A press may already have taken effect and there is no way to tell.
    Unrepeatable,
}

/// The rule for resuming without repeating an effect.
pub fn next(step: &Step, found: &Element, progress: &mut Progress) -> Next {
    match &step.act {
        Act::Press if progress.pressed => Next::Unrepeatable,
        Act::Press => Next::Send(Act::Press),
        // The executor trims values, so a value is compared as it would read.
        Act::SetValue { value }
            if found.value.as_deref().unwrap_or_default().trim() == value.trim() =>
        {
            Next::Done
        }
        Act::SetValue { value } => Next::Send(Act::SetValue {
            value: value.clone(),
        }),
        Act::Type { text } => {
            let now = found.value.clone().unwrap_or_default();
            let before = progress.before.get_or_insert_with(|| now.clone());
            match remaining(text, before, &now) {
                None => Next::Diverged,
                Some("") => Next::Done,
                Some(rest) => Next::Send(Act::Type {
                    text: pieces(rest, PIECE)[0].to_owned(),
                }),
            }
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub finished: bool,
    pub stopped: Option<String>,
    pub takeovers: u32,
    pub resumes: u32,
    pub receipts: Vec<Receipt>,
    /// What each step's element read when the run ended, from a fresh
    /// observation: the evidence that nothing was typed twice.
    pub fields: Vec<Option<String>>,
}

/// Reads and runs the task at `path`, keeping its journal at
/// `<path>.journal.json` and writing the report to `<path>.report.json`.
pub fn run_file(broker: &Broker, path: &Path) -> anyhow::Result<Report> {
    let task: Task = serde_json::from_slice(&std::fs::read(path)?)?;
    let journal = PathBuf::from(format!("{}.journal.json", path.display()));
    let report = run(broker, &task, &journal);
    std::fs::write(
        format!("{}.report.json", path.display()),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

fn target(broker: &Broker, aim: &Aim) -> Result<Target, String> {
    match aim {
        Aim::Window { pid, window } => Ok(Target {
            pid: *pid,
            window: *window,
            app: None,
        }),
        Aim::App { app, title } => {
            let mut windows = broker.windows(app).map_err(|why| format!("{why:?}"))?;
            let seen: Vec<_> = windows
                .iter()
                .map(|found| (found.target.window, found.on_screen, found.title.clone()))
                .collect();
            // The title is the identity; being on screen only orders the
            // candidates. Cua acts through Accessibility, which reaches a
            // window that is covered or on another Space, and its on-screen
            // flag was seen false for a window the window server showed.
            windows.sort_by_key(|found| (!found.on_screen, found.z_index.unwrap_or(i64::MAX)));
            windows
                .into_iter()
                .find(|found| {
                    title
                        .as_deref()
                        .is_none_or(|title| titled(broker, found, title))
                })
                .map(|found| found.target)
                .ok_or_else(|| format!("no window of {app} matches; saw {seen:?}"))
        }
    }
}

/// Whether a window's title holds `title`. Without Screen Recording the
/// window server gives no titles, so the window's Accessibility title, from
/// an observation of it, decides.
fn titled(broker: &Broker, window: &super::cua::Window, title: &str) -> bool {
    if !window.title.is_empty() {
        return window.title.contains(title);
    }
    broker.observe(&window.target, false).is_ok_and(|seen| {
        seen.elements
            .iter()
            .any(|element| element.role == "AXWindow" && element.label.contains(title))
    })
}

pub fn run(broker: &Broker, task: &Task, journal_path: &Path) -> Report {
    let mut report = Report::default();
    let target = match target(broker, &task.target) {
        Ok(target) => target,
        Err(why) => {
            return Report {
                stopped: Some(why),
                ..report
            };
        }
    };
    drive(broker, task, &target, journal_path, &mut report);
    if let Ok(seen) = broker.observe(&target, false) {
        report.fields = task
            .steps
            .iter()
            .map(|step| match resolve(&seen, &step.find) {
                Resolved::One(found) => found.value.clone(),
                _ => None,
            })
            .collect();
    }
    report
}

fn drive(broker: &Broker, task: &Task, target: &Target, journal_path: &Path, report: &mut Report) {
    let mut journal: Journal = std::fs::read(journal_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .filter(|journal: &Journal| journal.steps.len() == task.steps.len())
        .unwrap_or_else(|| Journal {
            steps: vec![Progress::default(); task.steps.len()],
        });
    let save = |journal: &Journal| {
        if let Ok(bytes) = serde_json::to_vec_pretty(journal) {
            let _ = std::fs::write(journal_path, bytes);
        }
    };
    let mut generation = broker.take_control();
    for (index, step) in task.steps.iter().enumerate() {
        let mut attempts = 0;
        while !journal.steps[index].done {
            match attempt(broker, target, generation, step, &mut journal.steps[index]) {
                Attempt::Progressed(receipt) => {
                    attempts = 0;
                    report.receipts.extend(receipt);
                }
                Attempt::Unchanged(receipt) => {
                    report.receipts.extend(receipt);
                    attempts += 1;
                    if attempts >= MOST_ATTEMPTS {
                        report.stopped = Some(format!("step {index} made no progress"));
                        save(&journal);
                        return;
                    }
                }
                Attempt::Revoked(receipt) => {
                    report.receipts.extend(receipt);
                    save(&journal);
                    if broker.stopped(generation) {
                        report.stopped = Some("Theo's Stop".into());
                        return;
                    }
                    report.takeovers += 1;
                    let Some(idle) = task.resume_after_idle_s else {
                        report.stopped = Some("the person took over".into());
                        return;
                    };
                    if !wait_for_idle(Duration::from_secs(idle)) {
                        report.stopped = Some("the person kept working".into());
                        return;
                    }
                    generation = broker.take_control();
                    report.resumes += 1;
                    log::info!("computer: resuming step {index} from a fresh observation");
                }
                Attempt::Stop(why, receipt) => {
                    report.receipts.extend(receipt);
                    report.stopped = Some(format!("step {index}: {why}"));
                    save(&journal);
                    return;
                }
            }
            save(&journal);
        }
    }
    report.finished = true;
}

enum Attempt {
    Progressed(Option<Receipt>),
    Unchanged(Option<Receipt>),
    Revoked(Option<Receipt>),
    Stop(String, Option<Receipt>),
}

/// One fresh observation and at most one dispatch for a step.
fn attempt(
    broker: &Broker,
    target: &Target,
    generation: u64,
    step: &Step,
    progress: &mut Progress,
) -> Attempt {
    if broker.generation() != generation {
        return Attempt::Revoked(None);
    }
    let seen = match broker.observe(target, true) {
        Ok(seen) => seen,
        Err(why) => return Attempt::Stop(format!("could not observe: {why:?}"), None),
    };
    if let Some(until) = &step.until
        && matches!(resolve(&seen, until), Resolved::One(_))
    {
        progress.done = true;
        return Attempt::Progressed(None);
    }
    let found = match resolve(&seen, &step.find) {
        Resolved::One(found) => found.clone(),
        Resolved::None => return Attempt::Stop(format!("nothing matches {:?}", step.find), None),
        Resolved::Ambiguous(count) => {
            return Attempt::Stop(format!("{count} elements match {:?}", step.find), None);
        }
    };
    let act = match next(step, &found, progress) {
        Next::Done => {
            progress.done = true;
            return Attempt::Progressed(None);
        }
        Next::Send(act) => act,
        Next::Diverged => return Attempt::Stop("the field changed under the step".into(), None),
        Next::Unrepeatable => {
            return Attempt::Stop(
                "a press was sent and its effect cannot be seen; not repeated".into(),
                None,
            );
        }
    };
    if act == Act::Press {
        progress.pressed = true;
    }
    let request = ActRequest {
        action: broker.next_action(),
        generation,
        observation: seen.id,
        element: found.index,
        act: act.clone(),
    };
    let receipt = broker.act(target, &request, step.until.as_ref());
    match &receipt.outcome {
        Outcome::Refused(Refusal::Revoked) => {
            if act == Act::Press {
                progress.pressed = false;
            }
            Attempt::Revoked(Some(receipt))
        }
        Outcome::Refused(Refusal::Stale | Refusal::NoElement) => {
            if act == Act::Press {
                progress.pressed = false;
            }
            Attempt::Unchanged(Some(receipt))
        }
        Outcome::Refused(why) => Attempt::Stop(format!("refused: {why:?}"), Some(receipt)),
        // A press is done once delivered; typing and values are settled by
        // the next attempt's fresh observation, which also reconciles an
        // uncertain delivery before anything is sent again.
        Outcome::Verified | Outcome::Delivered if act == Act::Press => {
            progress.done = true;
            Attempt::Progressed(Some(receipt))
        }
        Outcome::Uncertain(_) if act == Act::Press => Attempt::Stop(
            "a press may not have landed; not repeated".into(),
            Some(receipt),
        ),
        Outcome::Verified | Outcome::Delivered => Attempt::Progressed(Some(receipt)),
        // Cancelled by a takeover in flight: what landed is settled by a
        // fresh observation once the person is idle.
        Outcome::Uncertain(_) if broker.generation() != generation => {
            Attempt::Revoked(Some(receipt))
        }
        Outcome::Uncertain(_) => Attempt::Unchanged(Some(receipt)),
    }
}

/// Waits until neither keyboard nor mouse button has been used for `idle`.
fn wait_for_idle(idle: Duration) -> bool {
    let started = Instant::now();
    let mut mark = input::Mark::now();
    let mut quiet_since = Instant::now();
    while started.elapsed() < MOST_WAIT {
        std::thread::sleep(Duration::from_millis(250));
        if mark.person_acted() {
            mark = input::Mark::now();
            quiet_since = Instant::now();
        } else if quiet_since.elapsed() >= idle {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(value: &str) -> Element {
        Element {
            index: 1,
            token: "s00000001:1".into(),
            role: "AXTextArea".into(),
            subrole: None,
            label: String::new(),
            value: Some(value.into()),
            bounds: None,
            actions: vec![],
        }
    }

    fn step(act: Act) -> Step {
        Step {
            find: Query {
                role: Some("AXTextArea".into()),
                label: None,
            },
            act,
            until: None,
        }
    }

    #[test]
    fn typing_goes_in_pieces_and_resumes_where_it_stopped() {
        let typing = step(Act::Type {
            text: "Remember the milk".into(),
        });
        let mut progress = Progress::default();
        assert_eq!(
            next(&typing, &field("List: "), &mut progress),
            // An eight-letter word takes its space along: a piece never
            // stops just before whitespace the read-back cannot show.
            Next::Send(Act::Type {
                text: "Remember ".into()
            })
        );
        assert_eq!(progress.before.as_deref(), Some("List: "));
        // Interrupted after the first piece; a later run sees the field.
        assert_eq!(
            next(&typing, &field("List: Remember "), &mut progress),
            Next::Send(Act::Type {
                text: "the milk".into()
            })
        );
        assert_eq!(
            next(&typing, &field("List: Remember the milk"), &mut progress),
            Next::Done
        );
        // A dispatch that landed only in part leaves only the rest to type.
        // The value reads trimmed, so the space after "the" cannot be seen
        // and is taken to have landed: a space may be lost, never doubled.
        assert_eq!(
            next(&typing, &field("List: Remember the"), &mut progress),
            Next::Send(Act::Type {
                text: "milk".into()
            })
        );
        // Someone else typed into the field: stop rather than guess.
        assert_eq!(
            next(&typing, &field("List: Remember a"), &mut progress),
            Next::Diverged
        );
    }

    #[test]
    fn a_press_is_never_sent_twice_and_a_value_only_when_different() {
        let press = step(Act::Press);
        let mut progress = Progress::default();
        assert_eq!(
            next(&press, &field(""), &mut progress),
            Next::Send(Act::Press)
        );
        progress.pressed = true;
        assert_eq!(next(&press, &field(""), &mut progress), Next::Unrepeatable);
        let set = step(Act::SetValue { value: "42".into() });
        // A value reads back without its edge whitespace.
        let padded = step(Act::SetValue {
            value: " 42 ".into(),
        });
        assert_eq!(
            next(&padded, &field("42"), &mut Progress::default()),
            Next::Done
        );
        assert_eq!(
            next(&set, &field("42"), &mut Progress::default()),
            Next::Done
        );
        assert_eq!(
            next(&set, &field("7"), &mut Progress::default()),
            Next::Send(Act::SetValue { value: "42".into() })
        );
    }

    #[test]
    fn a_task_file_reads() {
        let task: Task = serde_json::from_str(
            r#"{"target": {"app": "TextEdit", "title": "theo-probe"}, "resume_after_idle_s": 3,
                "steps": [{"find": {"role": "AXTextArea"}, "act": {"type": {"text": "Hello"}}},
                          {"find": {"role": "AXButton", "label": "Proceed"}, "act": "press",
                           "until": {"label": "Done"}}]}"#,
        )
        .unwrap();
        assert!(matches!(task.target, Aim::App { ref app, .. } if app == "TextEdit"));
        assert_eq!(
            task.steps[0].act,
            Act::Type {
                text: "Hello".into()
            }
        );
        assert_eq!(task.steps[1].act, Act::Press);
        assert_eq!(task.resume_after_idle_s, Some(3));
        let task: Task =
            serde_json::from_str(r#"{"target": {"pid": 1, "window": 2}, "steps": []}"#).unwrap();
        assert!(matches!(task.target, Aim::Window { pid: 1, window: 2 }));
    }
}
