//! Browser data is partitioned by profile; source profiles are never modified.
pub mod credentials;
pub mod migration;

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub id: i64,
    pub name: String,
    pub email: String,
}

#[derive(Clone)]
pub struct ProfileStore {
    pub root: PathBuf,
    pub profile: Profile,
}

#[derive(Clone, Debug)]
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

fn catalog(root: &Path) -> Result<Connection> {
    std::fs::create_dir_all(root)?;
    let db = Connection::open(root.join("profiles.sqlite3"))?;
    db.busy_timeout(std::time::Duration::from_secs(3))?;
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS profiles (
          id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL DEFAULT '',
          source TEXT UNIQUE);
        CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
        INSERT OR IGNORE INTO profiles(id,name) VALUES(1,'Default');",
    )?;
    Ok(db)
}

impl ProfileStore {
    pub fn open(root: &Path) -> Result<Self> {
        let db = catalog(root)?;
        let id = db
            .query_row("SELECT value FROM settings WHERE key='active'", [], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or(1);
        let profile = db.query_row(
            "SELECT id,name,email FROM profiles WHERE id=?1",
            [id],
            |r| {
                Ok(Profile {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    email: r.get(2)?,
                })
            },
        )?;
        let store = Self {
            root: root.to_owned(),
            profile,
        };
        store.connect()?;
        Ok(store)
    }

    pub fn profiles(&self) -> Result<Vec<Profile>> {
        let db = catalog(&self.root)?;
        Ok(db
            .prepare("SELECT id,name,email FROM profiles ORDER BY id")?
            .query_map([], |r| {
                Ok(Profile {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    email: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn imported_profile(&self, source: &str, name: &str, email: &str) -> Result<Self> {
        let db = catalog(&self.root)?;
        db.execute(
            "INSERT INTO profiles(name,email,source) VALUES(?1,?2,?3)
          ON CONFLICT(source) DO UPDATE SET name=excluded.name,email=excluded.email",
            params![name, email, source],
        )?;
        let id = db.query_row("SELECT id FROM profiles WHERE source=?1", [source], |r| {
            r.get(0)
        })?;
        let store = Self {
            root: self.root.clone(),
            profile: Profile {
                id,
                name: name.into(),
                email: email.into(),
            },
        };
        store.connect()?;
        Ok(store)
    }

    pub fn select(&self, id: i64) -> Result<()> {
        ensure!(
            self.profiles()?.iter().any(|p| p.id == id),
            "Unknown profile"
        );
        catalog(&self.root)?.execute(
            "INSERT INTO settings(key,value) VALUES('active',?1)
            ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [id],
        )?;
        Ok(())
    }

    pub fn directory(&self) -> PathBuf {
        // Default keeps the existing Servo profile in place.
        if self.profile.id == 1 {
            self.root.join("profile")
        } else {
            self.root.join("profiles").join(self.profile.id.to_string())
        }
    }

    pub fn search_directory(&self) -> PathBuf {
        if self.profile.id == 1 {
            self.root.clone()
        } else {
            self.directory().join("search")
        }
    }

    pub fn connect(&self) -> Result<Connection> {
        let directory = self.directory();
        std::fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        }
        let db = Connection::open(directory.join("browser.sqlite3"))?;
        db.busy_timeout(std::time::Duration::from_secs(3))?;
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS bookmarks(url TEXT NOT NULL,title TEXT NOT NULL,folder TEXT NOT NULL,PRIMARY KEY(url,folder));
            CREATE TABLE IF NOT EXISTS history(url TEXT PRIMARY KEY,title TEXT NOT NULL,visited INTEGER NOT NULL,visits INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS credentials(origin TEXT NOT NULL,username TEXT NOT NULL,PRIMARY KEY(origin,username));
            CREATE TABLE IF NOT EXISTS extensions(id TEXT PRIMARY KEY,name TEXT NOT NULL,version TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS imports(id INTEGER PRIMARY KEY,source TEXT NOT NULL,category TEXT NOT NULL,completed INTEGER NOT NULL,imported INTEGER NOT NULL,skipped INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS session(id INTEGER PRIMARY KEY CHECK(id=1),value TEXT NOT NULL);")?;
        Ok(db)
    }

    pub fn bookmark(&self, url: &str, title: &str) -> Result<()> {
        let url = web_url(url).context("Only web addresses can be bookmarked")?;
        self.connect()?.execute(
            "INSERT INTO bookmarks VALUES(?1,?2,'Bookmarks')
            ON CONFLICT(url,folder) DO UPDATE SET title=excluded.title",
            params![url.as_str(), title],
        )?;
        Ok(())
    }

    pub fn visit(&self, url: &str, title: &str) -> Result<()> {
        let Some(url) = web_url(url) else {
            return Ok(());
        };
        self.connect()?.execute("INSERT INTO history VALUES(?1,?2,?3,1)
            ON CONFLICT(url) DO UPDATE SET title=excluded.title,visited=excluded.visited,visits=history.visits+1",
            params![url.as_str(),title,now()])?;
        Ok(())
    }

    pub fn suggestions(
        &self,
        query: &str,
        bookmarks_only: bool,
        limit: usize,
    ) -> Result<Vec<LibraryEntry>> {
        let db = self.connect()?;
        let sql = if bookmarks_only {
            "SELECT url,title,folder FROM bookmarks WHERE instr(lower(url || ' ' || title),lower(?1))>0 ORDER BY title LIMIT ?2"
        } else {
            "SELECT url,title,'' FROM history WHERE instr(lower(url || ' ' || title),lower(?1))>0 ORDER BY visited DESC,visits DESC LIMIT ?2"
        };
        Ok(db
            .prepare(sql)?
            .query_map(params![query, limit as i64], |r| {
                Ok(LibraryEntry {
                    url: r.get(0)?,
                    title: r.get(1)?,
                    folder: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn form_values(&self) -> Result<Vec<(String, String)>> {
        let db = self.connect()?;
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='form_information')",
            [],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(Vec::new());
        }
        Ok(db
            .prepare("SELECT name,value FROM form_information ORDER BY name,value")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn save_session(&self, session: &Session) -> Result<()> {
        self.connect()?.execute(
            "INSERT INTO session VALUES(1,?1)
            ON CONFLICT(id) DO UPDATE SET value=excluded.value",
            [serde_json::to_string(session)?],
        )?;
        Ok(())
    }

    pub fn session(&self) -> Result<Session> {
        let value: Option<String> = self
            .connect()?
            .query_row("SELECT value FROM session WHERE id=1", [], |r| r.get(0))
            .optional()?;
        value
            .map(|v| serde_json::from_str(&v).context("Could not read the saved session"))
            .unwrap_or(Ok(Session::default()))
    }
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
