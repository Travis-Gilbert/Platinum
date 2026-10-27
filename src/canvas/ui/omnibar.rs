//! The omnibar, bottom centre: "Find, ask, or enter URL", the Ask toggle
//! (only while AI is on) and Fit all. Typed text opens a list above it with
//! two groups, "On this canvas" and "Search the web".
//!
//! SOURCING: GPUI Kit's `Input` (the text lives in an `InputState` the owner
//! holds), `Button` for Ask and Fit all, `ListItem` for the suggestion rows
//! and the popover surface for the list. The list is not a `Popover` or
//! `Command`: those take focus, and typing must stay in the field.

use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::component::v_flex;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement,
    ParentElement as _, RenderImage, RenderOnce, SharedString, Styled as _, Window, div, img,
    prelude::FluentBuilder as _, relative,
};

use super::{Command, Handler, bar, chosen, display_host, surface, titled_row};
use crate::icons;

/// Where a suggestion comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuggestionGroup {
    /// A card or note already on the canvas.
    OnCanvas,
    /// A search of the web.
    Web,
}

impl SuggestionGroup {
    const ALL: [SuggestionGroup; 2] = [SuggestionGroup::OnCanvas, SuggestionGroup::Web];

    fn label(self) -> &'static str {
        match self {
            SuggestionGroup::OnCanvas => "On this canvas",
            SuggestionGroup::Web => "Search the web",
        }
    }
}

/// One row of the suggestions list.
#[derive(Clone)]
pub struct Suggestion {
    id: SharedString,
    group: SuggestionGroup,
    title: SharedString,
    detail: Option<SharedString>,
    favicon: Option<Arc<RenderImage>>,
}

impl Suggestion {
    /// `id` is the owner's name for what this row opens; `on_pick` gets it back.
    pub fn new(
        id: impl Into<SharedString>,
        group: SuggestionGroup,
        title: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            group,
            title: title.into(),
            detail: None,
            favicon: None,
        }
    }

    /// What else tells this row from another with its title, in grey after it.
    /// For a page's address use `host`.
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// The page's host, in grey after the title without its leading `www.`.
    /// It stays whole while a long title is cut, and is dropped when the row
    /// is too narrow for both.
    pub fn host(mut self, host: impl AsRef<str>) -> Self {
        self.detail = Some(display_host(host.as_ref()));
        self
    }

    pub fn favicon(mut self, favicon: Option<Arc<RenderImage>>) -> Self {
        self.favicon = favicon;
        self
    }

    pub fn id(&self) -> &SharedString {
        &self.id
    }
}

#[derive(IntoElement)]
pub struct Omnibar {
    input: Entity<InputState>,
    ai_enabled: bool,
    asking: bool,
    suggestions: Vec<Suggestion>,
    highlighted: Option<usize>,
    on_ask: Option<Handler<bool>>,
    on_fit_all: Option<Command>,
    on_pick: Option<Handler<SharedString>>,
}

impl Omnibar {
    /// The field's retained text, with the omnibar's placeholder. The owner
    /// keeps it and subscribes to its `InputEvent`s for Enter and changes.
    pub fn input<T: 'static>(window: &mut Window, cx: &mut Context<T>) -> Entity<InputState> {
        cx.new(|cx| InputState::new(window, cx).placeholder("Find, ask, or enter URL"))
    }

    pub fn new(input: &Entity<InputState>) -> Self {
        Self {
            input: input.clone(),
            ai_enabled: false,
            asking: false,
            suggestions: Vec::new(),
            highlighted: None,
            on_ask: None,
            on_fit_all: None,
            on_pick: None,
        }
    }

    /// Whether AI is on. Without it there is no Ask toggle.
    pub fn ai_enabled(mut self, ai_enabled: bool) -> Self {
        self.ai_enabled = ai_enabled;
        self
    }

    /// Whether the field asks rather than searches.
    pub fn asking(mut self, asking: bool) -> Self {
        self.asking = asking;
        self
    }

    /// The list for the typed text; none hides it. Rows are shown in their
    /// groups, in the order given.
    pub fn suggestions(mut self, suggestions: Vec<Suggestion>) -> Self {
        self.suggestions = suggestions;
        self
    }

    /// The row the arrow keys are on: an index into `suggestions`.
    pub fn highlighted(mut self, index: Option<usize>) -> Self {
        self.highlighted = index;
        self
    }

    /// Ask was toggled; told the state it asks for.
    pub fn on_ask(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_ask = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_fit_all(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_fit_all = Some(std::rc::Rc::new(handler));
        self
    }

    /// A suggestion was clicked; told its id.
    pub fn on_pick(mut self, handler: impl Fn(SharedString, &mut Window, &mut App) + 'static) -> Self {
        self.on_pick = Some(std::rc::Rc::new(handler));
        self
    }

    /// Filled while the field asks, outlined while it searches, as the
    /// address bar's Ask button is.
    fn ask_button(&self, cx: &App) -> Button {
        let asking = self.asking;
        let on_ask = self.on_ask.clone();
        let button = Button::new("canvas-ask")
            .accessibility_label("Ask AI")
            .toggled(asking)
            .tooltip(if asking { "Turn off Ask" } else { "Ask AI" })
            .on_click(move |_, window, cx| {
                if let Some(on_ask) = &on_ask {
                    on_ask(!asking, window, cx);
                }
            });
        if asking {
            chosen(button.icon(icons::ask_on()), true, cx)
        } else {
            button.icon(icons::ask()).ghost()
        }
    }

    fn list(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let mut list = surface(v_flex().w_full().p_1().gap_1(), cx);
        for group in SuggestionGroup::ALL {
            let rows: Vec<_> = self
                .suggestions
                .iter()
                .enumerate()
                .filter(|(_, suggestion)| suggestion.group == group)
                .collect();
            if rows.is_empty() {
                continue;
            }
            list = list.child(
                v_flex()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(group.label()),
                    )
                    .children(rows.into_iter().map(|(index, suggestion)| {
                        let id = suggestion.id.clone();
                        let on_pick = self.on_pick.clone();
                        let lead: AnyElement = match &suggestion.favicon {
                            Some(favicon) => img(favicon.clone()).size_4().flex_shrink_0().into_any_element(),
                            None => Icon::new(match group {
                                SuggestionGroup::OnCanvas => icons::page(),
                                SuggestionGroup::Web => icons::canvas_find(),
                            })
                            .small()
                            .text_color(theme.muted_foreground)
                            .into_any_element(),
                        };
                        // ListItem's own selected fill is a token barely off the
                        // list, so the highlighted row takes the primary tint.
                        ListItem::new(("canvas-suggestion", index))
                            .when(self.highlighted == Some(index), |item| {
                                item.bg(theme.primary.opacity(0.18))
                                    .rounded(theme.radius_tokens().lg)
                            })
                            .on_click(move |_, window, cx| {
                                if let Some(on_pick) = &on_pick {
                                    on_pick(id.clone(), window, cx);
                                }
                            })
                            .child(
                                titled_row(
                                    lead,
                                    div().child(suggestion.title.clone()),
                                    suggestion.detail.clone(),
                                    cx,
                                )
                                .w_full(),
                            )
                    })),
            );
        }
        list.into_any_element()
    }
}

impl RenderOnce for Omnibar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let on_fit_all = self.on_fit_all.clone();
        let fit_all = Button::new("canvas-fit-all")
            .ghost()
            .icon(icons::canvas_fit_all())
            .tooltip("Fit all")
            .accessibility_label("Fit all")
            .on_click(move |_, window, cx| {
                if let Some(on_fit_all) = &on_fit_all {
                    on_fit_all(window, cx);
                }
            });
        let prefix = if self.asking {
            icons::ask().small()
        } else {
            icons::canvas_find().small()
        };
        let list = (!self.suggestions.is_empty()).then(|| self.list(cx));
        let ask = self.ai_enabled.then(|| self.ask_button(cx));

        div()
            .relative()
            .w_96()
            .child(
                bar(cx)
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.input)
                                .id("canvas-omnibar")
                                .prefix(prefix.text_color(theme.muted_foreground))
                                .appearance(false)
                                .cleanable(true),
                        ),
                    )
                    .children(ask)
                    .child(fit_all),
            )
            .children(list.map(|list| {
                div()
                    .absolute()
                    .bottom(relative(1.))
                    .left_0()
                    .right_0()
                    .pb_2()
                    .child(list)
            }))
    }
}
