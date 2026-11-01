//! The prose of a markdown note.
//!
//! A note is markdown with Obsidian extensions, and most of its bytes are not
//! prose: code, link destinations, HTML, front matter, wikilink targets, tags,
//! `%%comments%%`, URLs, identifiers. Checking them as English would underline
//! `snake_case` and every URL. [`prose_view`] returns the same text with all
//! of that blanked out, so a checker sees only what a reader reads.
//!
//! The view has exactly the byte length of the source, and every byte of prose
//! sits at its original offset. That is the whole design: a diagnostic's range
//! in the view is its range in the note, with no map to keep in step with an
//! edit. A character that is not prose becomes spaces of the same byte length
//! and a newline stays a newline, so sentence and paragraph boundaries survive.

use std::{ops::Range, sync::OnceLock};

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
use regex::Regex;

/// Markdown dialect of a note: what is prose is decided by the same syntax
/// the note is written in.
fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH
        | Options::ENABLE_WIKILINKS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
}

/// Not worth underlining wherever it appears in running text.
fn noise() -> &'static Regex {
    static NOISE: OnceLock<Regex> = OnceLock::new();
    NOISE.get_or_init(|| {
        Regex::new(
            r"(?xsm)
              %%.*?%%                                   # Obsidian comment
            | https?://\S+ | www\.\S+                    # URL
            | [\w.+-]+@[\w-]+(?:\.[\w-]+)+               # email address
            | (?:^|\s)\#[\w/-]+                          # tag
            | \S*[_/\\]\S*                               # path, snake_case, and/or
            | \S*::\S*                                   # Rust path
            | \b\w*[a-z][A-Z]\w*\b                       # camelCase, iPhone, RedCore
            | \b\w*\d\w*\b                               # v2, 3rd, SHA256
            ",
        )
        .expect("noise pattern is valid")
    })
}

/// The note with everything that is not prose blanked out; same byte length.
#[must_use]
pub fn prose_view(source: &str) -> String {
    let mut view: Vec<u8> = Vec::with_capacity(source.len());
    for c in source.chars() {
        if c == '\n' {
            view.push(b'\n');
        } else {
            view.extend(std::iter::repeat_n(b' ', c.len_utf8()));
        }
    }

    // A container whose text is not prose: counted while inside it.
    let mut stack: Vec<bool> = Vec::new();
    let mut skipping = 0usize;
    for (event, range) in Parser::new_ext(source, options()).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                let skip = matches!(
                    tag,
                    Tag::CodeBlock(_)
                        | Tag::MetadataBlock(_)
                        | Tag::HtmlBlock
                        | Tag::Image { .. }
                        | Tag::Link {
                            link_type: LinkType::WikiLink { .. },
                            ..
                        }
                );
                skipping += usize::from(skip);
                stack.push(skip);
            }
            // Every `End` closes the `Start` pushed for it.
            Event::End(_) => {
                if stack.pop().unwrap_or(false) {
                    skipping -= 1;
                }
            }
            Event::Text(text) if skipping == 0 => copy_if_verbatim(&mut view, source, &range, &text),
            _ => {}
        }
    }

    // `view` was built from ASCII spaces and ranges of `source` that were
    // checked to be whole slices, so it is valid UTF-8.
    let mut view = String::from_utf8(view).expect("prose view is valid UTF-8");
    blank_noise(&mut view);
    view
}

/// Copy `source[range]` into the view when it is exactly the text the parser
/// produced. An escape (`\*`) or an entity (`&amp;`) produces text that is not
/// the bytes it came from; those are left blank rather than guessed at.
fn copy_if_verbatim(view: &mut [u8], source: &str, range: &Range<usize>, text: &str) {
    if let Some(original) = source.get(range.clone()) {
        if original == text {
            view[range.clone()].copy_from_slice(original.as_bytes());
        }
    }
}

fn blank_noise(view: &mut String) {
    let ranges: Vec<Range<usize>> = noise().find_iter(view).map(|m| m.range()).collect();
    if ranges.is_empty() {
        return;
    }
    let mut bytes = std::mem::take(view).into_bytes();
    for range in ranges {
        for byte in &mut bytes[range] {
            // Keep line structure; a match may contain a leading space, which
            // stays one, and multi-byte characters become several spaces.
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    }
    *view = String::from_utf8(bytes).expect("blanking ASCII keeps UTF-8 valid");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The words a checker would see, in order.
    fn words(view: &str) -> Vec<&str> {
        view.split_whitespace()
            .filter(|word| word.chars().any(char::is_alphabetic))
            .collect()
    }

    #[test]
    fn view_has_the_length_of_the_source_and_prose_at_the_same_offsets() {
        let source = "# Title\n\nSome *emphasised* prose with `code` and a [link](https://example.com/x).\n";
        let view = prose_view(source);
        assert_eq!(view.len(), source.len());
        for word in ["Title", "Some", "emphasised", "prose", "with", "and", "link"] {
            let at = source.find(word).unwrap();
            assert_eq!(&view[at..at + word.len()], word, "{word} moved");
        }
        assert_eq!(words(&view), ["Title", "Some", "emphasised", "prose", "with", "and", "a", "link"]);
    }

    #[test]
    fn code_html_front_matter_and_links_are_not_prose() {
        let source = "---\ntitle: Tehe\n---\n\nreal text\n\n```rust\nlet teh = 1;\n```\n\n    indented teh\n\n<div>htmlteh</div>\n\n![alt teh](img.png)\n\nafter [label](http://x.test/teh)\n";
        let view = prose_view(source);
        assert!(!view.contains("Tehe"), "front matter leaked: {view:?}");
        assert!(!view.contains("teh"), "code, html, image or url leaked: {view:?}");
        assert!(view.contains("real text"));
        assert_eq!(words(&view), ["real", "text", "after", "label"], "{view:?}");
    }

    #[test]
    fn wikilinks_tags_comments_urls_and_identifiers_are_not_prose() {
        let source = "See [[Some Note#Heading|the alias]] and #project/tag then %%hidden teh%% done https://example.com/teh?x=1 or me@example.com in snake_case or camelCase v2.\n";
        let view = prose_view(source);
        for leaked in ["Some", "alias", "project", "hidden", "example", "snake", "camel", "v2"] {
            assert!(!view.contains(leaked), "{leaked} leaked: {view:?}");
        }
        assert_eq!(words(&view), ["See", "and", "then", "done", "or", "in", "or"]);
    }

    #[test]
    fn escapes_and_entities_are_blanked_not_guessed() {
        let source = "a \\*star\\* and AT&amp;T end\n";
        let view = prose_view(source);
        assert_eq!(view.len(), source.len());
        assert!(!view.contains("&amp;"));
        assert!(view.contains("end"));
    }

    #[test]
    fn multibyte_text_keeps_offsets() {
        let source = "caf\u{e9} \u{1f600} na\u{ef}ve `x` teh\n";
        let view = prose_view(source);
        assert_eq!(view.len(), source.len());
        let at = source.find("teh").unwrap();
        assert_eq!(&view[at..at + 3], "teh");
        assert!(view.is_char_boundary(at));
    }

    #[test]
    fn plain_text_passes_through_untouched() {
        let source = "Just a plain sentence.\nAnd another one.\n";
        assert_eq!(prose_view(source), source);
    }

    #[test]
    fn unterminated_constructs_do_not_panic() {
        for source in ["```\nopen fence", "[[open", "%% open", "*", "<div", "---\nno end", "[x](", ""] {
            let view = prose_view(source);
            assert_eq!(view.len(), source.len(), "{source:?}");
        }
    }
}
