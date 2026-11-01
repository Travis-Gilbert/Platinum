//! The document a note's `frame-notes:` address serves: Obsidian's reading
//! view structure, the vault's appearance, and the note's rendering as data.
//!
//! The rendering is not placed in the markup. It travels as JSON in a
//! `<script type="application/json">`, which no browser runs, and the page
//! script sanitizes it and morphs it in, exactly as it does a patch. A note's
//! markup can therefore never be parsed as part of the shell, so it cannot end
//! the shell's own tags or smuggle in an element of the shell's.

use gpui_kit::Hsla;
use gpui_kit::component::Theme;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::json;

use super::ops::{Appearance, PageData, Scheme};

/// Obsidian's classes on the reading view, for the settings Frame has: readable
/// line width, folding of headings and lists, indentation guides, properties.
const VIEW_CLASSES: &str = "markdown-preview-view markdown-rendered node-insert-event is-readable-line-width allow-fold-headings allow-fold-lists show-indentation-guide show-properties";

/// The colors of the app's active theme that the page takes, so the page and
/// the chrome around it agree. They reach the page as Platinum's own custom
/// properties (`--frame-*`), which `reading.css` reads with a fallback: the
/// Obsidian names are the default theme's and the vault's to set, and a
/// vault's snippet still overrides them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chrome {
    dark: bool,
    background: Hsla,
    panel: Hsla,
    text: Hsla,
    border: Hsla,
    accent: Hsla,
}

impl Chrome {
    pub fn from_theme(theme: &Theme) -> Self {
        Self {
            dark: theme.is_dark(),
            background: theme.background,
            panel: theme.sidebar,
            text: theme.foreground,
            border: theme.border,
            accent: theme.primary,
        }
    }

    pub fn is_dark(&self) -> bool {
        self.dark
    }

    /// The `:root` rule that sets the properties. Every value is made of
    /// numbers, so nothing in it can close the `<style>` element it sits in.
    pub fn root_rule(&self) -> String {
        let accent = self.accent;
        format!(
            ":root{{--frame-background-primary:{};--frame-background-secondary:{};--frame-text-normal:{};--frame-border:{};--frame-accent-h:{:.0};--frame-accent-s:{:.0}%;--frame-accent-l:{:.0}%}}",
            css_color(self.background),
            css_color(self.panel),
            css_color(self.text),
            css_color(self.border),
            degrees(accent.h),
            percent(accent.s),
            percent(accent.l),
        )
    }

    /// The script that gives a loaded page this theme.
    pub fn script(&self) -> String {
        format!(
            "frameNotes.chrome({})",
            json!({ "css": self.root_rule(), "dark": self.dark })
        )
    }
}

/// A finite number: a color that is not one is drawn as zero, not as `NaN`.
fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn degrees(hue: f32) -> f32 {
    finite(hue).rem_euclid(1.0) * 360.0
}

fn percent(fraction: f32) -> f32 {
    finite(fraction).clamp(0.0, 1.0) * 100.0
}

fn css_color(color: Hsla) -> String {
    format!(
        "hsla({:.1},{:.1}%,{:.1}%,{:.3})",
        degrees(color.h),
        percent(color.s),
        percent(color.l),
        finite(color.a).clamp(0.0, 1.0),
    )
}

/// The page for the note at `path`, to be loaded under `token`. `chrome` is
/// the app's theme, when there is one to follow.
pub fn render(path: &str, token: &str, page: &PageData, chrome: Option<&Chrome>) -> String {
    let appearance = &page.appearance;
    let theme = match appearance.scheme {
        Scheme::Light => "light",
        Scheme::Dark => "dark",
        Scheme::System => "system",
    };
    // The vault's own scheme wins; a vault that follows the system follows the
    // app's theme when there is one (the page script applies the same rule).
    let dark = match appearance.scheme {
        Scheme::Dark => true,
        Scheme::Light => false,
        Scheme::System => chrome.is_some_and(Chrome::is_dark),
    };
    let config = json!({
        "token": token,
        "path": path,
        "revision": page.rendered.revision,
        "digest": page.rendered.digest,
        "theme": theme,
        "chrome": chrome.map(|chrome| if chrome.is_dark() { "dark" } else { "light" }),
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
<link rel=\"stylesheet\" href=\"/app/reading.css\">{chrome}{appearance}{snippets}</head>\
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
        chrome = chrome
            .map(|chrome| format!(
                "<style id=\"frame-notes-chrome\">{}</style>",
                chrome.root_rule()
            ))
            .unwrap_or_default(),
        appearance = appearance_style(appearance),
        scheme = if dark { "theme-dark" } else { "theme-light" },
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
impl Chrome {
    /// A theme whose colors are exact in binary, so their text is exact.
    pub(super) fn sample(dark: bool) -> Self {
        Self {
            dark,
            background: Hsla {
                h: 0.5,
                s: 0.25,
                l: 0.5,
                a: 1.0,
            },
            panel: Hsla {
                h: 0.25,
                s: 0.5,
                l: 0.125,
                a: 1.0,
            },
            text: Hsla {
                h: 0.0,
                s: 0.0,
                l: 0.875,
                a: 1.0,
            },
            border: Hsla {
                h: 0.75,
                s: 1.0,
                l: 0.5,
                a: 0.5,
            },
            accent: Hsla {
                h: 0.72,
                s: 0.6,
                l: 0.64,
                a: 1.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ops::Rendered;
    use super::*;

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
        let document = render("a/Note.md", "tok", &page("<p>hi</p>"), None);
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
        let config = config(&render("a/Note.md", "tok", &page("<p>hi</p>"), None));
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
        let document = render("n.md", "tok", &page(hostile), None);
        assert_eq!(config(&document)["html"], hostile);
        let start = document.find("id=\"frame-notes-config\">").unwrap();
        let end = start + document[start..].find("</script>").unwrap();
        let body = &document[start..end];
        assert!(
            !body.contains("<script") && !body.contains("<!--"),
            "{body}"
        );
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
        let document = render("n.md", "tok", &data, None);
        assert!(document.contains("<body class=\"theme-dark\">"));
        assert_eq!(config(&document)["theme"], "dark");
        let style = document
            .find("<style>body{--accent-h:248;--accent-s:59%;--accent-l:64%;--font-text-size:18px;}</style>")
            .expect("the appearance");
        let theme = document.find("href=\"/app/reading.css\"").unwrap();
        let first = document
            .find("href=\"/snippet/my%20callouts.css\"")
            .unwrap();
        let second = document.find("href=\"/snippet/tweaks.css\"").unwrap();
        assert!(theme < style && style < first && first < second);
    }

    #[test]
    fn a_default_appearance_adds_no_style() {
        assert!(!render("n.md", "tok", &page(""), None).contains("<style"));
    }

    #[test]
    fn the_shell_carries_the_app_themes_colors_as_platinums_own_properties() {
        let document = render("n.md", "tok", &page(""), Some(&Chrome::sample(true)));
        let rule = ":root{--frame-background-primary:hsla(180.0,25.0%,50.0%,1.000);\
--frame-background-secondary:hsla(90.0,50.0%,12.5%,1.000);\
--frame-text-normal:hsla(0.0,0.0%,87.5%,1.000);\
--frame-border:hsla(270.0,100.0%,50.0%,0.500);\
--frame-accent-h:259;--frame-accent-s:60%;--frame-accent-l:64%}";
        assert!(
            document.contains(&format!("<style id=\"frame-notes-chrome\">{rule}</style>")),
            "{document}"
        );
        // Not Obsidian's names: those are the default theme's and the vault's.
        assert!(!rule.contains("--background-primary") && !rule.contains("--accent-h"));
    }

    #[test]
    fn the_apps_colors_precede_the_vaults_appearance_and_snippets() {
        let mut data = page("");
        data.appearance = Appearance {
            scheme: Scheme::System,
            accent: Some((248, 59, 64)),
            font_size: None,
            snippets: vec!["tweaks".into()],
        };
        let document = render("n.md", "tok", &data, Some(&Chrome::sample(false)));
        let theme = document.find("href=\"/app/reading.css\"").unwrap();
        let app = document.find("<style id=\"frame-notes-chrome\">").unwrap();
        let vault = document.find("<style>body{--accent-h:248").unwrap();
        let snippet = document.find("href=\"/snippet/tweaks.css\"").unwrap();
        assert!(theme < app && app < vault && vault < snippet);
    }

    #[test]
    fn a_vault_that_follows_the_system_follows_the_apps_theme() {
        let dark = render("n.md", "tok", &page(""), Some(&Chrome::sample(true)));
        assert!(dark.contains("<body class=\"theme-dark\">"));
        assert_eq!(config(&dark)["chrome"], "dark");
        let light = render("n.md", "tok", &page(""), Some(&Chrome::sample(false)));
        assert!(light.contains("<body class=\"theme-light\">"));
        assert_eq!(config(&light)["chrome"], "light");
        // Without an app theme there is nothing to follow.
        assert_eq!(
            config(&render("n.md", "tok", &page(""), None))["chrome"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn a_vault_that_asks_for_a_scheme_keeps_it() {
        let mut data = page("");
        data.appearance.scheme = Scheme::Light;
        let document = render("n.md", "tok", &data, Some(&Chrome::sample(true)));
        assert!(document.contains("<body class=\"theme-light\">"));
        assert_eq!(config(&document)["theme"], "light");
    }

    #[test]
    fn a_color_that_is_not_a_number_cannot_break_the_style() {
        let mut odd = Chrome::sample(true);
        odd.background = Hsla {
            h: f32::NAN,
            s: f32::INFINITY,
            l: -3.0,
            a: 7.0,
        };
        let rule = odd.root_rule();
        assert!(
            rule.contains("--frame-background-primary:hsla(0.0,0.0%,0.0%,1.000)"),
            "{rule}"
        );
        assert!(!rule.contains('<'));
        assert_eq!(rule.matches('}').count(), 1);
    }

    #[test]
    fn the_script_that_themes_a_loaded_page_carries_the_rule_as_data() {
        let script = Chrome::sample(true).script();
        assert!(script.starts_with("frameNotes.chrome({"));
        assert!(script.contains("\"dark\":true") && script.contains("--frame-accent-h:259"));
    }
}
