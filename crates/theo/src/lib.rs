//! Theo's face without a renderer: programs, channels, matter and reactions.
//!
//! This is a port of web Theo's pure core (`Travis-Gilbert/Theorem-UI`,
//! `src/lib/face`), checked against fixtures written by that implementation
//! (`fixtures/generate.ts`): the same program on the same clock gives the
//! same channels and seats the same grains in both. It draws nothing, opens
//! no window and reads no clock of its own; a host gives it time, signals it
//! has measured and events that have happened.
pub mod autonomy;
pub mod book;
pub mod channels;
pub mod engine;
pub mod feature;
pub mod json;
pub mod layers;
pub mod memory;
pub mod ordered;
pub mod particles;
pub mod player;
pub mod program;
pub mod random;
pub mod reactions;
pub mod signals;
pub mod tables;
pub mod work;
