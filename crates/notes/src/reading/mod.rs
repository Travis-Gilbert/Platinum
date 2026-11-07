//! The reading view: a note as the HTML of Obsidian's reading view, for `frame-notes:` pages.
//!
//! comrak's tree (the options `parse` uses) is walked once and written in Obsidian's own DOM, with
//! its class names, because themes and CSS snippets are written against them. `parse_note`
//! supplies what comrak does not: tags, block ids, heading text and link parts. Links resolve
//! through [`crate::FileIndex`]. What Obsidian leaves to scripts that run in the page (Prism,
//! MathJax, Mermaid, pdf.js, the copy button) is left to the page here too: the renderer writes
//! the markup those scripts start from.
use crate::{Note, Offsets, VaultIndex, VaultSettings, parse_note};
use std::cell::Cell;
use std::hash::{DefaultHasher, Hash, Hasher};

mod blocks;
mod embeds;
mod header;
mod html;
mod icons;
mod inlines;

pub use blocks::callout_icon;

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

/// A page preview: what it shows, and the note that is from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    /// The note the link resolved to. The links and tasks in the preview are its.
    pub note: String,
    pub view: ReadingView,
}

/// Obsidian stops nesting embeds of embeds at this depth and shows a plain file embed instead.
const MAX_EMBED_DEPTH: usize = 5;

/// How many notes, and how much of their text, one render may read for embeds. The depth does not
/// bound that: a note embedding itself ten times is 111,111 embeds by `MAX_EMBED_DEPTH`, all read
/// and parsed on the vault's thread, ahead of its saves. Past either limit a note embed shows as a
/// plain file embed too. A cycle still runs to the depth, because Obsidian's does.
const MAX_EMBEDS: usize = 1024;
const MAX_EMBEDDED_BYTES: usize = 16 * 1024 * 1024;

/// What a render reads from outside the note.
pub(crate) struct Cx<'a> {
    index: &'a VaultIndex,
    settings: &'a VaultSettings,
    read: &'a dyn Fn(&str) -> Option<String>,
    /// The notes read so far for this render, and the bytes they held.
    embedded: Cell<(usize, usize)>,
}

impl<'a> Cx<'a> {
    fn new(
        index: &'a VaultIndex,
        settings: &'a VaultSettings,
        read: &'a dyn Fn(&str) -> Option<String>,
    ) -> Self {
        Self {
            index,
            settings,
            read,
            embedded: Cell::new((0, 0)),
        }
    }

    /// Whether this render may read another note to embed.
    fn may_embed(&self) -> bool {
        let (notes, bytes) = self.embedded.get();
        notes < MAX_EMBEDS && bytes < MAX_EMBEDDED_BYTES
    }

    /// The text of the note at `path`, counted against what this render may read.
    fn read_note(&self, path: &str) -> Option<String> {
        let text = (self.read)(path);
        let (notes, bytes) = self.embedded.get();
        self.embedded
            .set((notes + 1, bytes + text.as_ref().map_or(0, String::len)));
        text
    }
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
    let cx = Cx::new(index, settings, read);
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
) -> Option<Preview> {
    let cx = Cx::new(index, settings, read);
    embeds::preview(&cx, from, link)
}
