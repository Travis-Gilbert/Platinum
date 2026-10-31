//! Block structure: comrak's tree written as the reading view's per-block wrappers.
//!
//! Every top-level block becomes one `div.el-*` carrying its source position, as Obsidian renders
//! a note section by section. Blocks nested in callouts, quotes and list items are written as the
//! bare elements Obsidian puts there.
use super::{Doc, html, icons::icon, inlines};
use comrak::nodes::{
    AstNode, CalloutFold, ListType, NodeCallout, NodeCodeBlock, NodeList, NodeValue, Sourcepos,
    TableAlignment,
};
use comrak::{Arena, Options, parse_document};
use regex::Regex;
use std::sync::LazyLock;

/// The options `parse::options` reads a note with, plus bare URLs as links. The two walk the same
/// tree: what the index found in a note is what this renders.
fn options() -> Options<'static> {
    let mut o = Options::default();
    let e = &mut o.extension;
    e.strikethrough = true;
    e.table = true;
    e.tasklist = true;
    e.footnotes = true;
    e.inline_footnotes = true;
    e.math_dollars = true;
    e.wikilinks_title_after_pipe = true;
    e.highlight = true;
    e.ofm_callouts = true;
    e.ofm_comments = true;
    e.autolink = true;
    e.front_matter_delimiter = Some("---".into());
    o.parse.relaxed_tasklist_matching = true;
    o.parse.leave_footnote_definitions = true;
    o
}

static FIRST_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*</?([A-Za-z][A-Za-z0-9-]*)").unwrap());

/// A footnote the text cites, in the order it is first cited.
pub(super) struct Cited {
    pub(super) name: String,
    /// The line of its first citation.
    pub(super) line: usize,
    pub(super) count: usize,
}

#[derive(Default)]
pub(super) struct Notes {
    pub(super) cited: Vec<Cited>,
}

/// One walk over a note's tree.
pub(super) struct Walk<'d> {
    pub(super) doc: &'d Doc<'d>,
    pub(super) notes: Notes,
    /// The first line of the top-level block being written: `data-line` counts from it.
    origin: usize,
    /// A blockquote a list passes down to the last item that has no list at its end.
    tail: Option<String>,
}

/// Where a block sits decides which attributes Obsidian gives it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Top,
    Quote,
    Inner,
    Footnote,
}

/// The blocks of `doc`, then the footnotes its text cites.
pub(super) fn blocks(doc: &Doc) -> String {
    let arena = Arena::new();
    let root = parse_document(&arena, doc.source, &options());
    let mut walk = Walk {
        doc,
        notes: Notes::default(),
        origin: 0,
        tail: None,
    };
    let last = doc.offsets.line_of(doc.offsets.len()) as usize;
    let (mut out, mut covered) = (Vec::new(), 0);
    // comrak reads `---` twice in a row as two rules; Obsidian has an empty properties block there.
    let empty_properties = empty_frontmatter(doc.source)
        && !root
            .first_child()
            .is_some_and(|n| matches!(n.data().value, NodeValue::FrontMatter(_)));
    if empty_properties {
        out.push(format!(
            r#"<div class="el-pre mod-frontmatter mod-ui" data-sourcepos="1:1-2:3">{}</div>"#,
            frontmatter("---\n---")
        ));
        covered = 2;
    }
    for node in root.children() {
        let pos = node.data().sourcepos;
        let end = last_line(&pos);
        if empty_properties
            && pos.start.line <= 2
            && matches!(node.data().value, NodeValue::ThematicBreak)
        {
            continue;
        }
        if absorbed(node) {
            covered = covered.max(end);
            continue;
        }
        walk.definitions(&mut out, covered + 1, pos.start.line.saturating_sub(1));
        walk.origin = pos.start.line;
        let block = walk.top(node);
        if !block.is_empty() {
            out.push(block);
        }
        covered = covered.max(end);
    }
    walk.definitions(&mut out, covered + 1, last);
    let footnotes = walk.footnotes(root);
    if !footnotes.is_empty() {
        out.push(footnotes);
    }
    out.join("\n")
}

impl<'d> Walk<'d> {
    /// Text comrak drops between blocks is a link reference definition (`[ref]: url`); Obsidian
    /// renders each run of them as an empty block.
    fn definitions(&self, out: &mut Vec<String>, from: usize, to: usize) {
        let offsets = &self.doc.offsets;
        let mut open = false;
        for line in from..=to {
            let (a, b) = (
                offsets.line_start(line as u32),
                offsets.line_start(line as u32 + 1),
            );
            let blank = self
                .doc
                .source
                .get(a..b)
                .is_none_or(|t| t.trim().is_empty());
            if blank {
                open = false;
            } else if !std::mem::replace(&mut open, true) {
                out.push("<div></div>".to_string());
            }
        }
    }

    /// A top-level block in its `div.el-*` wrapper.
    fn top<'a>(&mut self, node: &'a AstNode<'a>) -> String {
        let (class, extra) = {
            let ast = node.data();
            match &ast.value {
                NodeValue::FrontMatter(_) => ("el-pre mod-frontmatter mod-ui".to_string(), ""),
                NodeValue::Paragraph if display_math(node).is_some() => ("el-div".into(), ""),
                NodeValue::Paragraph => ("el-p".into(), ""),
                NodeValue::Heading(h) => (format!("el-h{}", h.level), ""),
                NodeValue::List(l) if l.list_type == ListType::Ordered => ("el-ol".into(), ""),
                NodeValue::List(_) => ("el-ul".into(), ""),
                NodeValue::BlockQuote => ("el-blockquote".into(), ""),
                NodeValue::Callout(_) => ("el-div".into(), ""),
                NodeValue::CodeBlock(_) => ("el-pre".into(), ""),
                NodeValue::ThematicBreak => ("el-hr".into(), ""),
                NodeValue::Table(_) => {
                    ("el-table".into(), r#" dir="ltr" style="overflow-x: auto;""#)
                }
                NodeValue::HtmlBlock(h) => {
                    let tag = FIRST_TAG
                        .captures(&h.literal)
                        .map_or("div".to_string(), |c| c[1].to_ascii_lowercase());
                    (format!("el-{tag}"), "")
                }
                // Nothing to draw, but Obsidian keeps the block's place.
                NodeValue::FootnoteDefinition(d) if d.name.starts_with("__inline_") => {
                    return String::new();
                }
                NodeValue::FootnoteDefinition(_) | NodeValue::MultilineComment(_) => {
                    return "<div></div>".to_string();
                }
                _ => return String::new(),
            }
        };
        let inner = self.block(node, Place::Top);
        if inner.is_empty() {
            return String::new();
        }
        let inner = html::seal(inner, self.doc.path, self.doc.cx.index.names());
        // Raw HTML that is removed whole (a script, a comment) leaves an empty block.
        if inner.trim().is_empty() {
            return "<div></div>".to_string();
        }
        let mut pos = node.data().sourcepos;
        if let Some(quote) = follower(node) {
            pos.end = quote.data().sourcepos.end;
        }
        let sourcepos = if self.doc.depth == 0 {
            format!(
                r#" data-sourcepos="{}:{}-{}:{}""#,
                pos.start.line, pos.start.column, pos.end.line, pos.end.column
            )
        } else {
            String::new()
        };
        format!(r#"<div class="{class}"{extra}{sourcepos}>{inner}</div>"#)
    }

    /// A block's own element, without a wrapper.
    fn block<'a>(&mut self, node: &'a AstNode<'a>, place: Place) -> String {
        let ast = node.data();
        match &ast.value {
            NodeValue::FrontMatter(yaml) => frontmatter(yaml),
            NodeValue::Paragraph => self.paragraph(node, place),
            NodeValue::Heading(h) => self.heading(node, h.level),
            NodeValue::List(l) => self.list(node, l, place),
            NodeValue::BlockQuote => format!(
                "<blockquote dir=\"auto\">\n{}\n</blockquote>",
                self.children(node, Place::Quote)
            ),
            NodeValue::Callout(c) => self.callout(node, c),
            NodeValue::CodeBlock(c) => self.code(ast.sourcepos.start.line, c),
            NodeValue::ThematicBreak => "<hr>".to_string(),
            NodeValue::Table(t) => self.table(node, &t.alignments, place),
            NodeValue::HtmlBlock(h) => html::raw(&h.literal),
            _ => String::new(),
        }
    }

    fn children<'a>(&mut self, node: &'a AstNode<'a>, place: Place) -> String {
        let mut parts = Vec::new();
        for child in node.children().filter(|c| !absorbed(c)) {
            let part = self.block(child, place);
            if !part.is_empty() {
                parts.push(part);
            }
        }
        parts.join("\n")
    }

    fn paragraph<'a>(&mut self, node: &'a AstNode<'a>, place: Place) -> String {
        if let Some(latex) = display_math(node) {
            return format!(
                r#"<div class="math math-block">{}</div>"#,
                html::text(&latex)
            );
        }
        if only_block_id(node) {
            return String::new();
        }
        let dir = if matches!(place, Place::Top | Place::Inner) {
            r#" dir="auto""#
        } else {
            ""
        };
        format!("<p{dir}>{}</p>", self.inlines(node, true))
    }

    fn heading<'a>(&mut self, node: &'a AstNode<'a>, level: u8) -> String {
        let source = match (node.first_child(), node.last_child()) {
            (Some(first), Some(last)) => {
                let (start, end) = (first.data().sourcepos.start, last.data().sourcepos.end);
                self.doc
                    .offsets
                    .sourcepos((start.line, start.column), (end.line, end.column))
                    .slice(self.doc.source)
                    .to_string()
            }
            _ => String::new(),
        };
        format!(
            r#"<h{level} data-heading="{}" dir="auto"><span class="heading-collapse-indicator collapse-indicator collapse-icon">{}</span>{}</h{level}>"#,
            html::attr(&source),
            icon("right-triangle"),
            self.inlines(node, false),
        )
    }

    fn list<'a>(&mut self, node: &'a AstNode<'a>, list: &NodeList, place: Place) -> String {
        let ordered = list.list_type == ListType::Ordered;
        let tasks = node.children().any(|item| task_of(item).is_some());
        let tag = if ordered { "ol" } else { "ul" };
        let mut class = Vec::new();
        if tasks {
            class.push("contains-task-list");
        }
        if !ordered {
            class.push("has-list-bullet");
        }
        let class = if class.is_empty() {
            String::new()
        } else {
            format!(r#" class="{}""#, class.join(" "))
        };
        let start = if ordered && list.start != 1 {
            format!(r#" start="{}""#, list.start)
        } else {
            String::new()
        };
        let mut out = format!("<{tag}{class}{start}>\n");
        // Obsidian takes a quote on the line after the list for part of the last item.
        let mut tail = self
            .tail
            .take()
            .or_else(|| follower(node).map(|quote| self.block(quote, Place::Quote)));
        let last = node.last_child();
        for item in node.children() {
            let tail = if last.is_some_and(|l| std::ptr::eq(l, item)) {
                tail.take()
            } else {
                None
            };
            out.push_str(&self.item(item, list, place, tail));
            out.push('\n');
        }
        out.push_str(&format!("</{tag}>"));
        out
    }

    fn item<'a>(
        &mut self,
        node: &'a AstNode<'a>,
        list: &NodeList,
        place: Place,
        mut tail: Option<String>,
    ) -> String {
        let ordered = list.list_type == ListType::Ordered;
        let line = node.data().sourcepos.start.line.saturating_sub(self.origin);
        let (task, plain_symbol) = match task_of(node) {
            Some(symbol) => (Some(symbol), None),
            None => (None, odd_symbol(node)),
        };
        let nested = node
            .children()
            .any(|child| matches!(child.data().value, NodeValue::List(_)));
        let mut attrs = format!(r#" data-line="{line}""#);
        if let Some(symbol) = task {
            let done = if symbol == ' ' { "" } else { " is-checked" };
            attrs.push_str(&format!(
                r#" data-task="{}" class="task-list-item{done}""#,
                html::attr(&if symbol == ' ' {
                    String::new()
                } else {
                    symbol.to_string()
                })
            ));
        }
        attrs.push_str(r#" dir="auto""#);
        let mut out = format!("<li{attrs}>");
        if !ordered {
            out.push_str(r#"<span class="list-bullet"></span>"#);
        }
        if nested {
            out.push_str(&format!(
                r#"<span class="list-collapse-indicator collapse-indicator collapse-icon">{}</span>"#,
                icon("right-triangle")
            ));
        }
        if let Some(symbol) = task {
            let checked = if symbol == ' ' { "" } else { r#" checked="""# };
            // An item with nothing after its box has a line break before it.
            if node.first_child().is_none() {
                out.push('\n');
            }
            out.push_str(&format!(
                r#"<input data-line="{line}"{checked} type="checkbox" class="task-list-item-checkbox">"#
            ));
        }
        // Obsidian takes only a one-character symbol for a task: `[😀]` is text.
        if let Some(symbol) = plain_symbol {
            out.push_str(&html::text(&format!("[{symbol}] ")));
        }
        let mut first = true;
        for child in node.children() {
            let is_paragraph = matches!(child.data().value, NodeValue::Paragraph);
            if !first {
                out.push('\n');
            }
            first = false;
            // The quote joins the deepest last item: hand it to a list that ends this one.
            if node.last_child().is_some_and(|l| std::ptr::eq(l, child))
                && matches!(child.data().value, NodeValue::List(_))
            {
                self.tail = tail.take();
            }
            if is_paragraph && list_is_tight(node) {
                out.push_str(&self.inlines(child, true));
            } else {
                out.push_str(&self.block(child, place));
            }
        }
        if let Some(tail) = tail {
            out.push('\n');
            out.push_str(&tail);
        }
        out.push_str("</li>");
        out
    }

    fn callout<'a>(&mut self, node: &'a AstNode<'a>, c: &NodeCallout) -> String {
        let kind = c.callout_type.to_lowercase();
        let (fold, foldable, collapsed) = match c.fold {
            CalloutFold::None => ("", false, false),
            CalloutFold::Expanded => ("+", true, false),
            CalloutFold::Collapsed => ("-", true, true),
        };
        let mut kids = node.children();
        let titled = node
            .first_child()
            .is_some_and(|f| matches!(f.data().value, NodeValue::CalloutTitle));
        let custom = if titled {
            kids.next()
                .filter(|t| t.first_child().is_some())
                .map(|t| self.inlines(t, false))
        } else {
            None
        };
        let title = custom.unwrap_or_else(|| html::text(&c.default_title()));
        let mut body = Vec::new();
        for child in kids.filter(|c| !absorbed(c)) {
            let part = self.block(child, Place::Inner);
            if !part.is_empty() {
                body.push(part);
            }
        }
        let class = format!(
            "callout{}{}",
            if foldable { " is-collapsible" } else { "" },
            if collapsed { " is-collapsed" } else { "" }
        );
        let fold_icon = if foldable {
            format!(
                r#"<div class="callout-fold{}">{}</div>"#,
                if collapsed { " is-collapsed" } else { "" },
                icon("lucide-chevron-down")
            )
        } else {
            String::new()
        };
        let content = if body.is_empty() {
            String::new()
        } else {
            format!(
                r#"<div class="callout-content"{}>{}</div>"#,
                if collapsed {
                    r#" style="display: none;""#
                } else {
                    ""
                },
                body.join("\n")
            )
        };
        format!(
            r#"<div data-callout-metadata="{}" data-callout-fold="{fold}" data-callout="{}" class="{class}"><div class="callout-title" dir="auto"><div class="callout-icon">{}</div><div class="callout-title-inner">{title}</div>{fold_icon}</div>{content}</div>"#,
            html::attr(c.metadata.as_deref().unwrap_or("")),
            html::attr(&kind),
            icon(callout_icon(&kind)),
        )
    }

    fn code(&self, line: usize, c: &NodeCodeBlock) -> String {
        // A `base` block is a Bases view; the page fills the container from its YAML.
        if c.info.split_whitespace().next() == Some("base") {
            return format!(
                r#"<div class="block-language-base bases-embed interactive-child" data-frame-source="{}"></div>"#,
                html::attr(&c.literal)
            );
        }
        let class = c
            .info
            .split_whitespace()
            .next()
            .map(|lang| format!(r#" class="language-{}""#, html::attr(lang)))
            .unwrap_or_default();
        format!(
            r#"<pre{class}><code data-line="{}"{class}>{}</code></pre>"#,
            line.saturating_sub(self.origin),
            html::text(&c.literal)
        )
    }

    fn table<'a>(
        &mut self,
        node: &'a AstNode<'a>,
        alignments: &[TableAlignment],
        place: Place,
    ) -> String {
        let (mut head, mut body) = (String::new(), String::new());
        for row in node.children() {
            let header = matches!(row.data().value, NodeValue::TableRow(true));
            let tag = if header { "th" } else { "td" };
            let mut cells = String::new();
            for (i, cell) in row.children().enumerate() {
                let align = match alignments.get(i) {
                    Some(TableAlignment::Left) => r#" align="left""#,
                    Some(TableAlignment::Center) => r#" align="center""#,
                    Some(TableAlignment::Right) => r#" align="right""#,
                    _ => "",
                };
                // Only a table on the page sets a direction per cell, by its first strong character.
                let dir = if place == Place::Top {
                    format!(r#" dir="{}""#, inlines::direction(&self.plain(cell)))
                } else {
                    String::new()
                };
                cells.push_str(&format!(
                    "<{tag}{align}{dir}>{}</{tag}>\n",
                    self.inlines(cell, false)
                ));
            }
            let row = format!("<tr>\n{cells}</tr>\n");
            if header {
                head.push_str(&row);
            } else {
                body.push_str(&row);
            }
        }
        let body = if body.is_empty() {
            String::new()
        } else {
            format!("<tbody>\n{body}</tbody>\n")
        };
        format!("<table>\n<thead>\n{head}</thead>\n{body}</table>")
    }

    /// The footnotes the text cites, in Obsidian's `section.footnotes`.
    fn footnotes<'a>(&mut self, root: &'a AstNode<'a>) -> String {
        if self.notes.cited.is_empty() {
            return String::new();
        }
        let salt = self.doc.salt();
        // `data-line` counts from the last line of the note, so it is negative.
        let last = self.doc.offsets.line_of(self.doc.offsets.len()) as i64 - 1;
        let cited = std::mem::take(&mut self.notes.cited);
        let mut items = String::new();
        for (at, c) in cited.iter().enumerate() {
            let number = at + 1;
            let definition = root.descendants().find(
                |n| matches!(&n.data().value, NodeValue::FootnoteDefinition(d) if d.name == c.name),
            );
            let inline = c.name.starts_with("__inline_");
            let line = match definition {
                Some(d) if !inline => d.data().sourcepos.start.line,
                _ => c.line,
            };
            let backrefs: String = (0..c.count)
                .map(|k| {
                    let shown = if k == 0 {
                        number.to_string()
                    } else {
                        format!("{number}-{k}")
                    };
                    format!(
                        "<a href=\"#fnref-{shown}-{salt}\" class=\"footnote-backref footnote-link\" target=\"_blank\" rel=\"noopener nofollow\">\u{21a9}\u{fe0e}</a>"
                    )
                })
                .collect();
            let body = match definition {
                Some(d) if inline => {
                    let text = d
                        .first_child()
                        .map_or(String::new(), |p| self.inlines(p, false));
                    format!("{text}{backrefs}")
                }
                Some(d) => {
                    let body = self.children(d, Place::Footnote);
                    match body.rfind("</p>") {
                        Some(end) => format!("{}{backrefs}{}", &body[..end], &body[end..]),
                        None => format!("{body}{backrefs}"),
                    }
                }
                None => backrefs,
            };
            items.push_str(&format!(
                r#"<li data-line="{}" data-footnote-id="fn-{number}-{salt}" id="fn-{number}-{salt}" dir="auto">{body}</li>"#,
                line as i64 - 1 - last,
            ));
            items.push('\n');
        }
        let items = html::seal(items, self.doc.path, self.doc.cx.index.names());
        format!(
            "<div>\n</div>\n<div class=\"el-section\"><section class=\"footnotes\"><hr><ol>\n{items}</ol></section></div>"
        )
    }
}

/// The last line a block covers: comrak ends one that swallowed its final newline at column 0 of
/// the next line.
fn last_line(pos: &Sourcepos) -> usize {
    if pos.end.column == 0 && pos.end.line > pos.start.line {
        pos.end.line - 1
    } else {
        pos.end.line
    }
}

/// The blockquote on the line right after a list. Obsidian reads it as part of the list's last
/// item, as it does any unindented line that does not start a new block of its own kind.
fn follower<'a>(list: &'a AstNode<'a>) -> Option<&'a AstNode<'a>> {
    if !matches!(list.data().value, NodeValue::List(_)) {
        return None;
    }
    let next = list.next_sibling()?;
    let quote = matches!(next.data().value, NodeValue::BlockQuote);
    (quote && next.data().sourcepos.start.line == last_line(&list.data().sourcepos) + 1)
        .then_some(next)
}

/// Whether a list above has taken this blockquote into its last item.
fn absorbed<'a>(node: &'a AstNode<'a>) -> bool {
    matches!(node.data().value, NodeValue::BlockQuote)
        && node
            .previous_sibling()
            .and_then(follower)
            .is_some_and(|quote| std::ptr::eq(quote, node))
}

/// A note whose first two lines are `---`: an empty properties block.
fn empty_frontmatter(source: &str) -> bool {
    let mut lines = source.lines();
    matches!((lines.next(), lines.next()), (Some(a), Some(b)) if a.trim_end() == "---" && b.trim_end() == "---")
}

/// The task a list item is, by its status character (a space when unchecked). Obsidian takes a
/// symbol of one UTF-16 unit only.
fn task_of<'a>(item: &'a AstNode<'a>) -> Option<char> {
    match &item.data().value {
        NodeValue::TaskItem(t) => match t.symbol {
            None => Some(' '),
            Some(symbol) if symbol.len_utf16() == 1 => Some(symbol),
            Some(_) => None,
        },
        _ => None,
    }
}

/// The symbol of a bracket pair comrak took for a task that Obsidian leaves as text.
fn odd_symbol<'a>(item: &'a AstNode<'a>) -> Option<char> {
    match &item.data().value {
        NodeValue::TaskItem(t) => t.symbol.filter(|s| s.len_utf16() != 1),
        _ => None,
    }
}

fn list_is_tight<'a>(item: &'a AstNode<'a>) -> bool {
    item.parent()
        .is_some_and(|list| matches!(&list.data().value, NodeValue::List(l) if l.tight))
}

/// The LaTeX of a paragraph that is `$$…$$` and nothing else.
fn display_math<'a>(paragraph: &'a AstNode<'a>) -> Option<String> {
    let mut kids = paragraph.children();
    let only = kids.next()?;
    if kids.next().is_some() {
        return None;
    }
    match &only.data().value {
        NodeValue::Math(m) if m.display_math => Some(m.literal.trim().to_string()),
        _ => None,
    }
}

/// A paragraph that is a `^block-id` alone names the block before it and shows nothing.
fn only_block_id<'a>(paragraph: &'a AstNode<'a>) -> bool {
    let mut kids = paragraph.children();
    let Some(only) = kids.next() else {
        return false;
    };
    kids.next().is_none()
        && matches!(&only.data().value, NodeValue::Text(t) if inlines::is_block_id(t))
}

/// The properties block, which Obsidian renders hidden for scripts and snippets to find.
fn frontmatter(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().skip(1).collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if lines.last().is_some_and(|l| l.trim_end() == "---") {
        lines.pop();
    }
    format!(
        r#"<pre class="frontmatter language-yaml" style="display: none;"><code class="language-yaml">{}</code></pre>"#,
        html::text(&lines.join("\n"))
    )
}

/// The Lucide icon Obsidian draws for a callout of `kind` when no stylesheet
/// names another; its stylesheet names the same one for each type.
pub fn callout_icon(kind: &str) -> &'static str {
    match kind {
        "abstract" | "summary" | "tldr" => "lucide-clipboard-list",
        "info" => "lucide-info",
        "todo" => "lucide-check-circle-2",
        "tip" | "hint" | "important" => "lucide-flame",
        "success" | "check" | "done" => "lucide-check",
        "question" | "help" | "faq" => "lucide-help-circle",
        "warning" | "caution" | "attention" => "lucide-alert-triangle",
        "failure" | "fail" | "missing" => "lucide-x",
        "danger" | "error" => "lucide-zap",
        "bug" => "lucide-bug",
        "example" => "lucide-list",
        "quote" | "cite" => "lucide-quote",
        _ => "lucide-pencil",
    }
}
