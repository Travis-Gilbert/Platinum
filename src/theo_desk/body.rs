//! Theo's body drawn in GPUI: the dark plate, the grains on it, and the arcs
//! that leave its sides while it sounds.
//!
//! The measures are web Theo's (Theorem-UI `components/face` at 88043d3): a
//! visor 120 points wide, a plate radius of 21.6 points with a faint inset
//! ring and a soft drop shadow, grains as discs at 55% of their alpha, and
//! three arcs a side. The head's place, tilt, scale and squash move the plate
//! and its matter together, as the web's CSS transform does.
//!
//! Two differences from the web painter, both forced by GPUI: grains blend
//! over each other rather than adding light (`lighter`), so where many
//! overlap they read a little dimmer; and the shadow is painted upright
//! under a tilted plate, since GPUI's shadows do not rotate. A grain is kept
//! or dropped by whether its centre is on the plate, where the web clips it
//! at the plate's edge.
//!
//! SOURCING: GPUI's `canvas`, `paint_quad`, `paint_drop_shadows` and
//! `PathBuilder`; colours through `theo::particles::oklch`.

use std::f64::consts::PI;

use gpui_kit::{
    App, BorderStyle, Bounds, BoxShadow, Corners, Edges, Hsla, IntoElement, PathBuilder, Pixels,
    Point, Rgba, Styled as _, Window, canvas, point, px, quad, size,
};
use theo::engine::{Emitting, Head};
use theo::particles::{GRAIN_STRIDE, oklch};

/// The visor's width, in points.
pub const VISOR_PX: f64 = 120.0;
/// Room around the visor for the arcs and the head's travel, in points.
pub const BODY_SIZE: (f64, f64) = (264.0, 132.0);

const PLATE_RADIUS_PX: f64 = 21.6;
const RING_PX: f64 = 1.5;
/// Web Theo's arcs: where on each side they start, how near and far they
/// reach, how wide they open, and how long one takes to travel.
const WAVE_FROM: (f64, f64) = (44.0, 3.0);
const WAVE_NEAR: f64 = 20.0;
const WAVE_FAR: f64 = 52.0;
const WAVE_OPEN: f64 = 0.66;
const WAVE_PERIOD: f64 = 1_150.0;
const WAVE_WIDTH: f64 = 2.6;
const ARC_STEPS: usize = 14;
const CORNER_STEPS: usize = 6;

/// One frame of the face, copied out of the companion for painting.
#[derive(Clone, Debug)]
pub struct BodyFrame {
    pub grains: Vec<f32>,
    pub head: Head,
    pub emitting: Emitting,
    /// The face's width and height, in its own units.
    pub width: f64,
    pub height: f64,
    pub hue: f64,
    pub still: bool,
    pub now: f64,
    /// Whether the desktop behind is dark, which the arcs lean away from.
    pub dark: bool,
}

/// The body, as an element filling what it is given.
pub fn body(frame: BodyFrame) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window: &mut Window, _: &mut App| paint(bounds, &frame, window),
    )
    .size_full()
}

/// Face units to window points, through the head.
struct Placement {
    centre: (f64, f64),
    unit: f64,
    cos: f64,
    sin: f64,
    sx: f64,
    sy: f64,
}

impl Placement {
    fn new(bounds: Bounds<Pixels>, frame: &BodyFrame) -> Self {
        let unit = VISOR_PX / frame.width;
        let centre = bounds.center();
        let head = frame.head;
        let (sin, cos) = head.tilt.to_radians().sin_cos();
        Self {
            centre: (
                f64::from(f32::from(centre.x)) + head.dx * unit,
                f64::from(f32::from(centre.y)) + head.dy * unit,
            ),
            unit,
            cos,
            sin,
            sx: head.scale * (1.0 + head.squash),
            sy: head.scale * (1.0 - head.squash),
        }
    }

    /// A point given in visor points from the plate's centre.
    fn at(&self, x: f64, y: f64) -> Point<Pixels> {
        let (x, y) = (x * self.sx, y * self.sy);
        point(
            px((self.centre.0 + x * self.cos - y * self.sin) as f32),
            px((self.centre.1 + x * self.sin + y * self.cos) as f32),
        )
    }

    fn scale(&self) -> f64 {
        (self.sx + self.sy) / 2.0
    }
}

fn rgba([r, g, b]: [f64; 3], a: f64) -> Rgba {
    Rgba {
        r: r as f32,
        g: g as f32,
        b: b as f32,
        a: a as f32,
    }
}

/// The plate's outline, inset by `inset`, in visor points about its centre.
fn outline(half_w: f64, half_h: f64, radius: f64, inset: f64) -> Vec<(f64, f64)> {
    let (w, h, r) = (half_w - inset, half_h - inset, (radius - inset).max(0.0));
    // Each corner's centre, and the angle its quarter starts at.
    let corners = [
        ((w - r, -(h - r)), -PI / 2.0),
        ((w - r, h - r), 0.0),
        ((-(w - r), h - r), PI / 2.0),
        ((-(w - r), -(h - r)), PI),
    ];
    let mut points = Vec::with_capacity(4 * (CORNER_STEPS + 1));
    for ((cx, cy), start) in corners {
        for step in 0..=CORNER_STEPS {
            let angle = start + (PI / 2.0) * step as f64 / CORNER_STEPS as f64;
            points.push((cx + r * angle.cos(), cy + r * angle.sin()));
        }
    }
    points
}

/// Whether a point in visor points about the centre lies on the plate.
fn on_plate(x: f64, y: f64, half_w: f64, half_h: f64, radius: f64) -> bool {
    let (ax, ay) = (x.abs(), y.abs());
    if ax > half_w || ay > half_h {
        return false;
    }
    let (cx, cy) = (half_w - radius, half_h - radius);
    ax <= cx || ay <= cy || (ax - cx).hypot(ay - cy) <= radius
}

fn paint(bounds: Bounds<Pixels>, frame: &BodyFrame, window: &mut Window) {
    let place = Placement::new(bounds, frame);
    let half_w = VISOR_PX / 2.0;
    let half_h = VISOR_PX * frame.height / frame.width / 2.0;

    // The shadow, upright: 0 3px 10px rgb(0 0 0 / 0.28).
    let scale = place.scale();
    let shadow_bounds = Bounds::centered_at(
        point(px(place.centre.0 as f32), px(place.centre.1 as f32)),
        size(
            px((2.0 * half_w * place.sx) as f32),
            px((2.0 * half_h * place.sy) as f32),
        ),
    );
    window.paint_drop_shadows(
        shadow_bounds,
        Corners::all(px((PLATE_RADIUS_PX * scale) as f32)),
        &[BoxShadow {
            color: Hsla::from(rgba([0.0, 0.0, 0.0], 0.28)),
            offset: point(px(0.), px(3.)),
            blur_radius: px(10.),
            spread_radius: px(0.),
            inset: false,
        }],
    );

    // The plate, and its ring an inset hair inside the edge.
    let plate = oklch(0.17, 0.012, 255.0);
    let mut fill = PathBuilder::fill();
    trace(
        &mut fill,
        &place,
        &outline(half_w, half_h, PLATE_RADIUS_PX, 0.0),
    );
    if let Ok(path) = fill.build() {
        window.paint_path(path, rgba(plate, 1.0));
    }
    let mut ring = PathBuilder::stroke(px((RING_PX * scale) as f32));
    trace(
        &mut ring,
        &place,
        &outline(half_w, half_h, PLATE_RADIUS_PX, RING_PX / 2.0),
    );
    if let Ok(path) = ring.build() {
        window.paint_path(path, rgba([1.0, 1.0, 1.0], 0.1));
    }

    // The grains: their colour is premultiplied, so it is divided out.
    for grain in frame.grains.chunks_exact(GRAIN_STRIDE) {
        let alpha = f64::from(grain[7]);
        if alpha <= 0.004 {
            continue;
        }
        let (x, y) = (
            f64::from(grain[0]) * place.unit,
            f64::from(grain[1]) * place.unit,
        );
        if !on_plate(x, y, half_w, half_h, PLATE_RADIUS_PX) {
            continue;
        }
        let radius = f64::from(grain[2]) * place.unit * scale;
        let colour = [
            (f64::from(grain[4]) / alpha).min(1.0),
            (f64::from(grain[5]) / alpha).min(1.0),
            (f64::from(grain[6]) / alpha).min(1.0),
        ];
        let disc = Bounds::centered_at(
            place.at(x, y),
            size(px((radius * 2.0) as f32), px((radius * 2.0) as f32)),
        );
        window.paint_quad(quad(
            disc,
            Corners::all(px(radius as f32)),
            rgba(colour, alpha * 0.55),
            Edges::default(),
            gpui_kit::transparent_black(),
            BorderStyle::default(),
        ));
    }

    paint_arcs(&place, frame, window);
}

fn trace(path: &mut PathBuilder, place: &Placement, points: &[(f64, f64)]) {
    let mut points = points.iter();
    let Some(&(x, y)) = points.next() else {
        return;
    };
    path.move_to(place.at(x, y));
    for &(x, y) in points {
        path.line_to(place.at(x, y));
    }
    path.close();
}

/// Three arcs a side, travelling out while the face emits on that side; with
/// motion reduced they stand where they are.
fn paint_arcs(place: &Placement, frame: &BodyFrame, window: &mut Window) {
    let Emitting { left, right } = frame.emitting;
    // The eye's colour leaning to the ink, so the arcs read on a light
    // desktop as on a dark one: color-mix(in oklch, eye 62%, ink).
    let ink_lightness = if frame.dark { 0.97 } else { 0.21 };
    let colour = oklch(0.62 * 0.9 + 0.38 * ink_lightness, 0.62 * 0.11, frame.hue);
    let width = px((WAVE_WIDTH * place.scale()) as f32);
    for index in 0..6 {
        let side = if index < 3 { -1.0 } else { 1.0 };
        let amount = if side < 0.0 { left } else { right };
        if amount < 0.02 {
            continue;
        }
        let turn = ((index % 3) as f64 + 0.5) / 3.0;
        let along = if frame.still {
            turn
        } else {
            (frame.now / WAVE_PERIOD + turn).rem_euclid(1.0)
        };
        let radius = WAVE_NEAR + (WAVE_FAR - WAVE_NEAR) * along;
        let reach = ((0.3 + 0.85 * amount - along) / 0.2).clamp(0.0, 1.0);
        let opacity = (amount * 1.5).min(1.0) * (PI * along).sin().powf(0.7) * reach;
        if opacity <= 0.0 {
            continue;
        }
        // The web draws in the visor's box, whose centre is the plate's.
        let centre = (side * WAVE_FROM.0, WAVE_FROM.1);
        let mut arc = PathBuilder::stroke(width);
        for step in 0..=ARC_STEPS {
            let angle = -WAVE_OPEN + 2.0 * WAVE_OPEN * step as f64 / ARC_STEPS as f64;
            let x = centre.0 + side * radius * angle.cos();
            let y = centre.1 + radius * angle.sin();
            let at = place.at(x, y);
            if step == 0 {
                arc.move_to(at);
            } else {
                arc.line_to(at);
            }
        }
        if let Ok(path) = arc.build() {
            window.paint_path(path, rgba(colour, opacity));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plate_holds_its_rounded_corners() {
        let (w, h, r) = (60.0, 38.4, 21.6);
        assert!(on_plate(0.0, 0.0, w, h, r));
        assert!(on_plate(59.0, 0.0, w, h, r));
        // Inside the box, outside the corner's curve.
        assert!(!on_plate(59.5, 38.0, w, h, r));
        assert!(!on_plate(61.0, 0.0, w, h, r));
    }

    #[test]
    fn the_outline_closes_on_the_plate_edge() {
        let points = outline(60.0, 38.4, 21.6, 0.0);
        for (x, y) in points {
            let (ax, ay) = (x.abs(), y.abs());
            assert!(ax <= 60.0 + 1e-9 && ay <= 38.4 + 1e-9);
        }
    }
}
