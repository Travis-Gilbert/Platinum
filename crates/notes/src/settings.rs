//! The vault's own settings (`.obsidian/app.json`, `types.json`) and what they decide for exclusions, new notes, attachments and new links.
use crate::model::{Fragment, LinkSyntax, PropertyType};
use crate::resolve::{FileIndex, FileKind, dir, join_relative, normalize, stem};
use anyhow::Result;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NewFileLocation {
    /// The vault's root folder.
    #[default]
    Root,
    /// The folder of the note the link is in.
    Current,
    /// `VaultSettings::new_file_folder`.
    Folder,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LinkFormat {
    /// The base name, or as many folders as keep it unambiguous.
    #[default]
    Shortest,
    /// Relative to the note the link is in.
    Relative,
    /// From the vault root.
    Absolute,
}

#[derive(Clone, Debug)]
enum Exclusion {
    Prefix(String),
    Pattern(Regex),
}

#[derive(Clone, Debug)]
pub struct VaultSettings {
    pub new_file_location: NewFileLocation,
    /// Used when `new_file_location` is `Folder`.
    pub new_file_folder: String,
    /// `/` is the vault root, `./` the folder of the note, `./name` a folder inside that, anything else a
    /// folder from the root.
    pub attachment_folder: String,
    /// New links are `[text](Note.md)` rather than `[[Note]]`.
    pub use_markdown_links: bool,
    pub link_format: LinkFormat,
    exclusions: Vec<Exclusion>,
    pub property_types: HashMap<String, PropertyType>,
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            new_file_location: NewFileLocation::Root,
            new_file_folder: String::new(),
            attachment_folder: "/".into(),
            use_markdown_links: false,
            link_format: LinkFormat::Shortest,
            exclusions: Vec::new(),
            property_types: HashMap::new(),
        }
    }
}

// Characters that cannot be in a file name.
const FORBIDDEN: &[char] = &['\\', ':', '*', '?', '"', '<', '>', '|'];

// Everything but unreserved characters and `/` is escaped in a Markdown destination.
const DESTINATION: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~').remove(b'/');

/// Files in dot folders (`.obsidian`, `.theorem`, `.trash`) and dot files are not part of the vault.
pub fn is_hidden(path: &str) -> bool {
    path.split('/').any(|part| part.starts_with('.'))
}

impl VaultSettings {
    /// Reads `.obsidian/app.json`. A missing or wrongly typed setting keeps its default.
    pub fn from_app_json(json: &str) -> Result<Self> {
        let app: Value = serde_json::from_str(json)?;
        let text = |key: &str| app.get(key).and_then(Value::as_str);
        let filters = app.get("userIgnoreFilters").and_then(Value::as_array).into_iter().flatten();
        Ok(Self {
            new_file_location: match text("newFileLocation") {
                Some("current") => NewFileLocation::Current,
                Some("folder") => NewFileLocation::Folder,
                _ => NewFileLocation::Root,
            },
            new_file_folder: text("newFileFolderPath").unwrap_or_default().trim_matches('/').to_string(),
            attachment_folder: text("attachmentFolderPath").unwrap_or("/").to_string(),
            use_markdown_links: app.get("useMarkdownLinks").and_then(Value::as_bool).unwrap_or(false),
            link_format: match text("newLinkFormat") {
                Some("relative") => LinkFormat::Relative,
                Some("absolute") => LinkFormat::Absolute,
                _ => LinkFormat::Shortest,
            },
            exclusions: filters.filter_map(Value::as_str).filter_map(exclusion).collect(),
            ..Self::default()
        })
    }

    /// Reads `.obsidian/types.json`, the property types the vault has declared.
    pub fn read_property_types(&mut self, json: &str) -> Result<()> {
        let types: Value = serde_json::from_str(json)?;
        let declared = types.get("types").and_then(Value::as_object).into_iter().flatten();
        self.property_types = declared
            .filter_map(|(key, kind)| {
                let kind = match kind.as_str()? {
                    "text" => PropertyType::Text,
                    "multitext" => PropertyType::List,
                    "number" => PropertyType::Number,
                    "checkbox" => PropertyType::Checkbox,
                    "date" => PropertyType::Date,
                    "datetime" => PropertyType::DateTime,
                    "tags" => PropertyType::Tags,
                    "aliases" => PropertyType::Aliases,
                    _ => return None,
                };
                Some((key.clone(), kind))
            })
            .collect();
        Ok(())
    }

    /// The type of a property: the vault's declaration, else what its value looks like.
    pub fn property_type(&self, key: &str, value: &Value) -> PropertyType {
        self.property_types.get(key).copied().unwrap_or_else(|| PropertyType::infer(key, value))
    }

    /// Excluded files still resolve links, but search, the graph and unlinked mentions leave them out.
    pub fn is_excluded(&self, path: &str) -> bool {
        self.exclusions.iter().any(|e| match e {
            Exclusion::Prefix(prefix) => path.starts_with(prefix.as_str()),
            Exclusion::Pattern(pattern) => pattern.is_match(path),
        })
    }

    /// Where the note an unresolved link names would be made, or None when nothing should be: the target is
    /// not a note, is not a possible file name, or would land in a hidden folder or outside the vault.
    ///
    /// A path (`[[folder/Name]]`, `[[/Name]]`, `[[./Name]]`, any Markdown destination) is made where it says.
    /// A bare name goes where the vault keeps new notes. `.md` is added unless the target has it.
    pub fn new_note_path(&self, target: &str, from: &str, syntax: LinkSyntax) -> Option<String> {
        let target = target.trim();
        let relative = target.starts_with("./") || target.starts_with("../");
        let path = if target.starts_with('/') {
            normalize(target)?
        } else if syntax == LinkSyntax::Markdown || relative {
            join_relative(from, target)?
        } else if target.contains('/') {
            normalize(target)?
        } else {
            let folder = match self.new_file_location {
                NewFileLocation::Root => "",
                NewFileLocation::Current => dir(from),
                NewFileLocation::Folder => &self.new_file_folder,
            };
            normalize(&format!("{folder}/{target}"))?
        };
        let path = match FileKind::of(&path) {
            FileKind::Note => path,
            FileKind::Other => format!("{path}.md"),
            _ => return None,
        };
        let valid = path.split('/').all(|part| !part.contains(FORBIDDEN) && !part.contains(char::is_control));
        (valid && !is_hidden(&path)).then_some(path)
    }

    /// Where an attachment called `name` goes when it is added to the note `from`.
    pub fn attachment_path(&self, from: &str, name: &str) -> String {
        let folder = match self.attachment_folder.as_str() {
            "" | "/" => String::new(),
            "./" | "." => dir(from).to_string(),
            relative if relative.starts_with("./") => format!("{}/{}", dir(from), &relative[2..]),
            folder => folder.to_string(),
        };
        normalize(&format!("{folder}/{name}")).unwrap_or_else(|| name.to_string())
    }

    /// The text of a new link in the note `from` to the file `to`, in the vault's style: a wikilink or a
    /// Markdown link, named as short, relative or absolute as the vault prefers. `fragment` is written as given
    /// (`heading_fragment` makes a heading's text safe), `text` is the alias or the link text, and `embed`
    /// adds the `!`.
    ///
    /// The name is checked by resolving it from `from`; one that would reach another file, as a relative
    /// name can when another file shares its tail, becomes `./` relative and then a path from the root.
    pub fn link_to(
        &self,
        index: &FileIndex,
        from: &str,
        to: &str,
        fragment: Option<&Fragment>,
        text: Option<&str>,
        embed: bool,
    ) -> String {
        let markdown = self.use_markdown_links;
        let syntax = if markdown { LinkSyntax::Markdown } else { LinkSyntax::Wiki };
        let note = FileKind::of(to) == FileKind::Note;
        // Wikilinks leave `.md` out; Markdown links keep it.
        let shown = |path: &str| if note && !markdown { stem(path).to_string() } else { path.to_string() };
        let relative = pathdiff::diff_paths(to, dir(from)).map_or_else(|| to.to_string(), |p| p.to_string_lossy().into_owned());
        let primary = match self.link_format {
            LinkFormat::Shortest if note && markdown => format!("{}.md", index.shortest_name(to)),
            LinkFormat::Shortest => index.shortest_name(to),
            LinkFormat::Relative => shown(&relative),
            LinkFormat::Absolute => shown(to),
        };
        let dotted = if relative.starts_with("..") { shown(&relative) } else { shown(&format!("./{relative}")) };
        let rooted = shown(&format!("/{to}"));
        let name = [primary.clone(), dotted, rooted]
            .into_iter()
            .find(|candidate| index.resolve(candidate, from, syntax) == Some(to))
            .unwrap_or(primary);

        let fragment = fragment.map(Fragment::to_string).unwrap_or_default();
        if !markdown {
            let alias = text.map(|t| format!("|{t}")).unwrap_or_default();
            return format!("{}[[{name}{fragment}{alias}]]", if embed { "!" } else { "" });
        }
        let label = match (text, embed) {
            (Some(text), _) => text.to_string(),
            (None, true) => String::new(),
            (None, false) => {
                let file = to.rsplit('/').next().unwrap_or(to);
                let file = if note { stem(file) } else { file };
                match fragment.strip_prefix('#').filter(|f| !f.starts_with('^')) {
                    Some(heading) => format!("{file} > {heading}"),
                    None => file.to_string(),
                }
            }
        };
        let fragment = fragment.split('#').map(|part| utf8_percent_encode(part, DESTINATION).to_string()).collect::<Vec<_>>().join("#");
        let destination = format!("{}{fragment}", utf8_percent_encode(&name, DESTINATION));
        format!("{}[{label}]({destination})", if embed { "!" } else { "" })
    }
}

// `/regex/` is a pattern; anything else is a path prefix. A pattern that does not compile is ignored, as
// the JavaScript patterns that Rust's engine cannot run (look-around) are.
fn exclusion(filter: &str) -> Option<Exclusion> {
    match filter.strip_prefix('/').and_then(|rest| rest.strip_suffix('/')) {
        Some(pattern) if !pattern.is_empty() => Regex::new(pattern).ok().map(Exclusion::Pattern),
        _ => (!filter.is_empty()).then(|| Exclusion::Prefix(filter.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn settings(app: &str) -> VaultSettings {
        VaultSettings::from_app_json(app).unwrap()
    }

    #[test]
    fn an_empty_app_json_keeps_the_defaults() {
        let s = settings("{}");
        assert_eq!(s.new_file_location, NewFileLocation::Root);
        assert_eq!(s.attachment_folder, "/");
        assert_eq!(s.link_format, LinkFormat::Shortest);
        assert!(!s.use_markdown_links && !s.is_excluded("anything.md"));
        assert!(VaultSettings::from_app_json("{").is_err());
    }

    #[test]
    fn app_json_is_read_and_wrongly_typed_settings_are_ignored() {
        let s = settings(
            r#"{"newFileLocation":"folder","newFileFolderPath":"/Inbox/","attachmentFolderPath":"./files",
                "useMarkdownLinks":true,"newLinkFormat":"relative","userIgnoreFilters":["Private/", 4],"alwaysUpdateLinks":true}"#,
        );
        assert_eq!(s.new_file_location, NewFileLocation::Folder);
        assert_eq!(s.new_file_folder, "Inbox");
        assert_eq!(s.attachment_folder, "./files");
        assert!(s.use_markdown_links);
        assert_eq!(s.link_format, LinkFormat::Relative);
        let bad = settings(r#"{"newFileLocation":3,"useMarkdownLinks":"yes","newLinkFormat":[],"userIgnoreFilters":"nope"}"#);
        assert_eq!(bad.new_file_location, NewFileLocation::Root);
        assert!(!bad.use_markdown_links);
        assert_eq!(bad.link_format, LinkFormat::Shortest);
    }

    #[test]
    fn exclusions_are_prefixes_or_slashed_patterns() {
        let s = settings(r#"{"userIgnoreFilters":["Private/","Templates","/\\.draft\\.md$/","/(?=look)/","/[/","/"]}"#);
        assert!(s.is_excluded("Private/secret.md"));
        assert!(s.is_excluded("Templates/Daily.md") && s.is_excluded("Templates old/x.md"));
        assert!(!s.is_excluded("Notes/Private/x.md"));
        assert!(s.is_excluded("Notes/plan.draft.md") && !s.is_excluded("Notes/plan.md"));
        // Patterns Rust cannot compile and the lone slash are dropped rather than excluding everything.
        assert!(!s.is_excluded("lookup.md") && !s.is_excluded("x/y.md"));
    }

    #[test]
    fn dot_folders_and_files_are_hidden() {
        assert!(is_hidden(".obsidian/app.json") && is_hidden("a/.theorem/ids.json") && is_hidden(".trash/x.md") && is_hidden("a/.hidden"));
        assert!(!is_hidden("a/b.md") && !is_hidden("v1.2/notes.md"));
    }

    #[test]
    fn property_types_come_from_the_vault_before_the_value() {
        let mut s = VaultSettings::default();
        s.read_property_types(r#"{"types":{"rating":"number","status":"text","due":"date","labels":"multitext","bogus":"banana"}}"#).unwrap();
        assert_eq!(s.property_type("rating", &json!("5")), PropertyType::Number);
        assert_eq!(s.property_type("status", &json!(true)), PropertyType::Text);
        assert_eq!(s.property_type("labels", &json!("one")), PropertyType::List);
        assert_eq!(s.property_type("bogus", &json!(1)), PropertyType::Number);
        assert_eq!(s.property_type("done", &json!(true)), PropertyType::Checkbox);
        assert_eq!(s.property_type("tags", &json!(["a"])), PropertyType::Tags);
        assert!(s.read_property_types("[").is_err());
    }

    // PR 16 review finding: a missing target never becomes Missing.md.md.
    #[test]
    fn a_new_note_never_gets_a_double_extension() {
        let s = VaultSettings::default();
        for (target, want) in [
            ("Missing", "Missing.md"),
            ("Missing.md", "Missing.md"),
            ("Missing.MD", "Missing.MD"),
            ("folder/Missing", "folder/Missing.md"),
            ("folder/Missing.md", "folder/Missing.md"),
            ("v1.2 notes", "v1.2 notes.md"),
            (" Spaced ", "Spaced.md"),
        ] {
            assert_eq!(s.new_note_path(target, "a/b.md", LinkSyntax::Wiki).as_deref(), Some(want), "{target}");
            assert!(!s.new_note_path(target, "a/b.md", LinkSyntax::Wiki).unwrap().to_lowercase().ends_with(".md.md"));
        }
    }

    #[test]
    fn a_new_note_goes_where_the_target_or_the_vault_says() {
        let from = "notes/Plan.md";
        let root = VaultSettings::default();
        let current = settings(r#"{"newFileLocation":"current"}"#);
        let folder = settings(r#"{"newFileLocation":"folder","newFileFolderPath":"Inbox"}"#);
        let wiki = |s: &VaultSettings, t: &str| s.new_note_path(t, from, LinkSyntax::Wiki);
        assert_eq!(wiki(&root, "New").as_deref(), Some("New.md"));
        assert_eq!(wiki(&current, "New").as_deref(), Some("notes/New.md"));
        assert_eq!(wiki(&folder, "New").as_deref(), Some("Inbox/New.md"));
        // A path wins over the setting; ./ is relative to the note; / is the vault root.
        assert_eq!(wiki(&folder, "docs/New").as_deref(), Some("docs/New.md"));
        assert_eq!(wiki(&folder, "./New").as_deref(), Some("notes/New.md"));
        assert_eq!(wiki(&folder, "../New").as_deref(), Some("New.md"));
        assert_eq!(wiki(&current, "/New").as_deref(), Some("New.md"));
        // A Markdown destination is always relative to the note.
        assert_eq!(folder.new_note_path("New.md", from, LinkSyntax::Markdown).as_deref(), Some("notes/New.md"));
        assert_eq!(folder.new_note_path("../x/New.md", from, LinkSyntax::Markdown).as_deref(), Some("x/New.md"));
    }

    #[test]
    fn nothing_is_made_for_attachments_bad_names_hidden_folders_or_outside_the_vault() {
        let s = VaultSettings::default();
        for target in ["Figure 1.png", "clip.mp4", "doc.pdf", "Board.canvas", "a.base", "", "  ", "Why? Because", "a:b", "a\\b", "a|b", "../../out", ".obsidian/x", "a/.theorem/x", "..", "/"] {
            assert_eq!(s.new_note_path(target, "notes/Plan.md", LinkSyntax::Wiki), None, "{target}");
        }
    }

    #[test]
    fn attachments_go_where_the_vault_keeps_them() {
        let from = "notes/Plan.md";
        let at = |folder: &str| VaultSettings { attachment_folder: folder.into(), ..Default::default() }.attachment_path(from, "pic.png");
        assert_eq!(at("/"), "pic.png");
        assert_eq!(at(""), "pic.png");
        assert_eq!(at("./"), "notes/pic.png");
        assert_eq!(at("./assets"), "notes/assets/pic.png");
        assert_eq!(at("Attachments"), "Attachments/pic.png");
        assert_eq!(at("Attachments/images"), "Attachments/images/pic.png");
        assert_eq!(VaultSettings::default().attachment_path("Root.md", "pic.png"), "pic.png");
        let same_folder = VaultSettings { attachment_folder: "./".into(), ..Default::default() };
        assert_eq!(same_folder.attachment_path("Root.md", "pic.png"), "pic.png");
    }

    fn vault() -> FileIndex {
        FileIndex::new(["Ideas.md", "archive/Ideas.md", "x/y/Note.md", "z/Note.md", "notes/Plan.md", "notes/Sub Note.md", "Sub Note.md", "img/a b.png"])
    }

    fn link(app: &str, from: &str, to: &str, fragment: Option<&Fragment>, text: Option<&str>, embed: bool) -> String {
        settings(app).link_to(&vault(), from, to, fragment, text, embed)
    }

    #[test]
    fn wikilinks_in_each_format() {
        let f = |app: &str, from, to| link(app, from, to, None, None, false);
        assert_eq!(f("{}", "notes/Plan.md", "Ideas.md"), "[[Ideas]]");
        assert_eq!(f("{}", "notes/Plan.md", "archive/Ideas.md"), "[[archive/Ideas]]");
        assert_eq!(f("{}", "Ideas.md", "x/y/Note.md"), "[[y/Note]]");
        assert_eq!(f("{}", "Ideas.md", "img/a b.png"), "[[a b.png]]");
        let relative = r#"{"newLinkFormat":"relative"}"#;
        assert_eq!(f(relative, "x/Home.md", "x/y/Note.md"), "[[y/Note]]");
        // "Note" from x/y would be z/Note.md, the shorter path, so the name says where it is.
        assert_eq!(f(relative, "x/y/Home.md", "x/y/Note.md"), "[[./Note]]");
        assert_eq!(f(relative, "notes/Plan.md", "archive/Ideas.md"), "[[../archive/Ideas]]");
        assert_eq!(f(relative, "z/Home.md", "z/Note.md"), "[[Note]]");
        let absolute = r#"{"newLinkFormat":"absolute"}"#;
        assert_eq!(f(absolute, "notes/Plan.md", "Ideas.md"), "[[Ideas]]");
        assert_eq!(f(absolute, "notes/Plan.md", "x/y/Note.md"), "[[x/y/Note]]");
    }

    #[test]
    fn a_name_that_would_reach_another_file_falls_back_to_one_that_does_not() {
        // From x/Home.md the relative name "Note" is x/Note.md, but the file wanted is z/Note.md.
        let v = FileIndex::new(["x/Home.md", "x/Note.md", "z/Note.md", "z/y/Note.md", "y/Note.md"]);
        let s = settings(r#"{"newLinkFormat":"relative"}"#);
        assert_eq!(s.link_to(&v, "z/Home.md", "z/y/Note.md", None, None, false), "[[./y/Note]]");
        let text = s.link_to(&v, "x/Home.md", "z/Note.md", None, None, false);
        assert_eq!(text, "[[../z/Note]]");
        for (from, to) in [("x/Home.md", "x/Note.md"), ("x/Home.md", "y/Note.md"), ("z/Home.md", "x/Note.md")] {
            let text = s.link_to(&v, from, to, None, None, false);
            let target = text.trim_start_matches("[[").trim_end_matches("]]");
            assert_eq!(v.resolve(target, from, LinkSyntax::Wiki), Some(to), "{text} from {from}");
        }
    }

    #[test]
    fn wikilinks_carry_fragments_aliases_and_the_embed_mark() {
        let heading = Fragment::Heading("Later on".into());
        let block = Fragment::Block("b1".into());
        assert_eq!(link("{}", "x.md", "Ideas.md", Some(&heading), None, false), "[[Ideas#Later on]]");
        assert_eq!(link("{}", "x.md", "Ideas.md", Some(&block), Some("this"), false), "[[Ideas#^b1|this]]");
        assert_eq!(link("{}", "x.md", "img/a b.png", None, Some("100"), true), "![[a b.png|100]]");
    }

    #[test]
    fn markdown_links_keep_the_extension_and_escape_the_destination() {
        let md = r#"{"useMarkdownLinks":true}"#;
        assert_eq!(link(md, "x.md", "Ideas.md", None, None, false), "[Ideas](Ideas.md)");
        // "Sub Note" exists at the root and in notes/, so a link from elsewhere needs the folder.
        assert_eq!(link(md, "x.md", "notes/Sub Note.md", None, None, false), "[Sub Note](notes/Sub%20Note.md)");
        assert_eq!(link(md, "x.md", "img/a b.png", None, None, true), "![](a%20b.png)");
        let heading = Fragment::Heading("Later on".into());
        assert_eq!(link(md, "x.md", "Ideas.md", Some(&heading), None, false), "[Ideas > Later on](Ideas.md#Later%20on)");
        assert_eq!(link(md, "x.md", "Ideas.md", Some(&Fragment::Block("b1".into())), Some("see"), false), "[see](Ideas.md#%5Eb1)");
    }

    #[test]
    fn every_generated_link_resolves_back_to_its_file() {
        let v = vault();
        let paths: Vec<_> = v.paths().map(str::to_string).collect();
        for format in ["shortest", "relative", "absolute"] {
            for markdown in [false, true] {
                let s = settings(&format!(r#"{{"newLinkFormat":"{format}","useMarkdownLinks":{markdown}}}"#));
                for from in &paths {
                    for to in &paths {
                        let text = s.link_to(&v, from, to, None, None, false);
                        let name = if markdown {
                            let dest = text.rsplit_once("](").unwrap().1.trim_end_matches(')');
                            percent_encoding::percent_decode_str(dest).decode_utf8().unwrap().into_owned()
                        } else {
                            text.trim_start_matches("[[").trim_end_matches("]]").to_string()
                        };
                        let syntax = if markdown { LinkSyntax::Markdown } else { LinkSyntax::Wiki };
                        assert_eq!(v.resolve(&name, from, syntax), Some(to.as_str()), "{format} markdown={markdown}: {text} from {from}");
                    }
                }
            }
        }
    }
}
