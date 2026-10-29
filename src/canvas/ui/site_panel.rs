//! The site panel: what a page says about itself, beside its card. The page's
//! icon, its title with its host in grey, its description, the colors it uses
//! as swatches, and the fonts it sets as "Ag" specimens with the font's name
//! and role ("Headings", "Body").
//!
//! It takes plain props. Whatever reads a page's colors and fonts hands the
//! results to this builder; nothing here fetches or parses.
//!
//! SOURCING: hand-roll for the swatch and the specimen, which model a page's
//! palette and type and which GPUI Kit has no component for. The surface is
//! GPUI Kit's popover shape (`surface`), and the text uses its tokens.

use std::sync::Arc;

use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Hsla, IntoElement, ParentElement as _, RenderImage, RenderOnce, SharedString,
    Styled as _, Window, div, img, prelude::FluentBuilder as _,
};

use super::{display_host, surface, titled_row};
use crate::icons;

/// One font a page sets: its family, and what it is used for.
#[derive(Clone)]
pub struct FontSpecimen {
    family: SharedString,
    role: SharedString,
}

impl FontSpecimen {
    pub fn new(family: impl Into<SharedString>, role: impl Into<SharedString>) -> Self {
        Self {
            family: family.into(),
            role: role.into(),
        }
    }
}

#[derive(IntoElement)]
pub struct SitePanel {
    title: SharedString,
    host: Option<SharedString>,
    description: Option<SharedString>,
    favicon: Option<Arc<RenderImage>>,
    colors: Vec<Hsla>,
    fonts: Vec<FontSpecimen>,
}

impl SitePanel {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            host: None,
            description: None,
            favicon: None,
            colors: Vec::new(),
            fonts: Vec::new(),
        }
    }

    /// The page's host, in grey after the title without its leading `www.`.
    /// It stays whole while the title is cut, and is dropped when the header
    /// is too narrow for both.
    pub fn host(mut self, host: impl AsRef<str>) -> Self {
        self.host = Some(display_host(host.as_ref()));
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn favicon(mut self, favicon: Option<Arc<RenderImage>>) -> Self {
        self.favicon = favicon;
        self
    }

    /// The page's colors, in the order to show them.
    pub fn colors(mut self, colors: impl IntoIterator<Item = impl Into<Hsla>>) -> Self {
        self.colors = colors.into_iter().map(Into::into).collect();
        self
    }

    pub fn fonts(mut self, fonts: impl IntoIterator<Item = FontSpecimen>) -> Self {
        self.fonts = fonts.into_iter().collect();
        self
    }
}

fn swatch(color: Hsla, cx: &App) -> AnyElement {
    div()
        .size_6()
        .flex_shrink_0()
        .rounded_full()
        .bg(color)
        .border_1()
        .border_color(cx.theme().foreground.opacity(0.3))
        .into_any_element()
}

fn specimen(font: FontSpecimen, cx: &App) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .w_10()
                .flex_shrink_0()
                .text_2xl()
                .text_color(theme.foreground)
                .font_family(font.family.clone())
                .child("Ag"),
        )
        .child(
            v_flex()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_sm()
                        .text_color(theme.foreground)
                        .child(font.family),
                )
                .child(
                    div()
                        .truncate()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(font.role),
                ),
        )
        .into_any_element()
}

impl RenderOnce for SitePanel {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let lead = match self.favicon {
            Some(favicon) => img(favicon).size_5().flex_shrink_0().into_any_element(),
            None => icons::page()
                .small()
                .text_color(theme.muted_foreground)
                .into_any_element(),
        };
        let muted = theme.muted_foreground;
        let swatches: Vec<AnyElement> = self.colors.into_iter().map(|c| swatch(c, cx)).collect();
        let specimens: Vec<AnyElement> = self.fonts.into_iter().map(|f| specimen(f, cx)).collect();

        surface(v_flex().w_64().gap_3().p_3(), cx)
            .child(titled_row(
                lead,
                div().font_weight(gpui_kit::FontWeight::SEMIBOLD).child(self.title),
                self.host,
                cx,
            ))
            .when_some(self.description, |panel, description| {
                panel.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .line_clamp(4)
                        .child(description),
                )
            })
            .when(!swatches.is_empty(), |panel| {
                panel.child(h_flex().flex_wrap().gap_2().children(swatches))
            })
            .when(!specimens.is_empty(), |panel| {
                panel.child(v_flex().gap_2().children(specimens))
            })
    }
}
