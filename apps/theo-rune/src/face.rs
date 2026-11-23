//! Theo's face drawn as a Rune `Shape`: the dark plate, the grains on it,
//! and the arcs that leave its sides while it sounds.
//!
//! The measures are the GPUI desk's (`src/theo_desk/body.rs`), which are web
//! Theo's (Theorem-UI `components/face` at 88043d3): a visor 120 points wide,
//! a plate radius of 21.6 points with a faint inset ring and a soft drop
//! shadow, grains as discs at 55% of their alpha, and three arcs a side. The
//! head's place, tilt, scale and squash move the plate and its matter
//! together, as the web's CSS transform does.
//!
//! Differences from the web painter, forced by Rune's `Shape`: grains blend
//! over each other rather than adding light, a grain is kept or dropped by
//! whether its centre is on the plate, and the shadow is three widening
//! translucent outlines under the plate rather than a Gaussian blur (Rune's
//! blurred shadow is an element style, which would not follow the head's
//! tilt). The engine is unchanged: this module only reads a frame.
//!
//! SOURCING: `rune::shape::{Shape, Path}` and `rune::oklch`; the frame comes
//! from `theo::engine::Face` (Platinum `crates/theo`).

use std::f64::consts::PI;

use rune::color::srgb_decode;
use rune::shape::{LineCap, Shape};
use rune::{Color, oklch};
use theo::engine::{Emitting, Head};
use theo::particles::GRAIN_STRIDE;

/// The visor's width, in points.
pub const VISOR_PX: f64 = 120.0;

const PLATE_RADIUS_PX: f64 = 21.6;
const RING_PX: f64 = 1.5;
const WAVE_FROM: (f64, f64) = (44.0, 3.0);
const WAVE_NEAR: f64 = 20.0;
const WAVE_FAR: f64 = 52.0;
const WAVE_OPEN: f64 = 0.66;
const WAVE_PERIOD: f64 = 1_150.0;
const WAVE_WIDTH: f64 = 2.6;
const ARC_STEPS: usize = 14;
const CORNER_STEPS: usize = 6;
/// The shadow, 0 3px 10px rgb(0 0 0 / 0.28), as widening outlines.
const SHADOW_DROP: f64 = 3.0;
const SHADOW_LAYERS: [(f64, f32); 3] = [(1.5, 0.12), (3.5, 0.08), (6.0, 0.05)];

/// One frame of the face, copied out of the companion for painting.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceFrame {
    pub grains: Vec<f32>,
    pub head: Head,
    pub emitting: Emitting,
    /// The face's width and height, in its own units.
    pub width: f64,
    pub height: f64,
    pub hue: f64,
    pub still: bool,
    /// The face's clock, in milliseconds.
    pub now: f64,
    /// Whether the desktop behind is dark, which the arcs lean away from.
    pub dark: bool,
}

impl FaceFrame {
    /// The frame the companion draws at `now`, its primary activity's
    /// measurements fed in first.
    pub fn of(companion: &mut companion::Companion, now: f64, dark: bool) -> Self {
        let grains = companion.frame(now).to_vec();
        let face = &companion.face;
        Self {
            grains,
            head: face.head(),
            emitting: face.emitting(),
            width: f64::from(face.identity.width),
            height: f64::from(face.identity.height),
            hue: face.hue,
            still: face.still(),
            now,
            dark,
        }
    }

    /// The plate's half width and half height, in points.
    pub fn plate_half(&self) -> (f64, f64) {
        (VISOR_PX / 2.0, VISOR_PX * self.height / self.width / 2.0)
    }

    /// How many grains are drawn on the plate, for tests and diagnostics.
    pub fn drawn_grains(&self) -> usize {
        let place = Placement::new((0.0, 0.0), self);
        let (hw, hh) = self.plate_half();
        self.grains
            .chunks_exact(GRAIN_STRIDE)
            .filter(|g| g[7] > 0.004)
            .filter(|g| {
                let (x, y) = (f64::from(g[0]) * place.unit, f64::from(g[1]) * place.unit);
                on_plate(x, y, hw, hh, PLATE_RADIUS_PX)
            })
            .count()
    }
}

/// Face units to element points, through the head.
struct Placement {
    centre: (f64, f64),
    unit: f64,
    cos: f64,
    sin: f64,
    sx: f64,
    sy: f64,
}

impl Placement {
    fn new(centre: (f64, f64), frame: &FaceFrame) -> Self {
        let unit = VISOR_PX / frame.width;
        let head = frame.head;
        let (sin, cos) = head.tilt.to_radians().sin_cos();
        Self {
            centre: (centre.0 + head.dx * unit, centre.1 + head.dy * unit),
            unit,
            cos,
            sin,
            sx: head.scale * (1.0 + head.squash),
            sy: head.scale * (1.0 - head.squash),
        }
    }

    /// A point given in visor points from the plate's centre.
    fn at(&self, x: f64, y: f64) -> [f32; 2] {
        let (x, y) = (x * self.sx, y * self.sy);
        [
            (self.centre.0 + x * self.cos - y * self.sin) as f32,
            (self.centre.1 + x * self.sin + y * self.cos) as f32,
        ]
    }

    fn scale(&self) -> f64 {
        (self.sx + self.sy) / 2.0
    }
}

/// The plate's outline, inset by `inset`, in visor points about its centre.
fn outline(half_w: f64, half_h: f64, radius: f64, inset: f64) -> Vec<(f64, f64)> {
    let (w, h, r) = (half_w - inset, half_h - inset, (radius - inset).max(0.0));
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

/// The plate's colour: web Theo's oklch(0.17 0.012 255).
pub fn plate_colour() -> Color {
    oklch(0.17, 0.012, 255.0)
}

/// The face drawn about `centre` (element points), at `alpha` overall.
pub fn face(shape: Shape, frame: &FaceFrame, centre: (f64, f64), alpha: f32) -> Shape {
    if alpha <= 0.0 {
        return shape;
    }
    let place = Placement::new(centre, frame);
    let (half_w, half_h) = frame.plate_half();
    let mut shape = plate(shape, &place, half_w, half_h, alpha);
    shape = grains(shape, &place, frame, half_w, half_h, alpha);
    arcs(shape, &place, frame, alpha)
}

fn plate(mut shape: Shape, place: &Placement, half_w: f64, half_h: f64, alpha: f32) -> Shape {
    let at = |points: Vec<(f64, f64)>, dy: f64| -> Vec<[f32; 2]> {
        points
            .into_iter()
            .map(|(x, y)| {
                let [px, py] = place.at(x, y);
                [px, py + dy as f32]
            })
            .collect()
    };
    for (grow, a) in SHADOW_LAYERS.iter().rev() {
        let points = outline(half_w + grow, half_h + grow, PLATE_RADIUS_PX + grow, 0.0);
        shape = shape.fill(at(points, SHADOW_DROP), Color::BLACK.alpha(a * alpha));
    }
    let body = outline(half_w, half_h, PLATE_RADIUS_PX, 0.0);
    shape = shape.fill(at(body, 0.0), plate_colour().alpha(alpha));
    let ring = outline(half_w, half_h, PLATE_RADIUS_PX, RING_PX / 2.0);
    shape.stroke_with(
        at(ring, 0.0),
        true,
        (RING_PX * place.scale()) as f32,
        LineCap::Butt,
        Color::WHITE.alpha(0.1 * alpha),
    )
}

/// The disc around `centre` with `radius`, as a polygon fine enough to read
/// round at its size.
fn disc(centre: [f32; 2], radius: f32) -> Vec<[f32; 2]> {
    let sides = if radius < 1.5 {
        6
    } else if radius < 4.0 {
        10
    } else {
        16
    };
    (0..sides)
        .map(|i| {
            let angle = std::f32::consts::TAU * i as f32 / sides as f32;
            [
                centre[0] + radius * angle.cos(),
                centre[1] + radius * angle.sin(),
            ]
        })
        .collect()
}

fn grains(
    mut shape: Shape,
    place: &Placement,
    frame: &FaceFrame,
    half_w: f64,
    half_h: f64,
    alpha: f32,
) -> Shape {
    let scale = place.scale();
    // The engine's colour is premultiplied, gamma-encoded sRGB.
    for grain in frame.grains.chunks_exact(GRAIN_STRIDE) {
        let a = grain[7];
        if a <= 0.004 {
            continue;
        }
        let (x, y) = (
            f64::from(grain[0]) * place.unit,
            f64::from(grain[1]) * place.unit,
        );
        if !on_plate(x, y, half_w, half_h, PLATE_RADIUS_PX) {
            continue;
        }
        let radius = (f64::from(grain[2]) * place.unit * scale) as f32;
        let channel = |c: f32| srgb_decode((c / a).min(1.0));
        let colour = Color {
            r: channel(grain[4]),
            g: channel(grain[5]),
            b: channel(grain[6]),
            a: a * 0.55 * alpha,
        };
        shape = shape.fill(disc(place.at(x, y), radius), colour);
    }
    shape
}

/// Three arcs a side, travelling out while the face emits on that side; with
/// motion reduced they stand where they are.
fn arcs(mut shape: Shape, place: &Placement, frame: &FaceFrame, alpha: f32) -> Shape {
    let Emitting { left, right } = frame.emitting;
    // The eye's colour leaning to the ink: color-mix(in oklch, eye 62%, ink).
    let ink = if frame.dark { 0.97 } else { 0.21 };
    let colour = oklch(
        (0.62 * 0.9 + 0.38 * ink) as f32,
        (0.62 * 0.11) as f32,
        frame.hue as f32,
    );
    let width = (WAVE_WIDTH * place.scale()) as f32;
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
        let centre = (side * WAVE_FROM.0, WAVE_FROM.1);
        let points: Vec<[f32; 2]> = (0..=ARC_STEPS)
            .map(|step| {
                let angle = -WAVE_OPEN + 2.0 * WAVE_OPEN * step as f64 / ARC_STEPS as f64;
                place.at(
                    centre.0 + side * radius * angle.cos(),
                    centre.1 + radius * angle.sin(),
                )
            })
            .collect();
        shape = shape.stroke_with(
            points,
            false,
            width,
            LineCap::Round,
            colour.alpha(opacity as f32 * alpha),
        );
    }
    shape
}

#[cfg(test)]
mod tests {
    use super::*;
    use companion::Companion;
    use theo::engine::FaceOptions;

    fn frame_at(now: f64) -> FaceFrame {
        let mut companion = Companion::new(FaceOptions::default());
        FaceFrame::of(&mut companion, now, true)
    }

    #[test]
    fn the_plate_holds_its_rounded_corners() {
        let (w, h, r) = (60.0, 38.4, 21.6);
        assert!(on_plate(0.0, 0.0, w, h, r));
        assert!(on_plate(59.0, 0.0, w, h, r));
        assert!(!on_plate(59.5, 38.0, w, h, r));
        assert!(!on_plate(61.0, 0.0, w, h, r));
    }

    #[test]
    fn the_existing_engine_draws_its_matter_on_the_plate() {
        let frame = frame_at(1_000.0);
        assert_eq!(frame.grains.len() % GRAIN_STRIDE, 0);
        assert!(frame.drawn_grains() > 100, "the face shows its grains");
        let shape = face(Shape::new(), &frame, (132.0, 66.0), 1.0);
        assert!(!shape.is_empty());
    }

    #[test]
    fn a_hidden_face_draws_nothing() {
        let frame = frame_at(0.0);
        assert!(face(Shape::new(), &frame, (0.0, 0.0), 0.0).is_empty());
    }

    #[test]
    fn the_head_moves_the_plate() {
        let mut frame = frame_at(0.0);
        frame.head.dx = 0.0;
        frame.head.dy = 0.0;
        frame.head.tilt = 0.0;
        let still = Placement::new((100.0, 50.0), &frame);
        frame.head.dx = 10.0;
        let moved = Placement::new((100.0, 50.0), &frame);
        assert!(moved.at(0.0, 0.0)[0] > still.at(0.0, 0.0)[0]);
    }

    #[test]
    fn arcs_appear_only_while_the_face_sounds() {
        let mut frame = frame_at(0.0);
        frame.grains.clear();
        frame.emitting = Emitting {
            left: 0.0,
            right: 0.0,
        };
        let quiet = face(Shape::new(), &frame, (0.0, 0.0), 1.0);
        frame.emitting = Emitting {
            left: 1.0,
            right: 1.0,
        };
        frame.now = 300.0;
        let sounding = face(Shape::new(), &frame, (0.0, 0.0), 1.0);
        assert_ne!(quiet, sounding);
    }
}
