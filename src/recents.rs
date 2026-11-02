//! What Home lists under Continue: the last pages the person opened and the
//! last searches they ran, newest first. It is Frame's own short list for
//! getting back to things, kept in `recents.json` in Frame's data folder, not
//! a history: RustyWeb's store keeps the pages themselves (`search::capture`)
//! and has no recency listing to read back.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use gpui_kit::{App, AppContext as _, Global, Subscription, Task};
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
    /// When it was opened, in seconds since 1970; 0 in a record from before
    /// Frame kept the time.
    #[serde(default)]
    pub at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentSearch {
    pub query: String,
    pub ask: bool,
    /// When it was run, as [`RecentPage::at`].
    #[serde(default)]
    pub at: u64,
}

/// A page or a search, for one list of both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Recent<'a> {
    Page(&'a RecentPage),
    Search(&'a RecentSearch),
}

impl Recent<'_> {
    fn at(self) -> u64 {
        match self {
            Recent::Page(page) => page.at,
            Recent::Search(search) => search.at,
        }
    }
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
    generation: u64,
    written: Arc<Mutex<u64>>,
    _quit: Subscription,
}

impl Global for Recents {}

/// Load the record at `path`, if there is one, and install it.
pub fn init(path: Option<PathBuf>, cx: &mut App) {
    let record = path.as_deref().map(load).unwrap_or_default();
    let quit = cx.on_app_quit(|cx| {
        let recents = cx.global::<Recents>();
        let path = recents.path.clone();
        let record = recents.record.clone();
        let generation = recents.generation;
        let written = recents.written.clone();
        cx.background_spawn(async move {
            if let Some(path) = path {
                persist(&path, &record, generation, &written);
            }
        })
    });
    cx.set_global(Recents {
        record,
        path,
        _save: Task::ready(()),
        generation: 0,
        written: Arc::default(),
        _quit: quit,
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

/// The newest `count` pages and searches together, newest first. Those from
/// before Frame kept the time come last, pages before searches.
pub fn latest(count: usize, cx: &App) -> Vec<Recent<'_>> {
    merge(pages(cx), searches(cx), count)
}

fn merge<'a>(
    pages: &'a [RecentPage],
    searches: &'a [RecentSearch],
    count: usize,
) -> Vec<Recent<'a>> {
    let mut all: Vec<Recent> = pages
        .iter()
        .map(Recent::Page)
        .chain(searches.iter().map(Recent::Search))
        .collect();
    // Stable: each list is already newest first.
    all.sort_by_key(|recent| std::cmp::Reverse(recent.at()));
    all.truncate(count);
    all
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// A web page finished loading. Files and `about:` pages are not kept.
pub fn record_page(url: &Url, title: &str, cx: &mut App) {
    if !crate::search::capturable(url) {
        return;
    }
    let page = RecentPage {
        url: url.clone(),
        title: title.trim().to_string(),
        at: now(),
    };
    update(cx, |record| {
        remember(&mut record.pages, page, |a, b| a.url == b.url)
    });
}

/// A title change is not a visit and cannot restore a cleared entry.
pub fn update_title(url: &Url, title: &str, cx: &mut App) {
    update(cx, |record| {
        let Some(page) = record.pages.iter_mut().find(|page| page.url == *url) else {
            return false;
        };
        let title = title.trim();
        if page.title == title {
            return false;
        }
        page.title = title.to_owned();
        true
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
        at: now(),
    };
    update(cx, |record| {
        remember(&mut record.searches, search, |a, b| {
            a.query == b.query && a.ask == b.ask
        })
    });
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
    let generation = recents.generation + 1;
    let written = recents.written.clone();
    let save = match path {
        Some(path) => {
            let saved = record.clone();
            let timer = cx.background_executor().timer(SAVE_AFTER);
            cx.background_spawn(async move {
                timer.await;
                persist(&path, &saved, generation, &written);
            })
        }
        None => Task::ready(()),
    };
    // Cancel the debounce; any write already running is serialized by
    // persist, following session::Store’s ordered snapshot protocol.
    let recents = cx.global_mut::<Recents>();
    recents.record = record;
    recents.generation = generation;
    recents._save = save;
}

/// Serialize writes and reject a delayed snapshot older than the file.
fn persist(path: &Path, record: &Record, generation: u64, written: &Mutex<u64>) {
    let mut written = written
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if generation <= *written {
        return;
    }
    match save(path, record) {
        Ok(()) => *written = generation,
        Err(error) => log::warn!("recents: could not save {}: {error:#}", path.display()),
    }
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
        log::warn!(
            "recents: {} is not readable, starting over: {error}",
            path.display()
        );
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

    #[gpui_kit::test]
    fn title_changes_preserve_recency_and_do_not_restore_cleared_pages(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        cx.update(|cx| {
            init(None, cx);
            let url = Url::parse("https://example.com/").unwrap();
            record_page(&url, "Before", cx);
            cx.global_mut::<Recents>().record.pages[0].at = 1;
            record_search("newer search", false, cx);
            update_title(&url, "After", cx);
            assert_eq!(pages(cx)[0].at, 1);
            assert_eq!(pages(cx)[0].title, "After");
            assert!(matches!(latest(2, cx)[0], Recent::Search(_)));
            clear(cx);
            update_title(&url, "Late title", cx);
            assert!(pages(cx).is_empty());
        });
    }

    #[test]
    fn a_delayed_snapshot_cannot_overwrite_a_newer_clear() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("recents.json");
        let written = Mutex::new(0);
        let old = Record {
            searches: vec![RecentSearch {
                query: "old".into(),
                ask: false,
                at: 1,
            }],
            ..Record::default()
        };
        persist(&path, &Record::default(), 2, &written);
        persist(&path, &old, 1, &written);
        assert_eq!(load(&path), Record::default());
    }

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
            at: 0,
        };
        let mut list = vec![page("Loading")];
        assert!(remember(&mut list, page("Example"), |a, b| a.url == b.url));
        assert_eq!(list, [page("Example")]);
    }

    #[test]
    fn pages_and_searches_come_newest_first_together() {
        let page = |path: &str, at| RecentPage {
            url: Url::parse(&format!("https://example.com/{path}")).unwrap(),
            title: String::new(),
            at,
        };
        let search = |query: &str, at| RecentSearch {
            query: query.into(),
            ask: false,
            at,
        };
        // Newest first in each, as the record keeps them; the old ones have no time.
        let pages = [page("b", 30), page("a", 10), page("old", 0)];
        let searches = [search("y", 20), search("old", 0)];
        let order: Vec<String> = merge(&pages, &searches, 4)
            .into_iter()
            .map(|recent| match recent {
                Recent::Page(page) => page.url.path().to_owned(),
                Recent::Search(search) => search.query.clone(),
            })
            .collect();
        assert_eq!(order, ["/b", "y", "/a", "/old"]);
    }

    #[test]
    fn a_record_from_before_times_still_reads() {
        let record: Record = serde_json::from_str(
            r#"{"pages":[{"url":"https://example.com/","title":"E"}],"searches":[{"query":"q","ask":false}]}"#,
        )
        .unwrap();
        assert_eq!(record.pages[0].at, 0);
        assert_eq!(record.searches[0].at, 0);
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
                at: 1,
            }],
            searches: vec![RecentSearch {
                query: "servo".into(),
                ask: true,
                at: 2,
            }],
        };
        save(&path, &record).unwrap();
        assert_eq!(load(&path), record);
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(load(&path), Record::default());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
