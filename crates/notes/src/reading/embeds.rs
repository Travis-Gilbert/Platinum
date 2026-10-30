//! Embeds (`![[…]]`, `![](…)`) and the previews of links, drawn as Obsidian draws each kind of file.
//!
//! A note, heading or block embed is the reading view of that text inside the embedding page, made
//! by the same renderer as the page itself. What needs a script in the page (pdf.js, the Bases
//! and Canvas viewers) is left as the container those scripts fill, with the vault path of the
//! file in `data-frame-file`.
use super::{
    Cx, Doc, MAX_EMBED_DEPTH, ReadingView, blocks, header,
    html::{self, file_url},
    icons::icon,
    inlines::{label_of, wiki_target},
};
use crate::{FileKind, Fragment, Link, LinkSyntax, Target, locate, parse_note};
use regex::Regex;
use std::sync::LazyLock;

/// `100` or `100x50`, the size an embed's alias may end in.
static SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)(?:x(\d+))?$").unwrap());

/// Obsidian's default height for a PDF embed that does not say.
const PDF_HEIGHT: u32 = 800;

/// An embed's alias split into the alt text and the size it ends in.
struct Alias {
    alt: String,
    width: Option<String>,
    height: Option<String>,
}

fn alias(raw: &str) -> Alias {
    let (alt, size) = match raw.rsplit_once('|') {
        Some((alt, size)) if SIZE.is_match(size.trim()) => (alt, Some(size.trim())),
        _ if SIZE.is_match(raw.trim()) => ("", Some(raw.trim())),
        _ => (raw, None),
    };
    let caps = size.and_then(|s| SIZE.captures(s));
    Alias {
        alt: alt.trim().to_string(),
        width: caps.as_ref().map(|c| c[1].to_string()),
        height: caps.and_then(|c| c.get(2).map(|h| h.as_str().to_string())),
    }
}

fn size_attrs(a: &Alias) -> (String, String) {
    let attr = |name: &str, v: &Option<String>| {
        v.as_ref()
            .map(|v| format!(r#" {name}="{v}""#))
            .unwrap_or_default()
    };
    (attr("width", &a.width), attr("height", &a.height))
}

/// What the embed's `src` attribute holds: the target as written, with its `#…`.
fn source(link: &Link) -> String {
    format!(
        "{}{}",
        link.target,
        link.fragment
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default()
    )
}

/// `link` embedded in the note being rendered, with the alias the embed was written with.
pub(super) fn embed(doc: &Doc, link: &Link, raw_alias: &str) -> String {
    let a = alias(raw_alias);
    let src = html::attr(&source(link));
    let label = label_of(link);
    let alt = html::attr(if a.alt.is_empty() { &label } else { &a.alt });
    let path = match doc.cx.index.names().target_of(doc.path, link) {
        Target::File(path) => path,
        Target::Unresolved { .. } => {
            return format!(
                r#"<span alt="{alt}" src="{src}" class="internal-embed file-embed mod-empty">“{}” is not created yet. Click to create.</span>"#,
                html::text(&link.target)
            );
        }
    };
    let url = html::attr(&file_url(&path));
    let file = html::attr(&path);
    let kind = FileKind::of(&path);
    let (width, height) = size_attrs(&a);
    match kind {
        FileKind::Image => format!(
            r#"<span{width}{height} alt="{alt}" src="{src}" class="internal-embed media-embed image-embed"><img alt="{alt}"{width}{height} src="{url}"></span>"#
        ),
        FileKind::Audio => format!(
            r#"<span alt="{alt}" src="{src}" class="internal-embed media-embed audio-embed"><audio controls="" controlslist="nodownload" src="{url}"></audio></span>"#
        ),
        FileKind::Video => format!(
            r#"<span{width} alt="{alt}" src="{src}" class="internal-embed media-embed video-embed"><video controls="" preload="metadata" src="{url}#t=0.001" style=""></video></span>"#
        ),
        FileKind::Pdf => {
            let height = pdf_height(link);
            format!(
                r#"<span alt="{alt}" src="{src}" class="internal-embed pdf-embed" style="height: {height}px;" data-frame-file="{file}"></span>"#
            )
        }
        FileKind::Canvas => format!(
            r#"<span alt="{alt}" src="{src}" class="internal-embed canvas-embed inline-embed" data-frame-file="{file}"><div class="embed-title"><span class="file-embed-icon">{}</span> {}</div></span>"#,
            icon("lucide-layout-dashboard"),
            html::text(header::title_of(&path).trim_end_matches(".canvas")),
        ),
        FileKind::Base => format!(
            r#"<span alt="{alt}" src="{src}" class="internal-embed bases-embed interactive-child" data-frame-file="{file}"></span>"#
        ),
        FileKind::Note if doc.depth < MAX_EMBED_DEPTH => {
            match excerpt(doc.cx, &path, link.fragment.as_ref()) {
                Some(ex) => note_embed(doc, &ex, &alt, &src),
                None => format!(
                    r#"<span alt="{alt}" src="{src}" class="internal-embed file-embed mod-empty">“{}” is not created yet. Click to create.</span>"#,
                    html::text(&link.target)
                ),
            }
        }
        // A note too deep to embed, and files Obsidian has no viewer for, show as a file.
        FileKind::Note | FileKind::Other => format!(
            r#"<span alt="{alt}" src="{src}" class="internal-embed file-embed mod-generic"><div class="file-embed-title"><span class="file-embed-icon">{}</span> {}</div></span>"#,
            icon("lucide-file"),
            html::text(path.rsplit('/').next().unwrap_or(&path)),
        ),
    }
}

/// `height=N` in the `#page=2&height=300` after a PDF's name.
fn pdf_height(link: &Link) -> u32 {
    let Some(Fragment::Heading(params)) = &link.fragment else {
        return PDF_HEIGHT;
    };
    params
        .split('&')
        .find_map(|p| p.strip_prefix("height="))
        .and_then(|h| h.parse().ok())
        .unwrap_or(PDF_HEIGHT)
}

/// An image on the web. Obsidian loads it without a referrer, like the image in a note's own HTML.
pub(super) fn external_image(url: &str, raw_alias: &str) -> String {
    let a = alias(raw_alias);
    if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("//")) {
        return html::text(raw_alias);
    }
    let (width, height) = size_attrs(&a);
    format!(
        r#"<img alt="{}"{width}{height} src="{}" referrerpolicy="no-referrer">"#,
        html::attr(&a.alt),
        html::attr(url)
    )
}

/// The part of a note a link points at.
struct Excerpt {
    path: String,
    /// The text to render: the note, or the section or block cut out of it.
    source: String,
    kind: Kind,
    cssclasses: Vec<String>,
}

enum Kind {
    Note,
    Heading { level: u8, text: String },
    Block,
}

fn excerpt(cx: &Cx, path: &str, fragment: Option<&Fragment>) -> Option<Excerpt> {
    let full = (cx.read)(path)?;
    let note = parse_note(&full);
    let cssclasses = note
        .frontmatter
        .as_ref()
        .map(|fm| fm.css_classes())
        .unwrap_or_default();
    let (source, kind) = match fragment {
        None => (full, Kind::Note),
        Some(fragment) => {
            let span = locate(&note, fragment)?;
            match fragment {
                Fragment::Block(_) => (span.slice(&full).to_string(), Kind::Block),
                Fragment::Heading(_) => {
                    // The section runs to the next heading of the same level or higher.
                    let at = note.headings.iter().position(|h| h.span == span)?;
                    let heading = &note.headings[at];
                    let end = note.headings[at + 1..]
                        .iter()
                        .find(|h| h.level <= heading.level)
                        .map_or(full.len(), |h| h.span.start);
                    (
                        full[span.start..end].to_string(),
                        Kind::Heading {
                            level: heading.level,
                            text: heading.text.clone(),
                        },
                    )
                }
            }
        }
    };
    Some(Excerpt {
        path: path.to_string(),
        source,
        kind,
        cssclasses,
    })
}

/// The children of a `.markdown-preview-sizer` for `ex`, rendered `depth` levels down.
fn view(cx: &Cx, ex: &Excerpt, depth: usize) -> String {
    let doc = Doc::new(cx, &ex.path, &ex.source, depth);
    let mut out = match &ex.kind {
        Kind::Note => header::header(&doc, header::Title::Note),
        Kind::Heading { level, text } => header::header(
            &doc,
            header::Title::Heading {
                level: *level,
                text: text.clone(),
            },
        ),
        Kind::Block => String::new(),
    };
    out.push_str(&blocks::blocks(&doc));
    out
}

fn note_embed(doc: &Doc, ex: &Excerpt, alt: &str, src: &str) -> String {
    let (data_type, title) = match &ex.kind {
        Kind::Note => ("", html::text(header::title_of(&ex.path))),
        Kind::Heading { .. } => (r#" data-type="heading""#, String::new()),
        Kind::Block => (r#" data-type="block""#, String::new()),
    };
    let classes: String = ex
        .cssclasses
        .iter()
        .map(|c| format!(" {}", html::attr(c)))
        .collect();
    format!(
        r#"<span alt="{alt}" src="{src}" class="internal-embed markdown-embed inline-embed"{data_type}><div class="embed-title markdown-embed-title">{title}</div><div class="markdown-embed-content node-insert-event"><div class="markdown-preview-view markdown-rendered node-insert-event show-indentation-guide allow-fold-headings allow-fold-lists show-properties{classes}"><div class="markdown-preview-sizer markdown-preview-section" style="padding-bottom: 0px; min-height: 0px;"><div class="markdown-preview-pusher" style="width: 1px; height: 0.1px; margin-bottom: 0px;"></div>{}</div></div></div><div class="markdown-embed-content node-insert-event" style="display: none;"></div><div class="markdown-embed-link" aria-label="Open link">{}</div></span>"#,
        view(doc.cx, ex, doc.depth + 1),
        icon("lucide-maximize-2"),
    )
}

/// What a page preview of `link` (the text of a link written in the note `from`) shows.
pub(super) fn preview(cx: &Cx, from: &str, link: &str) -> Option<ReadingView> {
    let inner = link.trim();
    let inner = inner
        .strip_prefix("[[")
        .and_then(|l| l.strip_suffix("]]"))
        .unwrap_or(inner);
    let target = inner.split_once('|').map_or(inner, |(target, _)| target);
    let mut wiki = wiki_target(target.trim(), false);
    let names = cx.index.names();
    let path = match names.target_of(from, &wiki) {
        Target::File(path) => path,
        Target::Unresolved { .. } => {
            // A link copied from a Markdown link names its file the Markdown way.
            wiki.syntax = LinkSyntax::Markdown;
            match names.target_of(from, &wiki) {
                Target::File(path) => path,
                Target::Unresolved { .. } => return None,
            }
        }
    };
    if FileKind::of(&path) != FileKind::Note {
        return None;
    }
    let ex = excerpt(cx, &path, wiki.fragment.as_ref())?;
    // A whole note is the page itself; a section or block is drawn like an embed of it.
    let depth = if matches!(ex.kind, Kind::Note) { 0 } else { 1 };
    Some(ReadingView {
        html: view(cx, &ex, depth),
        cssclasses: ex.cssclasses.clone(),
    })
}
