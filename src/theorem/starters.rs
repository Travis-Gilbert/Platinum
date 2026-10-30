//! The card's first view, before anything is said: "What should we get
//! done?", a role to start from, and a few starters for it. Picking one
//! types its prompt into the composer, where Return sends it.
//!
//! The starters are Platinum's own text, not the authority's suggestions
//! (`ThreadDocument::suggestions`, drawn above the composer). They ask
//! only for what Theorem does with what the reader gives it: none of them
//! reads a calendar, mail or chat, which no connector here provides.
//!
//! SOURCING: GPUI Kit's `TabBar` (segmented) and `Button`; the composer is
//! filled through the chat host's `ThreadIntent::Suggestion`, as its own
//! suggestions are.

use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    scroll::ScrollableElement as _,
    tab::{Tab, TabBar},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Styled as _, Window, div, prelude::FluentBuilder as _, px, relative, rems,
};
use theorem_surface_contracts::intent::ThreadIntent;
use theorem_surface_contracts::thread::ThreadSuggestionAction;

use super::transcript::COLUMN;
use super::workbench::Workbench;

/// Who the reader is starting as, which picks the starters shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Role {
    #[default]
    Developer,
    Designer,
    Marketing,
    Manager,
    Student,
    Office,
    Founder,
}

impl Role {
    pub(super) const ALL: [Role; 7] = [
        Role::Developer,
        Role::Designer,
        Role::Marketing,
        Role::Manager,
        Role::Student,
        Role::Office,
        Role::Founder,
    ];

    fn label(self) -> &'static str {
        match self {
            Role::Developer => "Developer",
            Role::Designer => "Designer",
            Role::Marketing => "Marketing",
            Role::Manager => "Manager",
            Role::Student => "Student",
            Role::Office => "Office",
            Role::Founder => "Founder",
        }
    }

    pub(super) fn starters(self) -> [Starter; 2] {
        match self {
            Role::Developer => [
                Starter {
                    id: "review-change",
                    title: "Review a change",
                    description: "Read a diff or pull request for bugs, risks and missing tests.",
                    prompt: "Review this change for bugs, risky edge cases and missing tests:\n\n",
                    sketch: Sketch::Code,
                },
                Starter {
                    id: "explain-codebase",
                    title: "Explain a codebase",
                    description: "Map a repository's modules, entry points and how data moves.",
                    prompt: "Explain how this codebase is organized: its main modules, entry points and how data moves between them. The repository: ",
                    sketch: Sketch::Board,
                },
            ],
            Role::Designer => [
                Starter {
                    id: "critique-screen",
                    title: "Critique a screen",
                    description: "Hierarchy, spacing, contrast and copy, the biggest problems first.",
                    prompt: "Critique this screen's hierarchy, spacing, contrast and copy, the biggest problems first: ",
                    sketch: Sketch::Board,
                },
                Starter {
                    id: "write-ux-copy",
                    title: "Write interface copy",
                    description: "Labels, empty states and error messages in one voice.",
                    prompt: "Write the interface copy for this flow: labels, empty states and error messages, in one voice: ",
                    sketch: Sketch::Lines,
                },
            ],
            Role::Marketing => [
                Starter {
                    id: "plan-launch",
                    title: "Plan a launch",
                    description: "Audience, channels, messages and a timeline for a feature.",
                    prompt: "Plan a launch for this feature, with its audience, channels, key messages and a timeline: ",
                    sketch: Sketch::Checklist,
                },
                Starter {
                    id: "campaign-email",
                    title: "Draft a campaign email",
                    description: "A short email with a subject line and two variants to test.",
                    prompt: "Draft a campaign email, with a subject line and two variants to test, about: ",
                    sketch: Sketch::Lines,
                },
            ],
            Role::Manager => [
                Starter {
                    id: "weekly-status",
                    title: "Weekly status",
                    description: "Turn notes into progress, risks and what comes next.",
                    prompt: "Turn these notes into a weekly status update, with progress, risks and next steps:\n\n",
                    sketch: Sketch::Lines,
                },
                Starter {
                    id: "plan-meeting",
                    title: "Plan a meeting",
                    description: "An agenda with a goal, time boxes and the decisions to make.",
                    prompt: "Plan an agenda for this meeting, with its goal, time boxes and the decisions it has to make: ",
                    sketch: Sketch::Checklist,
                },
            ],
            Role::Student => [
                Starter {
                    id: "study-topic",
                    title: "Study a topic",
                    description: "An outline of the key ideas, then questions to test yourself.",
                    prompt: "Help me study this topic: outline its key ideas, then quiz me on them one question at a time. The topic: ",
                    sketch: Sketch::Checklist,
                },
                Starter {
                    id: "explain-paper",
                    title: "Explain a paper",
                    description: "Its question, method, findings and limits, plainly.",
                    prompt: "Explain this paper plainly: its question, method, main findings and limitations: ",
                    sketch: Sketch::Lines,
                },
            ],
            Role::Office => [
                Starter {
                    id: "plan-day",
                    title: "Plan my day",
                    description: "Put a list of tasks in order and fit them into your hours.",
                    prompt: "Here is what I need to do today. Put it in order of priority and fit it into the hours I have:\n\n",
                    sketch: Sketch::Checklist,
                },
                Starter {
                    id: "tidy-document",
                    title: "Tidy a document",
                    description: "Make a draft clearer and shorter without changing what it says.",
                    prompt: "Rewrite this to be clearer and shorter without changing what it says:\n\n",
                    sketch: Sketch::Lines,
                },
            ],
            Role::Founder => [
                Starter {
                    id: "pressure-test",
                    title: "Pressure-test an idea",
                    description: "Its riskiest assumptions, and a cheap way to test each.",
                    prompt: "Pressure-test this idea: list its riskiest assumptions and a cheap way to test each one: ",
                    sketch: Sketch::Board,
                },
                Starter {
                    id: "investor-update",
                    title: "Draft an investor update",
                    description: "Highlights, metrics, lowlights and asks, from your notes.",
                    prompt: "Draft a monthly investor update with highlights, key metrics, lowlights and asks, from these notes:\n\n",
                    sketch: Sketch::Lines,
                },
            ],
        }
    }
}

/// One way to start: what it is, in a title and a sentence, and the prompt
/// it types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Starter {
    id: &'static str,
    title: &'static str,
    description: &'static str,
    pub(super) prompt: &'static str,
    sketch: Sketch,
}

/// The shape a starter's work takes, drawn small at the top of its card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sketch {
    /// Paragraphs.
    Lines,
    /// Rows to tick.
    Checklist,
    /// Columns of cards.
    Board,
    /// Indented code.
    Code,
}

impl Workbench {
    /// The starter whose prompt the composer begins with, if any: the one
    /// last picked, until its text is edited away.
    fn picked(&self, cx: &App) -> Option<Starter> {
        let text = self.composer.read(cx).value();
        self.role
            .starters()
            .into_iter()
            .find(|starter| text.starts_with(starter.prompt))
    }

    fn pick(&mut self, starter: Starter, window: &mut Window, cx: &mut Context<Self>) {
        self.thread_intent(
            ThreadIntent::Suggestion {
                suggestion_id: format!("platinum-starter:{}", starter.id),
                text: starter.prompt.to_owned(),
                action: ThreadSuggestionAction::Replace,
            },
            cx,
        );
        self.focus_composer(window, cx);
    }

    pub(super) fn set_role(&mut self, role: Role, cx: &mut Context<Self>) {
        if self.role != role {
            self.role = role;
            cx.notify();
        }
    }

    pub(super) fn render_starters(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let picked = self.picked(cx);
        let roles = TabBar::new("theorem-roles")
            .segmented()
            .small()
            .selected_index(
                Role::ALL
                    .iter()
                    .position(|role| *role == self.role)
                    .unwrap_or_default(),
            )
            .children(Role::ALL.map(|role| Tab::new().label(SharedString::from(role.label()))))
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                if let Some(role) = Role::ALL.get(*index) {
                    this.set_role(*role, cx);
                }
            }));
        let cards = h_flex().w_full().gap_3().children(
            self.role
                .starters()
                .map(|starter| self.starter_card(starter, picked == Some(starter), cx)),
        );
        let column = v_flex()
            .w_full()
            .max_w(rems(COLUMN))
            .items_center()
            .gap_4()
            .child(
                v_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_2xl()
                            .font_semibold()
                            .child("What should we get done?"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Start from one of these, or ask for anything below."),
                    ),
            )
            .child(roles)
            .child(cards)
            .child(
                div()
                    .h(rems(1.25))
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .when(picked.is_some(), |hint| {
                        hint.child("Press Return to start, or add what matters first.")
                    }),
            );
        div()
            .id("theorem-starters")
            .debug_selector(|| "theorem-starters".into())
            .size_full()
            .overflow_y_scrollbar()
            .child(
                v_flex()
                    .min_h_full()
                    .justify_center()
                    .items_center()
                    .px_6()
                    .py_6()
                    .child(column),
            )
            .into_any_element()
    }

    fn starter_card(&self, starter: Starter, picked: bool, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        Button::new(SharedString::from(format!(
            "theorem-starter-{}",
            starter.id
        )))
        .debug_selector(move || format!("theorem-starter-{}", starter.id))
        .ghost()
        .flex_1()
        .min_w_0()
        .h_auto()
        .p_0()
        .accessibility_label(format!("{}: {}", starter.title, starter.description))
        .child(
            v_flex()
                .w_full()
                .p_2()
                .gap_2()
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(if picked { theme.primary } else { theme.border })
                .bg(theme.popover)
                .child(sketch(starter.sketch, cx))
                .child(
                    v_flex()
                        .px_1()
                        .pb_1()
                        .gap_0p5()
                        .items_start()
                        .child(div().text_sm().font_semibold().child(starter.title))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .whitespace_normal()
                                .text_left()
                                .child(starter.description),
                        ),
                ),
        )
        .on_click(cx.listener(move |this, _, window, cx| this.pick(starter, window, cx)))
    }
}

/// A starter's sketch: a few bars on the theme's quiet fill, in the shape
/// of the work.
fn sketch(sketch: Sketch, cx: &App) -> Div {
    let theme = cx.theme();
    let (ink, radius) = (theme.border, theme.radius);
    let bar = move |width: f32| div().h(px(5.)).w(relative(width)).rounded(radius).bg(ink);
    let frame = div()
        .h(rems(5.))
        .w_full()
        .p_3()
        .rounded(theme.radius)
        .bg(theme.secondary)
        .overflow_hidden();
    match sketch {
        Sketch::Lines => frame.child(
            v_flex()
                .gap_2()
                .child(bar(0.45))
                .child(bar(0.9))
                .child(bar(0.8))
                .child(bar(0.6)),
        ),
        Sketch::Checklist => {
            frame.child(v_flex().gap_2().children([0.7, 0.55, 0.8].map(|width| {
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .size(px(8.))
                            .flex_shrink_0()
                            .rounded(px(2.))
                            .border_1()
                            .border_color(ink),
                    )
                    .child(bar(width))
            })))
        }
        Sketch::Board => frame.child(h_flex().size_full().gap_2().children([2, 3, 1].map(
            |cards| {
                v_flex()
                    .flex_1()
                    .gap_1p5()
                    .child(bar(0.6))
                    .children((0..cards).map(|_| {
                        div()
                            .h(px(10.))
                            .w_full()
                            .rounded(radius)
                            .border_1()
                            .border_color(ink)
                    }))
            },
        ))),
        Sketch::Code => frame.child(
            v_flex().gap_2().children(
                [(0., 0.4), (12., 0.5), (24., 0.35), (12., 0.3), (0., 0.15)]
                    .map(|(indent, width)| div().pl(px(indent)).child(bar(width))),
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::Role;

    #[test]
    fn every_role_has_starters_with_their_own_ids_and_prompts() {
        let starters: Vec<_> = Role::ALL.iter().flat_map(|role| role.starters()).collect();
        for (index, starter) in starters.iter().enumerate() {
            assert!(!starter.prompt.trim().is_empty(), "{}", starter.id);
            for other in &starters[index + 1..] {
                assert_ne!(starter.id, other.id);
                // A prompt that began with another would mark both picked.
                assert!(
                    !starter.prompt.starts_with(other.prompt)
                        && !other.prompt.starts_with(starter.prompt),
                    "{} and {}",
                    starter.id,
                    other.id
                );
            }
        }
    }
}
