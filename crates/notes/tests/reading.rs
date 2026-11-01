//! The reading view where Obsidian's recording says nothing: what a note may not smuggle in, the
//! containers a page script fills, previews, and the embeds the fixture vault does not have.
//! (What the recording does hold is `reading_dom.rs`.)
use frame_notes::{Vault, render_note, render_preview};
use std::fs;
use tempfile::TempDir;

struct Fixture {
    _root: TempDir,
    _data: TempDir,
    vault: Vault,
    files: Vec<(String, String)>,
}

fn fixture(files: &[(&str, &str)]) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let full = root.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, text).unwrap();
    }
    let vault = Vault::open(root.path(), data.path()).unwrap();
    Fixture {
        _root: root,
        _data: data,
        vault,
        files: files
            .iter()
            .map(|(p, t)| (p.to_string(), t.to_string()))
            .collect(),
    }
}

impl Fixture {
    fn read(&self, path: &str) -> Option<String> {
        self.files
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, t)| t.clone())
    }

    fn html(&self, path: &str) -> String {
        let source = self.read(path).unwrap();
        render_note(
            self.vault.index(),
            self.vault.settings(),
            path,
            &source,
            &|p| self.read(p),
        )
        .html
    }

    fn preview(&self, from: &str, link: &str) -> Option<String> {
        render_preview(
            self.vault.index(),
            self.vault.settings(),
            from,
            link,
            &|p| self.read(p),
        )
        .map(|view| view.html)
    }
}

#[test]
fn raw_html_loses_scripts_handlers_frames_and_script_urls() {
    let f = fixture(&[(
        "Raw.md",
        "<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>\n\n<a href=\"javascript:alert(1)\">x</a>\n\n<iframe src=\"https://example.com\"></iframe>\n\n<details open><summary>s</summary>kept</details>\n",
    )]);
    let html = f.html("Raw.md");
    for forbidden in ["<script", "alert(1)</", "onerror", "javascript:", "<iframe"] {
        assert!(!html.contains(forbidden), "{forbidden} survived: {html}");
    }
    assert!(
        html.contains("<details open") && html.contains("kept"),
        "{html}"
    );
}

#[test]
fn each_top_level_block_carries_its_source_position() {
    let f = fixture(&[("N.md", "# T\n\npara\n\n- a\n- b\n")]);
    let html = f.html("N.md");
    for position in ["1:1-1:3", "3:1-3:4", "5:1-6:3"] {
        assert!(
            html.contains(&format!(r#"data-sourcepos="{position}""#)),
            "{position}: {html}"
        );
    }
    assert_eq!(html.matches("data-sourcepos").count(), 3, "{html}");
}

#[test]
fn an_embedded_note_has_no_source_positions_of_its_own() {
    let f = fixture(&[
        ("Page.md", "![[Other]]\n"),
        ("Other.md", "# Other\n\nbody\n\nmore\n"),
    ]);
    let html = f.html("Page.md");
    assert!(
        html.contains("markdown-embed") && html.contains("body"),
        "{html}"
    );
    assert_eq!(html.matches("data-sourcepos").count(), 1, "{html}");
}

#[test]
fn a_block_embed_shows_that_block_alone() {
    let f = fixture(&[
        ("Page.md", "![[Other#^abc]]\n"),
        ("Other.md", "Intro\n\nTarget paragraph ^abc\n\nOutro\n"),
    ]);
    let html = f.html("Page.md");
    assert!(
        html.contains(r#"data-type="block""#) && html.contains("Target paragraph"),
        "{html}"
    );
    assert!(!html.contains("Intro") && !html.contains("Outro"), "{html}");
}

#[test]
fn footnotes_of_embedded_notes_keep_their_own_ids() {
    let f = fixture(&[
        ("Page.md", "Page[^1]\n\n[^1]: page note\n\n![[Other]]\n"),
        ("Other.md", "Other[^1]\n\n[^1]: other note\n"),
    ]);
    let html = f.html("Page.md");
    let mut ids: Vec<&str> = html
        .match_indices(r#" id="fn-1-"#)
        .map(|(at, _)| &html[at + 5..at + 5 + 5 + 16])
        .collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 2, "{html}");
}

#[test]
fn an_image_on_the_web_is_sized_and_loaded_without_a_referrer() {
    let f = fixture(&[(
        "N.md",
        "![alt|100x50](https://example.com/a.png)\n\n![](http://example.com/b.png)\n",
    )]);
    let html = f.html("N.md");
    assert!(
        html.contains(r#"<img alt="alt" width="100" height="50" src="https://example.com/a.png" referrerpolicy="no-referrer">"#),
        "{html}"
    );
    assert!(html.contains(r#"src="http://example.com/b.png""#), "{html}");
}

#[test]
fn a_pdf_embed_is_a_container_for_pdfjs_sized_by_its_link() {
    let f = fixture(&[
        ("N.md", "![[doc.pdf#page=2&height=300]]\n\n![[doc.pdf]]\n"),
        ("doc.pdf", ""),
    ]);
    let html = f.html("N.md");
    assert!(html.contains(r#"style="height: 300px;""#), "{html}");
    assert!(html.contains(r#"style="height: 800px;""#), "{html}");
    assert_eq!(
        html.matches(r#"data-frame-file="doc.pdf""#).count(),
        2,
        "{html}"
    );
    assert!(
        html.contains(r#"src="doc.pdf#page=2&amp;height=300""#),
        "{html}"
    );
}

#[test]
fn bases_and_canvases_are_containers_for_the_page_to_fill() {
    let f = fixture(&[
        (
            "N.md",
            "![[Tasks.base]]\n\n![[Board.canvas]]\n\n```base\nviews:\n  - type: table\n```\n",
        ),
        ("Tasks.base", "views:\n  - type: table\n"),
        ("Board.canvas", r#"{"nodes":[],"edges":[]}"#),
    ]);
    let html = f.html("N.md");
    assert!(
        html.contains(
            r#"class="internal-embed bases-embed interactive-child" data-frame-file="Tasks.base""#
        ),
        "{html}"
    );
    assert!(
        html.contains(
            r#"class="internal-embed canvas-embed inline-embed" data-frame-file="Board.canvas""#
        ),
        "{html}"
    );
    assert!(
        html.contains(
            r#"class="block-language-base bases-embed interactive-child" data-frame-source="views:"#
        ),
        "{html}"
    );
}

#[test]
fn previews_show_the_note_heading_or_block_a_link_names() {
    let f = fixture(&[
        ("Page.md", "[[Other]]\n"),
        (
            "Other.md",
            "# Other\n\nbody ^id\n\n## Part\n\npart text\n\n## Next\n\nnext text\n",
        ),
        ("pic.png", ""),
    ]);
    let note = f.preview("Page.md", "[[Other]]").unwrap();
    assert!(
        note.contains(r#"<div class="inline-title">Other</div>"#) && note.contains("next text"),
        "{note}"
    );
    let heading = f.preview("Page.md", "Other#Part").unwrap();
    assert!(
        heading.contains("part text") && !heading.contains("next text"),
        "{heading}"
    );
    let block = f.preview("Page.md", "[[Other#^id]]").unwrap();
    assert!(
        block.contains("body") && !block.contains("part text"),
        "{block}"
    );
    assert!(f.preview("Page.md", "[[Nowhere]]").is_none());
    assert!(f.preview("Page.md", "[[pic.png]]").is_none());
}

#[test]
fn cssclasses_are_the_notes_own() {
    let f = fixture(&[(
        "N.md",
        "---\ncssclasses:\n  - wide\n  - dashboard\n---\nbody\n",
    )]);
    let source = f.read("N.md").unwrap();
    let view = render_note(f.vault.index(), f.vault.settings(), "N.md", &source, &|p| {
        f.read(p)
    });
    assert_eq!(view.cssclasses, ["wide", "dashboard"]);
}
