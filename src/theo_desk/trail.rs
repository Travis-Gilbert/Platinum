//! The conversation in Theo's menu: what the person asked and what Theorem
//! answered, oldest first, scrolled to the newest.
//!
//! SOURCING: GPUI Kit's read-only auto-grow `Textarea` with inline tokens for
//! Theorem's answers; a GPUI `ScrollHandle` for the trail itself.

use gpui_kit::component::input::{InputContent, InputToken, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    px, relative,
};
use theorem_surface_contracts::thread::{ThreadActor, ThreadMessage, ThreadPart};

use super::{reply_content, token_icon};

/// The trail's height before it scrolls, in points: about a dozen lines.
const TRAIL_HEIGHT: f32 = 260.0;
/// An answer grows to this many rows; past that the trail scrolls, not it.
const ANSWER_ROWS: usize = 400;

enum Turn {
    Person {
        text: SharedString,
    },
    Theorem {
        message_id: String,
        answer: Entity<TextareaState>,
        shown: InputContent,
    },
}

/// What the trail shows besides the turns.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrailState {
    /// A run is going: the trail says so until it ends.
    pub working: bool,
    /// Theorem is not set up on this Mac.
    pub unset: bool,
}

pub struct Trail {
    turns: Vec<Turn>,
    state: TrailState,
    scroll: ScrollHandle,
}

impl Trail {
    pub fn new() -> Self {
        Self {
            turns: Vec::new(),
            state: TrailState::default(),
            scroll: ScrollHandle::new(),
        }
    }

    /// Whether there is anything to show.
    pub fn shows(messages: &[ThreadMessage], state: TrailState) -> bool {
        state.working || state.unset || messages.iter().any(|m| readable(m).is_some())
    }

    /// Follows the conversation, keeping each answer's text box, and scrolls
    /// to the newest when anything changed.
    pub fn sync(
        &mut self,
        messages: &[ThreadMessage],
        state: TrailState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.outline();
        let mut kept: Vec<Turn> = Vec::with_capacity(messages.len());
        let mut old = std::mem::take(&mut self.turns);
        let mut changed = state != self.state;
        for message in messages {
            match readable(message) {
                Some(Readable::Person(text)) => kept.push(Turn::Person { text }),
                Some(Readable::Theorem(content)) => {
                    let reused = old.iter().position(|turn| {
                        matches!(turn, Turn::Theorem { message_id, .. } if *message_id == message.message_id)
                    });
                    let turn = match reused.map(|at| old.swap_remove(at)) {
                        Some(Turn::Theorem {
                            message_id,
                            answer,
                            shown,
                        }) => {
                            if shown != content {
                                changed = true;
                                answer.update(cx, |answer, cx| {
                                    answer.set_value(content.clone(), window, cx)
                                });
                            }
                            Turn::Theorem {
                                message_id,
                                answer,
                                shown: content,
                            }
                        }
                        _ => {
                            changed = true;
                            let answer = cx.new(|cx| {
                                let mut answer =
                                    TextareaState::new(window, cx).auto_grow(1, ANSWER_ROWS);
                                answer.set_value(content.clone(), window, cx);
                                answer
                            });
                            Turn::Theorem {
                                message_id: message.message_id.clone(),
                                answer,
                                shown: content,
                            }
                        }
                    };
                    kept.push(turn);
                }
                None => {}
            }
        }
        self.turns = kept;
        changed |= self.outline() != before;
        self.state = state;
        if changed {
            self.scroll.scroll_to_bottom();
            cx.notify();
        }
    }
}

impl Trail {
    /// The turns as an outline: what the person asked, and where Theorem
    /// answered. An answer's own words are compared where it is kept.
    fn outline(&self) -> Vec<Option<SharedString>> {
        self.turns
            .iter()
            .map(|turn| match turn {
                Turn::Person { text } => Some(text.clone()),
                Turn::Theorem { .. } => None,
            })
            .collect()
    }
}

enum Readable {
    Person(SharedString),
    Theorem(InputContent),
}

/// What a message shows in the trail, if anything yet.
fn readable(message: &ThreadMessage) -> Option<Readable> {
    match message.actor {
        ThreadActor::User => {
            let text = message
                .parts
                .iter()
                .filter_map(|part| match part {
                    ThreadPart::Text { text, .. } => Some(text.trim()),
                    _ => None,
                })
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then(|| Readable::Person(text.into()))
        }
        ThreadActor::Theorem => reply_content(&message.parts).map(Readable::Theorem),
        ThreadActor::Insight | ThreadActor::System => None,
    }
}

impl Render for Trail {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let asked = theme.secondary;
        let mut column = v_flex().w_full().gap_2().py_1();
        if self.state.unset {
            column = column.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("Theorem isn't set up on this Mac."),
            );
        }
        for turn in &self.turns {
            column = column.child(match turn {
                // What the person asked sits to the right, as in a chat.
                Turn::Person { text } => div().w_full().flex().justify_end().child(
                    div()
                        .max_w(relative(0.85))
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(asked)
                        .text_sm()
                        .child(text.clone()),
                ),
                Turn::Theorem { answer, .. } => div().w_full().child(
                    Textarea::new(answer)
                        .readonly(true)
                        .bordered(false)
                        .appearance(false)
                        .token(|token, _, _| {
                            InputToken::new(token).icon(token_icon(token.token().id()))
                        }),
                ),
            });
        }
        if self.state.working {
            column = column.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Theo is working on it."),
            );
        }
        div()
            .id("theo-trail")
            .w_full()
            .max_h(px(TRAIL_HEIGHT))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str, actor: ThreadActor, texts: &[&str]) -> ThreadMessage {
        ThreadMessage {
            message_id: id.to_owned(),
            actor,
            parts: texts
                .iter()
                .map(|text| ThreadPart::Text {
                    part_id: "p".to_owned(),
                    text: (*text).to_owned(),
                    status: Default::default(),
                })
                .collect(),
            ..ThreadMessage::default()
        }
    }

    #[test]
    fn the_person_reads_as_their_words_joined() {
        let asked = message(
            "u",
            ThreadActor::User,
            &[" What is 2 + 2? ", "", "Quickly."],
        );
        match readable(&asked) {
            Some(Readable::Person(text)) => assert_eq!(text.as_ref(), "What is 2 + 2?\nQuickly."),
            _ => panic!("the person's turn reads as their words"),
        }
    }

    #[test]
    fn theorem_reads_as_its_reply() {
        let answered = message("t", ThreadActor::Theorem, &["Four."]);
        match readable(&answered) {
            Some(Readable::Theorem(content)) => assert_eq!(content.text().as_ref(), "Four."),
            _ => panic!("Theorem's turn reads as its reply"),
        }
    }

    #[test]
    fn empty_turns_and_insights_are_not_shown() {
        assert!(readable(&message("u", ThreadActor::User, &["  "])).is_none());
        assert!(readable(&message("t", ThreadActor::Theorem, &[])).is_none());
        assert!(readable(&message("i", ThreadActor::Insight, &["A thought."])).is_none());
        assert!(readable(&message("s", ThreadActor::System, &["Started."])).is_none());
    }

    #[test]
    fn the_trail_shows_when_there_is_a_turn_or_a_state() {
        let quiet = TrailState::default();
        assert!(!Trail::shows(&[], quiet));
        assert!(!Trail::shows(
            &[message("i", ThreadActor::Insight, &["x"])],
            quiet
        ));
        assert!(Trail::shows(
            &[message("u", ThreadActor::User, &["Hi"])],
            quiet
        ));
        assert!(Trail::shows(
            &[],
            TrailState {
                working: true,
                unset: false
            }
        ));
        assert!(Trail::shows(
            &[],
            TrailState {
                working: false,
                unset: true
            }
        ));
    }
}
