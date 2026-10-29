//! Firefox's independent profile, Places, form and session formats.
use super::{Counts, SourceFormat, SourceProfile, credentials::Vault, import_login, snapshot};
use crate::{ProfileStore, SessionTab, web_url};
use anyhow::{Context, Result, ensure};
use configparser::ini::Ini;
use rusqlite::params;
use serde_json::Value;
use std::path::Path;

pub(super) fn discover_at(root: &Path) -> Result<Vec<SourceProfile>> {
    let mut ini = Ini::new();
    ini.load(root.join("profiles.ini"))
        .map_err(|_| anyhow::anyhow!("Could not read Firefox profiles.ini"))?;
    let mut profiles = Vec::new();
    for section in ini
        .sections()
        .into_iter()
        .filter(|s| s.to_ascii_lowercase().starts_with("profile"))
    {
        let Some(path) = ini.get(&section, "Path") else {
            continue;
        };
        let relative = ini.get(&section, "IsRelative").as_deref() != Some("0");
        if relative
            && (Path::new(&path).is_absolute()
                || Path::new(&path)
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)))
        {
            continue;
        }
        let directory = if relative {
            root.join(path)
        } else {
            path.into()
        };
        // Ignore uninitialized placeholders rather than offering empty imports.
        if !directory.join("places.sqlite").is_file() && !directory.join("logins.json").is_file() {
            continue;
        }
        let name = ini
            .get(&section, "Name")
            .unwrap_or_else(|| "Firefox".into());
        let email = std::fs::read(directory.join("signedInUser.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| {
                v.pointer("/accountData/email")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        profiles.push(SourceProfile {
            format: SourceFormat::Firefox,
            browser: "Firefox".into(),
            name,
            email,
            directory,
            keychain_service: String::new(),
            keychain_account: String::new(),
        });
    }
    profiles.sort_by(|a, b| a.directory.cmp(&b.directory));
    Ok(profiles)
}

pub(super) fn import_category(
    store: &ProfileStore,
    source: &SourceProfile,
    category: &str,
    vault: &impl Vault,
) -> Result<Counts> {
    match category {
        "Bookmarks" => bookmarks(store, &source.directory),
        "History" => history(store, &source.directory),
        "Passwords" => passwords(store, &source.directory, vault),
        "Extensions" => extensions(store, &source.directory),
        "Form information" => forms(store, &source.directory),
        "Open tabs" => tabs(store, &source.directory),
        _ => anyhow::bail!("Unsupported Firefox category"),
    }
}

fn bookmarks(store: &ProfileStore, dir: &Path) -> Result<Counts> {
    let src = snapshot(&dir.join("places.sqlite"))?;
    let mut db = store.connect()?;
    let tx = db.transaction()?;
    let mut counts = Counts::default();
    let mut stmt=src.prepare("WITH RECURSIVE folders(id,path,depth) AS (
        SELECT id,coalesce(title,''),0 FROM moz_bookmarks WHERE type=2 AND parent=0
        UNION ALL SELECT b.id,CASE WHEN f.path='' THEN coalesce(b.title,'') ELSE f.path||'/'||coalesce(b.title,'') END,f.depth+1
        FROM moz_bookmarks b JOIN folders f ON b.parent=f.id WHERE b.type=2 AND f.depth<128)
        SELECT p.url,coalesce(b.title,p.title,''),coalesce(f.path,'Imported bookmarks')
        FROM moz_bookmarks b JOIN moz_places p ON b.fk=p.id LEFT JOIN folders f ON b.parent=f.id WHERE b.type=1")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (url, title, folder) = row?;
        let Some(url) = web_url(&url) else {
            counts.skipped += 1;
            continue;
        };
        let n = tx.execute(
            "INSERT OR IGNORE INTO bookmarks VALUES(?1,?2,?3)",
            params![url.as_str(), title, folder],
        )?;
        counts.imported += n;
        counts.skipped += usize::from(n == 0);
    }
    tx.commit()?;
    Ok(counts)
}

fn history(store: &ProfileStore, dir: &Path) -> Result<Counts> {
    let src = snapshot(&dir.join("places.sqlite"))?;
    let mut db = store.connect()?;
    let tx = db.transaction()?;
    let mut counts = Counts::default();
    let mut stmt=src.prepare("SELECT url,coalesce(title,''),coalesce(last_visit_date,0),visit_count FROM moz_places WHERE hidden=0 AND visit_count>0")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })? {
        let (url, title, time, visits) = row?;
        let Some(url) = web_url(&url) else {
            counts.skipped += 1;
            continue;
        };
        let n=tx.execute("INSERT INTO history VALUES(?1,?2,?3,?4)
            ON CONFLICT(url) DO UPDATE SET title=excluded.title,visited=max(history.visited,excluded.visited),visits=max(history.visits,excluded.visits)
            WHERE excluded.visited>history.visited OR excluded.visits>history.visits",params![url.as_str(),title,time.max(0),visits.max(1)])?;
        counts.imported += n;
        counts.skipped += usize::from(n == 0);
    }
    tx.commit()?;
    Ok(counts)
}

fn passwords(store: &ProfileStore, dir: &Path, vault: &impl Vault) -> Result<Counts> {
    let logins: Value = serde_json::from_reader(std::fs::File::open(dir.join("logins.json"))?)?;
    let records = logins["logins"]
        .as_array()
        .context("Unsupported Firefox login format")?;
    if records.is_empty() {
        return Ok(Counts::default());
    }
    let nss = super::nss::Profile::open(dir)?;
    let mut counts = Counts::default();
    let mut failed = 0;
    for row in records {
        let Some(url) = row["hostname"].as_str().and_then(web_url) else {
            counts.skipped += 1;
            continue;
        };
        let (Some(user), Some(password)) = (
            row["encryptedUsername"].as_str(),
            row["encryptedPassword"].as_str(),
        ) else {
            counts.skipped += 1;
            continue;
        };
        match (nss.decrypt(user), nss.decrypt(password)) {
            (Ok(user), Ok(password)) if !password.is_empty() => {
                if import_login(store, vault, url.as_str(), &user, &password)? {
                    counts.imported += 1;
                } else {
                    counts.skipped += 1;
                }
            }
            _ => {
                failed += 1;
                counts.skipped += 1;
            }
        }
    }
    ensure!(
        failed == 0,
        "{} passwords imported; {} could not be decrypted. Use Firefox password CSV export for those logins",
        counts.imported,
        failed
    );
    Ok(counts)
}

fn extensions(store: &ProfileStore, dir: &Path) -> Result<Counts> {
    let data: Value = serde_json::from_reader(std::fs::File::open(dir.join("extensions.json"))?)?;
    let mut db = store.connect()?;
    let tx = db.transaction()?;
    let mut counts = Counts::default();
    for addon in data["addons"]
        .as_array()
        .context("Unsupported Firefox extension inventory")?
    {
        if addon["type"] != "extension" || addon["isBuiltin"] == true {
            continue;
        }
        let Some(id) = addon["id"].as_str() else {
            counts.skipped += 1;
            continue;
        };
        let name = addon
            .pointer("/defaultLocale/name")
            .and_then(Value::as_str)
            .unwrap_or(id);
        let version = addon["version"].as_str().unwrap_or("");
        let n = tx.execute(
            "INSERT OR IGNORE INTO extensions VALUES(?1,?2,?3)",
            params![id, name, version],
        )?;
        counts.imported += n;
        counts.skipped += usize::from(n == 0);
    }
    tx.commit()?;
    Ok(counts)
}

fn forms(store: &ProfileStore, dir: &Path) -> Result<Counts> {
    let src = snapshot(&dir.join("formhistory.sqlite"))?;
    let mut db = store.connect()?;
    let tx = db.transaction()?;
    let mut counts = Counts::default();
    tx.execute_batch("CREATE TABLE IF NOT EXISTS form_information(name TEXT NOT NULL,value TEXT NOT NULL,PRIMARY KEY(name,value));")?;
    for row in src
        .prepare("SELECT fieldname,value FROM moz_formhistory")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
    {
        let (name, value) = row?;
        if ![
            "email",
            "name",
            "first_name",
            "last_name",
            "city",
            "country",
        ]
        .contains(&name.to_ascii_lowercase().as_str())
        {
            counts.skipped += 1;
            continue;
        }
        let n = tx.execute(
            "INSERT OR IGNORE INTO form_information VALUES(?1,?2)",
            params![name, value],
        )?;
        counts.imported += n;
        counts.skipped += usize::from(n == 0);
    }
    tx.commit()?;
    Ok(counts)
}

fn tabs(store: &ProfileStore, dir: &Path) -> Result<Counts> {
    let candidates = [
        "sessionstore.jsonlz4",
        "sessionstore-backups/recovery.jsonlz4",
        "sessionstore-backups/recovery.baklz4",
        "sessionstore-backups/previous.jsonlz4",
    ];
    let path = candidates
        .iter()
        .map(|p| dir.join(p))
        .filter(|p| p.is_file())
        .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
        .context("No Firefox saved session found")?;
    let bytes = std::fs::read(path)?;
    ensure!(
        bytes.starts_with(b"mozLz40\0"),
        "Unsupported Firefox session format"
    );
    let size = u32::from_le_bytes(
        bytes
            .get(8..12)
            .context("Incomplete Firefox session")?
            .try_into()?,
    );
    ensure!(
        size <= 256 * 1024 * 1024,
        "Firefox session is too large to import safely"
    );
    let clear = lz4_flex::decompress_size_prepended(&bytes[8..])
        .context("Could not decompress Firefox session")?;
    let data: Value = serde_json::from_slice(&clear).context("Could not read Firefox session")?;
    let mut session = store.session()?;
    let mut counts = Counts::default();
    for window in data["windows"]
        .as_array()
        .context("No Firefox session windows found")?
    {
        let Some(tabs) = window["tabs"].as_array() else {
            continue;
        };
        for tab in tabs {
            let Some(entries) = tab["entries"].as_array() else {
                counts.skipped += 1;
                continue;
            };
            let selected = tab["index"]
                .as_u64()
                .unwrap_or(entries.len() as u64)
                .saturating_sub(1) as usize;
            let Some(url) = entries
                .get(selected)
                .and_then(|e| e["url"].as_str())
                .and_then(web_url)
            else {
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
            session.tabs.push(SessionTab {
                url: Some(url.to_string()),
                query: None,
            });
            counts.imported += 1;
        }
    }
    if counts.imported > 0 {
        session.layout = None;
        store.save_session(&session)?;
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::Vault;
    use std::{cell::RefCell, collections::HashMap};
    use tempfile::TempDir;
    use zeroize::Zeroizing;
    #[derive(Default)]
    struct MemoryVault(RefCell<HashMap<String, Vec<u8>>>);
    impl Vault for MemoryVault {
        fn put(&self, key: &str, secret: &[u8]) -> Result<()> {
            self.0.borrow_mut().insert(key.into(), secret.into());
            Ok(())
        }
        fn get(&self, key: &str) -> Result<Zeroizing<Vec<u8>>> {
            self.0
                .borrow()
                .get(key)
                .cloned()
                .map(Zeroizing::new)
                .context("Missing test secret")
        }
        fn remove(&self, key: &str) -> Result<()> {
            self.0.borrow_mut().remove(key);
            Ok(())
        }
    }
    #[test]
    fn discovers_initialized_profiles_and_rejects_relative_traversal() {
        let temp = TempDir::new().unwrap();
        std::fs::create_dir(temp.path().join("populated")).unwrap();
        std::fs::create_dir(temp.path().join("empty")).unwrap();
        std::fs::write(temp.path().join("populated/places.sqlite"), []).unwrap();
        std::fs::write(temp.path().join("profiles.ini"), "[Profile0]\nName=Personal\nIsRelative=1\nPath=populated\n[Profile1]\nPath=empty\n[Profile2]\nPath=../escape\n").unwrap();
        let found = discover_at(temp.path()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Personal");
        assert_eq!(found[0].format, SourceFormat::Firefox);
    }
    #[test]
    fn places_wal_folders_unix_microseconds_and_retry() {
        let source = TempDir::new().unwrap();
        let dest = TempDir::new().unwrap();
        let store = ProfileStore::open(dest.path()).unwrap();
        let db = rusqlite::Connection::open(source.path().join("places.sqlite")).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE moz_places(id INTEGER PRIMARY KEY,url TEXT,title TEXT,hidden INTEGER,last_visit_date INTEGER,visit_count INTEGER);
            CREATE TABLE moz_bookmarks(id INTEGER PRIMARY KEY,type INTEGER,parent INTEGER,fk INTEGER,title TEXT);
            INSERT INTO moz_places VALUES(1,'https://fixture.example.test/','Fixture',0,1720000000123456,7),(2,'javascript:alert(1)','Unsafe',0,42,1),(3,'https://hidden.example.test/','Hidden',1,42,1);
            INSERT INTO moz_bookmarks VALUES(1,2,0,NULL,''),(2,2,1,NULL,'Toolbar'),(3,2,2,NULL,'Work'),(4,1,3,1,'Saved'),(5,1,3,2,'Unsafe');").unwrap();
        assert_eq!(bookmarks(&store, source.path()).unwrap().imported, 1);
        assert_eq!(history(&store, source.path()).unwrap().imported, 1);
        let saved = store.suggestions("", true, 10).unwrap();
        assert_eq!(saved[0].folder, "Toolbar/Work");
        assert_eq!(
            store
                .connect()
                .unwrap()
                .query_row("SELECT visited FROM history", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1720000000123456
        );
        assert_eq!(bookmarks(&store, source.path()).unwrap().imported, 0);
        assert_eq!(history(&store, source.path()).unwrap().imported, 0);
    }
    #[test]
    fn sessions_take_current_navigation_exclude_closed_and_retry() {
        let source = TempDir::new().unwrap();
        let dest = TempDir::new().unwrap();
        let store = ProfileStore::open(dest.path()).unwrap();
        let data = serde_json::json!({"windows":[{"tabs":[{"index":1,"entries":[{"url":"https://current.example.test/"},{"url":"https://forward.example.test/"}]},{"index":1,"entries":[{"url":"about:blank"}]}],"_closedTabs":[{"state":{"entries":[{"url":"https://closed.example.test/"}]}}]}]});
        let mut bytes = b"mozLz40\0".to_vec();
        bytes.extend(lz4_flex::compress_prepend_size(
            &serde_json::to_vec(&data).unwrap(),
        ));
        std::fs::write(source.path().join("sessionstore.jsonlz4"), bytes).unwrap();
        assert_eq!(tabs(&store, source.path()).unwrap().imported, 1);
        assert_eq!(
            store.session().unwrap().tabs[0].url.as_deref(),
            Some("https://current.example.test/")
        );
        assert_eq!(tabs(&store, source.path()).unwrap().imported, 0);
        std::fs::write(
            source.path().join("sessionstore.jsonlz4"),
            b"mozLz40\0\xff\xff\xff\xff",
        )
        .unwrap();
        assert!(tabs(&store, source.path()).is_err());
    }
    #[test]
    fn forms_are_allowlisted_and_builtin_extensions_excluded() {
        let source = TempDir::new().unwrap();
        let dest = TempDir::new().unwrap();
        let store = ProfileStore::open(dest.path()).unwrap();
        let db = rusqlite::Connection::open(source.path().join("formhistory.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE moz_formhistory(fieldname TEXT,value TEXT); INSERT INTO moz_formhistory VALUES('email','fixture@example.test'),('password','excluded'),('card_number','excluded');").unwrap();
        std::fs::write(source.path().join("extensions.json"),br#"{"addons":[{"id":"fixture","type":"extension","version":"1","defaultLocale":{"name":"Fixture"}},{"id":"builtin","type":"extension","isBuiltin":true}]}"#).unwrap();
        assert_eq!(forms(&store, source.path()).unwrap().imported, 1);
        assert_eq!(store.form_values().unwrap().len(), 1);
        assert_eq!(extensions(&store, source.path()).unwrap().imported, 1);
        assert_eq!(extensions(&store, source.path()).unwrap().imported, 0);
    }
    #[test]
    #[ignore = "requires installed Mozilla NSS and certutil; synthetic credentials only"]
    fn native_nss_login_import_roundtrip_and_retry() {
        let source = TempDir::new().unwrap();
        let dest = TempDir::new().unwrap();
        let store = ProfileStore::open(dest.path()).unwrap();
        let encrypted = super::super::nss::fixture(source.path()).unwrap();
        let data = serde_json::json!({"logins":[{"hostname":"https://fixture.example.test","encryptedUsername":encrypted.0,"encryptedPassword":encrypted.1}]});
        std::fs::write(
            source.path().join("logins.json"),
            serde_json::to_vec(&data).unwrap(),
        )
        .unwrap();
        let vault = MemoryVault::default();
        assert_eq!(
            passwords(&store, source.path(), &vault).unwrap().imported,
            1
        );
        assert_eq!(
            &**crate::credentials::password(
                &vault,
                "https://fixture.example.test/",
                "fixture-user"
            )
            .unwrap(),
            "fixture-secret"
        );
        assert_eq!(
            passwords(&store, source.path(), &vault).unwrap().imported,
            0
        );
        assert!(source.path().join("key4.db").is_file());
    }
}
