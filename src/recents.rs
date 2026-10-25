//! What Home lists under Recent: the last pages the person opened and the
//! last searches they ran, newest first. It is Frame's own short list for
//! getting back to things, kept in `recents.json` in Frame's data folder, not
//! a history: RustyWeb's store keeps the pages themselves (`search::capture`)
//! and has no recency listing to read back.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{App, AppContext as _, Global, Task};
use serde::{Deserialize, Serialize};
use url::Url;

/// How many of each Home lists.
const KEEP: usize = 8;

/// A burst of changes, such as a page's title settling, saves once.
const SAVE_AFTER: Duration = Duration::from_millis(300);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentPage {
    pub url: Url,
    /// Empty when the page had none; Home shows the host instead.
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentSearch {
    pub query: String,
    pub ask: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Record {
    #[serde(default)]
    pages: Vec<RecentPage>,
    #[serde(default)]
    searches: Vec<RecentSearch>,
}

pub struct Recents {
    record: Record,
    /// Where the record is saved; `None` keeps it in memory, as tests do.
    path: Option<PathBuf>,
    _save: Task<()>,
}

impl Global for Recents {}

/// Load the record at `path`, if there is one, and install it.
pub fn init(path: Option<PathBuf>, cx: &mut App) {
    let record = path.as_deref().map(load).unwrap_or_default();
    cx.set_global(Recents {
        record,
        path,
        _save: Task::ready(()),
    });
}

pub fn pages(cx: &App) -> &[RecentPage] {
    cx.try_global::<Recents>()
        .map_or(&[], |recents| &recents.record.pages)
}

pub fn searches(cx: &App) -> &[RecentSearch] {
    cx.try_global::<Recents>()
        .map_or(&[], |recents| &recents.record.searches)
}

/// A web page finished loading. Files and `about:` pages are not kept.
pub fn record_page(url: &Url, title: &str, cx: &mut App) {
    if !crate::search::capturable(url) {
        return;
    }
    let page = RecentPage {
        url: url.clone(),
        title: title.trim().to_string(),
    };
    update(cx, |record| {
        remember(&mut record.pages, page, |a, b| a.url == b.url)
    });
}

pub fn record_search(query: &str, ask: bool, cx: &mut App) {
    let query = query.trim();
    if query.is_empty() {
        return;
    }
    let search = RecentSearch {
        query: query.to_string(),
        ask,
    };
    update(cx, |record| remember(&mut record.searches, search, |a, b| a == b));
}

pub fn clear(cx: &mut App) {
    update(cx, |record| std::mem::take(record) != Record::default());
}

/// Put `item` first, dropping an earlier copy and anything past [`KEEP`].
/// Whether the list changed.
fn remember<T: PartialEq>(list: &mut Vec<T>, item: T, same: impl Fn(&T, &T) -> bool) -> bool {
    if list.first() == Some(&item) {
        return false;
    }
    list.retain(|old| !same(old, &item));
    list.insert(0, item);
    list.truncate(KEEP);
    true
}

fn update(cx: &mut App, change: impl FnOnce(&mut Record) -> bool) {
    let Some(recents) = cx.try_global::<Recents>() else {
        return;
    };
    let mut record = recents.record.clone();
    if !change(&mut record) {
        return;
    }
    let path = recents.path.clone();
    let save = match path {
        Some(path) => {
            let saved = record.clone();
            let timer = cx.background_executor().timer(SAVE_AFTER);
            cx.background_spawn(async move {
                timer.await;
                if let Err(error) = save(&path, &saved) {
                    log::warn!("recents: could not save {}: {error:#}", path.display());
                }
            })
        }
        None => Task::ready(()),
    };
    // Replacing the pending save cancels it: only the newest record is
    // written. Observers (Home) redraw from here.
    let recents = cx.global_mut::<Recents>();
    recents.record = record;
    recents._save = save;
}

fn load(path: &Path) -> Record {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Record::default(),
        Err(error) => {
            log::warn!("recents: could not read {}: {error}", path.display());
            return Record::default();
        }
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        log::warn!("recents: {} is not readable, starting over: {error}", path.display());
        Record::default()
    })
}

/// Written beside the record, then moved over it, so a crash mid-write
/// leaves the previous record whole.
fn save(path: &Path, record: &Record) -> anyhow::Result<()> {
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, serde_json::to_vec_pretty(record)?)?;
    std::fs::rename(&partial, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembering_moves_a_repeat_to_the_front() {
        let mut list = vec![1, 2, 3];
        assert!(remember(&mut list, 3, |a, b| a == b));
        assert_eq!(list, [3, 1, 2]);
        assert!(!remember(&mut list, 3, |a, b| a == b));
    }

    #[test]
    fn remembering_keeps_only_the_newest() {
        let mut list = Vec::new();
        for n in 0..KEEP + 3 {
            remember(&mut list, n, |a, b| a == b);
        }
        assert_eq!(list.len(), KEEP);
        assert_eq!(list[0], KEEP + 2);
    }

    #[test]
    fn a_page_is_the_same_page_whatever_its_title() {
        let url = Url::parse("https://example.com/").unwrap();
        let page = |title: &str| RecentPage {
            url: url.clone(),
            title: title.into(),
        };
        let mut list = vec![page("Loading")];
        assert!(remember(&mut list, page("Example"), |a, b| a.url == b.url));
        assert_eq!(list, [page("Example")]);
    }

    #[test]
    fn the_record_survives_a_round_trip_and_a_missing_file() {
        let folder = std::env::temp_dir().join(format!("frame-recents-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("recents.json");
        assert_eq!(load(&path), Record::default());
        let record = Record {
            pages: vec![RecentPage {
                url: Url::parse("https://example.com/a").unwrap(),
                title: "A".into(),
            }],
            searches: vec![RecentSearch {
                query: "servo".into(),
                ask: true,
            }],
        };
        save(&path, &record).unwrap();
        assert_eq!(load(&path), record);
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(load(&path), Record::default());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
