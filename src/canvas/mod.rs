//! M1 developer surface. The view-less whiteboard Engine is always hosted:
//! it requests camera changes and emits commits; this host owns both values.
pub(crate) mod input;
pub(crate) mod surface;

use crate::page::Page;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Point, Render, Styled as _, Subscription, Task,
    Window, canvas, div, px, size,
};
use gpui_whiteboard::engine::{Engine, EngineEvent, Frame, Gesture, Role, View};
use gpui_whiteboard::{
    BoardThumbnailView, Camera, Element, ElementKind, EmbedGeom, LocalThumbnailSnapshot, Scene,
    WhiteboardEvent, WhiteboardStyle,
};
use input::SharedInput;
use std::rc::Rc;
use std::time::Duration;

const CARD: u64 = 1;
const CAPTION: f32 = 32.;

pub(crate) struct SpatialCanvas {
    page: Entity<Page>,
    painter: Entity<BoardThumbnailView>,
    engine: Engine,
    scene: Scene,
    camera: View,
    gesture: Option<Gesture>,
    selection: Vec<u64>,
    history: Vec<WhiteboardEvent>,
    focus: FocusHandle,
    input: SharedInput,
    bounds: Bounds<Pixels>,
    moving: bool,
    cached: bool,
    settle: Option<Task<()>>,
    _page_events: Subscription,
    _controls: Subscription,
}

impl SpatialCanvas {
    pub fn new(page: Entity<Page>, cx: &mut Context<Self>) -> Self {
        let world = page
            .read(cx)
            .bounds
            .map(|b| b.size)
            .unwrap_or(size(px(1280.), px(800.)));
        let input = SharedInput::default();
        let focus = cx.focus_handle();
        page.update(cx, |p, _| {
            p.canvas_input = Some(input.clone());
            p.canvas_focus = Some(focus.clone());
        });
        let scene = Scene {
            elements: vec![Element {
                id: CARD,
                kind: ElementKind::Embed(EmbedGeom {
                    page_id: 1,
                    title: page.read(cx).title().to_string(),
                    x: 48.,
                    y: 48.,
                    w: f32::from(world.width),
                    h: f32::from(world.height) + CAPTION,
                }),
                stroke: None,
                fill: None,
                label: None,
                label_color: None,
                styles: Vec::new(),
                mindmap: None,
            }],
            ..Default::default()
        };
        let theme = cx.theme().clone();
        let painter = cx.new(|_| {
            BoardThumbnailView::new(
                LocalThumbnailSnapshot::for_scene_all_content(scene.clone(), 1280., 800.),
                Rc::new(move || WhiteboardStyle {
                    bg: theme.background,
                    grid: theme.border,
                    text: theme.muted_foreground,
                    ink: theme.foreground,
                    panel: theme.secondary,
                    panel_strong: theme.popover,
                    accent: theme.accent,
                    selection: theme.primary,
                    swatches: vec![],
                }),
            )
        });
        let mut engine = Engine::new();
        engine.present(
            scene.clone(),
            [(CARD, Role::Card { connectable: false })],
            [],
        );
        let events = cx.observe(&page, |_, _, cx| cx.notify());
        let controls = cx.subscribe(&page, |this, _, event, cx| {
            if matches!(event, crate::page::PageEvent::RevealCanvas) {
                let r = this.card();
                let fit = (f32::from(this.bounds.size.width) as f64 / (r[2] + 96.))
                    .min(f32::from(this.bounds.size.height) as f64 / (r[3] + 96.));
                this.zoom_to(this.camera.zoom.min(fit), cx);
                this.reveal_page_point([r[2] as f32 / 2., (r[3] as f32 - CAPTION) / 2.], cx);
            }
        });
        Self {
            page,
            painter,
            engine,
            scene,
            camera: View::default(),
            gesture: None,
            selection: Vec::new(),
            history: Vec::new(),
            focus,
            input,
            bounds: Bounds::default(),
            moving: false,
            cached: false,
            settle: None,
            _page_events: events,
            _controls: controls,
        }
    }

    pub fn probe_state(&self) -> serde_json::Value {
        serde_json::json!({"camera": [self.camera.x, self.camera.y, self.camera.zoom],
            "card": self.card(), "selection": self.selection,
            "marquee": matches!(self.gesture, Some(Gesture::Marquee { .. })),
            "commits": self.history.len(), "board_drag": self.input.borrow().board_drag,
            "space": self.input.borrow().space.is_some(), "cached":self.cached})
    }

    pub fn set_cached(&mut self, cached: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.cached = cached;
        self.page.update(cx, |p, cx| {
            p.canvas_cached = cached;
            p.set_visible(!cached);
            p.camera_moved(cx);
        });
        let focus = if cached {
            self.focus.clone()
        } else {
            self.page.focus_handle(cx)
        };
        window.focus(&focus, cx);
        cx.notify();
    }

    pub fn detach(&self, cx: &mut App) {
        self.input.borrow_mut().cancel_space();
        self.page.update(cx, |page, cx| {
            page.canvas_cached = false;
            page.set_visible(true);
            page.canvas_input = None;
            page.canvas_focus = None;
            page.canvas_zoom = 1.;
            page.canvas_world = None;
            cx.notify();
        });
    }

    /// The page the canvas lays out.
    pub fn page(&self) -> &Entity<Page> {
        &self.page
    }

    fn frame(&self) -> Frame<'_> {
        Frame {
            selection: &self.selection,
            view: self.camera,
            engaged: None,
            hovered: None,
        }
    }

    fn local(&self, p: Point<Pixels>) -> [f64; 2] {
        [
            f32::from(p.x - self.bounds.origin.x) as f64,
            f32::from(p.y - self.bounds.origin.y) as f64,
        ]
    }

    fn card(&self) -> [f64; 4] {
        self.engine.preview(
            &self.gesture,
            CARD,
            self.engine.bounds(CARD).expect("M1 card"),
        )
    }

    fn apply(&mut self, events: Vec<EngineEvent>, cx: &mut Context<Self>) {
        for event in events {
            match event {
                EngineEvent::Camera(view) => {
                    self.camera = view;
                    self.moving = true;
                    self.page.update(cx, |p, cx| p.camera_moved(cx));
                    self.schedule_settle(cx);
                }
                EngineEvent::Select(selection) => self.selection = selection,
                EngineEvent::Commit(commit @ WhiteboardEvent::Commit { .. }) => {
                    if let WhiteboardEvent::Commit { after, .. } = &commit {
                        self.scene = after.clone();
                    }
                    self.history.push(commit);
                    self.engine.present(
                        self.scene.clone(),
                        [(CARD, Role::Card { connectable: false })],
                        [],
                    );
                }
                _ => {}
            }
        }
        cx.notify();
    }

    fn schedule_settle(&mut self, cx: &mut Context<Self>) {
        self.settle = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(180))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.gesture.is_none() {
                    this.moving = false;
                    cx.notify();
                }
            });
        }));
    }

    pub fn zoom_to(&mut self, zoom: f64, cx: &mut Context<Self>) {
        let event = self.engine.zoom_step(
            &self.frame(),
            [
                f32::from(self.bounds.size.width) as f64,
                f32::from(self.bounds.size.height) as f64,
            ],
            zoom / self.camera.zoom,
        );
        self.apply(vec![event], cx);
    }

    pub fn pan_by(&mut self, delta: [f64; 2], cx: &mut Context<Self>) {
        if let Some(event) = self.engine.wheel([0., 0.], delta, false, &self.frame()) {
            self.apply(vec![event], cx);
        }
    }

    pub fn reveal_page_point(&mut self, css: [f32; 2], cx: &mut Context<Self>) {
        let r = self.card();
        let screen = self
            .camera
            .to_screen([r[0] + css[0] as f64, r[1] + CAPTION as f64 + css[1] as f64]);
        self.pan_by(
            [
                f32::from(self.bounds.size.width) as f64 / 2. - screen[0],
                f32::from(self.bounds.size.height) as f64 / 2. - screen[1],
            ],
            cx,
        );
    }
}

impl Focusable for SpatialCanvas {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SpatialCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let r = self.card();
        let camera = Camera {
            x: self.camera.x as f32,
            y: self.camera.y as f32,
            zoom: self.camera.zoom as f32,
        };
        let mut scene = self.scene.clone();
        if let ElementKind::Embed(g) = &mut scene.elements[0].kind {
            g.x = r[0] as f32;
            g.y = r[1] as f32;
            g.w = r[2] as f32;
            g.h = r[3] as f32;
            g.title = self.page.read(cx).title().to_string();
        }
        let kind = scene.elements[0].kind.clone();
        let mut snapshot = LocalThumbnailSnapshot::for_scene_all_content(scene, 1280., 800.);
        snapshot.spec.camera = camera;
        self.painter.update(cx, |p, _| p.set_snapshot(snapshot));
        let z = camera.zoom;
        let pos = self.camera.to_screen([r[0], r[1] + CAPTION as f64]);
        let world = size(px(r[2] as f32), px(r[3] as f32 - CAPTION));
        self.page.update(cx, |p, _| {
            p.canvas_world = Some(world);
            p.canvas_zoom = z;
            p.canvas_settled = !self.cached && !self.moving && self.gesture.is_none();
            p.canvas_cap = window.viewport_size();
            p.corners = gpui_kit::Corners::all(px(8.));
        });
        let popup_page = self.page.downgrade();
        let popup = self
            .page
            .read(cx)
            .controls
            .popup
            .as_ref()
            .map(|(menu, position, _)| {
                gpui_kit::deferred(
                    gpui_kit::anchored()
                        .position(*position)
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            div().relative().child(menu.clone()).child(
                                gpui_kit::canvas(
                                    move |bounds, _, cx| {
                                        let _ = popup_page.update(cx, |p, _| {
                                            p.controls.popup_bounds = Some(bounds)
                                        });
                                    },
                                    |_, _, _, _| {},
                                )
                                // Without an inset, layout leaves it where it would
                                // flow: after the menu, at its bottom edge.
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full(),
                            ),
                        ),
                )
                .with_priority(2)
            });
        let selected = self.selection.contains(&CARD);
        let marquee = match &self.gesture {
            Some(Gesture::Marquee { start, now, .. }) => Some((*start, *now)),
            _ => None,
        };
        let color = cx.theme().primary;
        let weak = cx.weak_entity();
        div()
            .id("spatial-canvas")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .overflow_hidden()
            .capture_any_mouse_down(cx.listener(Self::press))
            .capture_any_mouse_up(cx.listener(Self::release))
            .on_mouse_move(cx.listener(Self::drag))
            .on_scroll_wheel(cx.listener(Self::wheel))
            .capture_pinch(cx.listener(|this, ev: &gpui_kit::PinchEvent, _, cx| {
                let mut view = this.camera;
                view.zoom_around(this.local(ev.position), (1. + ev.delta as f64).max(0.01));
                this.apply(vec![EngineEvent::Camera(view)], cx);
                cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|this, ev: &gpui_kit::KeyDownEvent, _, cx| {
                match ev.keystroke.key.as_str() {
                    "space" => this.input.borrow_mut().hold_space(&ev.keystroke),
                    "v" => this.input.borrow_mut().select_tool = true,
                    "h" => this.input.borrow_mut().select_tool = false,
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .on_key_up(cx.listener(|this, ev: &gpui_kit::KeyUpEvent, _, _| {
                if ev.keystroke.key == "space" {
                    this.input.borrow_mut().release_space();
                }
            }))
            .child(
                canvas(
                    move |b, _, cx| {
                        let _ = weak.update(cx, |s, _| s.bounds = b);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(div().absolute().inset_0().child(self.painter.clone()))
            .child(
                div()
                    .absolute()
                    .left(px(pos[0] as f32))
                    .top(px(pos[1] as f32))
                    .w(world.width * z)
                    .h(world.height * z)
                    .overflow_hidden()
                    .child(self.page.clone()),
            )
            .children(popup)
            .child(
                canvas(
                    |_, _, _| {},
                    move |b, _, window, _| {
                        if selected {
                            gpui_whiteboard::paint_selection(
                                &kind, camera, b.origin, color, window,
                            );
                        }
                        if let Some((a, c)) = marquee {
                            gpui_whiteboard::paint_marquee(
                                [a[0] as f32, a[1] as f32],
                                [c[0] as f32, c[1] as f32],
                                camera,
                                b.origin,
                                color,
                                window,
                            );
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}
