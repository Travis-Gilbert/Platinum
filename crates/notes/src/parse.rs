//! Parsing a note: comrak for the tree, then the passes it lacks (tags, block ids, highlight colours, inline fields).
use crate::frontmatter;
use crate::model::*;
use crate::offsets::{Offsets, Span};
use comrak::nodes::{AstNode, CalloutFold, NodeLink, NodeValue};
use comrak::{Arena, Options, parse_document};
use percent_encoding::percent_decode_str;
use regex::Regex;
use std::ops::Range;
use std::sync::LazyLock;

// Obsidian's tags: letters, marks, digits, `_`, `-` and `/`, starting at a word boundary, with at
// least one character that is not a digit. Theorem-UI's PR 16 settled the class.
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#([\p{L}\p{M}\p{N}_/-]+)").unwrap());
static BLOCK_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)\^([A-Za-z0-9-]+)\s*$").unwrap());
static FRONTMATTER_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(!?)\[\[([^\[\]\r\n]+)\]\]").unwrap());
static FRONTMATTER_TAGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^tags?[ \t]*:").unwrap());
static EXTERNAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i:[a-z][a-z0-9+.-]*:|//)").unwrap());
const FIELD_KEY: &str =
    r"[\p{L}\p{N}\p{Emoji_Presentation}_][\p{L}\p{N}\p{Emoji_Presentation}_ /-]*?";
static FIELD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^({FIELD_KEY})[ \t]*::[ \t]*(.*?)[ \t]*$")).unwrap());

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
    e.front_matter_delimiter = Some("---".into());
    o.parse.relaxed_tasklist_matching = true;
    o
}

/// Parses one Markdown note. `%%comments%%`, code and math contribute nothing.
pub fn parse(text: &str) -> Note {
    let arena = Arena::new();
    let root = parse_document(&arena, text, &options());
    let mut cx = Cx {
        text,
        o: Offsets::new(text),
        note: Note::default(),
    };
    for node in root.descendants() {
        if !in_inline_footnote_definition(node) {
            cx.visit(node);
        }
    }
    cx.note
}

// comrak gives the definition of `^[inline]` a position in the first line, so nothing in one is read.
fn in_inline_footnote_definition<'a>(node: &'a AstNode<'a>) -> bool {
    node.ancestors()
        .any(|a| matches!(&a.data().value, NodeValue::FootnoteDefinition(f) if is_inline(&f.name)))
}

fn is_inline(footnote_name: &str) -> bool {
    footnote_name.starts_with("__inline_")
}

struct Cx<'t> {
    text: &'t str,
    o: Offsets,
    note: Note,
}

impl<'t> Cx<'t> {
    fn visit<'a>(&mut self, node: &'a AstNode<'a>) {
        match &node.data().value {
            NodeValue::FrontMatter(literal) => self.frontmatter(node, literal),
            NodeValue::Heading(h) => {
                let (text, span) = (self.content_text(node), self.span(node));
                self.note.headings.push(Heading {
                    level: h.level,
                    text,
                    span,
                });
            }
            NodeValue::Paragraph => self.paragraph(node),
            NodeValue::Text(_) => self.text(node),
            NodeValue::WikiLink(w) => self.wikilink(node, w.embed),
            NodeValue::Link(l) => self.markdown_link(node, l, false),
            NodeValue::Image(l) => self.markdown_link(node, l, true),
            NodeValue::TaskItem(t) => {
                let status = t.symbol.unwrap_or(' ');
                let at = t.symbol_sourcepos.start;
                let start = self
                    .o
                    .sourcepos((at.line, at.column), (at.line, at.column))
                    .start;
                let text = node
                    .first_child()
                    .map(|p| self.slice(self.span(p)).trim().to_string())
                    .unwrap_or_default();
                self.note.tasks.push(Task {
                    status,
                    text,
                    span: self.span(node),
                    status_span: self.o.span(start, start + status.len_utf8()),
                });
            }
            NodeValue::FootnoteReference(f) => {
                let span = self.span(node);
                let kind = if is_inline(&f.name) {
                    FootnoteKind::Inline
                } else {
                    FootnoteKind::Reference
                };
                if kind == FootnoteKind::Inline {
                    // The text of `^[text]` exists only in the definition comrak positions wrongly.
                    let inner = Span {
                        start: span.start + 2,
                        end: span.end.saturating_sub(1).max(span.start + 2),
                        ..span
                    };
                    self.tags_in(inner);
                }
                self.note.footnotes.push(Footnote {
                    name: f.name.clone(),
                    kind,
                    span,
                });
            }
            NodeValue::FootnoteDefinition(f) => {
                let span = self.span(node);
                self.note.footnotes.push(Footnote {
                    name: f.name.clone(),
                    kind: FootnoteKind::Definition,
                    span,
                });
            }
            NodeValue::Callout(c) => {
                let title = node
                    .children()
                    .find(|c| matches!(c.data().value, NodeValue::CalloutTitle))
                    .map(|t| self.slice(self.span(t)).trim().to_string())
                    .filter(|t| !t.is_empty());
                let fold = match c.fold {
                    CalloutFold::None => Fold::None,
                    CalloutFold::Expanded => Fold::Expanded,
                    CalloutFold::Collapsed => Fold::Collapsed,
                };
                self.note.callouts.push(Callout {
                    kind: c.callout_type.clone(),
                    title,
                    fold,
                    metadata: c.metadata.clone(),
                    span: self.span(node),
                });
            }
            NodeValue::Highlight => {
                let span = self.span(node);
                let raw = self.slice(span);
                let text = raw
                    .strip_prefix("==")
                    .and_then(|r| r.strip_suffix("=="))
                    .unwrap_or(raw)
                    .to_string();
                let color = text.chars().next().and_then(HighlightColor::from_emoji);
                self.note.highlights.push(Highlight { text, color, span });
            }
            _ => {}
        }
    }

    fn slice(&self, span: Span) -> &'t str {
        span.slice(self.text)
    }

    /// The node's source span, without the newline a block's position may end on.
    fn span<'a>(&self, node: &'a AstNode<'a>) -> Span {
        let p = node.data().sourcepos;
        let mut s = self
            .o
            .sourcepos((p.start.line, p.start.column), (p.end.line, p.end.column));
        while s.end > s.start && matches!(self.text.as_bytes()[s.end - 1], b'\n' | b'\r') {
            s.end -= 1;
        }
        s
    }

    /// From the start of the first child to the end of the last.
    fn content_text<'a>(&self, node: &'a AstNode<'a>) -> String {
        let (Some(first), Some(last)) = (node.first_child(), node.last_child()) else {
            return String::new();
        };
        let (a, b) = (self.span(first), self.span(last));
        self.slice(self.o.span(a.start, b.end)).trim().to_string()
    }

    fn frontmatter<'a>(&mut self, node: &'a AstNode<'a>, literal: &str) {
        let span = self.span(node);
        let (properties, error) = match frontmatter::parse_yaml(frontmatter::body(literal)) {
            Ok(properties) => (properties, None),
            Err(error) => (Default::default(), Some(error)),
        };
        let fm = Frontmatter {
            span,
            properties,
            error,
        };

        let region = self.slice(span);
        for m in FRONTMATTER_LINK.captures_iter(region) {
            let (whole, inner) = (m.get(0).unwrap(), m.get(2).unwrap());
            let (start, open) = (span.start + whole.start(), span.start + inner.start() - 2);
            self.wiki_link(
                start,
                open,
                span.start + inner.end(),
                !m[1].is_empty(),
                true,
            );
        }
        let mut cursor = FRONTMATTER_TAGS.find(region).map_or(0, |m| m.end());
        for name in fm.tags() {
            let found = region[cursor..].find(&name).map(|i| cursor + i);
            let at = found.map_or(span, |i| {
                self.o.span(span.start + i, span.start + i + name.len())
            });
            cursor = found.map_or(cursor, |i| i + name.len());
            self.note.tags.push(Tag {
                name,
                source: TagSource::Frontmatter,
                span: at,
            });
        }
        self.note.frontmatter = Some(fm);
    }

    fn wikilink<'a>(&mut self, node: &'a AstNode<'a>, embed: bool) {
        // comrak's start is right, but its end is not inside a table row that holds `\|`; the
        // brackets are found in the source instead.
        let start = self.text.floor_char_boundary(self.span(node).start);
        let line_end = self.o.line_start(self.o.line_of(start) + 1);
        let Some(open) = self.text[start..line_end.max(start)]
            .find("[[")
            .map(|i| start + i)
        else {
            return;
        };
        let Some(close) = self.text[open + 2..].find("]]").map(|i| open + 2 + i) else {
            return;
        };
        let from = if embed && open > 0 && self.text.as_bytes()[open - 1] == b'!' {
            open - 1
        } else {
            open
        };
        self.wiki_link(from, open, close, embed, false);
    }

    /// `[[…]]` with its brackets at `open` and `close` (`from` is where an embed's `!` is).
    fn wiki_link(
        &mut self,
        from: usize,
        open: usize,
        close: usize,
        embed: bool,
        in_frontmatter: bool,
    ) {
        let inner = &self.text[open + 2..close];
        let parts = wiki_parts(inner);
        if parts.target.is_empty() && parts.fragment.is_none() {
            return;
        }
        let target_span = parts
            .range
            .map(|r| self.o.span(open + 2 + r.start, open + 2 + r.end));
        self.note.links.push(Link {
            syntax: LinkSyntax::Wiki,
            embed,
            target: parts.target,
            fragment: parts.fragment,
            text: parts.alias,
            span: self.o.span(from, close + 2),
            target_span,
            in_frontmatter,
        });
    }

    fn markdown_link<'a>(&mut self, node: &'a AstNode<'a>, link: &NodeLink, embed: bool) {
        if EXTERNAL.is_match(&link.url) {
            return;
        }
        let (path, fragment) = split_fragment(&link.url);
        let target = decode(path);
        let fragment = fragment.map(decode).and_then(fragment_of);
        if target.is_empty() && fragment.is_none() {
            return;
        }
        let span = self.span(node);
        let text = Some(self.content_text(node)).filter(|t| !t.is_empty());
        // Where the destination starts, when it is written as comrak reports it.
        let target_span = self.slice(span).rfind("](").and_then(|i| {
            let after = span.start + i + 2;
            let rest = &self.text[after..];
            let skipped = rest.len() - rest.trim_start().len()
                + usize::from(rest.trim_start().starts_with('<'));
            let at = after + skipped;
            (!path.is_empty() && self.text[at..].starts_with(path))
                .then(|| self.o.span(at, at + path.len()))
        });
        self.note.links.push(Link {
            syntax: LinkSyntax::Markdown,
            embed,
            target,
            fragment,
            text,
            span,
            target_span,
            in_frontmatter: false,
        });
    }

    fn text<'a>(&mut self, node: &'a AstNode<'a>) {
        // The words of `[[a|alias]]` and `![alt](x)` are not text of the note.
        if node
            .ancestors()
            .skip(1)
            .any(|a| matches!(a.data().value, NodeValue::WikiLink(_) | NodeValue::Image(_)))
        {
            return;
        }
        let span = self.span(node);
        self.tags_in(span);
    }

    /// Tags written in the source text of `span`. Scanning the source rather than the node's value
    /// keeps `\#escaped` out.
    fn tags_in(&mut self, span: Span) {
        let slice = self.slice(span);
        for m in TAG.captures_iter(slice) {
            let at = m.get(0).unwrap().start();
            let boundary = slice[..at]
                .chars()
                .next_back()
                .is_none_or(|c| c.is_whitespace() || "([{,;".contains(c));
            let name = m[1].trim_end_matches('/');
            if !boundary || name.is_empty() || name.chars().all(|c| c.is_ascii_digit() || c == '/')
            {
                continue;
            }
            let wanted = format!("#{name}");
            let start = self.anchor(span.line, &wanted, span.start + at);
            self.note.tags.push(Tag {
                name: name.to_string(),
                source: TagSource::Body,
                span: self.o.span(start, start + wanted.len()),
            });
        }
    }

    // A table row holding `\|` shifts every position after it left; `wanted` is searched for in the
    // line when it is not where `hint` says.
    fn anchor(&self, line: u32, wanted: &str, hint: usize) -> usize {
        if self.text.get(hint..).is_some_and(|t| t.starts_with(wanted)) {
            return hint;
        }
        let (from, to) = (self.o.line_start(line), self.o.line_start(line + 1));
        self.text[from..to].find(wanted).map_or(hint, |i| from + i)
    }

    fn paragraph<'a>(&mut self, node: &'a AstNode<'a>) {
        let span = self.span(node);
        let slice = self.slice(span);
        if let Some(id) = BLOCK_ID.captures(slice).and_then(|c| c.get(1)) {
            let id_span = self
                .o
                .span(span.start + id.start() - 1, span.start + id.end());
            let alone = slice.trim() == &slice[id.start() - 1..id.end()];
            let target = if alone {
                node.previous_sibling().map(|p| self.span(p))
            } else {
                // `- item ^id` names the item with its children.
                node.parent()
                    .filter(|p| {
                        matches!(p.data().value, NodeValue::Item(_) | NodeValue::TaskItem(_))
                    })
                    .filter(|p| p.first_child().is_some_and(|c| std::ptr::eq(c, node)))
                    .map(|p| self.span(p))
            };
            self.note.blocks.push(Block {
                id: id.as_str().to_string(),
                span: target.unwrap_or(span),
                id_span,
            });
        }
        self.fields(span);
    }

    /// Dataview inline fields in the lines of a paragraph.
    fn fields(&mut self, paragraph: Span) {
        let mut offset = paragraph.start;
        for raw in self.slice(paragraph).split_inclusive(['\n', '\r']) {
            let line_start = offset;
            offset += raw.len();
            let line = raw.trim_end_matches(['\n', '\r']);
            // Continuation lines of a quote start with `>`.
            let content = line.trim_start_matches(|c: char| c.is_whitespace() || c == '>');
            let base = line_start + line.len() - content.len();
            if let Some(c) = FIELD.captures(content) {
                let value = c.get(2).unwrap().as_str().to_string();
                let span = self.o.span(base, base + content.trim_end().len());
                self.note.fields.push(InlineField {
                    key: c[1].trim().to_string(),
                    value,
                    style: FieldStyle::Line,
                    span,
                });
                continue;
            }
            let mut at = 0;
            while let Some(i) = content[at..].find(['[', '(']).map(|i| at + i) {
                let end = closing(content, i);
                let inner = end.and_then(|end| FIELD.captures(&content[i + 1..end - 1]));
                match (end, inner) {
                    (Some(end), Some(c)) => {
                        let style = if content[i..].starts_with('[') {
                            FieldStyle::Bracket
                        } else {
                            FieldStyle::Paren
                        };
                        let value = c.get(2).unwrap().as_str().to_string();
                        let span = self.o.span(base + i, base + end);
                        self.note.fields.push(InlineField {
                            key: c[1].trim().to_string(),
                            value,
                            style,
                            span,
                        });
                        at = end;
                    }
                    _ => at = i + 1,
                }
            }
        }
    }
}

/// The index just past the bracket that closes the one at `open`.
fn closing(line: &str, open: usize) -> Option<usize> {
    let (o, c) = if line[open..].starts_with('[') {
        ('[', ']')
    } else {
        ('(', ')')
    };
    let mut depth = 0;
    for (i, ch) in line[open..].char_indices() {
        if ch == o {
            depth += 1;
        } else if ch == c {
            depth -= 1;
            if depth == 0 {
                return Some(open + i + ch.len_utf8());
            }
        }
    }
    None
}

struct WikiParts {
    target: String,
    /// Where the target is inside the brackets.
    range: Option<Range<usize>>,
    fragment: Option<Fragment>,
    alias: Option<String>,
}

/// The inside of `[[…]]`: `target#fragment|alias`.
fn wiki_parts(inner: &str) -> WikiParts {
    let (head, alias) = match inner.split_once('|') {
        Some((head, alias)) => (
            head,
            Some(alias.trim().to_string()).filter(|a| !a.is_empty()),
        ),
        None => (inner, None),
    };
    // A table cell escapes the divider as `\|`.
    let head = head.strip_suffix('\\').unwrap_or(head);
    let (path, fragment) = split_fragment(head);
    let target = path.trim();
    let lead = path.len() - path.trim_start().len();
    WikiParts {
        target: target.to_string(),
        range: (!target.is_empty()).then(|| lead..lead + target.len()),
        fragment: fragment.and_then(fragment_of),
        alias,
    }
}

/// Splits at the first `#`: the path, and what follows.
fn split_fragment(s: &str) -> (&str, Option<&str>) {
    s.split_once('#')
        .map_or((s, None), |(path, fragment)| (path, Some(fragment)))
}

pub(crate) fn fragment_of(s: impl AsRef<str>) -> Option<Fragment> {
    let s = s.as_ref().trim();
    match s.strip_prefix('^') {
        Some(block) if !block.is_empty() => Some(Fragment::Block(block.to_string())),
        _ if s.is_empty() => None,
        _ => Some(Fragment::Heading(s.to_string())),
    }
}

/// Percent-decoding that leaves a destination alone when it is not valid UTF-8.
fn decode(s: &str) -> String {
    percent_decode_str(s)
        .decode_utf8()
        .map_or_else(|_| s.to_string(), |d| d.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn link(note: &Note, i: usize) -> &Link {
        &note.links[i]
    }

    fn texts(src: &str, spans: impl IntoIterator<Item = Span>) -> Vec<&str> {
        spans.into_iter().map(|s| s.slice(src)).collect()
    }

    #[test]
    fn headings_atx_setext_and_formatted() {
        let src = "# One ##\n\n## Two **bold** and `code`\n\nThree\n=====\n\n###### Six\n";
        let n = parse(src);
        let got: Vec<_> = n
            .headings
            .iter()
            .map(|h| (h.level, h.text.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (1, "One"),
                (2, "Two **bold** and `code`"),
                (1, "Three"),
                (6, "Six")
            ]
        );
        assert_eq!(
            texts(src, n.headings.iter().map(|h| h.span)),
            [
                "# One ##",
                "## Two **bold** and `code`",
                "Three\n=====",
                "###### Six"
            ]
        );
        assert_eq!(n.headings[2].span.line, 5);
    }

    #[test]
    fn a_heading_holding_a_hash_keeps_it() {
        let n = parse("## C# basics\n");
        assert_eq!(n.headings[0].text, "C# basics");
    }

    #[test]
    fn wikilinks_with_every_part() {
        let src = "[[Note]] [[Note|alias]] [[Note#Head]] [[Note#H1#H2]] [[Note#^blk]] [[Note#Head|alias]] [[#Local]] [[#^local]] [[ Spaced ]] [[folder/Note.md]]";
        let n = parse(src);
        let got: Vec<_> = n
            .links
            .iter()
            .map(|l| (l.target.as_str(), l.fragment.clone(), l.text.as_deref()))
            .collect();
        assert_eq!(
            got,
            [
                ("Note", None, None),
                ("Note", None, Some("alias")),
                ("Note", Some(Fragment::Heading("Head".into())), None),
                ("Note", Some(Fragment::Heading("H1#H2".into())), None),
                ("Note", Some(Fragment::Block("blk".into())), None),
                (
                    "Note",
                    Some(Fragment::Heading("Head".into())),
                    Some("alias")
                ),
                ("", Some(Fragment::Heading("Local".into())), None),
                ("", Some(Fragment::Block("local".into())), None),
                ("Spaced", None, None),
                ("folder/Note.md", None, None),
            ]
        );
        assert!(
            n.links
                .iter()
                .all(|l| l.syntax == LinkSyntax::Wiki && !l.embed && !l.in_frontmatter)
        );
        assert_eq!(link(&n, 0).span.slice(src), "[[Note]]");
        assert_eq!(link(&n, 5).span.slice(src), "[[Note#Head|alias]]");
        assert_eq!(link(&n, 5).target_span.unwrap().slice(src), "Note");
        assert_eq!(link(&n, 8).target_span.unwrap().slice(src), "Spaced");
        assert_eq!(link(&n, 6).target_span, None);
    }

    #[test]
    fn a_heading_with_a_hash_in_a_link_is_kept_whole() {
        let n = parse("[[Plan#C# basics]]");
        assert_eq!(
            link(&n, 0).fragment,
            Some(Fragment::Heading("C# basics".into()))
        );
    }

    #[test]
    fn embeds_keep_their_size_or_caption() {
        let src =
            "![[img.png|100x200]] ![[Note#Head]] ![[Note#^b]] ![[doc.pdf#page=3]] ![[x.canvas]]";
        let n = parse(src);
        assert!(n.links.iter().all(|l| l.embed));
        assert_eq!(link(&n, 0).text.as_deref(), Some("100x200"));
        assert_eq!(link(&n, 0).span.slice(src), "![[img.png|100x200]]");
        assert_eq!(link(&n, 0).target_span.unwrap().slice(src), "img.png");
        assert_eq!(link(&n, 1).fragment, Some(Fragment::Heading("Head".into())));
        assert_eq!(link(&n, 2).fragment, Some(Fragment::Block("b".into())));
        assert_eq!(link(&n, 3).target, "doc.pdf");
        assert_eq!(
            link(&n, 3).fragment,
            Some(Fragment::Heading("page=3".into()))
        );
        assert_eq!(link(&n, 4).target, "x.canvas");
    }

    #[test]
    fn markdown_links_are_decoded_and_external_ones_dropped() {
        let src = "[a](Some%20Note.md#Head) ![i](img/a.png \"t\") [b](<with space.md>) [c](#Local) [d](../up/Note.md#^blk) [e](https://x.y/#a) <https://x.y> [f](mailto:a@b.c) [g]() [h](//cdn/x.png)";
        let n = parse(src);
        let got: Vec<_> = n
            .links
            .iter()
            .map(|l| {
                (
                    l.target.as_str(),
                    l.fragment.clone(),
                    l.embed,
                    l.text.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (
                    "Some Note.md",
                    Some(Fragment::Heading("Head".into())),
                    false,
                    Some("a")
                ),
                ("img/a.png", None, true, Some("i")),
                ("with space.md", None, false, Some("b")),
                (
                    "",
                    Some(Fragment::Heading("Local".into())),
                    false,
                    Some("c")
                ),
                (
                    "../up/Note.md",
                    Some(Fragment::Block("blk".into())),
                    false,
                    Some("d")
                ),
            ]
        );
        assert!(n.links.iter().all(|l| l.syntax == LinkSyntax::Markdown));
        assert_eq!(link(&n, 0).span.slice(src), "[a](Some%20Note.md#Head)");
        assert_eq!(
            link(&n, 0).target_span.unwrap().slice(src),
            "Some%20Note.md"
        );
        assert_eq!(link(&n, 2).target_span.unwrap().slice(src), "with space.md");
    }

    #[test]
    fn reference_style_links_count_at_their_use() {
        let n = parse("[ref][r] and [shortcut]\n\n[r]: Other.md\n");
        assert_eq!(n.links.len(), 1);
        assert_eq!(link(&n, 0).target, "Other.md");
        assert_eq!(link(&n, 0).target_span, None);
    }

    #[test]
    fn links_in_code_comments_and_math_are_not_links() {
        let src = "`[[inline]]`\n\n```\n[[fenced]] #fenced\n```\n\n%%[[commented]] #commented%% [[kept]]\n\n%%\n[[block comment]]\n%%\n\n$[[math]]$\n\n    [[indented]]\n";
        let n = parse(src);
        let targets: Vec<_> = n.links.iter().map(|l| l.target.as_str()).collect();
        assert_eq!(targets, ["kept"]);
        assert!(n.tags.is_empty());
    }

    #[test]
    fn a_table_cell_keeps_exact_spans_around_an_escaped_pipe() {
        let src = "| a | b |\n|---|---|\n| [[T\\|al]] | [[U#H]] #tag |\n";
        let n = parse(src);
        assert_eq!(link(&n, 0).target, "T");
        assert_eq!(link(&n, 0).text.as_deref(), Some("al"));
        assert_eq!(link(&n, 0).span.slice(src), "[[T\\|al]]");
        assert_eq!(link(&n, 1).span.slice(src), "[[U#H]]");
        assert_eq!(link(&n, 1).target_span.unwrap().slice(src), "U");
        assert_eq!(n.tags[0].span.slice(src), "#tag");
    }

    #[test]
    fn links_in_properties_count() {
        let src = "---\nrelated: \"[[Ideas]]\"\nlist:\n  - \"[[Plan#Head|p]]\"\n  - \"![[pic.png]]\"\n---\nBody [[Body]]\n";
        let n = parse(src);
        let got: Vec<_> = n
            .links
            .iter()
            .map(|l| (l.target.as_str(), l.in_frontmatter, l.embed))
            .collect();
        assert_eq!(
            got,
            [
                ("Ideas", true, false),
                ("Plan", true, false),
                ("pic.png", true, true),
                ("Body", false, false)
            ]
        );
        assert_eq!(link(&n, 0).span.slice(src), "[[Ideas]]");
        assert_eq!(link(&n, 1).span.slice(src), "[[Plan#Head|p]]");
        assert_eq!(link(&n, 2).span.slice(src), "![[pic.png]]");
        assert_eq!(link(&n, 3).span.line, 7);
    }

    #[test]
    fn tags_nested_unicode_and_digits() {
        let src = "#plain and #a/b/c and #ünï and #日本語 and #x_y-z and #a/ and #1984 and #1a and (#paren) and a#no and \\#escaped and [x](#frag) and `#code`";
        let n = parse(src);
        let names: Vec<_> = n.tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "plain",
                "a/b/c",
                "ünï",
                "日本語",
                "x_y-z",
                "a",
                "1a",
                "paren"
            ]
        );
        assert_eq!(texts(src, n.tags.iter().map(|t| t.span))[1], "#a/b/c");
        assert_eq!(n.tags[5].span.slice(src), "#a");
        assert!(n.tags.iter().all(|t| t.source == TagSource::Body));
    }

    #[test]
    fn tags_nest_and_compare_case_insensitively() {
        let t = Tag {
            name: "Area/Sub/Leaf".into(),
            source: TagSource::Body,
            span: Span::default(),
        };
        assert_eq!(t.key(), "area/sub/leaf");
        assert_eq!(t.parents().collect::<Vec<_>>(), ["Area/Sub", "Area"]);
        assert_eq!(
            Tag {
                name: "flat".into(),
                ..t
            }
            .parents()
            .count(),
            0
        );
    }

    #[test]
    fn tags_in_headings_lists_quotes_callouts_and_footnotes() {
        let src = "# Title #h\n\n- item #li\n\n> quote #q\n\n> [!note] Callout #ct\n> body #cb\n\ntext[^1] ^[inline #in]\n\n[^1]: def #fn\n";
        let n = parse(src);
        let names: Vec<_> = n.tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["h", "li", "q", "ct", "cb", "in", "fn"]);
        for t in &n.tags {
            assert_eq!(t.span.slice(src), format!("#{}", t.name));
        }
    }

    #[test]
    fn frontmatter_tags_come_first_with_their_own_source() {
        let src = "---\ntags: [alpha, 'beta/gamma']\n---\nBody #delta\n";
        let n = parse(src);
        let got: Vec<_> = n.tags.iter().map(|t| (t.name.as_str(), t.source)).collect();
        assert_eq!(
            got,
            [
                ("alpha", TagSource::Frontmatter),
                ("beta/gamma", TagSource::Frontmatter),
                ("delta", TagSource::Body)
            ]
        );
        assert_eq!(n.tags[0].span.slice(src), "alpha");
        assert_eq!(n.tags[1].span.slice(src), "beta/gamma");
        assert_eq!(n.tags[2].span.slice(src), "#delta");
    }

    #[test]
    fn frontmatter_properties_aliases_and_errors() {
        let n = parse("---\ntitle: Plan\naliases: [P, Plan B]\ncount: 2\n---\n# H\n");
        let fm = n.frontmatter.as_ref().unwrap();
        assert_eq!(fm.error, None);
        assert_eq!(
            fm.properties.keys().collect::<Vec<_>>(),
            ["title", "aliases", "count"]
        );
        assert_eq!(fm.aliases(), ["P", "Plan B"]);
        assert_eq!(fm.span.line, 1);
        assert_eq!(n.headings[0].span.line, 6);

        let bad = parse("---\na: [unclosed\n---\n# H\n");
        let fm = bad.frontmatter.unwrap();
        assert!(fm.error.is_some());
        assert!(fm.properties.is_empty());
        assert_eq!(bad.headings.len(), 1);

        assert!(
            parse("# No frontmatter\n\n---\nnot: frontmatter\n---\n")
                .frontmatter
                .is_none()
        );
    }

    #[test]
    fn block_ids_on_paragraphs_items_and_the_block_before_them() {
        let src = "Para ^pid\n\n- item one ^li1\n  - child\n- item two\n\n> quote\n\n^qid\n\n| a |\n|---|\n| b |\n\n^tid\n\nnot^an-id and ^bad_id\n\ntext\n^same-para\n";
        let n = parse(src);
        let got: Vec<_> = n
            .blocks
            .iter()
            .map(|b| (b.id.as_str(), b.span.slice(src)))
            .collect();
        assert_eq!(
            got,
            [
                ("pid", "Para ^pid"),
                ("li1", "- item one ^li1\n  - child"),
                ("qid", "> quote"),
                ("tid", "| a |\n|---|\n| b |"),
                ("same-para", "text\n^same-para"),
            ]
        );
        assert_eq!(
            texts(src, n.blocks.iter().map(|b| b.id_span)),
            ["^pid", "^li1", "^qid", "^tid", "^same-para"]
        );
    }

    #[test]
    fn highlights_with_and_without_colour() {
        let src =
            "==plain== ==🔴 red== ==🟠 o== ==🟡 y== ==🟢 g== ==🔵 b== ==🟣 p== ==⚫ none== ==a ==";
        let n = parse(src);
        let got: Vec<_> = n.highlights.iter().map(|h| h.color).collect();
        use HighlightColor::*;
        assert_eq!(
            got,
            [
                None,
                Some(Red),
                Some(Orange),
                Some(Yellow),
                Some(Green),
                Some(Blue),
                Some(Purple),
                None
            ]
        );
        assert_eq!(n.highlights[0].text, "plain");
        assert_eq!(n.highlights[1].text, "🔴 red");
        assert_eq!(n.highlights[1].span.slice(src), "==🔴 red==");
    }

    #[test]
    fn tasks_keep_any_status_character() {
        let src = "- [ ] open\n- [x] done\n- [X] caps\n- [/] partial\n- [-] cancelled\n- [✓] unicode\n  - [ ] nested #t\n\n1. [>] numbered\n";
        let n = parse(src);
        let got: Vec<_> = n
            .tasks
            .iter()
            .map(|t| (t.status, t.text.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (' ', "open"),
                ('x', "done"),
                ('X', "caps"),
                ('/', "partial"),
                ('-', "cancelled"),
                ('✓', "unicode"),
                (' ', "nested #t"),
                ('>', "numbered"),
            ]
        );
        assert_eq!(n.tasks[1].span.slice(src), "- [x] done");
        assert_eq!(n.tasks[1].status_span.slice(src), "x");
        assert_eq!(n.tasks[5].status_span.slice(src), "✓");
        assert_eq!(n.tasks[0].status_span.slice(src), " ");
        assert_eq!(n.tasks[6].span.line, 7);
        assert_eq!(n.tags.len(), 1);
    }

    #[test]
    fn footnotes_references_definitions_and_inline() {
        let src = "Text[^a] and[^b] and ^[inline note].\n\n[^a]: First.\n[^b]: Second.\n";
        let n = parse(src);
        let got: Vec<_> = n
            .footnotes
            .iter()
            .map(|f| (f.kind, f.span.slice(src)))
            .collect();
        assert_eq!(
            got,
            [
                (FootnoteKind::Reference, "[^a]"),
                (FootnoteKind::Reference, "[^b]"),
                (FootnoteKind::Inline, "^[inline note]"),
                (FootnoteKind::Definition, "[^a]: First."),
                (FootnoteKind::Definition, "[^b]: Second."),
            ]
        );
        assert_eq!(n.footnotes[0].name, "a");
    }

    #[test]
    fn inline_fields_in_three_styles() {
        let src = "Key:: Value with [[Link]]\n\nA sentence with [bracket:: 1] and (paren:: two words) and [nested:: [[Note]]].\n\n- list item:: shown\n- [ ] task [due:: 2026-01-02]\n";
        let n = parse(src);
        let got: Vec<_> = n
            .fields
            .iter()
            .map(|f| (f.key.as_str(), f.value.as_str(), f.style))
            .collect();
        assert_eq!(
            got,
            [
                ("Key", "Value with [[Link]]", FieldStyle::Line),
                ("bracket", "1", FieldStyle::Bracket),
                ("paren", "two words", FieldStyle::Paren),
                ("nested", "[[Note]]", FieldStyle::Bracket),
                ("list item", "shown", FieldStyle::Line),
                ("due", "2026-01-02", FieldStyle::Bracket),
            ]
        );
        assert_eq!(n.fields[1].span.slice(src), "[bracket:: 1]");
        assert_eq!(n.fields[3].span.slice(src), "[nested:: [[Note]]]");
        assert_eq!(n.fields[0].span.slice(src), "Key:: Value with [[Link]]");
    }

    #[test]
    fn inline_fields_do_not_appear_in_code_or_for_single_colons() {
        let n = parse("```\nkey:: value\n```\n\nNote: this is fine, see http://x.y\n");
        assert!(n.fields.is_empty());
    }

    #[test]
    fn callouts_with_titles_folds_metadata_and_nesting() {
        let src = "> [!note]\n> plain\n\n> [!Warning]+ Custom title\n> body\n> > [!tip|right]- Nested [[link]]\n> > inner\n";
        let n = parse(src);
        let got: Vec<_> = n
            .callouts
            .iter()
            .map(|c| {
                (
                    c.kind.as_str(),
                    c.title.as_deref(),
                    c.fold,
                    c.metadata.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("note", None, Fold::None, None),
                ("warning", Some("Custom title"), Fold::Expanded, None),
                (
                    "tip",
                    Some("Nested [[link]]"),
                    Fold::Collapsed,
                    Some("right")
                ),
            ]
        );
        assert_eq!(n.callouts[0].span.slice(src), "> [!note]\n> plain");
        assert_eq!(n.links.len(), 1);
    }

    #[test]
    fn crlf_and_a_byte_order_mark_keep_positions_exact() {
        for eol in ["\n", "\r\n"] {
            let src = format!(
                "\u{feff}---{eol}tags: [t]{eol}---{eol}# Head{eol}{eol}[[A]] #b{eol}- [x] done{eol}"
            );
            let n = parse(&src);
            assert_eq!(n.headings[0].span.slice(&src), "# Head", "{eol:?}");
            assert_eq!(n.headings[0].span.line, 4, "{eol:?}");
            let links: Vec<_> = n.links.iter().map(|l| l.span.slice(&src)).collect();
            assert_eq!(links, ["[[A]]"], "{eol:?}");
            assert_eq!(n.tags[0].span.slice(&src), "t", "{eol:?}");
            assert_eq!(n.tags[1].span.slice(&src), "#b", "{eol:?}");
            assert_eq!(n.tasks[0].span.slice(&src), "- [x] done", "{eol:?}");
        }
    }

    #[test]
    fn a_lone_cr_ends_a_line() {
        let src = "# Head\r\r[[A]] #b\r- [x] done\r";
        let n = parse(src);
        assert_eq!(n.headings[0].span.slice(src), "# Head");
        assert_eq!(n.links[0].span.slice(src), "[[A]]");
        assert_eq!(n.links[0].span.line, 3);
        assert_eq!(n.tags[0].span.slice(src), "#b");
        assert_eq!(n.tasks[0].span.slice(src), "- [x] done");
        assert_eq!(n.tasks[0].span.line, 4);
    }

    #[test]
    fn spans_convert_for_emoji_combining_marks_cjk_and_right_to_left() {
        let src = "😀 e\u{301} 日本 שלום [[Ünï]] #tag\n";
        let n = parse(src);
        let o = Offsets::new(src);
        let l = link(&n, 0);
        assert_eq!(l.span.slice(src), "[[Ünï]]");
        let scalars = (o.byte_to_scalar(l.span.start), o.byte_to_scalar(l.span.end));
        let units = (o.byte_to_utf16(l.span.start), o.byte_to_utf16(l.span.end));
        // Before the brackets: 😀, e and U+0301, 日本, four Hebrew letters and four spaces: 13 scalars,
        // 14 UTF-16 units (the emoji is a pair).
        assert_eq!(scalars, (13, 20));
        assert_eq!(units, (14, 21));
        let t = &n.tags[0];
        assert_eq!(o.byte_to_scalar(t.span.start), 21);
        assert_eq!(src.chars().skip(21).take(4).collect::<String>(), "#tag");
    }

    #[test]
    fn an_empty_or_markup_free_note_has_nothing() {
        assert_eq!(parse(""), Note::default());
        assert_eq!(parse("just words\n"), Note::default());
    }

    fn every_span(n: &Note) -> Vec<Span> {
        let mut all = Vec::new();
        all.extend(n.frontmatter.iter().map(|f| f.span));
        all.extend(n.headings.iter().map(|h| h.span));
        all.extend(n.blocks.iter().flat_map(|b| [b.span, b.id_span]));
        all.extend(
            n.links
                .iter()
                .flat_map(|l| [Some(l.span), l.target_span])
                .flatten(),
        );
        all.extend(n.tags.iter().map(|t| t.span));
        all.extend(n.tasks.iter().flat_map(|t| [t.span, t.status_span]));
        all.extend(n.footnotes.iter().map(|f| f.span));
        all.extend(n.fields.iter().map(|f| f.span));
        all.extend(n.callouts.iter().map(|c| c.span));
        all.extend(n.highlights.iter().map(|h| h.span));
        all
    }

    fn assert_spans_hold(src: &str) {
        let n = parse(src);
        for s in every_span(&n) {
            assert!(
                s.start <= s.end && src.get(s.range()).is_some(),
                "span {s:?} is not a slice of {src:?}"
            );
            assert!(s.line >= 1);
        }
    }

    const RICH: &str = "---\ntags: [a, b/c]\nlink: \"[[Prop]]\"\n---\n# Head #t\n\n> [!note]- Title\n> body [[A#B|c]] ![[img.png|100]] ^blk\n\n- [x] task #x ==🟢 hi==\n  - [ ] nested\n\n| a | b |\n|---|---|\n| [[T\\|x]] | #u |\n\nfield:: value [k:: v] %% hidden [[X]] %%\n\nSee[^1] and ^[inline #y] and $x#z$ `[[no]]`\n\n[^1]: Note. 日本語 e\u{301} 😀 שלום\n";

    #[test]
    fn every_prefix_of_a_rich_note_yields_spans_inside_it() {
        for (end, _) in RICH.char_indices().chain([(RICH.len(), ' ')]) {
            assert_spans_hold(&RICH[..end]);
        }
        assert!(!every_span(&parse(RICH)).is_empty());
    }

    #[test]
    fn unfinished_and_hostile_markup_never_panics() {
        let deep = format!("{}x{}", "> ".repeat(40), "]".repeat(40));
        let hostile = [
            "[[",
            "]]",
            "![[",
            "[[]]",
            "[[a]",
            "[[a]]]",
            "[[a|",
            "[[a|b|c]]",
            "[[#]]",
            "[[^]]",
            "[[#^]]",
            "![[a#b#c|d]]",
            "[](",
            "[a](<",
            "[a](%zz)",
            "[a](%ff%fe)",
            "#",
            "# ",
            "#/",
            "#1",
            "#a//b/",
            "a#b",
            "^",
            " ^id",
            "^a ^b",
            "==",
            "==🔴==",
            "== ==",
            "%%",
            "%% x",
            "x %%",
            "key::",
            ":: v",
            "[k::",
            "((k:: v",
            "[[a]] %%",
            "- [",
            "- [x",
            "- [x]",
            "> [!",
            "> [!x",
            "> [!x]-",
            "> [!x|",
            "[^",
            "[^1]:",
            "^[",
            "^[x",
            "---\n",
            "---\n---\n",
            "---\n: [\n---\n",
            "---\n- a\n---\n",
            "---\n\u{0}\n---\n",
            "\u{0}",
            "\u{feff}",
            "\u{feff}---\na: b\n---\n",
            "\r",
            "\r\r\n\n",
            "$$",
            "$a",
            "``",
            "```",
            "\\",
            "\\|",
            "| a |\n|---|\n| [[x\\|",
            "😀[[😀]]#😀",
            "[[日本#語|שלום]]",
            &deep,
        ];
        for src in hostile {
            assert_spans_hold(src);
        }
    }
}
