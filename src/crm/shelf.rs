//! The Index sidebar: a folder that can hold folders and files.
//!
//! Dropping a file onto a file turns that file into a folder. A folder may
//! hold another folder. The tree is saved beside the records, so a created
//! file and a deleted one are still that way after a relaunch.

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
        let Some(text) = dir.and_then(|dir| fs::read_to_string(dir.join("shelf.json")).ok()) else {
            let mut shelf = Self::new();
            shelf.extract = dir.map(|dir| dir.join("files"));
            return shelf;
        };
        let mut shelf: Self = serde_json::from_str(&text).unwrap_or_else(|_| Self::new());
        shelf.extract = dir.map(|dir| dir.join("files"));
        shelf
    }

    pub(crate) fn save(&self, dir: &Path) {
        let _ = fs::create_dir_all(dir);
        if let Ok(text) = serde_json::to_string(self) {
            let _ = fs::write(dir.join("shelf.json"), text);
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

    /// What the main page shows for this node, when it is not a built-in place.
    pub(crate) fn page(&self, id: u64) -> Option<Page> {
        let node = self.node(id)?;
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
                Stored::Zip { path } | Stored::Other { path } => Page::File {
                    name: node.name.clone(),
                    path: path.clone(),
                },
            }),
        }
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
                    path: None,
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
        if dragged == target || dragged == self.root.id || contains(&self.root, dragged, target) {
            return false;
        }
        let Some(node) = take(&mut self.root, dragged) else {
            return false;
        };
        self.nest(target, node)
    }

    /// Drop real paths onto `target`. A file target becomes a folder that
    /// holds them. A folder target holds them directly.
    pub(crate) fn drop_paths(&mut self, target: u64, paths: &[PathBuf]) -> Vec<u64> {
        if self.node(target).is_none() {
            return Vec::new();
        }
        if !self.is_folder(target) {
            self.become_folder(target);
        }
        let mut ids = Vec::new();
        for path in paths {
            let node = self.node_from_path(path);
            let id = node.id;
            if self.nest(target, node) {
                ids.push(id);
            }
        }
        ids
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

    fn node_from_path(&mut self, path: &Path) -> Node {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        if path.is_dir() {
            let mut children = Vec::new();
            if let Ok(read) = fs::read_dir(path) {
                let mut paths: Vec<_> = read
                    .filter_map(|entry| entry.ok())
                    .map(|e| e.path())
                    .collect();
                paths.sort();
                for child in paths {
                    children.push(self.node_from_path(&child));
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
            let bytes = fs::read(path).unwrap_or_default();
            let mut children = Vec::new();
            for entry in zip_entries(&bytes) {
                self.insert_zip(&mut children, &entry.name, entry.folder, entry.data);
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

    fn insert_zip(&mut self, nodes: &mut Vec<Node>, name: &str, folder: bool, data: Vec<u8>) {
        let parts: Vec<&str> = name.split('/').filter(|part| !part.is_empty()).collect();
        if parts.is_empty() {
            return;
        }
        self.insert_zip_parts(nodes, &parts, folder, data);
    }

    fn insert_zip_parts(
        &mut self,
        nodes: &mut Vec<Node>,
        parts: &[&str],
        folder: bool,
        data: Vec<u8>,
    ) {
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
                return;
            }
            let stored = self.stored_bytes(name, data);
            nodes.push(self.file(name, None, Some(stored)));
            return;
        }
        if nodes
            .iter()
            .any(|node| node.name == name && node.is_folder())
        {
            let child = nodes
                .iter_mut()
                .find(|node| node.name == name && node.is_folder())
                .expect("folder");
            self.insert_zip_parts(&mut child.children, &parts[1..], folder, data);
            return;
        }
        let mut child = Node {
            id: self.fresh(),
            name: name.into(),
            expanded: true,
            kind: Kind::Folder { place: None },
            children: Vec::new(),
        };
        self.insert_zip_parts(&mut child.children, &parts[1..], folder, data);
        nodes.push(child);
    }

    fn stored_bytes(&self, name: &str, data: Vec<u8>) -> Stored {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".md") || lower.ends_with(".markdown") {
            return Stored::Markdown {
                path: None,
                body: String::from_utf8_lossy(&data).into_owned(),
            };
        }
        let path = self.spill(name, &data);
        if lower.ends_with(".pdf") {
            Stored::Pdf { path }
        } else {
            Stored::Other { path }
        }
    }

    fn spill(&self, name: &str, data: &[u8]) -> PathBuf {
        let dir = self.extract.clone().unwrap_or_else(|| {
            std::env::temp_dir().join(format!("platinum-zip-{}", std::process::id()))
        });
        let _ = fs::create_dir_all(&dir);
        let safe = name.replace('/', "-");
        let path = dir.join(format!("{}-{safe}", self.next));
        let _ = fs::write(&path, data);
        path
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Folder { name: String, children: Vec<String> },
    Markdown { title: String, source: String },
    Pdf { name: String, path: PathBuf },
    File { name: String, path: PathBuf },
}

impl Node {
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

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    extensions
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
}

struct ZipEntry {
    name: String,
    folder: bool,
    data: Vec<u8>,
}

/// Files in a zip. Deflated entries are inflated; a zip with no central
/// directory is read from its local headers when those are stored.
fn zip_entries(bytes: &[u8]) -> Vec<ZipEntry> {
    if let Some(entries) = central_directory(bytes)
        && !entries.is_empty()
    {
        return entries;
    }
    local_entries(bytes)
}

fn central_directory(bytes: &[u8]) -> Option<Vec<ZipEntry>> {
    let (count, mut at) = directory_span(bytes)?;
    let mut entries = Vec::new();
    for _ in 0..count {
        let Some((entry, next)) = central_entry(bytes, at) else {
            break;
        };
        if let Some(entry) = entry {
            entries.push(entry);
        }
        at = next;
    }
    Some(entries)
}

fn directory_span(bytes: &[u8]) -> Option<(usize, usize)> {
    let eocd = find_eocd(bytes)?;
    if eocd + 22 > bytes.len() {
        return None;
    }
    let count = u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]) as usize;
    let offset = u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().ok()?) as usize;
    Some((count, offset))
}

fn central_entry(bytes: &[u8], at: usize) -> Option<(Option<ZipEntry>, usize)> {
    if at + 46 > bytes.len() || bytes[at..at + 4] != *b"PK\x01\x02" {
        return None;
    }
    let method = u16::from_le_bytes([bytes[at + 10], bytes[at + 11]]);
    let compressed = u32::from_le_bytes(bytes[at + 20..at + 24].try_into().ok()?) as usize;
    let name_len = u16::from_le_bytes([bytes[at + 28], bytes[at + 29]]) as usize;
    let extra_len = u16::from_le_bytes([bytes[at + 30], bytes[at + 31]]) as usize;
    let comment_len = u16::from_le_bytes([bytes[at + 32], bytes[at + 33]]) as usize;
    let local = u32::from_le_bytes(bytes[at + 42..at + 46].try_into().ok()?) as usize;
    let name_end = at + 46 + name_len;
    if name_end > bytes.len() {
        return None;
    }
    let raw_name = String::from_utf8_lossy(&bytes[at + 46..name_end]).into_owned();
    let next = name_end + extra_len + comment_len;
    Some((zip_entry(bytes, &raw_name, local, method, compressed), next))
}

fn zip_entry(
    bytes: &[u8],
    raw_name: &str,
    local: usize,
    method: u16,
    compressed: usize,
) -> Option<ZipEntry> {
    let folder = raw_name.ends_with('/');
    let name = raw_name.trim_end_matches('/').to_owned();
    if name.is_empty() {
        return None;
    }
    let data = if folder {
        Vec::new()
    } else {
        inflate_local(bytes, local, method, compressed)
    };
    Some(ZipEntry { name, folder, data })
}

fn find_eocd(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 22 {
        return None;
    }
    let start = bytes.len().saturating_sub(22 + 65535);
    (start..=bytes.len() - 22)
        .rev()
        .find(|&index| bytes[index..index + 4] == *b"PK\x05\x06")
}

fn inflate_local(bytes: &[u8], local: usize, method: u16, compressed: usize) -> Vec<u8> {
    if local + 30 > bytes.len() || bytes[local..local + 4] != *b"PK\x03\x04" {
        return Vec::new();
    }
    let name_len = u16::from_le_bytes([bytes[local + 26], bytes[local + 27]]) as usize;
    let extra_len = u16::from_le_bytes([bytes[local + 28], bytes[local + 29]]) as usize;
    let start = local + 30 + name_len + extra_len;
    let end = start.saturating_add(compressed).min(bytes.len());
    if start > bytes.len() {
        return Vec::new();
    }
    let raw = &bytes[start..end];
    match method {
        0 => raw.to_vec(),
        8 => {
            use std::io::Read;
            let mut decoder = flate2::read::DeflateDecoder::new(raw);
            let mut out = Vec::new();
            let _ = decoder.read_to_end(&mut out);
            out
        }
        _ => Vec::new(),
    }
}

fn local_entries(bytes: &[u8]) -> Vec<ZipEntry> {
    let mut entries = Vec::new();
    let mut index = 0;
    while local_header_at(bytes, index) {
        match local_step(bytes, index) {
            None => break,
            Some((entry, next)) => {
                if let Some(entry) = entry {
                    entries.push(entry);
                }
                index = next;
            }
        }
    }
    entries
}

fn local_header_at(bytes: &[u8], index: usize) -> bool {
    index + 30 <= bytes.len() && bytes[index..index + 4] == *b"PK\x03\x04"
}

fn local_step(bytes: &[u8], index: usize) -> Option<(Option<ZipEntry>, usize)> {
    let flags = u16::from_le_bytes([bytes[index + 6], bytes[index + 7]]);
    let method = u16::from_le_bytes([bytes[index + 8], bytes[index + 9]]);
    let compressed =
        u32::from_le_bytes(bytes[index + 18..index + 22].try_into().unwrap_or([0; 4])) as usize;
    let name_len = u16::from_le_bytes([bytes[index + 26], bytes[index + 27]]) as usize;
    let extra_len = u16::from_le_bytes([bytes[index + 28], bytes[index + 29]]) as usize;
    let name_at = index + 30;
    if name_at + name_len > bytes.len() || unreadable_local(flags, method) {
        return None;
    }
    let raw_name = String::from_utf8_lossy(&bytes[name_at..name_at + name_len]).into_owned();
    let entry = zip_entry(bytes, &raw_name, index, method, compressed);
    let next = name_at + name_len + extra_len + compressed;
    let next = if method != 0 && compressed == 0 {
        usize::MAX
    } else {
        next
    };
    Some((entry, next))
}

fn unreadable_local(flags: u16, method: u16) -> bool {
    flags & 0x8 != 0 || (method != 0 && method != 8)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let name = name.as_bytes();
        let crc = crc32(data);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PK\x03\x04");
        bytes.extend_from_slice(&20u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&crc.to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(data);
        bytes
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for byte in data {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        !crc
    }
}
