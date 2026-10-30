//! Render the spatial canvas's chrome in every state, in a real window with
//! Platinum's glass under it, in the dark and the light theme.
//!
//! Usage: canvas_chrome_check <png base path> [--screen] [--backdrop]
//!
//! For each sheet and theme it writes `<base>-<sheet>-<theme>.png` from GPUI's
//! own layer (`render_to_image`), which leaves out the glass. With `--screen`
//! it also asks `screencapture` for the window as the desktop composites it,
//! to `<base>-<sheet>-<theme>-window-<desktop|backdrop>.png`. With
//! `--backdrop` a busy window of this process lies under the gallery window,
//! so the glass has something other than the wallpaper to show.
//!
//! SOURCING: the production components in `src/canvas/ui`, with `glass::install`
//! as `main.rs` calls it. Nothing here is drawn by hand except the synthetic
//! cards and favicons that give the chrome something to sit beside.
#[allow(dead_code)]
#[path = "../src/glass.rs"]
mod glass;
#[allow(dead_code)]
#[path = "../src/icons.rs"]
mod icons;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;
#[allow(dead_code, unused_imports)]
#[path = "../src/canvas/ui/mod.rs"]
mod ui;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::input::InputState;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Root, TitleBar, h_flex, v_flex};
use gpui_kit::{
    AnyView, AnyWindowHandle, App, AppContext as _, Bounds, Context, Div, Entity,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, RenderImage, Stateful,
    Styled as _, Window, WindowBounds, WindowOptions, div, point, px, rgb, size,
    transparent_black,
};

use ui::{
    ArrangeAs, Caption, CaptionState, FocusedStrip, FontSpecimen, Layers, Mode, Omnibar,
    SelectionStrip, SitePanel, Suggestion, SuggestionGroup, Tool, Toolbar, Zoom, field,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sheet {
    Toolbar,
    Omnibar,
    Board,
}

impl Sheet {
    const ALL: [Sheet; 3] = [Sheet::Toolbar, Sheet::Omnibar, Sheet::Board];

    fn name(self) -> &'static str {
        match self {
            Sheet::Toolbar => "toolbar",
            Sheet::Omnibar => "omnibar",
            Sheet::Board => "board",
        }
    }
}

/// A 16 by 16 round favicon of one color, as the BGRA frame GPUI draws.
fn favicon(color: [u8; 3]) -> Arc<RenderImage> {
    let mut bgra = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
            let inside = (dx * dx + dy * dy).sqrt() <= 7.5;
            bgra.extend([color[2], color[1], color[0], if inside { 255 } else { 0 }]);
        }
    }
    let frame = image::RgbaImage::from_raw(16, 16, bgra).expect("16 by 16 pixels");
    Arc::new(RenderImage::new(vec![image::Frame::new(frame)]))
}

struct Gallery {
    sheet: Sheet,
    inputs: [Entity<InputState>; 5],
    insert_menu: Entity<PopupMenu>,
    tooltips: Vec<AnyView>,
    favicons: [Arc<RenderImage>; 4],
}

impl Gallery {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let inputs = [(); 5].map(|_| Omnibar::input(window, cx));
        inputs[3].update(cx, |state, cx| state.set_value("vit", window, cx));
        inputs[4].update(cx, |state, cx| state.set_value("what makes a good shelf", window, cx));
        let insert_menu =
            PopupMenu::build(window, cx, |menu, window, _| ui::insert_menu(menu, window, None));
        let tooltips = ["Select", "Insert", "Find related"]
            .into_iter()
            .map(|label| Tooltip::new(label).build(window, cx))
            .collect();
        Self {
            sheet: Sheet::Toolbar,
            inputs,
            insert_menu,
            tooltips,
            favicons: [
                favicon([0x8c, 0x86, 0x78]),
                favicon([0xd6, 0x2b, 0x2b]),
                favicon([0x2f, 0x6f, 0xd6]),
                favicon([0x2e, 0x9e, 0x6a]),
            ],
        }
    }
}

/// A name for what is beside it. The gallery's own text sits on `field`, as
/// the chrome's does, so no label reads against the glass alone.
fn label(text: &'static str, cx: &App) -> Div {
    div()
        .px_2()
        .py_1()
        .rounded(cx.theme().radius_tokens().md)
        .bg(field(cx))
        .text_xs()
        .text_color(cx.theme().foreground)
        .child(text)
}

/// A named specimen. Its name is also its element id: GPUI keys a component's
/// state (the segmented control's pill, a switch's thumb) by the ids above it,
/// and every toolbar draws the same ids, so without a scope per specimen the
/// toolbars on one sheet would share one pill and pull it back and forth.
fn specimen(name: &'static str, child: impl IntoElement, cx: &App) -> Stateful<Div> {
    v_flex()
        .id(name)
        .gap_2()
        .items_start()
        .child(label(name, cx))
        .child(child)
}

/// A page on the board: opaque, drawn the way a card's page would be.
fn card(page: u32, outlined: bool, cx: &App) -> Div {
    let (paper, ink, accent) = match page % 4 {
        0 => (0x111111, 0xf2f2f2, 0xd62b2b),
        1 => (0xf6f3ec, 0x1d1d1b, 0x2f6fd6),
        2 => (0xffffff, 0x222222, 0x2e9e6a),
        _ => (0xf3e3a3, 0x1d1d1b, 0x1d1d1b),
    };
    div()
        .w_48()
        .h_32()
        .flex_shrink_0()
        .p_2()
        .gap_1()
        .flex()
        .flex_col()
        .rounded(cx.theme().radius_tokens().sm)
        .bg(rgb(paper))
        .border_1()
        .border_color(if outlined {
            cx.theme().ring
        } else {
            transparent_black()
        })
        .child(div().h_1().w_12().bg(rgb(ink)))
        .child(div().h_4().w_32().bg(rgb(ink)))
        .child(div().flex_1().bg(rgb(accent)))
}

fn captioned(page: u32, state: CaptionState, caption: Caption, cx: &App) -> Div {
    let outlined = matches!(state, CaptionState::Selected | CaptionState::Focused);
    v_flex()
        .gap_1()
        .child(card(page, outlined, cx))
        .child(caption.state(state))
}

impl Gallery {
    fn toolbar_sheet(&self, cx: &App) -> Div {
        let tools = Tool::ALL.map(|tool| {
            specimen(
                tool.label(),
                Toolbar::new().tool(tool).mode(Mode::Canvas),
                cx,
            )
        });
        v_flex()
            .gap_8()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_6()
                    .children(tools)
                    .child(specimen(
                        "Tabs mode",
                        Toolbar::new().tool(Tool::Select).mode(Mode::Tabs),
                        cx,
                    )),
            )
            .child(specimen(
                "Insert menu open",
                v_flex()
                    .gap_1()
                    .child(Toolbar::new().tool(Tool::Note))
                    .child(div().ml_40().child(self.insert_menu.clone())),
                cx,
            ))
            .child(specimen(
                "Tooltips",
                h_flex().gap_4().children(self.tooltips.clone()),
                cx,
            ))
    }

    fn omnibar_sheet(&self, cx: &App) -> Div {
        let on_canvas = |id: &'static str, title: &'static str, host: &'static str, n: usize| {
            Suggestion::new(id, SuggestionGroup::OnCanvas, title)
                .host(host)
                .favicon(Some(self.favicons[n].clone()))
        };
        let web = |id: &'static str, title: &'static str| {
            Suggestion::new(id, SuggestionGroup::Web, title)
        };
        let both = vec![
            on_canvas("c1", "Vitsœ  Shelving, tables and chairs", "www.vitsoe.com", 0),
            on_canvas(
                "c2",
                "Vitra Design Museum  Eames, Panton, Nelson and the chairs that changed how we sit",
                "www.design-museum.de",
                1,
            ),
            web("w1", "vitsoe 606 universal shelving system"),
            web("w2", "vitra eames lounge chair"),
        ];
        let web_only = vec![
            web("w1", "what makes a good shelf"),
            web("w2", "what makes a good shelf, books"),
        ];
        let strips = [
            specimen(
                "Focused card, AI on",
                FocusedStrip::new().ai_enabled(true),
                cx,
            ),
            specimen("Focused card, AI off", FocusedStrip::new(), cx),
            specimen(
                "Focused card, expanded",
                FocusedStrip::new().expanded(true).ai_enabled(true),
                cx,
            ),
            specimen(
                "Selection, AI on",
                SelectionStrip::new().count(4).ai_enabled(true),
                cx,
            ),
            specimen(
                "Selection, AI off, arranged",
                SelectionStrip::new().count(4).arrange(ArrangeAs::Pile),
                cx,
            ),
        ];
        v_flex()
            .gap_6()
            .child(
                h_flex()
                    .gap_4()
                    .items_end()
                    .child(specimen(
                        "Typing, AI on, Ask off",
                        div().h_64().flex().items_end().child(
                            Omnibar::new(&self.inputs[3])
                                .ai_enabled(true)
                                .suggestions(both)
                                .highlighted(Some(1)),
                        ),
                        cx,
                    ))
                    .child(specimen(
                        "Asking, AI on",
                        div().h_64().flex().items_end().child(
                            Omnibar::new(&self.inputs[4])
                                .ai_enabled(true)
                                .asking(true)
                                .suggestions(web_only),
                        ),
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .gap_4()
                    .child(specimen("Empty, AI off", Omnibar::new(&self.inputs[0]), cx))
                    .child(specimen(
                        "Empty, AI on",
                        Omnibar::new(&self.inputs[1]).ai_enabled(true),
                        cx,
                    ))
                    .child(specimen(
                        "Empty, asking",
                        Omnibar::new(&self.inputs[2]).ai_enabled(true).asking(true),
                        cx,
                    )),
            )
            .child(h_flex().flex_wrap().gap_4().items_start().children(strips))
    }

    fn board_sheet(&self, cx: &App) -> Div {
        let caption = |id: &'static str, title: &'static str, n: usize| {
            Caption::new(id, title)
                .host("www.vitsoe.com")
                .favicon(Some(self.favicons[n].clone()))
        };
        let states = [
            ("Idle", CaptionState::Idle),
            ("Hovered", CaptionState::Hovered),
            ("Selected", CaptionState::Selected),
            ("Focused", CaptionState::Focused),
        ];
        let cards = states.into_iter().enumerate().map(|(n, (name, state))| {
            specimen(
                name,
                captioned(n as u32, state, caption(name, "Vitsœ", n), cx),
                cx,
            )
        });
        let panel = SitePanel::new("Vitsœ  Shelving, tables and chairs")
            .description(
                "Furniture designed to last a lifetime and to be reconfigured as your life changes.",
            )
            .favicon(Some(self.favicons[0].clone()))
            .host("www.vitsoe.com")
            .colors([rgb(0x1d1d1b), rgb(0xf6f3ec), rgb(0xd62b2b), rgb(0x8a8a82)])
            .fonts([
                FontSpecimen::new("Georgia", "Headings"),
                FontSpecimen::new("Helvetica Neue", "Body"),
                FontSpecimen::new("Menlo", "Code"),
            ]);
        let top = h_flex()
            .gap_8()
            .items_start()
            .child(
                v_flex()
                    .gap_6()
                    .child(h_flex().gap_4().children(cards))
                    .child(
                        h_flex()
                            .gap_4()
                            .items_start()
                            .child(specimen("Long title: the host stays whole", {
                                v_flex().gap_1().child(card(2, false, cx)).child(
                                    Caption::new("long", "A very long page title that has to be cut short")
                                        .host("www.example.com")
                                        .w_48(),
                                )
                            }, cx))
                            .child(specimen("Narrower: the host is dropped", {
                                v_flex().gap_1().child(card(3, false, cx)).child(
                                    Caption::new("narrow", "A very long page title that has to be cut short")
                                        .host("www.example.com")
                                        .w_32(),
                                )
                            }, cx))
                            .child(specimen("Zoom", Zoom::new(1.0), cx))
                            .child(specimen("Zoom, fully out", Zoom::new(0.1).at_min(true), cx)),
                    ),
            )
            .child(specimen("Site panel", panel, cx));
        // The panel opens up and to the left of its button, so it gets room.
        let layers = h_flex()
            .gap_3()
            .items_center()
            .ml_80()
            .mt_40()
            .child(Layers::new().open(true))
            .child(label("Layers, open", cx))
            .child(div().ml_16().child(specimen(
                "Captions as wide as their content",
                v_flex()
                    .gap_2()
                    .items_start()
                    .child(
                        Caption::new("hug-short", "Vitsœ")
                            .host("www.vitsoe.com")
                            .favicon(Some(self.favicons[0].clone())),
                    )
                    .child(Caption::new("hug-tiny", "HN").host("news.ycombinator.com")),
                cx,
            )));
        v_flex().child(top).child(layers)
    }
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sheet = match self.sheet {
            Sheet::Toolbar => self.toolbar_sheet(cx),
            Sheet::Omnibar => self.omnibar_sheet(cx),
            Sheet::Board => self.board_sheet(cx),
        };
        // The board shares the window's glass and inactive solid frame.
        div()
            .size_full()
            .bg(ui::frame(window, cx))
            .px_8()
            .pt_12()
            .pb_8()
            .child(sheet)
    }
}

/// Something busy for the glass to show other than the wallpaper.
struct Backdrop;

impl Render for Backdrop {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let bands = [
            0xe63946, 0xf4a261, 0xe9c46a, 0x2a9d8f, 0x264653, 0x6a4c93, 0xff595e, 0x1982c4,
        ];
        v_flex().size_full().children(bands.into_iter().map(|band| {
            div()
                .flex_1()
                .bg(rgb(band))
                .text_3xl()
                .text_color(rgb(0xffffff))
                .child("A window behind the glass, with type, edges and color")
        }))
    }
}

const WINDOW: (f32, f32, f32, f32) = (120., 60., 1280., 820.);

/// How long the window draws before a capture. GPUI's springs step by real
/// time on each draw, and the segmented control's pill and the switches'
/// thumbs run on them, so a capture waits until they have landed.
const SETTLE: Duration = Duration::from_millis(800);
/// About one frame at 60 Hz.
const FRAME: Duration = Duration::from_millis(16);

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("Supply a PNG base path"))?,
    );
    let flags: Vec<String> = args.collect();
    let screen = flags.iter().any(|flag| flag == "--screen");
    let backdrop = flags.iter().any(|flag| flag == "--backdrop");
    let stem = output.with_extension("");

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            if backdrop {
                cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(0.), px(30.)),
                            size(px(1520.), px(960.)),
                        ))),
                        ..Default::default()
                    },
                    |_, cx| cx.new(|_| Backdrop),
                )
                .expect("backdrop window");
            }
            let (x, y, width, height) = WINDOW;
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(x), px(y)),
                    size(px(width), px(height)),
                ))),
                ..TitleBar::window_options()
            };
            let mut gallery = None;
            let window = cx
                .open_window(options, |window, cx| {
                    glass::install(window, cx);
                    let view = cx.new(|cx| Gallery::new(window, cx));
                    gallery = Some(view.clone());
                    cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
                })
                .expect("native window");
            let gallery = gallery.expect("the gallery view");
            // The untyped handle leaves the Root entity free for `draw` to render.
            let window: AnyWindowHandle = window.into();
            cx.activate(true);
            cx.spawn(async move |cx| {
                for appearance in [theme::Appearance::Dark, theme::Appearance::Light] {
                    cx.update(|cx| theme::set_appearance(appearance, cx));
                    let theme_name = match appearance {
                        theme::Appearance::Dark => "dark",
                        _ => "light",
                    };
                    for sheet in Sheet::ALL {
                        gallery.update(cx, |gallery, cx| {
                            gallery.sheet = sheet;
                            cx.notify();
                        });
                        cx.background_executor().timer(Duration::from_millis(1200)).await;
                        let path = format!("{}-{}-{}.png", stem.display(), sheet.name(), theme_name);
                        // An idle window gets no frames, and `render_to_image` reads
                        // the last one drawn. Draw on a timer until the springs have
                        // landed (a popover's panel also lands a frame after the state
                        // that opens it), then draw once more and capture that frame.
                        let started = Instant::now();
                        while started.elapsed() < SETTLE {
                            window
                                .update(cx, |_: AnyView, window, cx| {
                                    window.refresh();
                                    window.draw(cx).clear(cx);
                                })
                                .ok();
                            cx.background_executor().timer(FRAME).await;
                        }
                        let saved = window.update(cx, |_: AnyView, window, cx| {
                            window.refresh();
                            window.draw(cx).clear(cx);
                            window
                                .render_to_image()
                                .and_then(|image| image.save(&path).map_err(Into::into))
                        });
                        match saved {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => eprintln!("Could not capture {path}: {error:#}"),
                            Err(error) => eprintln!("Could not reach the window: {error:#}"),
                        }
                        if screen {
                            let behind = if backdrop { "backdrop" } else { "desktop" };
                            let path = format!(
                                "{}-{}-{}-window-{}.png",
                                stem.display(),
                                sheet.name(),
                                theme_name,
                                behind
                            );
                            let region = format!("{x},{y},{width},{height}");
                            match std::process::Command::new("screencapture")
                                .args(["-x", "-R", &region, &path])
                                .status()
                            {
                                Ok(status) if status.success() => {}
                                Ok(status) => eprintln!("screencapture refused {path}: {status}"),
                                Err(error) => eprintln!("screencapture did not run: {error}"),
                            }
                        }
                    }
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    Ok(())
}
