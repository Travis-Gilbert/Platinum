//! The toolbar, a pill at the top centre of the board: the tool in hand
//! (Select, Pan, Link, Note), Insert with its menu, Find, and the Tabs/Canvas
//! switch.
//!
//! SOURCING: GPUI Kit's `Button` (ghost, a primary tint for the tool in hand) with
//! `DropdownMenu` and `PopupMenu` for Insert, which keep the menu's keyboard
//! and dismissal; `TabBar` (segmented) for the mode switch, which is a choice
//! of view; `Separator` between the groups. Only the glyphs are Frame's.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::separator::Separator;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window,
};

use super::{Command, Handler, bar, chosen};
use crate::icons;

/// The tool in hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Pan,
    Link,
    Note,
}

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::Select, Tool::Pan, Tool::Link, Tool::Note];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Pan => "Pan",
            Tool::Link => "Link",
            Tool::Note => "Note",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Tool::Select => "canvas-tool-select",
            Tool::Pan => "canvas-tool-pan",
            Tool::Link => "canvas-tool-link",
            Tool::Note => "canvas-tool-note",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Tool::Select => icons::canvas_select(),
            Tool::Pan => icons::canvas_pan(),
            Tool::Link => icons::canvas_link(),
            Tool::Note => icons::canvas_note(),
        }
    }
}

/// What Insert adds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertKind {
    /// A page, by its address.
    Page,
    Note,
    App,
    /// A file or an image.
    File,
}

/// Which of the window's two layouts is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Tabs,
    Canvas,
}

impl Mode {
    fn index(self) -> usize {
        match self {
            Mode::Tabs => 0,
            Mode::Canvas => 1,
        }
    }

    fn from_index(index: usize) -> Self {
        if index == 0 { Mode::Tabs } else { Mode::Canvas }
    }
}

/// The Insert menu's entries. A command that asks for more first ends in an
/// ellipsis; a note is placed at once.
pub fn insert_menu(
    menu: PopupMenu,
    window: &Window,
    on_insert: Option<Handler<InsertKind>>,
) -> PopupMenu {
    let entry = |label: &'static str, icon: Icon, kind: InsertKind| {
        let on_insert = on_insert.clone();
        PopupMenuItem::new(label)
            .icon(icon)
            .on_click(move |_, window, cx| {
                if let Some(on_insert) = &on_insert {
                    on_insert(kind, window, cx);
                }
            })
    };
    menu.min_w(window.rem_size() * 13.)
        .item(entry("Page by address…", icons::page(), InsertKind::Page))
        .item(entry("Note", icons::canvas_note(), InsertKind::Note))
        .item(entry("App…", icons::canvas_app(), InsertKind::App))
        .item(entry(
            "File or image…",
            icons::canvas_file_image(),
            InsertKind::File,
        ))
}

#[derive(IntoElement)]
pub struct Toolbar {
    tool: Tool,
    mode: Mode,
    on_tool: Option<Handler<Tool>>,
    on_insert: Option<Handler<InsertKind>>,
    on_find: Option<Command>,
    on_mode: Option<Handler<Mode>>,
}

impl Toolbar {
    pub fn new() -> Self {
        Self {
            tool: Tool::Select,
            mode: Mode::Canvas,
            on_tool: None,
            on_insert: None,
            on_find: None,
            on_mode: None,
        }
    }

    /// The tool in hand.
    pub fn tool(mut self, tool: Tool) -> Self {
        self.tool = tool;
        self
    }

    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    pub fn on_tool(mut self, handler: impl Fn(Tool, &mut Window, &mut App) + 'static) -> Self {
        self.on_tool = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_insert(
        mut self,
        handler: impl Fn(InsertKind, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_insert = Some(std::rc::Rc::new(handler));
        self
    }

    /// Find takes the omnibar.
    pub fn on_find(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_find = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_mode(mut self, handler: impl Fn(Mode, &mut Window, &mut App) + 'static) -> Self {
        self.on_mode = Some(std::rc::Rc::new(handler));
        self
    }
}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

fn icon_button(id: &'static str, icon: Icon, label: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .icon(icon)
        .tooltip(label)
        .accessibility_label(label)
}

impl RenderOnce for Toolbar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let on_tool = self.on_tool;
        let on_mode = self.on_mode;
        let on_find = self.on_find;
        let on_insert = self.on_insert;
        let tools = Tool::ALL.map(|tool| {
            let on_tool = on_tool.clone();
            chosen(
                icon_button(tool.id(), tool.icon(), tool.label()),
                tool == self.tool,
                cx,
            )
            .on_click(move |_, window, cx| {
                    if let Some(on_tool) = &on_tool {
                        on_tool(tool, window, cx);
                    }
                })
        });
        let insert = icon_button("canvas-insert", icons::canvas_insert(), "Insert")
            .dropdown_menu(move |menu, window, _| insert_menu(menu, window, on_insert.clone()));
        let find = icon_button("canvas-find", icons::canvas_find(), "Find").on_click(
            move |_, window, cx| {
                if let Some(on_find) = &on_find {
                    on_find(window, cx);
                }
            },
        );
        let modes = TabBar::new("canvas-mode")
            .segmented()
            .small()
            .selected_index(self.mode.index())
            .child(Tab::new().label(SharedString::from("Tabs")))
            .child(Tab::new().label(SharedString::from("Canvas")))
            .on_click(move |index, window, cx| {
                if let Some(on_mode) = &on_mode {
                    on_mode(Mode::from_index(*index), window, cx);
                }
            });

        bar(cx)
            .children(tools)
            .child(Separator::vertical().h_4().mx_1())
            .child(insert)
            .child(find)
            .child(Separator::vertical().h_4().mx_1())
            .child(modes)
    }
}
