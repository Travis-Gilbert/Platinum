//! Hosted-mode seams: a host that owns the undo stack, the camera, and the
//! context menu can run the board as a mechanics layer only.
//!
//! Nothing here changes the standalone defaults. A view starts in
//! [`HistoryMode::Local`] / [`CameraMode::Local`] with no context-menu hook, and
//! behaves exactly as upstream does. Each seam is opt-in:
//!
//! - **History.** [`HistoryMode::Hosted`] removes the view's own undo / redo
//!   stacks. A completed gesture is reported as one [`WhiteboardEvent::Commit`]
//!   carrying the scene before and after; the undo / redo keys (and
//!   [`WhiteboardView::undo`] / [`redo`](WhiteboardView::redo)) become
//!   [`UndoRequested`](WhiteboardEvent::UndoRequested) /
//!   [`RedoRequested`](WhiteboardEvent::RedoRequested). The host answers with
//!   [`WhiteboardView::apply_external`], which never commits.
//! - **Camera.** [`CameraMode::Hosted`] stops the view moving its own camera on
//!   wheel / pinch / pan-drag / reset; it reports
//!   [`CameraRequested`](WhiteboardEvent::CameraRequested) and waits for
//!   [`WhiteboardView::set_camera`].
//! - **Context menu.** [`WhiteboardView::set_on_context_menu`] replaces the
//!   built-in right-click menu.
//! - **Constraints.** [`WhiteboardView::set_constraints`] names elements whose
//!   geometry the host holds fixed (no move, resize or rotation by any gesture)
//!   and elements that support axis-aligned geometry only (no rotation). The
//!   host stays the authority and still refuses a violating commit; this keeps
//!   the board from offering the gesture at all.

use std::rc::Rc;

use gpui::{App, Context, EventEmitter, Pixels, Point, Window};

use crate::{Camera, MAX_ZOOM, MIN_ZOOM, ParsedScene, RetainedElement, Scene, WhiteboardView};

/// Who owns undo / redo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryMode {
    /// The view keeps its own bounded snapshot stack (upstream behavior).
    #[default]
    Local,
    /// The host owns history. The view keeps no stack and reports each completed
    /// gesture as [`WhiteboardEvent::Commit`].
    Hosted,
}

/// Who owns the camera (pan / zoom).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraMode {
    /// The view pans and zooms itself (upstream behavior).
    #[default]
    Local,
    /// The host owns the camera. The view reports what the user asked for and
    /// moves only when told via [`WhiteboardView::set_camera`].
    Hosted,
}

/// What the user asked the camera to do, as reported by
/// [`WhiteboardEvent::CameraRequested`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CameraIntent {
    /// The content should follow a drag / wheel by `(dx, dy)` screen px.
    Pan { dx: f32, dy: f32 },
    /// Multiply the zoom by `factor`, keeping the world point under the
    /// canvas-relative screen point `(x, y)` fixed.
    ZoomAbout { x: f32, y: f32, factor: f32 },
    /// Back to the origin at 100%.
    Reset,
}

/// Events a hosted view reports. Delivered both through [`EventEmitter`]
/// (`cx.subscribe`) and through the hook installed with
/// [`WhiteboardView::set_on_commit`].
#[derive(Clone, Debug, PartialEq)]
pub enum WhiteboardEvent {
    /// A gesture or discrete edit completed and changed the elements. Exactly one
    /// per completed gesture; never while a stroke, drag, resize, rotation or
    /// text edit is still in progress. `before` is the scene when the gesture
    /// began; `after` is the scene as it now stands. Equality (and so whether a
    /// no-op is reported) is over the elements only, never the camera.
    Commit {
        before: Scene,
        after: Scene,
        label: String,
    },
    /// The user pressed the undo shortcut (or something called
    /// [`WhiteboardView::undo`]) in [`HistoryMode::Hosted`].
    UndoRequested,
    /// The redo counterpart of [`UndoRequested`](Self::UndoRequested).
    RedoRequested,
    /// The user asked to move the camera in [`CameraMode::Hosted`]. `camera` is
    /// where the view would have put it (zoom already clamped), so a host that
    /// agrees can pass it straight to [`WhiteboardView::set_camera`].
    CameraRequested {
        camera: Camera,
        intent: CameraIntent,
    },
}

/// Hook receiving every [`WhiteboardEvent`] (see [`WhiteboardView::set_on_commit`]).
pub type CommitFn = Rc<dyn Fn(&WhiteboardEvent, &mut App)>;

/// Right-click hook: `(point_in_scene, hit_element_id, window_position, window,
/// app)`. `hit_element_id` is the topmost element under the pointer, if any.
pub type ContextMenuFn = Rc<dyn Fn([f32; 2], Option<u64>, Point<Pixels>, &mut Window, &mut App)>;

impl EventEmitter<WhiteboardEvent> for WhiteboardView {}

impl WhiteboardView {
    // --- history -----------------------------------------------------------

    /// Choose who owns undo / redo. Switching discards whichever side's history
    /// is being given up (the local stacks when going [`Hosted`](HistoryMode::Hosted),
    /// an unsettled gesture snapshot when going [`Local`](HistoryMode::Local)).
    pub fn set_history_mode(&mut self, mode: HistoryMode, cx: &mut Context<Self>) {
        if self.history_mode == mode {
            return;
        }
        self.history_mode = mode;
        self.history.clear();
        self.redo.clear();
        self.hosted_before = None;
        self.hosted_label = None;
        cx.notify();
    }

    /// The current history owner.
    pub fn history_mode(&self) -> HistoryMode {
        self.history_mode
    }

    /// Install the hook that receives every [`WhiteboardEvent`], so a host that
    /// does not subscribe to the entity can still be told. In
    /// [`HistoryMode::Hosted`] this is where each [`Commit`](WhiteboardEvent::Commit)
    /// arrives. It is called with the view mid-update, so it cannot read the
    /// view back; take what it needs from the event.
    pub fn set_on_commit(&mut self, f: CommitFn) {
        self.on_commit = Some(f);
    }

    /// Replace the board's state with `scene`'s elements, as the host's answer to
    /// an undo / redo request or to any change it made itself. Never emits a
    /// [`Commit`](WhiteboardEvent::Commit) and never calls the `on_change`
    /// persistence hook, so the host's own write does not echo back.
    ///
    /// The camera is left alone (use [`set_camera`](Self::set_camera)). Retained
    /// elements ([`retained`](Self::retained)) are kept. In-flight pointer
    /// gestures and any text edit are cancelled, because the ids they point at
    /// may no longer exist; the selection keeps whichever of its ids survive.
    pub fn apply_external(&mut self, scene: Scene, cx: &mut Context<Self>) {
        self.scene.elements = scene.elements;
        self.cancel_gestures();
        self.editing = None;
        self.marked_range = None;
        self.text_selecting = false;
        self.hosted_before = None;
        self.hosted_label = None;
        self.dirty = false;
        let live: std::collections::HashSet<u64> =
            self.scene.elements.iter().map(|e| e.id).collect();
        self.selected.retain(|id| live.contains(id));
        self.text_layout_cache.retain(|id, _| live.contains(id));
        self.label_layout_cache.retain(|id, _| live.contains(id));
        self.reserve_ids(&live);
        cx.notify();
    }

    /// Whether the view is mid-gesture. A [`Commit`](WhiteboardEvent::Commit) is
    /// held back until this is false, so a stroke, move, resize, rotation, picker
    /// drag or text edit yields one commit when it ends and none before.
    pub(crate) fn gesture_in_progress(&self) -> bool {
        self.pending.is_some()
            || self.connecting.is_some()
            || self.drag_from.is_some()
            || self.marquee.is_some()
            || self.resizing.is_some()
            || self.group_resizing.is_some()
            || self.endpoint.is_some()
            || self.rotating.is_some()
            || self.picker_drag.is_some()
            || self.editing.is_some()
    }

    /// Drop every in-flight pointer gesture without applying it.
    fn cancel_gestures(&mut self) {
        self.pending = None;
        self.connecting = None;
        self.hovered_connector = None;
        self.drag_from = None;
        self.moved = false;
        self.marquee = None;
        self.alignment_guides = Default::default();
        self.resizing = None;
        self.group_resizing = None;
        self.endpoint = None;
        self.rotating = None;
        self.picker_drag = None;
    }

    /// Hosted history only: remember the scene as it is now, as the `before` of
    /// the gesture that is starting. The first snapshot of a gesture wins, so a
    /// gesture that pushes several times (a text tool click, then the typing)
    /// still reports one `before` and the label of its first step.
    pub(crate) fn hosted_snapshot(&mut self, label: &'static str) {
        if self.history_mode == HistoryMode::Hosted && self.hosted_before.is_none() {
            self.hosted_before = Some(self.scene.clone());
            self.hosted_label = Some(label);
        }
    }

    /// Hosted history only: report the finished gesture, if there is one and it
    /// changed anything. A no-op (a click that moved nothing, an empty text box
    /// that was removed again) reports nothing. Called from every point a
    /// gesture can end; held back while one is still running.
    pub(crate) fn settle_hosted(&mut self, cx: &mut Context<Self>) {
        if self.history_mode != HistoryMode::Hosted || self.gesture_in_progress() {
            return;
        }
        let Some(before) = self.hosted_before.take() else {
            return;
        };
        let label = self.hosted_label.take().unwrap_or("edit");
        if before.elements == self.scene.elements {
            return;
        }
        let event = WhiteboardEvent::Commit {
            before,
            after: self.scene.clone(),
            label: label.to_string(),
        };
        self.emit_hosted(event, cx);
    }

    /// Deliver an event to the hook and to entity subscribers.
    pub(crate) fn emit_hosted(&mut self, event: WhiteboardEvent, cx: &mut Context<Self>) {
        if let Some(f) = self.on_commit.clone() {
            f(&event, cx);
        }
        cx.emit(event);
    }

    // --- camera ------------------------------------------------------------

    /// Choose who owns the camera. Local by default.
    pub fn set_camera_mode(&mut self, mode: CameraMode, cx: &mut Context<Self>) {
        if self.camera_mode != mode {
            self.camera_mode = mode;
            cx.notify();
        }
    }

    /// The current camera owner.
    pub fn camera_mode(&self) -> CameraMode {
        self.camera_mode
    }

    /// The camera (scroll origin and zoom) the board is drawn with.
    pub fn camera(&self) -> Camera {
        self.scene.camera
    }

    /// Put the camera where the host says. Zoom is clamped to the interactive
    /// range and a non-finite component is refused (the camera is left as it is),
    /// so a bad host value cannot blank the board. Works in either mode. Does not
    /// call `on_change`: a camera the host drives is the host's to persist.
    pub fn set_camera(&mut self, camera: Camera, cx: &mut Context<Self>) {
        if !(camera.x.is_finite() && camera.y.is_finite() && camera.zoom.is_finite()) {
            log::warn!("whiteboard: ignoring non-finite camera {camera:?}");
            return;
        }
        self.scene.camera = Camera {
            zoom: camera.zoom.clamp(MIN_ZOOM, MAX_ZOOM),
            ..camera
        };
        cx.notify();
    }

    /// Route one camera request. Local: apply it and mark the scene dirty (as the
    /// view always has). Hosted: report it and change nothing.
    pub(crate) fn request_camera(&mut self, intent: CameraIntent, cx: &mut Context<Self>) {
        let mut next = self.scene.camera;
        match intent {
            CameraIntent::Pan { dx, dy } => next.pan_by(dx, dy),
            CameraIntent::ZoomAbout { x, y, factor } => next.zoom_about(x, y, factor),
            CameraIntent::Reset => next = Camera::default(),
        }
        if self.camera_mode == CameraMode::Hosted {
            self.emit_hosted(
                WhiteboardEvent::CameraRequested {
                    camera: next,
                    intent,
                },
                cx,
            );
            return;
        }
        self.scene.camera = next;
        self.dirty = true;
        cx.notify();
    }

    // --- context menu ------------------------------------------------------

    /// Take over right-click. With a hook installed the built-in menu never
    /// opens; the hook gets the scene point, the topmost element under it, and
    /// the window position to anchor its own menu at. It fires in read-only
    /// mode too, since a host's menu is its own affair.
    pub fn set_on_context_menu(&mut self, f: ContextMenuFn) {
        self.on_context_menu = Some(f);
    }

    // --- constraints ---------------------------------------------------------

    /// Replace the host's geometry constraints. `held` elements are never moved,
    /// resized or rotated by a gesture: a drag that includes them carries only the
    /// rest of the selection, and no handle is offered while one is selected.
    /// `axis_aligned` elements are never rotated: no rotate grip is offered for
    /// them, alone or in a multi-selection. Ids that do not exist are kept, so a
    /// host can declare constraints before or after it applies a scene.
    pub fn set_constraints(
        &mut self,
        held: impl IntoIterator<Item = u64>,
        axis_aligned: impl IntoIterator<Item = u64>,
        cx: &mut Context<Self>,
    ) {
        self.held = held.into_iter().collect();
        self.axis_aligned = axis_aligned.into_iter().collect();
        cx.notify();
    }

    /// Whether the host holds this element's geometry fixed.
    pub fn is_held(&self, id: u64) -> bool {
        self.held.contains(&id)
    }

    /// Whether the host declared this element axis-aligned only.
    pub fn is_axis_aligned(&self, id: u64) -> bool {
        self.axis_aligned.contains(&id)
    }

    // --- retained elements and ids ------------------------------------------

    /// Build a view over a recovered parse. Elements the parse could not read
    /// ([`ParsedScene::retained`]) are carried and written back by
    /// [`to_json`](Self::to_json) and by the `on_change` payload, so saving the
    /// board never destroys them. Their ids are reserved so a new element cannot
    /// collide with one.
    pub fn from_parsed(
        parsed: ParsedScene,
        style: crate::WhiteboardStyleFn,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::new(parsed.scene, style, cx);
        view.retained = parsed.retained;
        let none = std::collections::HashSet::new();
        view.reserve_ids(&none);
        view
    }

    /// Elements the loader could not read and is holding verbatim. Non-empty
    /// means part of the saved board is not shown; surface it to the user.
    pub fn retained(&self) -> &[RetainedElement] {
        &self.retained
    }

    /// The whole board as JSON, retained elements written back in their original
    /// positions. This is what `on_change` is handed.
    pub fn to_json(&self) -> String {
        self.scene.to_json_with_retained(&self.retained)
    }

    /// Keep `next_id` above every live element id and every retained id.
    pub(crate) fn reserve_ids(&mut self, live: &std::collections::HashSet<u64>) {
        let live_max = live.iter().copied().max();
        let scene_max = self.scene.elements.iter().map(|e| e.id).max();
        let retained_max = self.retained.iter().filter_map(RetainedElement::id).max();
        let floor = [live_max, scene_max, retained_max]
            .into_iter()
            .flatten()
            .max()
            .map_or(0, |m| m + 1);
        self.next_id = self.next_id.max(floor);
    }

    /// The ids of the selected elements.
    pub fn selection(&self) -> &[u64] {
        &self.selected
    }

    /// Select exactly `ids` (those that exist). The host calls this from its
    /// context-menu hook to select what was right-clicked.
    pub fn select(&mut self, ids: &[u64], cx: &mut Context<Self>) {
        self.selected = ids
            .iter()
            .copied()
            .filter(|id| self.scene.elements.iter().any(|e| e.id == *id))
            .collect();
        cx.notify();
    }
}
