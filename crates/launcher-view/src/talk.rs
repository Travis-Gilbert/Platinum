//! The conversation, in the launcher's window: who is being talked to, what
//! is attached, what was said, and a way on to the full workspace.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, Div, InteractiveElement as _, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use crate::{Event, Launcher};

/// Who said a turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Person,
    Theo,
}

/// One thing said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub who: Who,
    pub text: String,
}

/// Something attached to the message being written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    /// The host's name for it, handed back to take it off.
    pub id: String,
    pub name: String,
}

/// The conversation as the host has it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Talk {
    /// The conversation's title, or `None` when the next message starts a
    /// new one.
    pub title: Option<String>,
    pub turns: Vec<Turn>,
    pub attachments: Vec<Attachment>,
    /// Whether Theo is at work on it now.
    pub working: bool,
    /// Why a message cannot be sent now, when it cannot. A message written
    /// meanwhile stays in the field.
    pub refusing: Option<String>,
}

pub(crate) fn pane(launcher: &Launcher, cx: &mut Context<Launcher>) -> Div {
    let talk = &launcher.talk;
    v_flex()
        .size_full()
        .child(bar(launcher, cx))
        .when(!talk.attachments.is_empty(), |pane| {
            pane.child(attachments(launcher, cx))
        })
        .child(turns(launcher, cx))
}

/// Which conversation this is, and the ways out of it.
fn bar(launcher: &Launcher, cx: &mut Context<Launcher>) -> Div {
    let talk = &launcher.talk;
    let muted = cx.theme().muted_foreground;
    let title: SharedString = match &talk.title {
        Some(title) => title.clone().into(),
        None => "New conversation".into(),
    };
    // A conversation with nothing in it is already new.
    let fresh = talk.title.is_none() && talk.turns.is_empty();
    h_flex()
        .flex_none()
        .h(px(44.))
        .px(px(18.))
        .gap(px(10.))
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(12.))
                .text_color(muted)
                .child("Theorem"),
        )
        .child(div().flex_1())
        .when(!fresh, |bar| {
            bar.child(
                Button::new("theo-new-conversation")
                    .small()
                    .ghost()
                    .label("New Conversation")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::NewConversation))),
            )
        })
        .child(
            Button::new("theo-open-workspace")
                .small()
                .outline()
                .label("Open in Workspace")
                .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::OpenWorkspace))),
        )
}

/// What goes with the next message, each with a way to take it off.
fn attachments(launcher: &Launcher, cx: &mut Context<Launcher>) -> Div {
    let theme = cx.theme();
    let (muted, ink, quiet) = (
        theme.muted,
        theme.popover_foreground,
        theme.muted_foreground,
    );
    h_flex()
        .flex_none()
        .flex_wrap()
        .px(px(18.))
        .pb(px(10.))
        .gap(px(6.))
        .children(launcher.talk.attachments.iter().map(|attachment| {
            let id = attachment.id.clone();
            h_flex()
                .h(px(26.))
                .pl(px(9.))
                .pr(px(4.))
                .gap(px(5.))
                .rounded(px(7.))
                .bg(muted)
                .text_size(px(12.5))
                .text_color(ink)
                .child(
                    Icon::new(IconName::Paperclip)
                        .size(px(13.))
                        .text_color(quiet),
                )
                .child(
                    div()
                        .max_w(px(220.))
                        .truncate()
                        .child(SharedString::from(attachment.name.clone())),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("detach-{}", attachment.id)))
                        .size(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .cursor_pointer()
                        .hover(move |close| close.bg(quiet.opacity(0.25)))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(Event::RemoveAttachment(id.clone()));
                        }))
                        .child(Icon::new(IconName::Close).size(px(11.)).text_color(quiet)),
                )
        }))
}

fn turns(launcher: &Launcher, cx: &mut Context<Launcher>) -> gpui_kit::Stateful<Div> {
    let talk = &launcher.talk;
    let theme = cx.theme();
    let column = v_flex()
        .id("theo-launcher-turns")
        .flex_1()
        .min_h_0()
        .px(px(18.))
        .pb(px(14.))
        .gap(px(12.))
        .overflow_y_scroll()
        .track_scroll(&launcher.turns);
    if talk.turns.is_empty() {
        let words: SharedString = match &talk.refusing {
            Some(why) => why.clone().into(),
            None if talk.attachments.is_empty() => "Say what you need.".into(),
            None => "Say what you need to know about what is attached.".into(),
        };
        return column.child(
            div()
                .pt(px(6.))
                .text_color(theme.muted_foreground)
                .child(words),
        );
    }
    column
        .children(talk.turns.iter().map(|turn| {
            let text = SharedString::from(turn.text.clone());
            match turn.who {
                Who::Person => h_flex().flex_none().justify_end().child(
                    div()
                        .max_w(px(520.))
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(12.))
                        .bg(theme.muted)
                        .line_height(px(21.))
                        .child(text),
                ),
                Who::Theo => h_flex()
                    .flex_none()
                    .child(div().max_w(px(640.)).line_height(px(21.)).child(text)),
            }
        }))
        .when_some(
            talk.refusing.clone().filter(|_| !talk.turns.is_empty()),
            |column, why| {
                column.child(
                    div()
                        .flex_none()
                        .text_size(px(12.5))
                        .text_color(theme.muted_foreground)
                        .child(SharedString::from(why)),
                )
            },
        )
}

/// The footer's right side while talking: that Theo is at work, with the
/// way to stop it. Stopping is its own act; closing the window is not it.
pub(crate) fn status(launcher: &Launcher, cx: &mut Context<Launcher>) -> Div {
    if !launcher.talk.working {
        return div();
    }
    h_flex().gap(px(8.)).child("Theo is working").child(
        Button::new("theo-stop")
            .xsmall()
            .danger()
            .label("Stop  ⌘.")
            .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Stop))),
    )
}
