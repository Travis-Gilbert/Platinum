//! The reading view: a note as the HTML of Obsidian's reading view, for `frame-notes:` pages.
//!
//! comrak's tree (the options `parse` uses) is walked once and written in Obsidian's own DOM, with
//! its class names, because themes and CSS snippets are written against them. `parse_note`
//! supplies what comrak does not: tags, block ids, heading text and link parts. Links resolve
//! through [`crate::FileIndex`]. What Obsidian leaves to scripts that run in the page (Prism,
//! MathJax, Mermaid, pdf.js, the copy button) is left to the page here too: the renderer writes
//! the markup those scripts start from.
use crate::{Note, Offsets, VaultIndex, VaultSettings, parse_note};
use std::hash::{DefaultHasher, Hash, Hasher};

mod blocks;
mod embeds;
mod header;
mod html;
mod icons;
mod inlines;

/// A note rendered for the reading view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadingView {
    /// The children of Obsidian's `.markdown-preview-sizer`: the header (inline
    /// title and properties), then one `div` per top-level block, each carrying
    /// the block's `data-sourcepos`.
    pub html: String,
    /// The note's `cssclasses` property, for the reading view's container.
    pub cssclasses: Vec<String>,
}

/// Obsidian stops nesting embeds of embeds at this depth and shows a plain file embed instead.
const MAX_EMBED_DEPTH: usize = 5;

/// What a render reads from outside the note.
pub(crate) struct Cx<'a> {
    index: &'a VaultIndex,
    settings: &'a VaultSettings,
    read: &'a dyn Fn(&str) -> Option<String>,
}

/// One note being rendered: the page's, or one embedded in it.
pub(crate) struct Doc<'a> {
    cx: &'a Cx<'a>,
    path: &'a str,
    source: &'a str,
    note: Note,
    offsets: Offsets,
    /// 0 for the note itself, n for an embed n levels down.
    depth: usize,
}

impl<'a> Doc<'a> {
    fn new(cx: &'a Cx<'a>, path: &'a str, source: &'a str, depth: usize) -> Self {
        // A byte order mark is not part of the text.
        let source = source.strip_prefix('\u{feff}').unwrap_or(source);
        Self {
            cx,
            path,
            source,
            note: parse_note(source),
            offsets: Offsets::new(source),
            depth,
        }
    }

    /// Keeps this note's footnote ids apart from those of the notes embedded in the same page.
    fn salt(&self) -> String {
        let mut hasher = DefaultHasher::new();
        (self.path, self.depth).hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

/// Renders `source`, the current text of the note at `path`. `read` returns the
/// current text of another note in the vault, for embeds, or `None` when it
/// does not exist.
pub fn render_note(
    index: &VaultIndex,
    settings: &VaultSettings,
    path: &str,
    source: &str,
    read: &dyn Fn(&str) -> Option<String>,
) -> ReadingView {
    let cx = Cx {
        index,
        settings,
        read,
    };
    let doc = Doc::new(&cx, path, source, 0);
    let mut html = header::header(&doc, header::Title::Note);
    html.push_str(&blocks::blocks(&doc));
    ReadingView {
        html,
        cssclasses: header::cssclasses(&doc),
    }
}

/// Renders what a page preview of `link`, written in the note at `from`, shows:
/// the linked note, heading or block, or `None` when the link is unresolved.
pub fn render_preview(
    index: &VaultIndex,
    settings: &VaultSettings,
    from: &str,
    link: &str,
    read: &dyn Fn(&str) -> Option<String>,
) -> Option<ReadingView> {
    let cx = Cx {
        index,
        settings,
        read,
    };
    embeds::preview(&cx, from, link)
}
