//! Inline content: text with its tags and double-pipe wikilinks, emphasis, links, math, footnote
//! references and the embeds that sit inside a line.
use super::{
    blocks::{Cited, Walk},
    embeds, html,
};
use crate::parse::fragment_of;
use crate::{Fragment, HighlightColor, Link, LinkSyntax, Span, Target};
use comrak::nodes::{AstNode, NodeValue};
use percent_encoding::percent_decode_str;
use regex::Regex;
use std::sync::LazyLock;

// The patterns `parse` reads tags and wikilinks with; they are private there. `parse_note`
// found the same ones in the same text, so the index and the page agree on what a tag is.
static TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r##"#([^\u{2000}-\u{206F}\u{2E00}-\u{2E7F}'!"#$%&()*+,.:;<=>?@^`{|}~\[\]\\\s]+)"##)
        .unwrap()
});
static WIKI_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(!?)\[\[([^\[\]\r\n]+)\]\]").unwrap());
static BLOCK_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)\^([A-Za-z0-9-]+)\s*$").unwrap());
/// Escaped text with the `#` that [`ESCAPED`] stood for put back.
fn unescaped(text: &str) -> String {
    html::text(&text.replace(ESCAPED, "#"))
}

/// Stands for a `#` the source escapes. It lies in the range of characters a tag cannot hold.
const ESCAPED: char = '\u{2060}';

static EXTERNAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i:[a-z][a-z0-9+.-]*:|//)").unwrap());

/// What a line of text is cut into: wikilinks comrak does not parse (a second `|` divides an
/// embed's alt text from its size), and tags.
enum Hit<'t> {
    Wiki { embed: bool, inner: &'t str },
    Tag(&'t str),
}

impl<'d> Walk<'d> {
    /// The inline children of `node`. `strip_id` drops a block id (`^id`) that ends the last of them.
    pub(super) fn inlines<'a>(&mut self, node: &'a AstNode<'a>, strip_id: bool) -> String {
        let mut out = String::new();
        self.run(&mut out, node, strip_id, 0);
        out
    }

    /// Writes the children of `node`. Adjacent text is one piece (comrak splits it at `_` and the
    /// like), so a tag or a wikilink that straddles a split is still found; `skip` bytes are
    /// dropped from the start of the first piece.
    fn run<'a>(&mut self, out: &mut String, node: &'a AstNode<'a>, strip_id: bool, skip: usize) {
        let (mut text, mut first) = (String::new(), true);
        for child in node.children() {
            let ast = child.data();
            if let NodeValue::Text(t) = &ast.value {
                text.push_str(&self.mark_escapes(t, &ast));
                continue;
            }
            self.piece(out, &mut text, false, &mut first, skip);
            self.inline(out, child);
        }
        self.piece(out, &mut text, strip_id, &mut first, skip);
    }

    /// The text of a node with each `#` its source escapes (`\#`) replaced by [`ESCAPED`], so no
    /// tag starts there. comrak hands over the text unescaped, so the source is the only witness.
    fn mark_escapes(&self, text: &str, ast: &comrak::nodes::Ast) -> String {
        let (start, end) = (ast.sourcepos.start, ast.sourcepos.end);
        let at = self
            .doc
            .offsets
            .sourcepos((start.line, start.column), (end.line, end.column));
        let Some(source) = self
            .doc
            .source
            .get(at.start..at.end)
            .filter(|s| s.len() != text.len())
        else {
            return text.to_string();
        };
        let mut out = String::with_capacity(text.len());
        let mut source = source.chars().peekable();
        for literal in text.chars() {
            match source.next() {
                Some('\\') if source.peek() == Some(&literal) && literal.is_ascii_punctuation() => {
                    source.next();
                    out.push(if literal == '#' { ESCAPED } else { literal });
                }
                Some(c) if c == literal => out.push(literal),
                // An entity or something else that changes the text: no alignment to trust.
                _ => return text.to_string(),
            }
        }
        out
    }

    fn piece(
        &mut self,
        out: &mut String,
        text: &mut String,
        strip_id: bool,
        first: &mut bool,
        skip: usize,
    ) {
        let mut piece = std::mem::take(text);
        if std::mem::take(first) && !piece.is_empty() {
            piece.drain(..skip.min(piece.len()));
        }
        let piece: &str = if strip_id {
            without_block_id(&piece)
        } else {
            &piece
        };
        self.text(out, piece);
    }

    fn inline<'a>(&mut self, out: &mut String, node: &'a AstNode<'a>) {
        let ast = node.data();
        match &ast.value {
            NodeValue::SoftBreak | NodeValue::LineBreak => out.push_str("<br>\n"),
            NodeValue::Code(c) => {
                out.push_str("<code>");
                out.push_str(&html::text(&c.literal));
                out.push_str("</code>");
            }
            // `***both***` is strong around emphasis in Obsidian, emphasis around strong in comrak.
            NodeValue::Emph if self.triple(node) => {
                out.push_str("<strong><em>");
                self.run(
                    out,
                    node.first_child().expect("a triple has its strong child"),
                    false,
                    0,
                );
                out.push_str("</em></strong>");
            }
            NodeValue::Emph => self.wrapped(out, node, "em"),
            NodeValue::Strong => self.wrapped(out, node, "strong"),
            NodeValue::Strikethrough => self.wrapped(out, node, "del"),
            NodeValue::Highlight => self.highlight(out, node),
            NodeValue::Math(m) => {
                let kind = if m.display_math {
                    "math-block"
                } else {
                    "math-inline"
                };
                out.push_str(&format!(
                    r#"<span class="math {kind}">{}</span>"#,
                    html::text(m.literal.trim())
                ));
            }
            NodeValue::HtmlInline(s) => out.push_str(&html::raw(s)),
            NodeValue::Link(l) => self.markdown_link(out, node, &l.url, &l.title),
            NodeValue::Image(l) => self.markdown_image(out, node, &l.url),
            NodeValue::WikiLink(w) => {
                let shown = self.plain(node);
                let alias = (shown != w.url).then_some(shown.as_str());
                out.push_str(&self.wiki(&w.url, w.embed, alias));
            }
            NodeValue::FootnoteReference(_) => self.footnote_reference(out, node),
            // A comment is not drawn.
            NodeValue::CommentInline(_) | NodeValue::MultilineComment(_) => {}
            _ => self.run(out, node, false, 0),
        }
    }

    /// An emphasis that is a `***` or `___` run: one strong child, from three of the same marker.
    fn triple<'a>(&self, node: &'a AstNode<'a>) -> bool {
        let one_strong = node.first_child().is_some_and(|c| {
            matches!(c.data().value, NodeValue::Strong) && c.next_sibling().is_none()
        });
        let start = node.data().sourcepos.start;
        let at = self
            .doc
            .offsets
            .sourcepos((start.line, start.column), (start.line, start.column + 2));
        one_strong && matches!(self.doc.source.get(at.start..at.end), Some("***" | "___"))
    }

    fn wrapped<'a>(&mut self, out: &mut String, node: &'a AstNode<'a>, tag: &str) {
        out.push_str(&format!("<{tag}>"));
        self.run(out, node, false, 0);
        out.push_str(&format!("</{tag}>"));
    }

    /// `==text==`, with the colour a leading colour emoji names (1.14 stores a highlight's colour that way).
    fn highlight<'a>(&mut self, out: &mut String, node: &'a AstNode<'a>) {
        let leading = node
            .first_child()
            .and_then(|first| match &first.data().value {
                NodeValue::Text(t) => t.chars().next(),
                _ => None,
            });
        let color = leading.and_then(HighlightColor::from_emoji);
        let name = match color {
            None => "",
            Some(HighlightColor::Red) => "red",
            Some(HighlightColor::Orange) => "orange",
            Some(HighlightColor::Yellow) => "yellow",
            Some(HighlightColor::Green) => "green",
            Some(HighlightColor::Blue) => "blue",
            Some(HighlightColor::Purple) => "purple",
        };
        out.push_str(&format!(r#"<mark data-highlight="{name}">"#));
        self.run(
            out,
            node,
            false,
            color.and(leading).map_or(0, char::len_utf8),
        );
        out.push_str("</mark>");
    }

    /// Text, cut where a tag or a wikilink with two dividers is.
    fn text(&mut self, out: &mut String, text: &str) {
        let mut hits: Vec<(usize, usize, Hit)> = Vec::new();
        for c in WIKI_LINK.captures_iter(text) {
            let inner = c.get(2).unwrap().as_str();
            if inner.matches('|').count() >= 2 {
                let whole = c.get(0).unwrap();
                hits.push((
                    whole.start(),
                    whole.end(),
                    Hit::Wiki {
                        embed: !c[1].is_empty(),
                        inner,
                    },
                ));
            }
        }
        for c in TAG.captures_iter(text) {
            let whole = c.get(0).unwrap();
            let name = c.get(1).unwrap().as_str();
            let boundary = text[..whole.start()]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace);
            let inside_wiki = hits
                .iter()
                .any(|(s, e, _)| whole.start() >= *s && whole.start() < *e);
            if boundary && !inside_wiki && !name.chars().all(|ch| ch.is_ascii_digit()) {
                hits.push((whole.start(), whole.end(), Hit::Tag(name)));
            }
        }
        hits.sort_by_key(|h| h.0);
        let mut at = 0;
        for (start, end, hit) in hits {
            out.push_str(&unescaped(&text[at..start]));
            match hit {
                Hit::Tag(name) => out.push_str(&format!(
                    r##"<a href="#{0}" class="tag" target="_blank" rel="noopener nofollow">#{0}</a>"##,
                    html::attr(name)
                )),
                Hit::Wiki { embed, inner } => {
                    let (url, alias) = wiki_parts(&inner.replace(ESCAPED, "#"));
                    out.push_str(&self.wiki(&url, embed, alias.as_deref()));
                }
            }
            at = end;
        }
        out.push_str(&unescaped(&text[at..]));
    }

    /// `[[url|alias]]` or `![[url|alias]]`; for an embed the alias is the alt text and size.
    fn wiki(&self, url: &str, embed: bool, alias: Option<&str>) -> String {
        let link = wiki_target(url, embed);
        if embed {
            return embeds::embed(self.doc, &link, alias.unwrap_or(""));
        }
        let label = label_of(&link);
        let tip = if alias.is_some() {
            format!(
                r#" data-tooltip-position="top" aria-label="{}""#,
                html::attr(&label)
            )
        } else {
            String::new()
        };
        format!(
            r#"<a{tip} data-href="{url}" href="{url}" class="internal-link{}" target="_blank" rel="noopener nofollow">{}</a>"#,
            self.unresolved(&link),
            html::text(alias.unwrap_or(&label)),
            url = html::attr(url),
        )
    }

    fn unresolved(&self, link: &Link) -> &'static str {
        match self.doc.cx.index.names().target_of(self.doc.path, link) {
            Target::File(_) => "",
            Target::Unresolved { .. } => " is-unresolved",
        }
    }

    fn markdown_link<'a>(
        &mut self,
        out: &mut String,
        node: &'a AstNode<'a>,
        url: &str,
        title: &str,
    ) {
        let children = self.inlines(node, false);
        let title_attr = if title.is_empty() {
            String::new()
        } else {
            format!(r#" title="{}""#, html::attr(title))
        };
        if EXTERNAL.is_match(url) {
            let tip = if self.plain(node) == url {
                String::new()
            } else {
                format!(
                    r#" data-tooltip-position="top" aria-label="{}""#,
                    html::attr(url)
                )
            };
            let href = if unsafe_scheme(url) {
                String::new()
            } else {
                format!(r#" href="{}""#, html::attr(url))
            };
            out.push_str(&format!(
                r#"<a{tip} rel="noopener nofollow" class="external-link"{title_attr}{href} target="_blank">{children}</a>"#
            ));
            return;
        }
        let (link, written) = markdown_target(url, false);
        out.push_str(&format!(
            r#"<a class="internal-link{}" data-href="{w}" href="{w}"{title_attr} target="_blank" rel="noopener nofollow">{children}</a>"#,
            self.unresolved(&link),
            w = html::attr(&written),
        ));
    }

    fn markdown_image<'a>(&mut self, out: &mut String, node: &'a AstNode<'a>, url: &str) {
        let alt = self.plain(node);
        if EXTERNAL.is_match(url) {
            out.push_str(&embeds::external_image(url, &alt));
            return;
        }
        let (link, _) = markdown_target(url, true);
        out.push_str(&embeds::embed(self.doc, &link, &alt));
    }

    fn footnote_reference<'a>(&mut self, out: &mut String, node: &'a AstNode<'a>) {
        let ast = node.data();
        let NodeValue::FootnoteReference(f) = &ast.value else {
            return;
        };
        let salt = self.doc.salt();
        let inline = f.name.starts_with("__inline_");
        let distinct = self.notes.cited.len();
        let at = match self.notes.cited.iter().position(|c| c.name == f.name) {
            Some(at) => at,
            None => {
                self.notes.cited.push(Cited {
                    name: f.name.clone(),
                    line: ast.sourcepos.start.line,
                    count: 0,
                });
                distinct
            }
        };
        let cited = &mut self.notes.cited[at];
        cited.count += 1;
        let number = at + 1;
        // The same footnote cited again is numbered `1-1`, `1-2`, … after its first citation.
        let shown = match cited.count {
            1 => number.to_string(),
            n => format!("{number}-{}", n - 1),
        };
        // Obsidian names an inline footnote after the distinct footnotes before it.
        let name = if inline {
            format!("[inline{distinct}")
        } else {
            f.name.clone()
        };
        out.push_str(&format!(
            r##"<sup data-footnote-id="fnref-{shown}-{salt}" class="footnote-ref" id="fnref-{shown}-{salt}"><a data-footref="{}" href="#fn-{number}-{salt}" class="footnote-link" target="_blank" rel="noopener nofollow">[{shown}]</a></sup>"##,
            html::attr(&name),
        ));
    }

    /// The text of `node` and what is under it, as a reader would copy it.
    pub(super) fn plain<'a>(&self, node: &'a AstNode<'a>) -> String {
        let mut out = String::new();
        plain_into(&mut out, node);
        out
    }
}

fn plain_into<'a>(out: &mut String, node: &'a AstNode<'a>) {
    match &node.data().value {
        NodeValue::Text(t) => out.push_str(t),
        NodeValue::Code(c) => out.push_str(&c.literal),
        NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
        NodeValue::WikiLink(w) if node.first_child().is_none() => out.push_str(&w.url),
        _ => {
            for child in node.children() {
                plain_into(out, child);
            }
        }
    }
}

/// The text without a `^id` that ends it.
fn without_block_id(text: &str) -> &str {
    match BLOCK_ID.captures(text).and_then(|c| c.get(1)) {
        Some(id) => &text[..id.start() - 1],
        None => text,
    }
}

/// Whether a block id is all a paragraph says.
pub(super) fn is_block_id(text: &str) -> bool {
    let text = text.trim();
    text.starts_with('^') && BLOCK_ID.is_match(text) && without_block_id(text).is_empty()
}

/// `target#fragment|alias` inside `[[…]]`: the part before the divider, and what follows it.
fn wiki_parts(inner: &str) -> (String, Option<String>) {
    let (head, alias) = match inner.split_once('|') {
        Some((head, alias)) => (
            head,
            Some(alias.trim().to_string()).filter(|a| !a.is_empty()),
        ),
        None => (inner, None),
    };
    // A table cell escapes the divider as `\|`.
    let head = head.strip_suffix('\\').unwrap_or(head);
    (head.trim().to_string(), alias)
}

/// The link `[[url]]` names, with the resolver's inputs filled in.
pub(super) fn wiki_target(url: &str, embed: bool) -> Link {
    let (path, fragment) = url
        .split_once('#')
        .map_or((url, None), |(p, f)| (p, Some(f)));
    Link {
        syntax: LinkSyntax::Wiki,
        embed,
        target: path.trim().to_string(),
        fragment: fragment.and_then(fragment_of),
        text: None,
        span: Span::default(),
        target_span: None,
        in_frontmatter: false,
    }
}

/// The link a Markdown destination names, and the destination as the page writes it in `href`.
fn markdown_target(url: &str, embed: bool) -> (Link, String) {
    let decode = |s: &str| percent_decode_str(s).decode_utf8_lossy().into_owned();
    let (path, fragment) = url
        .split_once('#')
        .map_or((url, None), |(p, f)| (p, Some(f)));
    let link = Link {
        syntax: LinkSyntax::Markdown,
        embed,
        target: decode(path),
        fragment: fragment.map(decode).and_then(fragment_of),
        text: None,
        span: Span::default(),
        target_span: None,
        in_frontmatter: false,
    };
    (link, decode_uri(url))
}

/// JavaScript's `decodeURI`, which Obsidian shows a destination through: `%20` is a space, but the
/// escapes of the characters that structure a URL (`#` among them) stay.
fn decode_uri(url: &str) -> String {
    let mut out = Vec::with_capacity(url.len());
    let mut at = 0;
    while at < url.len() {
        let escaped = url
            .get(at + 1..at + 3)
            .filter(|hex| url.as_bytes()[at] == b'%' && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            .filter(|byte| !b";/?:@&=+$,#".contains(byte));
        match escaped {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(url.as_bytes()[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What a link shows without an alias: the target as written, then the heading or block after ` > `.
pub(super) fn label_of(link: &Link) -> String {
    let fragment = match &link.fragment {
        Some(Fragment::Heading(h)) => h
            .split('#')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" > "),
        Some(Fragment::Block(id)) => format!("^{id}"),
        None => String::new(),
    };
    match (link.target.is_empty(), fragment.is_empty()) {
        (_, true) => link.target.clone(),
        (true, false) => fragment,
        (false, false) => format!("{} > {fragment}", link.target),
    }
}

/// Destinations a link must not carry into the page.
fn unsafe_scheme(url: &str) -> bool {
    let squeezed: String = url
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect::<String>()
        .to_ascii_lowercase();
    ["javascript:", "vbscript:", "data:"]
        .iter()
        .any(|scheme| squeezed.starts_with(scheme))
}

/// The direction a cell's text runs in: the first strong character decides, none leaves it to the browser.
pub(super) fn direction(text: &str) -> &'static str {
    use unicode_bidi::{BidiClass, bidi_class};
    for c in text.chars() {
        match bidi_class(c) {
            BidiClass::L => return "ltr",
            BidiClass::R | BidiClass::AL => return "rtl",
            _ => {}
        }
    }
    "auto"
}
