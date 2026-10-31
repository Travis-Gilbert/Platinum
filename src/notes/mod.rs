//! The notes app inside Platinum: a vault of Markdown notes, read in Servo.
//!
//! - `host` is the Servo side: the `frame-notes:` protocol, the reading page's
//!   conversation with the host, and the vault's single owning thread.
//! - `panel` is the reading panel, a dock panel that hosts a web view of one
//!   note.
//! - `open` is the entry point: launch arguments, and the window that opens
//!   a vault.
//!
//! An `editor` submodule (the text editor for a note) belongs beside `panel`;
//! it shares `host::service::Service`, which is how every view reaches a
//! vault, and does not touch the reading page.

pub mod host;
pub mod open;
pub mod panel;

use std::path::{Path, PathBuf};

use gpui_kit::{App, Global};

use crate::engine::Engine;
use host::service::{Service, Vaults, host_of};

/// The vaults that are open, and where their derived data lives.
pub struct Notes {
    vaults: Vaults,
    data: PathBuf,
}

impl Global for Notes {}

impl Notes {
    /// The vault at `root`, opened if it is not open already. Its derived
    /// state (index, journal, the person's choices) is kept in the profile,
    /// never in the vault.
    pub fn open(&self, root: &Path) -> Service {
        let data = self.data.join(host_of(root));
        if let Err(error) = std::fs::create_dir_all(&data) {
            log::warn!(target: "notes", "{}: {error}", data.display());
        }
        self.vaults.open(root, &data)
    }

    /// The registry the vaults' pages are served from, which also carries the
    /// app's theme to them.
    pub fn vaults(&self) -> &Vaults {
        &self.vaults
    }
}

/// Starts Servo with the `frame-notes:` pages registered, in place of
/// `Engine::start`.
pub fn start(profile_dir: &Path, cx: &mut App) {
    let vaults = Vaults::default();
    Engine::start_with_protocols(profile_dir, host::protocols(&vaults), cx);
    cx.set_global(Notes {
        vaults,
        data: profile_dir.join("notes"),
    });
}
