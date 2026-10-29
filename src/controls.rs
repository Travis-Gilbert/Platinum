//! The page's form controls and dialogs.
//!
//! Canvas menus and dialogs use GPUI Kit overlays so camera motion can dismiss
//! them. In Tabs, `<select>` is `NativeMenu`, alert and confirm are GPUI's native
//! prompt, the file picker is GPUI's open panel, the color picker is AppKit's
//! shared color panel, and `prompt()` is a small GPUI Kit dialog window. The
//! context menu is not drawn here: the page hands its entries to the browser,
//! which shows them in Platinum's search menu (`navigator.rs`).
//!
//! SOURCING: GPUI Kit `NativeMenu`, GPUI `prompt` / `prompt_for_paths`, and
//! objc2-app-kit `NSColorPanel`; only the glue between them and Servo's
//! `EmbedderControl` responders is ours. Adapted from Theorem UI's desktop
//! host (`desktop/src/controls.rs`).

use std::{time::Duration, cell::RefCell, rc::Rc};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::{ActiveTheme as _, Root, h_flex, v_flex};
use gpui_kit::{
    Action, App, AppContext as _, Bounds, Focusable as _, Subscription, Context, Entity, IntoElement, NoAction,
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
    pub(crate) popup: Option<(Entity<PopupMenu>, Point<Pixels>, Subscription)>,
    popup_window: Option<gpui_kit::AnyWindowHandle>,
    pub(crate) popup_bounds: Option<Bounds<Pixels>>,
    /// Native prompts do not nest; a second one waits for none and is refused.
    dialog_open: bool,
    file_open: bool,
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
    pub(crate) fn popup_has_focus(&self, window: &Window, cx: &App) -> bool {
        self.popup.as_ref().is_some_and(|(menu, _, _)| menu.focus_handle(cx).is_focused(window))
    }

    /// The color or file panel Platinum opened takes the key window, as in
    /// other browsers the page keeps focus meanwhile: Servo hides an
    /// input's controls on blur, which would close the panel at once.
    pub(crate) fn owns_native_panel(&self) -> bool {
        self.color.is_some() || self.file_open
    }

    pub fn dismiss_menus(&mut self) {
        if self.popup.is_some() { control_trace("popup dismissed"); }
        self.menu = None;
        self.popup = None;
        self.popup_bounds = None;
    }

    /// Claim the one native dialog slot; false when a dialog is already up.
    pub fn begin_dialog(&mut self) -> bool {
        !std::mem::replace(&mut self.dialog_open, true)
    }

    pub fn end_dialog(&mut self) {
        self.dialog_open = false;
    }
}

pub(crate) fn control_trace(message: &str) {
    if std::env::var_os("FRAME_CANVAS_TRACE").is_some() {
        eprintln!("CONTROL {} {message}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
    }
}

impl Page {
    pub(crate) fn show_control(
        &mut self,
        control: EmbedderControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        control_trace(match &control {
            EmbedderControl::SelectElement(_) => "show SelectElement",
            EmbedderControl::ContextMenu(_) => "show ContextMenu",
            EmbedderControl::ColorPicker(_) => "show ColorPicker",
            EmbedderControl::FilePicker(_) => "show FilePicker",
            EmbedderControl::SimpleDialog(_) => "show SimpleDialog",
            EmbedderControl::InputMethod(_) => "show InputMethod",
        });
        match control {
            EmbedderControl::SelectElement(select) => self.show_select(select, window, cx),
            EmbedderControl::ContextMenu(menu) => self.show_context_menu(menu, window, cx),
            EmbedderControl::FilePicker(picker) => self.show_file_picker(picker, cx),
            EmbedderControl::ColorPicker(picker) => self.show_color_picker(picker, cx),
            EmbedderControl::SimpleDialog(dialog) => {
                if self.canvas_world.is_some() {
                    window.focus(&self.focus_handle(cx), cx);
                    cx.emit(crate::page::PageEvent::RevealCanvas);
                    let page = cx.entity();
                    window.defer(cx, move |window, cx| page.update(cx, |p, cx| p.show_canvas_dialog(dialog, window, cx)));
                } else {
                    self.show_dialog(dialog, window, cx);
                }
            },
            EmbedderControl::InputMethod(control) => self.input_method = Some(control),
        }
    }

    pub(crate) fn hide_control(&mut self, id: EmbedderControlId, cx: &mut Context<Self>) {
        control_trace(&format!("hide {id:?}; popup={} color={}", self.controls.popup.is_some(), self.controls.color.is_some()));
        if self
            .controls
            .menu
            .as_ref()
            .is_some_and(|menu| menu.id() == id)
        {
            self.controls.dismiss_menus();
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
            cx.foreground_executor().spawn(async { color_panel::close(); }).detach();
        }
    }

    fn below(&self, rect: DeviceIntRect) -> Point<Pixels> {
        let [x, y] = select_device_anchor(rect, self.webview.device_pixels_per_css_pixel().get());
        self.transform().map_or(point(px(0.), px(0.)), |t| t.window_point(x, y))
    }

    fn show_select(&mut self, select: SelectElement, window: &mut Window, cx: &mut Context<Self>) {
        let position = self.below(select.position());
        let chosen = select.selected_options();
        if self.canvas_world.is_some() {
            let page = cx.entity().downgrade();
            let focus = self.focus_handle(cx);
            let popup = PopupMenu::build(window, cx, |mut menu, _, _| {
                menu = menu.action_context(focus);
                for entry in select.options() {
                    let options = match entry {
                        SelectElementOptionOrOptgroup::Option(option) => std::slice::from_ref(option),
                        SelectElementOptionOrOptgroup::Optgroup { label, options } => {
                            menu = menu.label(label.clone());
                            options.as_slice()
                        }
                    };
                    for option in options {
                        let page = page.clone();
                        let id = option.id;
                        menu = menu.item(PopupMenuItem::new(option.label.clone())
                            .disabled(option.is_disabled).checked(chosen.contains(&option.id))
                            .on_click(move |_, window, cx| { let _ = page.update(cx, |p, cx| p.pick_option(&PickOption { id }, window, cx)); }));
                    }
                }
                menu
            });
            self.controls.menu = Some(Menu::Select(select));
            self.mount_popup(popup, position, window, cx);
            return;
        }
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

    fn show_context_menu(&mut self, menu: ContextMenu, window: &mut Window, cx: &mut Context<Self>) {
        let rect = menu.position();
        let position = self.window_point(rect.min.x, rect.min.y);
        if self.canvas_world.is_some() {
            let page = cx.entity().downgrade();
            let focus = self.focus_handle(cx);
            let popup = PopupMenu::build(window, cx, |mut popup, _, _| {
                popup = popup.action_context(focus);
                for item in menu.items() {
                    if let ContextMenuItem::Item { label, action, enabled } = item {
                        let page = page.clone();
                        let action = *action;
                        popup = popup.item(PopupMenuItem::new(label.clone()).disabled(!enabled)
                            .on_click(move |_, _, cx| { let _ = page.update(cx, |p, _| p.select_context_action(action)); }));
                    }
                }
                popup
            });
            self.controls.menu = Some(Menu::Context(menu));
            self.mount_popup(popup, position, window, cx);
            return;
        }
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

    fn mount_popup(&mut self, popup: Entity<PopupMenu>, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let subscription = cx.subscribe_in(&popup, window, |page, _, _: &gpui_kit::DismissEvent, window, cx| {
            page.controls.dismiss_menus();
            window.focus(&page.focus_handle(cx), cx);
            cx.notify();
        });
        window.focus(&popup.focus_handle(cx), cx);
        self.controls.popup_window = Some(window.window_handle());
        self.controls.popup = Some((popup, position, subscription));
        control_trace("popup mounted");
        cx.notify();
    }

    pub(crate) fn native_picker_visible(&self, kind: &str) -> bool {
        if kind == "color" { color_panel::is_visible() } else { file_panel_visible(false) }
    }

    pub(crate) fn camera_moved(&mut self, cx: &mut Context<Self>) {
        if self.controls.popup.is_some() || self.controls.color.is_some() || self.controls.file_open { control_trace("camera dismiss controls"); }
        if let Some((popup, _, _)) = &self.controls.popup {
            let focus = popup.focus_handle(cx);
            let restore = self.focus_handle(cx);
            if let Some(owner) = self.controls.popup_window {
                cx.defer(move |cx| {
                    let _ = owner.update(cx, |_, window, cx| {
                        if focus.is_focused(window) { window.focus(&restore, cx); }
                    });
                });
            }
        }
        self.controls.dismiss_menus();
        if self.controls.color.take().is_some() {
            cx.foreground_executor().spawn(async { color_panel::close(); }).detach();
        }
        if self.controls.file_open {
            cx.foreground_executor().spawn(async { file_panel_visible(true); }).detach();
        }
        cx.notify();
    }

    fn show_canvas_dialog(&mut self, dialog: SimpleDialog, window: &mut Window, cx: &mut Context<Self>) {
        if !self.controls.begin_dialog() { return; }
        let (message, initial, cancel) = match &dialog {
            SimpleDialog::Alert(d) => (d.message().to_owned(), None, false),
            SimpleDialog::Confirm(d) => (d.message().to_owned(), None, true),
            SimpleDialog::Prompt(d) => (d.message().to_owned(), Some(d.current_value().to_owned()), true),
        };
        let input = initial.map(|value| cx.new(|cx| InputState::new(window, cx).default_value(value)));
        let responder = Rc::new(RefCell::new(Some(dialog)));
        let page = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let dismissed = responder.clone();
            let responder = responder.clone();
            let input_ok = input.clone();
            let page = page.clone();
            let dialog = if cancel { dialog.confirm() } else { dialog };
            dialog.title(message.clone()).close_button(false)
                .children(input.as_ref().map(|input| Input::new(input)))
                .on_ok(move |_, _, cx| {
                    if let Some(dialog) = responder.borrow_mut().take() {
                        match dialog {
                            SimpleDialog::Alert(d) => d.confirm(),
                            SimpleDialog::Confirm(d) => d.confirm(),
                            SimpleDialog::Prompt(mut d) => {
                                if let Some(input) = &input_ok { d.set_current_value(&input.read(cx).value()); }
                                d.confirm();
                            }
                        }
                    }
                    true
                })
                .on_close(move |_, window, cx| {
                    dismissed.borrow_mut().take();
                    let _ = page.update(cx, |p, cx| {
                        p.controls.end_dialog();
                        window.focus(&p.focus_handle(cx), cx);
                    });
                })
        });
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
        control_trace("file open begin");
        key_owner::remember();
        self.controls.file_open = true;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: picker.allow_select_multiple(),
            prompt: None,
        });
        control_trace("file open returned");
        cx.spawn(async move |page, cx| {
            match paths.await {
                Ok(Ok(Some(paths))) => { picker.select(&paths); picker.submit(); }
                _ => picker.dismiss(),
            }
            let _ = page.update(cx, |page, _| page.controls.file_open = false);
        })
        .detach();
    }

    fn show_color_picker(&mut self, picker: ColorPicker, cx: &mut Context<Self>) {
        let current = picker.current_color();
        let poll = cx.spawn(async move |page, cx| {
            control_trace("color open begin outside App update");
            color_panel::open(current);
            control_trace(&format!("color open returned visible={}", color_panel::is_visible()));
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

// Servo's no-point select rectangle is CSS pixels despite its DeviceIntRect type.
fn select_device_anchor(rect: DeviceIntRect, pixels_per_css: f32) -> [f32; 2] {
    [rect.min.x as f32 * pixels_per_css, rect.max.y as f32 * pixels_per_css]
}

#[cfg(test)]
mod anchor_tests {
    use super::*;
    use crate::canvas::surface::SurfaceTransform;

    #[test]
    fn select_css_anchor_maps_through_tabs_and_canvas_at_page_zoom() {
        let rect = DeviceIntRect::new((123, 87).into(), (223, 117).into());
        for (camera, hidpi) in [(1., 2.), (0.35, 0.7), (2., 1.40625)] {
            for page_zoom in [1., 1.25] {
                let [x, y] = select_device_anchor(rect, hidpi * page_zoom);
                let t = SurfaceTransform { bounds: Bounds::new(point(px(40.), px(60.)), size(px(800.), px(600.))), zoom: camera, raster_scale: hidpi };
                let anchor = t.window_point(x, y);
                assert!((f32::from(anchor.x) - (40. + 123. * page_zoom * camera)).abs() < 0.001);
                assert!((f32::from(anchor.y) - (60. + 117. * page_zoom * camera)).abs() < 0.001);
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
        super::key_owner::remember();
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
            let key = panel.isKeyWindow();
            panel.orderOut(None);
            if key {
                super::key_owner::restore();
            }
        }
    }
}

/// AppKit gives the key window back when the person closes a system panel,
/// but not when Platinum orders it out, which leaves the page's window without
/// the keyboard. Platinum hands it back to the window that had it.
#[cfg(target_os = "macos")]
mod key_owner {
    use std::cell::RefCell;

    use objc2::{MainThreadMarker, rc::Weak};
    use objc2_app_kit::{NSApplication, NSWindow};

    thread_local! {
        static OWNER: RefCell<Option<Weak<NSWindow>>> = const { RefCell::new(None) };
    }

    pub fn remember() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let key = NSApplication::sharedApplication(mtm).keyWindow();
        OWNER.with(|owner| *owner.borrow_mut() = key.as_ref().map(Weak::from));
    }

    pub fn restore() {
        let owner = OWNER.with(|owner| owner.borrow_mut().take());
        if let Some(window) = owner.and_then(|owner| owner.load()) {
            if window.isVisible() {
                window.makeKeyWindow();
            }
            super::control_trace(&format!("key returned to owner key={}", window.isKeyWindow()));
        } else {
            super::control_trace("key owner gone");
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod key_owner {
    pub fn remember() {}
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

#[cfg(target_os = "macos")]
fn file_panel_visible(cancel: bool) -> bool {
    let Some(mtm) = objc2::MainThreadMarker::new() else { return false; };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    let mut visible = false;
    for window in app.windows().iter() {
        if let Some(panel) = window.downcast_ref::<objc2_app_kit::NSOpenPanel>() {
            if panel.isVisible() {
                visible = true;
                if cancel {
                    let key = panel.isKeyWindow();
                    unsafe { panel.cancel(None); }
                    if key { key_owner::restore(); }
                }
            }
        }
    }
    visible
}

#[cfg(not(target_os = "macos"))]
fn file_panel_visible(_: bool) -> bool { false }
