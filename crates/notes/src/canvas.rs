//! References from a `.canvas` file (JSON Canvas): the files its cards hold and the links in its text cards.
use crate::model::{Link, LinkSyntax};
use crate::offsets::Span;
use crate::parse::{fragment_of, parse};
use anyhow::{Context, Result};
use serde_json::Value;

/// A reference made by one card of a canvas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanvasLink {
    /// The card's id, for opening it.
    pub node: String,
    /// A file card is a `LinkSyntax::Path` link with no span. A text card's links are read as a note
    /// would be, with spans in the card's text.
    pub link: Link,
}

/// The references in the JSON of a canvas, in card order. Cards of any other kind, and fields of the wrong type,
/// are skipped, so a canvas written by a newer Obsidian still yields what it can. A file that is not
/// JSON is an error.
pub fn canvas_links(json: &str) -> Result<Vec<CanvasLink>> {
    let canvas: Value = serde_json::from_str(json).context("a canvas is JSON")?;
    let mut links = Vec::new();
    for card in canvas.get("nodes").and_then(Value::as_array).into_iter().flatten() {
        let node = card.get("id").and_then(Value::as_str).unwrap_or_default();
        let text = |key: &str| card.get(key).and_then(Value::as_str);
        match card.get("type").and_then(Value::as_str) {
            Some("file") => {
                let Some(file) = text("file").filter(|f| !f.is_empty()) else { continue };
                links.push(CanvasLink {
                    node: node.to_string(),
                    link: Link {
                        syntax: LinkSyntax::Path,
                        embed: false,
                        target: file.to_string(),
                        fragment: text("subpath").and_then(|s| fragment_of(s.trim_start_matches('#'))),
                        text: None,
                        span: Span::default(),
                        target_span: None,
                        in_frontmatter: false,
                    },
                });
            }
            Some("text") => {
                let note = parse(text("text").unwrap_or_default());
                links.extend(note.links.into_iter().map(|link| CanvasLink { node: node.to_string(), link }));
            }
            _ => {}
        }
    }
    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Fragment;
    use pretty_assertions::assert_eq;

    const CANVAS: &str = r##"{
	"nodes":[
		{"id":"a","type":"file","file":"notes/Plan.md","x":0,"y":0,"width":400,"height":400},
		{"id":"b","type":"file","file":"notes/Plan.md","subpath":"#Phase one","x":0,"y":0,"width":400,"height":400},
		{"id":"c","type":"file","file":"notes/Plan.md","subpath":"#^blk","x":0,"y":0,"width":400,"height":400},
		{"id":"d","type":"file","file":"img/diagram.png","x":0,"y":0,"width":400,"height":400},
		{"id":"e","type":"text","text":"See [[Ideas#Later|this]] and [spec](../specs/My%20Spec.md)\n\n![[diagram.png]] `[[code]]` https://example.com","x":0,"y":0,"width":250,"height":60},
		{"id":"f","type":"link","url":"https://example.com","x":0,"y":0,"width":250,"height":60},
		{"id":"g","type":"group","label":"[[Not a link]]","x":0,"y":0,"width":250,"height":60},
		{"id":"h","type":"file","file":5},
		{"id":"i","type":"file"},
		{"id":"j","type":"future-kind","file":"x.md"},
		{"id":"k","type":"text","text":7}
	],
	"edges":[{"id":"x","fromNode":"a","toNode":"b"}]
}"##;

    #[test]
    fn file_cards_are_paths_with_their_subpath() {
        let links = canvas_links(CANVAS).unwrap();
        let files: Vec<_> = links
            .iter()
            .filter(|l| l.link.syntax == LinkSyntax::Path)
            .map(|l| (l.node.as_str(), l.link.target.as_str(), l.link.fragment.clone()))
            .collect();
        assert_eq!(
            files,
            [
                ("a", "notes/Plan.md", None),
                ("b", "notes/Plan.md", Some(Fragment::Heading("Phase one".into()))),
                ("c", "notes/Plan.md", Some(Fragment::Block("blk".into()))),
                ("d", "img/diagram.png", None),
            ]
        );
    }

    #[test]
    fn text_cards_hold_links_like_a_note() {
        let links = canvas_links(CANVAS).unwrap();
        let text: Vec<_> = links.iter().filter(|l| l.node == "e").collect();
        let got: Vec<_> = text.iter().map(|l| (l.link.syntax, l.link.embed, l.link.target.as_str())).collect();
        assert_eq!(
            got,
            [
                (LinkSyntax::Wiki, false, "Ideas"),
                (LinkSyntax::Markdown, false, "../specs/My Spec.md"),
                (LinkSyntax::Wiki, true, "diagram.png"),
            ]
        );
        // Spans are in the card's own text.
        let card = "See [[Ideas#Later|this]] and [spec](../specs/My%20Spec.md)\n\n![[diagram.png]] `[[code]]` https://example.com";
        assert_eq!(text[0].link.span.slice(card), "[[Ideas#Later|this]]");
        assert_eq!(text[2].link.span.slice(card), "![[diagram.png]]");
    }

    #[test]
    fn other_cards_and_odd_fields_contribute_nothing() {
        let links = canvas_links(CANVAS).unwrap();
        let nodes: Vec<_> = links.iter().map(|l| l.node.as_str()).collect();
        assert_eq!(nodes, ["a", "b", "c", "d", "e", "e", "e"]);
    }

    #[test]
    fn an_empty_or_unfamiliar_canvas_has_no_links_and_a_non_canvas_is_an_error() {
        assert_eq!(canvas_links("{}").unwrap(), []);
        assert_eq!(canvas_links(r#"{"nodes":"none","edges":[]}"#).unwrap(), []);
        assert_eq!(canvas_links(r#"{"nodes":[null,1,[],{}]}"#).unwrap(), []);
        assert!(canvas_links("").is_err());
        assert!(canvas_links("{").is_err());
    }
}
