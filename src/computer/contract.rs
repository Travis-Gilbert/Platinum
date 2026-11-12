//! The broker's data: what an observation holds, how a request names its
//! target, and what a receipt says happened. Everything here is pure, so the
//! rules that decide refusal, staleness, outcome and resumption are tested
//! without a desktop.

use serde::{Deserialize, Serialize};

/// A rectangle in global screen points: origin at the top left of the main
/// display, y downwards, the space Accessibility and the window server use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    /// This rectangle in a capture of `window` taken at `scale` pixels per
    /// point: the one transform from screen points to capture pixels.
    pub fn in_capture(self, window: Rect, scale: f64) -> Rect {
        Rect {
            x: (self.x - window.x) * scale,
            y: (self.y - window.y) * scale,
            w: self.w * scale,
            h: self.h * scale,
        }
    }
}

/// The window an observation or action is about. `pid` and `window` are the
/// identity: an element is only ever acted on in the window it was seen in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub pid: i32,
    pub window: u32,
    #[serde(default)]
    pub app: Option<String>,
}

/// One element as an observation saw it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    /// Its position in the observation, which receipts name it by.
    pub index: usize,
    /// The executor's handle for it, valid only for the snapshot it came from.
    pub token: String,
    pub role: String,
    #[serde(default)]
    pub subrole: Option<String>,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub bounds: Option<Rect>,
    #[serde(default)]
    pub actions: Vec<String>,
}

/// A window's picture, taken with the semantic snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Pixels per screen point.
    pub scale: f64,
}

/// What the broker saw of one window at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Observation {
    /// The broker's own number for it, increasing for the process.
    pub id: u64,
    pub target: Target,
    /// The executor's snapshot, which scopes the element tokens.
    pub snapshot: String,
    pub window_bounds: Option<Rect>,
    pub elements: Vec<Element>,
    /// False when the tree walk stopped early (a timeout or a cap).
    pub complete: bool,
    pub captured_at_ms: u64,
    pub capture: Option<Capture>,
    /// Why there is no picture, when there is none.
    pub capture_missing: Option<String>,
}

/// How a step names the element it means: by what it is, not by a token, so
/// the same step resolves again against a fresh observation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    /// Matched against the Accessibility role, with or without its `AX` prefix.
    #[serde(default)]
    pub role: Option<String>,
    /// A case-insensitive part of the element's label.
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum Resolved<'a> {
    One(&'a Element),
    None,
    /// More than one element matches; the step has to say which.
    Ambiguous(usize),
}

/// The current-target resolver: the one element of `observation` the query
/// names, or why there is not exactly one.
pub fn resolve<'a>(observation: &'a Observation, query: &Query) -> Resolved<'a> {
    let role = query
        .role
        .as_deref()
        .map(|role| role.trim_start_matches("AX").to_ascii_lowercase());
    let label = query.label.as_deref().map(str::to_lowercase);
    let found: Vec<&Element> = observation
        .elements
        .iter()
        .filter(|element| {
            role.as_deref().is_none_or(|role| {
                element.role.trim_start_matches("AX").to_ascii_lowercase() == role
            }) && label
                .as_deref()
                .is_none_or(|label| element.label.to_lowercase().contains(label))
        })
        .collect();
    match found.as_slice() {
        [] => Resolved::None,
        [one] => Resolved::One(one),
        many => Resolved::Ambiguous(many.len()),
    }
}

/// What to do to an element.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Act {
    /// The element's primary Accessibility action.
    Press,
    /// Insert text at the element's caret.
    Type { text: String },
    /// Replace the element's value.
    SetValue { value: String },
}

/// One action against an element of one observation, under one generation.
#[derive(Clone, Debug, PartialEq)]
pub struct ActRequest {
    pub action: u64,
    pub generation: u64,
    pub observation: u64,
    pub element: usize,
    pub act: Act,
}

/// What became of an action. Delivered input is not a verified result, and a
/// call that failed after it was sent is uncertain, never refused: on the
/// comparison's Mac, Finder's AXOpen reported failure and the folder opened.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum Outcome {
    /// The effect was read back from the application.
    Verified,
    /// The input reached the application; its effect was not confirmed.
    Delivered,
    /// Nothing was sent.
    Refused(Refusal),
    /// Something may have been sent; observe before repeating it.
    Uncertain(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "reason", content = "detail")]
pub enum Refusal {
    /// The request's generation was revoked by a takeover or Stop.
    Revoked,
    /// The observation is not the window's latest; observe again.
    Stale,
    /// The element is not in the observation it names.
    NoElement,
    /// The executor declined before sending anything.
    Executor(String),
    /// No executor in this build or on this Mac.
    Unavailable(String),
}

/// The receipt for one action, kept in the receipts log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Receipt {
    pub action: u64,
    pub generation: u64,
    pub observation: u64,
    pub target: Target,
    pub element: Option<Element>,
    pub act: Act,
    pub outcome: Outcome,
    /// The observation the outcome was verified against, if one was taken.
    pub verified_by: Option<u64>,
    pub at_ms: u64,
    pub elapsed_ms: u64,
}

/// What the executor's reply says about an action it was asked for, before
/// the broker reads the application back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// The executor read its effect back.
    Confirmed,
    /// Delivered, effect unconfirmed (`unverifiable` or `partial`).
    Delivered,
    /// Delivered and probably without effect.
    SuspectedNoop,
    /// Declined before sending: a stale token, a locked screen, bad arguments.
    Declined { stale: bool, why: String },
    /// Failed or was cancelled after it may have been sent.
    Failed(String),
}

/// The outcome of a reply, given whether the broker's own read-back of the
/// application shows the intended effect (`None` when it could not tell).
pub fn outcome(reply: &Reply, read_back: Option<bool>) -> Outcome {
    match (reply, read_back) {
        (Reply::Declined { stale: true, .. }, _) => Outcome::Refused(Refusal::Stale),
        (Reply::Declined { why, .. }, _) => Outcome::Refused(Refusal::Executor(why.clone())),
        (_, Some(true)) => Outcome::Verified,
        (Reply::Confirmed, None) => Outcome::Verified,
        (Reply::Confirmed | Reply::Delivered, Some(false)) => {
            Outcome::Uncertain("delivered, but the application does not show it".into())
        }
        (Reply::Delivered, None) => Outcome::Delivered,
        (Reply::SuspectedNoop, _) => Outcome::Uncertain("the executor suspects no effect".into()),
        (Reply::Failed(why), _) => Outcome::Uncertain(why.clone()),
    }
}

/// Splits text into the pieces typed one dispatch at a time, so a takeover
/// stops typing between pieces. Pieces end after a space where one falls
/// within `most` characters, and never just before whitespace: the field's
/// value is read back trimmed, so whitespace at the end of what landed is
/// invisible, and a piece that stops short of it could not be told from one
/// that typed it (see `remaining`).
pub fn pieces(text: &str, most: usize) -> Vec<&str> {
    let most = most.max(1);
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut end = rest.len();
        let mut cut_at_space = None;
        for (count, (at, ch)) in rest.char_indices().enumerate() {
            if count == most {
                end = at;
                break;
            }
            if ch.is_whitespace() {
                cut_at_space = Some(at + ch.len_utf8());
            }
        }
        if end < rest.len()
            && let Some(space) = cut_at_space
        {
            end = space;
        }
        end += rest[end..].len() - rest[end..].trim_start().len();
        out.push(&rest[..end]);
        rest = &rest[end..];
    }
    out
}

/// How much of `text` is still to be typed into a field that read `before`
/// when typing began and reads `now`. Typing inserts at the caret, wherever
/// it was, so what landed is whatever `now` holds between some start and the
/// matching end of `before`.
///
/// Both readings come from the executor, which reports a value without the
/// whitespace at its ends (Cua trims it). So where the insertion touches an
/// end of the field, whitespace at either side of what landed cannot be
/// seen; there the longest start of `text` that fits is taken, which may
/// leave a space untyped but never types anything twice. `None` when no
/// reading fits: someone else changed the field, and resuming would guess.
pub fn remaining<'a>(text: &'a str, before: &str, now: &str) -> Option<&'a str> {
    if now.len() < before.len() {
        return None;
    }
    let starts: Vec<usize> = text
        .char_indices()
        .map(|(at, _)| at)
        .chain([text.len()])
        .collect();
    let mut typed = None;
    for at in before
        .char_indices()
        .map(|(at, _)| at)
        .chain([before.len()])
    {
        let (head, tail) = before.split_at(at);
        if !now.starts_with(head) || !now.ends_with(tail) {
            continue;
        }
        let landed = &now[at..now.len() - tail.len()];
        let at_an_end = at == 0 || at == before.len();
        for &end in &starts {
            let start = &text[..end];
            let fits = if at_an_end {
                start.trim() == landed.trim()
            } else {
                start == landed
            };
            if fits {
                typed = typed.max(Some(end));
            }
        }
    }
    typed.map(|end| &text[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(index: usize, role: &str, label: &str) -> Element {
        Element {
            index,
            token: format!("s00000001:{index}"),
            role: role.into(),
            subrole: None,
            label: label.into(),
            value: None,
            bounds: None,
            actions: vec![],
        }
    }

    fn observation(elements: Vec<Element>) -> Observation {
        Observation {
            id: 1,
            target: Target {
                pid: 1,
                window: 2,
                app: None,
            },
            snapshot: "s00000001".into(),
            window_bounds: None,
            elements,
            complete: true,
            captured_at_ms: 0,
            capture: None,
            capture_missing: None,
        }
    }

    #[test]
    fn a_query_names_one_element_by_role_and_label() {
        let seen = observation(vec![
            element(0, "AXWindow", "notes.txt"),
            element(1, "AXButton", "Proceed"),
            element(2, "AXButton", "Cancel"),
        ]);
        let query = Query {
            role: Some("button".into()),
            label: Some("proc".into()),
        };
        assert_eq!(resolve(&seen, &query), Resolved::One(&seen.elements[1]));
        let query = Query {
            role: Some("AXButton".into()),
            label: None,
        };
        assert_eq!(resolve(&seen, &query), Resolved::Ambiguous(2));
        let query = Query {
            role: Some("AXTextArea".into()),
            label: None,
        };
        assert_eq!(resolve(&seen, &query), Resolved::None);
    }

    #[test]
    fn a_role_matches_whole_not_as_a_part() {
        let seen = observation(vec![
            element(0, "AXRadioButton", "Rotate"),
            element(1, "AXButton", ""),
        ]);
        let query = Query {
            role: Some("AXButton".into()),
            label: None,
        };
        assert_eq!(resolve(&seen, &query), Resolved::One(&seen.elements[1]));
    }

    #[test]
    fn a_rectangle_moves_into_capture_pixels_once() {
        let window = Rect {
            x: 300.0,
            y: 180.0,
            w: 673.0,
            h: 439.0,
        };
        let field = Rect {
            x: 310.0,
            y: 212.0,
            w: 100.0,
            h: 20.0,
        };
        assert_eq!(
            field.in_capture(window, 2.0),
            Rect {
                x: 20.0,
                y: 64.0,
                w: 200.0,
                h: 40.0
            }
        );
    }

    #[test]
    fn a_declined_reply_is_refused_and_a_failed_one_uncertain() {
        let stale = Reply::Declined {
            stale: true,
            why: "stale".into(),
        };
        assert_eq!(outcome(&stale, None), Outcome::Refused(Refusal::Stale));
        let locked = Reply::Declined {
            stale: false,
            why: "screen_locked".into(),
        };
        assert_eq!(
            outcome(&locked, Some(true)),
            Outcome::Refused(Refusal::Executor("screen_locked".into()))
        );
        // Reported failure after dispatch is never taken as "nothing happened".
        let failed = Reply::Failed("AXOpen returned -25205".into());
        assert!(matches!(outcome(&failed, None), Outcome::Uncertain(_)));
        assert_eq!(outcome(&failed, Some(true)), Outcome::Verified);
    }

    #[test]
    fn delivery_is_verified_only_by_a_read_back_or_the_executor() {
        assert_eq!(outcome(&Reply::Delivered, None), Outcome::Delivered);
        assert_eq!(outcome(&Reply::Delivered, Some(true)), Outcome::Verified);
        assert!(matches!(
            outcome(&Reply::Delivered, Some(false)),
            Outcome::Uncertain(_)
        ));
        assert_eq!(outcome(&Reply::Confirmed, None), Outcome::Verified);
        assert!(matches!(
            outcome(&Reply::Confirmed, Some(false)),
            Outcome::Uncertain(_)
        ));
        assert!(matches!(
            outcome(&Reply::SuspectedNoop, None),
            Outcome::Uncertain(_)
        ));
    }

    #[test]
    fn text_is_typed_in_pieces_that_end_at_spaces() {
        assert_eq!(
            pieces("Hello there world", 8),
            vec!["Hello ", "there ", "world"]
        );
        assert_eq!(pieces("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(pieces("héllo wörld", 6), vec!["héllo ", "wörld"]);
        assert_eq!(pieces("", 4), Vec::<&str>::new());
        assert_eq!(pieces("ab", 0), vec!["a", "b"]);
        // The gate's sentence: an eight-letter word takes its space along.
        assert_eq!(
            pieces("this sentence into", 8),
            vec!["this ", "sentence ", "into"]
        );
        assert_eq!(pieces("ab   cd", 4), vec!["ab   ", "cd"]);
    }

    #[test]
    fn resuming_types_only_what_did_not_land() {
        let text = "Remember the milk";
        assert_eq!(
            remaining(text, "List: ", "List: Remember "),
            Some("the milk")
        );
        assert_eq!(
            remaining(text, "List: ", "List: Remember the milk"),
            Some("")
        );
        assert_eq!(remaining(text, "List: ", "List: "), Some(text));
        // Someone else edited the field: do not guess.
        assert_eq!(remaining(text, "List: ", "Shopping: Remember"), None);
        assert_eq!(remaining(text, "List: ", "List: Remember a"), None);
        // The caret was not at the end.
        assert_eq!(remaining("Buy ", "milk", "Bmilk"), Some("uy "));
        assert_eq!(remaining("Buy ", "milk", "Buy milk"), Some(""));
        assert_eq!(remaining("xy", "x", "xxy"), Some(""));
        assert_eq!(remaining("xy", "x", "xyx"), Some(""));
        assert_eq!(remaining("ab", "long", "lo"), None);
        // The executor trims the field's value: a space typed at either end
        // is not seen, so it is taken to have landed rather than typed again.
        assert_eq!(remaining(" Theo typed", "", "Theo"), Some("typed"));
        assert_eq!(
            remaining(
                " Theo typed this",
                "Theo gate note.",
                "Theo Theo gate note."
            ),
            Some("typed this")
        );
        assert_eq!(remaining("a b", "x", "a x"), Some("b"));
    }
}
