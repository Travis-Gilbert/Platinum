//! Layers: one button that opens a small panel of switches for what the board
//! draws over the cards: the trail with its navigation edges, semantic edges,
//! and annotations.
//!
//! SOURCING: GPUI Kit's `Popover` (uncontrolled by default; `open` makes the
//! owner control it) with a ghost `Button` trigger, and a `Switch` per layer,
//! since each change applies at once.
//!
//! The kit's switch has no hook for its track: the off track is the theme's
//! `switch.background` token, which sits at about 1.1:1 against the panel in the
//! dark theme, and its own border is transparent. What the kit does take is
//! `Styled` on the element around the track, so a switch with no label of its
//! own (the label is a sibling, which toggles it too, and the switch is named
//! for assistive tech with `accessibility_label`) gets a ring of the text color
//! while it is off. The token itself belongs to the theme.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::{
    Anchor, App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    StatefulInteractiveElement as _, Styled as _, Window, div, transparent_black,
};

use super::{Handler, bar, field};
use crate::icons;

/// What the board can draw over the cards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// The trail of pages in the order they were opened, with its
    /// navigation edges.
    Trail,
    /// Edges between pages that are related in meaning.
    Semantic,
    Annotations,
}

impl Layer {
    pub const ALL: [Layer; 3] = [Layer::Trail, Layer::Semantic, Layer::Annotations];

    pub fn label(self) -> &'static str {
        match self {
            Layer::Trail => "Trail and navigation edges",
            Layer::Semantic => "Semantic edges",
            Layer::Annotations => "Annotations",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Layer::Trail => "canvas-layer-trail",
            Layer::Semantic => "canvas-layer-semantic",
            Layer::Annotations => "canvas-layer-annotations",
        }
    }
}

type Toggle = std::rc::Rc<dyn Fn(Layer, bool, &mut Window, &mut App)>;

/// How much of the text color the ring around an off switch takes. WCAG 1.4.11
/// asks 3:1 of a control's boundary against what is next to it. Measured in
/// the gallery's captures, this ring is above 5:1 against the panel in both
/// themes (0.6 reaches 4.8:1 dark and 3.8:1 light; the rest is margin for the
/// glass the panel sits on).
const OFF_RING: f32 = 0.7;

#[derive(IntoElement)]
pub struct Layers {
    trail: bool,
    semantic: bool,
    annotations: bool,
    open: Option<bool>,
    on_toggle: Option<Toggle>,
    on_open_change: Option<Handler<bool>>,
}

impl Layers {
    pub fn new() -> Self {
        Self {
            trail: true,
            semantic: false,
            annotations: true,
            open: None,
            on_toggle: None,
            on_open_change: None,
        }
    }

    /// Whether `layer` is drawn.
    pub fn shown(mut self, layer: Layer, shown: bool) -> Self {
        match layer {
            Layer::Trail => self.trail = shown,
            Layer::Semantic => self.semantic = shown,
            Layer::Annotations => self.annotations = shown,
        }
        self
    }

    /// Control the panel from outside. Without this the button opens and
    /// closes it itself.
    pub fn open(mut self, open: bool) -> Self {
        self.open = Some(open);
        self
    }

    /// A switch was flipped; told the layer and the state it asks for.
    pub fn on_toggle(
        mut self,
        handler: impl Fn(Layer, bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_open_change(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_change = Some(std::rc::Rc::new(handler));
        self
    }

    fn is_shown(&self, layer: Layer) -> bool {
        match layer {
            Layer::Trail => self.trail,
            Layer::Semantic => self.semantic,
            Layer::Annotations => self.annotations,
        }
    }
}

impl Default for Layers {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for Layers {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let radius = cx.theme().radius_tokens().xl;
        let off_ring = cx.theme().foreground.opacity(OFF_RING);
        let shown = Layer::ALL.map(|layer| self.is_shown(layer));
        let on_toggle = self.on_toggle.clone();
        let on_open_change = self.on_open_change.clone();
        let trigger = Button::new("canvas-layers")
            .ghost()
            .icon(icons::canvas_layers())
            .tooltip("Layers")
            .accessibility_label("Layers");

        let mut popover = Popover::new("canvas-layers-panel")
            .anchor(Anchor::BottomRight)
            .trigger(trigger)
            .bg(field(cx))
            .rounded(radius)
            .on_open_change(move |open, window, cx| {
                if let Some(on_open_change) = &on_open_change {
                    on_open_change(*open, window, cx);
                }
            })
            .content(move |_, _, _| {
                v_flex()
                    .gap_3()
                    .min_w_56()
                    .children(Layer::ALL.into_iter().zip(shown).map(|(layer, shown)| {
                        // One handler for the switch and its label.
                        let flip = {
                            let on_toggle = on_toggle.clone();
                            std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                                if let Some(on_toggle) = &on_toggle {
                                    on_toggle(layer, !shown, window, cx);
                                }
                            })
                        };
                        let flip_label = flip.clone();
                        h_flex()
                            .gap_3()
                            .child(
                                Switch::new(layer.id())
                                    .accessibility_label(layer.label())
                                    .checked(shown)
                                    .border_1()
                                    .rounded_full()
                                    .border_color(if shown { transparent_black() } else { off_ring })
                                    .on_click(move |_, window, cx| flip(window, cx)),
                            )
                            .child(
                                div()
                                    .id((ElementId::from(layer.id()), "label"))
                                    .text_sm()
                                    .cursor_pointer()
                                    .child(layer.label())
                                    .on_click(move |_, window, cx| flip_label(window, cx)),
                            )
                    }))
            });
        if let Some(open) = self.open {
            popover = popover.open(open);
        }
        bar(cx).child(popover)
    }
}
