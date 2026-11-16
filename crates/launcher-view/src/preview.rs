//! Beside the rows: what is known about the selected one, and what can be
//! done with it.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Div, IntoElement as _, ParentElement as _, SharedString, Styled as _, div, px,
};
use launcher::{Item, Kind, Verb};

use crate::{Launcher, rows};

pub(crate) fn pane(launcher: &Launcher, cx: &mut Context<Launcher>) -> Div {
    let border = cx.theme().border;
    let pane = v_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_hidden()
        .border_l_1()
        .border_color(border)
        .p(px(20.))
        .gap(px(16.));
    let Some(item) = launcher.session.selected() else {
        return pane;
    };
    // The verbs keep their room; what is known gives way when it is long.
    pane.child(title(item, cx))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(body(item, cx)),
        )
        .child(verbs(item, cx))
}

fn title(item: &Item, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .flex_none()
        .gap(px(14.))
        .child(
            div()
                .flex_none()
                .size(px(52.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(12.))
                .bg(theme.muted)
                .child(
                    Icon::new(rows::icon(item.kind))
                        .size(px(26.))
                        .text_color(theme.popover_foreground),
                ),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(5.))
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .line_clamp(2)
                        .child(SharedString::from(item.title.clone())),
                )
                .child(
                    h_flex()
                        .gap(px(8.))
                        .text_size(px(12.))
                        .text_color(theme.muted_foreground)
                        .child(
                            div()
                                .px(px(7.))
                                .py(px(1.))
                                .rounded(px(5.))
                                .bg(theme.muted)
                                .text_color(theme.popover_foreground)
                                .child(item.kind.label()),
                        )
                        .child(item.origin.label()),
                ),
        )
}

/// The facts, the passage, or for a row the launcher made itself, what
/// choosing it will do.
fn body(item: &Item, cx: &App) -> Div {
    let theme = cx.theme();
    let mut column = v_flex().flex_none().gap(px(8.));
    for fact in &item.facts {
        column = column.child(
            h_flex()
                .items_start()
                .gap(px(12.))
                .text_size(px(13.))
                .child(
                    div()
                        .flex_none()
                        .w(px(72.))
                        .text_color(theme.muted_foreground)
                        .child(SharedString::from(fact.name.clone())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .line_clamp(3)
                        .child(SharedString::from(fact.value.clone())),
                ),
        );
    }
    if let Some(excerpt) = &item.excerpt {
        column = column.child(
            div()
                .mt(px(4.))
                .p(px(12.))
                .rounded(px(9.))
                .bg(theme.muted)
                .text_size(px(12.5))
                .line_height(px(19.))
                .line_clamp(7)
                .child(SharedString::from(excerpt.clone())),
        );
    }
    let says: Option<SharedString> = match item.kind {
        Kind::Ask => Some(
            format!(
                "{}. Nothing is sent until you choose this row.",
                item.detail
            )
            .into(),
        ),
        Kind::WebSearch | Kind::Link => Some("Opens in a Platinum tab.".into()),
        _ if !item.has_preview() && !item.detail.is_empty() => Some(item.detail.clone().into()),
        _ => None,
    };
    column.when_some(says, |column, says| {
        column.child(
            div()
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(theme.muted_foreground)
                .child(says),
        )
    })
}

/// A button for each thing that can be done. The first is what Enter does.
fn verbs(item: &Item, cx: &mut Context<Launcher>) -> Div {
    h_flex()
        .flex_none()
        .flex_wrap()
        .gap(px(8.))
        .children(item.verbs().iter().enumerate().map(|(at, verb)| {
            let verb = *verb;
            let label = match key(verb, at == 0) {
                Some(key) => format!("{}  {key}", verb.label()),
                None => verb.label().to_owned(),
            };
            let button = Button::new(SharedString::from(format!("verb-{}", verb.label())))
                .small()
                .label(SharedString::from(label))
                .on_click(cx.listener(move |launcher, _, window, cx| {
                    launcher.choose(verb, window, cx);
                    launcher.focus(window, cx);
                }));
            if at == 0 {
                button.primary().into_any_element()
            } else {
                button.outline().into_any_element()
            }
        }))
}

/// The key that does `verb`, when there is one.
fn key(verb: Verb, first: bool) -> Option<&'static str> {
    match verb {
        _ if first => Some("↵"),
        Verb::AskAbout => Some("⌘↵"),
        Verb::Reveal => Some("⌘R"),
        Verb::CopyPath => Some("⇧⌘C"),
        _ => None,
    }
}
