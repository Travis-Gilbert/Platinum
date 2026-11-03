//! Browser data is partitioned by profile; source profiles are never modified.
pub mod credentials;
pub mod migration;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
mod legacy;
mod storage;
use std::time::{SystemTime, UNIX_EPOCH};
pub use storage::ImportBatch;
use url::Url;

/// The app's name, which its folders take.
pub const APP_NAME: &str = "Platinum";
/// What the app and its folders were called before.
const LEGACY_NAME: &str = "Frame";

/// The app's folder under `base` (the data or cache directory): `Platinum`,
/// taking over the `Frame` folder the app kept before it was renamed, so its
/// profile, index and downloaded model carry over. A `Frame` link is left in
/// its place, since builds of branches from before the rename still open it.
/// The app and `profile-import` both find their folder here.
pub fn app_dir(base: &Path) -> PathBuf {
    let dir = base.join(APP_NAME);
    let legacy = base.join(LEGACY_NAME);
    if !dir.exists() && legacy.is_dir() {
        let moved = std::fs::rename(&legacy, &dir)
            .and_then(|()| std::os::unix::fs::symlink(APP_NAME, &legacy));
        if let Err(error) = moved {
            log::warn!(
                "could not move {} to {}: {error}",
                legacy.display(),
                dir.display()
            );
        }
    }
    dir
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub id: i64,
    pub name: String,
    pub email: String,
}

#[derive(Clone)]
pub struct ProfileStore {
    pub root: PathBuf,
    pub profile: Profile,
    database: Arc<Mutex<rustyred::Database>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub url: String,
    pub title: String,
    pub folder: String,
}

/// One navigation, even when its URL repeats. Previews own their encoded
/// pixels; they never refer to a recyclable engine surface. Records are
/// durable user history and are paged, not silently cache-evicted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Visit {
    pub id: i64,
    pub tab: String,
    pub predecessor: Option<i64>,
    pub url: String,
    pub title: String,
    pub visited: i64,
    /// Kept beside the database as a file, so the graph holds only records.
    #[serde(skip)]
    pub preview: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SessionTab {
    pub url: Option<String>,
    pub query: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Session {
    #[serde(default)]
    pub layout: Option<serde_json::Value>,
    pub tabs: Vec<SessionTab>,
    pub active: usize,
}

pub fn web_url(value: &str) -> Option<Url> {
    let url = Url::parse(value).ok()?;
    (matches!(url.scheme(), "https" | "http")
        && url.has_host()
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(url)
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .min(i64::MAX as u128) as i64
}
