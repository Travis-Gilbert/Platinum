//! The face's matter: a fixed number of grains, each sprung toward its seat
//! in whichever feature owns it. Features share the grains by `share`, so a
//! feature that grows draws grains out of the others and they fly across.
//!
//! The grains are kept as parallel single-precision arrays, as the web keeps
//! them: every grain is stepped every frame, field by field, and the count is
//! fixed by the identity, so nothing here grows.
use std::f64::consts::PI;

use crate::channels::clamp;
use crate::feature::{Shape, seat_point};
use crate::tables::Identity;

/// What the matter is like this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matter {
    pub cohesion: f64,
    pub jitter: f64,
    pub drift: f64,
    pub size: f64,
    pub glow: f64,
    pub hue: f64,
}

/// Numbers per grain in the buffer a renderer draws: x, y, radius, phase, then premultiplied r, g, b, a.
pub const GRAIN_STRIDE: usize = 8;

const TAU: f64 = PI * 2.0;
const R2_A: f64 = 0.7548776662466927;
const R2_B: f64 = 0.5698402909980532;
const GOLDEN: f64 = 0.6180339887;

fn frac(value: f64) -> f64 {
    value - value.floor()
}

/// A cheap repeatable noise in [-1, 1] from two whole numbers.
fn noise(a: f64, b: f64) -> f64 {
    frac((a * 12.9898 + b * 78.233).sin() * 43758.5453) * 2.0 - 1.0
}

/// The matter's bounding box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    pub min_x: f64,
    pub max_x: f64,
    pub min_y: f64,
    pub max_y: f64,
}

#[derive(Clone, Debug)]
pub struct Field {
    count: usize,
    x: Vec<f32>,
    y: Vec<f32>,
    vx: Vec<f32>,
    vy: Vec<f32>,
    light: Vec<f32>,
    theta: Vec<f32>,
    u: Vec<f32>,
    phase: Vec<f32>,
    bounds: Vec<f64>,
    limit_x: f64,
    limit_y: f64,
    started: bool,
}

impl Field {
    pub fn new(identity: &Identity, seed: u32) -> Self {
        let count = identity.grains;
        let seed = f64::from(seed);
        let mut field = Self {
            count,
            x: vec![0.0; count],
            y: vec![0.0; count],
            vx: vec![0.0; count],
            vy: vec![0.0; count],
            light: vec![0.0; count],
            theta: vec![0.0; count],
            u: vec![0.0; count],
            phase: vec![0.0; count],
            bounds: Vec::new(),
            limit_x: identity.width / 2.0 - 1.0,
            limit_y: identity.height / 2.0 - 1.0,
            started: false,
        };
        let a = frac(seed * 0.000123 + 0.37);
        let b = frac(seed * 0.000457 + 0.11);
        // Seats are laid in a square and bent onto the feature shell by shell, so a boxy feature
        // fills evenly into its corners. One grain in four sits on the rim; the rest fill it.
        let mut fill = 0.0;
        let mut rim = 0.0;
        for index in 0..count {
            let (sx, sy);
            if index % 4 == 0 {
                rim += 1.0;
                let along = frac(a + rim * GOLDEN) * 4.0;
                let side = along.floor();
                let t = (along - side) * 2.0 - 1.0;
                sx = if side == 0.0 {
                    1.0
                } else if side == 1.0 {
                    -t
                } else if side == 2.0 {
                    -1.0
                } else {
                    t
                };
                sy = if side == 0.0 {
                    t
                } else if side == 1.0 {
                    1.0
                } else if side == 2.0 {
                    -t
                } else {
                    -1.0
                };
            } else {
                fill += 1.0;
                sx = frac(a + fill * R2_A) * 2.0 - 1.0;
                sy = frac(b + fill * R2_B) * 2.0 - 1.0;
            }
            let shell = sx.abs().max(sy.abs());
            field.theta[index] = ((sy.atan2(sx) + TAU) % TAU) as f32;
            field.u[index] = (shell * shell) as f32;
            field.phase[index] = frac(seed * 0.0001 + (index as f64 + 1.0) * GOLDEN) as f32;
        }
        field
    }

    pub fn count(&self) -> usize {
        self.count
    }

    /// Each grain's angle, depth and phase: what it is, not where it is.
    pub fn seats(&self) -> (&[f32], &[f32], &[f32]) {
        (&self.theta, &self.u, &self.phase)
    }

    /// Which shape owns each grain: ranks split by cumulative share.
    fn split(&mut self, shapes: &[Shape]) -> bool {
        let total: f64 = shapes.iter().map(|shape| shape.share.max(0.0)).sum();
        self.bounds.clear();
        let mut sum = 0.0;
        for shape in shapes {
            sum += shape.share.max(0.0);
            self.bounds.push(if total > 0.0 {
                (sum / total) * self.count as f64
            } else {
                0.0
            });
        }
        total > 0.0
    }

    /// Moves the grains on by `dt` seconds toward their seats. `time` is the
    /// face's own clock in seconds, which the tremble and wander are read from.
    pub fn step(&mut self, shapes: &mut [Shape], matter: &Matter, dt: f64, time: f64) {
        let any = self.split(shapes);
        // What a globe has turned and been stirred is kept here, so a change of speed carries on from where it was.
        for shape in shapes.iter_mut() {
            shape.spun += shape.spin * dt;
            shape.stirred += shape.stir * dt;
        }
        let cohesion = clamp(matter.cohesion, 0.0, 1.0);
        let base_omega = 6.0 + 26.0 * cohesion;
        let zeta = 0.5 + 0.45 * cohesion;
        let tremble = matter.jitter * 1.8;
        let wander = matter.drift * 1.3;
        let tick = (time * 24.0).floor();
        let fade = 1.0 - (-dt * 9.0).exp();
        let mut owner = 0usize;
        for index in 0..self.count {
            while owner + 1 < shapes.len() && index as f64 >= self.bounds[owner] {
                owner += 1;
            }
            let phase = f64::from(self.phase[index]);
            let mut tx = 0.0;
            let mut ty = 0.0;
            let mut light = 0.0;
            if any && let Some(shape) = shapes.get(owner) {
                let seat = seat_point(
                    shape,
                    f64::from(self.theta[index]),
                    f64::from(self.u[index]),
                );
                light = shape.alpha * seat.light;
                // The web reads the seat back out of a single-precision pair.
                tx = f64::from(seat.x as f32);
                ty = f64::from(seat.y as f32);
            }
            if tremble > 0.0 {
                tx += noise(index as f64, tick) * tremble;
                ty += noise(index as f64 + 7919.0, tick) * tremble;
            }
            if wander > 0.0 {
                tx += (time * 0.9 + phase * TAU * 7.0).sin() * wander;
                ty += (time * 0.7 + phase * TAU * 13.0).cos() * wander;
            }
            tx = clamp(tx, -self.limit_x, self.limit_x);
            ty = clamp(ty, -self.limit_y, self.limit_y);
            if !self.started {
                self.x[index] = tx as f32;
                self.y[index] = ty as f32;
                self.light[index] = light as f32;
                continue;
            }
            let omega = base_omega * (0.72 + 0.56 * phase);
            let px = f64::from(self.x[index]);
            let py = f64::from(self.y[index]);
            let mut vx = f64::from(self.vx[index]);
            let mut vy = f64::from(self.vy[index]);
            vx += (-omega * omega * (px - tx) - 2.0 * zeta * omega * vx) * dt;
            vy += (-omega * omega * (py - ty) - 2.0 * zeta * omega * vy) * dt;
            self.vx[index] = vx as f32;
            self.vy[index] = vy as f32;
            self.x[index] = (px + vx * dt) as f32;
            self.y[index] = (py + vy * dt) as f32;
            let lit = f64::from(self.light[index]);
            self.light[index] = (lit + (light - lit) * fade) as f32;
        }
        self.started = true;
    }

    /// The head moved under the matter by this much, in face units: the matter lags behind.
    pub fn shove(&mut self, dx: f64, dy: f64) {
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        for index in 0..self.count {
            let give = 0.5 + 0.5 * f64::from(self.phase[index]);
            self.x[index] = clamp(
                f64::from(self.x[index]) - dx * give,
                -self.limit_x,
                self.limit_x,
            ) as f32;
            self.y[index] = clamp(
                f64::from(self.y[index]) - dy * give,
                -self.limit_y,
                self.limit_y,
            ) as f32;
        }
    }

    /// Pushes the matter away from a point: a poke. `strength` is face units per second at the point.
    pub fn burst(&mut self, x: f64, y: f64, strength: f64, reach: f64) {
        for index in 0..self.count {
            let dx = f64::from(self.x[index]) - x;
            let dy = f64::from(self.y[index]) - y;
            let d2 = dx * dx + dy * dy;
            let fall = strength / (1.0 + d2 / (reach * reach));
            let root = d2.sqrt();
            let d = if root == 0.0 || root.is_nan() {
                1.0
            } else {
                root
            };
            let spin = f64::from(self.phase[index]) - 0.5;
            self.vx[index] =
                (f64::from(self.vx[index]) + ((dx / d) * fall - (dy / d) * fall * spin)) as f32;
            self.vy[index] =
                (f64::from(self.vy[index]) + ((dy / d) * fall + (dx / d) * fall * spin)) as f32;
        }
    }

    /// Writes every grain for drawing. `colour` is straight sRGB in 0..1. `out` holds `count * GRAIN_STRIDE` numbers.
    pub fn write(&self, out: &mut [f32], matter: &Matter, grain: f64, colour: [f64; 3]) {
        let glow = clamp(matter.glow, 0.0, 1.5);
        for (index, cell) in out
            .chunks_exact_mut(GRAIN_STRIDE)
            .take(self.count)
            .enumerate()
        {
            let phase = f64::from(self.phase[index]);
            let alpha = clamp(
                f64::from(self.light[index]) * glow * (0.62 + 0.3 * phase),
                0.0,
                1.0,
            );
            cell[0] = self.x[index];
            cell[1] = self.y[index];
            cell[2] = (grain * matter.size * (0.78 + 0.5 * phase)) as f32;
            cell[3] = self.phase[index];
            cell[4] = (colour[0] * alpha) as f32;
            cell[5] = (colour[1] * alpha) as f32;
            cell[6] = (colour[2] * alpha) as f32;
            cell[7] = alpha as f32;
        }
    }

    pub fn extent(&self) -> Extent {
        let mut extent = Extent {
            min_x: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            min_y: f64::INFINITY,
            max_y: f64::NEG_INFINITY,
        };
        for index in 0..self.count {
            let px = f64::from(self.x[index]);
            let py = f64::from(self.y[index]);
            extent.min_x = extent.min_x.min(px);
            extent.max_x = extent.max_x.max(px);
            extent.min_y = extent.min_y.min(py);
            extent.max_y = extent.max_y.max(py);
        }
        extent
    }
}

/// OKLCH to straight sRGB in 0..1 (Björn Ottosson's OKLab matrices).
pub fn oklch(lightness: f64, chroma: f64, hue: f64) -> [f64; 3] {
    let angle = (hue * PI) / 180.0;
    let a = chroma * angle.cos();
    let b = chroma * angle.sin();
    let l = (lightness + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m = (lightness - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s = (lightness - 0.0894841775 * a - 1.291485548 * b).powi(3);
    let gamma = |value: f64| {
        clamp(
            if value <= 0.0031308 {
                12.92 * value
            } else {
                1.055 * value.powf(1.0 / 2.4) - 0.055
            },
            0.0,
            1.0,
        )
    };
    [
        gamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        gamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        gamma(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
    ]
}
