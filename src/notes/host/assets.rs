//! The reading view's own files (`assets/notes/`): the page script, its
//! stylesheet and the vendored libraries, embedded in the binary. Nothing the
//! page runs comes from anywhere else.
//!
//! SOURCING: `rust-embed`, as GPUI Kit's icons are embedded; the content types
//! are named here because the page's scripts, styles and fonts must have the
//! right one (a script served as text is refused by the page).

use std::borrow::Cow;

use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets/notes/"]
#[exclude = "*.DS_Store"]
struct Bundle;

/// One bundled file.
pub struct Asset {
    pub bytes: Cow<'static, [u8]>,
    pub content_type: &'static str,
}

/// The bundled file at `path`, relative to `assets/notes/`. The embedded
/// table is looked up by name, so a path that is not in it names nothing;
/// `route` has already refused the ones that could name more.
pub fn get(path: &str) -> Option<Asset> {
    let file = Bundle::get(path)?;
    Some(Asset {
        content_type: content_type(path)?,
        bytes: file.data,
    })
}

/// The type of a bundled file by extension. A file of a kind the page never
/// loads has none, and is not served.
fn content_type(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let Some((_, extension)) = name.rsplit_once('.') else {
        // LICENSE and SOURCE.
        return Some("text/plain; charset=utf-8");
    };
    Some(match extension.to_ascii_lowercase().as_str() {
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "svg" => "image/svg+xml",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_script_and_style_are_served_with_their_types() {
        let script = get("reading.js").expect("the page script is bundled");
        assert_eq!(script.content_type, "text/javascript; charset=utf-8");
        assert!(script.bytes.starts_with(b"/* The reading view's page script"));
        assert_eq!(
            get("reading.css").unwrap().content_type,
            "text/css; charset=utf-8"
        );
    }

    #[test]
    fn every_vendored_library_is_bundled_with_its_licence() {
        for (library, entry) in [
            ("idiomorph", "idiomorph.min.js"),
            ("prismjs", "components/prism-core.min.js"),
            ("prismjs", "plugins/prism-autoloader.min.js"),
            ("mathjax", "tex-chtml.js"),
            ("mermaid", "mermaid.min.js"),
        ] {
            let script = get(&format!("vendor/{library}/{entry}"))
                .unwrap_or_else(|| panic!("{library}/{entry} is not bundled"));
            assert_eq!(script.content_type, "text/javascript; charset=utf-8");
            for file in ["LICENSE", "SOURCE"] {
                assert!(
                    get(&format!("vendor/{library}/{file}")).is_some(),
                    "{library} has no {file}"
                );
            }
        }
    }

    #[test]
    fn mathjax_fonts_are_fonts() {
        let font = Bundle::iter()
            .find(|name| name.starts_with("vendor/mathjax/font/") && name.ends_with(".woff2"))
            .expect("a MathJax font is bundled");
        assert_eq!(get(&font).unwrap().content_type, "font/woff2");
    }

    #[test]
    fn what_is_not_bundled_or_not_a_kind_of_file_the_page_loads_is_not_served() {
        assert!(get("nothing.js").is_none());
        assert!(get("../Cargo.toml").is_none());
        assert!(get("/reading.js").is_none());
        assert_eq!(content_type("a/b.exe"), None);
        assert_eq!(content_type("a/b.html"), None);
    }
}
