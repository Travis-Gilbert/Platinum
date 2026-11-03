//! The transcript: one scroller row per message of the projection's
//! `ThreadDocument`, drawn when the scroller asks for it.
//!
//! Texture carries what color cannot: the reader's turns sit on a filled
//! block at the trailing edge, Theorem's read as plain text, its work reads in
//! monospace rows that fold into one summary, and every status says its word.
//!
//! SOURCING: GPUI Kit's `Message`, `Bubble`, `TextView` (markdown and code
//! highlighting), `Alert`, `Clipboard`, `Button`, `Spinner` and `Icon`.

use std::collections::HashSet;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _,
    alert::Alert,
    bubble::{Bubble, BubbleContent, BubbleVariant},
    button::{Button, ButtonVariants as _},
    clipboard::Clipboard,
    h_flex,
    message::{Message, MessageAlignment, MessageContent, MessageFooter, MessageHeader},
    spinner::Spinner,
    text::TextView,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, div,
    prelude::FluentBuilder as _, rems,
};
use theorem_surface_contracts::intent::ThreadIntent;
use theorem_surface_contracts::thread::{
    HostCallState, LineMark, MessageFeedback, PartStatus, ProgramPart, ReasoningState, ThreadActor,
    ThreadMessage, ThreadMessageReason, ThreadMessageState, ThreadMessageStatus, ThreadPart,
};

use super::workbench::Workbench;

/// The transcript column's widest, in rems.
pub(super) const COLUMN: f32 = 44.;
const ACTIVITY_ROW: f32 = 1.375;

/// How a message's parts are drawn: alone, or run together with their
/// neighbors of the same kind.
#[derive(Debug, PartialEq)]
pub(super) enum Block<'a> {
    Part(&'a ThreadPart),
    /// Reasoning, progress, tool calls and their results, and host calls,
    /// folded under one summary.
    Activity(&'a [ThreadPart]),
    /// Sources, side by side.
    Citations(&'a [ThreadPart]),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Single,
    Activity,
    Citation,
}

impl Kind {
    fn of(part: &ThreadPart) -> Self {
        match part {
            ThreadPart::Reasoning(_)
            | ThreadPart::Progress { .. }
            | ThreadPart::ToolCall { .. }
            | ThreadPart::ToolResult { .. }
            | ThreadPart::HostCall(_) => Kind::Activity,
            ThreadPart::Citation { .. } => Kind::Citation,
            _ => Kind::Single,
        }
    }
}

pub(super) fn blocks(parts: &[ThreadPart]) -> Vec<Block<'_>> {
    let mut blocks = Vec::new();
    let mut start = 0;
    while let Some(first) = parts.get(start) {
        let kind = Kind::of(first);
        let end = match kind {
            Kind::Single => start + 1,
            _ => {
                start
                    + parts[start..]
                        .iter()
                        .take_while(|part| Kind::of(part) == kind)
                        .count()
            }
        };
        blocks.push(match kind {
            Kind::Single => Block::Part(first),
            Kind::Activity => Block::Activity(&parts[start..end]),
            Kind::Citation => Block::Citations(&parts[start..end]),
        });
        start = end;
    }
    blocks
}

/// How long something took, in words.
pub(super) fn duration(ms: u64) -> String {
    let seconds = ms.div_ceil(1000).max(1);
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}

/// A program part the authority left streaming reads as unfinished once its
/// message has settled, as an unfinished activity part does.
fn program_label(program: &ProgramPart, running: bool) -> String {
    let source = first_line(&program.source);
    match (program.streaming, running, source.is_empty()) {
        (true, true, _) => "Writing a program".to_owned(),
        (true, false, true) => "Unfinished program".to_owned(),
        (true, false, false) => format!("Unfinished program: {source}"),
        (false, _, _) => format!("Program: {source}"),
    }
}

fn first_line(text: &str) -> &str {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
}

fn status_word(status: PartStatus) -> &'static str {
    match status {
        PartStatus::Pending => "Waiting",
        PartStatus::Running => "Running",
        PartStatus::RequiresAction => "Needs action",
        PartStatus::Open => "Open",
        PartStatus::Complete => "Done",
        PartStatus::Incomplete => "Incomplete",
        PartStatus::Failed => "Failed",
    }
}

fn host_call_word(state: HostCallState) -> &'static str {
    match state {
        HostCallState::Pending => "Waiting",
        HostCallState::Resolved => "Done",
        HostCallState::Failed => "Failed",
        HostCallState::AwaitingApproval => "Needs approval",
    }
}

fn host_call_status(state: HostCallState) -> PartStatus {
    match state {
        HostCallState::Pending => PartStatus::Running,
        HostCallState::Resolved => PartStatus::Complete,
        HostCallState::Failed => PartStatus::Failed,
        HostCallState::AwaitingApproval => PartStatus::RequiresAction,
    }
}

/// An activity part's status as the reader sees it, for its glyph and the
/// group's liveness. Once its message has settled, a part the authority never
/// finished (a failed run leaves its tool calls pending) reads as incomplete,
/// not as still working.
fn activity_status(part: &ThreadPart, running: bool) -> PartStatus {
    let status = reported_status(part);
    if !running && status.is_live() {
        PartStatus::Incomplete
    } else {
        status
    }
}

fn reported_status(part: &ThreadPart) -> PartStatus {
    match part {
        ThreadPart::Reasoning(reasoning) => match reasoning.state {
            ReasoningState::Streaming { .. } => PartStatus::Running,
            _ => PartStatus::Complete,
        },
        ThreadPart::Progress { status, .. }
        | ThreadPart::ToolCall { status, .. }
        | ThreadPart::ToolResult { status, .. } => *status,
        ThreadPart::HostCall(call) => host_call_status(call.state),
        _ => PartStatus::Complete,
    }
}

/// What an activity row says: its label, what follows it, and what opening
/// it shows.
fn activity_text(part: &ThreadPart) -> (String, String, String) {
    match part {
        ThreadPart::Reasoning(reasoning) => match &reasoning.state {
            ReasoningState::Streaming { text } => ("Thinking".into(), String::new(), text.clone()),
            ReasoningState::Present { text } => (
                reasoning.elapsed_ms.map_or_else(
                    || "Thought".into(),
                    |ms| format!("Thought for {}", duration(ms)),
                ),
                String::new(),
                text.clone(),
            ),
            ReasoningState::Withheld { reason } => {
                ("Reasoning withheld".into(), reason.clone(), String::new())
            }
            ReasoningState::Absent => ("Reasoning".into(), String::new(), String::new()),
        },
        ThreadPart::Progress {
            label,
            lane_label,
            details,
            ..
        } => (label.clone(), lane_label.clone(), details.join("\n")),
        ThreadPart::ToolCall {
            tool_name,
            arguments_preview,
            messages,
            ..
        } => {
            let mut opened = arguments_preview.clone();
            for message in messages {
                for part in &message.parts {
                    if let ThreadPart::Text { text, .. } = part {
                        opened.push_str("\n\n");
                        opened.push_str(text);
                    }
                }
            }
            (
                tool_name.clone(),
                first_line(arguments_preview).to_owned(),
                opened.trim().to_owned(),
            )
        }
        ThreadPart::ToolResult { output_preview, .. } => (
            "Result".into(),
            first_line(output_preview).to_owned(),
            output_preview.clone(),
        ),
        ThreadPart::HostCall(call) => (call.name.clone(), call.effect.clone(), String::new()),
        _ => (String::new(), String::new(), String::new()),
    }
}

/// The folded group's summary: what is happening now, or how long it took.
/// `running` is whether its message is still being written.
pub(super) fn activity_summary(parts: &[ThreadPart], running: bool) -> (bool, String) {
    let live = parts
        .iter()
        .any(|part| activity_status(part, running).is_live());
    let tools = parts
        .iter()
        .filter(|part| matches!(part, ThreadPart::ToolCall { .. }))
        .count();
    let elapsed: u64 = parts
        .iter()
        .filter_map(|part| match part {
            ThreadPart::Reasoning(reasoning) => reasoning.elapsed_ms,
            _ => None,
        })
        .sum();
    let mut summary = if live {
        let current = parts
            .iter()
            .rev()
            .find(|part| activity_status(part, running).is_live());
        match current {
            // A progress label is already a sentence: "Working", "Searching".
            Some(ThreadPart::Progress { label, .. }) if !label.is_empty() => label.clone(),
            Some(part) => match activity_text(part).0 {
                label if label.is_empty() => "Working".to_owned(),
                label => format!("Working: {label}"),
            },
            None => "Working".to_owned(),
        }
    } else if elapsed > 0 {
        format!("Worked for {}", duration(elapsed))
    } else {
        "Worked".to_owned()
    };
    let count = if tools > 0 { tools } else { parts.len() };
    let noun = match (tools > 0, count == 1) {
        (true, true) => "tool call",
        (true, false) => "tool calls",
        (false, true) => "step",
        (false, false) => "steps",
    };
    summary.push_str(&format!(" · {count} {noun}"));
    (live, summary)
}

/// Clicking sends `intent` to the driver.
fn intent(
    workbench: &WeakEntity<Workbench>,
    intent: ThreadIntent,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let workbench = workbench.clone();
    move |_, _, cx| {
        let intent = intent.clone();
        let _ = workbench.update(cx, |this, cx| this.thread_intent(intent, cx));
    }
}

fn toggle(
    workbench: &WeakEntity<Workbench>,
    key: String,
    row: usize,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let workbench = workbench.clone();
    move |_, _, cx| {
        let key = key.clone();
        let _ = workbench.update(cx, |this, cx| this.toggle(key, row, cx));
    }
}

fn id(kind: &str, key: &str) -> SharedString {
    SharedString::from(format!("theorem-{kind}:{key}"))
}

/// What the scroller draws at `index`: the message there, in the column.
pub(super) fn row(workbench: &Entity<Workbench>, index: usize, cx: &App) -> AnyElement {
    let weak = workbench.downgrade();
    let this = workbench.read(cx);
    let driver = this.driver().read(cx);
    let Some(message) = driver.projection().thread.messages.get(index) else {
        return div().into_any_element();
    };
    let body = match message.actor {
        ThreadActor::User => user_message(message, &weak, cx),
        ThreadActor::System => system_message(message, cx),
        ThreadActor::Theorem | ThreadActor::Insight => {
            theorem_message(message, index, &this.expanded, &weak, cx)
        }
    };
    let selector = format!("theorem-message:{}", message.message_id);
    div()
        .id(id("row", &message.message_id))
        .debug_selector(move || selector)
        .w_full()
        .flex()
        .justify_center()
        .child(div().w_full().max_w(rems(COLUMN)).child(body))
        .into_any_element()
}

/// The sentence under a Theorem answer that did not finish.
///
/// The authority writes a failed run's answer as the activity it had
/// started, still running, under a status that says nothing went wrong. An
/// answer holding only activity, some of it never finished, did not finish
/// either, whatever its status says.
fn ending(status: ThreadMessageStatus, blocks: &[Block<'_>]) -> Option<&'static str> {
    let cut_off = !blocks.is_empty()
        && blocks
            .iter()
            .all(|block| matches!(block, Block::Activity(_)))
        && blocks.iter().any(|block| {
            matches!(block, Block::Activity(parts)
                if parts.iter().any(|part| reported_status(part).is_live()))
        });
    match (status.state, status.reason) {
        (ThreadMessageState::Incomplete, Some(ThreadMessageReason::Error)) => {
            Some("This answer stopped because of an error.")
        }
        (ThreadMessageState::Incomplete, Some(ThreadMessageReason::Cancelled)) => Some("Stopped."),
        (ThreadMessageState::Incomplete, Some(ThreadMessageReason::Length)) => {
            Some("This answer reached its length limit.")
        }
        (ThreadMessageState::Incomplete, _) => Some("This answer is incomplete."),
        (ThreadMessageState::Complete, _) if cut_off => Some("This answer is incomplete."),
        _ => None,
    }
}

fn joined_text(message: &ThreadMessage) -> String {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            ThreadPart::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn user_message(
    message: &ThreadMessage,
    workbench: &WeakEntity<Workbench>,
    cx: &App,
) -> AnyElement {
    let attachments = message.parts.iter().filter_map(|part| match part {
        ThreadPart::Attachment {
            part_id,
            name,
            media_type,
            state,
        } => Some(attachment(part_id, name, media_type, state, cx)),
        _ => None,
    });
    Message::new()
        .id(id("message", &message.message_id))
        .alignment(MessageAlignment::End)
        .content(
            MessageContent::new()
                .bubble(
                    Bubble::new().with_variant(BubbleVariant::Muted).content(
                        BubbleContent::new()
                            .text_color(cx.theme().foreground)
                            .child(joined_text(message)),
                    ),
                )
                .children(attachments),
        )
        .footer(actions(message, workbench, cx))
        .into_any_element()
}

fn system_message(message: &ThreadMessage, cx: &App) -> AnyElement {
    div()
        .w_full()
        .text_center()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(joined_text(message))
        .into_any_element()
}

fn theorem_message(
    message: &ThreadMessage,
    index: usize,
    expanded: &HashSet<String>,
    workbench: &WeakEntity<Workbench>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let running = message.status.state == ThreadMessageState::Running;
    let blocks = blocks(&message.parts);
    let ending = ending(message.status, &blocks);
    let mut content = MessageContent::new().gap(rems(0.375)).w_full();
    if blocks.is_empty() && running {
        content = content.child(
            h_flex()
                .gap_1p5()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(Spinner::new().xsmall().color(theme.muted_foreground))
                .child("Working"),
        );
    }
    for block in blocks {
        content = content.child(match block {
            Block::Activity(parts) => activity(parts, running, index, expanded, workbench, cx),
            Block::Citations(parts) => citations(message, parts, workbench),
            Block::Part(part) => part_view(part, running, workbench, cx),
        });
    }
    if let Some(ending) = ending {
        content = content.child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(ending),
        );
    }
    Message::new()
        .id(id("message", &message.message_id))
        .when(message.actor == ThreadActor::Insight, |this| {
            this.header(
                MessageHeader::new()
                    .content_inset(false)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(ThreadActor::Insight.label()),
            )
        })
        .content(content)
        .when(!running, |this| {
            this.footer(actions(message, workbench, cx).content_inset(false))
        })
        .into_any_element()
}

/// Copy, retry or edit, feedback and the branch picker, as the projection
/// allows each one.
fn actions(message: &ThreadMessage, workbench: &WeakEntity<Workbench>, cx: &App) -> MessageFooter {
    let key = &message.message_id;
    let allowed = &message.actions;
    let mut footer = MessageFooter::new().gap_0p5();
    if allowed.can_copy {
        footer = footer.child(
            Clipboard::new(id("copy", key))
                .value(allowed.copy_text.clone())
                .accessibility_label("Copy message")
                .tooltip("Copy"),
        );
    }
    if let (true, Some(edit_index)) = (allowed.can_reload, message.edit_index) {
        footer = footer.child(
            Button::new(id("retry", key))
                .ghost()
                .xsmall()
                .icon(Lucide::RotateCcw)
                .accessibility_label("Retry this answer")
                .tooltip("Retry")
                .on_click(intent(
                    workbench,
                    ThreadIntent::Reload {
                        message_id: key.clone(),
                        edit_index,
                    },
                )),
        );
    }
    if let (true, Some(edit_index)) = (allowed.can_edit, message.edit_index) {
        footer = footer.child(
            Button::new(id("edit", key))
                .ghost()
                .xsmall()
                .icon(Lucide::Pencil)
                .accessibility_label("Edit message")
                .tooltip("Edit")
                .on_click(intent(
                    workbench,
                    ThreadIntent::BeginEdit {
                        message_id: key.clone(),
                        edit_index,
                    },
                )),
        );
    }
    if allowed.can_feedback {
        for (feedback, icon, label) in [
            (MessageFeedback::Positive, Lucide::ThumbsUp, "Good answer"),
            (MessageFeedback::Negative, Lucide::ThumbsDown, "Poor answer"),
        ] {
            let kind = match feedback {
                MessageFeedback::Positive => "feedback-good",
                MessageFeedback::Negative => "feedback-poor",
            };
            footer = footer.child(
                Button::new(id(kind, key))
                    .ghost()
                    .xsmall()
                    .icon(icon)
                    .selected(allowed.submitted_feedback == Some(feedback))
                    .accessibility_label(label)
                    .tooltip(label)
                    .on_click(intent(
                        workbench,
                        ThreadIntent::Feedback {
                            message_id: key.clone(),
                            feedback,
                        },
                    )),
            );
        }
    }
    let branch = &message.branch;
    if branch.count >= 2 {
        let switch = |kind: &str, icon, label: &'static str, target: &Option<String>| {
            let button = Button::new(id(kind, key))
                .ghost()
                .xsmall()
                .icon(icon)
                .accessibility_label(label)
                .tooltip(label)
                .disabled(!branch.can_switch || target.is_none());
            match target {
                Some(branch_id) => button.on_click(intent(
                    workbench,
                    ThreadIntent::SwitchBranch {
                        branch_id: branch_id.clone(),
                        switch_during_run: branch.switch_during_run,
                    },
                )),
                None => button,
            }
        };
        footer = footer
            .child(switch(
                "branch-previous",
                Lucide::ChevronLeft,
                "Previous version",
                &branch.previous_branch_id,
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} of {}", branch.number, branch.count)),
            )
            .child(switch(
                "branch-next",
                Lucide::ChevronRight,
                "Next version",
                &branch.next_branch_id,
            ));
    }
    footer
}

fn activity(
    parts: &[ThreadPart],
    running: bool,
    row: usize,
    expanded: &HashSet<String>,
    workbench: &WeakEntity<Workbench>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let key = format!("group:{}", parts[0].part_id());
    let open = expanded.contains(&key);
    let (live, summary) = activity_summary(parts, running);
    let header = h_flex()
        .id(id("activity", &key))
        .debug_selector({
            let key = key.clone();
            move || format!("theorem-activity:{key}")
        })
        .h(rems(ACTIVITY_ROW))
        .gap_1p5()
        .px_1()
        .rounded(theme.radius)
        .text_xs()
        .text_color(theme.muted_foreground)
        .cursor_pointer()
        .hover(|this| this.bg(theme.muted))
        .tab_index(0)
        .aria_label(summary.clone())
        .on_click(toggle(workbench, key, row))
        .child(
            Icon::new(if open {
                Lucide::ChevronDown
            } else {
                Lucide::ChevronRight
            })
            .xsmall(),
        )
        .when(live, |this| {
            this.child(Spinner::new().xsmall().color(theme.muted_foreground))
        })
        .child(div().truncate().child(summary));
    v_flex()
        .w_full()
        .child(header)
        .when(open, |this| {
            this.child(
                v_flex()
                    .ml(rems(0.75))
                    .pl(rems(0.5))
                    .border_l_1()
                    .border_color(theme.border)
                    .children(
                        parts
                            .iter()
                            .map(|part| activity_row(part, running, row, expanded, workbench, cx)),
                    ),
            )
        })
        .into_any_element()
}

fn activity_row(
    part: &ThreadPart,
    running: bool,
    row: usize,
    expanded: &HashSet<String>,
    workbench: &WeakEntity<Workbench>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let status = activity_status(part, running);
    let (label, detail, opened) = activity_text(part);
    let word = match part {
        ThreadPart::HostCall(call) if status == reported_status(part) => host_call_word(call.state),
        _ => status_word(status),
    };
    let key = format!("part:{}", part.part_id());
    let open = expanded.contains(&key);
    let opens = !opened.trim().is_empty();
    let monospace = matches!(part, ThreadPart::ToolCall { .. } | ThreadPart::HostCall(_));
    let line = h_flex()
        .id(id("activity-row", &key))
        .h(rems(ACTIVITY_ROW))
        .w_full()
        .min_w_0()
        .gap_1p5()
        .px_1()
        .rounded(theme.radius)
        .text_xs()
        .child(glyph(status, cx))
        .child(
            div()
                .flex_shrink_0()
                .text_color(theme.foreground)
                .when(monospace, |this| {
                    this.font_family(theme.mono_font_family.clone())
                })
                .child(label.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(theme.muted_foreground)
                .when(monospace, |this| {
                    this.font_family(theme.mono_font_family.clone())
                })
                .child(detail),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_color(theme.muted_foreground)
                .child(word),
        )
        .when(opens, |this| {
            this.cursor_pointer()
                .hover(|this| this.bg(theme.muted))
                .tab_index(0)
                .aria_label(format!("{label}, {word}"))
                .on_click(toggle(workbench, key, row))
                .child(
                    Icon::new(if open {
                        Lucide::ChevronDown
                    } else {
                        Lucide::ChevronRight
                    })
                    .xsmall()
                    .text_color(theme.muted_foreground),
                )
        });
    v_flex()
        .w_full()
        .child(line)
        .when(open && opens, |this| {
            this.child(
                div()
                    .my_1()
                    .ml(rems(1.25))
                    .p_2()
                    .rounded(theme.radius)
                    .bg(theme.muted)
                    .text_xs()
                    .when(
                        monospace || matches!(part, ThreadPart::ToolResult { .. }),
                        |this| this.font_family(theme.mono_font_family.clone()),
                    )
                    .text_color(theme.foreground)
                    .child(opened),
            )
        })
        .into_any_element()
}

/// A status's glyph; its word is beside it in the row.
fn glyph(status: PartStatus, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let icon = |name: Lucide| Icon::new(name).xsmall();
    match status {
        PartStatus::Running => Spinner::new()
            .xsmall()
            .color(theme.muted_foreground)
            .into_any_element(),
        PartStatus::Pending => icon(Lucide::CircleDashed)
            .text_color(theme.muted_foreground)
            .into_any_element(),
        PartStatus::Open => icon(Lucide::CircleDot)
            .text_color(theme.muted_foreground)
            .into_any_element(),
        PartStatus::RequiresAction => icon(Lucide::Hand)
            .text_color(theme.warning)
            .into_any_element(),
        PartStatus::Complete => icon(Lucide::CircleCheck)
            .text_color(theme.muted_foreground)
            .into_any_element(),
        PartStatus::Incomplete => icon(Lucide::CircleAlert)
            .text_color(theme.muted_foreground)
            .into_any_element(),
        PartStatus::Failed => icon(Lucide::CircleX)
            .text_color(theme.danger)
            .into_any_element(),
    }
}

fn citations(
    message: &ThreadMessage,
    parts: &[ThreadPart],
    workbench: &WeakEntity<Workbench>,
) -> AnyElement {
    h_flex()
        .flex_wrap()
        .gap_1()
        .children(parts.iter().filter_map(|part| {
            match part {
                ThreadPart::Citation {
                    part_id,
                    source_id,
                    label,
                } => Some(
                    Button::new(id("citation", part_id))
                        .outline()
                        .xsmall()
                        .icon(Lucide::Link)
                        .label(label.clone())
                        .accessibility_label(format!("Open source: {label}"))
                        .on_click(intent(
                            workbench,
                            ThreadIntent::OpenCitation {
                                message_id: message.message_id.clone(),
                                part_id: part_id.clone(),
                                source_id: source_id.clone(),
                            },
                        )),
                ),
                _ => None,
            }
        }))
        .into_any_element()
}

fn attachment(part_id: &str, name: &str, media_type: &str, state: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .id(id("attachment", part_id))
        .gap_1p5()
        .px_2()
        .h(rems(1.5))
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .text_xs()
        .child(
            Icon::new(Lucide::Paperclip)
                .xsmall()
                .text_color(theme.muted_foreground),
        )
        .child(div().truncate().child(name.to_owned()))
        .when(!media_type.is_empty(), |this| {
            this.child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(media_type.to_owned()),
            )
        })
        .when(!state.is_empty() && state != "ready", |this| {
            this.child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(state.to_owned()),
            )
        })
        .into_any_element()
}

/// A part that is not one of the folded kinds.
fn part_view(
    part: &ThreadPart,
    running: bool,
    workbench: &WeakEntity<Workbench>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let note = |icon: Lucide, text: String| {
        h_flex()
            .w_full()
            .gap_1p5()
            .min_h(rems(ACTIVITY_ROW))
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(Icon::new(icon).xsmall())
            .child(div().flex_1().min_w_0().truncate().child(text))
    };
    match part {
        ThreadPart::Text { part_id, text, .. } => div()
            .w_full()
            .text_sm()
            .child(TextView::markdown(id("text", part_id), text.clone()).selectable(true))
            .into_any_element(),
        ThreadPart::Code {
            part_id,
            language,
            lines,
            source,
        } => {
            let marked = lines.iter().any(|line| line.mark != LineMark::None);
            let (language, source) = if marked {
                let source = lines
                    .iter()
                    .map(|line| {
                        let mark = match line.mark {
                            LineMark::Added => '+',
                            LineMark::Removed => '-',
                            _ => ' ',
                        };
                        format!("{mark}{}", line.text())
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                ("diff".to_owned(), source)
            } else {
                (language.clone(), source.clone())
            };
            div()
                .w_full()
                .text_sm()
                .child(
                    TextView::markdown(
                        id("code", part_id),
                        format!("```{language}\n{source}\n```"),
                    )
                    .selectable(true),
                )
                .into_any_element()
        }
        ThreadPart::Approval(approval) => {
            let outcome = approval.outcome();
            let open = outcome.is_open();
            let key = &approval.approval_id;
            v_flex()
                .w_full()
                .gap_2()
                .p_3()
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(if open { theme.warning } else { theme.border })
                .child(
                    h_flex()
                        .gap_1p5()
                        .text_sm()
                        .child(Icon::new(Lucide::Hand).small().text_color(if open {
                            theme.warning
                        } else {
                            theme.muted_foreground
                        }))
                        .child(div().flex_1().min_w_0().child(approval.summary.clone())),
                )
                .when(!approval.risk.trim().is_empty(), |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("Risk: {}", approval.risk)),
                    )
                })
                .child(if open {
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new(id("approve", key))
                                .primary()
                                .small()
                                .label("Approve")
                                .on_click(intent(
                                    workbench,
                                    ThreadIntent::Approval {
                                        approval_id: key.clone(),
                                        approved: true,
                                    },
                                )),
                        )
                        .child(
                            Button::new(id("decline", key))
                                .outline()
                                .small()
                                .label("Decline")
                                .on_click(intent(
                                    workbench,
                                    ThreadIntent::Approval {
                                        approval_id: key.clone(),
                                        approved: false,
                                    },
                                )),
                        )
                        .into_any_element()
                } else {
                    let said = match &approval.reason {
                        Some(reason) if !reason.trim().is_empty() => {
                            format!("{}: {reason}", outcome.label())
                        }
                        _ => outcome.label().to_owned(),
                    };
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(said)
                        .into_any_element()
                })
                .into_any_element()
        }
        ThreadPart::Attachment {
            part_id,
            name,
            media_type,
            state,
        } => attachment(part_id, name, media_type, state, cx),
        ThreadPart::Artifact {
            part_id,
            artifact_id,
            artifact_revision,
            artifact_kind,
            fallback_text,
            ..
        } => note(
            Lucide::Box,
            if fallback_text.trim().is_empty() {
                format!("A {artifact_kind} artifact")
            } else {
                fallback_text.clone()
            },
        )
        .child(
            Button::new(id("artifact", part_id))
                .ghost()
                .xsmall()
                .label("Open")
                .on_click(intent(
                    workbench,
                    ThreadIntent::OpenArtifact {
                        artifact_id: artifact_id.clone(),
                        artifact_revision: artifact_revision.clone(),
                    },
                )),
        )
        .into_any_element(),
        ThreadPart::GraphSelection {
            part_id,
            program_id,
            graph_revision,
            node_refs,
        } => note(
            Lucide::Network,
            match node_refs.len() {
                1 => "1 graph node selected".to_owned(),
                count => format!("{count} graph nodes selected"),
            },
        )
        .child(
            Button::new(id("graph", part_id))
                .ghost()
                .xsmall()
                .label("Show")
                .on_click(intent(
                    workbench,
                    ThreadIntent::FocusGraph {
                        program_id: program_id.clone(),
                        graph_revision: graph_revision.clone(),
                        node_refs: node_refs.clone(),
                    },
                )),
        )
        .into_any_element(),
        ThreadPart::Error {
            part_id,
            code,
            message: text,
            ..
        } => {
            let sentence = if text.is_empty() { code } else { text };
            v_flex()
                .w_full()
                .gap_1()
                .child(Alert::error(
                    id("error", part_id),
                    SharedString::from(sentence.clone()),
                ))
                .when(!code.is_empty() && !text.is_empty(), |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .font_family(theme.mono_font_family.clone())
                            .child(code.clone()),
                    )
                })
                .into_any_element()
        }
        ThreadPart::Usage {
            input_tokens,
            output_tokens,
            provider,
            model,
            model_disclosure,
            model_routing,
            ..
        } => {
            let mut text = format!("{input_tokens} tokens in, {output_tokens} out");
            if let Some(model) = model_disclosure.transcript_attribution(provider, model) {
                text.push_str(&format!(" · {model}"));
            }
            if let Some(routing) = model_routing {
                text.push_str(&format!(" · {} · requested {} · ran {}", routing.role.label(),
                    routing.requested.as_ref().map(|m| m.name()).unwrap_or_else(|| "Automatic".into()), routing.bound.name()));
                if let Some(reason) = &routing.fallback_reason { text.push_str(&format!(" · fallback: {reason}")); }
            }
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(text)
                .into_any_element()
        }
        ThreadPart::Expression { .. } => note(
            Lucide::Sparkles,
            "Theorem made a figure that Platinum can't draw yet.".to_owned(),
        )
        .into_any_element(),
        ThreadPart::ExpressionRefused { .. } => note(
            Lucide::Sparkles,
            "Theorem's figure couldn't be drawn.".to_owned(),
        )
        .into_any_element(),
        ThreadPart::Program(program) => {
            note(Lucide::Workflow, program_label(program, running)).into_any_element()
        }
        ThreadPart::TurnReceipt(receipt) => note(
            Lucide::CircleCheck,
            format!("Turn recorded as {}", receipt.receipt_ref),
        )
        .into_any_element(),
        ThreadPart::WorkspaceRequest(request) => {
            note(Lucide::Box, request.summary()).into_any_element()
        }
        ThreadPart::Unknown { variant, .. } => note(
            Lucide::CircleAlert,
            format!("Theorem sent a part ({variant}) that Platinum doesn't show yet."),
        )
        .into_any_element(),
        // Folded into activity and citation blocks before this is reached.
        ThreadPart::Reasoning(_)
        | ThreadPart::Progress { .. }
        | ThreadPart::ToolCall { .. }
        | ThreadPart::ToolResult { .. }
        | ThreadPart::HostCall(_)
        | ThreadPart::Citation { .. } => div().into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theorem_surface_contracts::thread::ReasoningPart;

    fn text(id: &str) -> ThreadPart {
        ThreadPart::Text {
            part_id: id.into(),
            text: "hello".into(),
            status: PartStatus::Complete,
        }
    }

    fn tool(id: &str, status: PartStatus) -> ThreadPart {
        ThreadPart::ToolCall {
            part_id: id.into(),
            tool_call_id: id.into(),
            tool_name: "search".into(),
            status,
            arguments_preview: "{\"q\": \"theorem\"}".into(),
            messages: Vec::new(),
        }
    }

    fn reasoning(id: &str, elapsed_ms: Option<u64>) -> ThreadPart {
        ThreadPart::Reasoning(ReasoningPart {
            part_id: id.into(),
            state: ReasoningState::Present {
                text: "because".into(),
            },
            elapsed_ms,
            expanded: false,
        })
    }

    fn citation(id: &str) -> ThreadPart {
        ThreadPart::Citation {
            part_id: id.into(),
            source_id: id.into(),
            label: id.into(),
        }
    }

    #[test]
    fn neighboring_activity_folds_and_citations_sit_together() {
        let parts = [
            reasoning("r", Some(1200)),
            tool("t1", PartStatus::Complete),
            tool("t2", PartStatus::Complete),
            text("a"),
            citation("c1"),
            citation("c2"),
            text("b"),
        ];
        let blocks = blocks(&parts);
        assert_eq!(
            blocks,
            [
                Block::Activity(&parts[0..3]),
                Block::Part(&parts[3]),
                Block::Citations(&parts[4..6]),
                Block::Part(&parts[6]),
            ]
        );
    }

    #[test]
    fn a_finished_group_says_how_long_it_worked_and_a_live_one_what_it_is_doing() {
        let parts = [reasoning("r", Some(1200)), tool("t1", PartStatus::Complete)];
        assert_eq!(
            activity_summary(&parts, false),
            (false, "Worked for 2s · 1 tool call".into())
        );

        let parts = [reasoning("r", None), tool("t1", PartStatus::Running)];
        assert_eq!(
            activity_summary(&parts, true),
            (true, "Working: search · 1 tool call".into())
        );

        let parts = [reasoning("r", None)];
        assert_eq!(
            activity_summary(&parts, false),
            (false, "Worked · 1 step".into())
        );
    }

    #[test]
    fn a_progress_label_is_the_summary_and_a_settled_turn_stops_working() {
        let progress = ThreadPart::Progress {
            part_id: "p".into(),
            label: "Working".into(),
            lane_id: String::new(),
            lane_label: String::new(),
            status: PartStatus::Running,
            details: Vec::new(),
        };
        assert_eq!(
            activity_summary(std::slice::from_ref(&progress), true),
            (true, "Working · 1 step".into())
        );

        // A failed run leaves its last tool call running; once the message
        // has settled, nothing in it reads as live.
        let parts = [progress, tool("t1", PartStatus::Running)];
        assert_eq!(
            activity_summary(&parts, false),
            (false, "Worked · 1 tool call".into())
        );
        assert_eq!(activity_status(&parts[1], false), PartStatus::Incomplete);
    }

    #[test]
    fn a_settled_turn_is_not_still_writing_its_program() {
        let program = |source: &str, streaming| ProgramPart {
            part_id: "program".into(),
            source: source.into(),
            streaming,
        };
        assert_eq!(program_label(&program("", true), true), "Writing a program");
        assert_eq!(
            program_label(&program("", true), false),
            "Unfinished program"
        );
        assert_eq!(
            program_label(&program("\n(search \"sky\")\n", true), false),
            "Unfinished program: (search \"sky\")"
        );
        assert_eq!(
            program_label(&program("(answer)", false), false),
            "Program: (answer)"
        );
    }

    #[test]
    fn an_answer_cut_off_in_its_activity_reads_as_incomplete() {
        let written = ThreadMessageStatus::default();
        let progress = ThreadPart::Progress {
            part_id: "p".into(),
            label: "Working".into(),
            lane_id: String::new(),
            lane_label: String::new(),
            status: PartStatus::Running,
            details: Vec::new(),
        };
        // How the authority writes a failed run's answer.
        assert_eq!(
            ending(written, &blocks(std::slice::from_ref(&progress))),
            Some("This answer is incomplete.")
        );
        // Finished activity alone, or activity followed by an answer, says
        // nothing more.
        assert_eq!(
            ending(written, &blocks(&[tool("t", PartStatus::Complete)])),
            None
        );
        assert_eq!(
            ending(written, &blocks(&[progress.clone(), text("a")])),
            None
        );
        // An ending the runtime knows is said as it is.
        let failed = ThreadMessageStatus {
            state: ThreadMessageState::Incomplete,
            reason: Some(ThreadMessageReason::Error),
        };
        assert_eq!(
            ending(failed, &blocks(std::slice::from_ref(&progress))),
            Some("This answer stopped because of an error.")
        );
    }

    #[test]
    fn durations_read_in_seconds_then_minutes() {
        assert_eq!(duration(0), "1s");
        assert_eq!(duration(59_000), "59s");
        assert_eq!(duration(61_000), "1m 1s");
    }
}
