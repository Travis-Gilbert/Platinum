//! Where behaviors are kept, on disk, so they outlive the session:
//!
//! ```text
//! behaviors/
//!   wave.luau             the inbox: an author writes here to define or revise
//!   wave/1.luau           every revision's exact source
//!   wave/2.luau
//!   wave/revisions.json   who wrote each, for which runtime, and what became of it
//! ```
//!
//! One thread writes; the session hands it the catalog's view of a behavior
//! and it makes the files match, so a save repeated after a crash ends the
//! same. Another thread watches the inbox and hands back what changed.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, SystemTime};

use super::{Catalog, Name, Revision};

const MANIFEST: &str = "revisions.json";
/// How often the inbox is looked at.
const INBOX_EVERY: Duration = Duration::from_millis(500);

/// The behaviors folder: `THEO_BEHAVIORS_DIR`, else the application's
/// support folder.
pub fn default_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("THEO_BEHAVIORS_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME").map_or_else(|| home.join(".local/share"), PathBuf::from)
    };
    base.join(crate::preferences::APP_ID).join("behaviors")
}

/// Reads every behavior's history. A behavior folder that cannot be read is
/// passed over, and says so, rather than stopping the rest.
pub fn load(dir: &Path) -> Catalog {
    let mut catalog = Catalog::default();
    let Ok(entries) = fs::read_dir(dir) else {
        return catalog;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| Name::parse(n).ok())
        else {
            continue;
        };
        match load_one(&path) {
            Ok(revisions) => catalog.restore(name, revisions),
            Err(e) => eprintln!("theo: behavior {} not loaded: {e}", path.display()),
        }
    }
    catalog
}

fn load_one(path: &Path) -> Result<Vec<Revision>, String> {
    let manifest = fs::read_to_string(path.join(MANIFEST)).map_err(|e| e.to_string())?;
    let mut revisions: Vec<Revision> =
        serde_json::from_str(&manifest).map_err(|e| e.to_string())?;
    for r in &mut revisions {
        r.source = fs::read_to_string(path.join(format!("{}.luau", r.number)))
            .map_err(|e| e.to_string())?;
    }
    Ok(revisions)
}

/// Makes the files for `name` match `revisions`.
pub fn save(dir: &Path, name: &Name, revisions: &[Revision]) -> std::io::Result<()> {
    let folder = dir.join(name.as_str());
    fs::create_dir_all(&folder)?;
    for r in revisions {
        let file = folder.join(format!("{}.luau", r.number));
        if !file.exists() {
            write_atomically(&file, r.source.as_bytes())?;
        }
    }
    let manifest = serde_json::to_vec_pretty(revisions).map_err(std::io::Error::other)?;
    write_atomically(&folder.join(MANIFEST), &manifest)
}

fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes)?;
    fs::rename(temp, path)
}

/// The one writer of the behaviors folder.
pub struct Writer {
    send: Sender<(Name, Vec<Revision>)>,
}

impl Writer {
    pub fn spawn(dir: PathBuf) -> Writer {
        let (send, receive) = mpsc::channel::<(Name, Vec<Revision>)>();
        thread::Builder::new()
            .name("theo-behaviors".into())
            .spawn(move || {
                while let Ok((name, revisions)) = receive.recv() {
                    if let Err(e) = save(&dir, &name, &revisions) {
                        eprintln!("theo: behavior {} not saved: {e}", name.as_str());
                    }
                }
            })
            .expect("spawn the behaviors writer");
        Writer { send }
    }

    pub fn store(&self, name: &Name, catalog: &Catalog) {
        let _ = self
            .send
            .send((name.clone(), catalog.revisions(name).to_vec()));
    }
}

/// The behaviors folder as the application keeps it: its writer, and what
/// authors leave in its inbox.
pub struct Library {
    pub writer: Writer,
    pub inbox: Receiver<Authored>,
}

impl Library {
    pub fn open(dir: PathBuf) -> Library {
        Library {
            writer: Writer::spawn(dir.clone()),
            inbox: watch_inbox(dir),
        }
    }
}

/// Source an author left in the inbox.
#[derive(Clone, Debug, PartialEq)]
pub struct Authored {
    pub name: Name,
    pub source: String,
    pub by: String,
}

/// Watches the inbox and hands back each file that is new or changed.
pub fn watch_inbox(dir: PathBuf) -> Receiver<Authored> {
    let (send, receive) = mpsc::channel();
    thread::Builder::new()
        .name("theo-behavior-inbox".into())
        .spawn(move || {
            let mut seen = HashMap::new();
            loop {
                for authored in scan(&dir, &mut seen) {
                    if send.send(authored).is_err() {
                        return;
                    }
                }
                thread::sleep(INBOX_EVERY);
            }
        })
        .expect("spawn the behavior inbox");
    receive
}

/// The inbox files that changed since `seen` was last updated.
pub fn scan(dir: &Path, seen: &mut HashMap<PathBuf, (SystemTime, u64)>) -> Vec<Authored> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut changed = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("luau") {
            continue;
        }
        let Some(name) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| Name::parse(s).ok())
        else {
            continue;
        };
        let Ok(meta) = entry.metadata() else { continue };
        let stamp = (
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
        );
        if seen.get(&path) == Some(&stamp) {
            continue;
        }
        if let Ok(source) = fs::read_to_string(&path) {
            seen.insert(path.clone(), stamp);
            changed.push(Authored {
                name,
                source,
                by: format!("file:{}", path.display()),
            });
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("theo-behaviors-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn every_revision_and_its_outcome_survive_a_restart() {
        let dir = temp("restart");
        let wave = Name::parse("wave").unwrap();
        let mut catalog = Catalog::default();
        let one = catalog.author(&wave, "return {}", "agent");
        catalog.outcome(&wave, one, Ok(()));
        let two = catalog.author(&wave, "return {", "agent");
        catalog.outcome(&wave, two, Err("expected '}'".into()));
        save(&dir, &wave, catalog.revisions(&wave)).unwrap();
        save(&dir, &wave, catalog.revisions(&wave)).unwrap();

        let loaded = load(&dir);
        assert_eq!(loaded, catalog);
        assert_eq!(
            loaded.active(&wave).unwrap().source,
            "return {}",
            "the good one runs again"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_inbox_reports_a_file_once_and_again_when_it_changes() {
        let dir = temp("inbox");
        let mut seen = HashMap::new();
        fs::write(dir.join("glow.luau"), "return {}").unwrap();
        fs::write(dir.join("Not A Name.luau"), "return {}").unwrap();
        fs::write(dir.join("notes.txt"), "x").unwrap();
        let first = scan(&dir, &mut seen);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].name.as_str(), "glow");
        assert!(scan(&dir, &mut seen).is_empty());
        fs::write(dir.join("glow.luau"), "return { revised = true }").unwrap();
        let again = scan(&dir, &mut seen);
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].source, "return { revised = true }");
        let _ = fs::remove_dir_all(&dir);
    }
}
