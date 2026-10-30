//! The composer at the card's foot: an inset field on its own fill, with what
//! goes along with the next send (attachments, a quote, an edit, queued
//! sends) above the text, and the send or stop control at its trailing edge.
//!
//! The composer document decides what may be sent; this only draws its
//! answer and says the reason when the answer is no.
//!
//! SOURCING: GPUI Kit's `Textarea` and `Button`.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Textarea,
    v_flex,
};
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _, rems,
};
use theorem_surface_contracts::composer::{
    AttachmentState, ComposerAttachment, ComposerMode, ComposerQueueLane, SendAffordance,
};
use theorem_surface_contracts::intent::ComposerIntent;

use super::transcript::COLUMN;
use super::workbench::{StopRun, Workbench};

fn id(kind: &str, key: &str) -> SharedString {
    SharedString::from(format!("theorem-composer-{kind}:{key}"))
}

impl Workbench {
    pub(super) fn render_composer(&self, _: &mut Window, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let composer = &self.driver().read(cx).projection().composer;
        let text = self.composer.read(cx).value().to_string();
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

        let attachments = (!composer.attachments.is_empty()).then(|| {
            h_flex().flex_wrap().gap_1().children(
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

        let field = v_flex()
            .id("theorem-composer")
            .debug_selector(|| "theorem-composer".into())
            .w_full()
            .gap_1p5()
            .p_2()
            .rounded(theme.radius_lg)
            .bg(theme.secondary)
            .border_1()
            .border_color(theme.border)
            .children(edit)
            .children(quote)
            .children(queue)
            .children(attachments)
            .child(
                Textarea::new(&self.composer)
                    .appearance(false)
                    .aria_label("Message to Theorem"),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("theorem-composer-attach")
                            .ghost()
                            .small()
                            .icon(Lucide::Paperclip)
                            .accessibility_label("Attach files")
                            .tooltip("Attach files")
                            .disabled(!composer.allows_attachments())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.composer_intent(ComposerIntent::PickAttachment, cx)
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .children(reason),
                    )
                    .child(send),
            );

        div()
            .w_full()
            .flex()
            .justify_center()
            .px_4()
            .pb_3()
            .pt_1()
            .child(div().w_full().max_w(rems(COLUMN)).child(field))
            .into_any_element()
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
        h_flex()
            .h(rems(1.5))
            .gap_1()
            .pl_2()
            .rounded(theme.radius)
            .border_1()
            .border_color(if failed { theme.danger } else { theme.border })
            .bg(theme.background)
            .text_xs()
            .child(
                Icon::new(if failed {
                    Lucide::CircleAlert
                } else {
                    Lucide::Paperclip
                })
                .xsmall()
                .text_color(if failed {
                    theme.danger
                } else {
                    theme.muted_foreground
                }),
            )
            .child(
                div()
                    .max_w(rems(12.))
                    .truncate()
                    .child(attachment.name.clone()),
            )
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
