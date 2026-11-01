//! The Index sidebar: a folder that can hold folders and files.
//!
//! Dropping a file onto a file turns that file into a folder. A folder may
//! hold another folder. The tree is saved beside the records, so a created
//! file and a deleted one are still that way after a relaunch.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Shelf {
    next: u64,
    root: Node,
    /// Where bytes taken out of a zip are written. Not part of the saved tree.
    #[serde(skip)]
    extract: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Node {
    id: u64,
    name: String,
    expanded: bool,
    kind: Kind,
    #[serde(default)]
    children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Kind {
    Folder {
        #[serde(default)]
        place: Option<String>,
    },
    /// A sidebar file. `place` opens a built-in screen. `stored` is a file
    /// the person created or dropped.
    File {
        #[serde(default)]
        place: Option<String>,
        #[serde(default)]
        stored: Option<Stored>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Stored {
    Markdown {
        #[serde(default)]
        path: Option<PathBuf>,
        #[serde(default)]
        body: String,
    },
    Pdf {
        path: PathBuf,
    },
    Zip {
        path: PathBuf,
        #[serde(default)]
        error: Option<String>,
    },
    Other {
        path: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeShape {
    Folder {
        open: bool,
        place: Option<&'static str>,
    },
    File {
        place: Option<&'static str>,
    },
    Markdown,
    Pdf,
    Zip,
    Other,
}

impl Shelf {
    pub(crate) fn new() -> Self {
        let mut shelf = Self {
            next: 1,
            root: Node {
                id: 0,
                name: "Index".into(),
                expanded: true,
                kind: Kind::Folder { place: None },
                children: Vec::new(),
            },
            extract: None,
        };
        shelf.root.id = shelf.fresh();
        for (name, place) in [
            ("Inbox", "inbox"),
            ("Companies", "companies"),
            ("People", "people"),
            ("Tasks", "tasks"),
            ("Highlights", "highlights"),
        ] {
            let file = shelf.file(name, Some(place), None);
            shelf.root.children.push(file);
        }
        shelf
    }

    pub(crate) fn load(dir: Option<&Path>) -> Self {
        let Some(dir) = dir else {
            return Self::new();
        };
        let mut shelf = read_shelf(&dir.join("shelf.json")).unwrap_or_else(Self::new);
        shelf.extract = Some(dir.join("files"));
        shelf
    }

    pub(crate) fn save(&self, dir: &Path) {
        let path = dir.join("shelf.json");
        let Ok(text) = serde_json::to_string(self) else {
            return;
        };
        if fs::create_dir_all(dir).is_err() {
            return;
        }
        let temporary = path.with_extension("json.tmp");
        if fs::write(&temporary, text).is_ok() {
            let _ = fs::rename(&temporary, path);
        }
    }

    pub(crate) fn root_id(&self) -> u64 {
        self.root.id
    }

    pub(crate) fn outline(&self) -> Outline {
        outline_of(&self.root)
    }

    pub(crate) fn set_expanded(&mut self, id: u64, expanded: bool) {
        if let Some(node) = self.node_mut(id) {
            node.expanded = expanded;
        }
    }

    /// Rows in the order the tree draws them, with their depth.
    pub(crate) fn rows(&self) -> Vec<(u64, String, usize, NodeShape, bool)> {
        let mut rows = Vec::new();
        push_rows(&self.root, 0, &mut rows);
        rows
    }

    pub(crate) fn place_of(&self, id: u64) -> Option<&str> {
        self.node(id).and_then(|node| node.place())
    }

    pub(crate) fn is_folder(&self, id: u64) -> bool {
        self.node(id).is_some_and(Node::is_folder)
    }

    pub(super) fn contains(&self, id: u64) -> bool {
        self.node(id).is_some()
    }

    /// What the main page shows for this node, when it is not a built-in place.
    pub(crate) fn page(&self, id: u64) -> Option<Page> {
        self.node(id)?.page()
    }

    pub(super) fn reader(&self, id: u64) -> Option<impl FnOnce() -> Option<Page> + use<>> {
        let node = self.node(id)?.clone();
        Some(move || node.page())
    }

    /// A blank markdown file inside `parent`, or inside the root when `parent`
    /// is a file.
    pub(crate) fn create_file(&mut self, parent: Option<u64>) -> u64 {
        let parent = parent
            .filter(|id| self.is_folder(*id))
            .unwrap_or(self.root.id);
        let id = self.fresh();
        let node = Node {
            id,
            name: "Untitled.md".into(),
            expanded: false,
            kind: Kind::File {
                place: None,
                stored: Some(Stored::Markdown {
                    path: self.write_blank(id),
                    body: String::new(),
                }),
            },
            children: Vec::new(),
        };
        if let Some(folder) = self.node_mut(parent) {
            folder.expanded = true;
            folder.children.push(node);
        }
        id
    }

    pub(crate) fn delete(&mut self, id: u64) -> bool {
        if id == self.root.id {
            return false;
        }
        remove(&mut self.root, id)
    }

    pub(crate) fn move_sibling(&mut self, id: u64, delta: isize) -> bool {
        move_among_siblings(&mut self.root, id, delta)
    }

    /// Drop `dragged` onto `target`. A file target becomes a folder.
    pub(crate) fn drop_node(&mut self, dragged: u64, target: u64) -> bool {
        if dragged == target
            || dragged == self.root.id
            || self.node(target).is_none()
            || contains(&self.root, dragged, target)
        {
            return false;
        }
        let Some(node) = take(&mut self.root, dragged) else {
            return false;
        };
        self.nest(target, node)
    }

    /// Drop real paths onto `target`. A file target becomes a folder that
    /// holds them. A folder target holds them directly.
    #[cfg(test)]
    pub(crate) fn drop_paths(&mut self, target: u64, paths: &[PathBuf]) -> Vec<u64> {
        let import = Import::read(paths, self.import_directory());
        self.finish_import(target, import).unwrap_or_default()
    }

    pub(super) fn import_directory(&self) -> Option<PathBuf> {
        self.extract.clone()
    }

    pub(super) fn finish_import(
        &mut self,
        target: u64,
        import: Import,
    ) -> anyhow::Result<Vec<u64>> {
        anyhow::ensure!(
            self.node(target).is_some(),
            "The destination was removed during import. Drop the files again."
        );
        let mut ids = Vec::new();
        for mut node in import.0 {
            // Allocate against the current owner, not the worker's snapshot.
            // Concurrent create, delete and reorder operations remain intact.
            self.assign_ids(&mut node);
            ids.push(node.id);
            self.nest(target, node);
        }
        Ok(ids)
    }

    fn assign_ids(&mut self, node: &mut Node) {
        node.id = self.fresh();
        for child in &mut node.children {
            self.assign_ids(child);
        }
    }

    fn nest(&mut self, target: u64, node: Node) -> bool {
        if !self.is_folder(target) {
            self.become_folder(target);
        }
        let Some(folder) = self.node_mut(target) else {
            return false;
        };
        folder.expanded = true;
        folder.children.push(node);
        true
    }

    fn become_folder(&mut self, id: u64) {
        let Some(node) = self.node(id) else {
            return;
        };
        if node.is_folder() {
            return;
        }
        let place = node.place().map(str::to_owned);
        let name = node.name.clone();
        let stored = match &node.kind {
            Kind::File { stored, .. } => stored.clone(),
            Kind::Folder { .. } => None,
        };
        let child = stored.map(|stored| self.file(&name, None, Some(stored)));
        let Some(node) = self.node_mut(id) else {
            return;
        };
        node.kind = Kind::Folder { place };
        node.expanded = true;
        if let Some(child) = child {
            node.children.push(child);
        }
    }

    fn write_blank(&self, id: u64) -> Option<PathBuf> {
        let dir = self.extract.as_ref()?;
        fs::create_dir_all(dir).ok()?;
        let path = dir.join(format!("{id}-Untitled.md"));
        fs::write(&path, "").ok()?;
        Some(path)
    }

    fn node_from_path(&mut self, path: &Path) -> Node {
        self.import_path(path, &mut HashSet::new(), 0)
    }

    fn import_path(&mut self, path: &Path, seen: &mut HashSet<PathBuf>, depth: usize) -> Node {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        if path.is_dir() && !directory_link(path) && depth < IMPORT_DEPTH {
            let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            if !seen.insert(canonical) {
                return Node {
                    id: self.fresh(),
                    name,
                    expanded: true,
                    kind: Kind::Folder { place: None },
                    children: Vec::new(),
                };
            }
            let mut children = Vec::new();
            if let Ok(read) = fs::read_dir(path) {
                let mut paths: Vec<_> = read
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.path())
                    .collect();
                paths.sort();
                for child in paths {
                    if children.len() >= IMPORT_CAP || directory_link(&child) {
                        continue;
                    }
                    children.push(self.import_path(&child, seen, depth + 1));
                }
            }
            return Node {
                id: self.fresh(),
                name,
                expanded: true,
                kind: Kind::Folder { place: None },
                children,
            };
        }
        if is_zip(path) {
            let entries =
                match super::archive::read(path, IMPORT_DEPTH - depth) {
                    Ok(entries) => entries,
                    Err(error) => return self.file(
                        &name,
                        None,
                        Some(Stored::Zip {
                            path: path.to_path_buf(),
                            error: Some(format!(
                                "Could not import archive: {error}. The original file is unchanged."
                            )),
                        }),
                    ),
                };
            let mut children = Vec::new();
            for entry in entries {
                if let Err(error) =
                    self.insert_zip(&mut children, &entry.name, entry.folder, entry.data)
                {
                    return self.file(&name, None, Some(Stored::Zip {
                        path: path.to_path_buf(),
                        error: Some(format!("Could not save archive contents: {error}. The original file is unchanged.")),
                    }));
                }
            }
            return Node {
                id: self.fresh(),
                name,
                expanded: true,
                kind: Kind::Folder { place: None },
                children,
            };
        }
        let stored = if is_markdown(path) {
            Stored::Markdown {
                path: Some(path.to_path_buf()),
                body: fs::read_to_string(path).unwrap_or_default(),
            }
        } else if is_pdf(path) {
            Stored::Pdf {
                path: path.to_path_buf(),
            }
        } else {
            Stored::Other {
                path: path.to_path_buf(),
            }
        };
        self.file(&name, None, Some(stored))
    }

    fn insert_zip(
        &mut self,
        nodes: &mut Vec<Node>,
        name: &str,
        folder: bool,
        data: Vec<u8>,
    ) -> anyhow::Result<()> {
        let parts: Vec<&str> = name.split('/').filter(|part| !part.is_empty()).collect();
        if parts.is_empty() {
            return Ok(());
        }
        self.insert_zip_parts(nodes, &parts, folder, data)
    }

    fn insert_zip_parts(
        &mut self,
        nodes: &mut Vec<Node>,
        parts: &[&str],
        folder: bool,
        data: Vec<u8>,
    ) -> anyhow::Result<()> {
        let name = parts[0];
        if parts.len() == 1 {
            if folder {
                if !nodes
                    .iter()
                    .any(|node| node.name == name && node.is_folder())
                {
                    nodes.push(Node {
                        id: self.fresh(),
                        name: name.into(),
                        expanded: true,
                        kind: Kind::Folder { place: None },
                        children: Vec::new(),
                    });
                }
                return Ok(());
            }
            let stored = self.stored_bytes(name, data)?;
            nodes.push(self.file(name, None, Some(stored)));
            return Ok(());
        }
        if nodes
            .iter()
            .any(|node| node.name == name && node.is_folder())
        {
            let child = nodes
                .iter_mut()
                .find(|node| node.name == name && node.is_folder())
                .expect("folder");
            self.insert_zip_parts(&mut child.children, &parts[1..], folder, data)?;
            return Ok(());
        }
        let mut child = Node {
            id: self.fresh(),
            name: name.into(),
            expanded: true,
            kind: Kind::Folder { place: None },
            children: Vec::new(),
        };
        self.insert_zip_parts(&mut child.children, &parts[1..], folder, data)?;
        nodes.push(child);
        Ok(())
    }

    fn stored_bytes(&self, name: &str, data: Vec<u8>) -> anyhow::Result<Stored> {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".md") || lower.ends_with(".markdown") {
            return Ok(Stored::Markdown {
                path: None,
                body: String::from_utf8_lossy(&data).into_owned(),
            });
        }
        let path = self.spill(name, &data)?;
        if lower.ends_with(".pdf") {
            Ok(Stored::Pdf { path })
        } else {
            Ok(Stored::Other { path })
        }
    }

    fn spill(&self, name: &str, data: &[u8]) -> anyhow::Result<PathBuf> {
        use std::io::Write as _;
        let dir = self.extract.clone().unwrap_or_else(std::env::temp_dir);
        fs::create_dir_all(&dir)?;
        // Unique files prevent concurrent workers or restored IDs from
        // overwriting earlier imports. Preserve extensions for file readers.
        let suffix = Path::new(name)
            .extension()
            .and_then(|s| s.to_str())
            .map(|ext| format!(".{ext}"))
            .unwrap_or_default();
        let mut file = tempfile::Builder::new()
            .prefix("import-")
            .suffix(&suffix)
            .tempfile_in(dir)?;
        file.write_all(data)?;
        file.as_file().sync_all()?;
        let (_, path) = file.keep()?;
        Ok(path)
    }

    fn file(&mut self, name: &str, place: Option<&str>, stored: Option<Stored>) -> Node {
        Node {
            id: self.fresh(),
            name: name.into(),
            expanded: false,
            kind: Kind::File {
                place: place.map(str::to_owned),
                stored,
            },
            children: Vec::new(),
        }
    }

    fn fresh(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }

    fn node(&self, id: u64) -> Option<&Node> {
        find(&self.root, id)
    }

    fn node_mut(&mut self, id: u64) -> Option<&mut Node> {
        find_mut(&mut self.root, id)
    }
}

/// Prepared off the UI thread; IDs are assigned only when admitted by Shelf.
pub(super) struct Import(Vec<Node>);
impl Import {
    pub(super) fn read(paths: &[PathBuf], directory: Option<PathBuf>) -> Self {
        let mut staging = Shelf::new();
        staging.extract = directory;
        Self(
            paths
                .iter()
                .map(|path| staging.node_from_path(path))
                .collect(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Folder { name: String, children: Vec<String> },
    Markdown { title: String, source: String },
    Pdf { name: String, path: PathBuf },
    File { name: String, path: PathBuf },
    Error { name: String, message: String },
}

impl Node {
    fn page(&self) -> Option<Page> {
        let node = self;
        match &node.kind {
            Kind::Folder { .. } => Some(Page::Folder {
                name: node.name.clone(),
                children: node
                    .children
                    .iter()
                    .map(|child| child.name.clone())
                    .collect(),
            }),
            Kind::File { stored, .. } => stored.as_ref().map(|stored| match stored {
                Stored::Markdown { path, body } => {
                    let source = path
                        .as_ref()
                        .and_then(|path| fs::read_to_string(path).ok())
                        .unwrap_or_else(|| body.clone());
                    Page::Markdown {
                        title: heading(&source).unwrap_or_else(|| node.name.clone()),
                        source,
                    }
                }
                Stored::Pdf { path } => Page::Pdf {
                    name: node.name.clone(),
                    path: path.clone(),
                },
                Stored::Zip {
                    error: Some(error), ..
                } => Page::Error {
                    name: node.name.clone(),
                    message: error.clone(),
                },
                Stored::Zip { path, .. } | Stored::Other { path } => Page::File {
                    name: node.name.clone(),
                    path: path.clone(),
                },
            }),
        }
    }

    fn is_folder(&self) -> bool {
        matches!(self.kind, Kind::Folder { .. })
    }

    fn place(&self) -> Option<&str> {
        match &self.kind {
            Kind::Folder { place } | Kind::File { place, .. } => place.as_deref(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Outline {
    pub id: u64,
    pub name: String,
    pub expanded: bool,
    pub shape: NodeShape,
    pub folder: bool,
    pub children: Vec<Outline>,
}

fn outline_of(node: &Node) -> Outline {
    Outline {
        id: node.id,
        name: node.name.clone(),
        expanded: node.expanded,
        shape: shape(node),
        folder: node.is_folder(),
        children: node.children.iter().map(outline_of).collect(),
    }
}

fn push_rows(node: &Node, depth: usize, rows: &mut Vec<(u64, String, usize, NodeShape, bool)>) {
    rows.push((
        node.id,
        node.name.clone(),
        depth,
        shape(node),
        node.is_folder(),
    ));
    if node.expanded {
        for child in &node.children {
            push_rows(child, depth + 1, rows);
        }
    }
}

fn shape(node: &Node) -> NodeShape {
    if zip_named_folder(node) {
        return NodeShape::Zip;
    }
    match &node.kind {
        Kind::Folder { place } => folder_shape(node.expanded, place.as_deref()),
        Kind::File { place, stored } => file_shape(place.as_deref(), stored.as_ref()),
    }
}

fn zip_named_folder(node: &Node) -> bool {
    node.is_folder() && node.name.to_ascii_lowercase().ends_with(".zip")
}

fn folder_shape(open: bool, place: Option<&str>) -> NodeShape {
    NodeShape::Folder {
        open,
        place: place.and_then(static_place),
    }
}

fn file_shape(place: Option<&str>, stored: Option<&Stored>) -> NodeShape {
    match stored {
        Some(Stored::Markdown { .. }) => NodeShape::Markdown,
        Some(Stored::Pdf { .. }) => NodeShape::Pdf,
        Some(Stored::Zip { .. }) => NodeShape::Zip,
        Some(Stored::Other { .. }) => NodeShape::Other,
        None => NodeShape::File {
            place: place.and_then(static_place),
        },
    }
}

fn static_place(place: &str) -> Option<&'static str> {
    match place {
        "inbox" => Some("inbox"),
        "companies" => Some("companies"),
        "people" => Some("people"),
        "tasks" => Some("tasks"),
        "highlights" => Some("highlights"),
        _ => None,
    }
}

/// The first markdown heading, without its marks. `None` when there is none.
pub(crate) fn heading(source: &str) -> Option<String> {
    source.lines().find_map(|line| {
        let trimmed = line.trim();
        let rest = trimmed.strip_prefix('#').map(str::trim)?;
        let rest = rest.trim_start_matches('#').trim();
        (!rest.is_empty()).then(|| rest.to_owned())
    })
}

fn find(node: &Node, id: u64) -> Option<&Node> {
    if node.id == id {
        return Some(node);
    }
    node.children.iter().find_map(|child| find(child, id))
}

fn find_mut(node: &mut Node, id: u64) -> Option<&mut Node> {
    if node.id == id {
        return Some(node);
    }
    node.children
        .iter_mut()
        .find_map(|child| find_mut(child, id))
}

fn contains(node: &Node, ancestor: u64, id: u64) -> bool {
    let Some(ancestor) = find(node, ancestor) else {
        return false;
    };
    ancestor.id != id && find(ancestor, id).is_some()
}

fn take(node: &mut Node, id: u64) -> Option<Node> {
    if let Some(index) = node.children.iter().position(|child| child.id == id) {
        return Some(node.children.remove(index));
    }
    node.children.iter_mut().find_map(|child| take(child, id))
}

fn remove(node: &mut Node, id: u64) -> bool {
    if let Some(index) = node.children.iter().position(|child| child.id == id) {
        node.children.remove(index);
        return true;
    }
    node.children.iter_mut().any(|child| remove(child, id))
}

fn move_among_siblings(node: &mut Node, id: u64, delta: isize) -> bool {
    if let Some(from) = node.children.iter().position(|child| child.id == id) {
        let to = (from as isize + delta).clamp(0, node.children.len() as isize - 1) as usize;
        if from == to {
            return false;
        }
        let child = node.children.remove(from);
        node.children.insert(to, child);
        return true;
    }
    node.children
        .iter_mut()
        .any(|child| move_among_siblings(child, id, delta))
}

fn is_markdown(path: &Path) -> bool {
    has_extension(path, &["md", "markdown"])
}

fn is_pdf(path: &Path) -> bool {
    has_extension(path, &["pdf"])
}

fn is_zip(path: &Path) -> bool {
    has_extension(path, &["zip"])
}

const IMPORT_DEPTH: usize = 24;
const IMPORT_CAP: usize = 2_000;

fn directory_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink() && path.is_dir())
}

fn read_shelf(path: &Path) -> Option<Shelf> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return None,
    };
    match serde_json::from_str(&text) {
        Ok(shelf) => Some(shelf),
        Err(_) => {
            let _ = fs::rename(path, path.with_extension("json.unreadable"));
            None
        }
    }
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    extensions
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_drop_target_does_not_remove_the_dragged_file() {
        let mut shelf = Shelf::new();
        let dragged = shelf.root.children[0].id;
        let before = shelf.clone();
        assert!(!shelf.drop_node(dragged, u64::MAX));
        assert_eq!(shelf, before, "a rejected move must leave the tree intact");
    }

    #[test]
    fn prepared_import_preserves_concurrent_edits_and_uses_unique_files() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("files.zip");
        fs::write(&archive, stored_zip("data.txt", b"first")).unwrap();
        let mut shelf = Shelf::load(Some(dir.path()));
        let first = Import::read(std::slice::from_ref(&archive), shelf.import_directory());
        fs::write(&archive, stored_zip("data.txt", b"second")).unwrap();
        let second = Import::read(std::slice::from_ref(&archive), shelf.import_directory());
        let created = shelf.create_file(None);
        shelf.finish_import(shelf.root_id(), first).unwrap();
        shelf.finish_import(shelf.root_id(), second).unwrap();
        let rows = shelf.rows();
        let ids: HashSet<_> = rows.iter().map(|row| row.0).collect();
        assert_eq!(
            ids.len(),
            rows.len(),
            "workers must not allocate durable IDs"
        );
        assert!(
            ids.contains(&created),
            "the concurrent file must survive admission"
        );
        let paths: Vec<_> = rows
            .iter()
            .filter_map(|row| match shelf.page(row.0) {
                Some(Page::File { path, .. }) => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(paths.len(), 2);
        assert_ne!(
            paths[0], paths[1],
            "imports must not overwrite previous extraction paths"
        );
        assert_eq!(fs::read(&paths[0]).unwrap(), b"first");
        assert_eq!(fs::read(&paths[1]).unwrap(), b"second");
        let prepared = Import::read(std::slice::from_ref(&archive), shelf.import_directory());
        let before = shelf.clone();
        assert!(shelf.finish_import(u64::MAX, prepared).is_err());
        assert_eq!(
            shelf, before,
            "late completion must validate the destination"
        );
    }

    #[test]
    fn failed_extraction_does_not_publish_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("files.zip");
        fs::write(&archive, stored_zip("data.txt", b"content")).unwrap();
        fs::write(dir.path().join("files"), b"blocks the extraction directory").unwrap();
        let mut shelf = Shelf::load(Some(dir.path()));
        let id = shelf.drop_paths(shelf.root_id(), &[archive])[0];
        assert!(
            matches!(shelf.page(id), Some(Page::Error { .. })),
            "a failed write must be visible"
        );
    }

    #[test]
    fn invalid_archive_keeps_its_path_and_explains_the_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.zip");
        fs::write(&path, b"not a ZIP").unwrap();
        let mut shelf = Shelf::load(Some(dir.path()));
        let id = shelf.drop_paths(shelf.root_id(), &[path.clone()])[0];
        assert!(
            !shelf.is_folder(id),
            "an invalid ZIP must not look like an empty folder"
        );
        match shelf.page(id).unwrap() {
            Page::Error { message, .. } => assert!(message.contains("invalid ZIP directory")),
            page => panic!("expected visible import error, got {page:?}"),
        }
        shelf.save(dir.path());
        assert!(matches!(
            Shelf::load(Some(dir.path())).page(id),
            Some(Page::Error { .. })
        ));
        assert_eq!(fs::read(path).unwrap(), b"not a ZIP");
    }

    #[test]
    fn index_is_a_folder_and_inbox_is_its_first_file() {
        let shelf = Shelf::new();
        let rows = shelf.rows();
        assert_eq!(rows[0].1, "Index");
        assert!(rows[0].4, "Index is a folder");
        assert_eq!(rows[1].1, "Inbox");
        assert_eq!(rows[1].2, 1, "a child is inset one level");
        assert!(!rows[1].4);
    }

    #[test]
    fn dropping_a_file_on_a_file_makes_a_folder_that_can_hold_a_folder() {
        let dir = std::env::temp_dir().join(format!("index-shelf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let note = dir.join("hello.md");
        fs::write(&note, "# Hello\n\nA paragraph.\n").unwrap();
        let nested = dir.join("pack");
        fs::create_dir_all(nested.join("inner")).unwrap();
        fs::write(nested.join("inner").join("note.md"), "# Inner\n").unwrap();

        let mut shelf = Shelf::new();
        let inbox = shelf.rows().iter().find(|row| row.1 == "Inbox").unwrap().0;
        let added = shelf.drop_paths(inbox, &[note, nested]);
        assert_eq!(added.len(), 2);
        assert!(shelf.is_folder(inbox), "the file became a folder");
        let rows = shelf.rows();
        let hello = rows.iter().find(|row| row.1 == "hello.md").unwrap();
        assert_eq!(hello.2, 2, "the dropped file is inside the new folder");
        let pack = rows.iter().find(|row| row.1 == "pack").unwrap();
        assert!(pack.4);
        let inner = rows.iter().find(|row| row.1 == "inner").unwrap();
        assert!(inner.4, "a folder holds another folder");
        assert!(inner.2 > pack.2);

        let hello_id = hello.0;
        let page = shelf.page(hello_id).unwrap();
        match page {
            Page::Markdown { title, source } => {
                assert_eq!(title, "Hello");
                assert!(source.contains("A paragraph."));
                assert_ne!(title, "# Hello");
            }
            other => panic!("markdown page, got {other:?}"),
        }

        let created = shelf.create_file(Some(inbox));
        assert!(
            shelf
                .rows()
                .iter()
                .any(|row| row.0 == created && row.1 == "Untitled.md")
        );
        assert!(shelf.delete(created));
        assert!(!shelf.rows().iter().any(|row| row.0 == created));

        shelf.save(&dir);
        let loaded = Shelf::load(Some(&dir));
        assert!(loaded.is_folder(inbox));
        assert!(!loaded.rows().iter().any(|row| row.1 == "Untitled.md"));
        assert!(loaded.rows().iter().any(|row| row.1 == "hello.md"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pdf_stays_a_pdf_and_a_zip_is_a_folder_of_its_entries() {
        let dir = std::env::temp_dir().join(format!("index-shelf-pdf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let pdf = dir.join("brief.pdf");
        fs::write(&pdf, b"%PDF-1.1\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF\n").unwrap();
        let zip_bytes = stored_zip("inside.md", b"# Inside\n");
        let archive = dir.join("pack.zip");
        fs::write(&archive, zip_bytes).unwrap();

        let mut shelf = Shelf::new();
        let ids = shelf.drop_paths(shelf.root_id(), &[pdf, archive]);
        assert_eq!(ids.len(), 2);
        let rows = shelf.rows();
        let pdf_row = rows.iter().find(|row| row.1 == "brief.pdf").unwrap();
        assert_eq!(pdf_row.3, NodeShape::Pdf);
        match shelf.page(pdf_row.0).unwrap() {
            Page::Pdf { name, path } => {
                assert_eq!(name, "brief.pdf");
                assert!(path.ends_with("brief.pdf"));
            }
            other => panic!("pdf stays a pdf, got {other:?}"),
        }
        assert!(
            rows.iter().any(|row| row.1 == "inside.md"),
            "the zip listed its file"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    fn stored_zip(name: &str, data: &[u8]) -> Vec<u8> {
        super::super::archive::tests::archive(&[(name, data)], zip::CompressionMethod::Stored)
    }

    #[test]
    fn a_folder_link_back_to_itself_is_imported_once() {
        let dir = std::env::temp_dir().join(format!("index-shelf-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let pack = dir.join("pack");
        fs::create_dir_all(&pack).unwrap();
        fs::write(pack.join("note.md"), "# Nested\n").unwrap();
        std::os::unix::fs::symlink(&pack, pack.join("again")).unwrap();

        let mut shelf = Shelf::new();
        shelf.drop_paths(shelf.root_id(), &[pack]);
        let rows = shelf.rows();
        assert_eq!(rows.iter().filter(|row| row.1 == "pack").count(), 1);
        assert_eq!(rows.iter().filter(|row| row.1 == "again").count(), 0);
        assert!(rows.iter().any(|row| row.1 == "note.md"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_shelf_is_kept_aside() {
        let dir = std::env::temp_dir().join(format!("index-shelf-bad-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("shelf.json"), "not json").unwrap();
        let shelf = Shelf::load(Some(&dir));
        assert_eq!(shelf.rows()[0].1, "Index");
        assert_eq!(
            fs::read_to_string(dir.join("shelf.json.unreadable")).unwrap(),
            "not json"
        );
        shelf.save(&dir);
        assert!(dir.join("shelf.json").is_file());
        assert_eq!(
            fs::read_to_string(dir.join("shelf.json.unreadable")).unwrap(),
            "not json"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
