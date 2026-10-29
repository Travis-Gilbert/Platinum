//! The two action strips that take the omnibar's place at the bottom centre:
//! four icons each, every one with a tooltip.
//!
//! - a focused card: Expand, Annotate, Ask (AI on) or Find related (AI off),
//!   Remove;
//! - a selection: Open side by side, Arrange, Ask or Find related, Remove.
//!
//! SOURCING: GPUI Kit's ghost `Button` with `.tooltip`, as Platinum's own toolbar
//! buttons are, and `Separator` before the one command that removes. A strip
//! is a short fixed row of commands, not a menu, so `DropdownMenu` does not
//! apply.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::separator::Separator;
use gpui_kit::component::Icon;
use gpui_kit::{App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window};

use super::{Command, Handler, bar, chosen};
use crate::icons;

/// What the Arrange button does when pressed: lay the selection out in a
/// grid, or gather it into a pile. The strip shows the button for the
/// arrangement the selection is not in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrangeAs {
    Grid,
    Pile,
}

impl ArrangeAs {
    fn label(self) -> &'static str {
        match self {
            ArrangeAs::Grid => "Arrange in a grid",
            ArrangeAs::Pile => "Gather into a pile",
        }
    }

    fn icon(self) -> Icon {
        match self {
            ArrangeAs::Grid => icons::canvas_grid(),
            ArrangeAs::Pile => icons::canvas_pile(),
        }
    }
}

fn command(id: &'static str, icon: Icon, label: impl Into<SharedString>) -> Button {
    let label = label.into();
    Button::new(id)
        .ghost()
        .icon(icon)
        .tooltip(label.clone())
        .accessibility_label(label)
}

fn press(button: Button, handler: Option<Command>) -> Button {
    button.on_click(move |_, window, cx| {
        if let Some(handler) = &handler {
            handler(window, cx);
        }
    })
}

/// Ask with AI on, Find related without it.
fn ask_or_related(
    ai_enabled: bool,
    on_ask: Option<Command>,
    on_find_related: Option<Command>,
) -> Button {
    if ai_enabled {
        press(command("canvas-strip-ask", icons::ask(), "Ask"), on_ask)
    } else {
        press(
            command(
                "canvas-strip-related",
                icons::canvas_related(),
                "Find related",
            ),
            on_find_related,
        )
    }
}

fn remove(count: usize, on_remove: Option<Command>) -> Button {
    let label = if count > 1 {
        format!("Remove {count} from canvas")
    } else {
        "Remove from canvas".to_string()
    };
    press(
        command("canvas-strip-remove", icons::canvas_remove(), label),
        on_remove,
    )
}

/// The strip for a focused card.
#[derive(IntoElement)]
pub struct FocusedStrip {
    ai_enabled: bool,
    expanded: bool,
    on_expand: Option<Command>,
    on_annotate: Option<Command>,
    on_ask: Option<Command>,
    on_find_related: Option<Command>,
    on_remove: Option<Command>,
}

impl FocusedStrip {
    pub fn new() -> Self {
        Self {
            ai_enabled: false,
            expanded: false,
            on_expand: None,
            on_annotate: None,
            on_ask: None,
            on_find_related: None,
            on_remove: None,
        }
    }

    /// With AI on the third button asks; without it, it finds related pages.
    pub fn ai_enabled(mut self, ai_enabled: bool) -> Self {
        self.ai_enabled = ai_enabled;
        self
    }

    /// Whether the page already fills the window; Expand then goes back.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    pub fn on_expand(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_expand = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_annotate(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_annotate = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_ask(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_ask = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_find_related(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_find_related = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_remove(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_remove = Some(std::rc::Rc::new(handler));
        self
    }
}

impl Default for FocusedStrip {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for FocusedStrip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let expand = chosen(
            command(
                "canvas-strip-expand",
                icons::canvas_expand(),
                if self.expanded {
                    "Back to reading size"
                } else {
                    "Expand"
                },
            ),
            self.expanded,
            cx,
        );
        bar(cx)
            .child(press(expand, self.on_expand))
            .child(press(
                command("canvas-strip-annotate", icons::canvas_annotate(), "Annotate"),
                self.on_annotate,
            ))
            .child(ask_or_related(
                self.ai_enabled,
                self.on_ask,
                self.on_find_related,
            ))
            .child(Separator::vertical().h_4().mx_1())
            .child(remove(1, self.on_remove))
    }
}

/// The strip for a selection of cards.
#[derive(IntoElement)]
pub struct SelectionStrip {
    count: usize,
    arrange: ArrangeAs,
    ai_enabled: bool,
    on_open_side_by_side: Option<Command>,
    on_arrange: Option<Handler<ArrangeAs>>,
    on_ask: Option<Command>,
    on_find_related: Option<Command>,
    on_remove: Option<Command>,
}

impl SelectionStrip {
    pub fn new() -> Self {
        Self {
            count: 1,
            arrange: ArrangeAs::Grid,
            ai_enabled: false,
            on_open_side_by_side: None,
            on_arrange: None,
            on_ask: None,
            on_find_related: None,
            on_remove: None,
        }
    }

    /// How many cards are selected; Remove names it.
    pub fn count(mut self, count: usize) -> Self {
        self.count = count;
        self
    }

    /// What Arrange does next: a grid for a pile or a scatter, a pile for a
    /// grid.
    pub fn arrange(mut self, arrange: ArrangeAs) -> Self {
        self.arrange = arrange;
        self
    }

    pub fn ai_enabled(mut self, ai_enabled: bool) -> Self {
        self.ai_enabled = ai_enabled;
        self
    }

    pub fn on_open_side_by_side(
        mut self,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_side_by_side = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_arrange(mut self, handler: impl Fn(ArrangeAs, &mut Window, &mut App) + 'static) -> Self {
        self.on_arrange = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_ask(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_ask = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_find_related(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_find_related = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_remove(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_remove = Some(std::rc::Rc::new(handler));
        self
    }
}

impl Default for SelectionStrip {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for SelectionStrip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let arrange = self.arrange;
        let on_arrange = self.on_arrange;
        bar(cx)
            .child(press(
                command(
                    "canvas-strip-split",
                    icons::canvas_split(),
                    "Open side by side",
                ),
                self.on_open_side_by_side,
            ))
            .child(
                command("canvas-strip-arrange", arrange.icon(), arrange.label()).on_click(
                    move |_, window, cx| {
                        if let Some(on_arrange) = &on_arrange {
                            on_arrange(arrange, window, cx);
                        }
                    },
                ),
            )
            .child(ask_or_related(
                self.ai_enabled,
                self.on_ask,
                self.on_find_related,
            ))
            .child(Separator::vertical().h_4().mx_1())
            .child(remove(self.count, self.on_remove))
    }
}
