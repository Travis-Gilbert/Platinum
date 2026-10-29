//! The Space adapter (Canvas 2.5 8.2): the board as the mechanical engine of
//! one presented Space, never as a second store.
//!
//! A [`SpaceBoard`] keeps nothing that a Space does not already own. It
//! **projects** a `theorem_space::Space` (placements, groups, nested Space
//! portals, freehand ink and the three connection classes) into a [`Scene`],
//! and it **converts** each finished whiteboard gesture back into intents for
//! the owners: exactly one Space-history [`SpaceCommand`] per gesture, plus any
//! command that belongs to another owner (a semantic relationship goes to its
//! relationship authority, an executable binding to the programmable graph).
//! The host applies those once and re-projects; the board never writes its own
//! copy of the content.
//!
//! The ownership split this module enforces:
//!
//! - **Undo.** The board runs in [`HistoryMode::Hosted`](crate::HistoryMode),
//!   so a gesture is one [`WhiteboardEvent::Commit`] and therefore one Space
//!   history entry. Undo and redo requests become `SpaceCommand::Undo` and
//!   `SpaceCommand::Redo`.
//! - **Camera.** The board runs in [`CameraMode::Hosted`](crate::CameraMode).
//!   A camera request becomes [`Route::Camera`]: device-local view state the
//!   host stores in its navigator. It never produces a command and never
//!   touches the shared Space.
//! - **Hit testing.** [`SpaceBoard::hit`] answers "what is under this point"
//!   with a Space identity, over the same projection the board paints.
//! - **Geometry states.** A pinned placement is a hard constraint: its
//!   elements are handed to the view as held (no gesture offers to move them)
//!   and any commit that would move it anyway is refused here, before the
//!   Space refuses it again. Cards and groups are axis-aligned live surfaces;
//!   they are never rotated.
//! - **Ink.** Freehand drawing and shapes are stored through the existing
//!   owner, `PlacementTarget::Ink`, as versioned opaque elements in placement
//!   local coordinates. An element this build cannot read, or a drawing in a
//!   format it does not know, is kept verbatim and written back unchanged; it
//!   is never replaced by an empty drawing.
//!
//! One `SpaceBoard` serves one Space. Opening a nested Space (a portal card)
//! presents the child with its own board, so placement identities, which are
//! unique only within one Space, never mix.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use gpui::Context;
use serde_json::Value;
use theorem_space::geom::{Point as SpacePoint, Rect, Side};
use theorem_space::space::RouteHint;
use theorem_space::{
    Actor, Authorship, BindingAuthority, BindingState, CommandContext, CommandError,
    ConnectionClass, ConnectionId, End, GroupId, INK_FORMAT, InkContent, Placement, PlacementId,
    PlacementState, PlacementTarget, PresentationMode, ProjectedConnection, Receipt,
    RelationshipAuthority, Space, SpaceCommand, SpaceError, SpaceId, SpaceOp, SpaceSession,
};

use crate::engine::{Role, View};
use crate::geometry::{bbox, rotate_element, translate};
use crate::{
    BoxGeom, Camera, CameraMode, Element, ElementKind, EmbedGeom, HistoryMode, Scene,
    SceneParseError, SegGeom, SegmentAnchor, SegmentStyle, WhiteboardEvent, WhiteboardView,
    mechanics,
};

/// World-space padding between a drawing's content and its placement frame.
pub const INK_PAD: f32 = 8.0;
/// Outline width of a projected group.
const GROUP_STROKE: f32 = 1.0;
/// Width of a projected connection.
const EDGE_WIDTH: f32 = 1.5;
/// Stroke of an executable binding by state (packed `0xRRGGBBAA`). Presentation
/// connections follow the theme ink and semantic ones are dashed, so each of
/// the three classes reads differently (spec 16.4).
const BINDING_CURRENT: u32 = 0x2f6fd0ff;
const BINDING_STALE: u32 = 0xc08a1eff;
const BINDING_FAILED: u32 = 0xc0392bff;
/// Two coordinates closer than this are the same position. Gestures accumulate
/// per-frame `f32` deltas, so an exact comparison would miss a pure translation.
const EPS: f64 = 1e-3;

/// The class of a drawn connection, by owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EdgeClass {
    /// Presentation-only, owned by this Space.
    Presentation,
    /// A canonical semantic relationship, owned by the relationship authority.
    Semantic,
    /// An executable binding, owned by the programmable graph.
    Executable,
}

impl From<&ConnectionClass> for EdgeClass {
    fn from(class: &ConnectionClass) -> Self {
        match class {
            ConnectionClass::Presentation => Self::Presentation,
            ConnectionClass::Semantic { .. } => Self::Semantic,
            ConnectionClass::Executable { .. } => Self::Executable,
        }
    }
}

/// The Space identity behind one projected element. Element ids are renderer
/// handles; these are the identities (spec 7).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Handle {
    /// A Placement drawn as a card: an object, a note, a nested Space portal,
    /// or a drawing this build cannot show element by element.
    Placement(PlacementId),
    /// A group (region).
    Group(GroupId),
    /// One drawn connection. A semantic relationship or a binding is drawn once
    /// per pair of placements of its endpoints, so the pair is part of it.
    Edge {
        class: EdgeClass,
        key: String,
        from: PlacementId,
        to: PlacementId,
    },
    /// One element of a drawing, by the id it carries inside the stored ink.
    Ink { placement: PlacementId, local: u64 },
}

impl Handle {
    /// The placement this handle draws, if it draws one.
    pub fn placement(&self) -> Option<&PlacementId> {
        match self {
            Handle::Placement(id) | Handle::Ink { placement: id, .. } => Some(id),
            Handle::Group(_) | Handle::Edge { .. } => None,
        }
    }
}

/// Ink kept verbatim because this build cannot read it.
#[derive(Clone, Debug, PartialEq)]
pub struct RetainedInk {
    pub placement: PlacementId,
    /// Position inside the drawing's element list; `None` when the whole
    /// drawing is in a format this build does not know.
    pub index: Option<usize>,
    pub error: String,
}

/// One projection of a Space: what the board paints, and the constraints it
/// enforces. Hand it to [`WhiteboardView::present`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Projection {
    pub scene: Scene,
    /// Elements of pinned placements: no gesture may move them.
    pub held: Vec<u64>,
    /// Elements that are axis-aligned live surfaces or regions: never rotated.
    pub axis_aligned: Vec<u64>,
    /// Ink this build could not read, kept verbatim in the Space. Non-empty
    /// means part of a drawing is not shown; surface it to the user.
    pub retained: Vec<RetainedInk>,
    /// Every element id the board has ever bound for this Space is below this.
    /// The view mints new elements at or above it, so a new element never
    /// takes the id of an identity that is off the board right now (a
    /// removed placement that undo can bring back).
    pub id_floor: u64,
    /// What each element is to the gesture [`Engine`](crate::engine::Engine):
    /// cards (connectable when they show an object, a note or a Space),
    /// regions with their stored member set, and inert connections.
    pub roles: Vec<(u64, Role)>,
}

impl Projection {
    /// Install this projection in a gesture engine.
    pub fn present(&self, engine: &mut crate::engine::Engine) {
        engine.present(
            self.scene.clone(),
            self.roles.iter().cloned(),
            self.held.iter().copied(),
        );
    }
}

/// Why a gesture was refused. The host re-presents the last projection, which
/// puts the board back exactly where the Space is.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    /// The gesture would move, resize or turn a pinned placement.
    Pinned { placement: PlacementId },
    /// The gesture would rotate an axis-aligned card or group.
    AxisAligned { handle: Handle },
    /// The gesture would move an owner's relationship or binding to other
    /// objects. Relationships are re-pointed through their owner, not drawn.
    OwnerEndpoint { key: String },
    /// The drawing holds content this build cannot read, and the edit would
    /// re-base it. Re-basing would misplace the unread content.
    RetainedContent { placement: PlacementId },
    /// Nothing the gesture changed has a Space meaning.
    Unsupported { reasons: Vec<String> },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Pinned { placement } => {
                write!(f, "{placement} is pinned; release the pin to move it")
            }
            Refusal::AxisAligned { handle } => {
                write!(f, "{handle:?} supports axis-aligned geometry only")
            }
            Refusal::OwnerEndpoint { key } => write!(
                f,
                "{key} belongs to its owner; change its endpoints there, not by drawing"
            ),
            Refusal::RetainedContent { placement } => write!(
                f,
                "{placement} holds content this build cannot read and cannot be re-framed"
            ),
            Refusal::Unsupported { reasons } => write!(f, "{}", reasons.join("; ")),
        }
    }
}

/// What the host does with one board event.
#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    /// Apply once, in order: the gesture's single Space-history command (if
    /// any), then each owner command. `ignored` lists changes that had no Space
    /// meaning and will snap back when the host re-presents.
    Commands {
        space: Option<SpaceCommand>,
        owners: Vec<SpaceCommand>,
        ignored: Vec<String>,
    },
    /// Device-local camera for the navigator's view of this Space. No command.
    Camera(theorem_space::Camera),
    /// Refused: re-present the last projection.
    Refused(Refusal),
    /// The event changes nothing.
    Nothing,
}

/// What [`apply`] did.
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub space: Option<Receipt>,
    pub owners: Vec<Receipt>,
}

/// One stored drawing, decoded.
struct Drawing {
    /// Readable elements in local coordinates, with their stored index and the
    /// stored JSON (written back verbatim while the element is unchanged).
    readable: Vec<(usize, Element, Value)>,
    /// Unreadable elements: stored index, the JSON, and the reason.
    unreadable: Vec<(usize, Value, String)>,
}

fn decode(ink: &InkContent) -> Option<Drawing> {
    if ink.format != INK_FORMAT {
        return None;
    }
    let mut seen = HashSet::new();
    let mut drawing = Drawing {
        readable: Vec::new(),
        unreadable: Vec::new(),
    };
    for (index, raw) in ink.elements.iter().enumerate() {
        match serde_json::from_value::<Element>(raw.clone()) {
            Ok(element) if seen.insert(element.id) => {
                drawing.readable.push((index, element, raw.clone()))
            }
            Ok(element) => drawing.unreadable.push((
                index,
                raw.clone(),
                format!("duplicate element id {}", element.id),
            )),
            Err(e) => drawing.unreadable.push((index, raw.clone(), e.to_string())),
        }
    }
    Some(drawing)
}

/// The parts of a stored drawing this build cannot read, which are kept
/// verbatim for recovery: each unreadable element's stored index and why, or
/// one entry with no index when the whole drawing is in an unknown format.
/// A host that paints its own drawing face reports these to the reader.
pub fn unreadable(ink: &InkContent) -> Vec<(Option<usize>, String)> {
    match decode(ink) {
        None => vec![(None, "unknown ink format".to_string())],
        Some(d) => d
            .unreadable
            .into_iter()
            .map(|(index, _, error)| (Some(index), error))
            .collect(),
    }
}

/// Whether a drawing's readable content can be shown element by element.
fn shows_elements(drawing: &Option<Drawing>) -> bool {
    drawing.as_ref().is_some_and(|d| !d.readable.is_empty())
}

/// The board camera for a device-local Space camera.
pub fn board_camera(camera: &theorem_space::Camera) -> Camera {
    Camera {
        x: camera.origin.x as f32,
        y: camera.origin.y as f32,
        zoom: camera.zoom as f32,
    }
}

/// The device-local Space camera for a board camera. Zoom is clamped to the
/// Space's range, which is the authority for its own view state.
pub fn space_camera(camera: Camera) -> theorem_space::Camera {
    theorem_space::Camera {
        origin: SpacePoint::new(f64::from(camera.x), f64::from(camera.y)),
        zoom: f64::from(camera.zoom)
            .clamp(theorem_space::geom::MIN_ZOOM, theorem_space::geom::MAX_ZOOM),
    }
}

/// The engine's full-precision view for a device-local Space camera.
pub fn view_of(camera: &theorem_space::Camera) -> View {
    View {
        x: camera.origin.x,
        y: camera.origin.y,
        zoom: camera.zoom,
    }
}

/// The device-local Space camera for an engine view, exactly. The engine's
/// range is the Space's, so the clamp only guards a host-built view.
pub fn space_view(view: View) -> theorem_space::Camera {
    theorem_space::Camera {
        origin: SpacePoint::new(view.x, view.y),
        zoom: view
            .zoom
            .clamp(theorem_space::geom::MIN_ZOOM, theorem_space::geom::MAX_ZOOM),
    }
}

/// Every connection the board draws for `space`: its own presentation
/// connections plus the relationships and bindings its owners report among
/// the objects placed here.
pub fn connections(
    space: &Space,
    relationships: &dyn RelationshipAuthority,
    bindings: &dyn BindingAuthority,
) -> Vec<ProjectedConnection> {
    let objects: Vec<_> = space
        .placements
        .iter()
        .filter_map(|p| p.object().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    theorem_space::relation::project_connections(
        space,
        &relationships.among(&objects),
        &bindings.among(&objects),
    )
}

/// A card title when the host supplies none: the label, the note's first line,
/// the object reference, or the nested Space.
pub fn default_title(placement: &Placement) -> String {
    if let Some(label) = placement.label.as_ref().filter(|l| !l.is_empty()) {
        return label.clone();
    }
    match &placement.target {
        PlacementTarget::Object { object } => object.to_string(),
        PlacementTarget::Note { text } => text.lines().next().unwrap_or("").to_string(),
        PlacementTarget::Space { space } => space.to_string(),
        PlacementTarget::Ink { ink } if ink.format != INK_FORMAT => {
            format!("Drawing ({})", ink.format)
        }
        PlacementTarget::Ink { .. } => "Drawing".to_string(),
    }
}

fn side_index(side: Side) -> usize {
    match side {
        Side::Top => 0,
        Side::Right => 1,
        Side::Bottom => 2,
        Side::Left => 3,
    }
}

fn index_side(index: usize) -> Option<Side> {
    match index {
        0 => Some(Side::Top),
        1 => Some(Side::Right),
        2 => Some(Side::Bottom),
        3 => Some(Side::Left),
        _ => None,
    }
}

fn plain(id: u64, kind: ElementKind) -> Element {
    Element {
        id,
        kind,
        stroke: None,
        fill: None,
        label: None,
        label_color: None,
        styles: Vec::new(),
        mindmap: None,
    }
}

/// A rect read back from the board, keeping each untouched component at its
/// full stored precision: a move changes position only, never the size's
/// low bits.
fn read_rect(stored: &Rect, before: (f32, f32, f32, f32), after: (f32, f32, f32, f32)) -> Rect {
    let keep = |old: f64, b: f32, a: f32| {
        if a == b {
            old
        } else {
            old + f64::from(a) - f64::from(b)
        }
    };
    let size = |old: f64, b: f32, a: f32| if a == b { old } else { f64::from(a) };
    Rect::new(
        keep(stored.x, before.0, after.0),
        keep(stored.y, before.1, after.1),
        size(stored.w, before.2, after.2),
        size(stored.h, before.3, after.3),
    )
}

fn rect_close(a: &Rect, b: &Rect) -> bool {
    (a.x - b.x).abs() <= EPS
        && (a.y - b.y).abs() <= EPS
        && (a.w - b.w).abs() <= EPS
        && (a.h - b.h).abs() <= EPS
}

/// Structural equality with a tolerance on numbers.
fn values_close(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= EPS * (1.0 + x.abs()),
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| values_close(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| values_close(v, w)))
        }
        _ => a == b,
    }
}

fn element_value(element: &Element) -> Value {
    serde_json::to_value(element).unwrap_or(Value::Null)
}

/// The padded world bounds of some elements.
fn padded_bounds<'a>(elements: impl IntoIterator<Item = &'a Element>) -> Option<Rect> {
    let mut acc: Option<(f32, f32, f32, f32)> = None;
    for e in elements {
        let b = bbox(&e.kind);
        acc = Some(match acc {
            None => b,
            Some(a) => mechanics::union_rect(a, b),
        });
    }
    acc.map(|(x0, y0, x1, y1)| {
        Rect::new(
            f64::from(x0 - INK_PAD),
            f64::from(y0 - INK_PAD),
            f64::from((x1 - x0) + 2.0 * INK_PAD).max(1.0),
            f64::from((y1 - y0) + 2.0 * INK_PAD).max(1.0),
        )
    })
}

/// Lay readable elements and retained ones into one list: each retained one
/// at its stored index when enough elements precede it, else after the last
/// readable one, keeping the retained ones' relative order. The same rule as
/// [`Scene::to_json_with_retained`].
fn merge_retained(readable: Vec<Value>, retained: &[(usize, Value, String)]) -> Vec<Value> {
    let total = readable.len() + retained.len();
    let mut out = Vec::with_capacity(total);
    let mut valid = readable.into_iter();
    let mut pending = retained.iter().peekable();
    for slot in 0..total {
        let next_is_retained = match (pending.peek(), valid.len()) {
            (Some(_), 0) => true,
            (Some((index, ..)), _) => *index <= slot,
            (None, _) => false,
        };
        if next_is_retained {
            if let Some((_, raw, _)) = pending.next() {
                out.push(raw.clone());
            }
        } else if let Some(v) = valid.next() {
            out.push(v);
        }
    }
    out
}

/// The board side of one presented Space.
#[derive(Clone, Debug, Default)]
pub struct SpaceBoard {
    space: Option<SpaceId>,
    ids: HashMap<Handle, u64>,
    handles: HashMap<u64, Handle>,
    next: u64,
    last: Projection,
}

/// A drawing a gesture creates, before it has a placement identity.
struct NewDrawing {
    rect: Rect,
    elements: Vec<Value>,
}

impl SpaceBoard {
    pub fn new() -> Self {
        Self {
            next: 1,
            ..Self::default()
        }
    }

    /// The Space identity behind a board element.
    pub fn handle(&self, element: u64) -> Option<&Handle> {
        self.handles.get(&element)
    }

    /// The board element drawing a Space identity, if it has been projected.
    pub fn element(&self, handle: &Handle) -> Option<u64> {
        self.ids.get(handle).copied()
    }

    /// The last projection handed out.
    pub fn projection(&self) -> &Projection {
        &self.last
    }

    /// The one hit test for the presented Space: the topmost identity under a
    /// world point (within `pad`), over exactly what the board paints.
    pub fn hit(&self, world: [f32; 2], pad: f32) -> Option<&Handle> {
        mechanics::hit_test(&self.last.scene.elements, world, pad).and_then(|id| self.handle(id))
    }

    /// What opening a card means: focus its object, or enter its nested Space.
    /// Hand it the `page_id` the board's open hook reports.
    pub fn open(&self, page_id: i64) -> Option<SpaceCommand> {
        match self.handle(u64::try_from(page_id).ok()?)? {
            Handle::Placement(placement) => Some(SpaceCommand::Focus {
                placement: placement.clone(),
            }),
            _ => None,
        }
    }

    fn bind(&mut self, handle: Handle, preferred: Option<u64>) -> u64 {
        if let Some(id) = self.ids.get(&handle) {
            return *id;
        }
        let id = match preferred {
            Some(p) if p != 0 && !self.handles.contains_key(&p) => p,
            _ => {
                while self.handles.contains_key(&self.next) {
                    self.next += 1;
                }
                self.next
            }
        };
        self.next = self.next.max(id + 1);
        self.ids.insert(handle.clone(), id);
        self.handles.insert(id, handle);
        id
    }

    /// Project `space` for the board. `connections` is every connection to draw
    /// (see [`connections`]); `title` names a card (the host reads the catalog).
    /// Element ids stay stable across projections, so selection and identity
    /// survive every re-presentation, undo included.
    pub fn project(
        &mut self,
        space: &Space,
        connections: &[ProjectedConnection],
        camera: &theorem_space::Camera,
        title: &dyn Fn(&Placement) -> String,
    ) -> Projection {
        self.project_as(space, connections, camera, title, false)
    }

    /// Project `space` with every placement as one card, drawings included:
    /// the projection for a host that paints its own surfaces and runs only
    /// the gesture engine over them (Theorem's spatial canvas). A drawing
    /// moves and resizes as its frame; unreadable ink is still reported.
    pub fn project_cards(
        &mut self,
        space: &Space,
        connections: &[ProjectedConnection],
        camera: &theorem_space::Camera,
    ) -> Projection {
        self.project_as(space, connections, camera, &|_| String::new(), true)
    }

    fn project_as(
        &mut self,
        space: &Space,
        connections: &[ProjectedConnection],
        camera: &theorem_space::Camera,
        title: &dyn Fn(&Placement) -> String,
        cards_only: bool,
    ) -> Projection {
        if self.space.as_ref() != Some(&space.id) {
            *self = Self::new();
            self.space = Some(space.id.clone());
        }
        let mut out = Projection {
            scene: Scene {
                camera: board_camera(camera),
                elements: Vec::new(),
            },
            ..Projection::default()
        };
        // A group frame carries what lies inside it, frames included, not a
        // stored member list.
        let carried: HashMap<&GroupId, theorem_space::space::Carried<'_>> = space
            .groups
            .iter()
            .map(|g| (&g.id, space.carried_by(&g.id)))
            .collect();
        let pinned_groups: HashSet<&GroupId> = carried
            .iter()
            .filter(|(_, c)| c.placements.iter().any(|p| p.authorship.pinned))
            .map(|(g, _)| *g)
            .collect();
        // Regions sit behind everything they hold.
        for group in &space.groups {
            let id = self.bind(Handle::Group(group.id.clone()), None);
            let mut element = plain(
                id,
                ElementKind::Rect(BoxGeom {
                    x: group.rect.x as f32,
                    y: group.rect.y as f32,
                    w: group.rect.w as f32,
                    h: group.rect.h as f32,
                    width: GROUP_STROKE,
                    rotation: 0.0,
                }),
            );
            element.label = group.label.clone();
            out.scene.elements.push(element);
            out.axis_aligned.push(id);
            // A group with a pinned member cannot move as a whole.
            if pinned_groups.contains(&group.id) {
                out.held.push(id);
            }
        }
        // The element each card is drawn as, so connections can attach to it.
        let mut cards: HashMap<&PlacementId, u64> = HashMap::new();
        for p in &space.placements {
            let drawing = match &p.target {
                PlacementTarget::Ink { ink } => Some(decode(ink)),
                _ => None,
            };
            if let Some(drawing) = &drawing {
                match drawing {
                    None => out.retained.push(RetainedInk {
                        placement: p.id.clone(),
                        index: None,
                        error: "unknown ink format".to_string(),
                    }),
                    Some(d) => out
                        .retained
                        .extend(d.unreadable.iter().map(|(index, _, error)| RetainedInk {
                            placement: p.id.clone(),
                            index: Some(*index),
                            error: error.clone(),
                        })),
                }
            }
            match drawing {
                Some(drawing) if !cards_only && shows_elements(&drawing) => {
                    let Some(drawing) = drawing else { continue };
                    let (ox, oy) = (p.rect.x as f32, p.rect.y as f32);
                    // A declared turn (Free placements only) is shown about
                    // the frame's centre and taken off again on read-back.
                    let turn = space.rotation(&p.id) as f32;
                    let (cx, cy) = (p.rect.center().x as f32, p.rect.center().y as f32);
                    let mut local_to_scene = HashMap::new();
                    for (_, element, _) in &drawing.readable {
                        let id = self.bind(
                            Handle::Ink {
                                placement: p.id.clone(),
                                local: element.id,
                            },
                            Some(element.id),
                        );
                        local_to_scene.insert(element.id, id);
                    }
                    for (_, element, _) in drawing.readable {
                        let mut element = element;
                        element.id = local_to_scene[&element.id];
                        translate(&mut element.kind, ox, oy);
                        if turn != 0.0 {
                            rotate_element(&mut element.kind, cx, cy, turn);
                        }
                        if let ElementKind::Line(s) | ElementKind::Arrow(s) = &mut element.kind {
                            for anchor in [&mut s.start_anchor, &mut s.end_anchor] {
                                *anchor = anchor.and_then(|a| {
                                    local_to_scene.get(&a.element_id).map(|&id| SegmentAnchor {
                                        element_id: id,
                                        connector: a.connector,
                                    })
                                });
                            }
                        }
                        if p.authorship.pinned {
                            out.held.push(element.id);
                        }
                        out.scene.elements.push(element);
                    }
                }
                _ => {
                    let id = self.bind(Handle::Placement(p.id.clone()), None);
                    out.scene.elements.push(plain(
                        id,
                        ElementKind::Embed(EmbedGeom {
                            page_id: id as i64,
                            title: title(p),
                            x: p.rect.x as f32,
                            y: p.rect.y as f32,
                            w: p.rect.w as f32,
                            h: p.rect.h as f32,
                        }),
                    ));
                    out.axis_aligned.push(id);
                    if p.authorship.pinned {
                        out.held.push(id);
                    }
                    cards.insert(&p.id, id);
                }
            }
        }
        // Connections draw over the cards they join.
        for c in connections {
            let (Some(from), Some(to)) = (space.placement(&c.from), space.placement(&c.to)) else {
                continue;
            };
            let class = EdgeClass::from(&c.class);
            let id = self.bind(
                Handle::Edge {
                    class,
                    key: c.key.clone(),
                    from: c.from.clone(),
                    to: c.to.clone(),
                },
                None,
            );
            let from_side = c
                .from_side
                .unwrap_or_else(|| from.rect.facing_side(to.rect.center()));
            let to_side = c
                .to_side
                .unwrap_or_else(|| to.rect.facing_side(from.rect.center()));
            let (a, b) = (from.rect.anchor(from_side), to.rect.anchor(to_side));
            let anchor = |placement: &PlacementId, side: Side| {
                cards.get(placement).map(|&element_id| SegmentAnchor {
                    element_id,
                    connector: side_index(side),
                })
            };
            let seg = SegGeom {
                x1: a.x as f32,
                y1: a.y as f32,
                x2: b.x as f32,
                y2: b.y as f32,
                width: EDGE_WIDTH,
                style: if class == EdgeClass::Semantic {
                    SegmentStyle::Dashed
                } else {
                    SegmentStyle::Solid
                },
                start_anchor: anchor(&c.from, from_side),
                end_anchor: anchor(&c.to, to_side),
            };
            let kind = if c.to_end == End::Arrow || c.from_end == End::Arrow {
                ElementKind::Arrow(seg)
            } else {
                ElementKind::Line(seg)
            };
            let mut element = plain(id, kind);
            element.label = c.label.clone();
            element.stroke = match &c.class {
                ConnectionClass::Executable { state, .. } => Some(match state {
                    BindingState::Current => BINDING_CURRENT,
                    BindingState::Stale => BINDING_STALE,
                    BindingState::Failed { .. } => BINDING_FAILED,
                }),
                _ => None,
            };
            out.scene.elements.push(element);
        }
        // Roles for the gesture engine: a region carries the elements drawn
        // for what its frame carries when the gesture starts, the frames
        // inside it among them.
        let mut members: HashMap<&GroupId, Vec<u64>> = HashMap::new();
        for e in &out.scene.elements {
            let inside = |c: &theorem_space::space::Carried<'_>| match self.handle(e.id) {
                Some(Handle::Group(g)) => c.frames.iter().any(|f| &f.id == g),
                Some(handle) => handle
                    .placement()
                    .is_some_and(|pid| c.placements.iter().any(|p| &p.id == pid)),
                None => false,
            };
            for (group, c) in &carried {
                if inside(c) {
                    members.entry(*group).or_default().push(e.id);
                }
            }
        }
        for e in &out.scene.elements {
            let role = match self.handle(e.id) {
                Some(Handle::Group(g)) => Role::Region {
                    members: members.get(g).cloned().unwrap_or_default(),
                },
                Some(Handle::Placement(pid)) => Role::Card {
                    connectable: space
                        .placement(pid)
                        .is_some_and(|p| !matches!(p.target, PlacementTarget::Ink { .. })),
                },
                Some(Handle::Ink { .. }) => Role::Card { connectable: false },
                Some(Handle::Edge { .. }) | None => Role::Inert,
            };
            out.roles.push((e.id, role));
        }
        out.id_floor = self.next;
        self.last = out.clone();
        out
    }

    /// Route one board event to its owner. A commit becomes the gesture's
    /// commands (see [`gesture`](Self::gesture)); undo and redo become the
    /// Space's own; a camera request becomes device-local view state.
    pub fn route(
        &mut self,
        session: &mut SpaceSession,
        actor: &Actor,
        event: &WhiteboardEvent,
    ) -> Route {
        match event {
            WhiteboardEvent::Commit {
                before,
                after,
                label,
            } => self.gesture(session, actor, before, after, label),
            WhiteboardEvent::UndoRequested => Route::Commands {
                space: Some(SpaceCommand::Undo { only_mine: false }),
                owners: Vec::new(),
                ignored: Vec::new(),
            },
            WhiteboardEvent::RedoRequested => Route::Commands {
                space: Some(SpaceCommand::Redo),
                owners: Vec::new(),
                ignored: Vec::new(),
            },
            WhiteboardEvent::CameraRequested { camera, .. } => Route::Camera(space_camera(*camera)),
        }
    }

    /// Convert one finished gesture into intents for the owners: at most one
    /// Space-history command (so one drag is one undo entry) and the owner
    /// commands for relationships and bindings it removed. Nothing is applied
    /// here. `session` is borrowed mutably only to mint identities for content
    /// the gesture created, exactly as the command executor mints them.
    pub fn gesture(
        &mut self,
        session: &mut SpaceSession,
        actor: &Actor,
        before: &Scene,
        after: &Scene,
        label: &str,
    ) -> Route {
        // Ids the view minted are taken from here on. An element the gesture
        // created is new whatever id it carries: a binding left on that id is
        // stale (the view is told to mint above every bound id, so this only
        // happens to a board presented without `id_floor`).
        let existed: HashSet<u64> = before.elements.iter().map(|e| e.id).collect();
        for e in &after.elements {
            self.next = self.next.max(e.id + 1);
            if !existed.contains(&e.id)
                && let Some(stale) = self.handles.remove(&e.id)
            {
                self.ids.remove(&stale);
            }
        }
        match self.plan(&session.space, before, after, label) {
            Err(refusal) => Route::Refused(refusal),
            Ok(plan) => plan.compile(session, actor, label),
        }
    }

    fn plan(
        &self,
        space: &Space,
        before: &Scene,
        after: &Scene,
        label: &str,
    ) -> Result<Plan, Refusal> {
        let b: HashMap<u64, &Element> = before.elements.iter().map(|e| (e.id, e)).collect();
        let a: HashMap<u64, &Element> = after.elements.iter().map(|e| (e.id, e)).collect();
        let mut plan = Plan::default();
        let rotating = label == "rotate";

        // Cards and groups.
        let mut removed_placements: BTreeSet<PlacementId> = BTreeSet::new();
        let mut group_carry: Vec<(GroupId, f64, f64)> = Vec::new();
        for e in &before.elements {
            let after_e = a.get(&e.id).copied();
            match self.handle(e.id) {
                Some(Handle::Placement(pid)) => {
                    let Some(p) = space.placement(pid) else {
                        continue;
                    };
                    match after_e {
                        None => {
                            removed_placements.insert(pid.clone());
                        }
                        Some(next) if next.kind != e.kind => {
                            if rotating {
                                return Err(Refusal::AxisAligned {
                                    handle: Handle::Placement(pid.clone()),
                                });
                            }
                            let (ElementKind::Embed(bg), ElementKind::Embed(ag)) =
                                (&e.kind, &next.kind)
                            else {
                                plan.ignored.push(format!("{pid} cannot change kind"));
                                continue;
                            };
                            let rect = read_rect(
                                &p.rect,
                                (bg.x, bg.y, bg.w, bg.h),
                                (ag.x, ag.y, ag.w, ag.h),
                            );
                            if !rect_close(&rect, &p.rect) {
                                if p.authorship.pinned {
                                    return Err(Refusal::Pinned {
                                        placement: pid.clone(),
                                    });
                                }
                                plan.rects.insert(pid.clone(), rect);
                            }
                        }
                        Some(next) if next != e => {
                            plan.ignored
                                .push(format!("{pid}: card style is set through the Space"));
                        }
                        Some(_) => {}
                    }
                }
                Some(Handle::Group(gid)) => {
                    let Some(g) = space.group(gid) else { continue };
                    let Some(next) = after_e else {
                        plan.ops_late.push(SpaceOp::RemoveGroup { id: gid.clone() });
                        continue;
                    };
                    let (ElementKind::Rect(bg), ElementKind::Rect(ag)) = (&e.kind, &next.kind)
                    else {
                        plan.ignored.push(format!("{gid} cannot change kind"));
                        continue;
                    };
                    if ag.rotation != 0.0 || (rotating && next.kind != e.kind) {
                        return Err(Refusal::AxisAligned {
                            handle: Handle::Group(gid.clone()),
                        });
                    }
                    let rect =
                        read_rect(&g.rect, (bg.x, bg.y, bg.w, bg.h), (ag.x, ag.y, ag.w, ag.h));
                    if !rect_close(&rect, &g.rect) {
                        if ag.w == bg.w && ag.h == bg.h {
                            group_carry.push((gid.clone(), rect.x - g.rect.x, rect.y - g.rect.y));
                        }
                        plan.group_rects.push((gid.clone(), rect));
                    }
                    if next.label != e.label {
                        plan.ops.push(SpaceOp::SetGroupLabel {
                            id: gid.clone(),
                            label: next.label.clone().filter(|l| !l.is_empty()),
                        });
                    }
                    if (next.stroke, next.fill, next.label_color, &next.styles)
                        != (e.stroke, e.fill, e.label_color, &e.styles)
                    {
                        plan.ignored
                            .push(format!("{gid}: region style is set through the Space"));
                    }
                }
                _ => {}
            }
        }

        // Drawings: which ones the gesture touched.
        let mut touched: BTreeSet<PlacementId> = BTreeSet::new();
        for e in before.elements.iter().chain(&after.elements) {
            if let Some(Handle::Ink { placement, .. }) = self.handle(e.id) {
                let changed = match (b.get(&e.id), a.get(&e.id)) {
                    (Some(x), Some(y)) => x != y,
                    _ => true,
                };
                if changed {
                    touched.insert(placement.clone());
                }
            }
        }

        // New elements: a connection between two cards, or ink.
        let mut joins: BTreeMap<PlacementId, Vec<u64>> = BTreeMap::new();
        let mut fresh: Vec<u64> = Vec::new();
        let mut new_connections = Vec::new();
        let card_of = |id: u64| match self.handle(id) {
            Some(Handle::Placement(pid)) => Some(pid.clone()),
            _ => None,
        };
        for e in &after.elements {
            if self.handle(e.id).is_some() || b.contains_key(&e.id) {
                continue;
            }
            match &e.kind {
                ElementKind::Embed(_) => {
                    plan.ignored
                        .push("objects are placed through the Space".to_string());
                }
                ElementKind::Arrow(s) | ElementKind::Line(s)
                    if s.start_anchor.and_then(|x| card_of(x.element_id)).is_some()
                        && s.end_anchor.and_then(|x| card_of(x.element_id)).is_some() =>
                {
                    let (sa, ea) = (s.start_anchor.unwrap(), s.end_anchor.unwrap());
                    let (from, to) = (
                        card_of(sa.element_id).unwrap(),
                        card_of(ea.element_id).unwrap(),
                    );
                    if from == to {
                        plan.ignored
                            .push("a connection joins two different cards".to_string());
                        continue;
                    }
                    new_connections.push((
                        from,
                        to,
                        index_side(sa.connector),
                        index_side(ea.connector),
                        matches!(e.kind, ElementKind::Arrow(_)),
                        e.label.clone(),
                    ));
                }
                _ => {
                    // Joined through an anchor to a drawing's element?
                    let anchored = match &e.kind {
                        ElementKind::Arrow(s) | ElementKind::Line(s) => {
                            [s.start_anchor, s.end_anchor]
                                .into_iter()
                                .flatten()
                                .find_map(|x| match self.handle(x.element_id) {
                                    Some(Handle::Ink { placement, .. }) => Some(placement.clone()),
                                    _ => None,
                                })
                        }
                        _ => None,
                    };
                    // Else drawn wholly inside a drawing's frame (topmost first).
                    let (x0, y0, x1, y1) = bbox(&e.kind);
                    let inside = || {
                        space.placements.iter().rev().find(|p| {
                            matches!(&p.target, PlacementTarget::Ink { ink } if ink.format == INK_FORMAT)
                                && p.rect.contains_rect(&Rect::new(
                                    f64::from(x0),
                                    f64::from(y0),
                                    f64::from(x1 - x0),
                                    f64::from(y1 - y0),
                                ))
                                && shows_elements(&p.target.ink().and_then(decode))
                        })
                    };
                    match anchored.or_else(|| inside().map(|p| p.id.clone())) {
                        Some(pid) => {
                            touched.insert(pid.clone());
                            joins.entry(pid).or_default().push(e.id);
                        }
                        None => fresh.push(e.id),
                    }
                }
            }
        }
        plan.new_connections = new_connections;

        // Edit each touched drawing.
        let order: HashMap<u64, usize> = after
            .elements
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        for pid in &touched {
            let Some(p) = space.placement(pid) else {
                continue;
            };
            let Some(ink) = p.target.ink() else { continue };
            let Some(drawing) = decode(ink) else { continue };
            let turn = space.rotation(pid);
            self.edit_drawing(
                p,
                ink,
                drawing,
                &b,
                &a,
                joins.get(pid),
                &order,
                turn,
                &mut plan,
            )?;
        }

        // A drawing for everything else the gesture drew.
        if !fresh.is_empty() {
            let elements: Vec<&Element> = fresh.iter().map(|id| a[id]).collect();
            let rect = padded_bounds(elements.iter().copied()).expect("non-empty");
            let locals = self.localize(&elements, &rect, &HashMap::new());
            plan.new_drawing = Some(NewDrawing {
                rect,
                elements: locals,
            });
        }

        // Edges.
        let mut owners_seen = BTreeSet::new();
        for e in &before.elements {
            let Some(Handle::Edge {
                class,
                key,
                from,
                to,
            }) = self.handle(e.id)
            else {
                continue;
            };
            let endpoint_survives = |pid: &PlacementId| {
                !removed_placements.contains(pid) && space.placement(pid).is_some()
            };
            let Some(next) = a.get(&e.id) else {
                if !(endpoint_survives(from) && endpoint_survives(to)) {
                    continue;
                }
                match class {
                    EdgeClass::Presentation => plan.ops.push(SpaceOp::RemoveConnection {
                        id: ConnectionId::new(key.clone()),
                    }),
                    EdgeClass::Semantic => {
                        if owners_seen.insert(key.clone()) {
                            plan.owners.push(SpaceCommand::Unrelate {
                                relationship: key.clone(),
                            });
                        }
                    }
                    EdgeClass::Executable => {
                        if owners_seen.insert(key.clone()) {
                            plan.owners.push(SpaceCommand::Unbind {
                                binding: key.clone(),
                            });
                        }
                    }
                }
                continue;
            };
            let (ElementKind::Arrow(bs) | ElementKind::Line(bs)) = &e.kind else {
                continue;
            };
            let (ElementKind::Arrow(as_) | ElementKind::Line(as_)) = &next.kind else {
                plan.ignored.push(format!("{key} cannot change kind"));
                continue;
            };
            // Each end: where the board now attaches it, if it re-attached.
            let end =
                |was: Option<SegmentAnchor>, now: Option<SegmentAnchor>, old: &PlacementId| {
                    match now {
                        Some(x) if Some(x) != was => {
                            let pid = self
                                .handle(x.element_id)
                                .and_then(Handle::placement)
                                .cloned();
                            pid.map(|pid| (pid, index_side(x.connector)))
                        }
                        Some(x) => Some((old.clone(), index_side(x.connector))),
                        None => None,
                    }
                };
            let new_from = end(bs.start_anchor, as_.start_anchor, from);
            let new_to = end(bs.end_anchor, as_.end_anchor, to);
            let changed_from = new_from
                .as_ref()
                .is_some_and(|_| as_.start_anchor != bs.start_anchor);
            let changed_to = new_to
                .as_ref()
                .is_some_and(|_| as_.end_anchor != bs.end_anchor);
            if (bs.start_anchor.is_some() && as_.start_anchor.is_none())
                || (bs.end_anchor.is_some() && as_.end_anchor.is_none())
            {
                plan.ignored
                    .push(format!("{key}: a connection follows the cards it joins"));
            }
            if !(changed_from || changed_to) {
                continue;
            }
            let from_now = new_from.clone().map_or(from.clone(), |(p, _)| p);
            let to_now = new_to.clone().map_or(to.clone(), |(p, _)| p);
            match class {
                EdgeClass::Presentation => {
                    if from_now == to_now {
                        plan.ignored.push(format!(
                            "{key}: a connection joins two different placements"
                        ));
                        continue;
                    }
                    let id = ConnectionId::new(key.clone());
                    let Some(old) = space.connection(&id) else {
                        continue;
                    };
                    let mut connection = old.clone();
                    connection.from = from_now;
                    connection.to = to_now;
                    if changed_from {
                        connection.from_side = new_from.and_then(|(_, s)| s);
                    }
                    if changed_to {
                        connection.to_side = new_to.and_then(|(_, s)| s);
                    }
                    plan.ops.push(SpaceOp::RemoveConnection { id });
                    plan.ops.push(SpaceOp::AddConnection { connection });
                }
                EdgeClass::Semantic | EdgeClass::Executable => {
                    if &from_now != from || &to_now != to {
                        return Err(Refusal::OwnerEndpoint { key: key.clone() });
                    }
                    let current = space.route_hints.get(key).cloned().unwrap_or(RouteHint {
                        from_side: None,
                        to_side: None,
                    });
                    let hint = RouteHint {
                        from_side: if changed_from {
                            new_from.and_then(|(_, s)| s)
                        } else {
                            current.from_side
                        },
                        to_side: if changed_to {
                            new_to.and_then(|(_, s)| s)
                        } else {
                            current.to_side
                        },
                    };
                    plan.ops.push(SpaceOp::SetRouteHint {
                        key: key.clone(),
                        hint: Some(hint),
                    });
                }
            }
        }

        // A dragged group carries what its frame held that the gesture did
        // not move itself: the frames inside it, then the cards.
        for (gid, dx, dy) in group_carry {
            let carried = space.carried_by(&gid);
            for frame in carried.frames {
                if !plan.group_rects.iter().any(|(id, _)| id == &frame.id) {
                    plan.group_rects
                        .push((frame.id.clone(), frame.rect.translated(dx, dy)));
                }
            }
            for member in carried.placements {
                if removed_placements.contains(&member.id)
                    || plan.rects.contains_key(&member.id)
                    || plan.inks.iter().any(|(id, ..)| id == &member.id)
                {
                    continue;
                }
                if member.authorship.pinned {
                    return Err(Refusal::Pinned {
                        placement: member.id.clone(),
                    });
                }
                plan.rects
                    .insert(member.id.clone(), member.rect.translated(dx, dy));
            }
        }

        // Paint order, when the gesture was a reorder.
        if label == "reorder" {
            let mut target: Vec<PlacementId> = Vec::new();
            for e in &after.elements {
                if let Some(pid) = self.handle(e.id).and_then(Handle::placement)
                    && !target.contains(pid)
                {
                    target.push(pid.clone());
                }
            }
            let mut current: Vec<PlacementId> = space
                .placements
                .iter()
                .map(|p| p.id.clone())
                .filter(|id| target.contains(id))
                .collect();
            let offset: HashMap<&PlacementId, usize> = space
                .placements
                .iter()
                .enumerate()
                .map(|(i, p)| (&p.id, i))
                .collect();
            // Placements outside the target keep their slots; reorder the rest
            // among the slots they already occupy.
            let slots: Vec<usize> = current.iter().map(|id| offset[id]).collect();
            for (i, id) in target.iter().enumerate() {
                if current[i] != *id {
                    let from = current.iter().position(|c| c == id).expect("same set");
                    let moving = current.remove(from);
                    current.insert(i, moving);
                    plan.ops_late.push(SpaceOp::Reorder {
                        id: id.clone(),
                        index: slots[i],
                    });
                }
            }
        }

        for pid in removed_placements {
            plan.ops_late.push(SpaceOp::RemovePlacement { id: pid });
        }
        Ok(plan)
    }

    /// Rewrite one touched drawing: a pure translation of all its content is a
    /// frame move; anything else is a content edit that bumps its revision.
    #[allow(clippy::too_many_arguments)]
    fn edit_drawing(
        &self,
        p: &Placement,
        ink: &InkContent,
        drawing: Drawing,
        b: &HashMap<u64, &Element>,
        a: &HashMap<u64, &Element>,
        joins: Option<&Vec<u64>>,
        order: &HashMap<u64, usize>,
        turn: f64,
        plan: &mut Plan,
    ) -> Result<(), Refusal> {
        let scene_of = |local: u64| {
            self.element(&Handle::Ink {
                placement: p.id.clone(),
                local,
            })
        };
        let before_ids: Vec<u64> = drawing
            .readable
            .iter()
            .filter_map(|(_, e, _)| scene_of(e.id))
            .collect();
        let mut after_ids: Vec<u64> = before_ids
            .iter()
            .copied()
            .filter(|id| a.contains_key(id))
            .chain(joins.into_iter().flatten().copied())
            .collect();
        after_ids.sort_by_key(|id| order.get(id).copied().unwrap_or(usize::MAX));

        // A whole-drawing translation is geometry, not content.
        if joins.is_none_or(Vec::is_empty) && after_ids == before_ids && !before_ids.is_empty() {
            let first_b = bbox(&b[&before_ids[0]].kind);
            let first_a = bbox(&a[&before_ids[0]].kind);
            let (dx, dy) = (first_a.0 - first_b.0, first_a.1 - first_b.1);
            let translated = before_ids.iter().all(|id| {
                let mut moved = b[id].clone();
                translate(&mut moved.kind, dx, dy);
                values_close(&element_value(&moved), &element_value(a[id]))
            });
            if translated && (dx != 0.0 || dy != 0.0) {
                if p.authorship.pinned {
                    return Err(Refusal::Pinned {
                        placement: p.id.clone(),
                    });
                }
                plan.rects.insert(
                    p.id.clone(),
                    p.rect.translated(f64::from(dx), f64::from(dy)),
                );
                return Ok(());
            }
        }

        // A turned drawing is shown turned about its frame's centre; its
        // content is stored unturned, so take the turn off before storing.
        let unturned: Vec<Element> = if turn != 0.0 {
            let c = p.rect.center();
            after_ids
                .iter()
                .map(|id| {
                    let mut e = a[id].clone();
                    rotate_element(&mut e.kind, c.x as f32, c.y as f32, -(turn as f32));
                    e
                })
                .collect()
        } else {
            Vec::new()
        };
        let elements: Vec<&Element> = if turn != 0.0 {
            unturned.iter().collect()
        } else {
            after_ids.iter().map(|id| a[id]).collect()
        };
        if elements.is_empty() && drawing.unreadable.is_empty() {
            plan.ops_late
                .push(SpaceOp::RemovePlacement { id: p.id.clone() });
            return Ok(());
        }
        let content = padded_bounds(elements.iter().copied());
        let rect = match content {
            _ if p.authorship.pinned => {
                // The frame of a pinned drawing never changes: content may be
                // edited only within it.
                if content.is_some_and(|c| {
                    !p.rect
                        .inflate(EPS)
                        .contains_rect(&c.inflate(-f64::from(INK_PAD)))
                }) {
                    return Err(Refusal::Pinned {
                        placement: p.id.clone(),
                    });
                }
                p.rect
            }
            // The frame holds while the content fits, so an edit rewrites only
            // what changed; content drawn past it grows the frame.
            Some(c) => {
                let grown = Rect::union_all([p.rect, c].iter()).expect("two rects");
                // Unread content cannot be re-based: the origin stays.
                if !drawing.unreadable.is_empty()
                    && (grown.x < p.rect.x - EPS || grown.y < p.rect.y - EPS)
                {
                    return Err(Refusal::RetainedContent {
                        placement: p.id.clone(),
                    });
                }
                grown
            }
            None => p.rect,
        };
        // Unchanged elements keep their stored JSON while the origin holds.
        let mut verbatim: HashMap<u64, Value> = HashMap::new();
        if rect_close(
            &Rect::new(rect.x, rect.y, 0.0, 0.0),
            &Rect::new(p.rect.x, p.rect.y, 0.0, 0.0),
        ) {
            for (_, element, raw) in &drawing.readable {
                if let Some(id) = scene_of(element.id)
                    && b.get(&id).is_some()
                    && b.get(&id) == a.get(&id)
                {
                    verbatim.insert(id, raw.clone());
                }
            }
        }
        let readable = self.localize_in(&elements, &rect, &p.id, &verbatim);
        let mut retained = drawing.unreadable;
        retained.sort_by_key(|(index, ..)| *index);
        let stored = merge_retained(readable, &retained);
        if stored == ink.elements && rect_close(&rect, &p.rect) {
            return Ok(());
        }
        if !rect_close(&rect, &p.rect) {
            plan.rects.insert(p.id.clone(), rect);
        }
        plan.inks
            .push((p.id.clone(), ink.revision, ink.format.clone(), stored));
        Ok(())
    }

    /// Elements of a new drawing in local coordinates. Ids stay the board's
    /// (they are unique on the board, so unique in the drawing); anchors to
    /// elements outside the drawing are dropped.
    fn localize(
        &self,
        elements: &[&Element],
        rect: &Rect,
        verbatim: &HashMap<u64, Value>,
    ) -> Vec<Value> {
        let locals: HashMap<u64, u64> = elements.iter().map(|e| (e.id, e.id)).collect();
        localize_with(elements, rect, &locals, verbatim)
    }

    /// Elements of an existing drawing in local coordinates: each keeps the id
    /// it already has in the drawing; a joined element takes its board id
    /// unless the drawing already uses it.
    fn localize_in(
        &self,
        elements: &[&Element],
        rect: &Rect,
        placement: &PlacementId,
        verbatim: &HashMap<u64, Value>,
    ) -> Vec<Value> {
        let mut locals: HashMap<u64, u64> = HashMap::new();
        let mut used: HashSet<u64> = HashSet::new();
        for e in elements {
            if let Some(Handle::Ink {
                placement: owner,
                local,
            }) = self.handle(e.id)
                && owner == placement
            {
                locals.insert(e.id, *local);
                used.insert(*local);
            }
        }
        for e in elements {
            if locals.contains_key(&e.id) {
                continue;
            }
            let mut local = e.id;
            while used.contains(&local) {
                local += 1;
            }
            used.insert(local);
            locals.insert(e.id, local);
        }
        localize_with(elements, rect, &locals, verbatim)
    }
}

fn localize_with(
    elements: &[&Element],
    rect: &Rect,
    locals: &HashMap<u64, u64>,
    verbatim: &HashMap<u64, Value>,
) -> Vec<Value> {
    let (ox, oy) = (rect.x as f32, rect.y as f32);
    elements
        .iter()
        .map(|e| {
            if let Some(raw) = verbatim.get(&e.id) {
                return raw.clone();
            }
            let mut local = (*e).clone();
            local.id = locals[&e.id];
            translate(&mut local.kind, -ox, -oy);
            if let ElementKind::Line(s) | ElementKind::Arrow(s) = &mut local.kind {
                for anchor in [&mut s.start_anchor, &mut s.end_anchor] {
                    *anchor = anchor.and_then(|x| {
                        locals.get(&x.element_id).map(|&element_id| SegmentAnchor {
                            element_id,
                            connector: x.connector,
                        })
                    });
                }
            }
            element_value(&local)
        })
        .collect()
}

/// The changes one gesture asks of the Space, before they become a command.
#[derive(Default)]
struct Plan {
    /// Placement frames (cards, drawings, carried members).
    rects: BTreeMap<PlacementId, Rect>,
    group_rects: Vec<(GroupId, Rect)>,
    /// Drawing content: placement, base revision, format, new elements.
    inks: Vec<(PlacementId, u64, String, Vec<Value>)>,
    new_drawing: Option<NewDrawing>,
    /// Presentation connections: from, to, sides, arrowhead, label.
    #[allow(clippy::type_complexity)]
    new_connections: Vec<(
        PlacementId,
        PlacementId,
        Option<Side>,
        Option<Side>,
        bool,
        Option<String>,
    )>,
    /// Ops that come after geometry (connections, labels, hints).
    ops: Vec<SpaceOp>,
    /// Ops that come last (removals, reorders), so earlier ops still find
    /// what they name.
    ops_late: Vec<SpaceOp>,
    owners: Vec<SpaceCommand>,
    ignored: Vec<String>,
}

/// The Space's label for a board gesture label.
fn history_label(label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => "Edit".to_string(),
    }
}

impl Plan {
    fn compile(self, session: &mut SpaceSession, actor: &Actor, label: &str) -> Route {
        let label = history_label(label);
        let Plan {
            rects,
            group_rects,
            inks,
            new_drawing,
            new_connections,
            ops,
            ops_late,
            owners,
            ignored,
        } = self;
        let geometry_only = inks.is_empty()
            && new_drawing.is_none()
            && new_connections.is_empty()
            && ops.is_empty()
            && ops_late.is_empty();
        let space = if geometry_only && (!rects.is_empty() || !group_rects.is_empty()) {
            // A move, resize or nudge: one Arrange, one undo entry.
            Some(SpaceCommand::Arrange {
                label,
                placements: rects.into_iter().collect(),
                groups: group_rects,
            })
        } else if geometry_only {
            None
        } else if rects.is_empty()
            && group_rects.is_empty()
            && inks.is_empty()
            && new_connections.is_empty()
            && ops.is_empty()
            && ops_late.is_empty()
        {
            new_drawing.map(|d| SpaceCommand::PlaceInk {
                elements: d.elements,
                rect: Some(d.rect),
            })
        } else if rects.is_empty()
            && group_rects.is_empty()
            && inks.len() == 1
            && new_drawing.is_none()
            && new_connections.is_empty()
            && ops.is_empty()
            && ops_late.is_empty()
        {
            let (placement, base_revision, _, elements) = inks.into_iter().next().expect("one");
            Some(SpaceCommand::EditInk {
                placement,
                base_revision,
                elements,
            })
        } else if rects.is_empty()
            && group_rects.is_empty()
            && inks.is_empty()
            && new_drawing.is_none()
            && new_connections.is_empty()
            && ops.is_empty()
            && ops_late
                .iter()
                .all(|op| matches!(op, SpaceOp::RemovePlacement { .. }))
        {
            Some(SpaceCommand::RemovePlacements {
                placements: ops_late
                    .into_iter()
                    .filter_map(|op| match op {
                        SpaceOp::RemovePlacement { id } => Some(id),
                        _ => None,
                    })
                    .collect(),
            })
        } else {
            // A compound gesture: still one transaction, one undo entry, through
            // the Space's own gesture command (the user's, never an agent's).
            let mut all: Vec<SpaceOp> = Vec::new();
            if let Some(d) = new_drawing {
                let id = match session.mint("pl") {
                    Ok(id) => PlacementId::new(id),
                    Err(e) => return refused_by(e),
                };
                all.push(SpaceOp::AddPlacement {
                    placement: Placement {
                        id,
                        target: PlacementTarget::Ink {
                            ink: InkContent::new(d.elements),
                        },
                        rect: d.rect,
                        group: None,
                        presentation: PresentationMode::Auto,
                        label: None,
                        color: None,
                        authorship: Authorship {
                            placed_by: actor.clone(),
                            arranged: !matches!(actor, Actor::System),
                            pinned: false,
                        },
                        state: PlacementState::Valid,
                    },
                });
            }
            for (id, rect) in rects {
                all.push(SpaceOp::SetRect { id, rect });
            }
            for (id, rect) in group_rects {
                all.push(SpaceOp::SetGroupRect { id, rect });
            }
            for (id, revision, format, elements) in inks {
                let Some(revision) = revision.checked_add(1) else {
                    return Route::Refused(Refusal::Unsupported {
                        reasons: vec![format!("{id}: ink revision counter is exhausted")],
                    });
                };
                all.push(SpaceOp::SetInk {
                    id,
                    ink: InkContent {
                        format,
                        revision,
                        elements,
                    },
                });
            }
            for (from, to, from_side, to_side, arrow, connection_label) in new_connections {
                let id = match session.mint("cx") {
                    Ok(id) => ConnectionId::new(id),
                    Err(e) => return refused_by(e),
                };
                all.push(SpaceOp::AddConnection {
                    connection: theorem_space::Connection {
                        id,
                        from,
                        to,
                        from_side,
                        to_side,
                        from_end: End::None,
                        to_end: if arrow { End::Arrow } else { End::None },
                        label: connection_label,
                        color: None,
                    },
                });
            }
            all.extend(ops);
            all.extend(ops_late);
            Some(SpaceCommand::Gesture { label, ops: all })
        };
        if space.is_none() && owners.is_empty() {
            return if ignored.is_empty() {
                Route::Nothing
            } else {
                Route::Refused(Refusal::Unsupported { reasons: ignored })
            };
        }
        Route::Commands {
            space,
            owners,
            ignored,
        }
    }
}

fn refused_by(error: SpaceError) -> Route {
    Route::Refused(Refusal::Unsupported {
        reasons: vec![error.to_string()],
    })
}

/// Apply a route once: the gesture's Space command, then each owner command.
/// A refused Space command stops there, so no owner acts on a gesture the
/// Space rejected. Camera and refusals apply nothing here; the host stores the
/// camera in its navigator and re-presents on a refusal.
pub fn apply(
    session: &mut SpaceSession,
    ctx: &mut CommandContext<'_>,
    route: Route,
) -> Result<Applied, CommandError> {
    let Route::Commands { space, owners, .. } = route else {
        return Ok(Applied {
            space: None,
            owners: Vec::new(),
        });
    };
    let space = space
        .map(|command| theorem_space::execute(session, ctx, command))
        .transpose()?;
    let owners = owners
        .into_iter()
        .map(|command| theorem_space::execute(session, ctx, command))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Applied { space, owners })
}

/// A legacy whiteboard document (its stored JSON) as a drawing to place in a
/// Space, through the recoverable parse. A document that is not a board is an
/// `Err` carrying its original text, so the caller keeps it instead of placing
/// an empty drawing over it. Elements the parse cannot read are carried
/// verbatim; with any of those present the board's own coordinates are kept
/// (local origin at `at`) because unread content cannot be re-based.
pub fn import_board(json: &str, at: SpacePoint) -> Result<SpaceCommand, SceneParseError> {
    let parsed = Scene::parse(json)?;
    let elements: Vec<&Element> = parsed.scene.elements.iter().collect();
    let retained: Vec<(usize, Value, String)> = parsed
        .retained
        .iter()
        .map(|r| (r.index, r.raw.clone(), r.error.clone()))
        .collect();
    let bounds = padded_bounds(elements.iter().copied());
    let (rect, origin) = match bounds {
        Some(b) if retained.is_empty() => (b.translated(at.x, at.y), Rect::new(b.x, b.y, 0.0, 0.0)),
        Some(b) => (
            Rect::new(at.x, at.y, b.right().max(1.0), b.bottom().max(1.0)),
            Rect::new(0.0, 0.0, 0.0, 0.0),
        ),
        None => (
            Rect::new(
                at.x,
                at.y,
                f64::from(INK_PAD) * 2.0,
                f64::from(INK_PAD) * 2.0,
            ),
            Rect::new(0.0, 0.0, 0.0, 0.0),
        ),
    };
    let locals: HashMap<u64, u64> = elements.iter().map(|e| (e.id, e.id)).collect();
    let readable = localize_with(&elements, &origin, &locals, &HashMap::new());
    Ok(SpaceCommand::PlaceInk {
        elements: merge_retained(readable, &retained),
        rect: Some(rect),
    })
}

impl WhiteboardView {
    /// Present one Space projection: the board becomes the Space's mechanical
    /// engine. Undo and the camera are handed to the host, the projected
    /// content replaces the board's, and the projection's constraints are
    /// installed. Never commits, so presenting does not echo back as a gesture.
    pub fn present(&mut self, projection: Projection, cx: &mut Context<Self>) {
        self.set_history_mode(HistoryMode::Hosted, cx);
        self.set_camera_mode(CameraMode::Hosted, cx);
        let camera = projection.scene.camera;
        self.next_id = self.next_id.max(projection.id_floor);
        self.apply_external(projection.scene, cx);
        self.set_camera(camera, cx);
        self.set_constraints(projection.held, projection.axis_aligned, cx);
    }
}
