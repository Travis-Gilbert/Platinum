//! The notes app's Servo side: the `frame-notes:` pages a vault serves, and
//! the conversation between a reading page and the host.
//!
//! Nothing here depends on the browser's chrome (`Page`, `Browser`), so the
//! acceptance example (`examples/notes_check.rs`) includes this module by path
//! and exercises the same code the app runs.
//!
//! Ownership, by the data-oriented rules:
//! - the authority is each vault, owned by one worker thread (`service`);
//! - rendered HTML, digests and previews are derived from it per request
//!   (`ops`), never stored beside it;
//! - what a reading page shows, and the callout folds, are view state of the
//!   session (`session`, `patch`).
//!
//! Servo's threads reach a vault only by sending the worker a closure on a
//! bounded queue and awaiting the answer (`protocol`).

pub mod assets;
pub mod ops;
pub mod patch;
pub mod protocol;
pub mod route;
pub mod service;
pub mod session;
pub mod shell;

use servo::protocol_handler::ProtocolRegistry;

use crate::engine::NOTES_SCHEME;
use service::Vaults;

/// The registry that serves the open vaults' pages. Vaults opened later are
/// found through `vaults`, so this is built once.
pub fn protocols(vaults: &Vaults) -> ProtocolRegistry {
    let mut registry = ProtocolRegistry::default();
    registry
        .register(NOTES_SCHEME, protocol::Handler::new(vaults.clone()))
        .expect("the notes scheme is registered once");
    registry
}
