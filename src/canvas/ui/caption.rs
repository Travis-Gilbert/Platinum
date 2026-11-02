//! A card's caption: favicon, title, and the host in grey ("Vitsœ  vitsoe.com"),
//! drawn under the card. It is the card's drag handle, so it shows the grab
//! cursor, and the closed one while the card is being dragged.
//!
//! The host is written without a leading `www.`, stays whole while a long title
//! is cut with an ellipsis, and is dropped when the caption is too narrow for
//! both (`titled_row`).
//!
//! SOURCING: hand-roll. GPUI Kit has no component for a label that is a drag
//! handle on a canvas; this is a `div` carrying the theme's tokens, with the
//! fill from `field` because the caption sits on the board's glass and its text
//! must never read against the glass alone.

use std::sync::Arc;

use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex};
use gpui_kit::{
    App, ElementId, FontWeight, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, RenderImage, RenderOnce, SharedString, StyleRefinement, Styled, Window,
    div, img, prelude::FluentBuilder as _, transparent_black,
};

use super::{display_host, field, titled_row};
use crate::icons;

/// How the card the caption belongs to stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptionState {
    /// Nothing is happening to the card. A pointer over the caption still
    /// shows the hover treatment.
    #[default]
    Idle,
    /// The pointer is over the card.
    Hovered,
    /// The card is in the selection.
    Selected,
    /// The card has the keyboard and the pointer's attention: it is the one
    /// card being read.
    Focused,
}

#[derive(IntoElement)]
pub struct Caption {
    id: ElementId,
    title: SharedString,
    host: Option<SharedString>,
    favicon: Option<Arc<RenderImage>>,
    state: CaptionState,
    dragging: bool,
    on_press: Option<std::rc::Rc<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Caption {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            host: None,
            favicon: None,
            state: CaptionState::Idle,
            dragging: false,
            on_press: None,
            style: StyleRefinement::default(),
        }
    }

    /// The address's host, shown after the title in grey without its leading
    /// `www.`.
    pub fn host(mut self, host: impl AsRef<str>) -> Self {
        self.host = Some(display_host(host.as_ref()));
        self
    }

    /// The page's icon; a generic page glyph stands in without one.
    pub fn favicon(mut self, favicon: Option<Arc<RenderImage>>) -> Self {
        self.favicon = favicon;
        self
    }

    pub fn state(mut self, state: CaptionState) -> Self {
        self.state = state;
        self
    }

    /// The card is being dragged by its caption.
    pub fn dragging(mut self, dragging: bool) -> Self {
        self.dragging = dragging;
        self
    }

    /// The primary button went down on the caption: the owner starts the drag.
    pub fn on_press(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_press = Some(std::rc::Rc::new(handler));
        self
    }
}

impl Styled for Caption {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Caption {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let radius = theme.radius_tokens().md;
        // Selected is a neutral, bright outline; Focused is the primary color,
        // heavier type and the primary color on the title.
        let (outline, weight, ink) = match self.state {
            CaptionState::Idle => (transparent_black(), FontWeight::NORMAL, theme.foreground),
            CaptionState::Hovered => (theme.border, FontWeight::NORMAL, theme.foreground),
            CaptionState::Selected => (
                theme.foreground.opacity(0.6),
                FontWeight::NORMAL,
                theme.foreground,
            ),
            CaptionState::Focused => (theme.primary, FontWeight::SEMIBOLD, theme.primary),
        };
        let hover_outline = theme.border;
        let idle = self.state == CaptionState::Idle;
        let lead = match self.favicon {
            Some(favicon) => img(favicon).size_4().flex_shrink_0().rounded(radius / 2.).into_any_element(),
            None => Icon::new(icons::page())
                .small()
                .text_color(theme.muted_foreground)
                .into_any_element(),
        };
        let on_press = self.on_press;

        h_flex()
            .id(self.id)
            .max_w_full()
            .px_2()
            .py_1()
            .rounded(radius)
            .border_1()
            .border_color(outline)
            .bg(field(cx))
            .text_color(theme.foreground)
            .when(idle, |caption| {
                caption.hover(move |style| style.border_color(hover_outline))
            })
            .when(self.dragging, |caption| caption.cursor_grabbing())
            .when(!self.dragging, |caption| caption.cursor_grab())
            .when_some(on_press, |caption, on_press| {
                caption.on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    on_press(event, window, cx);
                })
            })
            .refine_style(&self.style)
            .child(
                titled_row(
                    lead,
                    div()
                        .font_weight(weight)
                        .text_color(ink)
                        .child(self.title),
                    self.host,
                    cx,
                )
                .flex_grow(1.),
            )
    }
}
