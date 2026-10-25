//! Frame's tabs on GPUI Kit's Dock. Each tab is a panel and each tab group is
//! a row of tabs over a card that holds the tab's toolbar and content.
//! Dragging a tab reorders it, moves it to another group, or drops it on a
//! card's edge to split the window.
//!
//! SOURCING: GPUI Kit's base `DockArea` owns the layout tree, the tab groups,
//! drag and drop and the splits. This module is its renderer, drawn with GPUI
//! Kit's `TabBar`, `Tab`, `Button` and `TitleBar`. The styled `DockSkin`
//! does not fit: it has no segmented tabs, no room for the traffic lights,
//! no window-drag strip, and it draws panel chrome of its own.

use std::rc::Rc;

use gpui_kit::base::dock::{
    DockArea, DockAreaRenderer, DockPlacement, DropIndicator, InsertTarget, NodeId, PaneNode,
    PaneRef, Panel, PanelEvent, PanelId, TabGroupContext, TabGroupRenderer,
};
use gpui_kit::base::{ResizeHandleContext, spring};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    AnyElement, AnyView, App, Axis, Context, Div, Empty, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels,
    Render, SharedString, Stateful, Styled as _, Subscription, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, size,
};

use super::Browser;

/// The frame of window chrome around each card.
pub const FRAME: f32 = 6.;

/// The size of the tab that follows the pointer while a tab is dragged.
pub const DRAG_PREVIEW: gpui_kit::Size<Pixels> = size(px(120.), px(28.));

/// A tab, as the dock holds it. What it draws is the browser's.
pub struct TabPanel {
    browser: WeakEntity<Browser>,
    focus_handle: FocusHandle,
    _focus: [Subscription; 2],
}

impl TabPanel {
    pub fn new(browser: WeakEntity<Browser>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let id = PanelId::from(cx.entity_id());
        // The tab group tracks this handle, so a click in a card that lands
        // on nothing focusable focuses the panel itself; the browser passes
        // that on to the tab's content. Focus arriving anywhere in the card,
        // the page of a tab in another group say, makes it the active tab.
        let focused = cx.on_focus(&focus_handle, window, move |panel, window, cx| {
            panel.tell_browser(id, true, window, cx);
        });
        let focus_in = cx.on_focus_in(&focus_handle, window, move |panel, window, cx| {
            panel.tell_browser(id, false, window, cx);
        });
        Self {
            browser,
            focus_handle,
            _focus: [focused, focus_in],
        }
    }

    fn tell_browser(&self, id: PanelId, itself: bool, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.browser.update(cx, |browser, cx| {
            browser.panel_focused(id, itself, window, cx);
        });
    }
}

impl Panel for TabPanel {
    fn panel_name(&self) -> &'static str {
        "FrameTab"
    }

    fn zoomable(&self, _: &App) -> bool {
        false
    }
}

impl EventEmitter<PanelEvent> for TabPanel {}

impl Focusable for TabPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TabPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = PanelId::from(cx.entity_id());
        let card = self
            .browser
            .update(cx, |browser, cx| browser.render_card(id, window, cx))
            .ok();
        let browser = self.browser.clone();
        div()
            .size_full()
            // A right-click opens the navigator for this tab. The group must
            // not move the focus to the panel before the navigator records
            // where it was.
            .on_mouse_down(MouseButton::Right, move |_, window, cx| {
                window.prevent_default();
                let _ = browser.update(cx, |browser, cx| browser.panel_pointed(id, window, cx));
            })
            .children(card)
    }
}

/// One tab group, read off the layout tree.
pub struct Group {
    pub node: NodeId,
    pub panels: Vec<PanelId>,
    pub active_ix: usize,
}

impl Group {
    /// The tab the group shows.
    pub fn displayed(&self) -> Option<PanelId> {
        self.panels.get(self.active_ix).copied()
    }
}

/// The tab groups in layout order: left to right, top to bottom.
pub fn groups(dock: &DockArea) -> Vec<Group> {
    let mut groups = Vec::new();
    if let Some(tree) = dock.layout(DockPlacement::Center) {
        tree.root().walk(&mut |node| {
            if let PaneRef::Tabs { panels, active_ix } = node.kind() {
                groups.push(Group {
                    node: node.id(),
                    panels: panels.to_vec(),
                    active_ix,
                });
            }
        });
    }
    groups
}

/// The groups whose tab rows run along the top of the window, in layout
/// order; the first sits beside the traffic lights.
pub fn top_row(dock: &DockArea) -> Vec<NodeId> {
    fn walk(node: &PaneNode, top: bool, out: &mut Vec<NodeId>) {
        match node.kind() {
            PaneRef::Tabs { .. } => {
                if top {
                    out.push(node.id());
                }
            }
            PaneRef::Split { axis, children, .. } => {
                for (ix, child) in children.iter().enumerate() {
                    walk(child, top && (axis == Axis::Horizontal || ix == 0), out);
                }
            }
        }
    }
    let mut out = Vec::new();
    if let Some(tree) = dock.layout(DockPlacement::Center) {
        walk(tree.root(), true, &mut out);
    }
    out
}

/// Put a new tab in `anchor`'s group, right after it or at the end of the
/// group, without changing the tab any group shows.
pub fn place(
    dock: &mut DockArea,
    panel: Entity<TabPanel>,
    anchor: PanelId,
    after: bool,
    window: &mut Window,
    cx: &mut Context<DockArea>,
) {
    let shown: Vec<PanelId> = groups(dock).iter().filter_map(Group::displayed).collect();
    let id = PanelId::from(panel.entity_id());
    // The dock adds a panel to its first group only, and shows it there.
    dock.add_panel(panel, DockPlacement::Center, None, window, cx);
    let target = groups(dock).into_iter().find_map(|group| {
        let ix = group.panels.iter().position(|panel| *panel == anchor)?;
        Some((group.node, after.then_some(ix + 1)))
    });
    if let Some((node, ix)) = target {
        let target = InsertTarget::Tabs {
            node,
            ix,
            activate: false,
        };
        dock.move_panel(id, target, window, cx);
    }
    for panel in shown {
        dock.select_panel(panel, window, cx);
    }
}

/// The dock's appearance: rounded cards on the window's frame, which the
/// browser paints (`glass::frame`).
pub struct FrameDock {
    browser: WeakEntity<Browser>,
}

impl FrameDock {
    pub fn new(browser: WeakEntity<Browser>) -> Self {
        Self { browser }
    }

    fn compact(&self, cx: &App) -> bool {
        self.browser
            .upgrade()
            .is_some_and(|browser| browser.read(cx).compact)
    }
}

impl DockAreaRenderer for FrameDock {
    fn frame(&self, _: &mut Window, _: &mut App) -> Stateful<Div> {
        div().id("dock")
    }

    fn render_split_handle(
        &self,
        _: &ResizeHandleContext,
        _: &mut Window,
        _: &mut App,
    ) -> Option<AnyElement> {
        // The frame between two cards is the divider; base keeps the hit area.
        Some(Empty.into_any_element())
    }

    fn tab_group_renderer(&self) -> Rc<dyn TabGroupRenderer> {
        Rc::new(Self {
            browser: self.browser.clone(),
        })
    }
}

impl TabGroupRenderer for FrameDock {
    fn frame(&self, group: &TabGroupContext, _: &mut Window, _: &mut App) -> Stateful<Div> {
        div().id(("tab-group", group.node().as_u64()))
    }

    fn content_frame(
        &self,
        group: &TabGroupContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        let compact = self.compact(cx);
        let theme = cx.theme();
        div()
            .id(("card", group.node().as_u64()))
            .relative()
            .mx(px(FRAME))
            .mb(px(FRAME))
            .when(compact, |card| card.mt(px(FRAME)))
            .rounded(theme.radius_lg)
            .bg(theme.background)
            // The page sits a little above the frame, as in Dia.
            .shadow_lg()
    }

    fn render_tab_bar(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self.browser
            .update(cx, |browser, cx| browser.render_tab_row(group, window, cx))
            .unwrap_or_else(|_| Empty.into_any_element())
    }

    fn render_active_panel(
        &self,
        panel: AnyView,
        _: &TabGroupContext,
        _: &mut Window,
        _: &mut App,
    ) -> AnyElement {
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(panel)
            .into_any_element()
    }

    fn render_drop_indicator(
        &self,
        indicator: DropIndicator,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        // As GPUI Kit's own dock skin draws it: the placeholder springs from
        // where the dragged tab is to where the drop would land.
        let to = indicator.to();
        let id = "drop-placeholder";
        let motion = cx.theme().motion_tokens().spring_move.with_epsilon(0.5);
        let left = spring((id, "left"), to.origin().x, motion, window, cx);
        let top = spring((id, "top"), to.origin().y, motion, window, cx);
        let width = spring((id, "width"), to.size().width, motion, window, cx);
        let height = spring((id, "height"), to.size().height, motion, window, cx);
        Some(
            div()
                .absolute()
                .left(left)
                .top(top)
                .w(width)
                .h(height)
                .rounded(cx.theme().radius_lg)
                .bg(cx.theme().drop_target)
                .into_any_element(),
        )
    }
}

/// The tab that follows the pointer while a tab is dragged.
pub struct TabPreview {
    pub title: SharedString,
}

impl Render for TabPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .id("tab-preview")
            .cursor_grab()
            .w(DRAG_PREVIEW.width)
            .h(DRAG_PREVIEW.height)
            .px_3()
            .flex()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_sm()
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .text_color(theme.tab_active_foreground)
            .bg(theme.tab_active)
            .opacity(0.85)
            .child(self.title.clone())
    }
}
