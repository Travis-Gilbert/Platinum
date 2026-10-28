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
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::{App, Global, RenderImage};
use serde_json::Value;

pub use self::format::{Content, Pin, SavedTab, Session};

const PINS: &str = "pins.json";
const SESSION: &str = "session.json";
const FAVICONS: &str = "favicons";

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Global for Store {}

/// Keep Frame's state under `data_dir`.
pub fn init(data_dir: &Path, cx: &mut App) {
    cx.set_global(Store {
        dir: data_dir.to_owned(),
    });
}

impl Store {
    pub fn global(cx: &App) -> Option<Self> {
        cx.try_global::<Self>().cloned()
    }

    pub fn load_pins(&self) -> Vec<Pin> {
        let path = self.dir.join(PINS);
        let Some(value) = read_json(&path) else {
            return Vec::new();
        };
        format::pins_from_json(&value).unwrap_or_else(|| {
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

    pub fn save_session(&self, session: &Session) {
        write_json(&self.dir.join(SESSION), &format::session_to_json(session));
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
