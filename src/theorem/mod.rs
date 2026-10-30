//! The Agent place: Theorem inside Platinum, a navigator of projects and
//! conversations beside one raised card holding the conversation
//! (`docs/plans/theorem-workbench.md`).
//!
//! Theorem's renderer-neutral chat host (`theoremweb-agent-runtime`) owns the
//! conversation's state; `TheoremDriver` (`driver.rs`) fulfills its effects
//! against the authority, and [`Workbench`] draws its projection.

mod card;
mod composer;
mod driver;
mod navigator;
mod saved;
#[cfg(test)]
mod tests;
mod transcript;
mod workbench;

pub use driver::DriverEvent;
pub use saved::init;
pub use workbench::{
    CONTEXT, NewConversation, NextConversation, PreviousConversation, StopRun, Workbench,
    mounted_in,
};
