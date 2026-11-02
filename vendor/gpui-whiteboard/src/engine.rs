//! The board's gesture engine without a view: hit testing, hover handles,
//! drags, the marquee and the camera, over one presented [`Scene`].
//!
//! A host that paints its own surfaces (Theorem's spatial canvas mounts live
//! guests, not board shapes) still runs every pointer, wheel and camera key
//! through this engine, so one engine owns each gesture, camera move, hit
//! test and gesture commit for that surface. The engine keeps no store and no
//! selection of its own:
//!
//! - **Presented, not owned.** [`Engine::present`] installs a projection (the
//!   scene, each element's [`Role`], and the held elements). The host
//!   re-presents after every change it applies.
//! - **Transient state is the host's value.** A drag in progress is a
//!   [`Gesture`] in an `Option` the host keeps beside its store; selection and
//!   camera are read from a [`Frame`] the host builds per input.
//! - **One gesture, one commit.** A finished move, resize or nudge is one
//!   [`WhiteboardEvent::Commit`] carrying the scene before and after, the same
//!   event a hosted [`WhiteboardView`](crate::WhiteboardView) reports, so the
//!   host's converter (for a Space, [`crate::space::SpaceBoard::route`]) makes
//!   it one history entry.
//! - **Camera is a request.** Pan, wheel and zoom keys return
//!   [`EngineEvent::Camera`] in `f64`; nothing here writes a camera.
//! - **Held elements never move.** A gesture that includes a held element
//!   moves nothing in preview and reports [`EngineEvent::Held`] on release,
//!   all or nothing, as the host's own authority refuses it.
//!
//! Coordinates: world space, `y` down; the camera's origin is the world point
//! at the surface's top-left corner, so a screen point `s` is the world point
//! `origin + s / zoom`, the convention of [`Camera`](crate::Camera) and of
//! `theorem_space::geom::Camera`.

use std::collections::{HashMap, HashSet};

use crate::geometry::{bbox, translate};
use crate::{Element, ElementKind, Scene, WhiteboardEvent};

/// The one zoom range for a board and a Space (Canvas 2.5 decision, recorded
/// in docs/records/2026-10-04-canvas-2-5-implementation.md): equal to
/// `theorem_space::geom::{MIN_ZOOM, MAX_ZOOM}`; a test keeps them equal.
pub const MIN_ZOOM: f64 = 0.02;
pub const MAX_ZOOM: f64 = 4.0;
/// Screen pixels a press may travel before it becomes a drag.
pub const DRAG_SLOP: f64 = 3.0;
/// Screen radius of a side connection dot and a corner resize handle.
pub const HANDLE_RADIUS: f64 = 9.0;
/// Smallest card or region a resize may produce, in world units.
pub const MIN_SIZE: (f64, f64) = (60.0, 40.0);
/// Nudge distance in world units: plain, and further.
pub const NUDGE: (f64, f64) = (10.0, 50.0);
/// Keyboard pan in screen pixels.
pub const KEY_PAN: f64 = 80.0;
/// Wheel units per e-fold of zoom.
pub const WHEEL_ZOOM: f64 = 0.0025;

/// A device-local camera at full precision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl Default for View {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    }
}

impl View {
    pub fn to_world(&self, screen: [f64; 2]) -> [f64; 2] {
        [
            self.x + screen[0] / self.zoom,
            self.y + screen[1] / self.zoom,
        ]
    }

    pub fn to_screen(&self, world: [f64; 2]) -> [f64; 2] {
        [
            (world[0] - self.x) * self.zoom,
            (world[1] - self.y) * self.zoom,
        ]
    }

    /// Move the content by a screen delta: it follows the pointer.
    pub fn pan_screen(&mut self, dx: f64, dy: f64) {
        self.x -= dx / self.zoom;
        self.y -= dy / self.zoom;
    }

    /// Multiply the zoom by `factor` within the range, keeping the world point
    /// under the screen point `anchor` fixed.
    pub fn zoom_around(&mut self, anchor: [f64; 2], factor: f64) {
        let before = self.to_world(anchor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.x = before[0] - anchor[0] / self.zoom;
        self.y = before[1] - anchor[1] / self.zoom;
    }
}

/// A world rect `[x, y, w, h]`.
pub type Bounds = [f64; 4];

/// What an element is to the engine.
#[derive(Clone, Debug, PartialEq)]
pub enum Role {
    /// A surface the pointer selects, moves and resizes; `connectable` ones
    /// offer side dots that draw a connection.
    Card { connectable: bool },
    /// A region drawn behind cards. Dragging it moves it with `members`, the
    /// stored member set (not whatever happens to lie inside it).
    Region { members: Vec<u64> },
    /// Painted, never hit (connections follow the cards they join).
    Inert,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];

    pub fn of(self, r: &Bounds) -> [f64; 2] {
        match self {
            Corner::TopLeft => [r[0], r[1]],
            Corner::TopRight => [r[0] + r[2], r[1]],
            Corner::BottomLeft => [r[0], r[1] + r[3]],
            Corner::BottomRight => [r[0] + r[2], r[1] + r[3]],
        }
    }

    /// `r` with this corner moved to `to`, never smaller than `min`.
    pub fn drag(self, r: &Bounds, to: [f64; 2], min: (f64, f64)) -> Bounds {
        let (mut left, mut top, mut right, mut bottom) = (r[0], r[1], r[0] + r[2], r[1] + r[3]);
        match self {
            Corner::TopLeft => {
                left = to[0].min(right - min.0);
                top = to[1].min(bottom - min.1);
            }
            Corner::TopRight => {
                right = to[0].max(left + min.0);
                top = to[1].min(bottom - min.1);
            }
            Corner::BottomLeft => {
                left = to[0].min(right - min.0);
                bottom = to[1].max(top + min.1);
            }
            Corner::BottomRight => {
                right = to[0].max(left + min.0);
                bottom = to[1].max(top + min.1);
            }
        }
        [left, top, right - left, bottom - top]
    }
}

/// A card side, in the board's connector order (top, right, bottom, left).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

    /// Where an edge leaving through this side attaches: the side's midpoint.
    pub fn anchor(self, r: &Bounds) -> [f64; 2] {
        match self {
            Side::Top => [r[0] + r[2] / 2.0, r[1]],
            Side::Right => [r[0] + r[2], r[1] + r[3] / 2.0],
            Side::Bottom => [r[0] + r[2] / 2.0, r[1] + r[3]],
            Side::Left => [r[0], r[1] + r[3] / 2.0],
        }
    }
}

/// What the pointer is over, handles first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hover {
    Card(u64),
    Region(u64),
    Resize(u64, Corner),
    Connect(u64, Side),
}

/// What a world point hits: the topmost card, else the smallest region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Card(u64),
    Region(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Primary,
    Secondary,
    Middle,
}

/// Modifiers held at a press.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    /// The additive modifier (shift or the platform key).
    pub extend: bool,
    pub alt: bool,
    /// A pan key (Space) is held: a primary drag pans.
    pub pan_held: bool,
}

/// A drag in progress. The host keeps it in an `Option` beside its store.
#[derive(Clone, Debug, PartialEq)]
pub enum Gesture {
    Pan {
        last: [f64; 2],
    },
    /// A secondary press not yet resolved: released within the slop it is a
    /// context request where it was pressed; moved past it, it pans.
    Secondary {
        start: [f64; 2],
        extend: bool,
    },
    /// World corners of the marquee, and the selection it adds to.
    Marquee {
        start: [f64; 2],
        now: [f64; 2],
        additive: bool,
        base: Vec<u64>,
    },
    /// Cards and regions moving together, from their press-time positions.
    Move {
        start: [f64; 2],
        now: [f64; 2],
        elements: Vec<u64>,
        /// A region drag ("move group") rather than a card drag.
        region: bool,
        /// A press on an already selected card in a multi-selection selects
        /// only that card if released without moving.
        narrow: Option<u64>,
        /// Elements of the set the host holds fixed: the gesture moves none.
        held: Vec<u64>,
    },
    Resize {
        element: u64,
        corner: Corner,
        origin: Bounds,
        now: [f64; 2],
    },
    Connect {
        from: u64,
        side: Side,
        now: [f64; 2],
    },
}

/// What the host reads for one input: the selection, the camera, an engaged
/// card (its content owns its own pointer) and the last hover.
#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    pub selection: &'a [u64],
    pub view: View,
    pub engaged: Option<u64>,
    pub hovered: Option<&'a Hover>,
}

/// What one input asks of the host.
#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    /// A finished gesture: one commit, so one history entry.
    Commit(WhiteboardEvent),
    /// Device-local camera. Never a content change.
    Camera(View),
    /// Replace the selection.
    Select(Vec<u64>),
    /// A double press on a card.
    Open(u64),
    /// A double press on a region.
    EditRegion(u64),
    /// A double press on empty ground, at this world point.
    Create([f64; 2]),
    /// A secondary press released in place: `target` is the card under it,
    /// `extend` whether the additive modifier was held at the press.
    Context {
        at: [f64; 2],
        target: Option<u64>,
        extend: bool,
    },
    /// A connection drawn from one card's side dot onto another card.
    Connect { from: u64, side: Side, to: u64 },
    /// A move of held elements was refused: the elements and the world delta
    /// the gesture asked for.
    Held {
        elements: Vec<u64>,
        delta: [f64; 2],
        label: String,
    },
}

/// The board's gesture engine over one presented scene.
#[derive(Clone, Debug, Default)]
pub struct Engine {
    scene: Scene,
    roles: HashMap<u64, Role>,
    held: HashSet<u64>,
    index: HashMap<u64, usize>,
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn contains(r: &Bounds, p: [f64; 2]) -> bool {
    p[0] >= r[0] && p[0] <= r[0] + r[2] && p[1] >= r[1] && p[1] <= r[1] + r[3]
}

fn within(outer: &Bounds, inner: &Bounds) -> bool {
    inner[0] >= outer[0]
        && inner[1] >= outer[1]
        && inner[0] + inner[2] <= outer[0] + outer[2]
        && inner[1] + inner[3] <= outer[1] + outer[3]
}

fn corners(a: [f64; 2], b: [f64; 2]) -> Bounds {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        (a[0] - b[0]).abs(),
        (a[1] - b[1]).abs(),
    ]
}

/// An element's world bounds: a card's or region's frame exactly, else its
/// drawn extent.
fn bounds_of(kind: &ElementKind) -> Bounds {
    match kind {
        ElementKind::Embed(g) => [
            f64::from(g.x),
            f64::from(g.y),
            f64::from(g.w),
            f64::from(g.h),
        ],
        ElementKind::Rect(g) => [
            f64::from(g.x),
            f64::from(g.y),
            f64::from(g.w),
            f64::from(g.h),
        ],
        other => {
            let (x0, y0, x1, y1) = bbox(other);
            [
                f64::from(x0),
                f64::from(y0),
                f64::from(x1 - x0),
                f64::from(y1 - y0),
            ]
        }
    }
}

/// Set a card's or region's frame; other kinds translate to the new origin.
fn set_bounds(kind: &mut ElementKind, r: Bounds) {
    let (x, y, w, h) = (r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32);
    match kind {
        ElementKind::Embed(g) => {
            (g.x, g.y, g.w, g.h) = (x, y, w, h);
        }
        ElementKind::Rect(g) => {
            (g.x, g.y, g.w, g.h) = (x, y, w, h);
        }
        other => {
            let (x0, y0, _, _) = bbox(other);
            translate(other, x - x0, y - y0);
        }
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Install a projection: the scene, each element's role (an element with
    /// none is inert), and the elements no gesture may move.
    pub fn present(
        &mut self,
        scene: Scene,
        roles: impl IntoIterator<Item = (u64, Role)>,
        held: impl IntoIterator<Item = u64>,
    ) {
        self.index = scene
            .elements
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        self.scene = scene;
        self.roles = roles.into_iter().collect();
        self.held = held.into_iter().collect();
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn role(&self, id: u64) -> Option<&Role> {
        self.roles.get(&id)
    }

    pub fn is_held(&self, id: u64) -> bool {
        self.held.contains(&id)
    }

    fn element(&self, id: u64) -> Option<&Element> {
        self.index.get(&id).map(|&i| &self.scene.elements[i])
    }

    /// An element's world bounds.
    pub fn bounds(&self, id: u64) -> Option<Bounds> {
        self.element(id).map(|e| bounds_of(&e.kind))
    }

    fn is_card(&self, id: u64) -> bool {
        matches!(self.roles.get(&id), Some(Role::Card { .. }))
    }

    fn members(&self, region: u64) -> &[u64] {
        match self.roles.get(&region) {
            Some(Role::Region { members }) => members,
            _ => &[],
        }
    }

    /// The one hit test: the topmost card under `world`, else the smallest
    /// region holding it.
    pub fn hit(&self, world: [f64; 2]) -> Option<Hit> {
        let mut region: Option<(f64, u64)> = None;
        for e in self.scene.elements.iter().rev() {
            match self.roles.get(&e.id) {
                Some(Role::Card { .. }) => {
                    if contains(&bounds_of(&e.kind), world) {
                        return Some(Hit::Card(e.id));
                    }
                }
                Some(Role::Region { .. }) => {
                    let r = bounds_of(&e.kind);
                    let area = r[2] * r[3];
                    if contains(&r, world) && region.is_none_or(|(a, _)| area < a) {
                        region = Some((area, e.id));
                    }
                }
                _ => {}
            }
        }
        region.map(|(_, id)| Hit::Region(id))
    }

    /// Cards wholly inside `world`, in paint order.
    pub fn cards_within(&self, world: &Bounds) -> Vec<u64> {
        self.scene
            .elements
            .iter()
            .filter(|e| self.is_card(e.id) && within(world, &bounds_of(&e.kind)))
            .map(|e| e.id)
            .collect()
    }

    /// What lies under a screen point, handles of the selected and hovered
    /// cards first.
    pub fn hover(&self, at: [f64; 2], frame: &Frame<'_>) -> Option<Hover> {
        let view = frame.view;
        let hovered = frame.hovered.and_then(|h| match h {
            Hover::Card(id) | Hover::Connect(id, _) | Hover::Resize(id, _) => Some(*id),
            Hover::Region(_) => None,
        });
        let screen = |r: &Bounds| {
            let p = view.to_screen([r[0], r[1]]);
            [p[0], p[1], r[2] * view.zoom, r[3] * view.zoom]
        };
        for e in self.scene.elements.iter().rev() {
            let Some(Role::Card { connectable }) = self.roles.get(&e.id) else {
                continue;
            };
            let selected = frame.selection.contains(&e.id);
            if !(selected || hovered == Some(e.id)) || frame.engaged == Some(e.id) {
                continue;
            }
            let r = screen(&bounds_of(&e.kind));
            if selected && !self.held.contains(&e.id) {
                for corner in Corner::ALL {
                    if distance(corner.of(&r), at) <= HANDLE_RADIUS {
                        return Some(Hover::Resize(e.id, corner));
                    }
                }
            }
            if *connectable {
                for side in Side::ALL {
                    if distance(side.anchor(&r), at) <= HANDLE_RADIUS {
                        return Some(Hover::Connect(e.id, side));
                    }
                }
            }
        }
        match self.hit(view.to_world(at))? {
            Hit::Card(id) => Some(Hover::Card(id)),
            Hit::Region(id) => {
                let r = screen(&self.bounds(id)?);
                if !self.held.contains(&id)
                    && distance(Corner::BottomRight.of(&r), at) <= HANDLE_RADIUS
                {
                    return Some(Hover::Resize(id, Corner::BottomRight));
                }
                Some(Hover::Region(id))
            }
        }
    }

    /// A press at screen point `at`. Starts a gesture in `gesture` and returns
    /// what the press itself asks (a selection change, an open, a create).
    pub fn press(
        &self,
        gesture: &mut Option<Gesture>,
        at: [f64; 2],
        button: Button,
        mods: Mods,
        clicks: usize,
        frame: &Frame<'_>,
    ) -> Vec<EngineEvent> {
        let mut out = Vec::new();
        let world = frame.view.to_world(at);
        let pans = button == Button::Middle || mods.pan_held || mods.alt;
        if button == Button::Secondary && !pans {
            *gesture = Some(Gesture::Secondary {
                start: at,
                extend: mods.extend,
            });
            return out;
        }
        if button != Button::Primary || pans {
            *gesture = Some(Gesture::Pan { last: at });
            return out;
        }
        match self.hover(at, frame) {
            Some(Hover::Resize(element, corner)) => {
                if let Some(origin) = self.bounds(element) {
                    *gesture = Some(Gesture::Resize {
                        element,
                        corner,
                        origin,
                        now: world,
                    });
                }
            }
            Some(Hover::Connect(from, side)) => {
                *gesture = Some(Gesture::Connect {
                    from,
                    side,
                    now: world,
                });
            }
            Some(Hover::Card(id)) => {
                if frame.engaged == Some(id) {
                    return out;
                }
                if clicks >= 2 {
                    out.push(EngineEvent::Open(id));
                    return out;
                }
                let current = frame.selection;
                let was = current.contains(&id);
                let next: Vec<u64> = if mods.extend {
                    if was {
                        current.iter().copied().filter(|p| *p != id).collect()
                    } else {
                        current.iter().copied().chain([id]).collect()
                    }
                } else if was {
                    current.to_vec()
                } else {
                    vec![id]
                };
                if next != current {
                    out.push(EngineEvent::Select(next.clone()));
                }
                if next.contains(&id) {
                    let narrow = (was && !mods.extend).then_some(id);
                    *gesture = Some(self.begin_move(world, next, false, narrow));
                }
            }
            Some(Hover::Region(region)) => {
                if clicks >= 2 {
                    out.push(EngineEvent::EditRegion(region));
                    return out;
                }
                let members = self.members(region).to_vec();
                out.push(EngineEvent::Select(members.clone()));
                let elements = std::iter::once(region).chain(members).collect();
                *gesture = Some(self.begin_move(world, elements, true, None));
            }
            None => {
                if clicks >= 2 {
                    out.push(EngineEvent::Create(world));
                    return out;
                }
                let base = if mods.extend {
                    frame.selection.to_vec()
                } else {
                    if !frame.selection.is_empty() {
                        out.push(EngineEvent::Select(Vec::new()));
                    }
                    Vec::new()
                };
                *gesture = Some(Gesture::Marquee {
                    start: world,
                    now: world,
                    additive: mods.extend,
                    base,
                });
            }
        }
        out
    }

    fn begin_move(
        &self,
        world: [f64; 2],
        elements: Vec<u64>,
        region: bool,
        narrow: Option<u64>,
    ) -> Gesture {
        let elements: Vec<u64> = elements
            .into_iter()
            .filter(|id| self.index.contains_key(id))
            .collect();
        let held = elements
            .iter()
            .copied()
            .filter(|id| self.held.contains(id))
            .collect();
        Gesture::Move {
            start: world,
            now: world,
            elements,
            region,
            narrow,
            held,
        }
    }

    /// The pointer moved to `at` with a button down. Returns a camera request
    /// while panning; otherwise the gesture's preview moves.
    pub fn drag_to(
        &self,
        gesture: &mut Option<Gesture>,
        at: [f64; 2],
        frame: &Frame<'_>,
    ) -> Vec<EngineEvent> {
        let world = frame.view.to_world(at);
        if let Some(Gesture::Secondary { start, .. }) = gesture {
            if distance(*start, at) < DRAG_SLOP {
                return Vec::new();
            }
            let start = *start;
            *gesture = Some(Gesture::Pan { last: start });
        }
        match gesture {
            Some(Gesture::Pan { last }) => {
                let (dx, dy) = (at[0] - last[0], at[1] - last[1]);
                *last = at;
                let mut next = frame.view;
                next.pan_screen(dx, dy);
                vec![EngineEvent::Camera(next)]
            }
            Some(Gesture::Marquee { now, .. })
            | Some(Gesture::Move { now, .. })
            | Some(Gesture::Resize { now, .. })
            | Some(Gesture::Connect { now, .. }) => {
                *now = world;
                Vec::new()
            }
            Some(Gesture::Secondary { .. }) | None => Vec::new(),
        }
    }

    /// The button came up at `at`: finish the gesture.
    pub fn release(
        &self,
        gesture: &mut Option<Gesture>,
        at: [f64; 2],
        frame: &Frame<'_>,
    ) -> Vec<EngineEvent> {
        let view = frame.view;
        let world = view.to_world(at);
        let Some(done) = gesture.take() else {
            return Vec::new();
        };
        match done {
            Gesture::Pan { .. } => Vec::new(),
            Gesture::Secondary { start, extend } => {
                let target = self.hover(start, frame).and_then(|h| match h {
                    Hover::Card(id) | Hover::Connect(id, _) => Some(id),
                    Hover::Resize(id, _) if self.is_card(id) => Some(id),
                    _ => None,
                });
                vec![EngineEvent::Context {
                    at: view.to_world(start),
                    target,
                    extend,
                }]
            }
            Gesture::Marquee {
                start,
                additive,
                base,
                ..
            } => {
                let rect = corners(start, world);
                if rect[2] * view.zoom < DRAG_SLOP && rect[3] * view.zoom < DRAG_SLOP {
                    return Vec::new();
                }
                let mut next = if additive { base } else { Vec::new() };
                for id in self.cards_within(&rect) {
                    if !next.contains(&id) {
                        next.push(id);
                    }
                }
                vec![EngineEvent::Select(next)]
            }
            Gesture::Move {
                start,
                elements,
                region,
                narrow,
                held,
                ..
            } => {
                let (dx, dy) = (world[0] - start[0], world[1] - start[1]);
                if (dx * view.zoom).abs() < DRAG_SLOP && (dy * view.zoom).abs() < DRAG_SLOP {
                    return narrow
                        .map(|only| vec![EngineEvent::Select(vec![only])])
                        .unwrap_or_default();
                }
                let label = if region { "move group" } else { "move" };
                if !held.is_empty() {
                    return vec![EngineEvent::Held {
                        elements: held,
                        delta: [dx, dy],
                        label: label.to_owned(),
                    }];
                }
                self.translate(&elements, dx, dy, label)
                    .into_iter()
                    .collect()
            }
            Gesture::Resize {
                element,
                corner,
                origin,
                ..
            } => {
                let rect = corner.drag(&origin, world, MIN_SIZE);
                if rect == origin {
                    return Vec::new();
                }
                self.commit("resize", &[(element, rect)])
                    .into_iter()
                    .collect()
            }
            Gesture::Connect { from, side, .. } => match self.hit(world) {
                Some(Hit::Card(to)) if to != from => vec![EngineEvent::Connect { from, side, to }],
                _ => Vec::new(),
            },
        }
    }

    /// Move `elements` by a world delta as one commit (`label` names it).
    /// Refused as a whole, as [`EngineEvent::Held`], when one is held.
    pub fn translate(
        &self,
        elements: &[u64],
        dx: f64,
        dy: f64,
        label: &str,
    ) -> Option<EngineEvent> {
        let held: Vec<u64> = elements
            .iter()
            .copied()
            .filter(|id| self.held.contains(id))
            .collect();
        if !held.is_empty() {
            return Some(EngineEvent::Held {
                elements: held,
                delta: [dx, dy],
                label: label.to_owned(),
            });
        }
        let moved: Vec<(u64, Bounds)> = elements
            .iter()
            .filter_map(|id| {
                let r = self.bounds(*id)?;
                Some((*id, [r[0] + dx, r[1] + dy, r[2], r[3]]))
            })
            .collect();
        self.commit(label, &moved)
    }

    /// Nudge the selected cards one step (`far`: a longer one).
    pub fn nudge(&self, frame: &Frame<'_>, dir: [f64; 2], far: bool) -> Option<EngineEvent> {
        let step = if far { NUDGE.1 } else { NUDGE.0 };
        let cards: Vec<u64> = frame
            .selection
            .iter()
            .copied()
            .filter(|id| self.is_card(*id))
            .collect();
        if cards.is_empty() {
            return None;
        }
        self.translate(&cards, dir[0] * step, dir[1] * step, "nudge")
    }

    /// One commit: the scene now, and the scene with these frames replaced.
    fn commit(&self, label: &str, frames: &[(u64, Bounds)]) -> Option<EngineEvent> {
        let mut after = self.scene.clone();
        let mut changed = false;
        for (id, r) in frames {
            let Some(&i) = self.index.get(id) else {
                continue;
            };
            let before = after.elements[i].kind.clone();
            set_bounds(&mut after.elements[i].kind, *r);
            changed |= after.elements[i].kind != before;
        }
        changed.then(|| {
            EngineEvent::Commit(WhiteboardEvent::Commit {
                before: self.scene.clone(),
                after,
                label: label.to_owned(),
            })
        })
    }

    /// A wheel step at `at`: pan by the delta, or zoom around the pointer.
    /// Scrolling up with the zoom modifier zooms in.
    pub fn wheel(
        &self,
        at: [f64; 2],
        delta: [f64; 2],
        zoom: bool,
        frame: &Frame<'_>,
    ) -> Option<EngineEvent> {
        let mut next = frame.view;
        if zoom {
            next.zoom_around(at, (delta[1] * WHEEL_ZOOM).exp());
        } else {
            next.pan_screen(delta[0], delta[1]);
        }
        (next != frame.view).then_some(EngineEvent::Camera(next))
    }

    /// Zoom by `factor` around the centre of a viewport of `size` (screen).
    pub fn zoom_step(&self, frame: &Frame<'_>, size: [f64; 2], factor: f64) -> EngineEvent {
        let mut next = frame.view;
        next.zoom_around([size[0] / 2.0, size[1] / 2.0], factor);
        EngineEvent::Camera(next)
    }

    /// Back to 100% around the viewport centre.
    pub fn zoom_reset(&self, frame: &Frame<'_>, size: [f64; 2]) -> EngineEvent {
        self.zoom_step(frame, size, 1.0 / frame.view.zoom)
    }

    /// Pan one keyboard step in direction `dir` (the content moves against it).
    pub fn pan_step(&self, frame: &Frame<'_>, dir: [f64; 2]) -> EngineEvent {
        let mut next = frame.view;
        next.pan_screen(-dir[0] * KEY_PAN, -dir[1] * KEY_PAN);
        EngineEvent::Camera(next)
    }

    /// An element's frame this frame: moved or resized by the gesture in
    /// progress. Held elements and untouched ones keep `stored`.
    pub fn preview(&self, gesture: &Option<Gesture>, id: u64, stored: Bounds) -> Bounds {
        match gesture {
            Some(Gesture::Move {
                start,
                now,
                elements,
                held,
                ..
            }) if held.is_empty() && elements.contains(&id) => [
                stored[0] + now[0] - start[0],
                stored[1] + now[1] - start[1],
                stored[2],
                stored[3],
            ],
            Some(Gesture::Resize {
                element,
                corner,
                origin,
                now,
            }) if *element == id => corner.drag(origin, *now, MIN_SIZE),
            _ => stored,
        }
    }

    /// The marquee's world rect while one is drawn.
    pub fn marquee(gesture: &Option<Gesture>) -> Option<Bounds> {
        match gesture {
            Some(Gesture::Marquee { start, now, .. }) => Some(corners(*start, *now)),
            _ => None,
        }
    }

    /// A pending connection's world endpoints while one is drawn.
    pub fn pending_connection(&self, gesture: &Option<Gesture>) -> Option<([f64; 2], [f64; 2])> {
        match gesture {
            Some(Gesture::Connect { from, side, now }) => {
                Some((side.anchor(&self.bounds(*from)?), *now))
            }
            _ => None,
        }
    }

    /// Whether the gesture moves content (not a pan or a pending press).
    pub fn edits(gesture: &Option<Gesture>) -> bool {
        matches!(
            gesture,
            Some(
                Gesture::Move { .. }
                    | Gesture::Resize { .. }
                    | Gesture::Marquee { .. }
                    | Gesture::Connect { .. }
            )
        )
    }

    /// Whether the gesture is a pan.
    pub fn pans(gesture: &Option<Gesture>) -> bool {
        matches!(gesture, Some(Gesture::Pan { .. }))
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
