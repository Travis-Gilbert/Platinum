//! The page's form controls and dialogs.
//!
//! `<select>` is GPUI Kit's `NativeMenu`, alert and confirm are GPUI's native
//! prompt, the file picker is GPUI's open panel, the color picker is AppKit's
//! shared color panel, and `prompt()` is a small GPUI Kit dialog window. The
//! context menu is not drawn here: the page hands its entries to the browser,
//! which shows them in Platinum's search menu (`navigator.rs`).
//!
//! SOURCING: GPUI Kit `NativeMenu`, GPUI `prompt` / `prompt_for_paths`, and
//! objc2-app-kit `NSColorPanel`; only the glue between them and Servo's
//! `EmbedderControl` responders is ours. Adapted from Theorem UI's desktop
//! host (`desktop/src/controls.rs`).

use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::{ActiveTheme as _, Root, h_flex, v_flex};
use gpui_kit::{
    Action, App, AppContext as _, Bounds, Context, Entity, IntoElement, NoAction,
    ParentElement as _, PathPromptOptions, Pixels, Point, PromptLevel, Render, Styled as _, Task,
    TitlebarOptions, WeakEntity, Window, WindowBounds, WindowKind, WindowOptions, point, px, size,
};
use servo::{
    ColorPicker, ContextMenu, ContextMenuAction, ContextMenuItem, DeviceIntRect, EmbedderControl,
    EmbedderControlId, FilePicker, PromptDialog, SelectElement, SelectElementOption,
    SelectElementOptionOrOptgroup, SimpleDialog,
};

use crate::page::{Page, PageMenu, PageMenuItem};

/// The `<select>` option the person chose, by Servo's option id.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = page, no_json)]
pub struct PickOption {
    id: usize,
}

/// How often the color panel is checked for having been closed; AppKit has
/// no close callback for the shared panel short of a delegate class.
const COLOR_PANEL_POLL: Duration = Duration::from_millis(150);

#[derive(Default)]
pub struct Controls {
    /// The select or context menu on screen. Neither `NativeMenu` nor the
    /// search menu reports every dismissal, so the next click or key press in
    /// the page drops it, and dropping a responder answers Servo with no
    /// change.
    menu: Option<Menu>,
    /// Native prompts do not nest; a second one waits for none and is refused.
    dialog_open: bool,
    color: Option<ColorSession>,
}

enum Menu {
    Select(SelectElement),
    Context(ContextMenu),
}

impl Menu {
    fn id(&self) -> EmbedderControlId {
        match self {
            Menu::Select(select) => select.id(),
            Menu::Context(menu) => menu.id(),
        }
    }
}

struct ColorSession {
    picker: ColorPicker,
    _poll: Task<()>,
}

impl Controls {
    pub fn dismiss_menus(&mut self) {
        self.menu = None;
    }

    /// Claim the one native dialog slot; false when a dialog is already up.
    pub fn begin_dialog(&mut self) -> bool {
        !std::mem::replace(&mut self.dialog_open, true)
    }

    pub fn end_dialog(&mut self) {
        self.dialog_open = false;
    }
}

impl Page {
    pub(crate) fn show_control(
        &mut self,
        control: EmbedderControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match control {
            EmbedderControl::SelectElement(select) => self.show_select(select, window, cx),
            EmbedderControl::ContextMenu(menu) => self.show_context_menu(menu, cx),
            EmbedderControl::FilePicker(picker) => self.show_file_picker(picker, cx),
            EmbedderControl::ColorPicker(picker) => self.show_color_picker(picker, cx),
            EmbedderControl::SimpleDialog(dialog) => self.show_dialog(dialog, window, cx),
            EmbedderControl::InputMethod(control) => self.input_method = Some(control),
        }
    }

    pub(crate) fn hide_control(&mut self, id: EmbedderControlId, _: &mut Context<Self>) {
        if self
            .controls
            .menu
            .as_ref()
            .is_some_and(|menu| menu.id() == id)
        {
            self.controls.menu = None;
        }
        if self
            .input_method
            .as_ref()
            .is_some_and(|control| control.id() == id)
        {
            self.input_method = None;
        }
        if self
            .controls
            .color
            .as_ref()
            .is_some_and(|session| session.picker.id() == id)
        {
            self.controls.color = None;
            color_panel::close();
        }
    }

    /// Window coordinates, in points, of a point in the page's device pixels.
    fn window_point(&self, x: i32, y: i32) -> Point<Pixels> {
        let origin = self.bounds.map_or(point(px(0.), px(0.)), |b| b.origin);
        point(
            origin.x + px(x as f32 / self.scale),
            origin.y + px(y as f32 / self.scale),
        )
    }

    fn below(&self, rect: DeviceIntRect) -> Point<Pixels> {
        self.window_point(rect.min.x, rect.max.y)
    }

    fn show_select(&mut self, select: SelectElement, window: &mut Window, cx: &mut Context<Self>) {
        let position = self.below(select.position());
        let chosen = select.selected_options();
        let mut menu = NativeMenu::new();
        for entry in select.options() {
            match entry {
                SelectElementOptionOrOptgroup::Option(option) => {
                    menu = option_item(menu, option, &chosen);
                }
                SelectElementOptionOrOptgroup::Optgroup { label, options } => {
                    menu = menu.menu_with_disabled(label.clone(), true, Box::new(NoAction));
                    for option in options {
                        menu = option_item(menu, option, &chosen);
                    }
                }
            }
        }
        self.controls.menu = Some(Menu::Select(select));
        menu.show(position, window, cx);
    }

    pub(crate) fn pick_option(&mut self, pick: &PickOption, _: &mut Window, _: &mut Context<Self>) {
        let Some(Menu::Select(mut select)) = self.controls.menu.take() else {
            return;
        };
        let mut chosen = select.selected_options();
        if select.allow_select_multiple() {
            match chosen.iter().position(|id| *id == pick.id) {
                Some(index) => {
                    chosen.remove(index);
                }
                None => chosen.push(pick.id),
            }
        } else {
            chosen = vec![pick.id];
        }
        select.select(chosen);
        select.submit();
    }

    fn show_context_menu(&mut self, menu: ContextMenu, cx: &mut Context<Self>) {
        let position = self.last_pointer.unwrap_or_else(|| {
            let rect = menu.position();
            self.window_point(rect.min.x, rect.min.y)
        });
        let items = menu
            .items()
            .iter()
            .filter_map(|item| match item {
                ContextMenuItem::Item {
                    label,
                    action,
                    enabled,
                } => Some(PageMenuItem {
                    label: label.clone().into(),
                    action: *action,
                    enabled: *enabled,
                }),
                ContextMenuItem::Separator => None,
            })
            .collect();
        let info = menu.element_info();
        let (link, image) = (info.link_url.clone(), info.image_url.clone());
        self.controls.menu = Some(Menu::Context(menu));
        self.context_menu_requested(
            PageMenu {
                position,
                items,
                link,
                image,
            },
            cx,
        );
    }

    /// The person chose one of the page's own context menu entries.
    pub fn select_context_action(&mut self, action: ContextMenuAction) {
        if let Some(Menu::Context(menu)) = self.controls.menu.take() {
            menu.select(action);
        }
    }

    /// The search menu closed without a page entry chosen.
    pub fn dismiss_context_menu(&mut self) {
        if let Some(Menu::Context(menu)) = self.controls.menu.take() {
            menu.dismiss();
        }
    }

    fn show_file_picker(&mut self, mut picker: FilePicker, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: picker.allow_select_multiple(),
            prompt: None,
        });
        cx.spawn(async move |_, _| match paths.await {
            Ok(Ok(Some(paths))) => {
                picker.select(&paths);
                picker.submit();
            }
            _ => picker.dismiss(),
        })
        .detach();
    }

    fn show_color_picker(&mut self, picker: ColorPicker, cx: &mut Context<Self>) {
        color_panel::open(picker.current_color());
        let poll = cx.spawn(async move |page, cx| {
            loop {
                cx.background_executor().timer(COLOR_PANEL_POLL).await;
                let open = page.update(cx, |page, _| page.poll_color_panel());
                if !matches!(open, Ok(true)) {
                    break;
                }
            }
        });
        self.controls.color = Some(ColorSession {
            picker,
            _poll: poll,
        });
    }

    /// Submit the panel's color once the person closes it.
    fn poll_color_panel(&mut self) -> bool {
        if self.controls.color.is_none() {
            return false;
        }
        if color_panel::is_visible() {
            return true;
        }
        if let Some(mut session) = self.controls.color.take() {
            session.picker.select(color_panel::color());
            session.picker.submit();
        }
        false
    }

    fn show_dialog(&mut self, dialog: SimpleDialog, window: &mut Window, cx: &mut Context<Self>) {
        // Refusing drops the dialog, which answers it: OK for alert, Cancel
        // for confirm and prompt.
        if !self.controls.begin_dialog() {
            return;
        }
        match dialog {
            SimpleDialog::Alert(alert) => {
                let answer = window.prompt(PromptLevel::Info, alert.message(), None, &["OK"], cx);
                cx.spawn(async move |page, cx| {
                    let _ = answer.await;
                    alert.confirm();
                    let _ = page.update(cx, |page, _| page.controls.end_dialog());
                })
                .detach();
            }
            SimpleDialog::Confirm(confirm) => {
                let answer = window.prompt(
                    PromptLevel::Info,
                    confirm.message(),
                    None,
                    &["OK", "Cancel"],
                    cx,
                );
                cx.spawn(async move |page, cx| {
                    match answer.await {
                        Ok(0) => confirm.confirm(),
                        _ => confirm.dismiss(),
                    }
                    let _ = page.update(cx, |page, _| page.controls.end_dialog());
                })
                .detach();
            }
            SimpleDialog::Prompt(prompt) => {
                let page = cx.entity().downgrade();
                if let Err(error) = PromptWindow::open(prompt, page, window, cx) {
                    log::error!("prompt(): {error}");
                    self.controls.end_dialog();
                }
            }
        }
    }
}

fn option_item(menu: NativeMenu, option: &SelectElementOption, chosen: &[usize]) -> NativeMenu {
    let action = Box::new(PickOption { id: option.id });
    if option.is_disabled {
        menu.menu_with_disabled(option.label.clone(), true, action)
    } else {
        menu.menu_with_check(option.label.clone(), chosen.contains(&option.id), action)
    }
}

/// `prompt()`: the page's message, a text field and OK / Cancel, in a window
/// that blocks the page's window until it closes.
struct PromptWindow {
    prompt: Option<PromptDialog>,
    input: Entity<InputState>,
}

impl PromptWindow {
    fn open(
        prompt: PromptDialog,
        page: WeakEntity<Page>,
        parent: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<()> {
        let extent = size(px(420.), px(170.));
        let origin = parent.bounds().center() - point(extent.width / 2., extent.height / 2.);
        let options = WindowOptions {
            kind: WindowKind::Dialog,
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, extent))),
            titlebar: Some(TitlebarOptions {
                title: Some(crate::brand::NAME.into()),
                ..Default::default()
            }),
            is_resizable: false,
            is_minimizable: false,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| {
                let input = cx.new(|cx| {
                    InputState::new(window, cx).default_value(prompt.current_value().to_owned())
                });
                input.update(cx, |input, cx| input.focus(window, cx));
                cx.on_release(move |_, cx| {
                    let _ = page.update(cx, |page, _| page.controls.end_dialog());
                })
                .detach();
                PromptWindow {
                    prompt: Some(prompt),
                    input,
                }
            });
            cx.new(|cx| Root::new(view, window, cx))
        })?;
        Ok(())
    }

    fn answer(&mut self, ok: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut prompt) = self.prompt.take() {
            if ok {
                prompt.set_current_value(&self.input.read(cx).value());
                prompt.confirm();
            } else {
                prompt.dismiss();
            }
        }
        window.remove_window();
    }
}

impl Render for PromptWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let message = self
            .prompt
            .as_ref()
            .map(|prompt| prompt.message().to_owned())
            .unwrap_or_default();
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .bg(cx.theme().background)
            .child(message)
            .child(Input::new(&self.input))
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("cancel").label("Cancel").on_click(
                        cx.listener(|this, _, window, cx| this.answer(false, window, cx)),
                    ))
                    .child(Button::new("ok").primary().label("OK").on_click(
                        cx.listener(|this, _, window, cx| this.answer(true, window, cx)),
                    )),
            )
    }
}

/// AppKit's shared color panel, the system's own color picker.
#[cfg(target_os = "macos")]
mod color_panel {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSColor, NSColorPanel, NSColorSpace};
    use servo::RgbColor;

    fn panel() -> Option<objc2::rc::Retained<NSColorPanel>> {
        Some(NSColorPanel::sharedColorPanel(MainThreadMarker::new()?))
    }

    pub fn open(current: Option<RgbColor>) {
        let Some(panel) = panel() else {
            return;
        };
        panel.setShowsAlpha(false);
        if let Some(RgbColor { red, green, blue }) = current {
            let channel = |c: u8| f64::from(c) / 255.0;
            panel.setColor(&NSColor::colorWithSRGBRed_green_blue_alpha(
                channel(red),
                channel(green),
                channel(blue),
                1.0,
            ));
        }
        panel.makeKeyAndOrderFront(None);
    }

    pub fn is_visible() -> bool {
        panel().is_some_and(|panel| panel.isVisible())
    }

    pub fn color() -> Option<RgbColor> {
        let srgb = panel()?
            .color()
            .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
        let channel = |c: f64| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        Some(RgbColor {
            red: channel(srgb.redComponent()),
            green: channel(srgb.greenComponent()),
            blue: channel(srgb.blueComponent()),
        })
    }

    pub fn close() {
        if let Some(panel) = panel() {
            panel.orderOut(None);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod color_panel {
    use servo::RgbColor;

    pub fn open(_: Option<RgbColor>) {}
    pub fn is_visible() -> bool {
        false
    }
    pub fn color() -> Option<RgbColor> {
        None
    }
    pub fn close() {}
}
