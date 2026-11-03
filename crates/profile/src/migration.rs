//! Local browser adapters. SQLite sources are read through a consistent
//! snapshot; only selected categories are read, and category failures are isolated.
mod firefox;
mod nss;

use super::{
    ProfileStore,
    credentials::{self, Vault},
    now, web_url,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceFormat {
    Chromium,
    Firefox,
}

#[derive(Clone, Debug)]
pub struct SourceProfile {
    pub format: SourceFormat,
    pub browser: String,
    pub name: String,
    pub email: String,
    pub directory: PathBuf,
    pub keychain_service: String,
    pub keychain_account: String,
}
impl SourceProfile {
    pub fn identity(&self) -> String {
        self.directory.to_string_lossy().into_owned()
    }
}

#[derive(Clone, Copy)]
pub struct Categories {
    pub bookmarks: bool,
    pub history: bool,
    pub passwords: bool,
    pub extensions: bool,
    pub forms: bool,
    pub tabs: bool,
}
impl Default for Categories {
    fn default() -> Self {
        Self {
            bookmarks: true,
            history: true,
            passwords: false,
            extensions: true,
            forms: false,
            tabs: false,
        }
    }
}
#[derive(Default, Debug, Clone)]
pub struct Counts {
    pub imported: usize,
    pub skipped: usize,
}
#[derive(Debug, Clone)]
pub struct CategoryReport {
    pub category: String,
    pub counts: Counts,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub categories: Vec<CategoryReport>,
}
impl Report {
    pub fn summary(&self) -> String {
        self.categories
            .iter()
            .map(|c| match &c.error {
                Some(e) => format!("{}: {e}", c.category),
                None => format!(
                    "{}: {} imported, {} skipped or already present",
                    c.category, c.counts.imported, c.counts.skipped
                ),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn discover() -> Vec<SourceProfile> {
    let Some(base) = dirs::data_dir() else {
        return vec![];
    };
    let browsers = [
        ("Chrome", "Google/Chrome", "Chrome"),
        ("Chrome Beta", "Google/Chrome Beta", "Chrome"),
        ("Brave", "BraveSoftware/Brave-Browser", "Brave"),
        ("Edge", "Microsoft Edge", "Microsoft Edge"),
        ("Chromium", "Chromium", "Chromium"),
        ("Dia", "Dia/User Data", "Dia"),
    ];
    let mut profiles: Vec<_> = browsers
        .iter()
        .flat_map(|(name, path, key)| discover_at(&base.join(path), name, key).unwrap_or_default())
        .collect();
    profiles.extend(firefox::discover_at(&base.join("Firefox")).unwrap_or_default());
    profiles
}

pub fn discover_at(root: &Path, browser: &str, key: &str) -> Result<Vec<SourceProfile>> {
    let state: Value = serde_json::from_reader(std::fs::File::open(root.join("Local State"))?)?;
    let cache = state
        .pointer("/profile/info_cache")
        .and_then(Value::as_object)
        .context("No named browser profiles found")?;
    let mut profiles = vec![];
    for (folder, info) in cache {
        // Profile paths are untrusted metadata, never absolute or traversal paths.
        if !matches!(
            Path::new(folder)
                .components()
                .collect::<Vec<_>>()
                .as_slice(),
            [std::path::Component::Normal(_)]
        ) {
            continue;
        }
        let dir = root.join(folder);
        if !dir.is_dir() {
            continue;
        }
        profiles.push(SourceProfile {
            format: SourceFormat::Chromium,
            browser: browser.into(),
            name: info["name"].as_str().unwrap_or(folder).into(),
            email: info["user_name"].as_str().unwrap_or("").into(),
            directory: dir,
            keychain_service: format!("{key} Safe Storage"),
            keychain_account: key.into(),
        });
    }
    Ok(profiles)
}

pub fn snapshot(path: &Path) -> Result<Connection> {
    let source = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("Could not open source database; close the source browser and retry")?;
    source.busy_timeout(Duration::from_secs(2))?;
    let mut destination = Connection::open_in_memory()?;
    {
        let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            match backup.step(256)? {
                rusqlite::backup::StepResult::Done => break,
                _ => ensure!(
                    Instant::now() < deadline,
                    "Source database is busy; close the source browser and retry"
                ),
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(destination)
}

pub fn import(
    store: &ProfileStore,
    source: &SourceProfile,
    selected: Categories,
    vault: &impl Vault,
) -> Report {
    let mut report = Report::default();
    let categories = [
        ("Bookmarks", selected.bookmarks),
        ("History", selected.history),
        ("Extensions", selected.extensions),
        ("Form information", selected.forms),
        ("Open tabs", selected.tabs),
        ("Passwords", selected.passwords),
    ];
    for (category, enabled) in categories {
        if !enabled {
            continue;
        }
        let result = if source.format == SourceFormat::Firefox {
            firefox::import_category(store, source, category, vault)
        } else {
            match category {
                "Bookmarks" => bookmarks(store, source),
                "History" => history(store, source),
                "Passwords" => passwords(store, source, vault),
                "Extensions" => extensions(store, source),
                "Open tabs" => session_tabs(store, source),
                _ => form_information(store, source),
            }
        };
        let entry = match result {
            Ok(counts) => CategoryReport {
                category: category.into(),
                counts,
                error: None,
            },
            Err(error) => CategoryReport {
                category: category.into(),
                counts: Counts::default(),
                error: Some(format!("{error:#}")),
            },
        };
        // Report persistence failure as part of the report, never panic or discard
        // results from the other categories.
        let saved = (|| -> Result<()> {
            store.log_import(
                &source.identity(),
                category,
                if entry.error.is_none() { now() } else { 0 },
                entry.counts.imported,
                entry.counts.skipped,
            )?;
            Ok(())
        })();
        report.categories.push(entry);
        if saved.is_err() {
            report.categories.push(CategoryReport {
                category: "Import log".into(),
                counts: Counts::default(),
                error: Some("Could not save import report".into()),
            });
        }
    }
    report
}

fn bookmark_node(
    db: &mut crate::ImportBatch<'_>,
    node: &Value,
    folder: &str,
    counts: &mut Counts,
) -> Result<()> {
    if node["type"] == "url" {
        if let Some(url) = node["url"].as_str().and_then(web_url) {
            let changed = db.bookmark(url.as_str(), node["name"].as_str().unwrap_or(""), folder)?;
            counts.imported += changed;
            counts.skipped += usize::from(changed == 0);
        } else {
            counts.skipped += 1;
        }
    } else if let Some(children) = node["children"].as_array() {
        let name = node["name"].as_str().unwrap_or("");
        let folder = if folder.is_empty() {
            name.to_owned()
        } else {
            format!("{folder}/{name}")
        };
        for child in children {
            bookmark_node(db, child, &folder, counts)?;
        }
    }
    Ok(())
}

fn bookmarks(store: &ProfileStore, source: &SourceProfile) -> Result<Counts> {
    let mut tx = store.import_batch()?;
    let mut counts = Counts::default();
    let mut found = false;
    for name in ["Bookmarks", "AccountBookmarks"] {
        let path = source.directory.join(name);
        if !path.exists() {
            continue;
        }
        found = true;
        let tree: Value = serde_json::from_reader(std::fs::File::open(path)?)?;
        let roots = tree["roots"]
            .as_object()
            .context("Invalid bookmarks file")?;
        for node in roots.values() {
            bookmark_node(&mut tx, node, "", &mut counts)?;
        }
    }
    ensure!(found, "No bookmarks file found");
    tx.commit()?;
    Ok(counts)
}

fn history(store: &ProfileStore, source: &SourceProfile) -> Result<Counts> {
    let src = snapshot(&source.directory.join("History"))?;
    let mut tx = store.import_batch()?;
    let mut counts = Counts::default();
    let mut stmt =
        src.prepare("SELECT url,title,last_visit_time,visit_count FROM urls WHERE hidden=0")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    for row in rows {
        let (url, title, time, visits) = row?;
        let Some(url) = web_url(&url) else {
            counts.skipped += 1;
            continue;
        };
        // Chrome microseconds since 1601 -> Unix microseconds.
        let visited = time.saturating_sub(11_644_473_600_000_000).max(0);
        let n = tx.history(url.as_str(), &title, visited, visits.max(1))?;
        counts.imported += n;
        counts.skipped += usize::from(n == 0);
    }
    tx.commit()?;
    Ok(counts)
}

fn passwords(store: &ProfileStore, source: &SourceProfile, vault: &impl Vault) -> Result<Counts> {
    let sources = ["Login Data", "Login Data For Account"]
        .into_iter()
        .map(|p| source.directory.join(p))
        .filter(|p| p.exists())
        .collect::<Vec<_>>();
    ensure!(
        !sources.is_empty(),
        "No saved login database found; use password CSV import"
    );
    let key = credentials::chrome_key(&source.keychain_service, &source.keychain_account)?;
    let mut counts = Counts::default();
    let mut failures = 0;
    for path in sources {
        let src = snapshot(&path)?;
        let mut stmt=src.prepare("SELECT origin_url,username_value,password_value FROM logins WHERE blacklisted_by_user=0")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        for row in rows {
            let (url, username, encrypted) = row?;
            if encrypted.is_empty() || web_url(&url).is_none() {
                counts.skipped += 1;
                continue;
            }
            match credentials::decrypt_chrome(&key, &encrypted) {
                Ok(secret) => {
                    if import_login(store, vault, &url, &username, &secret)? {
                        counts.imported += 1;
                    } else {
                        counts.skipped += 1;
                    }
                }
                Err(_) => {
                    counts.skipped += 1;
                    failures += 1;
                }
            }
        }
    }
    ensure!(
        failures == 0,
        "{} passwords imported; {} could not be decrypted. Use password CSV import for the remaining logins",
        counts.imported,
        failures
    );
    Ok(counts)
}

fn import_login(
    store: &ProfileStore,
    vault: &impl Vault,
    url: &str,
    username: &str,
    password: &str,
) -> Result<bool> {
    if credentials::accounts(store, url)?
        .iter()
        .any(|u| u == username)
    {
        return Ok(false);
    }
    credentials::save(store, vault, url, username, password)?;
    Ok(true)
}

pub fn password_csv(store: &ProfileStore, path: &Path, vault: &impl Vault) -> Result<Counts> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .context("Could not open password CSV")?;
    let headers = reader
        .headers()
        .map_err(|_| anyhow::anyhow!("Invalid CSV header"))?
        .clone();
    let col = |name: &str| {
        headers
            .iter()
            .position(|h| {
                h.trim_start_matches('\u{feff}')
                    .trim()
                    .eq_ignore_ascii_case(name)
            })
            .with_context(|| format!("CSV needs a {name} column"))
    };
    let url = col("url")?;
    let user = col("username")?;
    let pass = col("password")?;
    let mut counts = Counts::default();
    for row in reader.records() {
        // csv errors can contain cell contents: never surface the underlying error.
        let row = row.map_err(|_| anyhow::anyhow!("Malformed password CSV row"))?;
        let (Some(url), Some(username), Some(password)) =
            (row.get(url), row.get(user), row.get(pass))
        else {
            counts.skipped += 1;
            continue;
        };
        if web_url(url).is_none() || password.is_empty() {
            counts.skipped += 1;
            continue;
        }
        if import_login(store, vault, url, username, password)? {
            counts.imported += 1;
        } else {
            counts.skipped += 1;
        }
    }
    Ok(counts)
}

fn extensions(store: &ProfileStore, source: &SourceProfile) -> Result<Counts> {
    let mut tx = store.import_batch()?;
    let mut counts = Counts::default();
    for file in ["Preferences", "Secure Preferences"] {
        let path = source.directory.join(file);
        if !path.exists() {
            continue;
        }
        let prefs: Value = serde_json::from_reader(std::fs::File::open(path)?)?;
        let Some(settings) = prefs
            .pointer("/extensions/settings")
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (id, ext) in settings {
            let name = ext
                .pointer("/manifest/name")
                .and_then(Value::as_str)
                .unwrap_or(id);
            let version = ext
                .pointer("/manifest/version")
                .and_then(Value::as_str)
                .unwrap_or("");
            let n = tx.extension(id, name, version)?;
            counts.imported += n;
            counts.skipped += usize::from(n == 0);
        }
    }
    tx.commit()?;
    Ok(counts)
}

fn form_information(store: &ProfileStore, source: &SourceProfile) -> Result<Counts> {
    let src = snapshot(&source.directory.join("Web Data"))?;
    import_forms(store, form_rows(&src, "SELECT name,value FROM autofill")?)
}

fn form_rows(src: &Connection, sql: &str) -> Result<Vec<(String, String)>> {
    Ok(src
        .prepare(sql)?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

fn import_forms(store: &ProfileStore, rows: Vec<(String, String)>) -> Result<Counts> {
    let mut tx = store.import_batch()?;
    let mut counts = Counts::default();
    for (name, value) in rows {
        if !credentials::form_field_allowed(&name) {
            counts.skipped += 1;
            continue;
        }
        let n = tx.form(&name, &value)?;
        counts.imported += n;
        counts.skipped += usize::from(n == 0);
    }
    tx.commit()?;
    Ok(counts)
}

pub fn bookmarks_html(store: &ProfileStore, path: &Path) -> Result<Counts> {
    let document = scraper::Html::parse_document(&std::fs::read_to_string(path)?);
    let selector = scraper::Selector::parse("a[href]").expect("static selector");
    let mut tx = store.import_batch()?;
    let mut counts = Counts::default();
    for a in document.select(&selector) {
        let Some(url) = a.value().attr("href").and_then(web_url) else {
            counts.skipped += 1;
            continue;
        };
        let title = a.text().collect::<String>();
        // Netscape HTML uses nested DL folders; walk ancestors to preserve them.
        let mut folders = Vec::new();
        for ancestor in a.ancestors().filter_map(scraper::ElementRef::wrap) {
            if ancestor.value().name() == "dl"
                && let Some(heading) = ancestor
                    .prev_siblings()
                    .filter_map(scraper::ElementRef::wrap)
                    .find_map(|e| {
                        if e.value().name() == "h3" {
                            Some(e.text().collect::<String>())
                        } else {
                            e.select(&scraper::Selector::parse("h3").unwrap())
                                .next()
                                .map(|h| h.text().collect())
                        }
                    })
            {
                folders.push(heading);
            }
        }
        folders.reverse();
        let folder = if folders.is_empty() {
            "Imported bookmarks".into()
        } else {
            folders.join("/")
        };
        let n = tx.bookmark(url.as_str(), &title, &folder)?;
        counts.imported += n;
        counts.skipped += usize::from(n == 0);
    }
    tx.commit()?;
    Ok(counts)
}

/// Import only the newest Session file. Tabs_* contains recently closed tabs,
/// and merging every historical file would resurrect old browsing sessions.
fn session_tabs(store: &ProfileStore, source: &SourceProfile) -> Result<Counts> {
    use std::collections::HashSet;
    let mut files = std::fs::read_dir(source.directory.join("Sessions"))?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("Session_"))
        .collect::<Vec<_>>();
    files.sort_by_key(|e| e.file_name());
    let path = files
        .last()
        .context("No supported session file found")?
        .path();
    let before = std::fs::metadata(&path)?;
    let bytes = std::fs::read(&path)?;
    let after = std::fs::metadata(&path)?;
    ensure!(
        before.len() == after.len() && before.modified()? == after.modified()?,
        "Session changed during import; close the source browser and retry"
    );
    let stream = snss::read_records(bytes.as_slice()).context("Unsupported session format")?;
    ensure!(
        stream.warnings.is_empty(),
        "Source session is incomplete; close the source browser and retry"
    );
    // snss-core handles framing/Pickle/replay. Its replay does not apply close
    // or prune commands. Filter known closed objects here; refuse pruned logs
    // rather than import a stale navigation. IDs/layout: Chromium's
    // components/sessions/core/session_service_commands.cc (2026-10-25).
    let mut closed_tabs = HashSet::new();
    let mut closed_windows = HashSet::new();
    for record in &stream.records {
        ensure!(
            !matches!(record.id, 5 | 11 | 24),
            "This session contains unsupported navigation pruning; export the open tabs as bookmarks instead"
        );
        if matches!(record.id, 16 | 17) {
            let id = i32::from_le_bytes(
                record
                    .payload
                    .get(..4)
                    .context("Invalid session close command")?
                    .try_into()?,
            );
            if record.id == 16 {
                closed_tabs.insert(id);
            } else {
                closed_windows.insert(id);
            }
        }
    }
    let replay = snss::replay(&stream, snss::Dialect::Session);
    ensure!(
        replay.warnings.is_empty(),
        "Some session records could not be decoded; no tabs were imported"
    );
    let mut session = store.session()?;
    let mut counts = Counts::default();
    for window in replay.windows {
        if closed_windows.contains(&window.id) {
            continue;
        }
        for tab in window.tabs {
            if closed_tabs.contains(&tab.id) {
                continue;
            }
            let Some(url) = web_url(&tab.current_nav().url) else {
                counts.skipped += 1;
                continue;
            };
            if session
                .tabs
                .iter()
                .any(|t| t.url.as_deref() == Some(url.as_str()))
            {
                counts.skipped += 1;
                continue;
            }
            session.tabs.push(super::SessionTab {
                url: Some(url.to_string()),
                query: None,
            });
            counts.imported += 1;
        }
    }
    if counts.imported > 0 {
        session.layout = None;
    }
    store.save_session(&session)?;
    Ok(counts)
}
