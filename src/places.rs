//! The places Frame keeps beneath its pages: mail, the library, notes, a
//! terminal, the agent and App Host apps. Each is a native GPUI Kit surface, never a web
//! page (`docs/vision/native-workspace.md`). The agent is Theorem's workbench
//! (`theorem/`); the others are not built yet, so each of their routes shows
//! what it will hold, and the way back.
//!
//!
//! A place shows over the tabs or the workspace, which stay as they were
//! beneath it. The places bump, on the window's leading edge, moves between
//! the places and back out (`PlacesBump`).
//!
//! SOURCING: GPUI Kit's `Empty`, `Button` and `Separator`; the icons are
//! `icons.rs`; the bump's shape is `bump.rs`.

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Selectable as _,
    button::{Button, ButtonVariants as _},
    empty::{
        Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant,
        EmptyTitle,
    },
    separator::Separator,
    v_flex,
};
use gpui_kit::{
    App, Background, InteractiveElement as _, IntoElement, ParentElement, RenderOnce, SharedString,
    Styled, Window, div, prelude::FluentBuilder as _, px,
};

use crate::actions::{GoBack, LeavePlace, OpenPlace};
use crate::bump::{self, Slot};
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

    /// Whether the place has its own surface; the others show `PlaceView`.
    pub fn is_built(self) -> bool {
        self == Place::Agent
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

/// Where the places bump's first button goes: out of the places, to what
/// is beneath them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// The workspace's Home.
    Home,
    /// The tabs.
    Tabs,
}

impl Exit {
    fn label(self) -> &'static str {
        match self {
            Exit::Home => "Home",
            Exit::Tabs => "Show Tabs",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Exit::Home => icons::home(),
            Exit::Tabs => icons::page(),
        }
    }
}

/// The places as a column of icons on a bump of the frame, on the window's
/// leading edge: cut into a place's card, whose outline runs around it
/// (`bump::card`), or standing alone beside a navigator or when the places
/// are always shown (`bump::standing`).
#[derive(IntoElement)]
pub struct PlacesBump {
    current: Option<Place>,
    exit: Option<Exit>,
    slot: Option<Slot>,
    fill: Option<Background>,
}

/// The space between the bump's buttons and its edges.
const INSET: f32 = 4.;

impl PlacesBump {
    /// How far the bump stands out: its buttons, medium icon buttons 32px
    /// wide, and the space either side.
    pub const WIDTH: f32 = 32. + INSET * 2.;

    /// The place showing, if one is.
    pub fn new(current: Option<Place>) -> Self {
        Self {
            current,
            exit: None,
            slot: None,
            fill: None,
        }
    }

    /// Lead with a way out of the places.
    pub fn exit(mut self, exit: Exit) -> Self {
        self.exit = Some(exit);
        self
    }

    /// Record the bump's bounds for the card it is cut into, which fills it.
    pub fn slot(mut self, slot: Slot) -> Self {
        self.slot = Some(slot);
        self
    }

    /// Stand alone, the bump's body filled with `fill`.
    pub fn fill(mut self, fill: impl Into<Background>) -> Self {
        self.fill = Some(fill.into());
        self
    }
}

impl RenderOnce for PlacesBump {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let current = self.current;
        v_flex()
            .id("places-bump")
            .debug_selector(|| "places-bump".into())
            .relative()
            .flex_shrink_0()
            .items_center()
            .gap_0p5()
            .py_1()
            .px(px(INSET))
            .when_some(self.fill, |this, fill| this.child(bump::standing(fill, cx)))
            .when_some(self.slot, |this, slot| this.child(slot.recorder()))
            .when_some(self.exit, |bump, exit| {
                bump.child(
                    Button::new("bump-exit")
                        .debug_selector(|| "bump-exit".into())
                        .ghost()
                        .icon(exit.icon())
                        .accessibility_label(exit.label())
                        .tooltip_with_action(exit.label(), &LeavePlace, None)
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(LeavePlace), cx)),
                )
                .child(Separator::horizontal().w(px(20.)).my_0p5())
            })
            .children(Place::ALL.map(|place| {
                Button::new(SharedString::from(format!("bump-{}", place.key())))
                    .debug_selector(move || format!("bump-{}", place.key()))
                    .ghost()
                    .icon(place.icon())
                    .selected(current == Some(place))
                    .accessibility_label(if place.is_built() {
                        SharedString::from(place.label())
                    } else {
                        format!("{}, not built yet", place.label()).into()
                    })
                    .tooltip(place.label())
                    .on_click(move |_, window, cx| {
                        window.dispatch_action(Box::new(OpenPlace { place }), cx)
                    })
            }))
    }
}
