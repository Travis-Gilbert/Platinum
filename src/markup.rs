//! Search titles and snippets arrive as HTML fragments: `<strong>` around the
//! words that matched, entities, now and then a stray tag. Nothing here
//! renders markup. html5ever parses a fragment the way a browser would, and
//! only its text comes out, with the emphasized ranges beside it, for the SERP
//! to draw as weight. Every other element is dropped, scripts and styles with
//! their contents.

use std::ops::Range;

use html5ever::tendril::TendrilSink as _;
use html5ever::{ParseOpts, QualName, local_name, namespace_url, ns, parse_fragment};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

/// Plain text, and the byte ranges of it the source emphasized.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Marked {
    pub text: String,
    pub emphasis: Vec<Range<usize>>,
}

/// A fragment's text, its whitespace collapsed to single spaces.
pub fn parse(fragment: &str) -> Marked {
    let mut writer = Writer::default();
    if fragment.contains(['<', '&']) {
        let dom = parse_fragment(
            RcDom::default(),
            ParseOpts::default(),
            QualName::new(None, ns!(html), local_name!("body")),
            Vec::new(),
        )
        .one(fragment);
        walk(&dom.document, false, &mut writer);
    } else {
        writer.push(fragment, false);
    }
    writer.marked
}

fn walk(node: &Handle, emphasized: bool, writer: &mut Writer) {
    match &node.data {
        NodeData::Text { contents } => writer.push(&contents.borrow(), emphasized),
        NodeData::Element { name, .. } => {
            let emphasized = emphasized
                || matches!(
                    name.local,
                    local_name!("strong")
                        | local_name!("b")
                        | local_name!("em")
                        | local_name!("i")
                        | local_name!("mark")
                );
            let breaks = match name.local {
                local_name!("script")
                | local_name!("style")
                | local_name!("template")
                | local_name!("title")
                | local_name!("iframe")
                | local_name!("noscript")
                | local_name!("noembed")
                | local_name!("noframes") => return,
                local_name!("br")
                | local_name!("p")
                | local_name!("div")
                | local_name!("li")
                | local_name!("tr")
                | local_name!("td")
                | local_name!("h1")
                | local_name!("h2")
                | local_name!("h3")
                | local_name!("h4")
                | local_name!("h5")
                | local_name!("h6") => true,
                _ => false,
            };
            writer.space |= breaks;
            for child in node.children.borrow().iter() {
                walk(child, emphasized, writer);
            }
            writer.space |= breaks;
        }
        NodeData::Document => {
            for child in node.children.borrow().iter() {
                walk(child, emphasized, writer);
            }
        }
        // Comments, doctypes and processing instructions say nothing.
        _ => {}
    }
}

#[derive(Default)]
struct Writer {
    marked: Marked,
    /// Whitespace since the last character: one space, written before the
    /// next one, so none leads or trails.
    space: bool,
}

impl Writer {
    fn push(&mut self, text: &str, emphasized: bool) {
        for character in text.chars() {
            if character.is_whitespace() {
                self.space = true;
                continue;
            }
            if std::mem::take(&mut self.space) && !self.marked.text.is_empty() {
                self.marked.text.push(' ');
            }
            let start = self.marked.text.len();
            self.marked.text.push(character);
            if !emphasized {
                continue;
            }
            let end = self.marked.text.len();
            let text = &self.marked.text;
            match self.marked.emphasis.last_mut() {
                // Emphasized words a space apart read as one run.
                Some(last) if text[last.end..start].trim().is_empty() => last.end = end,
                _ => self.marked.emphasis.push(start..end),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emphasized(marked: &Marked) -> Vec<&str> {
        marked
            .emphasis
            .iter()
            .map(|range| &marked.text[range.clone()])
            .collect()
    }

    #[test]
    fn strong_becomes_emphasis_and_entities_decode() {
        let marked = parse("<strong>Rust</strong> &amp; GPUI");
        assert_eq!(marked.text, "Rust & GPUI");
        assert_eq!(emphasized(&marked), ["Rust"]);
    }

    #[test]
    fn neighbouring_emphasis_joins_across_a_space() {
        let marked = parse("a <b>native</b> <em>browser</em> chrome");
        assert_eq!(marked.text, "a native browser chrome");
        assert_eq!(emphasized(&marked), ["native browser"]);
        let marked = parse("<b>a</b>x<b>b</b>");
        assert_eq!(emphasized(&marked), ["a", "b"]);
    }

    #[test]
    fn scripts_styles_and_unknown_elements_leave_no_trace() {
        let marked = parse(
            "<script>alert(1)</script><style>p{}</style>\
             <img src=x onerror=alert(1)>Safe <span class=x>text</span>",
        );
        assert_eq!(marked.text, "Safe text");
        assert!(marked.emphasis.is_empty());
    }

    #[test]
    fn escaped_markup_stays_text() {
        assert_eq!(parse("5 &lt; 6 &lt;b&gt;").text, "5 < 6 <b>");
    }

    #[test]
    fn an_unclosed_tag_still_emphasizes_what_follows() {
        let marked = parse("see <b>this");
        assert_eq!(marked.text, "see this");
        assert_eq!(emphasized(&marked), ["this"]);
    }

    #[test]
    fn whitespace_and_breaks_collapse_to_single_spaces() {
        assert_eq!(parse("  one\n\ttwo  ").text, "one two");
        assert_eq!(parse("one<br>two<p>three</p>").text, "one two three");
    }

    #[test]
    fn multibyte_text_keeps_its_ranges_on_character_boundaries() {
        let marked = parse("café <b>über</b>");
        assert_eq!(marked.text, "café über");
        assert_eq!(emphasized(&marked), ["über"]);
    }
}
