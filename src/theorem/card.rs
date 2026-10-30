//! The work card: level 1, beside the navigator, opaque so dense text never
//! sits on glass (`docs/plans/theorem-workbench.md`, Layout and States). Its
//! outline runs around the composer, a bump of the frame at its foot
//! (`bump.rs`).
//!
//! SOURCING: GPUI Kit's `MessageScroller`, `Empty`, `Skeleton`, `Spinner`,
//! `Button` and `DropdownMenu`; the card's shape is `bump::card`. The fade
//! under the header mirrors `MessageScroller`'s own above the composer,
//! which has no counterpart at the top.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    empty::{
        Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant,
        EmptyTitle,
    },
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    message_scroller::MessageScroller,
    skeleton::Skeleton,
    spinner::Spinner,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StyleRefinement, Styled as _, Window, div, linear_color_stop, linear_gradient,
    prelude::FluentBuilder as _, px, relative, rems,
};
use theorem_surface_contracts::intent::ThreadIntent;
use theoremweb_agent_runtime::ChatHostPhase;
use theoremweb_agent_runtime::chat_host::SIGN_IN_NOTICE;

use super::driver::Connection;
use super::transcript::{self, COLUMN};
use super::workbench::{NewConversation, StopRun, Workbench};
use crate::browser::FRAME;
use crate::bump::{self, Slot};

const HEADER: f32 = 2.25;

/// A row under the card's content, the composer or what goes with it,
/// centered under what it answers.
pub(super) fn under_content(element: impl IntoElement) -> Div {
    div().w_full().flex().justify_center().px_4().child(element)
}

/// What the card's body shows, from the connection and the host's phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Body {
    NotConnected,
    Connecting,
    SignedOut,
    Refused,
    /// Ready, with nothing said yet.
    Empty,
    Transcript,
}

impl Body {
    pub(super) fn of(connection: Connection, phase: ChatHostPhase, messages: usize) -> Self {
        match (connection, phase) {
            (Connection::NotConfigured, _) => Body::NotConnected,
            (_, ChatHostPhase::Unauthenticated) => Body::SignedOut,
            (_, ChatHostPhase::Refused) => Body::Refused,
            // A conversation already on screen stays while the next loads.
            (_, ChatHostPhase::Unhydrated | ChatHostPhase::Loading) if messages == 0 => {
                Body::Connecting
            }
            _ if messages == 0 => Body::Empty,
            _ => Body::Transcript,
        }
    }

    /// Whether the card draws the composer under this body.
    pub(super) fn has_composer(self) -> bool {
        matches!(self, Body::Empty | Body::Transcript)
    }
}

impl Workbench {
    pub(super) fn body(&self, cx: &App) -> Body {
        let driver = self.driver().read(cx);
        let projection = driver.projection();
        Body::of(
            driver.connection(),
            projection.phase,
            projection.thread.messages.len(),
        )
    }

    pub(super) fn render_card(&self, window: &mut Window, cx: &Context<Self>) -> AnyElement {
        let body = self.body(cx);
        let bottom = Slot::default();
        let content = match body {
            Body::NotConnected => self.not_connected(cx),
            Body::Connecting => skeleton_transcript(),
            Body::SignedOut => self.signed_out(cx),
            Body::Refused => self.refused(cx),
            Body::Empty => self.ready(false, bottom.clone(), window, cx),
            Body::Transcript => self.ready(true, bottom.clone(), window, cx),
        };
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .pt(px(FRAME))
            .pr(px(FRAME))
            .pb(px(FRAME))
            .child(
                v_flex()
                    .id("theorem-card")
                    .debug_selector(|| "theorem-card".into())
                    .relative()
                    .size_full()
                    .min_h_0()
                    // The theme's raised surface, edged with the line it
                    // draws between a sidebar and its content. In the light
                    // theme the general border is the frame's own color and
                    // would vanish against the navigator.
                    .child(bump::card(None, body.has_composer().then_some(bottom), cx))
                    .child(self.card_header(cx))
                    .child(div().flex_1().min_h_0().child(content)),
            )
            .into_any_element()
    }

    fn card_header(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let driver = self.driver().read(cx);
        let projection = driver.projection();
        let thread = &projection.thread;
        let title = match thread.title.trim() {
            "" if thread.messages.is_empty() => "New conversation".to_owned(),
            "" => "Untitled conversation".to_owned(),
            title => title.to_owned(),
        };
        let project = projection
            .selected_project_id
            .as_deref()
            .and_then(|id| projection.projects.iter().find(|p| p.project_id == id))
            .map(|project| project.title.clone());
        let word = Self::run_state_word(thread.run_state);
        let streaming = thread.run_state.is_live();
        let stoppable = self.can_stop(cx);
        let connection = driver.connection();
        let ready = projection.phase == ChatHostPhase::Ready;
        let signs_in = driver.sign_in_url().is_some();
        let workbench = cx.entity().downgrade();

        h_flex()
            .flex_shrink_0()
            .h(rems(HEADER))
            .gap_2()
            .px_1p5()
            .text_sm()
            .child(
                Button::new("theorem-card-menu")
                    .debug_selector(|| "theorem-card-menu".into())
                    .ghost()
                    .small()
                    .min_w_0()
                    .font_semibold()
                    .label(title)
                    .dropdown_caret(true)
                    .tooltip("Conversation options")
                    .dropdown_menu_with_anchor(gpui_kit::Anchor::TopLeft, move |menu, _, _| {
                        let new = workbench.clone();
                        let retry = workbench.clone();
                        let demo = workbench.clone();
                        let sign_in = workbench.clone();
                        menu.item(
                            PopupMenuItem::new("New conversation")
                                .icon(Lucide::SquarePen)
                                .action(Box::new(NewConversation))
                                .disabled(!ready)
                                .on_click(move |_, window, cx| {
                                    let _ = new
                                        .update(cx, |this, cx| this.new_conversation(window, cx));
                                }),
                        )
                        .separator()
                        .when(signs_in, |menu| {
                            menu.item(
                                PopupMenuItem::new("Open sign-in page")
                                    .icon(Lucide::LogIn)
                                    .on_click(move |_, _, cx| {
                                        let _ = sign_in.update(cx, |this, cx| {
                                            this.driver()
                                                .update(cx, |driver, cx| driver.open_sign_in(cx))
                                        });
                                    }),
                            )
                        })
                        .when(connection != Connection::Demo, |menu| {
                            menu.item(
                                PopupMenuItem::new("Reconnect")
                                    .icon(Lucide::RotateCcw)
                                    .on_click(move |_, _, cx| {
                                        let _ = retry.update(cx, |this, cx| {
                                            this.driver().update(cx, |driver, cx| driver.retry(cx))
                                        });
                                    }),
                            )
                        })
                        .item(
                            PopupMenuItem::new(if connection == Connection::Demo {
                                "Restart the demo"
                            } else {
                                "Try the demo"
                            })
                            .icon(Lucide::Sparkles)
                            .on_click(move |_, _, cx| {
                                let _ = demo.update(cx, |this, cx| {
                                    this.driver().update(cx, |driver, cx| driver.use_demo(cx))
                                });
                            }),
                        )
                    }),
            )
            .children(project.map(|project| {
                div()
                    .flex_shrink_0()
                    .max_w(rems(14.))
                    .truncate()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(project)
            }))
            .child(div().flex_1())
            .children(word.map(|word| {
                h_flex()
                    .flex_shrink_0()
                    .gap_1p5()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .when(streaming, |this| {
                        this.child(Spinner::new().xsmall().color(theme.muted_foreground))
                    })
                    .child(word)
            }))
            .when(stoppable, |this| {
                this.child(
                    Button::new("theorem-card-stop")
                        .debug_selector(|| "theorem-card-stop".into())
                        .outline()
                        .xsmall()
                        .icon(Lucide::Square)
                        .label("Stop")
                        .tooltip_with_action("Stop the run", &StopRun, None)
                        .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                )
            })
            .into_any_element()
    }

    fn ready(
        &self,
        transcript: bool,
        composer: Slot,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let projection = self.driver().read(cx).projection();
        let notice = projection.notice.clone();
        let suggestions = &projection.thread.suggestions;
        let suggestions = (!suggestions.is_empty()).then(|| {
            h_flex()
                .flex_wrap()
                .gap_1()
                .children(suggestions.iter().map(|suggestion| {
                    let intent = ThreadIntent::Suggestion {
                        suggestion_id: suggestion.suggestion_id.clone(),
                        text: suggestion.text.clone(),
                        action: suggestion.action,
                    };
                    Button::new(SharedString::from(format!(
                        "theorem-suggestion:{}",
                        suggestion.suggestion_id
                    )))
                    .outline()
                    .xsmall()
                    .label(suggestion.text.clone())
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.thread_intent(intent.clone(), cx)),
                    )
                }))
        });
        let above_composer =
            |element: AnyElement| under_content(div().w_full().max_w(rems(COLUMN)).child(element));
        // The transcript eases out under the header, as it does above the
        // composer, once more of it lies above.
        let scrolled = transcript && {
            let top = self.transcript.read(cx).logical_scroll_top();
            top.item_ix > 0 || top.offset_in_item > px(0.)
        };

        let body: AnyElement = if transcript {
            let this = cx.entity();
            MessageScroller::new(
                "theorem-transcript",
                self.transcript.clone(),
                move |index, _, cx| transcript::row(&this, index, cx),
            )
            .with_row_style({
                let mut style = StyleRefinement::default();
                style.padding.left = Some(rems(1.).into());
                style.padding.right = Some(rems(1.).into());
                style.padding.bottom = Some(rems(1.).into());
                style
            })
            .with_bottom_fade(theme.popover)
            .into_any_element()
        } else {
            self.render_starters(cx)
        };

        v_flex()
            .size_full()
            .min_h_0()
            .child(
                div()
                    .id("theorem-card-body")
                    .debug_selector(move || {
                        if transcript {
                            "theorem-transcript"
                        } else {
                            "theorem-empty"
                        }
                        .into()
                    })
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(body)
                    .when(scrolled, |body| {
                        body.child(div().absolute().top_0().left_0().right_0().h(rems(1.5)).bg(
                            linear_gradient(
                                180.,
                                linear_color_stop(theme.popover, 0.),
                                linear_color_stop(theme.popover.opacity(0.), 1.),
                            ),
                        ))
                    }),
            )
            .children(notice.map(|notice| {
                above_composer(
                    h_flex()
                        .gap_1p5()
                        .py_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(Icon::new(Lucide::CircleAlert).xsmall())
                        .child(div().flex_1().min_w_0().child(notice))
                        .into_any_element(),
                )
            }))
            .children(suggestions.map(|row| above_composer(row.pb_1().into_any_element())))
            .child(self.render_composer(composer, window, cx))
            .into_any_element()
    }

    fn not_connected(&self, cx: &Context<Self>) -> AnyElement {
        centered(
            Lucide::Network,
            "Theorem isn't connected",
            "Set THEOREMWEB_AUTHORITY_URL to your Theorem address and \
             THEOREMWEB_AUTH_TOKEN_FILE to a token file only you can read, in the .env \
             file in Platinum's data folder. Then check again.",
            h_flex()
                .gap_2()
                .child(
                    Button::new("theorem-try-demo")
                        .debug_selector(|| "theorem-try-demo".into())
                        .primary()
                        .small()
                        .label("Try the demo")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.driver().update(cx, |driver, cx| driver.use_demo(cx))
                        })),
                )
                .child(
                    Button::new("theorem-check-again")
                        .outline()
                        .small()
                        .label("Check again")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.driver().update(cx, |driver, cx| driver.retry(cx))
                        })),
                )
                .into_any_element(),
        )
    }

    fn signed_out(&self, cx: &Context<Self>) -> AnyElement {
        let driver = self.driver().read(cx);
        let notice = driver
            .projection()
            .notice
            .clone()
            .unwrap_or_else(|| SIGN_IN_NOTICE.to_owned());
        let signs_in = driver.sign_in_url().is_some();
        centered(
            Lucide::LogIn,
            "Sign in to Theorem",
            notice,
            h_flex()
                .gap_2()
                .when(signs_in, |this| {
                    this.child(
                        Button::new("theorem-sign-in")
                            .primary()
                            .small()
                            .label("Open sign-in page")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.driver()
                                    .update(cx, |driver, cx| driver.open_sign_in(cx))
                            })),
                    )
                })
                .child(
                    Button::new("theorem-signed-in")
                        .outline()
                        .small()
                        .label("Check again")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.driver().update(cx, |driver, cx| driver.retry(cx))
                        })),
                )
                .into_any_element(),
        )
    }

    fn refused(&self, cx: &Context<Self>) -> AnyElement {
        let notice = self
            .driver()
            .read(cx)
            .projection()
            .notice
            .clone()
            .unwrap_or_else(|| "Theorem didn't answer.".to_owned());
        centered(
            Lucide::TriangleAlert,
            "Theorem couldn't load",
            notice,
            Button::new("theorem-retry")
                .outline()
                .small()
                .label("Retry")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.driver().update(cx, |driver, cx| driver.retry(cx))
                }))
                .into_any_element(),
        )
    }
}

fn centered(
    icon: Lucide,
    title: &'static str,
    description: impl Into<SharedString>,
    actions: AnyElement,
) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_6()
        .child(
            Empty::new()
                .max_w_96()
                .header(
                    EmptyHeader::new()
                        .media(
                            EmptyMedia::new()
                                .with_variant(EmptyMediaVariant::Icon)
                                .child(Icon::new(icon)),
                        )
                        .title(EmptyTitle::new().child(title))
                        .description(EmptyDescription::new().child(description.into())),
                )
                .content(EmptyContent::new().child(actions)),
        )
        .into_any_element()
}

/// The transcript's shape while the conversation loads: a reader's turn,
/// then an answer's lines.
fn skeleton_transcript() -> AnyElement {
    let line = |fraction: f32| Skeleton::new().h(rems(0.875)).w(relative(fraction));
    div()
        .size_full()
        .flex()
        .justify_center()
        .px_4()
        .pt_6()
        .child(
            v_flex()
                .w_full()
                .max_w(rems(COLUMN))
                .gap_2()
                .child(
                    h_flex()
                        .justify_end()
                        .child(Skeleton::new().h(rems(2.)).w(relative(0.4))),
                )
                .child(line(0.9))
                .child(line(0.75))
                .child(line(0.82))
                .child(line(0.5)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_shows_what_the_connection_and_phase_allow() {
        use ChatHostPhase::*;
        assert_eq!(
            Body::of(Connection::NotConfigured, Ready, 3),
            Body::NotConnected
        );
        assert_eq!(
            Body::of(Connection::Native, Unhydrated, 0),
            Body::Connecting
        );
        assert_eq!(Body::of(Connection::Native, Loading, 0), Body::Connecting);
        assert_eq!(Body::of(Connection::Native, Loading, 2), Body::Transcript);
        assert_eq!(
            Body::of(Connection::Native, Unauthenticated, 2),
            Body::SignedOut
        );
        assert_eq!(Body::of(Connection::Native, Refused, 0), Body::Refused);
        assert_eq!(Body::of(Connection::Demo, Ready, 0), Body::Empty);
        assert_eq!(Body::of(Connection::Demo, Ready, 4), Body::Transcript);
    }
}
