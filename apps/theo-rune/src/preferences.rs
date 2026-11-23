//! What Theo remembers between launches: the form the person left it in,
//! where each form sat, and unsaved drafts with the resource they belong to.
//!
//! The store is Rune's `Prefs` (JSON in the platform config directory,
//! written whole through a rename). It is kept apart from the GPUI desk's
//! preferences during the transition. One thread owns the file: the UI sends
//! it snapshots and never waits on the disk, and a burst of snapshots is
//! written once.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use rune::prefs::Prefs;
use serde::{Deserialize, Serialize};

use crate::presentation::{Form, Frame};
use crate::surfaces::Draft;

pub const APP_ID: &str = "com.theorem.theo";
const KEY: &str = "theo";

/// The remembered state, as one value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default)]
    pub form: Form,
    #[serde(default)]
    pub presence: Option<Frame>,
    #[serde(default)]
    pub workspace: Option<Frame>,
    #[serde(default)]
    pub drafts: Vec<Draft>,
}

/// Reads what was saved, or the defaults when nothing was or it does not
/// parse (Rune moves an unreadable file aside).
pub fn load(prefs: &Prefs) -> Saved {
    prefs.get(KEY).unwrap_or_default()
}

/// The single writer of the preferences file.
pub struct Writer {
    send: Sender<Saved>,
    last: Option<Saved>,
}

impl Writer {
    /// Starts the writing thread over `prefs`.
    pub fn spawn(prefs: Prefs) -> Self {
        let (send, receive) = mpsc::channel();
        thread::Builder::new()
            .name("theo-preferences".into())
            .spawn(move || write_all(prefs, receive))
            .expect("the preferences thread starts");
        Self { send, last: None }
    }

    /// Queues `saved` to be written, unless it is what was written last.
    pub fn store(&mut self, saved: Saved) {
        if self.last.as_ref() == Some(&saved) {
            return;
        }
        self.last = Some(saved.clone());
        // The thread outlives every sender; a send fails only at exit.
        let _ = self.send.send(saved);
    }
}

fn write_all(mut prefs: Prefs, receive: Receiver<Saved>) {
    while let Ok(mut saved) = receive.recv() {
        // Only the newest of a burst matters.
        while let Ok(newer) = receive.try_recv() {
            saved = newer;
        }
        if let Err(error) = prefs.set(KEY, &saved) {
            eprintln!("theo: preferences not written: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::SurfaceRef;

    #[test]
    fn what_is_saved_reads_back() {
        let dir = std::env::temp_dir().join(format!("theo-prefs-{}", std::process::id()));
        let path = dir.join("prefs.json");
        let saved = Saved {
            form: Form::Workspace,
            presence: Some(Frame {
                x: 1,
                y: 2,
                w: 264.0,
                h: 132.0,
            }),
            workspace: None,
            drafts: vec![Draft {
                reference: SurfaceRef::new("conversation", "new"),
                text: "unsent".into(),
            }],
        };
        let mut writer = Writer::spawn(Prefs::at(&path));
        writer.store(saved.clone());
        drop(writer);
        // The thread ends once the sender is gone; wait for the file.
        for _ in 0..200 {
            if load(&Prefs::at(&path)) == saved {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(load(&Prefs::at(&path)), saved);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_empty_store_gives_the_defaults() {
        assert_eq!(load(&Prefs::memory()), Saved::default());
    }
}
