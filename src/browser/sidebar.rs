//! The sidebar: the pinned sites as a grid of their icons, and under them
//! the tabs as a tree, as Zen and Arc draw them. It is another way to show
//! the same tabs as the rows along the top, which it replaces while open.
//!
//! A tab opened from another hangs under it (`tabtree.rs`). A row's chevron
//! folds its branch and a click makes it the active tab; right-click offers
//! Rename, Pin, Close and Close Branch in the navigator. Dragging a row drops
//! it above, into or below another row, past the last one, or onto the pins
//! to pin it. A pinned site owns one tab: its button opens the site, or goes
//! to the tab when it is open, and the tab is not in the tree.
//!
//! A row drags as the dock's own `DragPanel`, the same as a tab in the
//! rows, so a row dropped on a page's edge splits it there.
//!
//! SOURCING: GPUI Kit's `Tree` (virtualized rows over `TreeState`) with its
//! `ListItem` rows, `Button` for the pins, the folds and closing, `Input`
//! for renaming, `TitleBar` for the window-drag strip, and `resize_handle`
//! for the sidebar's edge. GPUI Kit's tree folds a branch on any click of
//! its row; here a click activates the tab, so the rows stop the click and
//! the browser's `TabTree` holds the folds.

use gpui_kit::base::InteractiveElementExt as _;
use gpui_kit::base::dock::DragPanel;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::resizable::{HandleEdge, resize_handle};
use gpui_kit::component::tree::{TreeItem, tree};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, Axis, ClickEvent, Context, DragMoveEvent, Empty,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels,
    Point, Render, SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity, Window,
    div, img, prelude::FluentBuilder as _, px, transparent_black,
};

use super::dock::{self, DRAG_PREVIEW, FRAME, TabPreview};
use super::{Browser, Origin, TAB_ROW};
use crate::actions::{CloseTab, PinTab, ToggleSidebar};
use crate::icons;
use crate::navigator::{Placement, TabAction, TabMenu};
use crate::session::Pin;
use crate::tabtree::Place;

/// The sidebar's width until its edge is dragged.
pub(super) const WIDTH: f32 = 240.;
/// How narrow and how wide its edge drags it.
const MIN_WIDTH: f32 = 180.;
const MAX_WIDTH: f32 = 480.;
/// A pin's room in the tray along the rows of tabs, and its icon: as in
/// Dia's, about an icon's width apart.
const TRAY_CELL: f32 = 36.;
const TRAY_ICON: f32 = 18.;
/// The height of a row of the tree; GPUI Kit's tree needs them all equal.
const ROW: f32 = 30.;
/// How far each level of the tree sits in from the one above.
const INDENT: f32 = 14.;
/// The tree's last row, New Tab, by its id.
const NEW_TAB_ROW: &str = "new-tab";

/// `width`, kept within the widths the sidebar takes.
pub(super) fn fit_width(width: f32) -> Pixels {
    px(width.clamp(MIN_WIDTH, MAX_WIDTH))
}

/// The sidebar's edge, while it is dragged. It draws nothing: the edge
/// itself shows the drag.
#[derive(Clone)]
struct ResizeSidebar;

impl Render for ResizeSidebar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl Browser {
    pub(super) fn render_sidebar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let browser = cx.weak_entity();
        let dragging = cx.has_active_drag();
        let header = h_flex()
            .h(px(TAB_ROW))
            .flex_shrink_0()
            .items_center()
            .pr_1()
            .child(div().flex_1().h_full().child(Self::drag_strip()))
            .child(
                Button::new("hide-sidebar")
                    .debug_selector(|| "hide-sidebar".into())
                    .icon(IconName::PanelLeftClose)
                    .ghost()
                    .small()
                    .tooltip_with_action("Hide Sidebar", &ToggleSidebar, None)
                    .on_click(
                        cx.listener(|browser, _, window, cx| browser.toggle_sidebar(window, cx)),
                    ),
            );
        let rows = tree(&self.tree_view, move |_, entry, _, _, cx| {
            Self::render_row(&browser, entry.item().id.clone(), entry.depth(), cx)
        });
        // Past the traffic lights, unless full screen hides them.
        let clear = if window.is_fullscreen() { FRAME } else { 76. };
        // GPUI Kit's handle owns the band along the edge and its look; the
        // width follows the drag here, and a double click puts it back.
        let edge = resize_handle::<ResizeSidebar, ResizeSidebar>("sidebar-edge", Axis::Horizontal)
            .inside(HandleEdge::Trailing)
            .on_drag(ResizeSidebar, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_double_click(
                cx.listener(|browser, _, _, cx| browser.set_sidebar_width(px(WIDTH), cx)),
            );
        v_flex()
            .id("sidebar")
            .debug_selector(|| "sidebar".into())
            .relative()
            .w(self.sidebar_width)
            .h_full()
            .flex_shrink_0()
            .pb(px(FRAME))
            // Clicks in the sidebar leave the focus where it is.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_drag_move(
                cx.listener(|browser, event: &DragMoveEvent<ResizeSidebar>, _, cx| {
                    let width = event.event.position.x - event.bounds.left();
                    browser.set_sidebar_width(fit_width(f32::from(width)), cx);
                }),
            )
            .child(header)
            .children(self.pins_grid(cx))
            .child(div().flex_1().min_h_0().pl(px(FRAME)).child(rows))
            // With no pins yet, the header takes a dragged row to pin it. It
            // lies over the header, so the rows stay under the pointer.
            .when(dragging && self.pins.is_empty(), |sidebar| {
                sidebar.child(self.pin_drop(clear, cx))
            })
            .child(edge)
            .into_any_element()
    }

    fn set_sidebar_width(&mut self, width: Pixels, cx: &mut Context<Self>) {
        if width != self.sidebar_width {
            self.sidebar_width = width;
            self.schedule_save(cx);
            cx.notify();
        }
    }

    /// What dragging a tab carries: the dock's own `DragPanel`, as a tab in
    /// the rows carries, so the dock takes a row dropped on a page.
    fn drag_tab(&self, index: usize, cx: &App) -> Option<DragPanel> {
        let panel = self.tabs.get(index)?.panel_id();
        let group = dock::groups(self.dock.read(cx))
            .into_iter()
            .find(|group| group.panels.contains(&panel))?;
        Some(DragPanel::new(panel, group.node))
    }

    /// The tab a dragged `DragPanel` is.
    fn dragged_tab(&self, drag: &DragPanel) -> Option<u64> {
        Some(self.tabs[self.panel_index(drag.panel())?].id)
    }

    /// The pinned sites, four to a row. A row of the tree dropped on them
    /// is pinned.
    fn pins_grid(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.pins.is_empty() {
            return None;
        }
        let radius = cx.theme().radius_lg;
        let grid = div()
            .id("pins")
            .debug_selector(|| "pins".into())
            .flex_shrink_0()
            .mx(px(FRAME))
            .mb_2()
            .grid()
            .grid_cols(4)
            .gap_1p5()
            .rounded(radius)
            .children(self.pins.iter().map(|pin| self.pin_button(pin, true, cx)))
            .drag_over::<DragPanel>(|style, _, _, cx| style.bg(cx.theme().drop_target))
            .on_drop(cx.listener(Self::pin_dropped));
        Some(grid.into_any_element())
    }

    /// The pinned sites at the start of the first row of tabs, in one tray
    /// as Dia draws them, ending in a button that pins the active tab. A tab
    /// dragged onto the tray is pinned.
    pub(super) fn pin_tray(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let pinnable = self.tabs[self.active].pinnable_url(cx).is_some();
        let theme = cx.theme();
        let (radius, fg, muted) = (theme.radius_lg, theme.foreground, theme.muted_foreground);
        let pin_active = Button::new("pin-tab")
            .debug_selector(|| "pin-tab".into())
            .icon(icons::pin())
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(transparent_black())
                    // Quieter than the sites beside it, as Dia's is.
                    .foreground(muted.opacity(0.6))
                    .hover(fg.opacity(0.08))
                    .active(fg.opacity(0.12)),
            )
            .w(px(TRAY_CELL))
            .h(px(28.))
            .disabled(!pinnable)
            .tooltip_with_action("Pin Tab", &PinTab, None)
            .on_click(cx.listener(|browser, _, _, cx| {
                browser.pin_tab(browser.tabs[browser.active].id, cx)
            }));
        h_flex()
            .id("pin-tray")
            .debug_selector(|| "pin-tray".into())
            .flex_shrink_0()
            // The tab bar keeps room for its first tab's curve; this leaves
            // the tray as far from the first capsule as capsules are apart.
            .mr(px(-2.))
            .p_0p5()
            .gap_0p5()
            .items_center()
            .rounded(radius)
            .bg(fg.opacity(0.05))
            .children(self.pins.iter().map(|pin| self.pin_button(pin, false, cx)))
            .child(pin_active)
            .drag_over::<DragPanel>(|style, _, _, cx| style.bg(cx.theme().drop_target))
            .on_drop(cx.listener(Self::pin_dropped))
    }

    fn pin_dropped(&mut self, drag: &DragPanel, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.dragged_tab(drag) {
            self.pin_tab(id, cx);
        }
    }

    /// Where a dragged row goes to be pinned before there are any pins.
    fn pin_drop(&self, left: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .id("pin-drop")
            .debug_selector(|| "pin-drop".into())
            .absolute()
            .top(px(4.))
            .left(px(left))
            .right(px(FRAME))
            .h(px(TAB_ROW - 8.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.radius_lg)
            .border_1()
            .border_dashed()
            .border_color(theme.border)
            .bg(theme.background)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child("Drop here to pin")
            .drag_over::<DragPanel>(|style, _, _, cx| style.bg(cx.theme().drop_target))
            .on_drop(cx.listener(Self::pin_dropped))
    }

    /// A pinned site's button: a tile in the sidebar, or a square in the
    /// tray at the start of the first row of tabs. Its icon is the site's,
    /// kept from when it was pinned. The active one wears its page's color,
    /// as a selected tab does; in the sidebar a tile is also stronger while
    /// its tab is open, and in the tray, as in Dia, the tray is the rest's.
    pub(super) fn pin_button(&self, pin: &Pin, tile: bool, cx: &mut Context<Self>) -> Button {
        let theme = cx.theme();
        let pin_id = pin.id;
        let tab = self.tabs.iter().position(|tab| tab.pin == Some(pin_id));
        let active = tab == Some(self.active);
        let fg = theme.foreground;
        let (color, hover) = match tab {
            Some(ix) if active => {
                let fill = Self::chrome_fill(&self.tabs[ix], cx);
                (fill, fill)
            }
            _ if !tile => (transparent_black(), fg.opacity(0.08)),
            Some(_) => (fg.opacity(0.1), fg.opacity(0.14)),
            None => (fg.opacity(0.05), fg.opacity(0.1)),
        };
        let kept = self.kept_icon(&pin.url);
        let icon = match kept.or_else(|| tab.and_then(|ix| self.favicon_of(&self.tabs[ix], cx))) {
            Some(icon) => img(icon)
                .debug_selector(move || format!("pin-icon-{pin_id}"))
                .flex_shrink_0()
                .when(tile, |icon| icon.size_5())
                .when(!tile, |icon| icon.size(px(TRAY_ICON)))
                .into_any_element(),
            None => Icon::new(IconName::Globe).into_any_element(),
        };
        let label = if pin.title.trim().is_empty() {
            pin.url.host_str().unwrap_or(pin.url.as_str()).to_owned()
        } else {
            pin.title.clone()
        };
        let button = Button::new(("pin", pin_id))
            .debug_selector(move || format!("pin-{pin_id}"))
            // A button with a child is padded for a label, and clips what it
            // holds to what is left; the icon needs the whole button.
            .px_0()
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(color)
                    .foreground(fg)
                    .hover(hover)
                    .active(fg.opacity(0.16)),
            )
            .accessibility_label(label.clone())
            .tooltip(label)
            .child(icon)
            .on_click(
                cx.listener(move |browser, _, window, cx| browser.open_pin(pin_id, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |browser, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    browser.open_pin_menu(pin_id, event.position, window, cx);
                }),
            );
        if tile {
            button.w_full().h(px(40.)).rounded(theme.radius_lg)
        } else {
            button.w(px(TRAY_CELL)).h(px(28.))
        }
    }

    /// A row of the tree: the fold, the site's icon and the tab's name, and
    /// Close, shown on the active row and under the pointer.
    fn render_row(
        browser: &WeakEntity<Self>,
        item: SharedString,
        depth: usize,
        cx: &mut App,
    ) -> ListItem {
        let dragging = cx.has_active_drag();
        if item.as_ref() == NEW_TAB_ROW {
            return Self::new_tab_row(browser, dragging, cx);
        }
        let Some(entity) = browser.upgrade() else {
            return ListItem::new(item);
        };
        let Ok(id) = item.parse::<u64>() else {
            return ListItem::new(item);
        };
        let this = entity.read(cx);
        let Some(index) = this.tab_index(id) else {
            return ListItem::new(item);
        };
        let tab = &this.tabs[index];
        let theme = cx.theme();
        let node = this.tree.node(id);
        let collapsed = node.is_some_and(|node| node.collapsed);
        let title = this.label(tab, cx);
        let active = this.active == index;
        let renaming = this
            .renaming
            .as_ref()
            .filter(|(renamed, ..)| *renamed == id)
            .map(|(_, input, _)| input.clone());
        let group = SharedString::from(format!("tree-row-{id}"));
        let drag = this.drag_tab(index, cx);

        let fold = if this.tree.has_children(id) {
            let browser = browser.clone();
            Button::new(("fold", id))
                .debug_selector(move || format!("fold-{id}"))
                .icon(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .ghost()
                .xsmall()
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = browser.update(cx, |browser, cx| browser.toggle_fold(id, cx));
                })
                .into_any_element()
        } else {
            div().flex_shrink_0().size_5().into_any_element()
        };
        let label = match renaming {
            Some(input) => {
                let browser = browser.clone();
                div()
                    .flex_1()
                    .min_w_0()
                    .on_action(move |_: &Escape, window, cx| {
                        let _ = browser
                            .update(cx, |browser, cx| browser.finish_rename(false, window, cx));
                    })
                    .child(Input::new(&input).xsmall())
                    .into_any_element()
            }
            None => div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_sm()
                // A tab that is not loaded reads quieter.
                .when(tab.asleep.is_some(), |label| {
                    label.text_color(theme.muted_foreground)
                })
                .child(title.clone())
                .into_any_element(),
        };
        let close = {
            let browser = browser.clone();
            Button::new(("close-row", id))
                .icon(IconName::Close)
                .ghost()
                .xsmall()
                .tooltip_with_action("Close Tab", &CloseTab, None)
                .when(!active, |close| {
                    close
                        .invisible()
                        .group_hover(group.clone(), |style| style.visible())
                })
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    let _ = browser.update(cx, |browser, cx| {
                        if let Some(index) = browser.tab_index(id) {
                            browser.close_tab(index, window, cx);
                        }
                    });
                })
        };

        let on_click = {
            let browser = browser.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                // GPUI Kit's tree would fold the branch on this click.
                cx.stop_propagation();
                let _ = browser.update(cx, |browser, cx| browser.activate_id(id, window, cx));
            }
        };
        let on_right_click = {
            let browser = browser.clone();
            move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                window.prevent_default();
                cx.stop_propagation();
                let _ = browser.update(cx, |browser, cx| {
                    browser.open_tab_menu(id, event.position, window, cx)
                });
            }
        };
        ListItem::new(("tree-row", id))
            .debug_selector(move || format!("tree-tab-{id}"))
            .group(group)
            .h(px(ROW))
            .px_1()
            .mr(px(FRAME))
            .rounded(theme.radius)
            .on_click(on_click)
            .on_mouse_down(MouseButton::Right, on_right_click)
            .when_some(drag, |row, drag| {
                row.on_drag(drag, move |drag: &DragPanel, _, _, cx| {
                    drag.set_preview_size(DRAG_PREVIEW);
                    cx.new(|_| TabPreview {
                        title: title.clone(),
                    })
                })
            })
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .min_w_0()
                    .child(div().flex_shrink_0().w(px(depth as f32 * INDENT)))
                    .child(fold)
                    .child(this.tab_prefix(tab, cx))
                    .child(label)
                    .child(close),
            )
            .when(dragging, |row| row.children(Self::drop_zones(browser, id)))
    }

    /// While a row is dragged, the top quarter of each row puts it above
    /// this one, the bottom quarter below this one's branch, and the middle
    /// makes it this one's last child.
    fn drop_zones(browser: &WeakEntity<Self>, id: u64) -> [AnyElement; 3] {
        let edge = px(ROW / 4.);
        [Place::Before(id), Place::Into(id), Place::After(id)].map(|place| {
            let browser = browser.clone();
            let zone = div().absolute().left_0().right_0();
            let zone = match place {
                Place::Before(_) => zone.top_0().h(edge),
                Place::After(_) => zone.bottom_0().h(edge),
                _ => zone.top(edge).bottom(edge),
            };
            zone.drag_over::<DragPanel>(move |style, _, _, cx| {
                let theme = cx.theme();
                match place {
                    Place::Before(_) => style.border_t_2().border_color(theme.drag_border),
                    Place::After(_) => style.border_b_2().border_color(theme.drag_border),
                    _ => style.bg(theme.drop_target).rounded(theme.radius),
                }
            })
            .on_drop(move |drag: &DragPanel, window, cx| {
                let _ = browser.update(cx, |browser, cx| {
                    if let Some(id) = browser.dragged_tab(drag) {
                        browser.move_row(id, place, window, cx)
                    }
                });
            })
            .into_any_element()
        })
    }

    /// The tree's last row: a new tab at the top level. A row dropped on it
    /// goes last.
    fn new_tab_row(browser: &WeakEntity<Self>, dragging: bool, cx: &App) -> ListItem {
        let theme = cx.theme();
        let on_click = {
            let browser = browser.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                cx.stop_propagation();
                let _ = browser.update(cx, |browser, cx| browser.new_tab(window, cx));
            }
        };
        let drop = {
            let browser = browser.clone();
            move |drag: &DragPanel, window: &mut Window, cx: &mut App| {
                let _ = browser.update(cx, |browser, cx| {
                    if let Some(id) = browser.dragged_tab(drag) {
                        browser.move_row(id, Place::End, window, cx)
                    }
                });
            }
        };
        ListItem::new(NEW_TAB_ROW)
            .debug_selector(|| "sidebar-new-tab".into())
            .h(px(ROW))
            .px_1()
            .mr(px(FRAME))
            .rounded(theme.radius)
            .text_color(theme.muted_foreground)
            .on_click(on_click)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(div().flex_shrink_0().size_5())
                    .child(Icon::new(IconName::Plus).small())
                    .child(div().text_sm().child("New Tab")),
            )
            .when(dragging, |row| {
                row.child(
                    div()
                        .absolute()
                        .inset_0()
                        .drag_over::<DragPanel>(|style, _, _, cx| {
                            style.border_t_2().border_color(cx.theme().drag_border)
                        })
                        .on_drop(drop),
                )
            })
    }

    // The tree.

    /// Give GPUI Kit's tree the rows as they are now, and select the active
    /// tab's.
    pub(super) fn sync_tree(&mut self, cx: &mut Context<Self>) {
        let mut items: Vec<TreeItem> = self
            .tree
            .roots()
            .iter()
            .map(|id| self.tree_item(*id, cx))
            .collect();
        items.push(TreeItem::new(NEW_TAB_ROW, "New Tab"));
        self.tree_view
            .update(cx, |state, cx| state.set_items(items, cx));
        self.select_row(cx);
    }

    fn tree_item(&self, id: u64, cx: &App) -> TreeItem {
        let label = self
            .tab_index(id)
            .map(|index| self.tabs[index].title(cx))
            .unwrap_or_default();
        TreeItem::new(id.to_string(), label)
            .expanded(self.tree.node(id).is_some_and(|node| !node.collapsed))
            .children(
                self.tree
                    .children(id)
                    .iter()
                    .map(|child| self.tree_item(*child, cx)),
            )
    }

    /// The active tab's row is the tree's selected one.
    pub(super) fn select_row(&self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let id = SharedString::from(tab.id.to_string());
        self.tree_view.update(cx, |state, cx| {
            let ix = state.index_of(&id);
            if state.selected_index() != ix {
                state.set_selected_index(ix, cx);
            }
        });
    }

    pub(super) fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar = !self.sidebar;
        if self.renaming.is_some() {
            self.finish_rename(false, window, cx);
        }
        // The sidebar is chrome, which compact mode hides.
        if self.sidebar && self.compact {
            self.compact = false;
            self.revealed = false;
        }
        self.sync_tree(cx);
        self.schedule_save(cx);
        cx.notify();
    }

    fn toggle_fold(&mut self, id: u64, cx: &mut Context<Self>) {
        self.tree.toggle(id);
        self.sync_tree(cx);
        self.schedule_save(cx);
    }

    fn move_row(&mut self, id: u64, place: Place, _: &mut Window, cx: &mut Context<Self>) {
        if self.tree.move_to(id, place) {
            self.sync_tree(cx);
            self.schedule_save(cx);
        }
    }

    pub(super) fn tab_action(
        &mut self,
        action: TabAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            TabAction::Close(id) => {
                if let Some(index) = self.tab_index(id) {
                    self.close_tab(index, window, cx);
                }
            }
            TabAction::CloseBranch(id) => self.close_branch(id, window, cx),
            TabAction::Fold(id) | TabAction::Unfold(id) => self.toggle_fold(id, cx),
            TabAction::Rename(id) => self.start_rename(id, window, cx),
            TabAction::Pin(id) => self.pin_tab(id, cx),
            TabAction::Unpin(pin) => self.unpin(pin, cx),
        }
    }

    /// The navigator at the pointer, with the entries for the tab `id`, from
    /// its sidebar row or its tab in a row.
    pub(super) fn open_tab_menu(
        &mut self,
        id: u64,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        let branch = self.tree.has_children(id);
        let mut actions = Vec::new();
        // Only the sidebar shows a branch folded.
        if branch && self.sidebar {
            let collapsed = self.tree.node(id).is_some_and(|node| node.collapsed);
            actions.push(if collapsed {
                TabAction::Unfold(id)
            } else {
                TabAction::Fold(id)
            });
        }
        actions.push(TabAction::Rename(id));
        if self.tabs[index].pinnable_url(cx).is_some() {
            actions.push(TabAction::Pin(id));
        }
        actions.push(TabAction::Close(id));
        if branch {
            actions.push(TabAction::CloseBranch(id));
        }
        self.open_menu("Tab", actions, position, window, cx);
    }

    fn open_pin_menu(
        &mut self,
        pin: u64,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut actions = Vec::new();
        if let Some(tab) = self.tabs.iter().find(|tab| tab.pin == Some(pin)) {
            actions.push(TabAction::Close(tab.id));
        }
        actions.push(TabAction::Unpin(pin));
        self.open_menu("Pinned Site", actions, position, window, cx);
    }

    fn open_menu(
        &mut self,
        label: &'static str,
        actions: Vec<TabAction>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_navigator(Placement::Pointer(position), None, window, cx);
        if let Some((navigator, _)) = &mut self.navigator {
            navigator.tab = Some(TabMenu {
                label: label.into(),
                actions,
            });
        }
    }

    // Naming branches.

    fn start_rename(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        if !self.tree.contains(id) {
            return;
        }
        let current = self
            .tree
            .node(id)
            .and_then(|node| node.name.clone())
            .unwrap_or_else(|| self.tabs[index].title(cx).to_string());
        // The field is in the sidebar's row, which a folded branch above it
        // would hide; a tab in the rows can be renamed with the sidebar shut.
        self.sidebar = true;
        self.tree.reveal(id);
        self.sync_tree(cx);
        let input = cx.new(|cx| InputState::new(window, cx).default_value(current));
        let events = cx.subscribe_in(
            &input,
            window,
            |browser, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    browser.finish_rename(true, window, cx)
                }
                InputEvent::Change | InputEvent::Focus => {}
            },
        );
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        self.renaming = Some((id, input, events));
        cx.notify();
    }

    /// Keep the name typed, or, with `commit` off, the one there was. A name
    /// that is the page's title is no name: the row follows the title again.
    fn finish_rename(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, input, _)) = self.renaming.take() else {
            return;
        };
        if commit {
            let name = input.read(cx).value().to_string();
            let title = self
                .tab_index(id)
                .map(|index| self.tabs[index].title(cx).to_string());
            let named = title.is_none_or(|title| title.trim() != name.trim());
            self.tree.rename(id, named.then_some(name));
            self.sync_tree(cx);
            self.schedule_save(cx);
        }
        self.focus_content(window, cx);
        cx.notify();
    }

    // Pins.

    fn open_pin(&mut self, pin: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.tabs.iter().position(|tab| tab.pin == Some(pin)) {
            self.activate(index, window, cx);
            return;
        }
        let Some(url) = self
            .pins
            .iter()
            .find(|kept| kept.id == pin)
            .map(|pin| pin.url.clone())
        else {
            return;
        };
        let index = self.add_tab(self.active, true, Origin::Pin(pin), window, cx);
        if self.load(index, url, true, window, cx) {
            self.activate(index, window, cx);
        } else {
            self.close_tab(index, window, cx);
        }
    }

    /// Pin the site the tab `id` shows. The tab becomes the pin's and leaves
    /// the tree; its children move up into its place.
    pub(super) fn pin_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        let Some(url) = self.tabs[index].pinnable_url(cx) else {
            return;
        };
        if self
            .renaming
            .as_ref()
            .is_some_and(|(renamed, ..)| *renamed == id)
        {
            self.renaming = None;
        }
        self.remember_icon(index, cx);
        let title = self
            .tree
            .node(id)
            .and_then(|node| node.name.clone())
            .unwrap_or_else(|| self.tabs[index].title(cx).to_string());
        let pin = Pin {
            id: self.next_pin,
            url,
            title,
        };
        self.next_pin += 1;
        if let (Some(store), Some(host)) = (&self.store, pin.url.host_str())
            && let Some(icon) = self.icons.get(host)
        {
            store.save_favicon(host, icon);
        }
        self.tabs[index].pin = Some(pin.id);
        self.tree.remove(id);
        self.pins.push(pin);
        self.save_pins();
        self.sync_tree(cx);
        self.schedule_save(cx);
        cx.notify();
    }

    /// Forget the pin. Its tab, if open, joins the tree at the top level.
    fn unpin(&mut self, pin: u64, cx: &mut Context<Self>) {
        self.pins.retain(|kept| kept.id != pin);
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.pin == Some(pin)) {
            tab.pin = None;
            self.tree.add_root(tab.id);
        }
        self.save_pins();
        self.sync_tree(cx);
        self.schedule_save(cx);
        cx.notify();
    }

    fn save_pins(&self) {
        if let Some(store) = &self.store {
            store.save_pins(&self.pins);
        }
    }
}
