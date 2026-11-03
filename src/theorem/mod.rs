//! The Agent place: Theorem inside Platinum, a navigator of projects and
//! conversations beside one card holding the conversation, cut around the
//! places bump and the composer (`docs/plans/theorem-workbench.md`).
//!
//! Theorem's renderer-neutral chat host (`theoremweb-agent-runtime`) owns the
//! conversation's state; `TheoremDriver` (`driver.rs`) fulfills its effects
//! against the authority, and [`Workbench`] draws its projection.

mod card;
mod composer;
mod driver;
mod navigator;
mod models;
mod saved;
mod starters;
#[cfg(test)]
mod tests;
mod transcript;
mod workbench;

pub use driver::DriverEvent;
#[cfg(feature = "snapshot")]
pub use driver::TheoremDriver;
pub use saved::init;
#[cfg(feature = "snapshot")]
pub(crate) use models::open as open_models;
pub use workbench::{
    CONTEXT, NewConversation, NextConversation, PreviousConversation, StopRun, Workbench,
};
