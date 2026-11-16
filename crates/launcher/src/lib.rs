//! Theo's launcher, less the window.
//!
//! A host has a window, a keyboard, a conversation and the Mac under it. This
//! crate has none of them. It holds what the person typed, what each lane
//! found for it, which row is selected and what Enter will do there, so that
//! every one of those is decided in one place and can be tested without an
//! application.
//!
//! The data, and who owns it:
//!
//! - **Standing items** (applications, commands, conversations) are handed in
//!   whole by the host and matched against each keystroke here. Tens to a few
//!   hundred rows; replaced, never appended to.
//! - **Asked items** (files) are found off the main thread for one query and
//!   delivered with the generation they were asked under. A delivery for an
//!   older query is refused, so a slow answer never replaces a newer one.
//! - **The selection** is an item's identity, not a row number, so it stays
//!   on the same thing when slower results arrive around it.
//!
//! What it will not do: act. `Session::confirm` names the item and the verb;
//! the host opens the file, runs the command or sends the message. Looking
//! something up never starts a conversation: asking Theo is its own row, and
//! only choosing that row asks.
//!
//! SOURCING: the matcher is a small subsequence scorer in the manner of fzf
//! and Sublime Text, written here so the crate depends on nothing and tests
//! without the pinned checkouts. `apps` reads application folders with the
//! standard library; `spotlight` asks macOS's own index through `mdfind`.

pub mod apps;
mod item;
pub mod matching;
mod session;
pub mod spotlight;
#[cfg(test)]
mod testing;
pub mod words;

pub use item::{Fact, Item, ItemId, Kind, Origin, Target, Verb};
pub use session::{Chosen, Escaped, Lane, Mode, Refusal, Scope, Section, Session, Waiting};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A way to give up a search that is still running: the flag says stop, and
/// a process the search started is ended so a blocked read returns.
#[derive(Clone, Debug, Default)]
pub struct Cancel {
    stopped: Arc<AtomicBool>,
    child: Arc<Mutex<Option<std::process::Child>>>,
}

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stops the search. What it has found so far is not delivered.
    pub fn cancel(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Ok(mut child) = self.child.lock()
            && let Some(child) = child.as_mut()
        {
            let _ = child.kill();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Keeps the process a search runs, so `cancel` can end it. A search
    /// already cancelled has its process ended at once.
    pub(crate) fn keep(&self, mut child: std::process::Child) {
        if self.is_cancelled() {
            let _ = child.kill();
        }
        if let Ok(mut kept) = self.child.lock() {
            *kept = Some(child);
        }
    }

    /// Takes the kept process back, to wait for it.
    pub(crate) fn take(&self) -> Option<std::process::Child> {
        self.child.lock().ok().and_then(|mut kept| kept.take())
    }
}

/// Something that finds items for a query off the main thread: files on
/// this Mac, say. It is asked once per query and may be cancelled.
pub trait Source: Send + Sync {
    /// The items for `query`, best first, or why there are none to be had.
    /// An empty list means nothing matched; an error means the lane could
    /// not look, which the launcher says in its place.
    fn find(&self, query: &str, cancel: &Cancel) -> Result<Vec<Item>, String>;
}
