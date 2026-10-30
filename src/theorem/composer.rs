//! The composer: the bump at the card's foot (`bump.rs`), with what goes
//! along with the next send (an edit, a quote, queued sends, and what the
//! send will use: its project, an edit's references and attachments) and
//! the reason a send is refused above one row of attach, the text and the
//! send or stop control.
//!
//! At rest it is narrower than the column; in use (focused, written in, or
//! carrying something) it widens to the column and shows what the send will
//! use. It grows upward as the text does, its base on the card's foot.
//!
//! The composer document decides what may be sent; this only draws its
//! answer and says the reason when the answer is no.
//!
//! SOURCING: GPUI Kit's `Textarea`, `Button` and `Separator`; the widths at
//! rest and in use follow Zephium's work bar (`WorkBar.svelte`).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Textarea,
    separator::Separator,
    v_flex,
};
use gpui_kit::{
    AnyElement, Context, Div, Focusable as _, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Styled as _, Window, div, prelude::FluentBuilder as _, rems,
};
use theorem_surface_contracts::composer::{
    AttachmentState, ComposerAttachment, ComposerMode, ComposerQueueLane, SendAffordance,
};
use theorem_surface_contracts::intent::ComposerIntent;

use super::card::under_content;
use super::transcript::COLUMN;
use super::workbench::{StopRun, Workbench};
use crate::bump::Slot;

/// The composer's width at rest, in rems; in use it widens to the column's.
const AT_REST: f32 = 39.;

fn id(kind: &str, key: &str) -> SharedString {
    SharedString::from(format!("theorem-composer-{kind}:{key}"))
}

impl Workbench {
    /// The composer, drawn into `slot` so the card's outline runs around it.
    pub(super) fn render_composer(
        &self,
        slot: Slot,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let projection = self.driver().read(cx).projection();
        let composer = &projection.composer;
        let text = self.composer.read(cx).value().to_string();
        let edit_chips = composer
            .edit
            .as_ref()
            .map_or(&[][..], |edit| edit.chips.as_slice());
        let engaged = self.composer.focus_handle(cx).contains_focused(window, cx)
            || !text.trim().is_empty()
            || !composer.attachments.is_empty()
            || composer.quote.is_some()
            || composer.mode == ComposerMode::Edit;
        let affordance = composer.affordance(&text);
        let stoppable = composer.can_cancel();
        // An empty field is not worth a sentence: the field's placeholder
        // already says what to do.
        let reason = match affordance {
            _ if !composer.refusal.trim().is_empty() => Some(composer.refusal.clone()),
            SendAffordance::Blocked(block)
                if !text.trim().is_empty() || !composer.attachments.is_empty() =>
            {
                Some(block.reason().to_owned())
            }
            _ => None,
        };

        let edit = (composer.mode == ComposerMode::Edit).then(|| {
            h_flex()
                .gap_1p5()
                .px_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(Icon::new(Lucide::Pencil).xsmall())
                .child(div().flex_1().child("Editing a message"))
                .child(
                    Button::new("theorem-composer-end-edit")
                        .ghost()
                        .xsmall()
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.composer_intent(ComposerIntent::EndEdit, cx)
                        })),
                )
        });

        let quote = composer.quote.as_ref().map(|quote| {
            h_flex()
                .gap_1p5()
                .pl_2()
                .border_l_2()
                .border_color(theme.border)
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(quote.text.clone()),
                )
                .child(
                    Button::new("theorem-composer-quote-dismiss")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::X)
                        .accessibility_label("Remove quote")
                        .tooltip("Remove quote")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.composer_intent(ComposerIntent::DismissQuote, cx)
                        })),
                )
        });

        let queue = (!composer.queue.is_empty()).then(|| {
            v_flex()
                .gap_0p5()
                .children(composer.queue.iter().map(|item| {
                    let queue_item_id = item.queue_item_id.clone();
                    let steer_id = item.queue_item_id.clone();
                    h_flex()
                        .h(rems(1.5))
                        .gap_1p5()
                        .px_1()
                        .text_xs()
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(theme.muted_foreground)
                                .child(match item.lane {
                                    ComposerQueueLane::Queue => "Queued",
                                    ComposerQueueLane::Steer => "Steering",
                                }),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(item.text.clone()))
                        .when(item.lane == ComposerQueueLane::Queue, |this| {
                            this.child(
                                Button::new(id("queue-steer", &item.queue_item_id))
                                    .ghost()
                                    .xsmall()
                                    .label("Send now")
                                    .tooltip("Send this into the current run")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.composer_intent(
                                            ComposerIntent::MoveQueueItem {
                                                queue_item_id: steer_id.clone(),
                                                lane: ComposerQueueLane::Steer,
                                            },
                                            cx,
                                        )
                                    })),
                            )
                        })
                        .child(
                            Button::new(id("queue-remove", &item.queue_item_id))
                                .ghost()
                                .xsmall()
                                .icon(Lucide::X)
                                .accessibility_label("Remove queued message")
                                .tooltip("Remove")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.composer_intent(
                                        ComposerIntent::RemoveQueueItem {
                                            queue_item_id: queue_item_id.clone(),
                                        },
                                        cx,
                                    )
                                })),
                        )
                }))
        });

        // What the send will use: the project it goes to, the references an
        // edit lifted from its message, and the attachments.
        let project = projection
            .selected_project_id
            .as_deref()
            .and_then(|id| projection.projects.iter().find(|p| p.project_id == id));
        let uses = project.is_some() || !edit_chips.is_empty() || !composer.attachments.is_empty();
        let context = (engaged && uses).then(|| {
            h_flex()
                .id("theorem-composer-context")
                .debug_selector(|| "theorem-composer-context".into())
                .flex_wrap()
                .gap_1()
                .children(project.map(|project| {
                    chip(Lucide::FolderOpen, None, project.title.clone(), cx)
                        .pr_2()
                        .debug_selector(|| "theorem-composer-context-project".into())
                }))
                .children(edit_chips.iter().map(|reference| {
                    let icon = if reference.resource_path.is_some() {
                        Lucide::Paperclip
                    } else {
                        Lucide::Link
                    };
                    chip(icon, None, reference.label.clone(), cx).pr_2()
                }))
                .children(
                    composer
                        .attachments
                        .iter()
                        .map(|attachment| self.attachment_chip(attachment, cx)),
                )
        });

        let send: AnyElement = if stoppable {
            Button::new("theorem-composer-stop")
                .small()
                .icon(Lucide::Square)
                .accessibility_label("Stop")
                .tooltip_with_action("Stop", &StopRun, None)
                .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
                .into_any_element()
        } else {
            Button::new("theorem-composer-send")
                .debug_selector(|| "theorem-composer-send".into())
                .primary()
                .small()
                .icon(Lucide::ArrowUp)
                .accessibility_label(affordance.label())
                .tooltip(affordance.label())
                .disabled(!affordance.is_send())
                .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
                .into_any_element()
        };

        let reason = reason.map(|reason| {
            div()
                .px_1()
                .truncate()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(reason)
        });

        let field = v_flex()
            .id("theorem-composer")
            .debug_selector(|| "theorem-composer".into())
            .relative()
            .w_full()
            .max_w(rems(if engaged { COLUMN } else { AT_REST }))
            .gap_1p5()
            .px_2()
            .pt_2()
            .pb_2()
            .child(slot.recorder())
            .children(edit)
            .children(quote)
            .children(queue)
            .children(context)
            .children(reason)
            .child(Button::new("theorem-model-settings").ghost().xsmall()
                .label(super::models::routing_label(cx))
                .tooltip("Choose workflow role and models")
                .on_click(cx.listener(|_, _, window, cx| super::models::open(window, cx))))
            .child(
                h_flex()
                    .items_end()
                    .gap_1()
                    .child(
                        Button::new("theorem-composer-attach")
                            .ghost()
                            .small()
                            .icon(Lucide::Plus)
                            .accessibility_label("Attach files")
                            .tooltip("Attach files")
                            .disabled(!composer.allows_attachments())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.composer_intent(ComposerIntent::PickAttachment, cx)
                            })),
                    )
                    .child(
                        Button::new("theorem-workspace-context")
                            .ghost()
                            .small()
                            .icon(Lucide::Layers)
                            .accessibility_label("Attach selected workspace sources")
                            .tooltip("Attach selected page links and notes")
                            .disabled(!composer.allows_attachments())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.driver().update(cx, |_, cx| {
                                    cx.emit(super::driver::DriverEvent::AttachWorkspace)
                                })
                            })),
                    )
                    .child(Separator::vertical().h_4().mb_1())
                    .child(
                        div().flex_1().min_w_0().child(
                            Textarea::new(&self.composer)
                                .appearance(false)
                                .aria_label("Message to Theorem"),
                        ),
                    )
                    .child(send),
            );

        // Flush with the card's foot, so the bump rises from it.
        under_content(field).pt_1().into_any_element()
    }

    fn attachment_chip(&self, attachment: &ComposerAttachment, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let key = &attachment.attachment_id;
        let failed = attachment.state.has_failed();
        let detail = match &attachment.state {
            AttachmentState::Ready => None,
            state => state.detail(),
        };
        let remove_id = key.clone();
        let retry_id = key.clone();
        let (icon, alarm) = if failed {
            (Lucide::CircleAlert, Some(theme.danger))
        } else {
            (Lucide::Paperclip, None)
        };
        chip(icon, alarm, attachment.name.clone(), cx)
            .children(detail.map(|detail| div().text_color(theme.muted_foreground).child(detail)))
            .when(failed, |this| {
                this.child(
                    Button::new(id("attachment-retry", key))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::RotateCcw)
                        .accessibility_label(format!("Try attaching {} again", attachment.name))
                        .tooltip("Try again")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.composer_intent(
                                ComposerIntent::RetryAttachment {
                                    attachment_id: retry_id.clone(),
                                },
                                cx,
                            )
                        })),
                )
            })
            .child(
                Button::new(id("attachment-remove", key))
                    .ghost()
                    .xsmall()
                    .icon(Lucide::X)
                    .accessibility_label(format!("Remove {}", attachment.name))
                    .tooltip("Remove")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.composer_intent(
                            ComposerIntent::RemoveAttachment {
                                attachment_id: remove_id.clone(),
                            },
                            cx,
                        )
                    })),
            )
            .into_any_element()
    }
}

/// A chip for one thing a send will use: its icon and name, edged and
/// tinted with `alarm` when it failed.
fn chip(
    icon: Lucide,
    alarm: Option<Hsla>,
    name: impl Into<SharedString>,
    cx: &Context<Workbench>,
) -> Div {
    let theme = cx.theme();
    h_flex()
        .h(rems(1.5))
        .gap_1()
        .pl_2()
        .rounded(theme.radius)
        .border_1()
        .border_color(alarm.unwrap_or(theme.border))
        .bg(theme.background)
        .text_xs()
        .child(
            Icon::new(icon)
                .xsmall()
                .text_color(alarm.unwrap_or(theme.muted_foreground)),
        )
        .child(div().max_w(rems(12.)).truncate().child(name.into()))
}
