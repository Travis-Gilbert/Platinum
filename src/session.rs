//! What Platinum keeps between launches, in its data directory: the pinned
//! sites (`pins.json`), the tabs with their tree (`session.json`), and the
//! icons of the sites those name (`favicons/`), so a pin or a restored tab
//! shows its site's icon before its page has loaded.
//!
//! Each file is written whole, through a temporary file, a moment after a
//! change, and read once at launch. A file Platinum cannot make sense of is set
//! aside rather than written over, and one it cannot read at all is left
//! alone. Icons are kept only for the sites the pins and the session name,
//! so the folder is not a record of browsing.
//!
//! Without the `Store` global (the UI tests) nothing is read or written.

#[path = "session/format.rs"]
pub(crate) mod format;

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
    /// Files there at launch that could not be read then, left as they are
    /// rather than written over.
    unread: Arc<Mutex<HashSet<&'static str>>>,
}

impl Global for Store {}

/// What one save writes: the session, the pins, and the icons of the sites
/// those name, gathered on the UI thread.
pub struct Snapshot {
    /// Its place among the snapshots gathered, from 1.
    order: u64,
    session: Session,
    pins: Vec<Pin>,
    icons: Vec<(String, Arc<RenderImage>)>,
    /// Every site whose icon is kept; the others' are forgotten.
    hosts: HashSet<String>,
}

/// Keep Platinum's state under `data_dir`.
pub fn init(data_dir: &Path, cx: &mut App) {
    cx.set_global(Store::new(data_dir.to_owned()));
}

/// Move what Platinum kept in `from`, the data folder itself, before each
/// profile had its own, into `into`, the default profile's folder. A file
/// already in `into` is kept; the older one stays where it was.
pub fn adopt(from: &Path, into: &Path) {
    for name in [SESSION, PINS, FAVICONS, "recents.json"] {
        let (old, new) = (from.join(name), into.join(name));
        if !old.exists() || new.exists() {
            continue;
        }
        if let Err(error) = fs::create_dir_all(into).and_then(|()| fs::rename(&old, &new)) {
            log::warn!("moving {} to {}: {error}", old.display(), new.display());
        }
    }
}

/// Open the tabs an import left in `profile`: they join the saved session,
/// and the profile lets go of them once they are in it, so they open once.
pub fn open_imported_tabs(profile: &frame_profile::ProfileStore, cx: &App) {
    let Some(store) = Store::global(cx) else {
        return;
    };
    let imported = match profile.session() {
        Ok(imported) if !imported.tabs.is_empty() => imported,
        Ok(_) => return,
        Err(error) => {
            log::warn!("reading the imported tabs: {error}");
            return;
        }
    };
    let contents = imported
        .tabs
        .into_iter()
        .filter_map(|tab| match (tab.url, tab.query) {
            (Some(url), _) => url::Url::parse(&url).ok().map(|url| Content::Page {
                url,
                title: String::new(),
            }),
            (None, Some(query)) => Some(Content::Search { query, ask: false }),
            (None, None) => None,
        });
    if store.add_tabs(contents)
        && let Err(error) = profile.save_session(&frame_profile::Session::default())
    {
        log::warn!("clearing the imported tabs: {error}");
    }
}

impl Store {
    fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            gathered: Arc::default(),
            written: Arc::default(),
            unread: Arc::default(),
        }
    }

    pub fn global(cx: &App) -> Option<Self> {
        cx.try_global::<Self>().cloned()
    }

    /// The pins kept, or none.
    pub fn load_pins(&self) -> Vec<Pin> {
        self.load(PINS, format::pins_from_json).unwrap_or_default()
    }

    pub fn load_session(&self) -> Option<Session> {
        self.load(SESSION, format::session_from_json)
    }

    /// The file `name` as `parse` reads it, or none. A file that is there
    /// but is not one Platinum can read, broken JSON included, is set aside
    /// before the next save could write over it. One that cannot be read
    /// now, for want of permission say, is left alone and not written over.
    fn load<T>(&self, name: &'static str, parse: impl FnOnce(&Value) -> Option<T>) -> Option<T> {
        let path = self.dir.join(name);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return None,
            Err(error) => {
                log::warn!("reading {}: {error}; leaving it as it is", path.display());
                self.unread
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(name);
                return None;
            }
        };
        let parsed = serde_json::from_str(&text)
            .ok()
            .and_then(|value| parse(&value));
        if parsed.is_none() {
            let aside = path.with_extension("json.unreadable");
            log::warn!(
                "{} is not Platinum's; moved to {}",
                path.display(),
                aside.display()
            );
            let _ = fs::rename(&path, aside);
        }
        parsed
    }

    /// Write `value` as the file `name`, unless it could not be read. True
    /// once it is written.
    fn write(&self, name: &'static str, value: &Value) -> bool {
        let unread = self.unread.lock().unwrap_or_else(PoisonError::into_inner);
        !unread.contains(name) && write_json(&self.dir.join(name), value)
    }

    /// Add a tab for each of `contents` to the saved session, after the
    /// others. True once they are in the file.
    pub fn add_tabs(&self, contents: impl IntoIterator<Item = Content>) -> bool {
        let mut session = self.load_session().unwrap_or_default();
        session.add_tabs(contents);
        self.write(SESSION, &format::session_to_json(&session))
    }

    /// What one save writes, in its place among the others. Gather on the
    /// UI thread, where the tabs change, so the order is theirs.
    pub fn snapshot(
        &self,
        session: Session,
        pins: Vec<Pin>,
        icons: Vec<(String, Arc<RenderImage>)>,
        hosts: HashSet<String>,
    ) -> Snapshot {
        Snapshot {
            order: self.gathered.fetch_add(1, Ordering::Relaxed) + 1,
            session,
            pins,
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
        self.write(SESSION, &format::session_to_json(&snapshot.session));
        self.write(PINS, &format::pins_to_json(&snapshot.pins));
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

    /// Keep `icon` for `host`, in place of any other kept before.
    fn save_favicon(&self, host: &str, icon: &RenderImage) {
        let Some(pixels) = icon.as_bytes(0) else {
            return;
        };
        let size = icon.size(0);
        let bytes = format::encode_icon(size.width.0 as u32, size.height.0 as u32, pixels);
        let path = self.icon_path(host);
        if fs::read(&path).is_ok_and(|kept| kept == bytes) {
            return;
        }
        if let Err(error) =
            fs::create_dir_all(self.dir.join(FAVICONS)).and_then(|()| write_whole(&path, &bytes))
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

/// A session file's text, as Platinum reads it at launch.
#[cfg(test)]
pub fn parse_session(text: &str) -> Option<Session> {
    format::session_from_json(&serde_json::from_str(text).ok()?)
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
            visit_session: 0,
            next_tab_id: None,
            voice: Default::default(),
            board: format::BoardState::default(),
            sidebar: false,
            sidebar_width: Some(sidebar_width),
            places_pinned: false,
            active: None,
            tabs: Vec::new(),
            dock: Layout::default(),
        }
    }

    #[test]
    fn a_save_older_than_the_one_written_is_dropped() {
        let (store, dir) = store("save-order");
        let older = store.snapshot(session(200.), Vec::new(), Vec::new(), HashSet::new());
        let newer = store.snapshot(session(300.), Vec::new(), Vec::new(), HashSet::new());
        // The save at quit finishes first; the one from before reaches its
        // turn after.
        store.save(&newer);
        store.save(&older);
        let written = fs::read_to_string(dir.join(SESSION)).unwrap();
        let written: Value = serde_json::from_str(&written).unwrap();
        assert_eq!(written["sidebar_width"], 300.);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_kept_icon_follows_the_site_and_a_broken_one_is_rewritten() {
        let (store, dir) = store("icons");
        let icon = |side| crate::page::icon_image(image::RgbaImage::new(side, side));
        let size = |store: &Store| store.load_favicon("example.org").map(|icon| icon.size(0));
        store.save_favicon("example.org", &icon(16));
        store.save_favicon("example.org", &icon(32));
        assert_eq!(size(&store).map(|size| size.width.0), Some(32), "the newer");

        let path = store.icon_path("example.org");
        fs::write(&path, b"FRI1 cut short").unwrap();
        assert_eq!(size(&store), None);
        store.save_favicon("example.org", &icon(32));
        assert_eq!(
            size(&store).map(|size| size.width.0),
            Some(32),
            "whole again"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_cannot_be_read_is_left_alone() {
        use std::os::unix::fs::PermissionsExt as _;
        let (store, dir) = store("unread");
        let pins = r#"{"version": 1, "pins": []}"#;
        fs::write(dir.join(PINS), pins).unwrap();
        let locked = |mode| fs::set_permissions(dir.join(PINS), fs::Permissions::from_mode(mode));
        locked(0o000).unwrap();
        assert!(store.load_pins().is_empty());
        let pin = Pin {
            id: 1,
            url: url::Url::parse("https://example.org/").unwrap(),
            title: "Example".into(),
        };
        store.save(&store.snapshot(session(240.), vec![pin], Vec::new(), HashSet::new()));
        locked(0o644).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join(PINS)).unwrap(),
            pins,
            "not written over"
        );
        assert!(!dir.join("pins.json.unreadable").exists(), "nor set aside");
        assert!(dir.join(SESSION).exists(), "the session is still kept");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_data_folders_own_files_move_into_the_default_profile() {
        let (_, root) = store("adopt");
        let profile = root.join("profile");
        fs::create_dir_all(root.join(FAVICONS)).unwrap();
        fs::write(root.join(FAVICONS).join("example.org"), b"icon").unwrap();
        fs::write(root.join(SESSION), "old session").unwrap();
        fs::write(root.join(PINS), "old pins").unwrap();
        fs::write(root.join("recents.json"), "old recents").unwrap();
        fs::create_dir_all(&profile).unwrap();
        fs::write(profile.join(PINS), "newer pins").unwrap();
        adopt(&root, &profile);
        assert_eq!(
            fs::read_to_string(profile.join(SESSION)).unwrap(),
            "old session"
        );
        assert_eq!(
            fs::read_to_string(profile.join("recents.json")).unwrap(),
            "old recents"
        );
        assert!(!root.join("recents.json").exists());
        assert!(profile.join(FAVICONS).join("example.org").exists());
        assert!(!root.join(SESSION).exists() && !root.join(FAVICONS).exists());
        assert_eq!(
            fs::read_to_string(profile.join(PINS)).unwrap(),
            "newer pins",
            "a file already there is kept"
        );
        assert_eq!(fs::read_to_string(root.join(PINS)).unwrap(), "old pins");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn added_tabs_join_the_saved_session_once() {
        let (store, dir) = store("add-tabs");
        let page = |url: &str| Content::Page {
            url: url::Url::parse(url).unwrap(),
            title: String::new(),
        };
        let mut saved = session(240.);
        saved.add_tabs([page("https://example.org/")]);
        store.save(&store.snapshot(saved, Vec::new(), Vec::new(), HashSet::new()));
        let search = Content::Search {
            query: "rounded selection".into(),
            ask: false,
        };
        let imported = [
            page("https://example.org/"),
            page("https://example.com/"),
            search.clone(),
        ];
        assert!(store.add_tabs(imported.clone()));
        assert!(store.add_tabs(imported));
        let session = store.load_session().unwrap();
        let tabs: Vec<_> = session
            .tabs
            .iter()
            .map(|tab| (tab.id, tab.content.clone()))
            .collect();
        assert_eq!(
            tabs,
            [
                (0, page("https://example.org/")),
                (1, page("https://example.com/")),
                (2, search)
            ]
        );
        assert_eq!(session.sidebar_width, Some(240.), "the rest is kept");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_session_is_set_aside() {
        let (store, dir) = store("session-aside");
        fs::write(dir.join(SESSION), "{ not json").unwrap();
        assert!(store.load_session().is_none());
        assert!(!dir.join(SESSION).exists());
        assert!(dir.join("session.json.unreadable").exists());
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

fn write_json(path: &Path, value: &Value) -> bool {
    serde_json::to_vec_pretty(value)
        .map_err(std::io::Error::other)
        .and_then(|bytes| write_whole(path, &bytes))
        .inspect_err(|error| log::warn!("writing {}: {error}", path.display()))
        .is_ok()
}

/// Write through a temporary file, so a crash mid-write leaves the old file.
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
}
