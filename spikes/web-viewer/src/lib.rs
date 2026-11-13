//! W0 throwaway: the native whiteboard mounted on the browser's WebGL2 platform.
use std::{borrow::Cow, cell::RefCell, rc::Rc};

use gpui::{AppContext, Application, ApplicationHandle, Entity, WindowOptions, rgb};
use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, div};
use gpui_whiteboard::{Scene, WhiteboardStyle, WhiteboardView};
use wasm_bindgen::prelude::*;

thread_local! {
    static APPLICATION: RefCell<Option<ApplicationHandle>> = const { RefCell::new(None) };
    static BOARD: RefCell<Option<Entity<WhiteboardView>>> = const { RefCell::new(None) };
}

struct Host(Entity<WhiteboardView>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .font_family("JetBrains Mono")
            .child(self.0.clone())
    }
}

/// Supply the board's deterministic outline font before mounting, using its
/// existing provider seam. The font remains a separately measured web asset.
#[wasm_bindgen]
pub fn set_board_font(bytes: Vec<u8>) -> Result<(), JsValue> {
    let font = gpui_whiteboard::Font::from_bytes(bytes, 0)
        .ok_or_else(|| JsValue::from_str("Invalid board font"))?;
    gpui_whiteboard::set_font_provider(Some(move || Some(font.clone())));
    Ok(())
}

fn fixture() -> Scene {
    let elements: Vec<_> = (0..200).map(|i| {
        let x = 80.0 + (i % 20) as f32 * 180.0;
        let y = 120.0 + (i / 20) as f32 * 120.0;
        let kind = match i % 3 {
            0 => serde_json::json!({"rect": {"x": x, "y": y, "w": 140, "h": 80, "width": 2}}),
            1 => serde_json::json!({"embed": {"page_id": i, "title": format!("Card {i}"), "x": x, "y": y, "w": 140, "h": 80}}),
            _ => serde_json::json!({"text": {"x": x, "y": y, "content": format!("Text {i}"), "size": 20}}),
        };
        serde_json::json!({"id": i + 1, "kind": kind})
    }).collect();
    serde_json::from_value(serde_json::json!({"elements": elements})).expect("valid W0 scene")
}

#[wasm_bindgen]
pub fn mount() {
    if APPLICATION.with(|app| app.borrow().is_some()) {
        return;
    }
    gpui_web::init_logging();
    let platform = Rc::new(gpui_web::WebPlatform::new_with_backend(
        false,
        gpui_web::WebBackendPreference::WebGl,
    ));
    let http = std::sync::Arc::new(platform.fetch_http_client());
    let app = Application::with_platform(platform).with_http_client(http);
    let handle = app.run_embedded(|cx| {
        cx.text_system()
            .add_fonts(vec![Cow::Borrowed(include_bytes!(
                "../../../vendor/gpui-whiteboard/assets/JetBrainsMono-Regular.ttf"
            ))])
            .expect("register the board's bundled font for GPUI toolbar text");
        cx.open_window(WindowOptions::default(), |_, cx| {
            let style = Rc::new(|| WhiteboardStyle {
                bg: rgb(0xf7f5ef).into(),
                grid: rgb(0xd6d2c7).into(),
                text: rgb(0x585a60).into(),
                ink: rgb(0x20242b).into(),
                panel: rgb(0xffffff).into(),
                panel_strong: rgb(0xffffff).into(),
                accent: rgb(0xcbdcf5).into(),
                selection: rgb(0x2466ce).into(),
                swatches: vec![rgb(0x20242b).into(), rgb(0x2466ce).into()],
            });
            let board = cx.new(|cx| WhiteboardView::new(fixture(), style, cx));
            BOARD.with(|stored| *stored.borrow_mut() = Some(board.clone()));
            cx.new(|_| Host(board))
        })
        .expect("open W0 browser window");
    });
    APPLICATION.with(|app| *app.borrow_mut() = Some(handle));
}

/// Read the actual mounted view; never infer success from dispatched events.
#[wasm_bindgen]
pub fn snapshot() -> Option<String> {
    APPLICATION.with(|app| {
        let app = app.borrow();
        app.as_ref()?.update(|cx| {
            BOARD.with(|board| Some(board.borrow().as_ref()?.read(cx).scene().to_json()))
        })
    })
}

#[wasm_bindgen]
pub fn unmount() {
    BOARD.with(|board| board.borrow_mut().take());
    APPLICATION.with(|app| app.borrow_mut().take());
}
