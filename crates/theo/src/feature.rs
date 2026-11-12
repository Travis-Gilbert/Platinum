//! Where one grain sits in a feature. A feature is a superellipse bent by
//! eight radial gains, cut by two lids, gathered into rings, bars, a mesh or
//! a globe when asked, then placed and turned. The arithmetic is web Theo's
//! (`src/lib/face/feature.ts`), step for step, so the same shape seats the
//! same grain in the same place.
use std::f64::consts::PI;

use crate::random::round;
use crate::tables::{Side, tables};

/// A feature's numbers at one instant, read off the rig.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// -1 left of the centre line, 1 right, 0 on it.
    pub side: f64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub round: f64,
    /// Kept in single precision, as the web keeps them.
    pub gains: [f32; 8],
    pub lid: f64,
    pub lid_tilt: f64,
    pub lid_bow: f64,
    pub low: f64,
    pub low_tilt: f64,
    pub low_bow: f64,
    pub hollow: f64,
    pub rings: f64,
    pub slats: f64,
    pub mesh: f64,
    pub globe: f64,
    pub spin: f64,
    pub stir: f64,
    pub loose: f64,
    /// How far a globe has turned, in turns, and how much it has been stirred: kept by the matter, not read off the rig.
    pub spun: f64,
    pub stirred: f64,
    pub turn: f64,
    pub scale: f64,
    pub alpha: f64,
    pub share: f64,
}

impl Shape {
    /// A shape at rest, to be filled in.
    pub fn rest(side: Side) -> Shape {
        let rest = |index: usize| tables().feature_params[index].rest;
        Shape {
            side: side.sign(),
            x: rest(0),
            y: rest(1),
            w: rest(2),
            h: rest(3),
            round: rest(4),
            gains: [1.0; 8],
            lid: 0.0,
            lid_tilt: 0.0,
            lid_bow: 0.0,
            low: 0.0,
            low_tilt: 0.0,
            low_bow: 0.0,
            hollow: 0.0,
            rings: 0.0,
            slats: 0.0,
            mesh: 0.0,
            globe: 0.0,
            spin: 0.0,
            stir: 0.0,
            loose: 0.0,
            spun: 0.0,
            stirred: 0.0,
            turn: 0.0,
            scale: 1.0,
            alpha: 1.0,
            share: 1.0,
        }
    }

    /// Sets a param by its place in `tables::PARAMS`.
    pub fn set(&mut self, param: usize, value: f64) {
        match param {
            0 => self.x = value,
            1 => self.y = value,
            2 => self.w = value,
            3 => self.h = value,
            4 => self.round = value,
            5..=12 => self.gains[param - 5] = value as f32,
            13 => self.lid = value,
            14 => self.lid_tilt = value,
            15 => self.lid_bow = value,
            16 => self.low = value,
            17 => self.low_tilt = value,
            18 => self.low_bow = value,
            19 => self.hollow = value,
            20 => self.rings = value,
            21 => self.slats = value,
            22 => self.mesh = value,
            23 => self.globe = value,
            24 => self.spin = value,
            25 => self.stir = value,
            26 => self.loose = value,
            27 => self.turn = value,
            28 => self.scale = value,
            29 => self.alpha = value,
            30 => self.share = value,
            _ => {}
        }
    }
}

const TAU: f64 = PI * 2.0;
/// How far down the eye the lids meet when the upper one shuts, as a fraction of the half height.
const MEET: f64 = 0.3;
/// What is left of a cut grain's distance past the lid: a shut eye is a line with a little body.
const RESIDUE: f64 = 0.07;
/// How much of the gap between two rings, or two bars, the matter of one fills.
const BAND: f64 = 0.34;
/// How much of the step between two dots of a mesh the matter of one fills.
const DOT: f64 = 0.3;
/// A meshed feature's rim: its share of the matter, its width, and how far the dots keep from it.
const RIM_SHARE: f64 = 0.24;
const RIM_WIDTH: f64 = 0.07;
const RIM_GAP: f64 = 0.03;
/// How many dots a globe has round its middle for each row from pole to pole.
const AROUND: f64 = 1.9;
/// How much of the step between two dots of a globe the matter of one fills.
const BEAD: f64 = 0.4;
/// How far the globe's axis leans toward whoever looks, in radians, and how far a stirred one wanders from that.
const LEAN: f64 = 0.5;
const WANDER: f64 = 0.38;
/// Orbits: the part of the rim's matter sent into orbit, how many, how far out in radii, how long a tail in radians.
const ORBIT_SHARE: f64 = 0.5;
const ORBITS: f64 = 3.0;
const ORBIT_REACH: f64 = 1.5;
const TAIL: f64 = 1.3;
/// A globe that has lost its shape.
const LUMP: f64 = 0.2;
const LUMP_SMALL: f64 = 0.14;
const STRAY: f64 = 0.3;
const STRAYS: f64 = 0.3;
const WAVER: f64 = 0.12;
const FRAY: f64 = 0.5;
/// How dim the far side of a globe is.
const FAR: f64 = 0.1;

fn fraction(value: f64) -> f64 {
    value - value.floor()
}

/// A repeatable number in [-1, 1] for a whole number: each row of a globe has its own.
fn own(index: f64) -> f64 {
    fraction((index * 12.9898 + 4.1414).sin() * 43758.5453) * 2.0 - 1.0
}

/// Where a grain sits, in face units, and how much of its light it keeps there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seat {
    pub x: f64,
    pub y: f64,
    /// 1, but for the far side of a globe.
    pub light: f64,
}

/// Where one grain sits in a feature. `theta` is its angle around the
/// feature (0 the outer side, a quarter turn the bottom) and `u` how far out
/// it is by area (0 the middle, 1 the rim).
pub fn seat_point(shape: &Shape, theta: f64, u: f64) -> Seat {
    let cos = theta.cos();
    let sin = theta.sin();
    let n = shape.round;
    let rim = (cos.abs().powf(n) + sin.abs().powf(n)).powf(-1.0 / n);
    let turn_at = ((theta / TAU) % 1.0) * 8.0;
    let index = (turn_at.floor() as i64 & 7) as usize;
    let part = turn_at - turn_at.floor();
    let smooth = part * part * (3.0 - 2.0 * part);
    let from = f64::from(shape.gains[index]);
    let gain = from + (f64::from(shape.gains[(index + 1) & 7]) - from) * smooth;
    let hollow = shape.hollow;
    let mut radius = (hollow * hollow + (1.0 - hollow * hollow) * u).sqrt();
    // Rings: each grain goes to the ring its depth falls in, the outermost on the rim.
    let rings = round(shape.rings);
    if rings >= 1.0 {
        let depth = (rings - 1e-6).min(u * rings);
        let ring = depth.floor();
        let outer = hollow + ((1.0 - hollow) * (ring + 1.0)) / rings;
        radius = outer - ((1.0 - hollow) / rings) * BAND * (1.0 - (depth - ring));
    }
    let mut x = radius * rim * gain * shape.w * cos;
    let mut y = radius * rim * gain * shape.h * sin;
    // Bars: each grain goes to the bar its height falls in.
    let slats = round(shape.slats);
    if slats >= 1.0 && shape.h > 0.0 {
        let height = (y / shape.h).min(1.0).max(-1.0);
        let bar = (slats - 1.0).min((((height + 1.0) / 2.0) * slats).floor());
        let middle = -1.0 + (2.0 * bar + 1.0) / slats;
        y = (middle + (height - middle) * BAND) * shape.h;
    }
    // Mesh: the outermost matter is the rim, and each grain inside goes to the dot its place falls in,
    // every other row half a step along. Where the nearest dot would cross the rim the grain looks further in.
    let mesh = round(shape.mesh);
    if mesh >= 1.0 && shape.w > 0.0 && shape.h > 0.0 {
        if u > 1.0 - RIM_SHARE {
            let band = 1.0 - RIM_WIDTH * ((1.0 - u) / RIM_SHARE);
            x = band * rim * gain * shape.w * cos;
            y = band * rim * gain * shape.h * sin;
        } else {
            let row_step = 3f64.sqrt() / 2.0;
            let step = (2.0 * shape.w) / mesh;
            let reach = 1.0 - RIM_WIDTH - RIM_GAP - (DOT * step) / (2.0 * shape.w);
            let mut inward = (u / (1.0 - RIM_SHARE)).sqrt();
            for _ in 0..4 {
                let px = inward * rim * gain * shape.w * cos;
                let py = inward * rim * gain * shape.h * sin;
                let row = round(py / (step * row_step));
                let odd = (row as i64) & 1 == 1;
                let shift = if odd { mesh % 2.0 } else { 1.0 - (mesh % 2.0) } / 2.0;
                let dot_x = (round(px / step - shift) + shift) * step;
                let dot_y = row * step * row_step;
                x = dot_x + (px - dot_x) * DOT;
                y = dot_y + (py - dot_y) * DOT;
                let past = ((dot_x / shape.w).abs().powf(n) + (dot_y / shape.h).abs().powf(n))
                    .powf(1.0 / n);
                if past <= reach {
                    break;
                }
                inward *= 0.82;
            }
        }
    }

    // Globe: the matter is dots on a ball that turns, seen from the side, and a few grains orbit it.
    let mut light = 1.0;
    let rows = round(shape.globe);
    if rows >= 1.0 {
        let turned = shape.spun * TAU;
        let stirred = shape.stirred * TAU;
        let (mut px, mut py, pz);
        // The rim of a flat feature is a quarter of its matter, all at one depth: part of it orbits, the rest is spread over the ball.
        let edge = u > 0.9995;
        let lot = fraction(theta * 7.3137);
        let pick = fraction(theta * 23.1731);
        if edge && lot < ORBIT_SHARE {
            let orbit = (ORBITS - 1.0).min((pick * ORBITS).floor());
            let back = fraction(pick * ORBITS);
            let head = orbit * 2.1 + (turned * 1.6 + stirred * 2.4) * (0.7 + 0.3 * orbit);
            let angle = head - back * back * TAIL;
            // How open the orbit is to whoever looks: never edge on, and each one different.
            let tip = 0.35 + 0.2 * orbit + 0.12 * (stirred * 0.31 + orbit).sin();
            let roll = (orbit * TAU) / ORBITS + 0.4;
            // A globe that has lost its shape has orbits that are no longer circles.
            let wide =
                ORBIT_REACH * (1.0 + shape.loose * WAVER * (3.0 * angle + orbit * 1.7).sin());
            let ox = angle.cos() * wide;
            let oy = angle.sin() * wide * tip.sin();
            let oz = angle.sin() * wide * tip.cos();
            px = ox * roll.cos() - oy * roll.sin();
            py = ox * roll.sin() + oy * roll.cos();
            pz = oz;
            light = 1.0 - 0.75 * back;
        } else {
            let deep = if edge { pick } else { u / 0.9995 };
            let lat = (1.0 - 2.0 * deep).min(1.0).max(-1.0).asin();
            let row = (rows - 1.0).min(((lat / PI + 0.5) * rows).floor().max(0.0));
            let row_lat = ((row + 0.5) / rows - 0.5) * PI;
            let dots = round(rows * AROUND * row_lat.cos()).max(1.0);
            let along = (theta / TAU) * dots;
            // The looser it is, the more each dot frays toward where its matter would lie if nothing gathered it.
            let bead = BEAD + (1.0 - BEAD) * FRAY * shape.loose;
            let dot = along.floor();
            // Each row turns with the ball, and slides past its neighbours by as much as the ball has been stirred.
            let lon = ((dot + 0.5 + (along - dot - 0.5) * bead) / dots) * TAU
                + ((row as i64) & 1) as f64 * (PI / dots)
                + turned
                + own(row) * stirred * 0.6;
            let up = row_lat
                + (lat - row_lat) * bead
                + shape.stir * 0.1 * (2.0 * lon + stirred * 1.3 + row).sin();
            // The ball's axis leans toward whoever looks, and wanders the more it is stirred.
            let lean = LEAN + WANDER * shape.stir * (stirred * 0.23 + 1.3).sin();
            let flat = up.cos() * lon.cos();
            px = up.cos() * lon.sin();
            py = up.sin() * lean.cos() - flat * lean.sin();
            let mut depth = up.sin() * lean.sin() + flat * lean.cos();
            if shape.loose > 0.0 {
                // Two lumps that ride the turning surface, and a part of the grains strayed from their dots.
                let lump = LUMP * (2.0 * lon + 1.3 * up + stirred * 0.7).sin()
                    + LUMP_SMALL * (3.0 * up - lon + 2.1 + stirred * 0.45).sin();
                let drawn = fraction(theta * 131.7 + u * 57.3);
                let strayed = if drawn > 1.0 - STRAYS {
                    ((drawn - (1.0 - STRAYS)) / STRAYS).powi(2) * STRAY
                } else {
                    0.0
                };
                let reach = 1.0 + shape.loose * (lump + strayed);
                px *= reach;
                py *= reach;
                depth *= reach;
            }
            pz = depth;
        }
        // Ball and orbits sway together, the more it is stirred.
        let sway = WANDER * shape.stir * (stirred * 0.17).sin();
        let swayed = px * sway.cos() - py * sway.sin();
        py = px * sway.sin() + py * sway.cos();
        px = swayed;
        x = px * shape.w;
        y = py * shape.h;
        let near = ((pz + 0.35) / 0.9).min(1.0).max(0.0);
        light *= FAR + (1.0 - FAR) * near * near * (3.0 - 2.0 * near);
    }

    // Lids. `across` runs -1 at the inner end to 1 at the outer.
    let h = shape.h;
    let across = if shape.w > 0.0 {
        (x / shape.w).min(1.4).max(-1.4)
    } else {
        0.0
    };
    let dome = 1.0 - across * across;
    let dragged = if shape.lid > 0.6 {
        (shape.lid - 0.6) / 0.4
    } else {
        0.0
    };
    let low = shape.low + dragged * (1.0 - shape.low) * ((1.0 - MEET) / 2.0);
    let mut top = -h + shape.lid * (1.0 + MEET) * h - shape.lid_tilt * h * across
        + shape.lid_bow * h * 0.9 * dome;
    let mut bottom =
        h - low * 2.0 * h - shape.low_tilt * h * -across - shape.low_bow * h * 0.9 * dome;
    if top > bottom {
        top = (top + bottom) / 2.0;
        bottom = top;
    }
    if y < top {
        y = top + (y - top) * RESIDUE;
    } else if y > bottom {
        y = bottom + (y - bottom) * RESIDUE;
    }

    x *= shape.scale;
    y *= shape.scale;
    if shape.turn != 0.0 {
        let angle = (shape.turn * PI) / 180.0;
        let (s, c) = angle.sin_cos();
        let turned = x * c - y * s;
        y = x * s + y * c;
        x = turned;
    }
    Seat {
        x: if shape.side == 0.0 {
            shape.x + x
        } else {
            shape.side * (shape.x + x)
        },
        y: shape.y + y,
        light,
    }
}
