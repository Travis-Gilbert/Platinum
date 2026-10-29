//! The places Frame keeps beneath its pages: mail, the library, notes, a
//! terminal, the agent and App Host apps. Each is a native GPUI Kit surface, never a web
//! page (`docs/vision/native-workspace.md`). None is built yet, so each route
//! shows what it will hold, and the way back.
//!
//! SOURCING: GPUI Kit's `Empty` and `Button`; the icons are `icons.rs`.

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName,
    button::Button,
    empty::{
        Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant,
        EmptyTitle,
    },
};
use gpui_kit::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

use crate::actions::GoBack;
use crate::icons;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    Mail,
    Library,
    Notes,
    Terminal,
    Agent,
    Apps,
}

impl Place {
    pub const ALL: [Place; 6] = [
        Place::Mail,
        Place::Library,
        Place::Notes,
        Place::Terminal,
        Place::Agent,
        Place::Apps,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Place::Mail => "Mail",
            Place::Library => "Library",
            Place::Notes => "Notes",
            Place::Terminal => "Terminal",
            Place::Agent => "Agent",
            Place::Apps => "All apps",
        }
    }

    /// For element ids.
    pub fn key(self) -> &'static str {
        match self {
            Place::Mail => "mail",
            Place::Library => "library",
            Place::Notes => "notes",
            Place::Terminal => "terminal",
            Place::Agent => "agent",
            Place::Apps => "apps",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Place::Mail => icons::mail(),
            Place::Library => icons::media(),
            Place::Notes => icons::notes(),
            Place::Terminal => icons::terminal(),
            Place::Agent => icons::agent(),
            Place::Apps => icons::apps(),
        }
    }

    /// What the place will hold, in a sentence.
    pub fn summary(self) -> &'static str {
        match self {
            Place::Mail => {
                "Your mailbox, with Frame's own interface: threads, drafts and labels, starting with Gmail."
            }
            Place::Library => {
                "Saved pages, media and files, with their sources, notes and collections."
            }
            Place::Notes => {
                "Notes you write as you go, kept with the pages and searches they came from."
            }
            Place::Terminal => "A full terminal with your shell, beside your pages.",
            Place::Agent => {
                "Theorem: conversation, ongoing tasks and memory, working with the pages and places beside it."
            }
            Place::Apps => "Apps installed through App Host.",
        }
    }
}

/// A place's surface, while it is a placeholder: what it will hold and the
/// way back (Back, ⌘[).
#[derive(IntoElement)]
pub struct PlaceView {
    place: Place,
}

impl PlaceView {
    pub fn new(place: Place) -> Self {
        Self { place }
    }
}

impl RenderOnce for PlaceView {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let place = self.place;
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .text_color(cx.theme().foreground)
            .child(
                Empty::new()
                    .max_w_96()
                    .header(
                        EmptyHeader::new()
                            .media(
                                EmptyMedia::new()
                                    .with_variant(EmptyMediaVariant::Icon)
                                    .child(place.icon()),
                            )
                            .title(
                                EmptyTitle::new()
                                    .child(format!("{} is not built yet", place.label())),
                            )
                            .description(EmptyDescription::new().child(place.summary())),
                    )
                    .content(
                        EmptyContent::new().child(
                            Button::new("place-back")
                                .label("Back")
                                .icon(IconName::ArrowLeft)
                                .outline()
                                .tooltip_with_action("Back", &GoBack, None)
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(GoBack), cx)
                                }),
                        ),
                    ),
            )
    }
}
