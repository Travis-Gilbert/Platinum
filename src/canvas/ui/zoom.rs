//! Zoom: zoom out, the percentage, zoom in.
//!
//! SOURCING: GPUI Kit's ghost `Button` for the two steps, drawn with the
//! `Minus` and `Plus` glyphs its default assets carry. The percentage is plain
//! text, a reading and not a control.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, IconName};
use gpui_kit::{App, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window, div};

use super::{Command, bar};

#[derive(IntoElement)]
pub struct Zoom {
    scale: f32,
    at_min: bool,
    at_max: bool,
    on_zoom_out: Option<Command>,
    on_zoom_in: Option<Command>,
}

impl Zoom {
    /// `scale` is the camera's zoom, 1.0 for 100%.
    pub fn new(scale: f32) -> Self {
        Self {
            scale,
            at_min: false,
            at_max: false,
            on_zoom_out: None,
            on_zoom_in: None,
        }
    }

    /// The camera cannot zoom out further.
    pub fn at_min(mut self, at_min: bool) -> Self {
        self.at_min = at_min;
        self
    }

    /// The camera cannot zoom in further.
    pub fn at_max(mut self, at_max: bool) -> Self {
        self.at_max = at_max;
        self
    }

    pub fn on_zoom_out(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_zoom_out = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_zoom_in(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_zoom_in = Some(std::rc::Rc::new(handler));
        self
    }
}

fn step(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    disabled: bool,
    handler: Option<Command>,
) -> Button {
    Button::new(id)
        .ghost()
        .icon(icon)
        .tooltip(label)
        .accessibility_label(label)
        .disabled(disabled)
        .on_click(move |_, window, cx| {
            if let Some(handler) = &handler {
                handler(window, cx);
            }
        })
}

impl RenderOnce for Zoom {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let percent = (self.scale * 100.).round() as i32;
        bar(cx)
            .child(step(
                "canvas-zoom-out",
                IconName::Minus,
                "Zoom out",
                self.at_min,
                self.on_zoom_out,
            ))
            .child(
                div()
                    .w_12()
                    .text_center()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(format!("{percent}%")),
            )
            .child(step(
                "canvas-zoom-in",
                IconName::Plus,
                "Zoom in",
                self.at_max,
                self.on_zoom_in,
            ))
    }
}
