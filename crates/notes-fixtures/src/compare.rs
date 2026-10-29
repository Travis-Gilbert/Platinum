//! Compares two recordings of a reading view's DOM: Obsidian's (`reference/<vault>/dom/`) with
//! what Frame renders. See "Comparing DOM structure" in the README for the normalized structure
//! and every exclusion; the constants below carry the same reasons next to the code.
//!
//! Both sides are parsed with html5ever (through `scraper`), so a difference of serialization
//! (attribute order, quoting, `<br/>` against `<br>`) is never a difference of structure.
use anyhow::{Context, Result, bail};
use percent_encoding::percent_decode_str;
use regex::Regex;
use scraper::{ElementRef, Html, Node, Selector};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};
use walkdir::WalkDir;

/// The element whose children are the reading view's content. Everything above it (the
/// `markdown-reading-view` and `markdown-preview-view` wrappers) belongs to the host page:
/// `cssclasses` reaches the wrapper's classes through `ReadingView::cssclasses`, and the
/// sizer's `style` is layout.
const CONTENT_ROOT: &str = ".markdown-preview-sizer";

/// Children of the content root that are not the note's rendering, and why.
const EXCLUDED_CHILDREN: &[(&str, &str)] = &[
    (
        ".markdown-preview-pusher",
        "a 1px spacer Obsidian's virtual scroller inserts to size the scroll area",
    ),
    (
        ".mod-footer",
        "the embedded-backlinks footer: a host feature built from the index, not from the note",
    ),
];

/// Attributes that are not compared, and why.
const IGNORED_ATTRIBUTES: &[(&str, &str)] = &[
    (
        "data-sourcepos",
        "Frame's own annotation for the host (task toggles, scroll sync); Obsidian keeps it internally",
    ),
    (
        "tabindex",
        "focus order of Obsidian's editing UI; a theme cannot select it",
    ),
    (
        "contenteditable",
        "editing affordance: Frame's reading view edits through the host, never in the page",
    ),
    ("spellcheck", "editing affordance"),
    ("autocapitalize", "editing affordance"),
    ("enterkeyhint", "editing affordance"),
    ("inputmode", "editing affordance"),
    (
        "placeholder",
        "editing affordance: hint text of an empty editor",
    ),
    (
        "draggable",
        "drag source flag Obsidian's workspace wires to its own drag manager",
    ),
    (
        "aria-disabled",
        "momentary state of an icon the editor enables and disables",
    ),
];

/// Attributes that are DOM properties in Obsidian's property editor and so absent from its
/// serialized DOM; a renderer that shows the value has to write them as attributes.
const EDITOR_VALUE_ATTRIBUTES: &[&str] = &["value", "checked"];

/// Classes the scripts of the page add to what they have finished with.
const SCRIPT_CLASSES: &[&str] = &["is-loaded"];

/// Elements the recorder's footnote ids use: `fn-1-<16 hex>`, `fnref-1-1-<16 hex>`.
const FOOTNOTE_ID: &str = r"^(#?(?:fn|fnref)-\d+(?:-\d+)?-)([a-f0-9]{16})$";

/// The recorder's stand-in for a layout number (see `normalize.rs`); in the expected side it
/// matches any number in the actual one.
const LAYOUT: &str = "__layout__";

/// The recorder's form of a vault resource URL.
const RESOURCE_PREFIX: &str = "app://__app__/__vault__/";

/// Elements whose text is not rendered at its edges.
const BLOCK: &[&str] = &[
    "address",
    "article",
    "aside",
    "audio",
    "blockquote",
    "canvas",
    "details",
    "dd",
    "div",
    "dl",
    "dt",
    "figure",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "iframe",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
    "video",
];

/// What to compare and how strictly.
#[derive(Clone, Debug)]
pub struct Options {
    /// CSS selectors for elements dropped from both trees: what a page script adds to the
    /// markup (the copy button on a code block) and the renderer therefore does not emit.
    pub ignore: Vec<String>,
    /// CSS selectors for elements compared by their tag, classes and `data-*` attributes alone:
    /// their subtree is produced by a script (MathJax, Mermaid, pdf.js) that does not run here.
    pub opaque: Vec<String>,
    /// CSS selectors for elements compared by tag, classes, `data-*` attributes and the plain
    /// text of their subtree: a script (Prism) wraps the text in elements.
    pub opaque_text: Vec<String>,
    /// The expected side may end before the actual one. Obsidian renders a note's sections as
    /// they come into view, so the recording of a long note stops where the window did. The
    /// blocks present are compared; the verdict says how many of the actual ones that is.
    pub allow_truncated_expected: bool,
    /// The URL prefix under which the actual side serves vault files; `src` attributes are
    /// compared as vault-relative paths.
    pub resource_prefix: String,
    /// Differences reported per note before the rest are only counted.
    pub max_differences: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            ignore: Vec::new(),
            opaque: Vec::new(),
            opaque_text: Vec::new(),
            allow_truncated_expected: false,
            resource_prefix: "/files/".into(),
            max_differences: 12,
        }
    }
}

/// One place where the two trees differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    /// The path from the content root, e.g. `div.el-ul[3] > ul.has-list-bullet > li[0]`.
    pub path: String,
    pub expected: String,
    pub actual: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Every block matches.
    Match { blocks: usize },
    /// The expected side is a truncated recording: its `verified` blocks match the first
    /// `verified` of the actual `total`.
    Prefix { verified: usize, total: usize },
    Mismatch {
        differences: Vec<Difference>,
        /// Differences found beyond those listed.
        omitted: usize,
    },
}

impl Verdict {
    pub fn matches(&self) -> bool {
        !matches!(self, Verdict::Mismatch { .. })
    }
}

/// The result for one note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteReport {
    /// The recording's path under its directory, e.g. `a/Note.md.html`.
    pub note: String,
    pub verdict: Verdict,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub notes: Vec<NoteReport>,
    /// Recordings only the expected directory has.
    pub missing: Vec<String>,
    /// Recordings only the actual directory has; they have nothing to be compared with.
    pub extra: Vec<String>,
}

impl Report {
    pub fn matched(&self) -> usize {
        self.notes
            .iter()
            .filter(|n| matches!(n.verdict, Verdict::Match { .. }))
            .count()
    }

    pub fn prefix_matched(&self) -> usize {
        self.notes
            .iter()
            .filter(|n| matches!(n.verdict, Verdict::Prefix { .. }))
            .count()
    }

    pub fn mismatched(&self) -> usize {
        self.notes.iter().filter(|n| !n.verdict.matches()).count()
    }

    /// Every note matched and none is missing.
    pub fn passes(&self) -> bool {
        self.mismatched() == 0 && self.missing.is_empty()
    }

    /// The report as the CLI prints it.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for n in &self.notes {
            match &n.verdict {
                Verdict::Match { blocks } => {
                    let _ = writeln!(out, "ok       {} ({blocks} blocks)", n.note);
                }
                Verdict::Prefix { verified, total } => {
                    let _ = writeln!(
                        out,
                        "prefix   {} (expected recording holds {verified} of {total} blocks; those match)",
                        n.note
                    );
                }
                Verdict::Mismatch {
                    differences,
                    omitted,
                } => {
                    let _ = writeln!(out, "MISMATCH {}", n.note);
                    for d in differences {
                        let _ = writeln!(
                            out,
                            "    at {}\n      expected: {}\n      actual:   {}",
                            d.path, d.expected, d.actual
                        );
                    }
                    if *omitted > 0 {
                        let _ = writeln!(out, "    ... and {omitted} more");
                    }
                }
            }
        }
        for m in &self.missing {
            let _ = writeln!(out, "MISSING  {m} (no actual recording)");
        }
        for e in &self.extra {
            let _ = writeln!(out, "extra    {e} (no expected recording; not compared)");
        }
        let _ = writeln!(
            out,
            "{} notes: {} match, {} match on the recorded prefix, {} mismatch, {} missing, {} extra",
            self.notes.len() + self.missing.len(),
            self.matched(),
            self.prefix_matched(),
            self.mismatched(),
            self.missing.len(),
            self.extra.len()
        );
        out
    }
}

/// Compares every recording under `expected` with the one of the same relative path under
/// `actual`. `note` limits the comparison to recordings whose path contains it.
pub fn compare_dirs(
    expected: &Path,
    actual: &Path,
    options: &Options,
    note: Option<&str>,
) -> Result<Report> {
    let (exp, act) = (recordings(expected)?, recordings(actual)?);
    let keep = |name: &String| note.is_none_or(|n| name.contains(n));
    let mut report = Report::default();
    for (name, path) in exp.iter().filter(|(n, _)| keep(n)) {
        match act.get(name) {
            None => report.missing.push(name.clone()),
            Some(other) => {
                let read =
                    |p: &PathBuf| fs::read_to_string(p).with_context(|| p.display().to_string());
                report.notes.push(NoteReport {
                    note: name.clone(),
                    verdict: compare_html(&read(path)?, &read(other)?, options)?,
                });
            }
        }
    }
    report.extra = act
        .keys()
        .filter(|n| keep(n) && !exp.contains_key(*n))
        .cloned()
        .collect();
    if report.notes.is_empty() && report.missing.is_empty() {
        bail!("no recordings under {} match", expected.display());
    }
    Ok(report)
}

fn recordings(dir: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut out = BTreeMap::new();
    for e in WalkDir::new(dir) {
        let e = e.with_context(|| dir.display().to_string())?;
        if e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "html") {
            let rel = e
                .path()
                .strip_prefix(dir)?
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(rel, e.into_path());
        }
    }
    if out.is_empty() {
        bail!("no .html recordings under {}", dir.display());
    }
    Ok(out)
}

/// Compares two recordings of one note.
pub fn compare_html(expected: &str, actual: &str, options: &Options) -> Result<Verdict> {
    let cx = Walker::new(options)?;
    let (e, a) = (cx.tree(expected), cx.tree(actual));
    let mut found = Vec::new();
    let mut path = String::new();
    // The content root is compared by its children alone, so the verdict's counts are blocks.
    let verified = if options.allow_truncated_expected && e.children.len() < a.children.len() {
        let total = a.children.len();
        let prefix = Elem {
            children: a.children[..e.children.len()].to_vec(),
            ..a.clone()
        };
        diff_elem(&e, &prefix, &mut path, &mut found);
        Some((e.children.len(), total))
    } else {
        diff_elem(&e, &a, &mut path, &mut found);
        None
    };
    Ok(if found.is_empty() {
        match verified {
            Some((verified, total)) => Verdict::Prefix { verified, total },
            None => Verdict::Match {
                blocks: a.children.len(),
            },
        }
    } else {
        let omitted = found.len().saturating_sub(options.max_differences);
        found.truncate(options.max_differences);
        Verdict::Mismatch {
            differences: found,
            omitted,
        }
    })
}

/// The normalized structure of one element.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Elem {
    tag: String,
    classes: BTreeSet<String>,
    attrs: BTreeMap<String, String>,
    children: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Item {
    Text(String),
    Elem(Elem),
}

struct Walker {
    ignore: Vec<Selector>,
    opaque: Vec<Selector>,
    opaque_text: Vec<Selector>,
    excluded: Vec<Selector>,
    root: Selector,
    footnote: Regex,
    prefix: String,
}

fn selectors(list: &[String]) -> Result<Vec<Selector>> {
    list.iter()
        .map(|s| Selector::parse(s).map_err(|e| anyhow::anyhow!("selector {s:?}: {e}")))
        .collect()
}

impl Walker {
    fn new(o: &Options) -> Result<Self> {
        Ok(Self {
            ignore: selectors(&o.ignore)?,
            opaque: selectors(&o.opaque)?,
            opaque_text: selectors(&o.opaque_text)?,
            excluded: selectors(
                &EXCLUDED_CHILDREN
                    .iter()
                    .map(|(s, _)| s.to_string())
                    .collect::<Vec<_>>(),
            )?,
            root: Selector::parse(CONTENT_ROOT).expect("constant selector"),
            footnote: Regex::new(FOOTNOTE_ID)?,
            prefix: o.resource_prefix.clone(),
        })
    }

    /// The tree of the content root's children, or of the whole fragment when the recording
    /// is one (a rendered `html` before the page wraps it).
    fn tree(&self, html: &str) -> Elem {
        let doc = Html::parse_fragment(html);
        let root = doc.root_element();
        let root = root.select(&self.root).next().unwrap_or(root);
        let mut footnotes = Vec::new();
        let mut e = self.elem(root, &mut footnotes, true);
        e.tag = "content".into();
        e.classes.clear();
        e.attrs.clear();
        e
    }

    fn elem(&self, el: ElementRef, footnotes: &mut Vec<String>, is_root: bool) -> Elem {
        let value = el.value();
        let opaque = self.opaque.iter().any(|s| s.matches(&el));
        let opaque_text = self.opaque_text.iter().any(|s| s.matches(&el));
        let scripted = opaque || opaque_text;
        let classes = value
            .classes()
            .filter(|c| !(scripted && SCRIPT_CLASSES.contains(c)))
            .map(str::to_string)
            .collect();
        let editor_value = value.classes().any(|c| c.starts_with("metadata-input"));
        let mut attrs = BTreeMap::new();
        for (name, v) in value.attrs() {
            let name = name.to_ascii_lowercase();
            let ignored = name == "class"
                || IGNORED_ATTRIBUTES.iter().any(|(n, _)| *n == name)
                || (editor_value && EDITOR_VALUE_ATTRIBUTES.contains(&name.as_str()));
            // Only a `data-*` attribute is compared on an element whose content a script
            // owns: its other attributes are the script's.
            if ignored || (scripted && !name.starts_with("data-")) {
                continue;
            }
            attrs.insert(name.clone(), self.attribute(&name, v, footnotes));
        }
        let children = if opaque {
            Vec::new()
        } else if opaque_text {
            let text: String = el.text().collect();
            if text.is_empty() {
                Vec::new()
            } else {
                vec![Item::Text(text)]
            }
        } else {
            self.children(el, footnotes, is_root)
        };
        Elem {
            tag: value.name().to_string(),
            classes,
            attrs,
            children,
        }
    }

    fn children(&self, el: ElementRef, footnotes: &mut Vec<String>, is_root: bool) -> Vec<Item> {
        let mut items: Vec<Item> = Vec::new();
        let in_pre = el.value().name() == "pre"
            || el
                .ancestors()
                .filter_map(ElementRef::wrap)
                .any(|a| a.value().name() == "pre");
        for child in el.children() {
            match child.value() {
                Node::Text(t) => match items.last_mut() {
                    Some(Item::Text(prev)) => prev.push_str(t),
                    _ => items.push(Item::Text(t.to_string())),
                },
                Node::Element(_) => {
                    let c = ElementRef::wrap(child).expect("element node");
                    if self.ignore.iter().any(|s| s.matches(&c)) {
                        continue;
                    }
                    if is_root && self.excluded.iter().any(|s| s.matches(&c)) {
                        continue;
                    }
                    items.push(Item::Elem(self.elem(c, footnotes, false)));
                }
                _ => {}
            }
        }
        if !in_pre {
            collapse_whitespace(el.value().name(), &mut items);
        }
        items
    }

    fn attribute(&self, name: &str, value: &str, footnotes: &mut Vec<String>) -> String {
        match name {
            "style" => self.style(value),
            "src" => self.resource(value),
            "id" | "href" | "data-footnote-id" => match self.footnote.captures(value) {
                Some(c) => {
                    let hex = c[2].to_string();
                    let n = footnotes.iter().position(|h| *h == hex).unwrap_or_else(|| {
                        footnotes.push(hex);
                        footnotes.len() - 1
                    });
                    format!("{}__footnote_{}__", &c[1], n + 1)
                }
                None => value.to_string(),
            },
            _ => value.to_string(),
        }
    }

    /// A vault file's URL as its vault-relative path, whichever side wrote it.
    fn resource(&self, url: &str) -> String {
        let path = url
            .strip_prefix(RESOURCE_PREFIX)
            .or_else(|| url.strip_prefix(self.prefix.as_str()))
            .map(|p| p.split(['?', '#']).next().unwrap_or(p));
        match path {
            Some(p) => percent_decode_str(p).decode_utf8_lossy().into_owned(),
            None => url.to_string(),
        }
    }

    /// `a: 1px; b: x` as sorted `a:1px;b:x`.
    fn style(&self, value: &str) -> String {
        let mut declarations: BTreeMap<&str, &str> = BTreeMap::new();
        for d in value.split(';') {
            if let Some((k, v)) = d.split_once(':') {
                declarations.insert(k.trim(), v.trim());
            }
        }
        declarations
            .iter()
            .map(|(k, v)| format!("{k}:{v};"))
            .collect()
    }
}

/// Collapses runs of whitespace to one space, as CSS does outside `pre`, and drops what a
/// browser would not render: whitespace beside a block and at a block's edges.
fn collapse_whitespace(parent: &str, items: &mut Vec<Item>) {
    for item in items.iter_mut() {
        if let Item::Text(t) = item {
            let mut out = String::with_capacity(t.len());
            let mut space = false;
            for c in t.chars() {
                if c.is_ascii_whitespace() {
                    space = true;
                } else {
                    if space {
                        out.push(' ');
                    }
                    space = false;
                    out.push(c);
                }
            }
            if space {
                out.push(' ');
            }
            *t = out;
        }
    }
    let is_block =
        |i: Option<&Item>| matches!(i, Some(Item::Elem(e)) if BLOCK.contains(&e.tag.as_str()));
    let parent_block = BLOCK.contains(&parent) || parent == "content";
    let snapshot: Vec<bool> = (0..items.len())
        .map(|i| {
            let before = if i == 0 { None } else { items.get(i - 1) };
            let after = items.get(i + 1);
            matches!(&items[i], Item::Text(t) if t == " ")
                && (is_block(before)
                    || is_block(after)
                    || (parent_block && (i == 0 || i + 1 == items.len())))
        })
        .collect();
    let mut keep = snapshot.into_iter();
    items.retain(|_| !keep.next().unwrap_or(false));
    if parent_block {
        if let Some(Item::Text(t)) = items.first_mut() {
            *t = t.trim_start().to_string();
        }
        if let Some(Item::Text(t)) = items.last_mut() {
            *t = t.trim_end().to_string();
        }
        items.retain(|i| !matches!(i, Item::Text(t) if t.is_empty()));
    }
}

fn label(e: &Elem) -> String {
    let mut s = e.tag.clone();
    for c in &e.classes {
        s.push('.');
        s.push_str(c);
    }
    s
}

fn describe(e: &Elem) -> String {
    let mut s = format!("<{}", label(e));
    for (k, v) in &e.attrs {
        let _ = write!(s, " {k}=\"{v}\"");
    }
    s.push('>');
    s
}

fn describe_item(i: &Item) -> String {
    match i {
        Item::Text(t) => format!("{t:?}"),
        Item::Elem(e) => describe(e),
    }
}

fn diff_elem(e: &Elem, a: &Elem, path: &mut String, out: &mut Vec<Difference>) {
    let here = if path.is_empty() {
        label(e)
    } else {
        path.clone()
    };
    let mut push = |what: &str, expected: String, actual: String| {
        out.push(Difference {
            path: format!("{here}{what}"),
            expected,
            actual,
        });
    };
    if e.tag != a.tag {
        push("", format!("<{}>", e.tag), format!("<{}>", a.tag));
        return;
    }
    if e.classes != a.classes {
        let list = |c: &BTreeSet<String>| c.iter().cloned().collect::<Vec<_>>().join(" ");
        push("@class", list(&e.classes), list(&a.classes));
    }
    for name in e
        .attrs
        .keys()
        .chain(a.attrs.keys())
        .collect::<BTreeSet<_>>()
    {
        let (x, y) = (e.attrs.get(name), a.attrs.get(name));
        let same = match (x, y) {
            (Some(x), Some(y)) => x == y || (name == "style" && layout_style_equal(x, y)),
            (None, None) => true,
            _ => false,
        };
        if !same {
            let show = |v: Option<&String>| v.map_or("(absent)".to_string(), |v| format!("{v:?}"));
            push(&format!("@{name}"), show(x), show(y));
        }
    }
    // Align children by index; a count difference is reported once, then the common part is
    // still compared, so one inserted element does not hide the rest.
    let mut idx = 0usize;
    let mut elem_index = 0usize;
    if e.children.len() != a.children.len() {
        push(
            "/children",
            format!(
                "{} children: {}",
                e.children.len(),
                e.children
                    .iter()
                    .map(describe_item)
                    .take(6)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            format!(
                "{} children: {}",
                a.children.len(),
                a.children
                    .iter()
                    .map(describe_item)
                    .take(6)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }
    for (x, y) in e.children.iter().zip(&a.children) {
        match (x, y) {
            (Item::Text(x), Item::Text(y)) => {
                if x != y {
                    out.push(Difference {
                        path: format!("{here}/text[{idx}]"),
                        expected: format!("{x:?}"),
                        actual: format!("{y:?}"),
                    });
                }
            }
            (Item::Elem(x), Item::Elem(y)) => {
                let mut child = format!("{here} > {}[{elem_index}]", label(x));
                diff_elem(x, y, &mut child, out);
            }
            (x, y) => out.push(Difference {
                path: format!("{here}/child[{idx}]"),
                expected: describe_item(x),
                actual: describe_item(y),
            }),
        }
        if matches!(x, Item::Elem(_)) {
            elem_index += 1;
        }
        idx += 1;
    }
}

/// Styles are compared declaration by declaration; see [`LAYOUT`].
fn layout_style_equal(expected: &str, actual: &str) -> bool {
    static NUMBER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"-?\d+(?:\.\d+)?").expect("constant regex"));
    let decl = |s: &str| -> BTreeMap<String, String> {
        s.split(';')
            .filter_map(|d| d.split_once(':'))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    let (x, y) = (decl(expected), decl(actual));
    x.len() == y.len()
        && x.iter().all(|(k, ev)| {
            y.get(k).is_some_and(|av| {
                av == ev || (ev.contains(LAYOUT) && NUMBER.replace_all(av, LAYOUT) == *ev)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(expected: &str, actual: &str, o: &Options) -> Verdict {
        compare_html(expected, actual, o).unwrap()
    }

    fn wrap(inner: &str) -> String {
        format!(r#"<div class="markdown-preview-sizer">{inner}</div>"#)
    }

    #[test]
    fn serialization_is_not_structure() {
        let o = Options::default();
        let e = wrap(r#"<div class="el-p"><p dir="auto">a<br>b</p></div>"#);
        let a = wrap(r#"<div class="el-p"><p dir='auto'>a<br/>b</p></div>"#);
        assert_eq!(run(&e, &a, &o), Verdict::Match { blocks: 1 });
    }

    #[test]
    fn a_difference_names_its_place() {
        let o = Options::default();
        let e = wrap(
            r#"<div class="el-ul"><ul><li data-line="0">x</li><li data-line="1">y</li></ul></div>"#,
        );
        let a = wrap(
            r#"<div class="el-ul"><ul><li data-line="0">x</li><li data-line="2">y</li></ul></div>"#,
        );
        let Verdict::Mismatch { differences, .. } = run(&e, &a, &o) else {
            panic!("matched")
        };
        assert_eq!(differences.len(), 1);
        assert_eq!(
            differences[0].path,
            "content > div.el-ul[0] > ul[0] > li[1]@data-line"
        );
        assert_eq!(
            (
                differences[0].expected.as_str(),
                differences[0].actual.as_str()
            ),
            ("\"1\"", "\"2\"")
        );
    }

    #[test]
    fn classes_are_a_set_and_text_is_exact() {
        let o = Options::default();
        let e = wrap(r#"<div class="a b">x y</div>"#);
        assert!(run(&e, &wrap(r#"<div class="b a">x   y</div>"#), &o).matches());
        assert!(!run(&e, &wrap(r#"<div class="a b c">x y</div>"#), &o).matches());
        assert!(!run(&e, &wrap(r#"<div class="a b">x z</div>"#), &o).matches());
    }

    #[test]
    fn editing_affordances_sources_and_host_annotations_are_not_compared() {
        let o = Options::default();
        let e = wrap(
            r#"<div class="el-p" tabindex="0"><div class="inline-title" contenteditable="true" spellcheck="true">T</div></div>"#,
        );
        let a = wrap(
            r#"<div class="el-p" data-sourcepos="1:1-1:2"><div class="inline-title">T</div></div>"#,
        );
        assert!(run(&e, &a, &o).matches());
    }

    #[test]
    fn data_attributes_are_compared() {
        let o = Options::default();
        let e = wrap(r#"<a class="internal-link" data-href="X">x</a>"#);
        assert!(
            !run(
                &e,
                &wrap(r#"<a class="internal-link" data-href="Y">x</a>"#),
                &o
            )
            .matches()
        );
    }

    #[test]
    fn expected_layout_numbers_match_any_number() {
        let o = Options::default();
        let e = wrap(r#"<div style="width: __layout__px; color: red;">x</div>"#);
        assert!(
            run(
                &e,
                &wrap(r#"<div style="color:red; width:417.5px">x</div>"#),
                &o
            )
            .matches()
        );
        assert!(
            !run(
                &e,
                &wrap(r#"<div style="width:417px; color:blue">x</div>"#),
                &o
            )
            .matches()
        );
    }

    #[test]
    fn footnote_ids_are_numbered_by_first_use() {
        let o = Options::default();
        let e = wrap(
            r##"<sup id="fnref-1-1-__footnote_1__"><a href="#fn-1-__footnote_1__">1</a></sup>"##,
        );
        let a = wrap(
            r##"<sup id="fnref-1-1-0123456789abcdef"><a href="#fn-1-0123456789abcdef">1</a></sup>"##,
        );
        assert!(run(&e, &a, &o).matches());
        // The recorder's own tokens are not 16 hex digits and are kept as they are.
        assert!(run(&e, &e, &o).matches());
        let other = wrap(
            r##"<sup id="fnref-1-1-0123456789abcdef"><a href="#fn-1-ffffffffffffffff">1</a></sup>"##,
        );
        assert!(!run(&e, &other, &o).matches());
    }

    #[test]
    fn resources_compare_as_vault_paths() {
        let o = Options::default();
        let e = wrap(r#"<img src="app://__app__/__vault__/attachments/pixel%20x.png?__mtime__">"#);
        assert!(
            run(
                &e,
                &wrap(r#"<img src="/files/attachments/pixel%20x.png">"#),
                &o
            )
            .matches()
        );
        assert!(!run(&e, &wrap(r#"<img src="/files/attachments/other.png">"#), &o).matches());
    }

    #[test]
    fn opaque_subtrees_are_compared_by_their_container() {
        let o = Options {
            opaque: vec![".math".into()],
            opaque_text: vec!["code".into()],
            ..Options::default()
        };
        let e = wrap(
            r#"<span class="math is-loaded"><mjx-container>x</mjx-container></span><code class="is-loaded"><span class="token">a</span>b</code>"#,
        );
        let a = wrap(r#"<span class="math">x^2</span><code>ab</code>"#);
        assert!(run(&e, &a, &o).matches());
        let b = wrap(r#"<span class="math">x^2</span><code>ac</code>"#);
        assert!(!run(&e, &b, &o).matches());
    }

    #[test]
    fn ignored_elements_and_has_selectors() {
        let o = Options {
            ignore: vec!["button.copy-code-button".into()],
            opaque: vec![
                "div.el-pre:has(.mermaid-wrapper)".into(),
                "div.el-pre:has(pre.language-mermaid)".into(),
            ],
            ..Options::default()
        };
        let e = wrap(
            r#"<pre><code>x</code><button class="copy-code-button">Copy</button></pre><div class="el-pre"><div><div class="mermaid-wrapper">g</div></div></div>"#,
        );
        let a = wrap(
            r#"<pre><code>x</code></pre><div class="el-pre"><pre class="language-mermaid"><code>g</code></pre></div>"#,
        );
        assert!(run(&e, &a, &o).matches(), "{:?}", run(&e, &a, &o));
    }

    #[test]
    fn excluded_children_of_the_root_are_not_blocks() {
        let o = Options::default();
        let e = wrap(
            r#"<div class="markdown-preview-pusher"></div><div class="el-p">x</div><div class="mod-footer">f</div>"#,
        );
        assert_eq!(
            run(&e, &wrap(r#"<div class="el-p">x</div>"#), &o),
            Verdict::Match { blocks: 1 }
        );
    }

    #[test]
    fn a_truncated_recording_matches_on_its_prefix_only_when_allowed() {
        let e = wrap(r#"<div class="el-p">a</div>"#);
        let a = wrap(r#"<div class="el-p">a</div><div class="el-p">b</div>"#);
        assert!(!run(&e, &a, &Options::default()).matches());
        let allow = Options {
            allow_truncated_expected: true,
            ..Options::default()
        };
        assert_eq!(
            run(&e, &a, &allow),
            Verdict::Prefix {
                verified: 1,
                total: 2
            }
        );
        let c = wrap(r#"<div class="el-p">z</div><div class="el-p">b</div>"#);
        assert!(!run(&e, &c, &allow).matches());
    }

    #[test]
    fn whitespace_between_blocks_is_not_content() {
        let o = Options::default();
        let e = wrap(
            r#"<ul><li>a</li>
<li>b</li></ul>"#,
        );
        assert!(run(&e, &wrap("<ul><li>a</li><li>b</li></ul>"), &o).matches());
        let p = wrap("<pre>a\n  b</pre>");
        assert!(!run(&p, &wrap("<pre>a b</pre>"), &o).matches());
    }
}
