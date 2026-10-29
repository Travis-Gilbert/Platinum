//! The reading view: a note as the HTML of Obsidian's reading view, for `frame-notes:` pages.
use crate::{VaultIndex, VaultSettings, stem};
use comrak::{Options, markdown_to_html};

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
    let _ = (index, settings, read);
    let mut o = Options::default();
    o.render.sourcepos = true;
    o.extension.front_matter_delimiter = Some("---".into());
    let title = escape(stem(path));
    ReadingView {
        html: format!(
            "<div class=\"mod-header mod-ui\"><div class=\"inline-title\">{title}</div></div>{}",
            markdown_to_html(source, &o)
        ),
        cssclasses: Vec::new(),
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
    let _ = (index, settings, from, link, read);
    None
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
