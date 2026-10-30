//! One-way SQLite migration. Sources are snapshotted with SQLite's backup API,
//! including committed WAL data. The completion marker shares the graph commit.
use crate::{Session, migration::snapshot, storage::node};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, types::ValueRef};
use rustyred::{Database, GraphMutation, GraphMutationBatch, NodeRecord};
use serde_json::{Map, Value, json};
use std::path::Path;

struct LegacyProfile {
    id: i64,
    name: String,
    email: String,
    source: Option<String>,
}

impl LegacyProfile {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            name: row.get(1)?,
            email: row.get(2)?,
            source: row.get(3)?,
        })
    }
}

pub fn migrate(root: &Path, db: &mut Database) -> Result<()> {
    if let Some(marker) = db.engine().get_node("schema")? {
        ensure!(
            marker.properties["version"] == 1,
            "Unsupported browser database schema"
        );
        return Ok(());
    }
    let (profiles, active) = catalog(root)?;
    ensure!(
        profiles.iter().any(|p| p.id == active),
        "Legacy active profile does not exist"
    );
    let mut nodes = Vec::new();
    for profile in profiles {
        nodes.extend(profile_records(root, profile)?);
    }
    nodes.push(NodeRecord::new("active", ["Setting"], json!({"id":active})));
    nodes.push(NodeRecord::new("schema", ["Setting"], json!({"version":1})));
    db.mutate(GraphMutationBatch::new(
        nodes.into_iter().map(GraphMutation::NodeUpsert),
    ))?;
    Ok(())
}

fn catalog(root: &Path) -> Result<(Vec<LegacyProfile>, i64)> {
    let path = root.join("profiles.sqlite3");
    if !path.exists() {
        return Ok((
            vec![LegacyProfile {
                id: 1,
                name: "Default".into(),
                email: String::new(),
                source: None,
            }],
            1,
        ));
    }
    let old = snapshot(&path)?;
    let profiles = old
        .prepare("SELECT id,name,email,source FROM profiles ORDER BY id")?
        .query_map([], LegacyProfile::from_row)?
        .collect::<rusqlite::Result<_>>()?;
    let active = old
        .query_row("SELECT value FROM settings WHERE key='active'", [], |r| {
            r.get(0)
        })
        .optional()?
        .unwrap_or(1);
    Ok((profiles, active))
}

fn profile_records(root: &Path, profile: LegacyProfile) -> Result<Vec<NodeRecord>> {
    let LegacyProfile {
        id,
        name,
        email,
        source,
    } = profile;
    let mut nodes = vec![NodeRecord::new(
        format!("profile:{id}"),
        ["Profile"],
        json!({"id":id,"name":name,"email":email,"source":source}),
    )];
    let path = super::storage::profile_directory(root, id).join("browser.sqlite3");
    if !path.exists() {
        return Ok(nodes);
    }
    let old = snapshot(&path)?;
    for (table, label, keys) in [
        ("bookmarks", "Bookmark", &["url", "folder"][..]),
        ("history", "History", &["url"][..]),
        ("credentials", "Credential", &["origin", "username"][..]),
        ("extensions", "Extension", &["id"][..]),
        ("imports", "Import", &["id"][..]),
        ("form_information", "Form", &["name", "value"][..]),
        ("session", "Session", &["id"][..]),
    ] {
        nodes.extend(table_records(&old, id, table, label, keys)?);
    }
    Ok(nodes)
}

fn table_records(
    old: &Connection,
    profile: i64,
    table: &str,
    label: &str,
    keys: &[&str],
) -> Result<Vec<NodeRecord>> {
    let exists: bool = old.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(Vec::new());
    }
    let mut statement = old.prepare(&format!("SELECT * FROM {table}"))?;
    let columns: Vec<_> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut rows = statement.query([])?;
    let mut nodes = Vec::new();
    while let Some(row) = rows.next()? {
        let mut props = row_properties(row, &columns)?;
        let key = record_key(label, keys, &mut props)?;
        nodes.push(node(profile, label, &key, Value::Object(props)));
    }
    Ok(nodes)
}

fn row_properties(row: &rusqlite::Row<'_>, columns: &[String]) -> Result<Map<String, Value>> {
    columns
        .iter()
        .enumerate()
        .map(|(index, column)| Ok((column.clone(), json_value(row.get_ref(index)?)?)))
        .collect()
}

fn json_value(value: ValueRef<'_>) -> Result<Value> {
    Ok(match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(n) => json!(n),
        ValueRef::Real(n) => json!(n),
        ValueRef::Text(s) => json!(std::str::from_utf8(s)?),
        ValueRef::Blob(_) => anyhow::bail!("Unexpected binary field in legacy browser data"),
    })
}

fn record_key(label: &str, keys: &[&str], props: &mut Map<String, Value>) -> Result<String> {
    if label == "Session" {
        let session: Session =
            serde_json::from_str(props["value"].as_str().context("Invalid legacy session")?)?;
        props.insert("value".into(), serde_json::to_value(session)?);
        return Ok("current".into());
    }
    if let [key] = keys {
        return Ok(props[*key]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| props[*key].to_string()));
    }
    Ok(serde_json::to_string(
        &keys.iter().map(|k| &props[*k]).collect::<Vec<_>>(),
    )?)
}

pub fn clear_history(directory: &Path) -> Result<()> {
    let path = directory.join("browser.sqlite3");
    if !path.exists() {
        return Ok(());
    }
    let db = Connection::open(path)?;
    db.busy_timeout(std::time::Duration::from_secs(3))?;
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='history')",
        [],
        |r| r.get(0),
    )?;
    if exists {
        db.execute_batch("PRAGMA secure_delete=ON; DELETE FROM history; PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")?;
    }
    Ok(())
}
