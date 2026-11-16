//! Theo's mark in the launcher: the dark plate and two lights, small enough
//! to sit beside the field. It says whose window this is and whether Theo is
//! at work, and nothing else; the face itself lives on the desktop.

use std::time::Duration;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, IntoElement as _, ParentElement as _,
    Styled as _, div, hsla, pulsating_between, px,
};

/// The mark. While `working`, its lights breathe.
pub(crate) fn mark(working: bool, cx: &App) -> AnyElement {
    let light = cx.theme().primary;
    let eye = move || div().w(px(4.)).h(px(9.)).rounded(px(2.)).bg(light);
    let eyes = div()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .size_full()
        .child(eye())
        .child(eye());
    let plate = div()
        .flex_none()
        .w(px(34.))
        .h(px(22.))
        .rounded(px(7.))
        .bg(hsla(0., 0., 0.06, 1.))
        .border_1()
        .border_color(hsla(0., 0., 1., 0.14));
    if working {
        plate
            .child(
                eyes.with_animation(
                    "theo-mark-working",
                    Animation::new(Duration::from_millis(1_150))
                        .repeat()
                        .with_easing(pulsating_between(0.35, 1.0)),
                    |eyes, glow| eyes.opacity(glow),
                ),
            )
            .into_any_element()
    } else {
        plate.child(eyes).into_any_element()
    }
}
