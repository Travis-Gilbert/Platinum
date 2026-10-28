//! What Frame keeps between launches, in its data directory: the pinned
//! sites (`pins.json`), the tabs with their tree (`session.json`), and the
//! icons of the sites those name (`favicons/`), so a pin or a restored tab
//! shows its site's icon before its page has loaded.
//!
//! Each file is written whole, through a temporary file, once changes
//! settle, and read once at launch. A pins file Frame cannot read is set
//! aside rather than written over. Icons are kept only for the sites the
//! pins and the session name, so the folder is not a record of browsing.
//!
//! Without the `Store` global (the UI tests) nothing is read or written.

mod format;

use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use gpui_kit::{App, Global, RenderImage};
use serde_json::Value;

pub use self::format::{Content, Layout, Pin, SavedTab, Session};

const PINS: &str = "pins.json";
const SESSION: &str = "session.json";
const FAVICONS: &str = "favicons";

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
    /// How many snapshots have been gathered.
    gathered: Arc<AtomicU64>,
    /// The newest snapshot written, by when it was gathered; writes take
    /// turns holding it.
    written: Arc<Mutex<u64>>,
}

impl Global for Store {}

/// What one save writes: the session, and the icons of the sites it and the
/// pins name, gathered on the UI thread.
pub struct Snapshot {
    /// Its place among the snapshots gathered, from 1.
    order: u64,
    session: Session,
    icons: Vec<(String, Arc<RenderImage>)>,
    /// Every site whose icon is kept; the others' are forgotten.
    hosts: HashSet<String>,
}

/// Keep Frame's state under `data_dir`.
pub fn init(data_dir: &Path, cx: &mut App) {
    cx.set_global(Store::new(data_dir.to_owned()));
}

impl Store {
    fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            gathered: Arc::default(),
            written: Arc::default(),
        }
    }

    pub fn global(cx: &App) -> Option<Self> {
        cx.try_global::<Self>().cloned()
    }

    /// The pins kept, or none. A file that is there but is not pins Frame
    /// can read, broken JSON included, is set aside before the next save
    /// could write over it.
    pub fn load_pins(&self) -> Vec<Pin> {
        let path = self.dir.join(PINS);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Vec::new(),
            Err(error) => {
                log::warn!("reading {}: {error}", path.display());
                String::new()
            }
        };
        let pins = serde_json::from_str(&text)
            .ok()
            .and_then(|value| format::pins_from_json(&value));
        pins.unwrap_or_else(|| {
            let aside = path.with_extension("json.unreadable");
            log::warn!(
                "{} is not Frame's; moved to {}",
                path.display(),
                aside.display()
            );
            let _ = fs::rename(&path, aside);
            Vec::new()
        })
    }

    pub fn save_pins(&self, pins: &[Pin]) {
        write_json(&self.dir.join(PINS), &format::pins_to_json(pins));
    }

    pub fn load_session(&self) -> Option<Session> {
        format::session_from_json(&read_json(&self.dir.join(SESSION))?)
    }

    /// What one save writes, in its place among the others. Gather on the
    /// UI thread, where the tabs change, so the order is theirs.
    pub fn snapshot(
        &self,
        session: Session,
        icons: Vec<(String, Arc<RenderImage>)>,
        hosts: HashSet<String>,
    ) -> Snapshot {
        Snapshot {
            order: self.gathered.fetch_add(1, Ordering::Relaxed) + 1,
            session,
            icons,
            hosts,
        }
    }

    /// Write what one save gathered. Off the UI thread as a rule, so a save
    /// at quit can overlap one from before, or one from before can reach
    /// its turn only after it: writes take turns, and one older than what
    /// is written already is dropped.
    pub fn save(&self, snapshot: &Snapshot) {
        let mut written = self.written.lock().unwrap_or_else(PoisonError::into_inner);
        if snapshot.order <= *written {
            return;
        }
        write_json(
            &self.dir.join(SESSION),
            &format::session_to_json(&snapshot.session),
        );
        for (host, icon) in &snapshot.icons {
            self.save_favicon(host, icon);
        }
        self.keep_favicons(&snapshot.hosts);
        *written = snapshot.order;
    }

    pub fn load_favicon(&self, host: &str) -> Option<Arc<RenderImage>> {
        let bytes = fs::read(self.icon_path(host)).ok()?;
        let (width, height, pixels) = format::decode_icon(&bytes)?;
        let frame = image::RgbaImage::from_raw(width, height, pixels.to_vec())?;
        Some(crate::page::icon_image(frame))
    }

    /// Keep `icon` for `host`, unless one is kept already.
    pub fn save_favicon(&self, host: &str, icon: &RenderImage) {
        let path = self.icon_path(host);
        if path.exists() {
            return;
        }
        let Some(pixels) = icon.as_bytes(0) else {
            return;
        };
        let size = icon.size(0);
        let bytes = format::encode_icon(size.width.0 as u32, size.height.0 as u32, pixels);
        if let Err(error) =
            fs::create_dir_all(self.dir.join(FAVICONS)).and_then(|()| fs::write(&path, bytes))
        {
            log::warn!("keeping the icon for {host}: {error}");
        }
    }

    /// Forget the icons of every site but `hosts`.
    pub fn keep_favicons(&self, hosts: &HashSet<String>) {
        let keep: HashSet<String> = hosts
            .iter()
            .map(|host| format::icon_file_name(host))
            .collect();
        let Ok(entries) = fs::read_dir(self.dir.join(FAVICONS)) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !keep.contains(name.to_string_lossy().as_ref()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    fn icon_path(&self, host: &str) -> PathBuf {
        self.dir.join(FAVICONS).join(format::icon_file_name(host))
    }
}

/// A session file's text, as Frame reads it at launch.
#[cfg(test)]
pub fn parse_session(text: &str) -> Option<Session> {
    format::session_from_json(&serde_json::from_str(text).ok()?)
}

fn read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(value) => Some(value),
        Err(error) => {
            log::warn!("reading {}: {error}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(test: &str) -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!("frame-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        (Store::new(dir.clone()), dir)
    }

    fn session(sidebar_width: f32) -> Session {
        Session {
            sidebar: false,
            sidebar_width: Some(sidebar_width),
            active: None,
            tabs: Vec::new(),
            dock: Layout::default(),
        }
    }

    #[test]
    fn a_save_older_than_the_one_written_is_dropped() {
        let (store, dir) = store("save-order");
        let older = store.snapshot(session(200.), Vec::new(), HashSet::new());
        let newer = store.snapshot(session(300.), Vec::new(), HashSet::new());
        // The save at quit finishes first; the one from before reaches its
        // turn after.
        store.save(&newer);
        store.save(&older);
        let written = read_json(&dir.join(SESSION)).unwrap();
        assert_eq!(written["sidebar_width"], 300.);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_pins_file_is_set_aside_before_it_can_be_written_over() {
        let (store, dir) = store("pins");
        assert!(store.load_pins().is_empty(), "no file, no pins");
        assert!(!dir.join("pins.json.unreadable").exists());

        let broken = r#"[{"id": 1, "url": "https://example.org/","#;
        fs::write(dir.join(PINS), broken).unwrap();
        assert!(store.load_pins().is_empty());
        assert!(!dir.join(PINS).exists());
        let aside = fs::read_to_string(dir.join("pins.json.unreadable")).unwrap();
        assert_eq!(aside, broken);
        let _ = fs::remove_dir_all(&dir);
    }
}

/// Write through a temporary file, so a crash mid-write leaves the old file.
fn write_json(path: &Path, value: &Value) {
    let temporary = path.with_extension("json.tmp");
    let result = serde_json::to_vec_pretty(value)
        .map_err(std::io::Error::other)
        .and_then(|bytes| fs::write(&temporary, bytes))
        .and_then(|()| fs::rename(&temporary, path));
    if let Err(error) = result {
        log::warn!("writing {}: {error}", path.display());
    }
}
