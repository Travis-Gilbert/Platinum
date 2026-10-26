//! A rebuildable RedCore projection. File bytes and authority revisions remain the source of truth.
use crate::{
    FileIndex, FileKind, Link, Note, PropertyType, Target, VaultSettings, canvas_links, parse_note,
};
use anyhow::Result;
use rustyred_thg_core::{
    EdgeRecord, GraphMutation, GraphMutationBatch, GraphRead, NodeQuery, NodeRecord,
    RedCoreDurability, RedCoreGraphStore, RedCoreOptions,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) fn store(path: &Path) -> Result<RedCoreGraphStore> {
    Ok(RedCoreGraphStore::open(
        path,
        RedCoreOptions {
            durability: RedCoreDurability::AofAlways,
            ..Default::default()
        },
    )?)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexedLink {
    pub link: Link,
    pub target: Target,
    pub canvas_node: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexedFile {
    pub path: String,
    pub id: Option<String>,
    pub kind: FileKind,
    pub excluded: bool,
    pub hash: String,
    pub note: Option<Note>,
    pub links: Vec<IndexedLink>,
    pub properties: BTreeMap<String, PropertyType>,
    pub words: usize,
    pub characters: usize,
    pub error: Option<String>,
}

impl IndexedFile {
    pub(crate) fn resolution_changes(&self, names: &FileIndex) -> bool {
        self.links
            .iter()
            .any(|link| names.target_of(&self.path, &link.link) != link.target)
    }
}

pub type LinkMap = BTreeMap<String, BTreeMap<String, usize>>;

/// Queries use the same materialized records persisted in RedCore. Exclusions are flags, never deletions.
pub struct VaultIndex {
    graph: RedCoreGraphStore,
    files: BTreeMap<String, IndexedFile>,
    incoming: LinkMap,
    names: FileIndex,
    generation: u64,
    folders: BTreeSet<String>,
}

impl VaultIndex {
    pub fn open(data: &Path) -> Result<Self> {
        let graph = store(data)?;
        let mut files = BTreeMap::new();
        for node in
            GraphRead::query_nodes(&graph, NodeQuery::label("NotesFile").with_limit(usize::MAX))
        {
            let file: IndexedFile = serde_json::from_value(node.properties)?;
            files.insert(file.path.clone(), file);
        }
        let names = FileIndex::new(files.keys().cloned());
        let mut incoming = LinkMap::new();
        for file in files.values() {
            add_backlinks(&mut incoming, file);
        }
        let generation = graph
            .get_node_record("notes:meta")
            .and_then(|n| n.properties["generation"].as_u64())
            .unwrap_or_default();
        let folders = GraphRead::query_nodes(
            &graph,
            NodeQuery::label("NotesFolder").with_limit(usize::MAX),
        )
        .into_iter()
        .filter_map(|n| n.properties["path"].as_str().map(str::to_owned))
        .collect();
        Ok(Self {
            graph,
            files,
            incoming,
            names,
            generation,
            folders,
        })
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
    pub(crate) fn checkpoint(&mut self, generation: u64) -> Result<()> {
        if self.generation != generation {
            self.graph.upsert_node(NodeRecord::new(
                "notes:meta",
                ["NotesMeta"],
                json!({"generation": generation}),
            ))?;
            self.generation = generation;
        }
        Ok(())
    }

    pub fn files(&self) -> &BTreeMap<String, IndexedFile> {
        &self.files
    }
    pub fn names(&self) -> &FileIndex {
        &self.names
    }
    pub fn graph(&self) -> &RedCoreGraphStore {
        &self.graph
    }
    pub fn folders(&self) -> BTreeSet<String> {
        self.folders.clone()
    }
    pub(crate) fn update_folders(&mut self, folders: BTreeSet<String>) -> Result<()> {
        let mut mutations = Vec::new();
        for path in self.folders.difference(&folders) {
            mutations.push(GraphMutation::NodeDelete(format!("folder:{path}")));
        }
        for path in folders.difference(&self.folders) {
            mutations.push(GraphMutation::NodeUpsert(NodeRecord::new(
                format!("folder:{path}"),
                ["NotesFolder"],
                json!({"path":path}),
            )));
        }
        if !mutations.is_empty() {
            self.graph
                .commit_batch(GraphMutationBatch::new(mutations))?;
        }
        self.folders = folders;
        Ok(())
    }

    pub(crate) fn project(
        path: &str,
        text: Option<&str>,
        hash: String,
        id: Option<String>,
        settings: &VaultSettings,
    ) -> IndexedFile {
        let kind = FileKind::of(path);
        let note = (kind == FileKind::Note).then(|| parse_note(text.unwrap_or_default()));
        let mut error = None;
        let links = if let Some(note) = &note {
            note.links
                .iter()
                .cloned()
                .map(|link| (link, None))
                .collect::<Vec<_>>()
        } else if kind == FileKind::Canvas {
            match canvas_links(text.unwrap_or_default()) {
                Ok(links) => links.into_iter().map(|c| (c.link, Some(c.node))).collect(),
                Err(e) => {
                    error = Some(e.to_string());
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        if kind == FileKind::Base {
            error = crate::vault::validate(path, text.unwrap_or_default())
                .err()
                .map(|e| e.to_string());
        }
        let properties = note
            .as_ref()
            .and_then(|n| n.frontmatter.as_ref())
            .map(|fm| {
                fm.properties
                    .iter()
                    .map(|(k, v)| (k.clone(), settings.property_type(k, v)))
                    .collect()
            })
            .unwrap_or_default();
        IndexedFile {
            path: path.into(),
            id,
            kind,
            excluded: settings.is_excluded(path),
            hash,
            note,
            links: links
                .into_iter()
                .map(|(link, canvas_node)| IndexedLink {
                    target: Target::Unresolved {
                        key: String::new(),
                        name: link.target.clone(),
                    },
                    link,
                    canvas_node,
                })
                .collect(),
            properties,
            words: text.map_or(0, |t| t.split_whitespace().count()),
            characters: text.map_or(0, |t| t.encode_utf16().count()),
            error,
        }
    }

    /// One atomic graph batch; unchanged files are neither reparsed nor rewritten. A path-set change
    /// re-resolves cached links, since even a formerly unambiguous shortest name can change its target.
    pub(crate) fn update(&mut self, changed: Vec<IndexedFile>, deleted: &[String]) -> Result<()> {
        let mut names = self.names.clone();
        let mut structural = false;
        for path in deleted {
            structural |= names.remove(path);
        }
        let mut staged: BTreeMap<_, _> = changed
            .into_iter()
            .map(|file| (file.path.clone(), file))
            .collect();
        for path in staged.keys() {
            structural |= names.insert(path.clone());
        }
        if structural {
            for (path, file) in &self.files {
                if staged.contains_key(path) || deleted.contains(path) {
                    continue;
                }
                if file.resolution_changes(&names) {
                    staged.insert(path.clone(), file.clone());
                }
            }
        }
        for (path, file) in &mut staged {
            for link in &mut file.links {
                link.target = names.target_of(path, &link.link);
            }
        }
        staged.retain(|path, file| self.files.get(path) != Some(file));
        let mut mutations = Vec::new();
        for path in deleted.iter().chain(staged.keys()) {
            if let Some(old) = self.files.get(path) {
                remove_projection(old, deleted.contains(path), &mut mutations)?;
            }
        }
        for file in staged.values() {
            add_projection(file, &mut mutations)?;
        }
        for (path, file) in &staged {
            for (i, link) in file.links.iter().enumerate() {
                if let Target::File(target) = &link.target {
                    mutations.push(GraphMutation::EdgeUpsert(EdgeRecord::new(
                        format!("link:{path}:{i}"),
                        format!("file:{path}"),
                        if link.link.embed {
                            "EMBEDS"
                        } else {
                            "LINKS_TO"
                        },
                        format!("file:{target}"),
                        serde_json::to_value(link)?,
                    )));
                }
            }
        }
        if !mutations.is_empty() {
            self.graph
                .commit_batch(GraphMutationBatch::new(mutations))?;
        }
        for path in deleted.iter().chain(staged.keys()) {
            if let Some(old) = self.files.get(path) {
                for link in &old.links {
                    if let Target::File(target) = &link.target
                        && let Some(sources) = self.incoming.get_mut(target)
                    {
                        sources.remove(path);
                        if sources.is_empty() {
                            self.incoming.remove(target);
                        }
                    }
                }
            }
        }
        for file in staged.values() {
            add_backlinks(&mut self.incoming, file);
        }
        for path in deleted {
            self.files.remove(path);
        }
        self.files.extend(staged);
        self.names = names;
        Ok(())
    }

    pub fn resolved_links(&self) -> LinkMap {
        self.files
            .iter()
            .filter(|(_, f)| matches!(f.kind, FileKind::Note | FileKind::Canvas))
            .map(|(path, file)| {
                let mut links = BTreeMap::new();
                for link in &file.links {
                    if let Target::File(target) = &link.target {
                        *links.entry(target.clone()).or_default() += 1;
                    }
                }
                (path.clone(), links)
            })
            .collect()
    }
    pub fn unresolved_links(&self) -> LinkMap {
        self.files
            .iter()
            .filter(|(_, f)| matches!(f.kind, FileKind::Note | FileKind::Canvas))
            .map(|(path, file)| {
                let mut links = BTreeMap::new();
                for link in &file.links {
                    if let Target::Unresolved { name, .. } = &link.target {
                        *links.entry(name.clone()).or_default() += 1;
                    }
                }
                (path.clone(), links)
            })
            .collect()
    }
    pub fn outgoing(&self, path: &str) -> &[IndexedLink] {
        self.files.get(path).map_or(&[], |f| &f.links)
    }
    pub fn backlinks(&self, path: &str) -> BTreeMap<String, usize> {
        self.incoming.get(path).cloned().unwrap_or_default()
    }
    pub fn unresolved(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for links in self.unresolved_links().values() {
            for (name, n) in links {
                *counts.entry(name.clone()).or_default() += n;
            }
        }
        counts
    }
    pub fn orphans(&self) -> Vec<String> {
        self.files
            .keys()
            .filter(|p| !self.incoming.contains_key(*p))
            .cloned()
            .collect()
    }
    pub fn dead_ends(&self) -> Vec<String> {
        self.files
            .iter()
            .filter(|(_, f)| f.links.is_empty())
            .map(|(p, _)| p.clone())
            .collect()
    }
    pub fn tags(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for file in self.files.values() {
            if let Some(note) = &file.note {
                for tag in &note.tags {
                    *counts.entry(tag.key()).or_default() += 1;
                }
            }
        }
        counts
    }
    pub fn properties(&self) -> BTreeMap<String, (PropertyType, usize)> {
        let mut counts = BTreeMap::new();
        for file in self.files.values() {
            for (key, kind) in &file.properties {
                counts.entry(key.clone()).or_insert((*kind, 0)).1 += 1;
            }
        }
        counts
    }
    pub fn tasks(&self) -> Vec<(&str, &crate::Task)> {
        self.files
            .iter()
            .flat_map(|(path, f)| {
                f.note
                    .iter()
                    .flat_map(move |n| n.tasks.iter().map(move |t| (path.as_str(), t)))
            })
            .collect()
    }
    pub fn outline(&self, path: &str) -> &[crate::Heading] {
        self.files
            .get(path)
            .and_then(|f| f.note.as_ref())
            .map_or(&[], |n| &n.headings)
    }
    pub fn word_count(&self, path: &str) -> Option<(usize, usize)> {
        self.files.get(path).map(|f| (f.words, f.characters))
    }
}

fn add_backlinks(incoming: &mut LinkMap, file: &IndexedFile) {
    for link in &file.links {
        if let Target::File(target) = &link.target {
            *incoming
                .entry(target.clone())
                .or_default()
                .entry(file.path.clone())
                .or_default() += 1;
        }
    }
}

fn components(file: &IndexedFile) -> Result<Vec<(&'static str, Value)>> {
    let mut out = Vec::new();
    if let Some(note) = &file.note {
        let value = serde_json::to_value(note)?;
        for (key, label) in [
            ("headings", "Heading"),
            ("blocks", "Block"),
            ("tags", "Tag"),
            ("tasks", "Task"),
            ("footnotes", "Footnote"),
            ("fields", "InlineField"),
        ] {
            for item in value[key].as_array().into_iter().flatten() {
                out.push((label, item.clone()));
            }
        }
        if let Some(fm) = &note.frontmatter {
            for alias in fm.aliases() {
                out.push(("Alias", json!({"name":alias})));
            }
            for (name, value) in &fm.properties {
                out.push((
                    "Property",
                    json!({"name":name,"value":value,"type":file.properties.get(name)}),
                ));
            }
        }
    }
    for link in &file.links {
        out.push((
            if matches!(link.target, Target::Unresolved { .. }) {
                "UnresolvedLink"
            } else if link.link.embed {
                "Embed"
            } else {
                "Link"
            },
            serde_json::to_value(link)?,
        ));
    }
    Ok(out)
}
fn remove_projection(
    file: &IndexedFile,
    delete_file: bool,
    batch: &mut Vec<GraphMutation>,
) -> Result<()> {
    for (i, link) in file.links.iter().enumerate() {
        if matches!(link.target, Target::File(_)) {
            batch.push(GraphMutation::EdgeDelete(format!("link:{}:{i}", file.path)));
        }
    }
    if delete_file {
        batch.push(GraphMutation::NodeDelete(format!("file:{}", file.path)));
    }
    // Component order and count derive from the stored parser result.
    for (i, _) in components(file)?.iter().enumerate() {
        let id = format!("item:{}:{i}", file.path);
        batch.push(GraphMutation::EdgeDelete(format!("owns:{id}")));
        batch.push(GraphMutation::NodeDelete(id));
    }
    Ok(())
}
fn add_projection(file: &IndexedFile, batch: &mut Vec<GraphMutation>) -> Result<()> {
    let parent = format!("file:{}", file.path);
    batch.push(GraphMutation::NodeUpsert(NodeRecord::new(
        &parent,
        ["NotesFile"],
        serde_json::to_value(file)?,
    )));
    for (i, (label, value)) in components(file)?.into_iter().enumerate() {
        let id = format!("item:{}:{i}", file.path);
        batch.push(GraphMutation::NodeUpsert(NodeRecord::new(
            &id,
            [label],
            value,
        )));
        batch.push(GraphMutation::EdgeUpsert(EdgeRecord::new(
            format!("owns:{id}"),
            &parent,
            "CONTAINS",
            &id,
            json!({}),
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(path: &str, text: &str) -> IndexedFile {
        VaultIndex::project(
            path,
            Some(text),
            String::new(),
            None,
            &VaultSettings::default(),
        )
    }

    #[test]
    fn reverse_links_track_counts_structural_changes_and_reopen() {
        let data = tempfile::tempdir().unwrap();
        let mut index = VaultIndex::open(data.path()).unwrap();
        index
            .update(
                vec![
                    project("a.md", "[[b]] [[b]] ![[b]] [[missing]]"),
                    project("b.md", "B"),
                    project(
                        "board.canvas",
                        r#"{"nodes":[{"id":"b","type":"file","file":"b.md"}]}"#,
                    ),
                ],
                &[],
            )
            .unwrap();
        assert_eq!(
            index.backlinks("b.md"),
            BTreeMap::from([("a.md".into(), 3), ("board.canvas".into(), 1)])
        );
        assert_eq!(index.orphans(), ["a.md", "board.canvas"]);
        assert!(index.backlinks("missing.md").is_empty());

        index
            .update(
                vec![
                    project("b.md", "Edited target"),
                    project("missing.md", "New target"),
                ],
                &[],
            )
            .unwrap();
        assert_eq!(index.backlinks("b.md")["a.md"], 3);
        assert_eq!(
            index.backlinks("missing.md"),
            BTreeMap::from([("a.md".into(), 1)])
        );
        index
            .update(vec![project("a.md", "[[b]] ![[missing]] [[missing]]")], &[])
            .unwrap();
        assert_eq!(index.backlinks("b.md")["a.md"], 1);
        assert_eq!(index.backlinks("missing.md")["a.md"], 2);

        index
            .update(vec![], &["board.canvas".into(), "b.md".into()])
            .unwrap();
        assert!(index.backlinks("b.md").is_empty());
        assert_eq!(index.orphans(), ["a.md"]);
        index
            .update(vec![project("nested/b.md", "Replacement target")], &[])
            .unwrap();
        assert_eq!(
            index.backlinks("nested/b.md"),
            BTreeMap::from([("a.md".into(), 1)])
        );
        let resolved = index.resolved_links();
        drop(index);
        let mut index = VaultIndex::open(data.path()).unwrap();
        assert_eq!(index.resolved_links(), resolved);
        assert_eq!(
            index.backlinks("nested/b.md"),
            BTreeMap::from([("a.md".into(), 1)])
        );
        assert_eq!(
            index.backlinks("missing.md"),
            BTreeMap::from([("a.md".into(), 2)])
        );
        assert_eq!(index.orphans(), ["a.md"]);

        index.update(vec![], &["a.md".into()]).unwrap();
        assert!(index.backlinks("nested/b.md").is_empty());
        assert!(index.backlinks("missing.md").is_empty());
        assert_eq!(index.orphans(), ["missing.md", "nested/b.md"]);
    }
}
