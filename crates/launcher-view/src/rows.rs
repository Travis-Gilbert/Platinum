//! The results: a heading per lane and a row per item, with the selected
//! row filled so it reads first.

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Div, InteractiveElement as _, ParentElement as _, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use launcher::{Item, Kind, Lane, Session};

use crate::Launcher;

/// The list's share of the window's width.
const WIDTH: f32 = 360.;

/// The glyph for a kind of item, from the whole Lucide catalog: the host
/// registers `gpui_kit::assets::AllAssets`, as Platinum does.
pub(crate) fn icon(kind: Kind) -> IconName {
    match kind {
        Kind::App => IconName::AppWindow,
        Kind::Command => IconName::Command,
        Kind::Conversation => IconName::MessageSquare,
        Kind::File => IconName::FileText,
        Kind::Folder => IconName::Folder,
        Kind::Link => IconName::ExternalLink,
        Kind::WebSearch => IconName::Globe,
        Kind::Ask => IconName::Sparkles,
    }
}

/// The rows, or what to say when there are none.
pub(crate) fn list(launcher: &Launcher, cx: &mut Context<Launcher>) -> Stateful<Div> {
    let session = &launcher.session;
    let selected = session.selected().map(|item| item.id.clone());
    let mut column = v_flex()
        .id("theo-launcher-rows")
        .flex_none()
        .w(px(WIDTH))
        .h_full()
        .px(px(8.))
        .pb(px(8.))
        .overflow_y_scroll()
        .track_scroll(&launcher.rows);
    if session.count() == 0 {
        return column.child(nothing(session, cx));
    }
    for section in session.sections() {
        column = column.child(heading(section.lane, cx));
        for item in &section.items {
            let lit = selected.as_ref() == Some(&item.id);
            column = column.child(row(item, lit, cx));
        }
    }
    column
}

fn heading(lane: Lane, cx: &App) -> Div {
    div()
        .flex_none()
        .px(px(10.))
        .pt(px(12.))
        .pb(px(5.))
        .text_size(px(11.5))
        .text_color(cx.theme().muted_foreground)
        .child(lane.label())
}

fn row(item: &Item, lit: bool, cx: &mut Context<Launcher>) -> Stateful<Div> {
    let theme = cx.theme();
    let (ink, quiet, tile, hover) = if lit {
        (
            theme.primary_foreground,
            theme.primary_foreground.opacity(0.78),
            theme.primary_foreground.opacity(0.2),
            theme.primary,
        )
    } else {
        (
            theme.popover_foreground,
            theme.muted_foreground,
            theme.muted,
            theme.accent,
        )
    };
    let fill = theme.primary;
    let id = item.id.clone();
    let verb = item.primary().label();
    h_flex()
        .id(SharedString::from(item.id.as_str().to_owned()))
        .flex_none()
        .h(px(46.))
        .px(px(10.))
        .gap(px(10.))
        .rounded(px(9.))
        .text_color(ink)
        .when(lit, |row| row.bg(fill))
        .hover(move |row| row.bg(hover))
        .on_click(
            cx.listener(move |launcher, event: &gpui_kit::ClickEvent, window, cx| {
                launcher.clicked(&id, event.click_count() >= 2, window, cx);
            }),
        )
        .child(
            div()
                .flex_none()
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .bg(tile)
                .child(Icon::new(icon(item.kind)).size(px(15.)).text_color(ink)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .child(SharedString::from(item.title.clone())),
                )
                .when(!item.detail.is_empty(), |words| {
                    words.child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(quiet)
                            .child(SharedString::from(item.detail.clone())),
                    )
                }),
        )
        .child(if lit {
            // What Enter does here, said before it is pressed.
            h_flex()
                .flex_none()
                .gap(px(6.))
                .text_size(px(12.))
                .child(verb)
                .child(
                    div()
                        .px(px(5.))
                        .rounded(px(4.))
                        .bg(tile)
                        .text_size(px(11.))
                        .child("↵"),
                )
        } else {
            // Where it lives, so a file here is not taken for a record there.
            h_flex()
                .flex_none()
                .text_size(px(11.5))
                .text_color(quiet)
                .child(item.origin.label())
        })
}

fn nothing(session: &Session, cx: &App) -> Div {
    let words: SharedString = if session.query().trim().is_empty() {
        "Find an application, a file, a command or a conversation, or ask Theo.".into()
    } else if let Some(lane) = session.looking() {
        lane.looking().into()
    } else {
        format!(
            "Nothing under {} answers to “{}”.",
            session.scope().label(),
            session.query().trim()
        )
        .into()
    };
    div()
        .px(px(10.))
        .pt(px(18.))
        .text_color(cx.theme().muted_foreground)
        .child(words)
}

/// The footer's right side while finding: a lane still looking, a lane that
/// could not look, or how many rows there are.
pub(crate) fn status(session: &Session, cx: &App) -> Div {
    let theme = cx.theme();
    if let Some(lane) = session.looking() {
        return h_flex().gap(px(6.)).child(lane.looking());
    }
    if let Some((lane, why)) = session.unavailable().into_iter().next() {
        // Said in the lane's place; the other lanes still answer.
        return h_flex()
            .gap(px(6.))
            .text_color(theme.warning)
            .child(SharedString::from(format!("{}: {why}", lane.label())));
    }
    let count = session.count();
    let words = match count {
        0 => return div(),
        1 => "1 result".to_owned(),
        count => format!("{count} results"),
    };
    h_flex().child(SharedString::from(words))
}
