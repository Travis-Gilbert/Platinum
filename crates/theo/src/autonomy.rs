//! What a face does when nothing asks it to: blinks at irregular intervals
//! with the odd double, small gaze flicks, breath. Laid over the rig as
//! one-frame modifiers, so it never holds a channel. Each scales with its
//! `auto.*` channel, so a performance or a mood turns them up, down or off
//! like anything else.
use std::f64::consts::PI;

use crate::layers::Rig;
use crate::random::Random;

const TAU: f64 = PI * 2.0;

#[derive(Clone, Debug)]
pub struct Autonomy {
    random: Random,
    blink_in: f64,
    /// Seconds into the current blink, or negative when not blinking.
    blink_at: f64,
    again: bool,
    flick_in: f64,
    flick_x: f64,
    flick_y: f64,
    now_x: f64,
    now_y: f64,
    breath: f64,
    /// The eyes' lid and height channels, kept so a frame builds no names. Rebuilt when the eyes change.
    eyes: Vec<String>,
    lids: Vec<String>,
    heights: Vec<String>,
}

impl Autonomy {
    pub fn new(mut random: Random) -> Self {
        let blink_in = random.between(1.2, 3.4);
        let flick_in = random.between(0.8, 2.4);
        Self {
            random,
            blink_in,
            blink_at: -1.0,
            again: false,
            flick_in,
            flick_x: 0.0,
            flick_y: 0.0,
            now_x: 0.0,
            now_y: 0.0,
            breath: 0.0,
            eyes: Vec::new(),
            lids: Vec::new(),
            heights: Vec::new(),
        }
    }

    /// Starts a blink now, whatever the timer says.
    pub fn blink(&mut self) {
        if self.blink_at < 0.0 {
            self.blink_at = 0.0;
        }
    }

    fn know_eyes(&mut self, rig: &Rig) {
        let eyes = || {
            rig.channels
                .features()
                .filter(|feature| feature.id.starts_with("eye"))
        };
        if eyes()
            .map(|feature| feature.id.as_str())
            .eq(self.eyes.iter().map(String::as_str))
        {
            return;
        }
        self.eyes = eyes().map(|feature| feature.id.clone()).collect();
        self.lids = self.eyes.iter().map(|id| format!("{id}.lid")).collect();
        self.heights = self.eyes.iter().map(|id| format!("{id}.h")).collect();
    }

    /// Moves on by `dt` seconds and lays this frame's modifiers over the rig.
    pub fn tick(&mut self, rig: &mut Rig, dt: f64) {
        if rig.still {
            return;
        }
        self.know_eyes(rig);
        let blink_rate = rig.read("auto.blink");
        if self.blink_at < 0.0 && blink_rate > 0.0 {
            self.blink_in -= dt * blink_rate;
            if self.blink_in <= 0.0 {
                self.blink_at = 0.0;
                self.again = self.random.next() < 0.16;
                self.blink_in = self.random.between(2.4, 6.2);
            }
        }
        if self.blink_at >= 0.0 {
            self.blink_at += dt;
            // Shut in 70 ms, held 40, open in 130.
            let t = self.blink_at;
            let amount = if t < 0.07 {
                t / 0.07
            } else if t < 0.11 {
                1.0
            } else {
                (1.0 - (t - 0.11) / 0.13).max(0.0)
            };
            for lid in &self.lids {
                rig.toward(lid, 1.0, amount * amount * (3.0 - 2.0 * amount));
            }
            if t >= 0.24 {
                self.blink_at = if self.again { 0.0 } else { -1.0 };
                self.again = false;
            }
        }

        let flick = rig.read("auto.saccade");
        if flick > 0.0 {
            self.flick_in -= dt * flick.max(0.4);
            if self.flick_in <= 0.0 {
                self.flick_in = self.random.between(0.7, 3.2);
                let reach = 0.16 * flick.min(2.0);
                self.flick_x = self.random.between(-reach, reach);
                self.flick_y = self.random.between(-reach * 0.6, reach * 0.6);
            }
        } else {
            self.flick_x = 0.0;
            self.flick_y = 0.0;
        }
        // A flick is quick but not instant.
        let quick = 1.0 - (-dt * 26.0).exp();
        self.now_x += (self.flick_x - self.now_x) * quick;
        self.now_y += (self.flick_y - self.now_y) * quick;
        rig.offset("gaze.x", self.now_x);
        rig.offset("gaze.y", self.now_y);

        let depth = rig.read("auto.breath");
        if depth > 0.0 {
            // Deeper breath is slower breath: sleep breathes long.
            self.breath += (dt * TAU) / (3.4 + depth * 1.1);
            let level = self.breath.sin();
            rig.offset("face.dy", level * 0.5 * depth);
            rig.offset("face.scale", level * 0.012 * depth);
            for height in &self.heights {
                rig.offset(height, level * 0.28 * depth);
            }
        }
    }
}
