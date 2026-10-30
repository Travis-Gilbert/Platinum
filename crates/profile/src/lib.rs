//! Browser data is partitioned by profile; source profiles are never modified.
pub mod credentials;
pub mod migration;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
mod legacy;
mod storage;
use std::time::{SystemTime, UNIX_EPOCH};
pub use storage::ImportBatch;
use url::Url;

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
