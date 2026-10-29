//! Chrome for the spatial canvas: the toolbar, the omnibar and its
//! suggestions, the two action strips, zoom, layers, a card's caption and
//! the site panel.
//!
//! Every component is stateless. It takes plain values through builder
//! methods and reports what the person did through handler closures, the way
//! GPUI Kit's own components do. None reads a Platinum action, a canvas entity or
//! a global, so the canvas backend decides what each handler means. The one
//! retained piece is the omnibar's text, which GPUI Kit keeps in an
//! `InputState` the owner holds (`Omnibar::input` makes it).
//!
//! The board is the window's clear glass, as the frame around the cards is in
//! Tabs mode, and cards stay opaque. The floating pieces share one surface,
//! `bar`: GPUI Kit's popover shape and shadow over `field`, the theme's color
//! nearly opaque over the glass, so what people read never sits on the glass
//! alone. No component picks a fill of its own; they ask `frame` or `field`,
//! which are `glass::frame` and `glass::field`, directly or through `surface`.
//! Without the glass both keep the theme's solid colors.
//!
//! Wherever a page's host is shown (a caption, a suggestion row, the site
//! panel's header) it is written without a leading `www.` (`display_host`),
//! kept whole while the title is cut, and dropped when there is no room for it
//! beside the start of the title (`titled_row`).
//!
//! SOURCING: GPUI Kit's `Button`, `TabBar`, `DropdownMenu`/`PopupMenu`,
//! `Input`, `ListItem`, `Popover`, `Switch`, `Separator` and `Tooltip`, and
//! its `popover_style` surface; each file names its own parts. Platinum's icons
//! (`crate::icons`) draw every glyph GPUI Kit's default set lacks.

mod caption;
mod layers;
mod omnibar;
mod site_panel;
mod strip;
mod toolbar;
mod zoom;

pub use caption::{Caption, CaptionState};
pub use layers::{Layer, Layers};
pub use omnibar::{Omnibar, Suggestion, SuggestionGroup};
pub use site_panel::{FontSpecimen, SitePanel};
pub use strip::{ArrangeAs, FocusedStrip, SelectionStrip};
pub use toolbar::{InsertKind, Mode, Tool, Toolbar, insert_menu};
pub use zoom::Zoom;

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, ThemeStyled as _, h_flex};
use gpui_kit::{
    App, Div, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _, px, rems,
};

pub(crate) use crate::glass::{field, frame};

/// A handler that is told what changed.
pub(crate) type Handler<T> = Rc<dyn Fn(T, &mut Window, &mut App)>;

/// A handler for a command that has nothing to report.
pub(crate) type Command = Rc<dyn Fn(&mut Window, &mut App)>;

/// The floating surface under what people read or press: GPUI Kit's popover
/// shape and shadow on `field`, wrapped around `base`. It blocks the pointer,
/// so the board under it does not see clicks meant for a control.
pub(crate) fn surface(base: Div, cx: &App) -> Div {
    let radius = cx.theme().radius_tokens().xl;
    base.popover_style(cx)
        .bg(field(cx))
        .rounded(radius)
        .occlude()
}

/// `surface` around a row of controls, with a small inset.
pub(crate) fn bar(cx: &App) -> Div {
    surface(h_flex().items_center().gap_1().p_1(), cx)
}

/// A page's host as a person reads it: without the `www.` nearly every site
/// carries, so `www.vitsoe.com` is `vitsoe.com`. A host that is only `www.`
/// stays as it is.
pub(crate) fn display_host(host: &str) -> SharedString {
    let host = host.trim();
    let bare = match host.split_at_checked(4) {
        Some((prefix, rest)) if prefix.eq_ignore_ascii_case("www.") && !rest.is_empty() => rest,
        _ => host,
    };
    SharedString::from(bare.to_owned())
}

/// The room, icon included, that a title keeps beside its host. With less
/// than this the host is dropped, since a title cut to a couple of letters
/// says less about the page than the host adds.
const TITLE_MIN: Pixels = px(72.);

/// One line of a lead (a favicon or glyph), a title and, in grey, the page's
/// host: "Vitsœ  vitsoe.com". Pass the host through `display_host`.
///
/// The title gives way first. It is cut with an ellipsis while the host stays
/// whole, and when `TITLE_MIN` of icon and title will not fit beside the host,
/// the host is dropped and the title keeps the line.
///
/// Flex has no "drop this if it does not fit", so the line is a wrapping box
/// one line tall that clips what overflows. The icon and title are one item
/// whose flex basis is `TITLE_MIN`, which is the width the line is broken on.
/// The host, last in order, wraps onto a second line, clipped away, when the
/// first is full. The icon-and-title item then grows into what the first line
/// has left, so with room to spare the host sits at the line's far end.
///
/// The item is also at least `TITLE_MIN` wide. Without that a short title
/// ("Vitsœ") would make a box that is shrink-wrapped to its content narrower
/// than the width the line is broken on, and the host would be dropped from a
/// box that had room for it. A title that short gets a little air after it
/// where the box is only as wide as its content.
pub(crate) fn titled_row(
    lead: impl IntoElement,
    title: Div,
    host: Option<SharedString>,
    cx: &App,
) -> Div {
    let muted = cx.theme().muted_foreground;
    h_flex()
        .flex_wrap()
        .content_start()
        .min_w_0()
        .h(rems(1.25))
        .line_height(rems(1.25))
        .gap_2()
        .overflow_hidden()
        .child(
            h_flex()
                .gap_2()
                .flex_basis(TITLE_MIN)
                .min_w(TITLE_MIN)
                .flex_grow(1.)
                .flex_shrink(1.)
                .child(div().flex_shrink_0().child(lead))
                .child(title.flex_1().min_w_0().truncate().text_sm()),
        )
        .when_some(host, |row, host| {
            row.child(
                div()
                    .flex_shrink_0()
                    .whitespace_nowrap()
                    .text_xs()
                    .text_color(muted)
                    .child(host),
            )
        })
}

/// A ghost button's selected fill is a theme token a few percent off the
/// pill under it, which does not read as chosen. A chosen control takes a tint
/// of the theme's primary color and its text.
pub(crate) fn chosen(button: Button, chosen: bool, cx: &App) -> Button {
    if !chosen {
        return button;
    }
    let primary = cx.theme().primary;
    button.custom(
        ButtonCustomVariant::new(cx)
            .color(primary.opacity(0.18))
            .foreground(primary)
            .hover(primary.opacity(0.26))
            .active(primary.opacity(0.26)),
    )
}

#[cfg(test)]
mod tests {
    use super::display_host;

    #[test]
    fn display_host_drops_a_leading_www_only() {
        assert_eq!(display_host("www.vitsoe.com").as_ref(), "vitsoe.com");
        assert_eq!(display_host("WWW.Vitsoe.com").as_ref(), "Vitsoe.com");
        assert_eq!(display_host("vitsoe.com").as_ref(), "vitsoe.com");
        assert_eq!(display_host("shop.www.vitsoe.com").as_ref(), "shop.www.vitsoe.com");
        assert_eq!(display_host("www2.vitsoe.com").as_ref(), "www2.vitsoe.com");
        assert_eq!(display_host("www.").as_ref(), "www.");
        assert_eq!(display_host(" www.vitsoe.com ").as_ref(), "vitsoe.com");
        assert_eq!(display_host("").as_ref(), "");
    }
}
