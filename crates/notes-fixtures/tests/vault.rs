//! Lints for the fixture vault: every note parses, every link resolves or is meant not to, every
//! canvas is valid JSON Canvas, every base is valid Bases syntax, and each construct the parity
//! matrix names is in the file `COVERAGE.md` says it is in.
mod common;

use common::{crate_dir, vault, visible_files};
use percent_encoding::percent_decode_str;
use regex::Regex;
use serde_json::Value;
use serde_yaml_ng::Value as Yaml;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    sync::OnceLock,
};

struct Vault {
    files: Vec<String>,
    /// Notes by path: BOM removed, CRLF turned into LF.
    notes: BTreeMap<String, String>,
}

fn vault_files() -> &'static Vault {
    static V: OnceLock<Vault> = OnceLock::new();
    V.get_or_init(|| {
        let root = vault();
        let files = visible_files(&root);
        let notes = files
            .iter()
            .filter(|f| f.ends_with(".md"))
            .map(|f| {
                let text = fs::read_to_string(root.join(f)).unwrap();
                (
                    f.clone(),
                    text.trim_start_matches('\u{feff}').replace("\r\n", "\n"),
                )
            })
            .collect();
        Vault { files, notes }
    })
}

fn raw(path: &str) -> Vec<u8> {
    fs::read(vault().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn text(path: &str) -> String {
    String::from_utf8(raw(path))
        .unwrap()
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
}

/// The YAML between the opening `---` and the next line that is only `---`, and the rest.
fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// Fenced code blocks, inline code and `%%comments%%` blanked out: what links and tags are read from.
fn strip_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fence: Option<(char, usize)> = None;
    for line in text.lines() {
        let t = line.trim_start();
        let c = t.chars().next().unwrap_or(' ');
        let run = if c == '`' || c == '~' {
            t.chars().take_while(|x| *x == c).count()
        } else {
            0
        };
        match fence {
            Some((fc, n)) if c == fc && run >= n && t[run..].trim().is_empty() => fence = None,
            Some(_) => {}
            None if run >= 3 => fence = Some((c, run)),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    let inline = Regex::new(r"``.+?``|`[^`\n]+`").unwrap();
    let out = inline.replace_all(&out, "");
    Regex::new(r"(?s)%%.*?%%")
        .unwrap()
        .replace_all(&out, "")
        .into_owned()
}

#[derive(Debug, Clone)]
struct Link {
    from: String,
    /// The path part, as written (percent-encoded for Markdown links).
    path: String,
    /// What follows the first `#`, split on `#`, each part decoded for Markdown links.
    sub: Vec<String>,
}

fn links_in(from: &str, source: &str) -> Vec<Link> {
    let body = strip_code(source);
    let mut links = Vec::new();
    let wiki = Regex::new(r"!?\[\[([^\[\]\n]+)\]\]").unwrap();
    for c in wiki.captures_iter(&body) {
        let inner = c[1].replace("\\|", "|");
        let target = inner.split_once('|').map_or(inner.as_str(), |(t, _)| t);
        let (path, sub) = target.split_once('#').map_or((target, ""), |(p, s)| (p, s));
        links.push(Link {
            from: from.into(),
            path: path.trim().into(),
            sub: if sub.is_empty() {
                vec![]
            } else {
                sub.split('#').map(|s| s.trim().to_string()).collect()
            },
        });
    }
    let md = Regex::new(r#"!?\[[^\]\n]*\]\((<[^>\n]+>|[^)\s]+)(?:\s+"[^"]*")?\)"#).unwrap();
    let scheme = Regex::new(r"^[A-Za-z][A-Za-z0-9+.-]*:").unwrap();
    for c in md.captures_iter(&body) {
        let target = c[1].trim_start_matches('<').trim_end_matches('>');
        if scheme.is_match(target) || target.starts_with('#') {
            continue;
        }
        let (path, sub) = target.split_once('#').map_or((target, ""), |(p, s)| (p, s));
        let decode = |s: &str| percent_decode_str(s).decode_utf8_lossy().into_owned();
        links.push(Link {
            from: from.into(),
            path: decode(path),
            sub: if sub.is_empty() {
                vec![]
            } else {
                sub.split('#').map(decode).collect()
            },
        });
    }
    links
}

fn normalise_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// Every file a link could mean: by path from the root, relative to the linking note, by the
/// shortest unique path, or by name alone. File names compare without regard to case.
fn candidates(files: &[String], from: &str, path: &str) -> Vec<String> {
    if path.is_empty() {
        return vec![from.to_string()];
    }
    let lower = |s: &str| s.to_lowercase();
    let matches = |want: &str, f: &String| {
        lower(f) == lower(want) || lower(f) == lower(&format!("{want}.md"))
    };
    let relative = path.starts_with("./") || path.starts_with("../");
    let from_root = if relative {
        normalise_path(&format!("{}/{path}", folder_of(from)))
    } else {
        normalise_path(path)
    };
    let mut found: Vec<String> = files
        .iter()
        .filter(|f| matches(&from_root, f))
        .cloned()
        .collect();
    if found.is_empty() && !relative && !folder_of(from).is_empty() {
        let near = normalise_path(&format!("{}/{path}", folder_of(from)));
        found = files
            .iter()
            .filter(|f| matches(&near, f))
            .cloned()
            .collect();
    }
    if found.is_empty() && !relative {
        let want = lower(path);
        found = files
            .iter()
            .filter(|f| {
                let f = lower(f);
                [format!("/{want}"), format!("/{want}.md")]
                    .iter()
                    .any(|s| f.ends_with(s))
                    || (!want.contains('/') && (f == want || f == format!("{want}.md")))
            })
            .cloned()
            .collect();
    }
    found
}

/// The one a link goes to: the same folder first, then the shortest path.
fn resolve(files: &[String], link: &Link) -> Option<String> {
    let mut found = candidates(files, &link.from, &link.path);
    found.sort_by_key(|f| (folder_of(f) != folder_of(&link.from), f.len(), f.clone()));
    found.into_iter().next()
}

/// A heading as a link spells it: Obsidian drops `# | ^ :`, brackets and `%%` from the heading it
/// links to.
fn heading_key(text: &str) -> String {
    let dropped: String = text.chars().filter(|c| !"#|^:[]".contains(*c)).collect();
    dropped
        .replace("%%", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn headings(text: &str) -> Vec<String> {
    let atx = Regex::new(r"^#{1,6}\s+(.+?)\s*$").unwrap();
    let closing = Regex::new(r"\s+#+$").unwrap();
    strip_code(&text.replace("%%", ""))
        .lines()
        .filter_map(|l| atx.captures(l))
        .map(|c| heading_key(&closing.replace(&c[1], "")))
        .collect()
}

fn has_block(text: &str, id: &str) -> bool {
    Regex::new(&format!(r"(?m)(?:^|\s)\^{}\s*$", regex::escape(id)))
        .unwrap()
        .is_match(text)
}

fn base_view_names(text: &str) -> Vec<String> {
    let doc: Yaml = serde_yaml_ng::from_str(text).unwrap();
    doc.get("views")
        .and_then(Yaml::as_sequence)
        .map(|v| {
            v.iter()
                .filter_map(|v| v.get("name")?.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// What a link's `#...` part asks for, checked against the file it resolved to.
fn sub_ok(v: &Vault, target: &str, sub: &[String]) -> bool {
    if target.ends_with(".base") {
        return sub.len() == 1 && base_view_names(&text(target)).contains(&sub[0]);
    }
    if !target.ends_with(".md") {
        return true; // `#page=2` and `#height=300` belong to the viewer
    }
    let body = &v.notes[target];
    let known = headings(body);
    sub.iter().all(|part| match part.strip_prefix('^') {
        Some(id) => has_block(body, id),
        None => known.contains(&heading_key(part)),
    })
}

fn all_links() -> Vec<Link> {
    let v = vault_files();
    v.notes
        .iter()
        .flat_map(|(path, body)| links_in(path, body))
        .collect()
}

// Links that are meant not to resolve. Each is a case the notes app has to cope with, and each is
// either dangling under Obsidian's rules or a probe whose answer the reference run records.
const UNRESOLVED_FILES: &[&str] = &[
    "Created Deep/Created In Folder", // follow-to-create, with a folder
    "Created On Click",               // follow-to-create
    "Created With Extension.md",      // follow-to-create, must not become `.md.md`
    "Gone.md",                        // Markdown link to nothing
    "Missing",              // links/Unresolved Links.md and review/Missing Note Creation.md
    "Missing Embed",        // embed of nothing
    "Missing Embed Target", // embed of a note that does not exist
    "Missing In Frontmatter", // link in a property
    "Missing In Frontmatter 2", // link in a property
    "Missing Node One",     // an unresolved node in the graph
    "Missing Node Two",
    "Missing With Extension.md",
    "Nothing Here.md", // Markdown link to nothing
    "Second Alias",    // an alias is not a file name
    "Start",           // an alias of Home
    "missing-image.png",
    "nowhere/Missing Deep",
    "Ünïcode Note", // an alias of Unicode é 日本
];

// Links to a note that exists with a heading or block that does not, under the simple rules above.
const UNRESOLVED_SUBPATHS: &[&str] = &[
    "Blocks Target#^no-such-block",
    "Headings Target#No such heading",
    // Probe: `C# basics` split at its `#` is the nested path C then basics, which no heading has.
    // What Obsidian does with it is in the reference run.
    "Heading With Hash#C#basics",
];

#[test]
fn links_resolve_except_the_ones_meant_not_to() {
    let v = vault_files();
    let (mut files, mut subs) = (BTreeSet::new(), BTreeSet::new());
    for link in all_links() {
        match resolve(&v.files, &link) {
            None => {
                files.insert(link.path.clone());
            }
            Some(target) if !link.sub.is_empty() && !sub_ok(v, &target, &link.sub) => {
                let name = target
                    .rsplit_once('/')
                    .map_or(target.as_str(), |(_, n)| n)
                    .trim_end_matches(".md");
                subs.insert(format!("{name}#{}", link.sub.join("#")));
            }
            Some(_) => {}
        }
    }
    assert_eq!(
        files,
        UNRESOLVED_FILES
            .iter()
            .map(|s| s.to_string())
            .collect::<BTreeSet<_>>(),
        "unresolved targets"
    );
    assert_eq!(
        subs,
        UNRESOLVED_SUBPATHS
            .iter()
            .map(|s| s.to_string())
            .collect::<BTreeSet<_>>(),
        "unresolved headings and blocks"
    );
}

#[test]
fn same_name_notes_stay_apart() {
    let v = vault_files();
    assert!(v.notes.contains_key("a/Note.md") && v.notes.contains_key("b/Note.md"));
    let from = |path: &str, target: &str| Link {
        from: path.into(),
        path: target.into(),
        sub: vec![],
    };
    assert_eq!(
        resolve(&v.files, &from("a/Links From A.md", "Note")).as_deref(),
        Some("a/Note.md"),
        "same folder first"
    );
    assert_eq!(
        resolve(&v.files, &from("b/Links From B.md", "Note")).as_deref(),
        Some("b/Note.md")
    );
    assert_eq!(
        resolve(&v.files, &from("a/Links From A.md", "../b/Note")).as_deref(),
        Some("b/Note.md")
    );
    assert_eq!(
        resolve(&v.files, &from("Home.md", "a/Note")).as_deref(),
        Some("a/Note.md")
    );
    let twins: Vec<_> = candidates(&v.files, "Home.md", "Note");
    assert_eq!(twins.len(), 2, "{twins:?}");
    for note in [
        "Home.md",
        "links/Link Forms.md",
        "review/Missing Note Creation.md",
        "embeds/Embed Notes.md",
    ] {
        let t = &v.notes[note];
        assert!(
            t.contains("[[a/Note") && t.contains("b/Note"),
            "{note} names both twins by folder"
        );
    }
}

/// The notes nothing links to, by the rules above, with canvases counted as linkers (1.12).
fn notes_without_incoming() -> BTreeSet<String> {
    let v = vault_files();
    let mut linked = BTreeSet::new();
    for link in all_links() {
        if let Some(target) = resolve(&v.files, &link)
            && target != link.from
        {
            linked.insert(target);
        }
    }
    for f in v.files.iter().filter(|f| f.ends_with(".canvas")) {
        let canvas: Value = serde_json::from_str(&text(f)).unwrap();
        for n in canvas["nodes"].as_array().unwrap() {
            if let Some(file) = n["file"].as_str() {
                linked.insert(file.to_string());
            }
        }
    }
    v.notes
        .keys()
        .filter(|n| !linked.contains(*n))
        .cloned()
        .collect()
}

#[test]
fn only_the_named_notes_have_no_incoming_link() {
    let orphans = notes_without_incoming();
    let coverage = fs::read_to_string(crate_dir().join("COVERAGE.md")).unwrap();
    let section = coverage
        .split("## Notes without an incoming link")
        .nth(1)
        .expect("COVERAGE.md names them")
        .split("\n## ")
        .next()
        .unwrap();
    let named: BTreeSet<String> = Regex::new(r"`([^`]+\.md)`")
        .unwrap()
        .captures_iter(section)
        .map(|c| c[1].to_string())
        .collect();
    assert_eq!(
        orphans, named,
        "COVERAGE.md and the vault disagree about which notes nothing links to"
    );
    assert!(orphans.contains("Orphan.md"));
}

#[test]
fn orphan_and_dead_end_are_what_they_say() {
    let v = vault_files();
    let out = |p: &str| {
        links_in(p, &v.notes[p])
            .into_iter()
            .filter(|l| resolve(&v.files, l).is_some_and(|t| t != p))
            .count()
    };
    assert_eq!(
        out("Orphan.md"),
        0,
        "an orphan with no outgoing links either"
    );
    assert_eq!(out("Dead End.md"), 0, "a dead end");
    assert!(
        !notes_without_incoming().contains("Dead End.md"),
        "a dead end that is not an orphan"
    );
    assert!(out("Home.md") > 50, "Home is the hub");
    let unresolved_only = &v.notes["links/Unresolved Links.md"];
    assert!(links_in("links/Unresolved Links.md", unresolved_only).len() >= 10);
}

#[test]
fn frontmatter_parses_except_the_note_that_is_broken_on_purpose() {
    let v = vault_files();
    let mut broken = Vec::new();
    for (path, body) in &v.notes {
        if let Some((yaml, _)) = split_frontmatter(body)
            && serde_yaml_ng::from_str::<Yaml>(yaml).is_err()
        {
            broken.push(path.as_str());
        }
    }
    assert_eq!(broken, ["properties/Broken Frontmatter.md"]);
    let templates = ["templates/Daily.md", "templates/Meeting.md"];
    assert!(templates.iter().all(|t| v.notes.contains_key(*t)));
}

#[test]
fn every_property_type_is_present_and_typed() {
    let body = text("properties/All Property Types.md");
    let (yaml, _) = split_frontmatter(&body).unwrap();
    let doc: Yaml = serde_yaml_ng::from_str(yaml).unwrap();
    let get = |k: &str| doc.get(k).unwrap_or_else(|| panic!("no property {k}"));
    assert!(get("text").is_string());
    assert!(
        get("list").is_sequence()
            && get("tags").is_sequence()
            && get("aliases").is_sequence()
            && get("cssclasses").is_sequence()
    );
    assert!(get("number").is_i64() && get("decimal").is_f64());
    assert!(get("checkbox").as_bool() == Some(true) && get("unchecked").as_bool() == Some(false));
    assert!(get("date").is_string() && get("datetime").as_str().unwrap().contains('T'));
    assert!(get("empty").is_null() && get("null_value").is_null());
    assert!(get("nested").is_mapping() && get("multiline").as_str().unwrap().contains('\n'));
    assert!(get("link").as_str().unwrap().starts_with("[["));
    assert!(doc.get("key with spaces").is_some() && doc.get("unicode-key-ñ").is_some());

    let types: Value = serde_json::from_str(&text(".obsidian/types.json")).unwrap();
    let used: BTreeSet<&str> = types["types"]
        .as_object()
        .unwrap()
        .values()
        .map(|t| t.as_str().unwrap())
        .collect();
    for t in [
        "text",
        "multitext",
        "number",
        "checkbox",
        "date",
        "datetime",
        "tags",
        "aliases",
    ] {
        assert!(used.contains(t), "no property is typed {t}");
    }
    let all: String = v_all_frontmatter();
    for name in types["types"].as_object().unwrap().keys() {
        assert!(
            Regex::new(&format!(r"(?m)^{}:", regex::escape(name)))
                .unwrap()
                .is_match(&all),
            "types.json types {name}, which no note has"
        );
    }
}

fn v_all_frontmatter() -> String {
    vault_files()
        .notes
        .values()
        .filter_map(|b| split_frontmatter(b).map(|(y, _)| y))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn type_mismatch_disagrees_with_every_type_it_names() {
    let body = text("properties/Type Mismatch.md");
    let (yaml, _) = split_frontmatter(&body).unwrap();
    let doc: Yaml = serde_yaml_ng::from_str(yaml).unwrap();
    let types: Value = serde_json::from_str(&text(".obsidian/types.json")).unwrap();
    let date = Regex::new(r"^\d{4}-\d\d-\d\d").unwrap();
    for (name, value) in doc.as_mapping().unwrap() {
        let name = name.as_str().unwrap();
        if name == "tags" {
            continue;
        }
        let ty = types["types"][name]
            .as_str()
            .unwrap_or_else(|| panic!("{name} has no type"));
        let agrees = match ty {
            "number" => value.is_number(),
            "checkbox" => value.is_bool(),
            "multitext" => value.is_sequence(),
            "date" | "datetime" => value.as_str().is_some_and(|s| date.is_match(s)),
            t => panic!("unexpected {t}"),
        };
        assert!(!agrees, "{name} agrees with its type {ty}");
    }
}

const NOTES_WITH_NO_TASKS: &[&str] = &["Orphan.md", "Dead End.md"];

#[test]
fn tasks_use_many_status_characters() {
    let body = text("tasks/Task Statuses.md");
    let re = Regex::new(r"(?m)^\s*(?:[-*+]|\d+[.)])\s\[(.)\]").unwrap();
    let chars: BTreeSet<char> = re
        .captures_iter(&body)
        .map(|c| c[1].chars().next().unwrap())
        .collect();
    assert!(chars.len() >= 20, "{chars:?}");
    for c in [
        ' ', 'x', 'X', '/', '-', '>', '<', '?', '!', '*', '"', 'l', 'b', 'i', 'S', 'I', 'p', 'c',
        'f', 'k', 'w', 'u', 'd', '😀',
    ] {
        assert!(chars.contains(&c), "{c:?}");
    }
    for (path, t) in &vault_files().notes {
        if NOTES_WITH_NO_TASKS.contains(&path.as_str()) {
            assert!(!re.is_match(t), "{path}");
        }
    }
}

#[test]
fn the_thirteen_callouts_their_aliases_and_the_odd_ones_are_all_there() {
    let body = text("callouts/All Callouts.md");
    let re = Regex::new(r"(?m)^> \[!([^\]]*)\]").unwrap();
    let types: Vec<String> = re.captures_iter(&body).map(|c| c[1].to_string()).collect();
    for t in [
        "note", "abstract", "info", "todo", "tip", "success", "question", "warning", "failure",
        "danger", "bug", "example", "quote",
    ] {
        assert!(types.iter().any(|x| x == t), "callout {t}");
    }
    for alias in [
        "summary",
        "tldr",
        "hint",
        "important",
        "check",
        "done",
        "help",
        "faq",
        "caution",
        "attention",
        "fail",
        "missing",
        "error",
        "cite",
    ] {
        assert!(types.iter().any(|x| x == alias), "callout alias {alias}");
    }
    for odd in ["NOTE", "Warning", "not-a-type", "", " note"] {
        assert!(types.iter().any(|x| x == odd), "odd callout {odd:?}");
    }
    let folding = text("callouts/Callout Titles and Folding.md");
    assert!(
        folding.contains("[!faq]-")
            && folding.contains("[!faq]+")
            && folding.contains("[!note]-\n")
    );
    assert!(folding.contains("[!tip] A custom title") && folding.contains("[!note] Title only"));
    let nested = text("callouts/Nested Callouts.md");
    assert!(nested.contains("> > > [!example]"), "three deep");
    let css = text(".obsidian/snippets/callouts.css");
    assert!(css.contains("custom-purple") && css.contains("--callout-color"));
    assert!(text("callouts/Custom Callout.md").contains("[!custom-purple]"));
    let appearance: Value = serde_json::from_str(&text(".obsidian/appearance.json")).unwrap();
    assert_eq!(appearance["enabledCssSnippets"][0], "callouts");
}

#[test]
fn line_endings_and_byte_order_marks_are_really_there() {
    let crlf = raw("syntax/Line Endings.md");
    assert!(crlf.windows(2).filter(|w| w == b"\r\n").count() >= 8);
    assert!(
        !crlf
            .iter()
            .enumerate()
            .any(|(i, b)| *b == b'\n' && (i == 0 || crlf[i - 1] != b'\r')),
        "every line ends in CRLF"
    );
    let bom = raw("syntax/Byte Order Mark.md");
    assert_eq!(&bom[..3], [0xEF, 0xBB, 0xBF]);
    assert!(!vault_files().notes["syntax/Byte Order Mark.md"].starts_with('\u{feff}'));
    for path in vault_files().notes.keys() {
        if path != "syntax/Line Endings.md" {
            assert!(!raw(path).contains(&b'\r'), "{path} has a CR");
        }
        if path != "syntax/Byte Order Mark.md" {
            assert!(
                !raw(path).starts_with(&[0xEF, 0xBB, 0xBF]),
                "{path} has a BOM"
            );
        }
    }
}

/// The calendar draws a dot per 250 words and a hollow one for open tasks (reference screenshot).
#[test]
fn daily_notes_land_on_chosen_sides_of_the_word_boundaries() {
    let v = vault_files();
    let expect = [
        ("daily/2026-10-23.md", 0, false),
        ("daily/2026-10-24.md", 1, false),
        ("daily/2026-10-25.md", 2, true),
    ];
    for (path, dots, open) in expect {
        let body = split_frontmatter(&v.notes[path]).unwrap().1;
        let words = body.split_whitespace().count();
        assert_eq!(words / 250, dots, "{path}: {words} words");
        assert!(
            words % 250 >= 40 && words % 250 <= 210 || words < 210,
            "{path}: {words} words is too close to a boundary for another counter to disagree"
        );
        assert_eq!(
            body.contains("- [ ]") || body.contains("- [/]"),
            open,
            "{path}"
        );
    }
    let daily: Value = serde_json::from_str(&text(".obsidian/daily-notes.json")).unwrap();
    assert_eq!(daily["format"], "YYYY-MM-DD");
    assert!(v.notes.contains_key("weekly/2026-W43.md"));
}

#[test]
fn the_obsidian_folder_agrees_with_the_vault() {
    let v = vault_files();
    let json = |f: &str| -> Value {
        serde_json::from_str(&text(&format!(".obsidian/{f}")))
            .unwrap_or_else(|e| panic!("{f}: {e}"))
    };
    let dir_exists = |d: &str| {
        v.files
            .iter()
            .any(|f| f.starts_with(&format!("{}/", d.trim_end_matches('/'))))
    };
    let app = json("app.json");
    for ignored in app["userIgnoreFilters"].as_array().unwrap() {
        assert!(
            dir_exists(ignored.as_str().unwrap()),
            "excluded folder {ignored}"
        );
    }
    assert!(dir_exists(app["newFileFolderPath"].as_str().unwrap()));
    assert!(dir_exists(app["attachmentFolderPath"].as_str().unwrap()));
    assert_eq!(
        app["useMarkdownLinks"], false,
        "wikilinks are the default link format"
    );
    assert_eq!(app["defaultViewMode"], "preview");

    let daily = json("daily-notes.json");
    assert!(dir_exists(daily["folder"].as_str().unwrap()));
    assert!(
        v.files
            .contains(&format!("{}.md", daily["template"].as_str().unwrap()))
    );
    let templates = json("templates.json");
    assert!(dir_exists(templates["folder"].as_str().unwrap()));

    let plugins = json("core-plugins.json");
    for p in [
        "file-explorer",
        "global-search",
        "switcher",
        "graph",
        "backlink",
        "canvas",
        "outgoing-link",
        "tag-pane",
        "properties",
        "daily-notes",
        "templates",
        "bookmarks",
        "outline",
        "word-count",
        "slides",
        "workspaces",
        "bases",
        "footnotes",
        "page-preview",
    ] {
        assert_eq!(plugins[p], true, "core plugin {p}");
    }
    for p in plugins.as_object().unwrap().values() {
        assert!(p.is_boolean());
    }

    for item in bookmark_items(&json("bookmarks.json")["items"]) {
        match item["type"].as_str().unwrap() {
            "file" => {
                let path = item["path"].as_str().unwrap();
                assert!(v.files.iter().any(|f| f == path), "bookmark {path}");
                if let Some(sub) = item["subpath"].as_str() {
                    assert!(
                        sub_ok(
                            v,
                            path,
                            &sub.split('#').skip(1).map(String::from).collect::<Vec<_>>()
                        ),
                        "bookmark {path}{sub}"
                    );
                }
            }
            "folder" => assert!(dir_exists(item["path"].as_str().unwrap())),
            "search" => assert!(item["query"].is_string()),
            "group" => {}
            t => panic!("bookmark type {t}"),
        }
    }
    let types: BTreeSet<_> = bookmark_items(&json("bookmarks.json")["items"])
        .iter()
        .map(|i| i["type"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        types,
        ["file", "folder", "group", "search"]
            .map(String::from)
            .into()
    );

    let graph = json("graph.json");
    assert!(graph["colorGroups"].as_array().unwrap().len() >= 3);
    for key in [
        "centerStrength",
        "repelStrength",
        "linkStrength",
        "linkDistance",
        "nodeSizeMultiplier",
        "lineSizeMultiplier",
        "textFadeMultiplier",
    ] {
        assert!(graph[key].is_number(), "graph.{key}");
    }
    let appearance = json("appearance.json");
    for snippet in appearance["enabledCssSnippets"].as_array().unwrap() {
        let css = text(&format!(
            ".obsidian/snippets/{}.css",
            snippet.as_str().unwrap()
        ));
        assert!(css.contains('{'), "snippet {snippet}");
    }
    assert!(
        text(".obsidian/snippets/folder-colours.css")
            .contains(r#".nav-folder-title[data-path="projects"]"#)
    );

    let hotkeys = json("hotkeys.json");
    for (command, keys) in hotkeys.as_object().unwrap() {
        assert!(command.contains(':'), "{command}");
        for key in keys.as_array().unwrap() {
            assert!(
                key["modifiers"].is_array() && key["key"].is_string(),
                "{command}"
            );
        }
    }
    assert!(
        hotkeys
            .as_object()
            .unwrap()
            .values()
            .any(|k| k.as_array().unwrap().is_empty()),
        "a command whose default hotkey was removed"
    );
}

/// Every id in a saved layout, depth first.
fn layout_nodes<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
    if node.get("type").is_some() && node.get("id").is_some() {
        out.push(node);
    }
    for child in node["children"].as_array().into_iter().flatten() {
        layout_nodes(child, out);
    }
}

#[test]
fn the_saved_layout_opens_real_notes_in_every_arrangement() {
    let v = vault_files();
    let doc: Value = serde_json::from_str(&text(".obsidian/workspaces.json")).unwrap();
    assert_eq!(doc["active"], "Reading layout");
    let layout = &doc["workspaces"]["Reading layout"];
    let mut nodes = Vec::new();
    for part in ["main", "left", "right"] {
        layout_nodes(&layout[part], &mut nodes);
    }
    let ids: BTreeSet<&str> = nodes.iter().map(|n| n["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), nodes.len(), "ids are unique");
    assert!(ids.contains(layout["active"].as_str().unwrap()));

    let leaves: Vec<&&Value> = nodes.iter().filter(|n| n["type"] == "leaf").collect();
    for leaf in &leaves {
        if let Some(file) = leaf["state"]["state"]["file"].as_str() {
            assert!(v.files.iter().any(|f| f == file), "the layout opens {file}");
        }
    }
    for file in layout["lastOpenFiles"].as_array().unwrap() {
        assert!(
            v.files.iter().any(|f| f == file.as_str().unwrap()),
            "{file}"
        );
    }
    let main_groups = layout["main"]["children"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["type"] == "tabs")
        .count();
    assert!(main_groups >= 2, "a split between tab groups");
    assert!(
        nodes
            .iter()
            .any(|n| n["type"] == "tabs" && n["stacked"] == true),
        "stacked tabs"
    );
    assert!(
        leaves.iter().any(|l| l["state"]["pinned"] == true),
        "a pinned tab"
    );
    let mut linked: BTreeMap<&str, usize> = BTreeMap::new();
    for l in &leaves {
        if let Some(g) = l["group"].as_str() {
            *linked.entry(g).or_default() += 1;
        }
    }
    assert!(
        linked.values().any(|n| *n >= 2),
        "a note tab and a backlinks pane linked together: {linked:?}"
    );
    let kinds: BTreeSet<&str> = leaves
        .iter()
        .map(|l| l["state"]["type"].as_str().unwrap())
        .collect();
    for k in [
        "markdown",
        "canvas",
        "bases",
        "file-explorer",
        "search",
        "bookmarks",
        "backlink",
        "outgoing-link",
        "outline",
        "localgraph",
    ] {
        assert!(kinds.contains(k), "no {k} pane in the layout");
    }
    let modes: BTreeSet<&str> = leaves
        .iter()
        .filter_map(|l| l["state"]["state"]["mode"].as_str())
        .collect();
    assert_eq!(
        modes,
        ["preview", "source"].into(),
        "reading view and source mode"
    );
}

fn bookmark_items(items: &Value) -> Vec<Value> {
    let mut all = Vec::new();
    for item in items.as_array().into_iter().flatten() {
        all.push(item.clone());
        all.extend(bookmark_items(&item["items"]));
    }
    all
}

#[test]
fn single_search_markers_are_unique() {
    let v = vault_files();
    let count = |needle: &str| {
        v.notes
            .values()
            .map(|t| t.matches(needle).count())
            .sum::<usize>()
    };
    assert_eq!(count("xyzzy-unique-token"), 1, "one place");
    let marker: Vec<_> = v
        .notes
        .iter()
        .filter(|(_, t)| t.contains("excluded-marker-xyzzy"))
        .map(|(p, _)| p.as_str())
        .collect();
    assert_eq!(
        marker,
        ["excluded/Hidden Note.md"],
        "found only in the excluded folder"
    );
    assert!(v.notes["search/Case Sensitivity.md"].contains("Apple apple APPLE aPpLe"));
}

// ---- attachments ----

const MAGIC: &[(&str, &[u8], usize)] = &[
    (".png", b"\x89PNG\r\n\x1a\n", 0),
    (".jpg", b"\xff\xd8\xff", 0),
    (".gif", b"GIF8", 0),
    (".bmp", b"BM", 0),
    (".webp", b"WEBP", 8),
    (".avif", b"ftypavif", 4),
    (".svg", b"<svg", 0),
    (".wav", b"WAVE", 8),
    (".ogg", b"OggS", 0),
    (".m4a", b"ftyp", 4),
    (".mp4", b"ftyp", 4),
    (".webm", b"\x1a\x45\xdf\xa3", 0),
    (".pdf", b"%PDF-", 0),
];

#[test]
fn attachments_are_small_and_what_their_names_say() {
    let v = vault_files();
    let mut total = 0;
    let mut kinds = BTreeSet::new();
    for f in v.files.iter().filter(|f| f.starts_with("attachments/")) {
        let data = raw(f);
        total += data.len();
        assert!(data.len() < 20_000, "{f} is {} bytes", data.len());
        let ext = format!(".{}", f.rsplit('.').next().unwrap());
        kinds.insert(ext.clone());
        let ok = if ext == ".mp3" {
            data.starts_with(b"ID3")
                || data.starts_with(&[0xFF, 0xFB])
                || data.starts_with(&[0xFF, 0xF3])
        } else {
            let (_, magic, at) = MAGIC
                .iter()
                .find(|m| m.0 == ext)
                .unwrap_or_else(|| panic!("no magic for {ext}"));
            data.len() >= at + magic.len() && &data[*at..at + magic.len()] == *magic
        };
        assert!(ok, "{f} does not look like {ext}");
    }
    assert!(total < 150_000, "attachments weigh {total} bytes");
    for k in [
        ".png", ".jpg", ".gif", ".bmp", ".webp", ".avif", ".svg", ".wav", ".mp3", ".m4a", ".ogg",
        ".mp4", ".webm", ".pdf",
    ] {
        assert!(kinds.contains(k), "no {k} attachment");
    }
    let pdf = raw("attachments/sample.pdf");
    let pages = regex::bytes::Regex::new(r"(?-u)/Type\s*/Page[^s]")
        .unwrap()
        .find_iter(&pdf)
        .count();
    assert!(
        pages >= 3,
        "the page= embeds need 3 pages, the PDF has {pages}"
    );
}

#[test]
fn every_attachment_is_used_except_the_one_left_behind() {
    let v = vault_files();
    let mut corpus = String::new();
    for f in &v.files {
        if f.starts_with("attachments/") && !f.ends_with(".svg") {
            continue;
        }
        if let Ok(t) = fs::read_to_string(vault().join(f)) {
            corpus.push_str(&t);
        }
    }
    for dotfile in ["bookmarks.json", "app.json", "appearance.json"] {
        corpus.push_str(&text(&format!(".obsidian/{dotfile}")));
    }
    for f in v.files.iter().filter(|f| f.starts_with("attachments/")) {
        let name = f.trim_start_matches("attachments/");
        let encoded = name.replace(' ', "%20");
        let used = corpus.contains(name) || corpus.contains(&encoded);
        assert_eq!(used, name != "unused.png", "{name}");
    }
    assert!(
        !corpus.contains("unused.png"),
        "no note may mention the unused file by name"
    );
}

// ---- canvases ----

const SIDES: [&str; 4] = ["top", "right", "bottom", "left"];

/// JSON Canvas 1.0: https://jsoncanvas.org/spec/1.0/
fn check_canvas(path: &str, canvas: &Value, ids_look_like_obsidian: bool, missing_ok: &[&str]) {
    let v = vault_files();
    let colour = Regex::new(r"^(?:[1-6]|#[0-9a-fA-F]{6})$").unwrap();
    let id_shape = Regex::new(r"^[0-9a-f]{16}$").unwrap();
    for k in canvas.as_object().unwrap().keys() {
        assert!(k == "nodes" || k == "edges", "{path}: key {k}");
    }
    let mut ids = BTreeSet::new();
    for n in canvas["nodes"].as_array().unwrap_or(&vec![]) {
        let id = n["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{path}: node without id"));
        assert!(ids.insert(id.to_string()), "{path}: duplicate id {id}");
        assert!(
            !ids_look_like_obsidian || id_shape.is_match(id),
            "{path}: id {id}"
        );
        for k in ["x", "y", "width", "height"] {
            assert!(n[k].is_i64(), "{path}: {id}.{k}");
        }
        if let Some(c) = n.get("color") {
            assert!(colour.is_match(c.as_str().unwrap()), "{path}: colour {c}");
        }
        match n["type"].as_str().unwrap() {
            "text" => assert!(n["text"].is_string()),
            "link" => assert!(n["url"].as_str().unwrap().starts_with("http")),
            "file" => {
                let file = n["file"].as_str().unwrap();
                assert!(
                    missing_ok.contains(&file) || v.files.iter().any(|f| f == file),
                    "{path}: file card {file}"
                );
                if let Some(sub) = n.get("subpath") {
                    let sub = sub.as_str().unwrap();
                    assert!(sub.starts_with('#'), "{path}: subpath {sub}");
                    if file.ends_with(".md") && v.notes.contains_key(file) {
                        let parts: Vec<String> = sub.split('#').skip(1).map(String::from).collect();
                        assert!(sub_ok(v, file, &parts), "{path}: {file}{sub}");
                    }
                }
            }
            "group" => {
                if let Some(bg) = n.get("background") {
                    assert!(
                        v.files.iter().any(|f| f == bg.as_str().unwrap()),
                        "{path}: background {bg}"
                    );
                }
                if let Some(style) = n.get("backgroundStyle") {
                    assert!(["cover", "ratio", "repeat"].contains(&style.as_str().unwrap()));
                }
            }
            t => panic!("{path}: node type {t}"),
        }
    }
    let mut edge_ids = BTreeSet::new();
    for e in canvas["edges"].as_array().unwrap_or(&vec![]) {
        let id = e["id"].as_str().unwrap();
        assert!(
            edge_ids.insert(id.to_string()) && !ids.contains(id),
            "{path}: edge id {id}"
        );
        for k in ["fromNode", "toNode"] {
            assert!(
                ids.contains(e[k].as_str().unwrap()),
                "{path}: edge {id} {k}"
            );
        }
        for k in ["fromSide", "toSide"] {
            assert!(
                e.get(k)
                    .is_none_or(|s| SIDES.contains(&s.as_str().unwrap())),
                "{path}: {id}.{k}"
            );
        }
        for k in ["fromEnd", "toEnd"] {
            assert!(
                e.get(k)
                    .is_none_or(|s| ["none", "arrow"].contains(&s.as_str().unwrap())),
                "{path}: {id}.{k}"
            );
        }
        if let Some(c) = e.get("color") {
            assert!(
                colour.is_match(c.as_str().unwrap()),
                "{path}: edge colour {c}"
            );
        }
        assert!(e.get("label").is_none_or(Value::is_string));
    }
}

fn canvases() -> Vec<(String, Value)> {
    vault_files()
        .files
        .iter()
        .filter(|f| f.ends_with(".canvas"))
        .map(|f| (f.clone(), serde_json::from_str(&text(f)).unwrap()))
        .collect()
}

#[test]
fn canvases_are_valid_json_canvas() {
    let list = canvases();
    assert_eq!(list.len(), 4);
    for (path, canvas) in &list {
        check_canvas(path, canvas, true, &["Missing Canvas Target.md"]);
    }
    let sample: Value = serde_json::from_str(
        &fs::read_to_string(crate_dir().join("jsoncanvas/sample.canvas")).unwrap(),
    )
    .unwrap();
    let upstream_files = ["readme.md", "_site/logo.svg", "spec/1.0.md"];
    check_canvas("jsoncanvas/sample.canvas", &sample, false, &upstream_files);
}

#[test]
fn canvases_cover_the_formats_features() {
    let list = canvases();
    let nodes: Vec<&Value> = list
        .iter()
        .flat_map(|(_, c)| c["nodes"].as_array().unwrap())
        .collect();
    let edges: Vec<&Value> = list
        .iter()
        .flat_map(|(_, c)| c["edges"].as_array().unwrap())
        .collect();
    let of = |t: &str| nodes.iter().filter(|n| n["type"] == t).count();
    assert!(
        of("text") >= 8 && of("file") >= 20 && of("link") >= 2 && of("group") >= 8,
        "node types"
    );

    let colours: BTreeSet<&str> = nodes
        .iter()
        .chain(&edges)
        .filter_map(|x| x["color"].as_str())
        .collect();
    for c in ["1", "2", "3", "4", "5", "6"] {
        assert!(colours.contains(c), "preset colour {c}");
    }
    assert!(colours.iter().any(|c| c.starts_with('#')), "a hex colour");
    assert!(
        edges.iter().any(|e| e["color"].is_string())
            && edges.iter().any(|e| e["label"].is_string()),
        "coloured and labelled edges"
    );
    assert!(
        edges
            .iter()
            .any(|e| e["color"].is_string() && e["label"].is_string())
    );
    let sides: BTreeSet<&str> = edges
        .iter()
        .flat_map(|e| [e["fromSide"].as_str(), e["toSide"].as_str()])
        .flatten()
        .collect();
    assert_eq!(sides.len(), 4, "every side");
    assert!(
        edges.iter().any(|e| e["fromSide"].is_null()),
        "an edge without sides"
    );
    assert!(
        edges.iter().any(|e| e["fromEnd"] == "arrow")
            && edges.iter().any(|e| e["toEnd"] == "none")
            && edges.iter().any(|e| e["toEnd"] == "arrow")
    );

    let groups: Vec<&&Value> = nodes.iter().filter(|n| n["type"] == "group").collect();
    assert!(
        groups.iter().any(|g| g["label"].is_string())
            && groups.iter().any(|g| g["label"].is_null()),
        "labelled and unlabelled groups"
    );
    let styles: BTreeSet<&str> = groups
        .iter()
        .filter_map(|g| g["backgroundStyle"].as_str())
        .collect();
    assert_eq!(
        styles,
        ["cover", "ratio", "repeat"].into(),
        "every background style"
    );
    assert!(
        groups
            .iter()
            .all(|g| g["background"].is_null() == g["backgroundStyle"].is_null())
    );
    let contains = |a: &Value, b: &Value| {
        let n = |v: &Value, k: &str| v[k].as_i64().unwrap();
        n(a, "x") <= n(b, "x")
            && n(a, "y") <= n(b, "y")
            && n(a, "x") + n(a, "width") >= n(b, "x") + n(b, "width")
            && n(a, "y") + n(a, "height") >= n(b, "y") + n(b, "height")
    };
    assert!(
        groups
            .iter()
            .any(|a| groups.iter().any(|b| a["id"] != b["id"] && contains(a, b))),
        "nested groups"
    );
    let group_ids: BTreeSet<&str> = groups.iter().map(|g| g["id"].as_str().unwrap()).collect();
    assert!(
        edges
            .iter()
            .any(|e| group_ids.contains(e["toNode"].as_str().unwrap())),
        "an edge into a group"
    );
    assert!(
        edges
            .iter()
            .any(|e| group_ids.contains(e["fromNode"].as_str().unwrap())),
        "an edge out of a group"
    );

    let files: Vec<&&Value> = nodes.iter().filter(|n| n["type"] == "file").collect();
    let subpaths: Vec<&str> = files.iter().filter_map(|n| n["subpath"].as_str()).collect();
    assert!(
        subpaths.iter().any(|s| s.starts_with("#^")),
        "a card narrowed to a block"
    );
    assert!(
        subpaths
            .iter()
            .any(|s| s.starts_with('#') && !s.starts_with("#^")),
        "a card narrowed to a heading"
    );
    assert!(
        subpaths.iter().any(|s| s.matches('#').count() == 2),
        "a nested heading"
    );
    let ext = |e: &str| {
        files
            .iter()
            .any(|n| n["file"].as_str().unwrap().ends_with(e))
    };
    for e in [
        ".md", ".png", ".jpg", ".gif", ".webp", ".svg", ".mp3", ".mp4", ".pdf", ".base", ".canvas",
    ] {
        assert!(ext(e), "a file card for {e}");
    }
    let urls: Vec<&str> = nodes.iter().filter_map(|n| n["url"].as_str()).collect();
    assert!(
        urls.iter().any(|u| u.contains("youtube.com"))
            && urls.iter().any(|u| !u.contains("youtube.com")),
        "a web page and a video"
    );
    assert!(
        nodes
            .iter()
            .any(|n| n["type"] == "file" && n["file"] == "Missing Canvas Target.md"),
        "a card whose file is gone"
    );
    assert!(
        files
            .iter()
            .any(|n| n["file"].as_str().unwrap().starts_with("projects/")),
        "a folder's notes as cards"
    );
    assert!(
        nodes
            .iter()
            .any(|n| n["type"] == "text" && n["text"].as_str().unwrap().contains("[[")),
        "a text card with a link"
    );
}

// ---- bases ----

const DEFAULT_SUMMARIES: &[&str] = &[
    "Average",
    "Min",
    "Max",
    "Sum",
    "Range",
    "Median",
    "Stddev",
    "Earliest",
    "Latest",
    "Checked",
    "Unchecked",
    "Empty",
    "Filled",
    "Unique",
];
const VIEW_TYPES: &[&str] = &["table", "cards", "list", "kanban"];
// https://obsidian.md/help/bases/functions as of 2026-10-26.
const GLOBAL_FUNCTIONS: &[&str] = &[
    "escapeHTML",
    "date",
    "duration",
    "file",
    "html",
    "if",
    "image",
    "icon",
    "link",
    "list",
    "max",
    "min",
    "now",
    "number",
    "today",
    "random",
];
const METHODS: &[&str] = &[
    "isTruthy",
    "isType",
    "toString", // any
    "date",
    "format",
    "time",
    "relative",
    "isEmpty", // date
    "contains",
    "containsAll",
    "containsAny",
    "endsWith",
    "lower",
    "replace",
    "repeat",
    "reverse",
    "slice",
    "split",
    "startsWith",
    "title",
    "trim", // string
    "abs",
    "ceil",
    "floor",
    "round",
    "toFixed", // number
    "filter",
    "flat",
    "join",
    "map",
    "reduce",
    "sort",
    "unique", // list
    "asFile",
    "linksTo", // link
    "asLink",
    "hasLink",
    "hasProperty",
    "hasTag",
    "inFolder", // file
    "keys",
    "values",  // object
    "matches", // regexp
];
// The summaries section of the syntax page calls `values.mean()`, which the functions page lacks.
const SUMMARY_METHODS: &[&str] = &["mean", "median", "sum"];

fn called_functions(expr: &str) -> Vec<(bool, String)> {
    let literals = Regex::new(r#""[^"]*"|'[^']*'"#).unwrap();
    let bare = literals.replace_all(expr, "\"\"");
    Regex::new(r"(\.?)\s*\b([A-Za-z_]\w*)\s*\(")
        .unwrap()
        .captures_iter(&bare)
        .map(|c| (!c[1].is_empty(), c[2].to_string()))
        .collect()
}

fn check_expression(what: &str, expr: &str, extra_methods: &[&str]) {
    for (method, name) in called_functions(expr) {
        let known = if method {
            METHODS.contains(&name.as_str()) || extra_methods.contains(&name.as_str())
        } else {
            GLOBAL_FUNCTIONS.contains(&name.as_str())
        };
        assert!(
            known,
            "{what}: `{expr}` calls {name}, which Bases does not have"
        );
    }
}

fn check_filter(what: &str, filter: &Yaml) {
    match filter {
        Yaml::String(s) => check_expression(what, s, &[]),
        Yaml::Mapping(m) => {
            assert_eq!(
                m.len(),
                1,
                "{what}: a filter object has one of and, or, not"
            );
            let (k, v) = m.iter().next().unwrap();
            assert!(
                ["and", "or", "not"].contains(&k.as_str().unwrap()),
                "{what}: {k:?}"
            );
            v.as_sequence()
                .unwrap_or_else(|| panic!("{what}: {k:?} takes a list"))
                .iter()
                .for_each(|f| check_filter(what, f));
        }
        other => panic!("{what}: filter {other:?}"),
    }
}

fn base_documents() -> Vec<(String, Yaml)> {
    let mut docs: Vec<(String, Yaml)> = vault_files()
        .files
        .iter()
        .filter(|f| f.ends_with(".base"))
        .map(|f| {
            (
                f.clone(),
                serde_yaml_ng::from_str(&text(f)).unwrap_or_else(|e| panic!("{f}: {e}")),
            )
        })
        .collect();
    // `base` code blocks in notes are bases too.
    let fence = Regex::new(r"(?s)```base\n(.*?)```").unwrap();
    for (path, body) in &vault_files().notes {
        for (i, c) in fence.captures_iter(body).enumerate() {
            docs.push((
                format!("{path} (base block {})", i + 1),
                serde_yaml_ng::from_str(&c[1]).unwrap_or_else(|e| panic!("{path}: {e}")),
            ));
        }
    }
    docs
}

#[test]
fn bases_follow_the_documented_syntax() {
    let docs = base_documents();
    assert!(docs.len() >= 5, "three files and two code blocks");
    for (path, doc) in &docs {
        for k in doc.as_mapping().unwrap().keys() {
            assert!(
                ["filters", "formulas", "properties", "summaries", "views"]
                    .contains(&k.as_str().unwrap()),
                "{path}: top-level key {k:?}"
            );
        }
        if let Some(f) = doc.get("filters") {
            check_filter(path, f);
        }
        let formulas: BTreeSet<String> = doc
            .get("formulas")
            .and_then(Yaml::as_mapping)
            .map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        check_expression(path, v.as_str().unwrap(), &[]);
                        k.as_str().unwrap().to_string()
                    })
                    .collect()
            })
            .unwrap_or_default();
        let custom: BTreeSet<String> = doc
            .get("summaries")
            .and_then(Yaml::as_mapping)
            .map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        check_expression(path, v.as_str().unwrap(), SUMMARY_METHODS);
                        k.as_str().unwrap().to_string()
                    })
                    .collect()
            })
            .unwrap_or_default();
        let known_property = |p: &str| match p.split_once('.') {
            Some(("formula", name)) => formulas.contains(name),
            Some(("file" | "note", _)) | None => true,
            Some(_) => false,
        };
        if let Some(props) = doc.get("properties").and_then(Yaml::as_mapping) {
            for (k, v) in props {
                assert!(
                    known_property(k.as_str().unwrap()),
                    "{path}: properties.{k:?}"
                );
                assert!(v.get("displayName").is_some_and(Yaml::is_string));
            }
        }
        let views = doc["views"]
            .as_sequence()
            .unwrap_or_else(|| panic!("{path}: no views"));
        assert!(!views.is_empty());
        for view in views {
            let name = view["name"]
                .as_str()
                .unwrap_or_else(|| panic!("{path}: a view without a name"));
            let what = format!("{path} / {name}");
            assert!(
                VIEW_TYPES.contains(&view["type"].as_str().unwrap()),
                "{what}: type"
            );
            for k in view.as_mapping().unwrap().keys() {
                // `sort` is not on the syntax page but is what Obsidian writes for a sorted table.
                assert!(
                    [
                        "type",
                        "name",
                        "filters",
                        "groupBy",
                        "groupOrder",
                        "sort",
                        "limit",
                        "order",
                        "summaries"
                    ]
                    .contains(&k.as_str().unwrap()),
                    "{what}: key {k:?}"
                );
            }
            if let Some(f) = view.get("filters") {
                check_filter(&what, f);
            }
            for p in view
                .get("order")
                .and_then(Yaml::as_sequence)
                .into_iter()
                .flatten()
            {
                assert!(known_property(p.as_str().unwrap()), "{what}: order {p:?}");
            }
            if let Some(g) = view.get("groupBy") {
                assert!(
                    known_property(g["property"].as_str().unwrap()),
                    "{what}: groupBy"
                );
                assert!(["ASC", "DESC"].contains(&g["direction"].as_str().unwrap()));
            }
            if let Some(order) = view.get("groupOrder") {
                assert!(
                    view.get("groupBy").is_some(),
                    "{what}: groupOrder without groupBy"
                );
                assert!(order.is_sequence());
            }
            for s in view
                .get("sort")
                .and_then(Yaml::as_sequence)
                .into_iter()
                .flatten()
            {
                assert!(
                    known_property(s["property"].as_str().unwrap()),
                    "{what}: sort"
                );
                assert!(["ASC", "DESC"].contains(&s["direction"].as_str().unwrap()));
            }
            if let Some(l) = view.get("limit") {
                assert!(l.is_u64(), "{what}: limit");
            }
            for (p, s) in view
                .get("summaries")
                .and_then(Yaml::as_mapping)
                .into_iter()
                .flatten()
            {
                assert!(
                    known_property(p.as_str().unwrap()),
                    "{what}: summaries.{p:?}"
                );
                let s = s.as_str().unwrap();
                assert!(
                    DEFAULT_SUMMARIES.contains(&s) || custom.contains(s),
                    "{what}: summary {s}"
                );
            }
        }
    }
}

#[test]
fn the_function_lint_catches_what_it_should() {
    let bad = |expr: &str| std::panic::catch_unwind(|| check_expression("t", expr, &[])).is_err();
    assert!(!bad(r#"if(rating, rating + " / 5", "unrated")"#));
    assert!(!bad("(file.size / 1024).round(1)"));
    assert!(!bad(
        r#"file.hasTag("a", "b") && file.name.lower().contains("x")"#
    ));
    assert!(
        !bad(r#""a(b" + file.name"#),
        "parentheses inside a string literal are not calls"
    );
    assert!(bad("file.nope()"));
    assert!(bad("lower(file.name)"), "a method is not a global");
    assert!(
        !bad("file.name.hasTag(1)"),
        "types are not checked, only names"
    );
    assert!(bad("values.mean()"), "mean is for summaries");
}

#[test]
fn bases_cover_filters_formulas_views_summaries_and_grouping() {
    let docs = base_documents();
    let yaml: String = vault_files()
        .files
        .iter()
        .filter(|f| f.ends_with(".base"))
        .map(|f| text(f))
        .collect::<Vec<_>>()
        .join("\n");
    let types: BTreeSet<String> = docs
        .iter()
        .flat_map(|(_, d)| {
            d["views"]
                .as_sequence()
                .unwrap()
                .iter()
                .map(|v| v["type"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        types,
        VIEW_TYPES
            .iter()
            .map(|s| s.to_string())
            .collect::<BTreeSet<_>>(),
        "the four view types"
    );
    for needle in [
        "  and:",
        "  or:",
        "not:",
        "formulas:",
        "summaries:",
        "groupBy:",
        "groupOrder:",
        "limit:",
        "sort:",
        "properties:",
        "displayName:",
        "formula.",
        "this.file",
        "file.hasTag",
        "file.inFolder",
        "file.hasLink",
        "values.mean()",
    ] {
        assert!(yaml.contains(needle), "no base uses {needle}");
    }
    let used_defaults: BTreeSet<&str> = DEFAULT_SUMMARIES
        .iter()
        .copied()
        .filter(|s| Regex::new(&format!(r": {s}\b")).unwrap().is_match(&yaml))
        .collect();
    for s in ["Average", "Sum", "Latest", "Checked", "Unique", "Filled"] {
        assert!(used_defaults.contains(s), "summary {s}");
    }
    for (view_link, base) in [
        ("Notes.base#All notes", "bases/Notes.base"),
        ("Projects.base#Board", "bases/Projects.base"),
        ("Tasks.base#By tag", "bases/Tasks.base"),
    ] {
        let (_, name) = view_link.split_once('#').unwrap();
        assert!(
            base_view_names(&text(base)).contains(&name.to_string()),
            "{view_link}"
        );
    }
    let projects: Yaml = serde_yaml_ng::from_str(&text("bases/Projects.base")).unwrap();
    let kanban = &projects["views"][0];
    assert_eq!(kanban["type"], "kanban");
    assert!(
        kanban["groupOrder"]
            .as_sequence()
            .unwrap()
            .iter()
            .any(Yaml::is_null),
        "null shows the notes without a status"
    );
}

#[test]
fn project_notes_match_what_the_projects_base_groups_by() {
    let v = vault_files();
    let mut statuses = BTreeMap::new();
    for (path, body) in v.notes.iter().filter(|(p, _)| p.starts_with("projects/")) {
        let doc: Yaml = serde_yaml_ng::from_str(split_frontmatter(body).unwrap().0).unwrap();
        statuses.insert(
            path.as_str(),
            doc.get("status").and_then(Yaml::as_str).map(String::from),
        );
    }
    let seen: BTreeSet<Option<&str>> = statuses.values().map(|s| s.as_deref()).collect();
    assert_eq!(
        seen,
        [Some("Planned"), Some("In progress"), Some("Done"), None].into(),
        "every kanban column, and the empty group"
    );
}

// ---- constructs ----

// (what, file, pattern): a construct the parity matrix names, and the line that holds it.
#[rustfmt::skip]
const CONSTRUCTS: &[(&str, &str, &str)] = &[
    ("[[Note]]", "links/Link Forms.md", r"\[\[Headings Target\]\]"),
    ("[[Note.md]]", "links/Link Forms.md", r"\[\[Headings Target\.md\]\]"),
    ("[[folder/Note]]", "links/Link Forms.md", r"\[\[links/Headings Target\]\]"),
    ("[[Note#Heading]]", "links/Link Forms.md", r"\[\[Headings Target#Second heading\]\]"),
    ("[[Note#H1#H2]]", "links/Link Forms.md", r"\[\[Headings Target#First heading#Nested heading\]\]"),
    ("[[#Heading]]", "links/Link Forms.md", r"\[\[#Wikilinks\]\]"),
    ("[[Note#^block]]", "links/Link Forms.md", r"\[\[Blocks Target#\^quote-block\]\]"),
    ("[[#^block]]", "links/Link Forms.md", r"\[\[#\^own-block\]\]"),
    ("[[Note|alias]]", "links/Link Forms.md", r"\[\[Headings Target\|display text\]\]"),
    ("[[Note#Heading|alias]]", "links/Link Forms.md", r"\[\[Headings Target#Second heading\|the second heading\]\]"),
    ("[[Note#^block|alias]]", "links/Link Forms.md", r"\[\[Blocks Target#\^quote-block\|a quoted block\]\]"),
    ("link with a different case", "links/Link Forms.md", r"\[\[headings target\]\]"),
    ("link to an alias", "links/Link Forms.md", r"\[\[Start\]\]"),
    ("link to a non-ASCII name", "links/Link Forms.md", r"\[\[Unicode é 日本\]\]"),
    ("link to a PDF page", "links/Link Forms.md", r"\[\[sample\.pdf#page=2\]\]"),
    ("link to a canvas", "links/Link Forms.md", r"\[\[Board\.canvas\]\]"),
    ("link to a base", "links/Link Forms.md", r"\[\[Notes\.base\]\]"),
    ("[[## trigger", "links/Link Forms.md", r"(?m)^\[\[##$"),
    ("[[^^ trigger", "links/Link Forms.md", r"(?m)^\[\[\^\^$"),
    ("heading that holds a link", "links/Link Forms.md", r"(?m)^### See \[\[Dead End\]\]$"),
    ("Markdown link, encoded", "links/Markdown Links.md", r"\]\(Headings%20Target\.md\)"),
    ("Markdown link, angle brackets", "links/Markdown Links.md", r"\]\(<Headings Target\.md>\)"),
    ("Markdown link to a heading", "links/Markdown Links.md", r"\(Headings%20Target\.md#Second%20heading\)"),
    ("Markdown link to nested headings", "links/Markdown Links.md", r"#First%20heading#Nested%20heading\)"),
    ("Markdown link to a block", "links/Markdown Links.md", r"#\^quote-block\)"),
    ("Markdown link to a missing note", "links/Markdown Links.md", r"\(Nothing%20Here\.md\)"),
    ("external, autolink, mailto, reference link", "links/Markdown Links.md", r"(?s)\(https://obsidian\.md\).*<https://obsidian\.md/help>.*mailto:.*\[ref\]: https"),
    ("same-folder preference", "a/Links From A.md", r"\[\[Note\]\]"),
    ("relative link", "a/Links From A.md", r"\[\[\./Note\]\] and \[\[\.\./b/Note\]\]"),
    ("block id on a paragraph", "links/Blocks Target.md", r"A paragraph with a block id\. \^para-block"),
    ("block id on a quote", "links/Blocks Target.md", r"(?m)^\^quote-block$"),
    ("block id on a list item", "links/Blocks Target.md", r"list item with an id \^list-item"),
    ("block id on a table", "links/Blocks Target.md", r"(?m)^\^table-block$"),
    ("repeated heading", "links/Headings Target.md", r"(?s)## Second heading.*## Second heading"),
    ("heading levels skipped", "links/Headings Target.md", r"(?m)^#### Skips two levels$"),
    ("unresolved with an alias", "links/Unresolved Links.md", r"\[\[Missing\|shown as another name\]\]"),
    ("unresolved with a heading", "links/Unresolved Links.md", r"\[\[Missing#Heading\]\]"),
    ("missing heading in a note", "links/Unresolved Links.md", r"\[\[Headings Target#No such heading\]\]"),
    ("missing block in a note", "links/Unresolved Links.md", r"\[\[Blocks Target#\^no-such-block\]\]"),
    ("unresolved link in a property", "links/Unresolved Links.md", r#"(?m)^  - "\[\[Missing In Frontmatter\]\]""#),
    ("unresolved embed", "links/Unresolved Links.md", r"!\[\[Missing Embed\]\]"),
    ("alias note", "links/Aliases.md", r"(?m)^  - Alias One$"),
    ("unlinked mention", "links/Mentions.md", r"The note Aliases is named here without a link"),
    ("embed a note", "embeds/Embed Notes.md", r"(?m)^!\[\[Headings Target\]\]$"),
    ("embed a heading", "embeds/Embed Notes.md", r"!\[\[Headings Target#Second heading\]\]"),
    ("embed nested headings", "embeds/Embed Notes.md", r"!\[\[Headings Target#First heading#Nested heading\]\]"),
    ("embed a block", "embeds/Embed Notes.md", r"!\[\[Blocks Target#\^para-block\]\]"),
    ("embed a list item", "embeds/Embed Notes.md", r"!\[\[Blocks Target#\^list-item\]\]"),
    ("embed cycle", "embeds/Loop A.md", r"!\[\[Loop B\]\]"),
    ("embed image", "embeds/Embed Images.md", r"(?m)^!\[\[pixel\.png\]\]$"),
    ("embed image width", "embeds/Embed Images.md", r"!\[\[wide\.png\|100\]\]"),
    ("embed image width and height", "embeds/Embed Images.md", r"!\[\[wide\.png\|100x50\]\]"),
    ("embed image alt text", "embeds/Embed Images.md", r"!\[\[wide\.png\|Alt text for the image\]\]"),
    ("embed image alt text and size", "embeds/Embed Images.md", r"!\[\[wide\.png\|Alt text with a size\|120\]\]"),
    ("Markdown image with a size", "embeds/Embed Images.md", r"!\[A wide image\|160x90\]\(attachments/wide\.png\)"),
    ("external image", "embeds/Embed Images.md", r"!\[An external image with alt text\]\(https://example\.com/images/external\.jpg\)"),
    ("external image with a size", "embeds/Embed Images.md", r"!\[An external image with a size\|300x200\]\(https://"),
    ("image with spaces in its name", "embeds/Embed Images.md", r"Pasted%20image%2020261024101500\.png"),
    ("embed PDF", "embeds/Embed Media.md", r"(?m)^!\[\[sample\.pdf\]\]$"),
    ("embed PDF page", "embeds/Embed Media.md", r"!\[\[sample\.pdf#page=2\]\]"),
    ("embed PDF page and height", "embeds/Embed Media.md", r"!\[\[sample\.pdf#page=3&height=300\]\]"),
    ("embed PDF height", "embeds/Embed Media.md", r"!\[\[sample\.pdf#height=400\]\]"),
    ("embed audio", "embeds/Embed Media.md", r"!\[\[tone\.m4a\]\]"),
    ("embed video", "embeds/Embed Media.md", r"!\[\[clip\.webm\]\]"),
    ("embed canvas", "embeds/Embed Canvas and Base.md", r"(?m)^!\[\[Board\.canvas\]\]$"),
    ("embed base", "embeds/Embed Canvas and Base.md", r"(?m)^!\[\[Notes\.base\]\]$"),
    ("embed a base view", "embeds/Embed Canvas and Base.md", r"!\[\[Projects\.base#Board\]\]"),
    ("embedded search", "embeds/Embed Query.md", r"(?m)^```query$"),
    ("callout with a custom title", "callouts/Callout Titles and Folding.md", r"\[!tip\] A custom title"),
    ("callout folded by default", "callouts/Callout Titles and Folding.md", r"\[!faq\]- Folded by default"),
    ("callout open and foldable", "callouts/Callout Titles and Folding.md", r"\[!faq\]\+ Open by default"),
    ("callout with rich content", "callouts/Callout Titles and Folding.md", r"(?s)> ```js.*> !\[\[pixel\.png\]\]"),
    ("nested callouts", "callouts/Nested Callouts.md", r"(?m)^> > > \[!example\] Innermost$"),
    ("callout in a plain quote", "callouts/Nested Callouts.md", r"(?m)^> > \[!tip\] Inside the quote$"),
    ("unknown callout type", "callouts/Custom Callout.md", r"\[!custom-undefined\]"),
    ("nested tag", "tagging/Tags.md", r"#project/alpha/beta"),
    ("tag case", "tagging/Tags.md", r"#MIXED-CASE"),
    ("tag in a heading, list, task, quote, table, callout", "tagging/Tags.md", r"(?s)#heading-tag.*#list-tag.*#task-tag.*#quote-tag.*#table-tag.*#callout-tag"),
    ("numeric-only tag", "tagging/Tags.md", r"A number alone: #2026"),
    ("not a tag: URL fragment", "tagging/Tags.md", r"page#anchor"),
    ("not a tag: escaped", "tagging/Tags.md", r"\\#escaped"),
    ("Unicode tags", "tagging/Unicode Tags.md", r"(?s)#café.*#日本語.*#ключ.*#ελληνικά"),
    ("frontmatter tag with a hash", "tagging/Frontmatter Tags.md", r##"- "#hashed-in-frontmatter""##),
    ("frontmatter tag, nested", "tagging/Frontmatter Tags.md", r"- fm/nested"),
    ("tags as a comma-separated string", "tagging/Tags As Text.md", r"(?m)^tags: text-tag-a, text-tag-b$"),
    ("cssclasses", "properties/Aliases and Cssclasses.md", r"(?m)^cssclasses:\n  - wide-page$"),
    ("alias with a comma", "properties/Aliases and Cssclasses.md", r#""Alias, with comma""#),
    ("link in a property", "properties/Link Properties.md", r#"author: "\[\[Dead End\]\]""#),
    ("Markdown link in a property", "properties/Link Properties.md", r#""\[Blocks Target\]\(Blocks%20Target\.md\)""#),
    ("empty frontmatter", "properties/Empty Frontmatter.md", r"\A---\n---\n"),
    ("only frontmatter", "properties/Frontmatter Only.md", r"(?s)\A---\n.*\n---\n+\z"),
    ("task: open", "tasks/Task Statuses.md", r"(?m)^- \[ \] To do$"),
    ("task: capital X", "tasks/Task Statuses.md", r"(?m)^- \[X\] "),
    ("task: emoji status", "tasks/Task Statuses.md", r"(?m)^- \[😀\] "),
    ("task: nested", "tasks/Task Statuses.md", r"(?m)^    - \[/\] Grandchild$"),
    ("task: numbered", "tasks/Task Statuses.md", r"(?m)^1\. \[ \] Numbered task$"),
    ("task: in a callout", "tasks/Task Statuses.md", r"(?m)^> - \[ \] Task in a callout$"),
    ("task: with a block id", "tasks/Task Statuses.md", r"\[ \] A task with a block id \^task-block"),
    ("task: in a fence is not one", "tasks/Task Statuses.md", r"(?s)```\n- \[ \] A task inside a fence\n```"),
    ("footnote reference", "syntax/Footnotes.md", r"\[\^1\].*\[\^named\]"),
    ("inline footnote", "syntax/Footnotes.md", r"\^\[This text is written in place\.\]"),
    ("footnote without a definition", "syntax/Footnotes.md", r"\[\^undefined\]"),
    ("footnote definition with a second paragraph", "syntax/Footnotes.md", r"(?m)^    It has a second paragraph"),
    ("inline comment", "syntax/Comments and Highlights.md", r"visible %%hidden in reading view%% visible"),
    ("block comment", "syntax/Comments and Highlights.md", r"(?m)^%%\nA block comment"),
    ("highlight", "syntax/Comments and Highlights.md", r"==Highlighted text=="),
    ("highlight colours", "syntax/Comments and Highlights.md", r"==🔴 red== ==🟠 orange== ==🟡 yellow== ==🟢 green== ==🔵 blue== ==🟣 purple=="),
    ("inline math", "syntax/Math.md", r"\$e\^\{i\\pi\} \+ 1 = 0\$"),
    ("block math", "syntax/Math.md", r"(?m)^\$\$\n\\int_0\^1"),
    ("dollar amounts that are not math", "syntax/Math.md", r"it cost \$5 and then \$10"),
    ("mermaid flowchart", "syntax/Mermaid.md", r"```mermaid\nflowchart LR"),
    ("mermaid sequence diagram", "syntax/Mermaid.md", r"sequenceDiagram"),
    ("mermaid error", "syntax/Mermaid.md", r"this is not a diagram"),
    ("table alignment", "syntax/Tables.md", r"\| :--- \| :----: \| ----: \|"),
    ("table with an escaped pipe", "syntax/Tables.md", r"a \\\| b"),
    ("table without leading pipes", "syntax/Tables.md", r"(?m)^Name \| Value$"),
    ("raw HTML block", "syntax/Raw HTML.md", r#"<div class="custom-block" id="custom-id">"#),
    ("raw HTML details", "syntax/Raw HTML.md", r"<details>\n<summary>"),
    ("HTML removed by sanitising", "syntax/Raw HTML.md", r#"(?s)<script>.*<iframe.*javascript:.*<style>.*onerror="#),
    ("Hebrew", "syntax/Bidirectional Text.md", r"שלום עולם"),
    ("Arabic", "syntax/Bidirectional Text.md", r"مرحبا بالعالم"),
    ("setext heading", "syntax/Inline Formatting.md", r"(?m)^Setext heading\n=+$"),
    ("hard line break", "syntax/Inline Formatting.md", r"(?m)^A line ending in two spaces  $"),
    ("heading with a hash", "review/Heading With Hash.md", r"(?m)^## C# basics$"),
    ("link to a heading with a hash", "review/Heading With Hash.md", r"\[\[Heading With Hash#C# basics\]\]"),
    ("link with the hash dropped", "review/Heading With Hash.md", r"\[\[Heading With Hash#C basics\]\]"),
    ("follow-to-create", "review/Missing Note Creation.md", r"\[\[Created On Click\]\]"),
    ("create with an extension named", "review/Missing Note Creation.md", r"\[\[Created With Extension\.md\]\]"),
    ("create in a new folder", "review/Missing Note Creation.md", r"\[\[Created Deep/Created In Folder\]\]"),
    ("twins as separate nodes", "review/Missing Note Creation.md", r"\[\[a/Note\]\] and \[\[b/Note\]\]"),
    ("startup note", "Home.md", r"(?s)^---\n.*aliases:.*Start.*cssclasses:.*dashboard"),
    ("base view embedded in the startup note", "Home.md", r"!\[\[Notes\.base#All notes\]\]"),
    ("base code block", "Home.md", r"```base\nfilters:"),
    ("slides", "slides/Deck.md", r"(?m)^---$\n\n## Second slide"),
    ("Dataview inline field", "dataview/Dataview Fields.md", r"(?m)^status:: active$"),
    ("Dataview bracketed field", "dataview/Dataview Fields.md", r"\[mood:: calm\]"),
    ("Dataview inline query", "dataview/Dataview Fields.md", r"`= this\.status`"),
    ("Dataview TABLE, LIST, TASK, CALENDAR", "dataview/Dataview Fields.md", r"(?s)```dataview\nTABLE.*```dataview\nLIST.*```dataview\nTASK.*```dataview\nCALENDAR"),
    ("daily note template", "templates/Daily.md", r"\{\{date:YYYY-MM-DD\}\}.*"),
    ("template variables", "templates/Meeting.md", r"\{\{title\}\}(?s:.*)\{\{date:dddd, MMMM Do YYYY\}\}(?s:.*)\{\{time\}\}"),
    ("weekly note", "weekly/2026-W43.md", r"\[\[2026-10-23\]\]"),
    ("excluded note", "excluded/Hidden Note.md", r"excluded-marker-xyzzy"),
    ("search: phrase, regex target, block id", "search/Search Corpus.md", r#"(?s)"an exact phrase".*id-1234.*\^search-block"#),
    ("search: case", "search/Case Sensitivity.md", r"Apple apple APPLE aPpLe"),
    ("project properties", "projects/Project Alpha.md", r"(?s)status: In progress.*rating: 5.*due: 2026-11-15.*done: false.*cover: attachments/pixel\.png"),
];

#[test]
fn each_construct_is_in_its_file() {
    let v = vault_files();
    let mut missing = Vec::new();
    for (what, file, pattern) in CONSTRUCTS {
        let body = v
            .notes
            .get(*file)
            .unwrap_or_else(|| panic!("{what}: no note {file}"));
        if !Regex::new(pattern)
            .unwrap_or_else(|e| panic!("{what}: {e}"))
            .is_match(body)
        {
            missing.push(format!("{what} in {file}"));
        }
    }
    assert!(
        missing.is_empty(),
        "{} constructs are not where they should be:\n{}",
        missing.len(),
        missing.join("\n")
    );
    assert!(CONSTRUCTS.len() >= 120);
    let whats: BTreeSet<&str> = CONSTRUCTS.iter().map(|c| c.0).collect();
    assert_eq!(whats.len(), CONSTRUCTS.len(), "a construct is listed twice");
}

#[test]
fn code_blocks_cover_many_languages() {
    let body = text("syntax/Code Blocks.md");
    let langs: BTreeSet<String> = Regex::new(r"(?m)^```(\S+)$")
        .unwrap()
        .captures_iter(&body)
        .map(|c| c[1].to_string())
        .collect();
    for l in [
        "rust",
        "python",
        "javascript",
        "typescript",
        "json",
        "yaml",
        "bash",
        "css",
        "html",
        "sql",
        "c",
        "cpp",
        "go",
        "diff",
        "toml",
        "markdown",
        "nonsense-language",
    ] {
        assert!(langs.contains(l), "no {l} block");
    }
    assert!(
        body.contains("~~~\nA tilde fence")
            && body.contains("````\nA four-backtick fence")
            && body.contains("\n    An indented code block")
    );
    assert!(body.contains("```\nno language at all\n```"));
}

// ---- the crate, not the vault ----

#[test]
fn json_canvas_samples_carry_their_licence_and_source() {
    let dir = crate_dir().join("jsoncanvas");
    let licence = fs::read_to_string(dir.join("LICENSE")).unwrap();
    assert!(licence.contains("MIT License") && licence.contains("Permission is hereby granted"));
    let source = fs::read_to_string(dir.join("SOURCE.md")).unwrap();
    assert!(
        Regex::new(r"\b[0-9a-f]{40}\b").unwrap().is_match(&source),
        "SOURCE.md names a commit"
    );
    assert!(source.contains("github.com/obsidianmd/jsoncanvas"));
    assert!(
        source.contains("obsidian-help") && source.to_lowercase().contains("licen"),
        "and says why the help vault is not here"
    );
    assert!(dir.join("sample.canvas").exists());
}

#[test]
fn reference_runs_are_ignored_except_the_fixture_vaults() {
    let crate_dir = crate_dir();
    let check = |path: &str| -> Option<bool> {
        let out = std::process::Command::new("git")
            .args([
                "-C",
                crate_dir.to_str().unwrap(),
                "check-ignore",
                "-q",
                "--no-index",
                path,
            ])
            .output()
            .ok()?;
        match out.status.code()? {
            0 => Some(true),
            1 => Some(false),
            _ => None,
        }
    };
    let Some(ignored) = check("reference/obsidian-help/manifest.json") else {
        eprintln!(
            "git is not available here; the ignore rules are checked by reading .gitignore instead"
        );
        let rules = fs::read_to_string(crate_dir.join(".gitignore")).unwrap();
        assert!(
            rules.lines().any(|l| l == "/reference/*")
                && rules.lines().any(|l| l == "!/reference/fixture-vault/")
        );
        return;
    };
    assert!(
        ignored,
        "a reference run against an unlicensed vault must never be committed"
    );
    assert_eq!(check("reference/anything-else/links.json"), Some(true));
    assert_eq!(
        check("reference/fixture-vault/links.json"),
        Some(false),
        "the fixture vault's own run is kept"
    );
    assert_eq!(
        check("vault/.obsidian/workspace.json"),
        Some(true),
        "Obsidian rewrites it on every open"
    );
    assert_eq!(check("vault/.obsidian/app.json"), Some(false));
}

#[test]
fn nothing_but_the_fixture_vault_and_licensed_samples_is_here() {
    let root = crate_dir();
    let top: BTreeSet<String> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != "target")
        .collect();
    let expected: BTreeSet<String> = [
        ".gitignore",
        "COVERAGE.md",
        "Cargo.toml",
        "README.md",
        "jsoncanvas",
        "queries.json",
        "script",
        "src",
        "tests",
        "vault",
    ]
    .map(String::from)
    .into();
    let extra: Vec<_> = top.difference(&expected).collect();
    assert!(
        extra.is_empty() || extra == [&"reference".to_string()],
        "unexpected entries: {extra:?}"
    );
    let reference = root.join("reference");
    if reference.exists() {
        let kept: Vec<_> = fs::read_dir(reference)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            kept.iter().all(|k| k == "fixture-vault"),
            "{kept:?}: only the fixture vault's run may be kept"
        );
    }
    assert!(
        !Path::new(&vault()).join(".obsidian/plugins").exists(),
        "no community plugin code in the vault"
    );
}
