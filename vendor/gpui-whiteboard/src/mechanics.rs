//! Board mechanics as pure functions over [`Element`] slices: hit-testing,
//! marquee selection, z-order, selection / scene bounds, and thumbnail fitting.
//!
//! Nothing here needs a GPUI entity, a window, or a camera held by a view, so a
//! host that owns its own gesture / camera / hit-test loop (Theorem's spatial
//! canvas) can call the same mechanics the standalone [`WhiteboardView`] does,
//! and the view calls these same functions rather than carrying a second copy.
//! Bounds come from the one `geometry::bbox`, so a rotated shape is measured the
//! same way here, in the view's selection box, and in the thumbnail.
//!
//! Conventions: world space, `y` down; a rect is `(min_x, min_y, max_x, max_y)`
//! ([`WorldRect`]); `elements` is paint order, so **later is on top**.
//!
//! [`WhiteboardView`]: crate::WhiteboardView

use crate::geometry::{bbox, hit_test as within_bounds};
use crate::{Camera, Element, ElementKind, Scene};

/// `(min_x, min_y, max_x, max_y)` in world space.
pub type WorldRect = (f32, f32, f32, f32);

// --- hit-testing -------------------------------------------------------------

/// The topmost element under `point` (world space), within `pad` of its bounds.
///
/// "Topmost" is paint order: the **last** matching element wins, so an element
/// drawn later covers one drawn earlier. `pad` is world units (a screen-px
/// tolerance divided by the zoom).
///
/// The hit region is an element's padded axis-aligned bounds. For a rotated
/// box or text that is the bounds of the *rotated* geometry, so it never misses
/// the shape but can claim the empty corners of its own bounding box.
pub fn hit_test(elements: &[Element], point: [f32; 2], pad: f32) -> Option<u64> {
    hit_test_where(elements, point, pad, |_| true)
}

/// [`hit_test`] restricted to elements `keep` accepts (a kind filter, e.g. text
/// only). A rejected element is skipped, not treated as a blocker: it does not
/// hide what is beneath it.
pub fn hit_test_where(
    elements: &[Element],
    point: [f32; 2],
    pad: f32,
    keep: impl Fn(&Element) -> bool,
) -> Option<u64> {
    elements
        .iter()
        .rev()
        .find(|e| keep(e) && within_bounds(&e.kind, point[0], point[1], pad))
        .map(|e| e.id)
}

/// Every element under `point`, topmost first (for cycling through a stack).
pub fn hit_test_all(elements: &[Element], point: [f32; 2], pad: f32) -> Vec<u64> {
    elements
        .iter()
        .rev()
        .filter(|e| within_bounds(&e.kind, point[0], point[1], pad))
        .map(|e| e.id)
        .collect()
}

// --- marquee -----------------------------------------------------------------

/// Which elements a marquee box selects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarqueeMode {
    /// Any element whose bounds touch the box. The standalone view's behavior.
    #[default]
    Intersecting,
    /// Only elements whose bounds lie entirely inside the box.
    Contained,
}

/// The ids selected by a marquee dragged between world points `a` and `b`
/// (either corner order), in paint order (bottom to top). Edges are inclusive.
pub fn marquee_select(
    elements: &[Element],
    a: [f32; 2],
    b: [f32; 2],
    mode: MarqueeMode,
) -> Vec<u64> {
    let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
    let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
    elements
        .iter()
        .filter(|e| {
            let bb = bbox(&e.kind);
            match mode {
                MarqueeMode::Intersecting => bb.0 <= x1 && bb.2 >= x0 && bb.1 <= y1 && bb.3 >= y0,
                MarqueeMode::Contained => bb.0 >= x0 && bb.2 <= x1 && bb.1 >= y0 && bb.3 <= y1,
            }
        })
        .map(|e| e.id)
        .collect()
}

// --- z-order -----------------------------------------------------------------

/// How a selection moves through the paint order (later = on top).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZOrder {
    /// To the very top.
    ToFront,
    /// One step toward the top, past the next unselected element.
    Forward,
    /// One step toward the bottom.
    Backward,
    /// To the very bottom.
    ToBack,
}

/// Reorder `items` in place so the ones `is_selected` accepts move per `op`.
/// Returns whether anything moved (an element already at the edge is a no-op).
///
/// A multi-selection moves as a block: unselected items keep their relative
/// order and so do selected ones, and selected items never leapfrog each other.
pub fn reorder_by<T>(items: &mut [T], is_selected: impl Fn(&T) -> bool, op: ZOrder) -> bool {
    let on = |t: &T| is_selected(t);
    let len = items.len();
    match op {
        ZOrder::ToFront => {
            // Moves iff some selected item has an unselected one after it.
            let first = items.iter().position(on);
            let moves = first.is_some_and(|f| items[f..].iter().any(|t| !on(t)));
            // Stable: `false` (unselected) sorts before `true` (selected).
            items.sort_by_key(|t| on(t));
            moves
        }
        ZOrder::ToBack => {
            let first = items.iter().position(|t| !on(t));
            let moves = first.is_some_and(|f| items[f..].iter().any(on));
            items.sort_by_key(|t| !on(t));
            moves
        }
        // One step: swap each selected item past its adjacent unselected
        // neighbor, walking away from the destination edge so nothing moves
        // twice.
        ZOrder::Forward => {
            let mut moved = false;
            for i in (0..len.saturating_sub(1)).rev() {
                if on(&items[i]) && !on(&items[i + 1]) {
                    items.swap(i, i + 1);
                    moved = true;
                }
            }
            moved
        }
        ZOrder::Backward => {
            let mut moved = false;
            for i in 1..len {
                if on(&items[i]) && !on(&items[i - 1]) {
                    items.swap(i, i - 1);
                    moved = true;
                }
            }
            moved
        }
    }
}

/// The paint order (`ids`, bottom to top) after moving `selected` per `op`.
/// Pure: `ids` is left alone and the new order is returned.
pub fn reorder(ids: &[u64], selected: &[u64], op: ZOrder) -> Vec<u64> {
    let mut out = ids.to_vec();
    reorder_by(&mut out, |id| selected.contains(id), op);
    out
}

/// [`reorder_by`] over a board's elements, selecting by id.
pub fn reorder_elements(elements: &mut [Element], selected: &[u64], op: ZOrder) -> bool {
    reorder_by(elements, |e| selected.contains(&e.id), op)
}

// --- bounds ------------------------------------------------------------------

/// The union of two rects.
pub fn union_rect(a: WorldRect, b: WorldRect) -> WorldRect {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}

/// The bounds of one element, or `None` if `id` is not in `elements`.
pub fn element_bounds(elements: &[Element], id: u64) -> Option<WorldRect> {
    elements.iter().find(|e| e.id == id).map(|e| bbox(&e.kind))
}

/// The bounds enclosing every element in `selected`, or `None` if none of them
/// exist (an empty selection has no box).
pub fn selection_bounds(elements: &[Element], selected: &[u64]) -> Option<WorldRect> {
    elements
        .iter()
        .filter(|e| selected.contains(&e.id))
        .map(|e| bbox(&e.kind))
        .reduce(union_rect)
}

/// The bounds enclosing every element, or `None` for an empty board.
pub fn scene_bounds(scene: &Scene) -> Option<WorldRect> {
    scene
        .elements
        .iter()
        .map(|e| bbox(&e.kind))
        .reduce(union_rect)
}

// --- thumbnails --------------------------------------------------------------

/// A world-to-thumbnail map: `thumb = world * scale + offset`, both axes scaled
/// alike (aspect preserved).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThumbnailTransform {
    /// Thumbnail pixels per world unit.
    pub scale: f32,
    /// Thumbnail-space position of the world origin.
    pub offset: [f32; 2],
}

impl ThumbnailTransform {
    /// A world point in thumbnail pixels.
    pub fn apply(&self, p: [f32; 2]) -> [f32; 2] {
        [
            p[0] * self.scale + self.offset[0],
            p[1] * self.scale + self.offset[1],
        ]
    }

    /// A world rect in thumbnail pixels.
    pub fn apply_rect(&self, r: WorldRect) -> WorldRect {
        let a = self.apply([r.0, r.1]);
        let b = self.apply([r.2, r.3]);
        (a[0], a[1], b[0], b[1])
    }

    /// The thumbnail pixel back to world space.
    pub fn invert(&self, p: [f32; 2]) -> [f32; 2] {
        [
            (p[0] - self.offset[0]) / self.scale,
            (p[1] - self.offset[1]) / self.scale,
        ]
    }

    /// The same mapping as a view [`Camera`]: rendering the scene through it
    /// into a canvas of the thumbnail's size lands every element where
    /// [`apply`](Self::apply) says. (Unlike a view's own camera it is not
    /// clamped to the interactive zoom range.)
    pub fn camera(&self) -> Camera {
        Camera {
            x: -self.offset[0] / self.scale,
            y: -self.offset[1] / self.scale,
            zoom: self.scale,
        }
    }
}

/// Fit world-space `bounds` into a `width` x `height` thumbnail, centered, with
/// `padding_px` of margin on every side.
///
/// The scale is the largest that keeps the whole of `bounds` (and the margin)
/// inside the target: it is never clamped, so a very large scene still fits.
/// A degenerate extent (a lone point or a perfectly flat line) is floored to one
/// world unit so the scale stays finite. `None` for a target with no usable area
/// (non-positive, non-finite, or smaller than twice the padding) or non-finite
/// bounds.
pub fn fit_thumbnail(
    bounds: WorldRect,
    width: f32,
    height: f32,
    padding_px: f32,
) -> Option<ThumbnailTransform> {
    let pad = padding_px.max(0.0);
    let (avail_w, avail_h) = (width - 2.0 * pad, height - 2.0 * pad);
    let finite = [bounds.0, bounds.1, bounds.2, bounds.3, width, height, pad]
        .iter()
        .all(|v| v.is_finite());
    if !finite || avail_w <= 0.0 || avail_h <= 0.0 {
        return None;
    }
    let bw = (bounds.2 - bounds.0).abs().max(1.0);
    let bh = (bounds.3 - bounds.1).abs().max(1.0);
    let scale = (avail_w / bw).min(avail_h / bh);
    let (cx, cy) = ((bounds.0 + bounds.2) * 0.5, (bounds.1 + bounds.3) * 0.5);
    Some(ThumbnailTransform {
        scale,
        offset: [width * 0.5 - cx * scale, height * 0.5 - cy * scale],
    })
}

/// [`fit_thumbnail`] for a whole board (`None` if it is empty).
pub fn fit_scene_thumbnail(
    scene: &Scene,
    width: f32,
    height: f32,
    padding_px: f32,
) -> Option<ThumbnailTransform> {
    fit_thumbnail(scene_bounds(scene)?, width, height, padding_px)
}

// --- rotation capability -----------------------------------------------------

/// How an element kind responds to rotation, as paint and hit-testing actually
/// implement it (not as the data model might suggest: every box carries a
/// `rotation` field, but only some kinds draw it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotationSupport {
    /// Any angle. The stored `rotation` is drawn as given, and the hit region is
    /// the bounds of the rotated geometry. Box shapes and text.
    Free,
    /// No stored angle: rotating rewrites the points, so what is drawn and what
    /// is hit are the rotated geometry. Lines, arrows, freehand strokes.
    Baked,
    /// Quarter turns only. The stored angle is snapped to a multiple of 90 degrees
    /// for both drawing and hit-testing, so any other angle has no effect. Images.
    QuarterTurns,
    /// Not rotatable. Page cards are overlays GPUI cannot transform.
    None,
}

/// What `kind` supports. The match has no wildcard, so a new element kind cannot
/// be added without deciding this.
pub fn rotation_support(kind: &ElementKind) -> RotationSupport {
    match kind {
        ElementKind::Rect(_)
        | ElementKind::Ellipse(_)
        | ElementKind::Diamond(_)
        | ElementKind::Triangle(_)
        | ElementKind::RoundRect(_)
        | ElementKind::Star(_)
        | ElementKind::Hexagon(_)
        | ElementKind::Text(_) => RotationSupport::Free,
        ElementKind::Line(_) | ElementKind::Arrow(_) | ElementKind::Draw(_) => {
            RotationSupport::Baked
        }
        ElementKind::Image(_) => RotationSupport::QuarterTurns,
        ElementKind::Embed(_) => RotationSupport::None,
    }
}

/// Whether an arbitrary rotation of `kind` is both drawn and hit-tested. True
/// for [`RotationSupport::Free`] and [`RotationSupport::Baked`]; false for
/// images (quarter turns only) and page cards (none). A host should offer a
/// free rotate handle only where this is true.
pub fn supports_rotation(kind: &ElementKind) -> bool {
    matches!(
        rotation_support(kind),
        RotationSupport::Free | RotationSupport::Baked
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoxGeom, ElementKind, SegGeom, SegmentStyle};

    fn rect(id: u64, x: f32, y: f32, w: f32, h: f32) -> Element {
        element(
            id,
            ElementKind::Rect(BoxGeom {
                x,
                y,
                w,
                h,
                width: 1.0,
                rotation: 0.0,
            }),
        )
    }

    fn element(id: u64, kind: ElementKind) -> Element {
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

    fn order(els: &[Element]) -> Vec<u64> {
        els.iter().map(|e| e.id).collect()
    }

    #[test]
    fn hit_test_returns_the_topmost_of_overlapping_elements() {
        // Wrong implementation rejected: iterating bottom-up and returning the
        // first match (the lowest element) instead of the last.
        let els = [
            rect(1, 0.0, 0.0, 100.0, 100.0),
            rect(2, 50.0, 50.0, 100.0, 100.0),
        ];
        assert_eq!(hit_test(&els, [75.0, 75.0], 0.0), Some(2));
        assert_eq!(hit_test(&els, [10.0, 10.0], 0.0), Some(1));
        assert_eq!(hit_test(&els, [500.0, 500.0], 0.0), None);
        assert_eq!(hit_test_all(&els, [75.0, 75.0], 0.0), vec![2, 1]);
    }

    #[test]
    fn hit_test_follows_paint_order_not_id_order() {
        // Wrong implementation rejected: choosing the highest id as "top".
        let els = [
            rect(9, 0.0, 0.0, 100.0, 100.0),
            rect(3, 0.0, 0.0, 100.0, 100.0),
        ];
        assert_eq!(hit_test(&els, [5.0, 5.0], 0.0), Some(3));
    }

    #[test]
    fn hit_test_pad_widens_the_target_and_filter_skips_without_blocking() {
        let els = [rect(1, 0.0, 0.0, 10.0, 10.0), rect(2, 0.0, 0.0, 10.0, 10.0)];
        assert_eq!(hit_test(&els, [12.0, 5.0], 0.0), None);
        assert_eq!(hit_test(&els, [12.0, 5.0], 3.0), Some(2));
        // The filter rejects the top element; the one beneath is still found.
        assert_eq!(
            hit_test_where(&els, [5.0, 5.0], 0.0, |e| e.id == 1),
            Some(1)
        );
    }

    #[test]
    fn marquee_contained_excludes_what_intersecting_includes() {
        // Wrong implementation rejected: a single mode that treats "touches the
        // box" as "inside it" (or the reverse).
        let els = [
            rect(1, 10.0, 10.0, 20.0, 20.0),   // fully inside
            rect(2, 90.0, 90.0, 40.0, 40.0),   // straddles the box edge
            rect(3, 300.0, 300.0, 10.0, 10.0), // far outside
        ];
        let a = [0.0, 0.0];
        let b = [100.0, 100.0];
        assert_eq!(
            marquee_select(&els, a, b, MarqueeMode::Intersecting),
            vec![1, 2]
        );
        assert_eq!(marquee_select(&els, a, b, MarqueeMode::Contained), vec![1]);
        // Corner order does not matter.
        assert_eq!(marquee_select(&els, b, a, MarqueeMode::Contained), vec![1]);
    }

    #[test]
    fn marquee_edges_are_inclusive_and_results_are_in_paint_order() {
        let els = [rect(7, 0.0, 0.0, 10.0, 10.0), rect(4, 0.0, 0.0, 10.0, 10.0)];
        // Box edge exactly on the rects' bounds: Contained still takes them.
        assert_eq!(
            marquee_select(&els, [0.0, 0.0], [10.0, 10.0], MarqueeMode::Contained),
            vec![7, 4]
        );
    }

    #[test]
    fn z_order_operations_return_the_new_order() {
        // Wrong implementation rejected: ToFront that only swaps with the top,
        // or a block move that reverses the selected elements' relative order.
        let ids = [1, 2, 3, 4, 5];
        assert_eq!(reorder(&ids, &[2], ZOrder::Forward), vec![1, 3, 2, 4, 5]);
        assert_eq!(reorder(&ids, &[4], ZOrder::Backward), vec![1, 2, 4, 3, 5]);
        assert_eq!(reorder(&ids, &[2], ZOrder::ToFront), vec![1, 3, 4, 5, 2]);
        assert_eq!(reorder(&ids, &[4], ZOrder::ToBack), vec![4, 1, 2, 3, 5]);
        // A multi-selection moves as a block, keeping its own order.
        assert_eq!(reorder(&ids, &[2, 4], ZOrder::ToFront), vec![1, 3, 5, 2, 4]);
        assert_eq!(reorder(&ids, &[2, 4], ZOrder::ToBack), vec![2, 4, 1, 3, 5]);
        assert_eq!(reorder(&ids, &[2, 4], ZOrder::Forward), vec![1, 3, 2, 5, 4]);
        assert_eq!(
            reorder(&ids, &[2, 4], ZOrder::Backward),
            vec![2, 1, 4, 3, 5]
        );
        // The input is untouched.
        assert_eq!(ids, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn z_order_at_an_edge_is_a_reported_no_op() {
        // Wrong implementation rejected: always reporting `changed`, which would
        // push a pointless history entry for "bring to front" on the top element.
        let mut els = [rect(1, 0.0, 0.0, 1.0, 1.0), rect(2, 0.0, 0.0, 1.0, 1.0)];
        assert!(!reorder_elements(&mut els, &[2], ZOrder::ToFront));
        assert!(!reorder_elements(&mut els, &[2], ZOrder::Forward));
        assert!(!reorder_elements(&mut els, &[1], ZOrder::ToBack));
        assert!(!reorder_elements(&mut els, &[1], ZOrder::Backward));
        assert!(!reorder_elements(&mut els, &[], ZOrder::ToFront));
        assert!(!reorder_elements(&mut els, &[1, 2], ZOrder::ToFront));
        assert_eq!(order(&els), vec![1, 2]);
        assert!(reorder_elements(&mut els, &[1], ZOrder::ToFront));
        assert_eq!(order(&els), vec![2, 1]);
    }

    #[test]
    fn selection_bounds_unions_rotated_bounds_and_ignores_missing_ids() {
        let line = element(
            3,
            ElementKind::Line(SegGeom {
                x1: -10.0,
                y1: 5.0,
                x2: 0.0,
                y2: 200.0,
                width: 1.0,
                style: SegmentStyle::Solid,
                start_anchor: None,
                end_anchor: None,
            }),
        );
        let els = [
            rect(1, 0.0, 0.0, 10.0, 10.0),
            rect(2, 50.0, 60.0, 10.0, 10.0),
            line,
        ];
        assert_eq!(
            selection_bounds(&els, &[1, 2]),
            Some((0.0, 0.0, 60.0, 70.0))
        );
        assert_eq!(
            selection_bounds(&els, &[2, 3]),
            Some((-10.0, 5.0, 60.0, 200.0))
        );
        assert_eq!(selection_bounds(&els, &[99]), None);
        assert_eq!(selection_bounds(&els, &[]), None);
        assert_eq!(element_bounds(&els, 2), Some((50.0, 60.0, 60.0, 70.0)));
        let scene = Scene {
            elements: els.to_vec(),
            ..Default::default()
        };
        assert_eq!(scene_bounds(&scene), Some((-10.0, 0.0, 60.0, 200.0)));
        assert_eq!(scene_bounds(&Scene::default()), None);
    }

    #[test]
    fn thumbnail_transform_fits_centers_and_preserves_aspect() {
        // Wrong implementation rejected: scaling each axis independently
        // (distortion), fitting to the wrong dimension (overflow), or ignoring
        // the padding.
        let bounds = (100.0, 200.0, 500.0, 300.0); // 400 x 100
        let t = fit_thumbnail(bounds, 200.0, 100.0, 10.0).unwrap();
        // Width-limited: (200 - 20) / 400.
        assert!((t.scale - 0.45).abs() < 1e-6, "scale {}", t.scale);
        let r = t.apply_rect(bounds);
        assert!(
            (r.0 - 10.0).abs() < 1e-4 && (r.2 - 190.0).abs() < 1e-4,
            "{r:?}"
        );
        // Centered vertically: (100 - 100 * 0.45) / 2 = 27.5.
        assert!(
            (r.1 - 27.5).abs() < 1e-4 && (r.3 - 72.5).abs() < 1e-4,
            "{r:?}"
        );
        assert!(r.0 >= 10.0 - 1e-4 && r.2 <= 190.0 + 1e-4);
        assert!(r.1 >= 10.0 - 1e-4 && r.3 <= 90.0 + 1e-4);
        // A tall scene is height-limited instead.
        let tall = fit_thumbnail((0.0, 0.0, 10.0, 1000.0), 200.0, 100.0, 0.0).unwrap();
        assert!((tall.scale - 0.1).abs() < 1e-6);
        let r = tall.apply_rect((0.0, 0.0, 10.0, 1000.0));
        assert!(r.3 <= 100.0 + 1e-4 && r.0 >= -1e-4 && r.2 <= 200.0);
        // Round trip, and the equivalent camera agrees with `apply`.
        let p = [321.0, 247.0];
        let back = t.invert(t.apply(p));
        assert!((back[0] - p[0]).abs() < 1e-3 && (back[1] - p[1]).abs() < 1e-3);
        let cam = t.camera();
        let via_cam = [(p[0] - cam.x) * cam.zoom, (p[1] - cam.y) * cam.zoom];
        let direct = t.apply(p);
        assert!((via_cam[0] - direct[0]).abs() < 1e-3 && (via_cam[1] - direct[1]).abs() < 1e-3);
    }

    #[test]
    fn thumbnail_fit_is_not_clamped_to_the_interactive_zoom_range() {
        // A 100000-wide scene needs a scale far below the view's MIN_ZOOM (0.02).
        // Wrong implementation rejected: reusing the clamped camera spec, whose
        // result would overflow the thumbnail.
        let t = fit_thumbnail((0.0, 0.0, 100_000.0, 100.0), 100.0, 100.0, 0.0).unwrap();
        assert!(t.scale < 0.01);
        assert!(t.apply([100_000.0, 0.0])[0] <= 100.0 + 1e-3);
    }

    #[test]
    fn thumbnail_fit_refuses_unusable_targets_and_survives_degenerate_bounds() {
        let b = (0.0, 0.0, 10.0, 10.0);
        assert!(fit_thumbnail(b, 0.0, 100.0, 0.0).is_none());
        assert!(fit_thumbnail(b, 100.0, -1.0, 0.0).is_none());
        assert!(fit_thumbnail(b, 20.0, 20.0, 10.0).is_none()); // padding eats it all
        assert!(fit_thumbnail((f32::NAN, 0.0, 1.0, 1.0), 10.0, 10.0, 0.0).is_none());
        let point = fit_thumbnail((5.0, 5.0, 5.0, 5.0), 64.0, 64.0, 0.0).unwrap();
        assert!(point.scale.is_finite() && point.scale > 0.0);
        let at = point.apply([5.0, 5.0]);
        assert!((at[0] - 32.0).abs() < 1e-3 && (at[1] - 32.0).abs() < 1e-3);
        assert!(fit_scene_thumbnail(&Scene::default(), 10.0, 10.0, 0.0).is_none());
    }

    // --- rotation capability ---------------------------------------------------

    fn one_of_each_kind() -> Vec<ElementKind> {
        use crate::{EmbedGeom, ImageGeom, Stroke, TextGeom};
        let b = BoxGeom {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 20.0,
            width: 1.0,
            rotation: 0.0,
        };
        let seg = SegGeom {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 0.0,
            width: 1.0,
            style: SegmentStyle::Solid,
            start_anchor: None,
            end_anchor: None,
        };
        vec![
            ElementKind::Rect(b.clone()),
            ElementKind::Ellipse(b.clone()),
            ElementKind::Diamond(b.clone()),
            ElementKind::Triangle(b.clone()),
            ElementKind::RoundRect(b.clone()),
            ElementKind::Star(b.clone()),
            ElementKind::Hexagon(b.clone()),
            ElementKind::Line(seg.clone()),
            ElementKind::Arrow(seg),
            ElementKind::Draw(Stroke {
                points: vec![[0.0, 0.0], [100.0, 0.0]],
                width: 1.0,
            }),
            ElementKind::Text(TextGeom {
                x: 0.0,
                y: 0.0,
                content: "wide wide wide".into(),
                size: 18.0,
                rotation: 0.0,
                measured_w: 0.0,
                measured_h: 0.0,
            }),
            ElementKind::Image(ImageGeom {
                src: "x".into(),
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
                rotation: 0.0,
            }),
            ElementKind::Embed(EmbedGeom {
                page_id: 1,
                title: "t".into(),
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            }),
        ]
    }

    #[test]
    fn rotation_capability_matches_what_rotating_actually_does() {
        // The claim is checked against behavior, not restated: rotate each kind
        // 30 degrees about its own center with the same routine the rotate
        // handle uses, and see whether the bounds that paint and hit-testing
        // read followed. A wrong implementation that returned `true` for every
        // kind (the data model has a `rotation` field on images) or `false` for
        // baked lines fails here.
        let angle = 30.0_f32.to_radians();
        for kind in one_of_each_kind() {
            let before = crate::geometry::bbox(&kind);
            let (cx, cy) = ((before.0 + before.2) / 2.0, (before.1 + before.3) / 2.0);
            let mut turned = kind.clone();
            crate::geometry::rotate_element(&mut turned, cx, cy, angle);
            let after = crate::geometry::bbox(&turned);
            let moved = (0..4).any(|i| {
                let (a, b) = (
                    [before.0, before.1, before.2, before.3][i],
                    [after.0, after.1, after.2, after.3][i],
                );
                (a - b).abs() > 0.01
            });
            assert_eq!(
                supports_rotation(&kind),
                moved,
                "{kind:?}: capability says {:?} but a 30 degree turn {} the bounds",
                rotation_support(&kind),
                if moved { "changed" } else { "left" }
            );
        }
    }

    #[test]
    fn rotation_support_names_the_four_behaviors() {
        use RotationSupport::*;
        let got: Vec<_> = one_of_each_kind().iter().map(rotation_support).collect();
        assert_eq!(
            got,
            [
                Free,
                Free,
                Free,
                Free,
                Free,
                Free,
                Free, // box shapes
                Baked,
                Baked,
                Baked,        // line, arrow, stroke
                Free,         // text
                QuarterTurns, // image
                None,         // page card
            ]
        );
    }

    #[test]
    fn a_rotated_free_shape_is_hit_by_its_rotated_bounds() {
        // Hit-testing follows the rotated geometry, not the stale unrotated box:
        // a 100x20 bar turned a quarter reaches 40 units above and below its
        // center, and no longer reaches 40 units either side of it. A hit-test
        // that ignored rotation gets both wrong.
        let mut bar = rect(1, 0.0, 0.0, 100.0, 20.0);
        if let ElementKind::Rect(b) = &mut bar.kind {
            b.rotation = std::f32::consts::FRAC_PI_2;
        }
        let els = [bar];
        assert_eq!(hit_test(&els, [50.0, 45.0], 0.0), Some(1)); // above the unrotated bar
        assert_eq!(hit_test(&els, [5.0, 10.0], 0.0), None); // inside the unrotated bar
    }
}
