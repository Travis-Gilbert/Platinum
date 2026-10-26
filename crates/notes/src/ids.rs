//! Stable note ids: `.theorem/ids.json` maps each path to a `vf_<ULID>`, so a note keeps its identity when it moves.
use anyhow::{Context, Result, bail};
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static NOTE_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^vf_[0-9A-HJKMNP-TV-Z]{26}$").unwrap());

/// Whether `id` has the form Theorem gives ids (`vf_` and a ULID). Only those are honoured from a note's
/// `theorem-id` property.
pub fn is_note_id(id: &str) -> bool {
    NOTE_ID.is_match(id)
}

fn new_id() -> String {
    format!("vf_{}", ulid::Ulid::new())
}

/// The path-to-id map in `.theorem/ids.json`, sorted by path so it diffs cleanly in git. Ids are never
/// written into the notes.
#[derive(Debug)]
pub struct IdStore {
    file: PathBuf,
    by_path: BTreeMap<String, String>,
    by_id: HashMap<String, String>,
    dirty: bool,
}

impl IdStore {
    pub const FILE: &str = ".theorem/ids.json";

    /// Reads the store of the vault at `root`; a vault with no file starts empty. An entry that is not a
    /// well-formed id, or repeats an id already taken by an earlier path, is dropped.
    pub fn load(root: &Path) -> Result<Self> {
        let file = root.join(Self::FILE);
        let mut store = Self {
            file,
            by_path: BTreeMap::new(),
            by_id: HashMap::new(),
            dirty: false,
        };
        let text = match std::fs::read_to_string(&store.file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(store),
            Err(e) => return Err(e).with_context(|| format!("reading {}", store.file.display())),
        };
        let Value::Object(entries) = serde_json::from_str(&text)
            .with_context(|| format!("{} is not valid JSON", store.file.display()))?
        else {
            bail!("{} is not a map of paths to ids", store.file.display());
        };
        let sorted: BTreeMap<_, _> = entries.into_iter().collect();
        for (path, id) in sorted {
            if let Some(id) = id
                .as_str()
                .filter(|id| is_note_id(id) && !store.by_id.contains_key(*id))
            {
                store.assign(&path, id.to_string());
            }
        }
        store.dirty = false;
        Ok(store)
    }

    /// Every path with an id, in path order.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.by_path.keys().map(String::as_str)
    }

    pub fn get(&self, path: &str) -> Option<&str> {
        self.by_path.get(path).map(String::as_str)
    }

    pub fn path_of(&self, id: &str) -> Option<&str> {
        self.by_id.get(id).map(String::as_str)
    }

    /// The id of `path`. One that has none gets `preferred` (the `theorem-id` of its properties) when that is
    /// well-formed and no other path holds it, else a new id.
    pub fn ensure(&mut self, path: &str, preferred: Option<&str>) -> &str {
        if !self.by_path.contains_key(path) {
            let id = match preferred {
                Some(id) if is_note_id(id) && !self.by_id.contains_key(id) => id.to_string(),
                _ => new_id(),
            };
            self.assign(path, id);
        }
        &self.by_path[path]
    }

    /// Moves an id from one path to another; the note keeps it. A path with no id gets a new one. Whatever id
    /// `to` had is dropped.
    pub fn rename(&mut self, from: &str, to: &str) -> &str {
        let id = self.by_path.get(from).cloned().unwrap_or_else(new_id);
        self.delete(from);
        self.assign(to, id);
        &self.by_path[to]
    }

    pub fn delete(&mut self, path: &str) {
        if let Some(id) = self.by_path.remove(path) {
            self.by_id.remove(&id);
            self.dirty = true;
        }
    }

    /// Whether the file is out of date.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn assign(&mut self, path: &str, id: String) {
        if let Some(previous) = self.by_path.insert(path.to_string(), id.clone()) {
            self.by_id.remove(&previous);
        }
        self.by_id.insert(id, path.to_string());
        self.dirty = true;
    }

    /// Writes the file if anything changed: sorted, indented by two spaces, ending in a newline, and
    /// replaced atomically so a crash leaves the old file or the new one.
    pub fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let dir = self.file.parent().context("ids file has no folder")?;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let mut json = serde_json::to_string_pretty(&self.by_path)?;
        json.push('\n');
        let mut temp = tempfile::NamedTempFile::new_in(dir)?;
        temp.write_all(json.as_bytes())?;
        temp.as_file().sync_all()?;
        temp.persist(&self.file)
            .with_context(|| format!("replacing {}", self.file.display()))?;
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const A: &str = "vf_01J9ZK3Q4M5N6P7R8S9T0V1W2X";
    const B: &str = "vf_01J9ZK3Q4M5N6P7R8S9T0V1W2Y";

    fn vault(ids: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        if let Some(ids) = ids {
            std::fs::create_dir(dir.path().join(".theorem")).unwrap();
            std::fs::write(dir.path().join(IdStore::FILE), ids).unwrap();
        }
        dir
    }

    #[test]
    fn ids_are_vf_and_a_ulid() {
        assert!(is_note_id(A));
        for bad in [
            "",
            "vf_",
            "abc",
            "vf_01J9ZK3Q4M5N6P7R8S9T0V1W2",
            "vf_01J9ZK3Q4M5N6P7R8S9T0V1W2XX",
            "vf_01J9ZK3Q4M5N6P7R8S9T0V1W2U",
            "vf_01j9zk3q4m5n6p7r8s9t0v1w2x",
            "ws_01J9ZK3Q4M5N6P7R8S9T0V1W2X",
        ] {
            assert!(!is_note_id(bad), "{bad}");
        }
        assert!(is_note_id(&new_id()));
        assert_ne!(new_id(), new_id());
    }

    #[test]
    fn an_id_is_made_once_and_kept() {
        let dir = vault(None);
        let mut store = IdStore::load(dir.path()).unwrap();
        assert!(!store.is_dirty());
        let id = store.ensure("a/Note.md", None).to_string();
        assert!(is_note_id(&id));
        assert_eq!(store.ensure("a/Note.md", None), id);
        assert_eq!(store.ensure("a/Note.md", Some(A)), id);
        assert_eq!(store.get("a/Note.md"), Some(id.as_str()));
        assert_eq!(store.path_of(&id), Some("a/Note.md"));
        assert_eq!(store.get("other.md"), None);
        assert!(store.is_dirty());
    }

    #[test]
    fn a_theorem_id_in_the_note_is_honoured_when_it_is_free() {
        let dir = vault(None);
        let mut store = IdStore::load(dir.path()).unwrap();
        assert_eq!(store.ensure("one.md", Some(A)), A);
        // Taken by another path: the second note gets a new id.
        let second = store.ensure("two.md", Some(A)).to_string();
        assert_ne!(second, A);
        assert!(is_note_id(&second));
        // Not Theorem's form: ignored.
        let third = store.ensure("three.md", Some("abc")).to_string();
        assert!(is_note_id(&third) && third != "abc");
    }

    #[test]
    fn a_moved_note_keeps_its_id() {
        let dir = vault(None);
        let mut store = IdStore::load(dir.path()).unwrap();
        let id = store.ensure("old.md", Some(A)).to_string();
        assert_eq!(store.rename("old.md", "new/Name.md"), id);
        assert_eq!(store.get("old.md"), None);
        assert_eq!(store.path_of(&id), Some("new/Name.md"));
        // A note that had no id gets one at its new path.
        let fresh = store.rename("never.md", "now.md").to_string();
        assert!(is_note_id(&fresh) && store.get("never.md").is_none());
        // Moving onto a path that has an id replaces it.
        let other = store.ensure("target.md", Some(B)).to_string();
        store.rename("now.md", "target.md");
        assert_eq!(store.get("target.md"), Some(fresh.as_str()));
        assert_eq!(store.path_of(&other), None);
    }

    #[test]
    fn deleting_frees_the_id() {
        let dir = vault(None);
        let mut store = IdStore::load(dir.path()).unwrap();
        store.ensure("a.md", Some(A));
        store.delete("a.md");
        store.delete("a.md");
        assert_eq!(store.path_of(A), None);
        assert_eq!(store.ensure("b.md", Some(A)), A);
    }

    #[test]
    fn the_file_is_sorted_indented_and_replaced_atomically() {
        let dir = vault(None);
        let mut store = IdStore::load(dir.path()).unwrap();
        store.ensure("b/Two.md", Some(B));
        store.ensure("a/One.md", Some(A));
        store.save().unwrap();
        assert!(!store.is_dirty());
        let text = std::fs::read_to_string(dir.path().join(IdStore::FILE)).unwrap();
        assert_eq!(
            text,
            format!("{{\n  \"a/One.md\": \"{A}\",\n  \"b/Two.md\": \"{B}\"\n}}\n")
        );
        // No temporary file is left beside it.
        let names: Vec<_> = std::fs::read_dir(dir.path().join(".theorem"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["ids.json"]);

        let again = IdStore::load(dir.path()).unwrap();
        assert!(!again.is_dirty());
        assert_eq!(again.paths().collect::<Vec<_>>(), ["a/One.md", "b/Two.md"]);
        assert_eq!(again.get("b/Two.md"), Some(B));
        assert_eq!(again.path_of(A), Some("a/One.md"));
    }

    #[test]
    fn saving_with_nothing_changed_writes_nothing() {
        let dir = vault(None);
        let mut store = IdStore::load(dir.path()).unwrap();
        store.save().unwrap();
        assert!(!dir.path().join(".theorem").exists());
        store.ensure("a.md", None);
        store.delete("a.md");
        store.save().unwrap();
        let text = std::fs::read_to_string(dir.path().join(IdStore::FILE)).unwrap();
        assert_eq!(text, "{}\n");
    }

    #[test]
    fn loading_drops_malformed_and_repeated_ids() {
        let ids = format!(r#"{{"a.md":"{A}","b.md":"{A}","c.md":"nope","d.md":7,"e.md":"{B}"}}"#);
        let dir = vault(Some(&ids));
        let store = IdStore::load(dir.path()).unwrap();
        assert_eq!(store.paths().collect::<Vec<_>>(), ["a.md", "e.md"]);
        assert_eq!(store.path_of(A), Some("a.md"));
        assert!(!store.is_dirty());
    }

    #[test]
    fn a_file_that_is_not_a_map_is_an_error_not_a_reset() {
        for bad in ["{", "[]", "\"x\"", ""] {
            let dir = vault(Some(bad));
            let message = IdStore::load(dir.path()).unwrap_err().to_string();
            assert!(message.contains("ids.json"), "{bad}: {message}");
        }
    }
}
