//! Theo in Rune: one application whose session outlives every window, shown
//! as a persistent desktop presence that transforms into a full workspace.

pub mod app;
pub mod behavior;
pub mod drawing;
pub mod face;
pub mod platform;
pub mod preferences;
pub mod presentation;
pub mod session;
pub mod signals;
pub mod surfaces;
pub mod views;

pub use app::Theo;
