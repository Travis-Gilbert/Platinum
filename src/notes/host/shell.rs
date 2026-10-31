//! The document a note's `frame-notes:` address serves: Obsidian's reading
//! view structure, the vault's appearance, and the note's rendering as data.
//!
//! The rendering is not placed in the markup. It travels as JSON in a
//! `<script type="application/json">`, which no browser runs, and the page
//! script sanitizes it and morphs it in, exactly as it does a patch. A note's
//! markup can therefore never be parsed as part of the shell, so it cannot end
//! the shell's own tags or smuggle in an element of the shell's.

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::json;

use super::ops::{Appearance, PageData, Scheme};

/// Obsidian's classes on the reading view, for the settings Frame has: readable
/// line width, folding of headings and lists, indentation guides, properties.
const VIEW_CLASSES: &str = "markdown-preview-view markdown-rendered node-insert-event is-readable-line-width allow-fold-headings allow-fold-lists show-indentation-guide show-properties";

/// The page for the note at `path`, to be loaded under `token`.
pub fn render(path: &str, token: &str, page: &PageData) -> String {
    let appearance = &page.appearance;
    let theme = match appearance.scheme {
        Scheme::Light => "light",
        Scheme::Dark => "dark",
        Scheme::System => "system",
    };
    let config = json!({
        "token": token,
        "path": path,
        "revision": page.rendered.revision,
        "digest": page.rendered.digest,
        "theme": theme,
        "mermaid": page.mermaid,
        "folds": {},
        "cssclasses": page.rendered.cssclasses,
        "html": page.rendered.html,
    });
    let snippets: String = appearance
        .snippets
        .iter()
        .map(|name| {
            format!(
                "<link rel=\"stylesheet\" href=\"/snippet/{}.css\">",
                utf8_percent_encode(name, NON_ALPHANUMERIC)
            )
        })
        .collect();
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{title}</title>\
<link rel=\"stylesheet\" href=\"/app/reading.css\">{appearance}{snippets}</head>\
<body class=\"{scheme}\">\
<div class=\"app-container\"><div class=\"horizontal-main-container\"><div class=\"workspace\">\
<div class=\"workspace-leaf\"><div class=\"workspace-leaf-content\"><div class=\"view-content\">\
<div class=\"markdown-reading-view\" style=\"width: 100%; height: 100%;\">\
<div class=\"{VIEW_CLASSES}\" tabindex=\"-1\">\
<div class=\"markdown-preview-sizer markdown-preview-section\" style=\"padding-bottom: 0px; min-height: 0px;\"></div>\
</div></div></div></div></div></div></div></div>\
<script type=\"application/json\" id=\"frame-notes-config\">{config}</script>\
<script src=\"/app/vendor/idiomorph/idiomorph.min.js\"></script>\
<script src=\"/app/reading.js\"></script>\
</body></html>",
        title = escape(&page.title),
        appearance = appearance_style(appearance),
        scheme = if appearance.scheme == Scheme::Dark {
            "theme-dark"
        } else {
            "theme-light"
        },
        config = script_json(&config.to_string()),
    )
}

/// The vault's accent color and font size, after the default theme and before
/// the vault's own snippets.
fn appearance_style(appearance: &Appearance) -> String {
    let mut declarations = String::new();
    if let Some((h, s, l)) = appearance.accent {
        declarations.push_str(&format!("--accent-h:{h};--accent-s:{s}%;--accent-l:{l}%;"));
    }
    if let Some(size) = appearance.font_size {
        declarations.push_str(&format!("--font-text-size:{size}px;"));
    }
    if declarations.is_empty() {
        String::new()
    } else {
        format!("<style>body{{{declarations}}}</style>")
    }
}

/// JSON that can sit in a `<script>` element: nothing in it can end the
/// element or open a comment.
fn script_json(json: &str) -> String {
    json.replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ops::Rendered;

    fn page(html: &str) -> PageData {
        PageData {
            title: "A <b> & note".into(),
            rendered: Rendered {
                revision: 7,
                html: html.into(),
                cssclasses: vec!["wide".into()],
                digest: "abc".into(),
            },
            appearance: Appearance::default(),
            mermaid: false,
        }
    }

    /// The config the page script would read.
    fn config(document: &str) -> serde_json::Value {
        let start = document.find("id=\"frame-notes-config\">").unwrap() + 24;
        let end = start + document[start..].find("</script>").unwrap();
        serde_json::from_str(&document[start..end]).unwrap()
    }

    #[test]
    fn the_shell_has_the_reading_views_structure_and_the_page_script() {
        let document = render("a/Note.md", "tok", &page("<p>hi</p>"));
        for part in [
            "<div class=\"markdown-reading-view\" style=\"width: 100%; height: 100%;\">",
            "markdown-preview-view markdown-rendered",
            "<div class=\"markdown-preview-sizer markdown-preview-section\"",
            "href=\"/app/reading.css\"",
            "src=\"/app/vendor/idiomorph/idiomorph.min.js\"",
            "src=\"/app/reading.js\"",
            "<body class=\"theme-light\">",
        ] {
            assert!(document.contains(part), "{part}\n{document}");
        }
        assert!(document.contains("<title>A &lt;b&gt; &amp; note</title>"));
        // The rendering is data, not markup.
        assert!(!document.contains("<p>hi</p>"));
    }

    #[test]
    fn the_config_carries_the_rendering_and_what_the_page_needs() {
        let config = config(&render("a/Note.md", "tok", &page("<p>hi</p>")));
        assert_eq!(config["token"], "tok");
        assert_eq!(config["path"], "a/Note.md");
        assert_eq!(config["revision"], 7);
        assert_eq!(config["digest"], "abc");
        assert_eq!(config["html"], "<p>hi</p>");
        assert_eq!(config["cssclasses"], serde_json::json!(["wide"]));
        assert_eq!(config["theme"], "system");
        assert_eq!(config["mermaid"], false);
    }

    #[test]
    fn a_note_cannot_end_the_config_or_open_a_comment() {
        let hostile = "</script><script>alert(1)</script><!-- <script> & -->";
        let document = render("n.md", "tok", &page(hostile));
        assert_eq!(config(&document)["html"], hostile);
        let start = document.find("id=\"frame-notes-config\">").unwrap();
        let end = start + document[start..].find("</script>").unwrap();
        let body = &document[start..end];
        assert!(!body.contains("<script") && !body.contains("<!--"), "{body}");
        assert_eq!(document.matches("<script").count(), 3);
    }

    #[test]
    fn the_vaults_appearance_follows_the_default_theme_and_precedes_its_snippets() {
        let mut data = page("");
        data.appearance = Appearance {
            scheme: Scheme::Dark,
            accent: Some((248, 59, 64)),
            font_size: Some(18),
            snippets: vec!["my callouts".into(), "tweaks".into()],
        };
        let document = render("n.md", "tok", &data);
        assert!(document.contains("<body class=\"theme-dark\">"));
        assert_eq!(config(&document)["theme"], "dark");
        let style = document
            .find("<style>body{--accent-h:248;--accent-s:59%;--accent-l:64%;--font-text-size:18px;}</style>")
            .expect("the appearance");
        let theme = document.find("href=\"/app/reading.css\"").unwrap();
        let first = document.find("href=\"/snippet/my%20callouts.css\"").unwrap();
        let second = document.find("href=\"/snippet/tweaks.css\"").unwrap();
        assert!(theme < style && style < first && first < second);
    }

    #[test]
    fn a_default_appearance_adds_no_style() {
        assert!(!render("n.md", "tok", &page("")).contains("<style>"));
    }
}
