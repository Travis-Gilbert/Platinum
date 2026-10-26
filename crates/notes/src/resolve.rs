//! Link resolution as Obsidian does it: which file a link names, which heading or block of it, and how a new link names a file.
use crate::model::{Fragment, Link, LinkSyntax, Note};
use crate::offsets::Span;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;
use typed_path::{Utf8UnixComponent, Utf8UnixPath};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileKind {
    Note,
    Canvas,
    Base,
    Image,
    Audio,
    Video,
    Pdf,
    Other,
}

impl FileKind {
    /// By extension, as Obsidian's accepted file formats list them. `webm` is both audio and video there; a
    /// file is taken for video.
    pub fn of(path: &str) -> Self {
        let name = path.rsplit('/').next().unwrap_or(path);
        let Some((_, ext)) = name.rsplit_once('.') else { return Self::Other };
        match ext.to_ascii_lowercase().as_str() {
            "md" => Self::Note,
            "canvas" => Self::Canvas,
            "base" => Self::Base,
            "avif" | "bmp" | "gif" | "jpeg" | "jpg" | "png" | "svg" | "webp" => Self::Image,
            "flac" | "m4a" | "mp3" | "ogg" | "wav" | "3gp" => Self::Audio,
            "mkv" | "mov" | "mp4" | "ogv" | "webm" => Self::Video,
            "pdf" => Self::Pdf,
            _ => Self::Other,
        }
    }

    /// Files that appear in the graph as attachments.
    pub fn is_attachment(self) -> bool {
        !matches!(self, Self::Note)
    }
}

/// What a link points at.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Target {
    File(String),
    /// Names no file. Links that name the same thing share a `key`, which ignores case but not folders:
    /// `a/Note` and `b/Note` are two targets, `a/Note` and `A/NOTE` one.
    Unresolved { key: String, name: String },
}

/// The key a link and the file it resolves to share: the base name, lowercased, without `.md`.
pub fn name_key(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    base.strip_suffix(".md").map(str::to_string).unwrap_or(base)
}

/// The path without a trailing `.md`, in any case.
pub fn stem(path: &str) -> &str {
    match path.len().checked_sub(3).and_then(|i| path.split_at_checked(i)) {
        Some((head, ext)) if ext.eq_ignore_ascii_case(".md") => head,
        _ => path,
    }
}

/// The folder of a vault path, empty at the root.
pub fn dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// `path` as a normalized vault path, or None when it is empty or leaves the vault by `..`.
pub fn normalize(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for component in Utf8UnixPath::new(path).components() {
        match component {
            Utf8UnixComponent::Normal(part) => parts.push(part),
            Utf8UnixComponent::ParentDir => {
                parts.pop()?;
            }
            Utf8UnixComponent::RootDir | Utf8UnixComponent::CurDir => {}
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `target` relative to the folder of the note `from`, normalized.
pub fn join_relative(from: &str, target: &str) -> Option<String> {
    normalize(&format!("{}/{target}", dir(from)))
}

fn is_explicitly_relative(target: &str) -> bool {
    target.starts_with("./") || target.starts_with("../")
}

// Fewer folders first, then the shorter path, then by code point.
fn by_shortest_path(a: &&str, b: &&str) -> Ordering {
    let folders = |p: &str| p.matches('/').count();
    folders(a)
        .cmp(&folders(b))
        .then_with(|| a.chars().count().cmp(&b.chars().count()))
        .then_with(|| a.cmp(b))
}

fn ends_with_path(file: &str, tail: &str) -> bool {
    file == tail || file.strip_suffix(tail).is_some_and(|head| head.ends_with('/'))
}

/// The files of a vault, found by name. Hidden folders (`.obsidian`, `.theorem`) are not part of it;
/// files that settings exclude are, since links to them still resolve.
#[derive(Clone, Debug, Default)]
pub struct FileIndex {
    named: HashMap<String, BTreeSet<String>>,
}

impl FileIndex {
    pub fn new<S: Into<String>>(paths: impl IntoIterator<Item = S>) -> Self {
        let mut index = Self::default();
        for path in paths {
            index.insert(path);
        }
        index
    }

    pub fn insert(&mut self, path: impl Into<String>) -> bool {
        let path = path.into();
        self.named.entry(name_key(&path)).or_default().insert(path)
    }

    pub fn remove(&mut self, path: &str) -> bool {
        let key = name_key(path);
        let Some(files) = self.named.get_mut(&key) else { return false };
        let removed = files.remove(path);
        if files.is_empty() {
            self.named.remove(&key);
        }
        removed
    }

    pub fn contains(&self, path: &str) -> bool {
        self.files_named(&name_key(path)).is_some_and(|files| files.contains(path))
    }

    /// Every path, in no particular order.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.named.values().flatten().map(String::as_str)
    }

    /// The files whose name key is `key`, in path order.
    pub fn named<'a>(&'a self, key: &str) -> impl Iterator<Item = &'a str> + use<'a> {
        self.files_named(key).into_iter().flatten().map(String::as_str)
    }

    fn files_named(&self, key: &str) -> Option<&BTreeSet<String>> {
        self.named.get(key)
    }

    // The file at `path` or `path.md`, matching case exactly.
    fn exactly(&self, path: &str) -> Option<&str> {
        let files = self.files_named(&name_key(path))?;
        files.get(path).or_else(|| files.get(&format!("{path}.md"))).map(String::as_str)
    }

    // The shortest-path file whose path ends with `target`'s segments; case-sensitive matches before
    // case-insensitive ones.
    fn by_suffix(&self, target: &str) -> Option<&str> {
        let want = stem(target);
        let files = || self.named(&name_key(target));
        let mut pool: Vec<&str> = files().filter(|file| ends_with_path(stem(file), want)).collect();
        if pool.is_empty() {
            let want = want.to_lowercase();
            pool = files().filter(|file| ends_with_path(&stem(file).to_lowercase(), &want)).collect();
        }
        pool.into_iter().min_by(by_shortest_path)
    }

    // The file at `path` or `path.md`, ignoring case.
    fn ignoring_case(&self, path: &str) -> Option<&str> {
        let want = stem(path).to_lowercase();
        self.named(&name_key(path)).find(|file| stem(file).to_lowercase() == want)
    }

    /// The file `target` (without `#heading` or `#^block`) names in the note `from`, or None.
    ///
    /// - `./` and `../` targets and Markdown destinations are tried relative to the linking note first.
    /// - A leading `/` means the vault root.
    /// - Then the target as a vault path, with or without `.md`.
    /// - Then, as Obsidian does for `[[Note]]` and `[[folder/Note]]`, the file with the shortest path that
    ///   ends with the target, matching case if any does.
    /// - `LinkSyntax::Path` is a vault path and nothing else.
    pub fn resolve(&self, target: &str, from: &str, syntax: LinkSyntax) -> Option<&str> {
        if syntax == LinkSyntax::Path {
            return self.exactly(target);
        }
        let target = target.trim();
        if target.is_empty() {
            return None;
        }
        let explicit = is_explicitly_relative(target);
        if !target.starts_with('/') && (syntax == LinkSyntax::Markdown || explicit) {
            let joined = join_relative(from, target)?;
            if let Some(found) = self.exactly(&joined) {
                return Some(found);
            }
            if explicit {
                return self.ignoring_case(&joined);
            }
        }
        let vault_path = normalize(target.trim_start_matches('/'))?;
        self.exactly(&vault_path).or_else(|| self.by_suffix(&vault_path))
    }

    /// What `link`, written in the note `from`, points at. A link with no target (`[[#Heading]]`) points
    /// at `from` itself.
    pub fn target_of(&self, from: &str, link: &Link) -> Target {
        let name = link.target.trim();
        if name.is_empty() {
            return Target::File(from.to_string());
        }
        match self.resolve(name, from, link.syntax) {
            Some(path) => Target::File(path.to_string()),
            None => Target::Unresolved { key: unresolved_key(name, from, link.syntax), name: name.to_string() },
        }
    }

    /// How a new link names `path` when it should be as short as stays unambiguous: the base name, or
    /// the fewest trailing folders no other file shares. Notes leave out `.md`; other files keep their
    /// extension.
    pub fn shortest_name(&self, path: &str) -> String {
        let shown = if FileKind::of(path) == FileKind::Note { stem(path) } else { path };
        let segments: Vec<&str> = shown.split('/').collect();
        let key = name_key(path);
        for take in 1..=segments.len() {
            let tail = segments[segments.len() - take..].join("/");
            let want = tail.to_lowercase();
            let rivals = self
                .named(&key)
                .filter(|file| ends_with_path(&stem(file).to_lowercase(), &want))
                .filter(|file| *file != path)
                .count();
            if rivals == 0 {
                return tail;
            }
        }
        shown.to_string()
    }

    /// `wanted`, or the first of `wanted 1`, `wanted 2`, ... that no file has, ignoring case as a
    /// case-insensitive disk does.
    pub fn free_path(&self, wanted: &str) -> String {
        let taken = |candidate: &str| {
            let lower = candidate.to_lowercase();
            self.named(&name_key(candidate)).any(|file| file.to_lowercase() == lower)
        };
        if !taken(wanted) {
            return wanted.to_string();
        }
        let (stem, ext) = match wanted.rsplit_once('.') {
            Some((stem, ext)) if !stem.ends_with('/') && !ext.contains('/') && !stem.is_empty() => (stem, format!(".{ext}")),
            _ => (wanted, String::new()),
        };
        (1..).map(|n| format!("{stem} {n}{ext}")).find(|candidate| !taken(candidate)).expect("an unbounded range")
    }
}

// Unresolved links share a node by what they name, compared without case. Relative destinations are
// first made vault paths, since the same text names different files from different folders.
fn unresolved_key(name: &str, from: &str, syntax: LinkSyntax) -> String {
    let relative = !name.starts_with('/') && (syntax == LinkSyntax::Markdown || is_explicitly_relative(name));
    let named = if relative { join_relative(from, name) } else { None };
    named.as_deref().unwrap_or(name).to_lowercase()
}

// Obsidian matches headings, and writes them into links, with these replaced by spaces.
static UNSAFE_IN_LINKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"%%|\[\[|\]\]|[:#|^\\]").unwrap());

fn link_safe_heading(text: &str) -> String {
    UNSAFE_IN_LINKS.replace_all(text, " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How a heading is compared: unsafe link characters as spaces, spacing collapsed, case ignored.
pub fn heading_key(text: &str) -> String {
    link_safe_heading(text).to_lowercase()
}

/// A heading as a link writes it after `#`.
pub fn heading_fragment(text: &str) -> String {
    link_safe_heading(text)
}

/// Where in `note` a heading or block fragment points, or None.
///
/// A heading is matched whole first, so `C# basics` is found before `A#B` is read as A, then B. Then `A#B`
/// is the heading B inside the section of A; failing that, the last part alone, anywhere.
pub fn locate(note: &Note, fragment: &Fragment) -> Option<Span> {
    match fragment {
        Fragment::Block(id) => note.blocks.iter().find(|b| b.id.eq_ignore_ascii_case(id)).map(|b| b.span),
        Fragment::Heading(path) => {
            let by_key = |key: &str| {
                let found = note.headings.iter().find(|h| heading_key(&h.text) == key);
                found.filter(|_| !key.is_empty()).map(|h| h.span)
            };
            by_key(&heading_key(path))
                .or_else(|| in_sections(note, path))
                .or_else(|| path.rsplit('#').next().and_then(|last| by_key(&heading_key(last))))
        }
    }
}

// `A#B#C`: the first heading A, then the first heading B in the section of A, then C in that of B. A
// section runs to the next heading of the same level or higher.
fn in_sections(note: &Note, path: &str) -> Option<Span> {
    let headings = &note.headings;
    let (mut from, mut limit) = (0, headings.len());
    let mut found = None;
    for part in path.split('#').map(heading_key).filter(|part| !part.is_empty()) {
        let at = (from..limit).find(|&i| heading_key(&headings[i].text) == part)?;
        let level = headings[at].level;
        limit = (at + 1..limit).find(|&i| headings[i].level <= level).unwrap_or(limit);
        from = at + 1;
        found = Some(headings[at].span);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_note;
    use pretty_assertions::assert_eq;

    fn vault() -> FileIndex {
        FileIndex::new([
            "Ideas.md",
            "archive/Ideas.md",
            "notes/Plan.md",
            "notes/sub/Deep.md",
            "projects/alpha/Deep.md",
            "specs/My Spec.md",
            "img/diagram.png",
        ])
    }

    fn wiki<'a>(index: &'a FileIndex, target: &str, from: &str) -> Option<&'a str> {
        index.resolve(target, from, LinkSyntax::Wiki)
    }

    #[test]
    fn a_bare_name_takes_the_shortest_path_with_that_name() {
        let v = vault();
        assert_eq!(wiki(&v, "Ideas", "notes/Plan.md"), Some("Ideas.md"));
        assert_eq!(wiki(&v, "Deep", "Ideas.md"), Some("notes/sub/Deep.md"));
    }

    #[test]
    fn a_path_picks_among_files_with_the_same_name() {
        let v = vault();
        assert_eq!(wiki(&v, "archive/Ideas", "notes/Plan.md"), Some("archive/Ideas.md"));
        assert_eq!(wiki(&v, "alpha/Deep", "Ideas.md"), Some("projects/alpha/Deep.md"));
        assert_eq!(wiki(&v, "projects/alpha/Deep.md", "Ideas.md"), Some("projects/alpha/Deep.md"));
    }

    #[test]
    fn case_is_matched_exactly_before_it_is_ignored() {
        let v = FileIndex::new(["a/Note.md", "b/note.md", "Ideas.md"]);
        assert_eq!(wiki(&v, "note", "x.md"), Some("b/note.md"));
        assert_eq!(wiki(&v, "Note", "x.md"), Some("a/Note.md"));
        assert_eq!(wiki(&v, "NOTE", "x.md"), Some("a/Note.md"));
        assert_eq!(wiki(&v, "ideas", "x.md"), Some("Ideas.md"));
        assert_eq!(wiki(&vault(), "NOTES/plan.md", "x.md"), Some("notes/Plan.md"));
    }

    #[test]
    fn relative_and_absolute_targets() {
        let v = vault();
        assert_eq!(wiki(&v, "./sub/Deep", "notes/Plan.md"), Some("notes/sub/Deep.md"));
        assert_eq!(v.resolve("../specs/My Spec.md", "notes/Plan.md", LinkSyntax::Markdown), Some("specs/My Spec.md"));
        assert_eq!(wiki(&v, "/Ideas", "archive/x.md"), Some("Ideas.md"));
        assert_eq!(v.resolve("../../outside.md", "notes/Plan.md", LinkSyntax::Markdown), None);
        // An explicit relative path ignores case only against the folder it names.
        assert_eq!(wiki(&v, "./SUB/deep", "notes/Plan.md"), Some("notes/sub/Deep.md"));
        assert_eq!(wiki(&v, "../Ideas", "notes/Plan.md"), Some("Ideas.md"));
    }

    #[test]
    fn markdown_destinations_are_relative_to_the_note_first() {
        let v = FileIndex::new(["Ideas.md", "notes/Ideas.md"]);
        assert_eq!(v.resolve("Ideas.md", "notes/Plan.md", LinkSyntax::Markdown), Some("notes/Ideas.md"));
        assert_eq!(v.resolve("Ideas.md", "Plan.md", LinkSyntax::Markdown), Some("Ideas.md"));
        // A wikilink is not relative: the shortest path wins.
        assert_eq!(wiki(&v, "Ideas", "notes/Plan.md"), Some("Ideas.md"));
        // A Markdown destination that is not under the note's folder falls back to the vault path.
        assert_eq!(v.resolve("notes/Ideas.md", "other/Plan.md", LinkSyntax::Markdown), Some("notes/Ideas.md"));
    }

    #[test]
    fn attachments_keep_their_extension_and_missing_targets_resolve_to_nothing() {
        let v = vault();
        assert_eq!(wiki(&v, "diagram.png", "Ideas.md"), Some("img/diagram.png"));
        assert_eq!(wiki(&v, "diagram", "Ideas.md"), None);
        assert_eq!(wiki(&v, "Nowhere", "Ideas.md"), None);
        assert_eq!(wiki(&v, "", "Ideas.md"), None);
        assert_eq!(wiki(&v, "   ", "Ideas.md"), None);
        assert_eq!(wiki(&v, "..", "Ideas.md"), None);
        assert_eq!(wiki(&v, "/", "Ideas.md"), None);
    }

    #[test]
    fn a_path_target_names_one_file_exactly() {
        let v = vault();
        assert_eq!(v.resolve("notes/Plan.md", "x.canvas", LinkSyntax::Path), Some("notes/Plan.md"));
        assert_eq!(v.resolve("Plan.md", "x.canvas", LinkSyntax::Path), None);
        assert_eq!(v.resolve("notes/plan.md", "x.canvas", LinkSyntax::Path), None);
        assert_eq!(v.resolve("img/diagram.png", "x.canvas", LinkSyntax::Path), Some("img/diagram.png"));
    }

    #[test]
    fn a_link_to_a_heading_of_the_same_note_points_at_that_note() {
        let v = vault();
        let note = parse_note("[[#Later]] [[Nowhere]]");
        assert_eq!(v.target_of("Ideas.md", &note.links[0]), Target::File("Ideas.md".into()));
        assert_eq!(
            v.target_of("Ideas.md", &note.links[1]),
            Target::Unresolved { key: "nowhere".into(), name: "Nowhere".into() }
        );
    }

    // PR 16 review finding: unresolved links dedupe by the whole target, not by the file name.
    #[test]
    fn unresolved_links_in_different_folders_stay_two_targets() {
        let v = FileIndex::new(["Home.md"]);
        let note = parse_note("[[a/Note]] [[b/Note]] [[A/NOTE]] [[ a/note ]]");
        let keys: Vec<_> = note
            .links
            .iter()
            .map(|l| match v.target_of("Home.md", l) {
                Target::Unresolved { key, .. } => key,
                Target::File(f) => panic!("{f} should not resolve"),
            })
            .collect();
        assert_eq!(keys, ["a/note", "b/note", "a/note", "a/note"]);
    }

    #[test]
    fn unresolved_relative_links_are_keyed_by_the_file_they_would_make() {
        let v = FileIndex::default();
        let note = parse_note("[x](Gone.md) [y](../Gone.md)");
        let key = |from: &str, i: usize| match v.target_of(from, &note.links[i]) {
            Target::Unresolved { key, .. } => key,
            Target::File(f) => panic!("{f}"),
        };
        assert_eq!(key("a/b.md", 0), "a/gone.md");
        assert_eq!(key("a/b.md", 1), "gone.md");
        assert_eq!(key("c/d.md", 0), "c/gone.md");
    }

    #[test]
    fn the_index_follows_creations_removals_and_moves() {
        let mut v = vault();
        assert!(v.contains("Ideas.md") && !v.contains("ideas.md"));
        assert_eq!(wiki(&v, "Ideas", "x.md"), Some("Ideas.md"));
        assert!(v.remove("Ideas.md"));
        assert!(!v.remove("Ideas.md"));
        assert_eq!(wiki(&v, "Ideas", "x.md"), Some("archive/Ideas.md"));
        assert!(v.insert("moved/Ideas.md"));
        assert!(!v.insert("moved/Ideas.md"));
        // Same depth, so the shorter path wins.
        assert_eq!(wiki(&v, "Ideas", "x.md"), Some("moved/Ideas.md"));
        assert_eq!(v.paths().count(), 7);
        assert_eq!(v.named("ideas").collect::<Vec<_>>(), ["archive/Ideas.md", "moved/Ideas.md"]);
    }

    #[test]
    fn file_kinds_follow_the_extension() {
        let kinds: Vec<_> = ["a.md", "a.MD", "b.canvas", "c.base", "d.PNG", "e.mp3", "f.mp4", "g.webm", "h.pdf", "i.txt", "j", "dir.v2/k"]
            .map(FileKind::of)
            .into();
        use FileKind::*;
        assert_eq!(kinds, [Note, Note, Canvas, Base, Image, Audio, Video, Video, Pdf, Other, Other, Other]);
        assert!(Image.is_attachment() && !Note.is_attachment());
    }

    #[test]
    fn a_new_link_names_a_file_as_short_as_stays_unambiguous() {
        let v = FileIndex::new(["Ideas.md", "archive/Ideas.md", "x/y/Note.md", "z/Note.md", "Solo.md", "img/diagram.png", "img/a/diagram.png"]);
        assert_eq!(v.shortest_name("Solo.md"), "Solo");
        assert_eq!(v.shortest_name("Ideas.md"), "Ideas");
        assert_eq!(v.shortest_name("archive/Ideas.md"), "archive/Ideas");
        assert_eq!(v.shortest_name("x/y/Note.md"), "y/Note");
        assert_eq!(v.shortest_name("z/Note.md"), "z/Note");
        assert_eq!(v.shortest_name("img/diagram.png"), "img/diagram.png");
        assert_eq!(v.shortest_name("img/a/diagram.png"), "a/diagram.png");
        for path in v.paths() {
            let name = v.shortest_name(path);
            let back = v.resolve(&name, "Elsewhere.md", LinkSyntax::Wiki);
            assert_eq!(back, Some(path), "{name} resolves back to {path}");
        }
    }

    #[test]
    fn free_paths_count_up_and_ignore_case() {
        let v = FileIndex::new(["Untitled.md", "Untitled 1.md", "notes/image.png", "Archive"]);
        assert_eq!(v.free_path("Fresh.md"), "Fresh.md");
        assert_eq!(v.free_path("untitled.md"), "untitled 2.md");
        assert_eq!(v.free_path("notes/Image.png"), "notes/Image 1.png");
        assert_eq!(v.free_path("archive"), "archive 1");
    }

    #[test]
    fn normalizing_rejects_what_leaves_the_vault() {
        assert_eq!(normalize("a//b/./c/../d"), Some("a/b/d".into()));
        assert_eq!(normalize("/a"), Some("a".into()));
        assert_eq!(normalize("a/.."), None);
        assert_eq!(normalize("../a"), None);
        assert_eq!(normalize(""), None);
        assert_eq!(join_relative("notes/Plan.md", "../Ideas.md"), Some("Ideas.md".into()));
        assert_eq!(join_relative("Plan.md", "./x"), Some("x".into()));
        assert_eq!(join_relative("Plan.md", "../x"), None);
    }

    const NOTE: &str = "# Notes\n\n## basics\n\nOne.\n\n## C# basics\n\nTwo. ^two\n\n# Part A\n\n## Setup\n\n### Detail\n\n# Part B\n\n## Setup\n\n### Other\n\n## Bold **and** `code`\n";

    fn at(fragment: Fragment) -> Option<String> {
        let note = parse_note(NOTE);
        locate(&note, &fragment).map(|s| s.slice(NOTE).to_string())
    }

    fn heading(text: &str) -> Fragment {
        Fragment::Heading(text.into())
    }

    // PR 16 review finding: a heading whose own text holds `#` is reachable.
    #[test]
    fn a_heading_holding_a_hash_is_found_whole() {
        assert_eq!(at(heading("C# basics")).as_deref(), Some("## C# basics"));
        assert_eq!(at(heading("basics")).as_deref(), Some("## basics"));
        // Neither is mistaken for the other, and case and spacing do not matter.
        assert_eq!(at(heading("  c#   BASICS ")).as_deref(), Some("## C# basics"));
    }

    #[test]
    fn nested_headings_are_found_inside_their_sections() {
        assert_eq!(at(heading("Part A#Setup#Detail")).as_deref(), Some("### Detail"));
        assert_eq!(at(heading("Part B#Setup")).as_deref(), Some("## Setup"));
        let note = parse_note(NOTE);
        let b = locate(&note, &heading("Part B#Setup")).unwrap();
        let a = locate(&note, &heading("Part A#Setup")).unwrap();
        assert!(b.start > a.start, "the Setup under Part B is not the one under Part A");
        assert_eq!(at(heading("Part B#Setup#Other")).as_deref(), Some("### Other"));
    }

    #[test]
    fn a_chain_that_does_not_fit_falls_back_to_its_last_part() {
        // Detail is under Part A, not Part B; the last part is found anywhere.
        assert_eq!(at(heading("Part B#Detail")).as_deref(), Some("### Detail"));
        assert_eq!(at(heading("Nowhere#basics")).as_deref(), Some("## basics"));
        assert_eq!(at(heading("Nowhere")), None);
        assert_eq!(at(heading("")), None);
    }

    #[test]
    fn headings_compare_without_the_characters_a_link_cannot_hold() {
        assert_eq!(heading_key("Why: a | b ^ c %% [[d]]"), "why a b c d");
        assert_eq!(heading_fragment("Why: a  |  b"), "Why a b");
        assert_eq!(at(heading("Bold **and** `code`")).as_deref(), Some("## Bold **and** `code`"));
        let note = parse_note("## Why: this\n");
        assert!(locate(&note, &heading("Why this")).is_some());
        assert!(locate(&note, &heading("Why: this")).is_some());
    }

    #[test]
    fn blocks_are_found_by_id() {
        assert_eq!(at(Fragment::Block("two".into())).as_deref(), Some("Two. ^two"));
        assert_eq!(at(Fragment::Block("TWO".into())).as_deref(), Some("Two. ^two"));
        assert_eq!(at(Fragment::Block("nope".into())), None);
    }
}
