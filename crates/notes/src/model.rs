//! What a note contains: the records the index writes into the graph.
use crate::offsets::Span;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Everything the index keeps about one Markdown note, in document order. Offsets are UTF-8 bytes
/// of the text that was parsed; `Offsets` converts them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub frontmatter: Option<Frontmatter>,
    pub headings: Vec<Heading>,
    pub blocks: Vec<Block>,
    pub links: Vec<Link>,
    pub tags: Vec<Tag>,
    pub tasks: Vec<Task>,
    pub footnotes: Vec<Footnote>,
    pub fields: Vec<InlineField>,
    pub callouts: Vec<Callout>,
    pub highlights: Vec<Highlight>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frontmatter {
    /// The block including both `---` lines.
    pub span: Span,
    /// In file order. Empty when `error` is set.
    pub properties: IndexMap<String, Value>,
    /// Why the YAML is not a map of properties.
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    pub level: u8,
    /// The heading's source text, as a link would name it.
    pub text: String,
    pub span: Span,
}

/// A `^id` that other notes can link to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    /// What the id names: a paragraph, a list item, or the block before a line holding only the id.
    pub span: Span,
    pub id_span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkSyntax {
    /// `[[Note]]` and `![[Note]]`.
    Wiki,
    /// `[text](Note.md)` and `![alt](image.png)`.
    Markdown,
    /// A vault path written exactly, as a canvas file card holds it.
    Path,
}

/// The part after `#`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fragment {
    /// `#Heading` or `#H1#H2`, kept whole: a heading may hold `#` itself (`C# basics`).
    Heading(String),
    /// `#^id`, without the caret.
    Block(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub syntax: LinkSyntax,
    pub embed: bool,
    /// The note, path or attachment named, without `#…` and, for Markdown, percent-decoding
    /// undone. Empty for a link to a heading or block of the same note.
    pub target: String,
    pub fragment: Option<Fragment>,
    /// The alias after `|` (an embed's size or caption), or a Markdown link's text.
    pub text: Option<String>,
    /// The whole link, brackets included.
    pub span: Span,
    /// The target as written, without `#…`, where it can be located.
    pub target_span: Option<Span>,
    /// Written in the YAML of the properties.
    pub in_frontmatter: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TagSource {
    Body,
    Frontmatter,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    /// Without the `#`, in the case written. Tags compare case-insensitively (`key`).
    pub name: String,
    pub source: TagSource,
    pub span: Span,
}

impl Tag {
    pub fn key(&self) -> String {
        self.name.to_lowercase()
    }

    /// `a/b/c` is also tagged `a/b` and `a`: this tag's parents, nearest first.
    pub fn parents(&self) -> impl Iterator<Item = &str> {
        let name = self.name.as_str();
        name.match_indices('/').rev().map(move |(i, _)| &name[..i])
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// The character between the brackets; a space when unchecked.
    pub status: char,
    /// The item's first paragraph, as written.
    pub text: String,
    /// The list item, nested items included.
    pub span: Span,
    /// The status character, where a toggle writes.
    pub status_span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FootnoteKind {
    Reference,
    Definition,
    /// `^[text]`, a reference with its text in place.
    Inline,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footnote {
    pub name: String,
    pub kind: FootnoteKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldStyle {
    /// `key:: value` filling a line.
    Line,
    /// `[key:: value]`
    Bracket,
    /// `(key:: value)`
    Paren,
}

/// A Dataview inline field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineField {
    pub key: String,
    /// As written; typing it is the index's business.
    pub value: String,
    pub style: FieldStyle,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fold {
    None,
    Expanded,
    Collapsed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Callout {
    /// Lowercased: `note` for `[!Note]`.
    pub kind: String,
    /// The custom title as written.
    pub title: Option<String>,
    pub fold: Fold,
    /// What follows `|` in `[!kind|metadata]`.
    pub metadata: Option<String>,
    pub span: Span,
}

/// The six colours Obsidian 1.14 stores as a coloured circle at the start of a highlight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HighlightColor {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
}

impl HighlightColor {
    pub fn from_emoji(c: char) -> Option<Self> {
        Some(match c {
            '\u{1F534}' => Self::Red,
            '\u{1F7E0}' => Self::Orange,
            '\u{1F7E1}' => Self::Yellow,
            '\u{1F7E2}' => Self::Green,
            '\u{1F535}' => Self::Blue,
            '\u{1F7E3}' => Self::Purple,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Highlight {
    /// Between the `==` markers, colour emoji included.
    pub text: String,
    pub color: Option<HighlightColor>,
    pub span: Span,
}

/// The types Obsidian gives properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyType {
    Text,
    List,
    Number,
    Checkbox,
    Date,
    DateTime,
    Tags,
    Aliases,
}
