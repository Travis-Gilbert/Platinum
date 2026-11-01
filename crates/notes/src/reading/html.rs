//! Escaping, vault file URLs and the sanitiser for the HTML a note writes itself.
use crate::{FileIndex, LinkSyntax, join_relative};
use ammonia::{Builder, UrlRelative, UrlRelativeEvaluate};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use std::borrow::Cow;
use std::collections::HashSet;

pub(super) fn text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

pub(super) fn attr(s: &str) -> String {
    text(s).replace('"', "&quot;")
}

const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'/');

/// Where the page serves the vault file at `path`: the page's protocol handler maps
/// `/files/<vault-relative path>` to the file.
pub(super) fn file_url(path: &str) -> String {
    format!("/files/{}", utf8_percent_encode(path, SEGMENT))
}

// Raw HTML is written into the output between these two private-use characters and sanitised
// when its block is finished; what the renderer generated around it is never handed to the
// sanitiser.
pub(super) const RAW_OPEN: char = '\u{F8FD}';
pub(super) const RAW_CLOSE: char = '\u{F8FE}';
const MASK: char = '\u{F8FF}';

pub(super) fn raw(html: &str) -> String {
    format!("{RAW_OPEN}{html}{RAW_CLOSE}")
}

/// Sanitises the raw HTML in a finished block's markup, in the context of the markup around it.
pub(super) fn seal(markup: String, from: &str, names: &FileIndex) -> String {
    if !markup.contains(RAW_OPEN) {
        return markup;
    }
    let (mut masked, mut generated) = (String::new(), Vec::new());
    let mut rest = markup.as_str();
    while let Some(open) = rest.find(RAW_OPEN) {
        generated.push(&rest[..open]);
        masked.push_str(&format!("{MASK}{}{MASK}", generated.len() - 1));
        let after = &rest[open + RAW_OPEN.len_utf8()..];
        let close = after.find(RAW_CLOSE).unwrap_or(after.len());
        masked.push_str(&after[..close]);
        rest = after.get(close + RAW_CLOSE.len_utf8()..).unwrap_or("");
    }
    generated.push(rest);
    masked.push_str(&format!("{MASK}{}{MASK}", generated.len() - 1));
    let mut clean = sanitiser(from, names).clean(&masked).to_string();
    for (i, part) in generated.iter().enumerate() {
        clean = clean.replace(&format!("{MASK}{i}{MASK}"), part);
    }
    clean
}

/// What a note may write for itself: formatting, tables, media and `details`, with scripts,
/// frames, forms, event handlers and `javascript:` links removed. `style` is removed as well:
/// ammonia cannot sanitise CSS, and a vault synced from elsewhere is not trusted.
fn sanitiser<'a>(from: &'a str, names: &'a FileIndex) -> Builder<'a> {
    let mut b = Builder::default();
    b.add_tags([
        "mark", "kbd", "audio", "video", "source", "details", "summary", "del",
    ])
    .add_tag_attributes(
        "audio",
        [
            "src",
            "controls",
            "controlslist",
            "preload",
            "loop",
            "muted",
        ],
    )
    .add_tag_attributes(
        "video",
        [
            "src",
            "controls",
            "controlslist",
            "preload",
            "loop",
            "muted",
            "poster",
            "width",
            "height",
        ],
    )
    .add_tag_attributes("source", ["src", "type"])
    .add_tag_attributes("a", ["target"])
    .add_tag_attributes("details", ["open"])
    .add_generic_attributes(["class", "id", "dir", "lang", "title"])
    .add_generic_attribute_prefixes(["data-"])
    .set_tag_attribute_value("img", "referrerpolicy", "no-referrer")
    .set_tag_attribute_value("a", "target", "_blank")
    .link_rel(Some("noopener nofollow"))
    .strip_comments(true)
    .clean_content_tags(HashSet::from(["script", "style"]))
    .url_relative(UrlRelative::Custom(Box::new(VaultFiles { from, names })));
    b
}

/// Relative URLs in a note's HTML name vault files, as links and embeds do.
struct VaultFiles<'a> {
    from: &'a str,
    names: &'a FileIndex,
}

impl<'a> UrlRelativeEvaluate<'a> for VaultFiles<'a> {
    fn evaluate<'url>(&self, url: &'url str) -> Option<Cow<'url, str>> {
        let rel = url.split(['?', '#']).next().unwrap_or(url);
        let rel = percent_decode_str(rel).decode_utf8_lossy();
        let resolved = join_relative(self.from, &rel)
            .filter(|p| self.names.contains(p))
            .or_else(|| {
                self.names
                    .resolve(&rel, self.from, LinkSyntax::Markdown)
                    .map(str::to_string)
            });
        Some(match resolved {
            Some(path) => Cow::Owned(file_url(&path)),
            None => Cow::Borrowed(url),
        })
    }
}
