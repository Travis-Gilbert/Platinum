//! One writer per installation, shared by profile clones and background tasks.
//! RustyRed owns transactions, durability and full-text indexing; this adapter
//! defines browser record identities and ranking only.
use super::*;
use anyhow::{Context, Result, ensure};
use rustyred::engine::NodeQuery;
use rustyred::{Database, GraphMutation, GraphMutationBatch, NodeRecord, OpenOptions};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{MutexGuard, OnceLock, Weak};

type Owners = HashMap<PathBuf, Weak<Mutex<Database>>>;
static OWNERS: OnceLock<Mutex<Owners>> = OnceLock::new();

pub(super) fn node(profile: i64, label: &str, key: &str, mut value: Value) -> NodeRecord {
    value["profile"] = json!(profile);
    NodeRecord::new(
        format!("{profile}:{label}:{:x}", Sha256::digest(key.as_bytes())),
        [label],
        value,
    )
}
fn records(db: &Database, profile: i64, label: &str) -> Result<Vec<NodeRecord>> {
    Ok(db
        .engine()
        .query_nodes(NodeQuery::label(label).with_property("profile", json!(profile)))?)
}
fn value(db: &Database, id: &str) -> Result<Option<Value>> {
    Ok(db.engine().get_node(id)?.map(|n| n.properties))
}

fn shared_database(root: &Path) -> Result<Arc<Mutex<Database>>> {
    std::fs::create_dir_all(root)?;
    let canonical = root.canonicalize()?;
    let mut owners = OWNERS
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| anyhow::anyhow!("Profile owner registry unavailable"))?;
    owners.retain(|_, owner| owner.strong_count() > 0);
    if let Some(db) = owners.get(&canonical).and_then(Weak::upgrade) {
        return Ok(db);
    }
    let db = Arc::new(Mutex::new(open_database(&canonical)?));
    owners.insert(canonical, Arc::downgrade(&db));
    Ok(db)
}

fn open_database(root: &Path) -> Result<Database> {
    let path = root.join("browser.RR");
    let options = OpenOptions::new()
        .device_name("platinum")
        .control_events(false)
        .keep_history(false);
    let mut db = if path.exists() {
        Database::open_with(&path, &options)?
    } else {
        Database::create_with(&path, &options)?
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    // The completion marker and all old records share one atomic commit.
    // Failed or interrupted migrations cannot activate partial data.
    super::legacy::migrate(root, &mut db)?;
    index_library(&mut db)?;
    Ok(db)
}

fn index_library(db: &mut Database) -> Result<()> {
    db.designate_fulltext("Bookmark", &[("title", 3.0), ("url", 2.0)])?;
    db.designate_fulltext("History", &[("title", 3.0), ("url", 2.0)])?;
    Ok(())
}

fn active_profile(database: &Mutex<Database>) -> Result<Profile> {
    let db = database
        .lock()
        .map_err(|_| anyhow::anyhow!("Profile database unavailable"))?;
    let active = value(&db, "active")?
        .and_then(|v| v["id"].as_i64())
        .unwrap_or(1);
    Ok(serde_json::from_value(
        value(&db, &format!("profile:{active}"))?.context("Unknown active profile")?,
    )?)
}

pub(super) fn profile_directory(root: &Path, id: i64) -> PathBuf {
    if id == 1 {
        root.join("profile")
    } else {
        root.join("profiles").join(id.to_string())
    }
}

fn search_scores(
    db: &Database,
    nodes: &[NodeRecord],
    label: &str,
    query: &str,
) -> Result<HashMap<String, f32>> {
    if query.is_empty() {
        return Ok(HashMap::new());
    }
    let within: Vec<_> = nodes.iter().map(|n| n.id.clone()).collect();
    Ok(db
        .fulltext_search(label, query, within.len().max(1), Some(&within))?
        .into_iter()
        .map(|h| (h.id, h.score))
        .collect())
}

fn exact_address(n: &NodeRecord, query: &str) -> bool {
    n.properties["url"].as_str().is_some_and(|url| {
        url.trim_end_matches('/')
            .eq_ignore_ascii_case(query.trim_end_matches('/'))
            || url
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_end_matches('/')
                .eq_ignore_ascii_case(query.trim_end_matches('/'))
    })
}

impl ProfileStore {
    pub fn open(root: &Path) -> Result<Self> {
        let database = shared_database(root)?;
        let profile = active_profile(&database)?;
        let store = Self {
            root: root.to_owned(),
            profile,
            database,
        };
        store.ensure_directory()?;
        Ok(store)
    }
    fn db(&self) -> Result<MutexGuard<'_, Database>> {
        self.database
            .lock()
            .map_err(|_| anyhow::anyhow!("Profile database unavailable"))
    }
    fn ensure_directory(&self) -> Result<()> {
        let directory = self.directory();
        std::fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
    pub fn profiles(&self) -> Result<Vec<Profile>> {
        let db = self.db()?;
        let mut profiles = db
            .engine()
            .query_nodes(NodeQuery::label("Profile"))?
            .into_iter()
            .map(|n| serde_json::from_value(n.properties))
            .collect::<Result<Vec<Profile>, _>>()?;
        profiles.sort_by_key(|p| p.id);
        Ok(profiles)
    }
    pub fn imported_profile(&self, source: &str, name: &str, email: &str) -> Result<Self> {
        let mut db = self.db()?;
        let profiles = db.engine().query_nodes(NodeQuery::label("Profile"))?;
        let id = profiles
            .iter()
            .find(|n| n.properties["source"] == source)
            .and_then(|n| n.properties["id"].as_i64())
            .unwrap_or_else(|| {
                profiles
                    .iter()
                    .filter_map(|n| n.properties["id"].as_i64())
                    .max()
                    .unwrap_or(0)
                    + 1
            });
        let profile = Profile {
            id,
            name: name.into(),
            email: email.into(),
        };
        db.upsert_node(NodeRecord::new(
            format!("profile:{id}"),
            ["Profile"],
            json!({"id":id,"name":name,"email":email,"source":source}),
        ))?;
        drop(db);
        let store = Self {
            root: self.root.clone(),
            profile,
            database: self.database.clone(),
        };
        store.ensure_directory()?;
        Ok(store)
    }
    pub fn select(&self, id: i64) -> Result<()> {
        let mut db = self.db()?;
        ensure!(
            value(&db, &format!("profile:{id}"))?.is_some(),
            "Unknown profile"
        );
        db.upsert_node(NodeRecord::new("active", ["Setting"], json!({"id":id})))?;
        Ok(())
    }
    pub fn directory(&self) -> PathBuf {
        profile_directory(&self.root, self.profile.id)
    }
    pub fn search_directory(&self) -> PathBuf {
        if self.profile.id == 1 {
            self.root.clone()
        } else {
            self.directory().join("search")
        }
    }
    pub fn import_batch(&self) -> Result<ImportBatch<'_>> {
        Ok(ImportBatch {
            db: self.db()?,
            profile: self.profile.id,
            pending: BTreeMap::new(),
        })
    }
    pub fn bookmark(&self, url: &str, title: &str) -> Result<()> {
        let url = web_url(url).context("Only web addresses can be bookmarked")?;
        let record = node(
            self.profile.id,
            "Bookmark",
            &serde_json::to_string(&(url.as_str(), "Bookmarks"))?,
            json!({"url":url.as_str(),"title":title,"folder":"Bookmarks"}),
        );
        self.db()?.upsert_node(record)?;
        Ok(())
    }
    pub fn visit(&self, url: &str, title: &str) -> Result<()> {
        self.visit_at(url, title, now())
    }
    /// Capture time at navigation, so a queued write cannot undo Clear History.
    pub fn visit_at(&self, url: &str, title: &str, visited: i64) -> Result<()> {
        let Some(url) = web_url(url) else {
            return Ok(());
        };
        let mut db = self.db()?;
        let cleared = node(self.profile.id, "HistoryCleared", "current", json!({})).id;
        if value(&db, &cleared)?
            .and_then(|v| v["at"].as_i64())
            .is_some_and(|at| visited <= at)
        {
            return Ok(());
        }

        let mut record = node(
            self.profile.id,
            "History",
            url.as_str(),
            json!({"url":url.as_str(),"title":title,"visited":visited,"visits":1}),
        );
        if let Some(old) = value(&db, &record.id)? {
            record.properties["visits"] =
                json!(old["visits"].as_i64().unwrap_or(0).saturating_add(1));
        }
        db.upsert_node(record)?;
        Ok(())
    }
    pub fn suggestions(
        &self,
        query: &str,
        bookmarks_only: bool,
        limit: usize,
    ) -> Result<Vec<LibraryEntry>> {
        if limit == 0 {
            return Ok(vec![]);
        }
        let db = self.db()?;
        let label = if bookmarks_only {
            "Bookmark"
        } else {
            "History"
        };
        let mut nodes = records(&db, self.profile.id, label)?;
        let query = query.trim().to_lowercase();
        let scores = search_scores(&db, &nodes, label, &query)?;
        // Literal URL substrings also match punctuation and partial host/path
        // fragments which a word tokenizer cannot represent.
        nodes.retain(|n| {
            query.is_empty()
                || scores.contains_key(&n.id)
                || n.properties["url"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&query)
        });
        nodes.sort_by(|a, b| {
            exact_address(b, &query)
                .cmp(&exact_address(a, &query))
                .then_with(|| {
                    scores
                        .get(&b.id)
                        .unwrap_or(&0.0)
                        .total_cmp(scores.get(&a.id).unwrap_or(&0.0))
                })
                .then_with(|| {
                    b.properties["visited"]
                        .as_i64()
                        .cmp(&a.properties["visited"].as_i64())
                })
                .then_with(|| {
                    b.properties["visits"]
                        .as_i64()
                        .cmp(&a.properties["visits"].as_i64())
                })
                .then_with(|| {
                    a.properties["title"]
                        .as_str()
                        .cmp(&b.properties["title"].as_str())
                })
                .then_with(|| a.id.cmp(&b.id))
        });
        nodes
            .into_iter()
            .take(limit)
            .map(|n| {
                Ok(LibraryEntry {
                    url: n.properties["url"]
                        .as_str()
                        .context("Missing address")?
                        .into(),
                    title: n.properties["title"].as_str().unwrap_or("").into(),
                    folder: n.properties["folder"].as_str().unwrap_or("").into(),
                })
            })
            .collect()
    }
    pub fn history_metadata(&self, url: &str) -> Result<Option<(i64, i64)>> {
        let id = node(self.profile.id, "History", url, json!({})).id;
        Ok(value(&*self.db()?, &id)?.map(|v| {
            (
                v["visited"].as_i64().unwrap_or(0),
                v["visits"].as_i64().unwrap_or(0),
            )
        }))
    }
    pub fn clear_history(&self) -> Result<()> {
        let mut db = self.db()?;
        // Remove the legacy rollback copy first. If this fails, report failure
        // rather than promising that the migration copy was forgotten.
        super::legacy::clear_history(&self.directory())?;
        let deletes = records(&db, self.profile.id, "History")?
            .into_iter()
            .map(|n| {
                // A normal logical delete retains the old payload in its
                // tombstone. Replace it with an empty tombstone so even the
                // engine's diagnostic read cannot recover the title or URL.
                let mut forgotten = NodeRecord::new(n.id, Vec::<String>::new(), json!({}));
                forgotten.tombstone = true;
                GraphMutation::NodeUpsert(forgotten)
            });
        db.mutate(GraphMutationBatch::new(deletes.chain(std::iter::once(
            GraphMutation::NodeUpsert(node(
                self.profile.id,
                "HistoryCleared",
                "current",
                json!({"at":now()}),
            )),
        ))))?;
        db.compact()?;
        Ok(())
    }
    pub fn form_values(&self) -> Result<Vec<(String, String)>> {
        self.pairs("Form", "name", "value")
    }
    pub fn extensions(&self) -> Result<Vec<(String, String)>> {
        self.pairs("Extension", "name", "version")
    }
    pub fn credential_labels(&self) -> Result<Vec<(String, String)>> {
        self.pairs("Credential", "origin", "username")
    }
    fn pairs(&self, label: &str, a: &str, b: &str) -> Result<Vec<(String, String)>> {
        let mut pairs: Vec<_> = records(&*self.db()?, self.profile.id, label)?
            .into_iter()
            .map(|n| {
                (
                    n.properties[a].as_str().unwrap_or("").into(),
                    n.properties[b].as_str().unwrap_or("").into(),
                )
            })
            .collect();
        pairs.sort();
        Ok(pairs)
    }
    pub fn save_credential_label(&self, origin: &str, username: &str) -> Result<()> {
        self.db()?.upsert_node(node(
            self.profile.id,
            "Credential",
            &serde_json::to_string(&(origin, username))?,
            json!({"origin":origin,"username":username}),
        ))?;
        Ok(())
    }
    pub fn delete_credential_label(&self, origin: &str, username: &str) -> Result<()> {
        let id = node(
            self.profile.id,
            "Credential",
            &serde_json::to_string(&(origin, username))?,
            json!({}),
        )
        .id;
        self.db()?
            .mutate(GraphMutationBatch::new([GraphMutation::NodeDelete(id)]))?;
        Ok(())
    }
    pub fn import_count(&self) -> Result<usize> {
        Ok(records(&*self.db()?, self.profile.id, "Import")?.len())
    }
    pub fn log_import(
        &self,
        source: &str,
        category: &str,
        completed: i64,
        imported: usize,
        skipped: usize,
    ) -> Result<()> {
        let mut db = self.db()?;
        let id = format!("{}:{}", now(), db.stats()?.version);
        db.upsert_node(node(self.profile.id,"Import",&id,json!({"source":source,"category":category,"completed":completed,"imported":imported,"skipped":skipped})))?;
        Ok(())
    }
    pub fn save_session(&self, session: &Session) -> Result<()> {
        self.db()?.upsert_node(node(
            self.profile.id,
            "Session",
            "current",
            json!({"value":session}),
        ))?;
        Ok(())
    }
    pub fn session(&self) -> Result<Session> {
        let id = node(self.profile.id, "Session", "current", json!({})).id;
        value(&*self.db()?, &id)?
            .map(|v| {
                serde_json::from_value(v["value"].clone()).context("Could not read saved session")
            })
            .unwrap_or(Ok(Session::default()))
    }
}

/// A category import is one atomic mutation batch, including retry deduplication.
pub struct ImportBatch<'a> {
    db: MutexGuard<'a, Database>,
    profile: i64,
    pending: BTreeMap<String, NodeRecord>,
}
impl ImportBatch<'_> {
    fn insert(&mut self, record: NodeRecord) -> Result<usize> {
        if self.pending.contains_key(&record.id) || value(&self.db, &record.id)?.is_some() {
            return Ok(0);
        }
        self.pending.insert(record.id.clone(), record);
        Ok(1)
    }
    pub fn bookmark(&mut self, url: &str, title: &str, folder: &str) -> Result<usize> {
        self.insert(node(
            self.profile,
            "Bookmark",
            &serde_json::to_string(&(url, folder))?,
            json!({"url":url,"title":title,"folder":folder}),
        ))
    }
    pub fn history(&mut self, url: &str, title: &str, visited: i64, visits: i64) -> Result<usize> {
        let mut record = node(
            self.profile,
            "History",
            url,
            json!({"url":url,"title":title,"visited":visited,"visits":visits}),
        );
        let old = self
            .pending
            .get(&record.id)
            .map(|n| n.properties.clone())
            .or(value(&self.db, &record.id)?);
        if let Some(old) = old {
            let old_time = old["visited"].as_i64().unwrap_or(0);
            let old_visits = old["visits"].as_i64().unwrap_or(0);
            if visited <= old_time && visits <= old_visits {
                return Ok(0);
            }
            record.properties["visited"] = json!(visited.max(old_time));
            record.properties["visits"] = json!(visits.max(old_visits));
        }
        self.pending.insert(record.id.clone(), record);
        Ok(1)
    }
    pub fn extension(&mut self, id: &str, name: &str, version: &str) -> Result<usize> {
        self.insert(node(
            self.profile,
            "Extension",
            id,
            json!({"id":id,"name":name,"version":version}),
        ))
    }
    pub fn form(&mut self, name: &str, value: &str) -> Result<usize> {
        self.insert(node(
            self.profile,
            "Form",
            &serde_json::to_string(&(name, value))?,
            json!({"name":name,"value":value}),
        ))
    }
    pub fn commit(mut self) -> Result<()> {
        if !self.pending.is_empty() {
            self.db.mutate(GraphMutationBatch::new(
                self.pending.into_values().map(GraphMutation::NodeUpsert),
            ))?;
        }
        Ok(())
    }
}
