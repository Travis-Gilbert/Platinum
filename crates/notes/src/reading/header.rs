//! The reading view's header: the inline title and the properties, as Obsidian draws them.
//!
//! Obsidian's properties are an editor; the markup here is read-only: no `contenteditable`,
//! `tabindex` or `enterkeyhint`, and the values Obsidian sets as DOM properties after it builds
//! the editor (`input.value`, `input.checked`) are written as attributes so they show.
use super::{Doc, html, icons::icon};
use crate::{
    Fragment, Link, LinkSyntax, PropertyType, Target, frontmatter, parse::valid_tag, parse_note,
    stem,
};
use indexmap::IndexMap;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

/// What the inline title shows.
pub(super) enum Title {
    /// The note's name.
    Note,
    /// The heading an embed of `Note#Heading` shows.
    Heading { level: u8, text: String },
}

pub(super) fn header(doc: &Doc, title: Title) -> String {
    let embedded = doc.depth > 0;
    let mut out = String::from(r#"<div class="mod-header mod-ui">"#);
    match &title {
        Title::Note => out.push_str(&format!(
            r#"<div class="inline-title">{}</div>"#,
            html::text(title_of(doc.path))
        )),
        Title::Heading { level, text } => out.push_str(&format!(
            r#"<div class="inline-title" data-level="{level}">{}</div>"#,
            html::text(text)
        )),
    }
    // An embedded heading shows no properties: they belong to the note, not to its section.
    out.push_str(&properties(doc, embedded, matches!(title, Title::Note)));
    out.push_str("</div>");
    out
}

/// A note's name as Obsidian shows it: the file's name without its folders or `.md`.
pub(super) fn title_of(path: &str) -> &str {
    stem(path.rsplit('/').next().unwrap_or(path))
}

/// The `cssclasses` property (and the older `cssclass`), as a list whether written as one or as text.
pub(super) fn cssclasses(doc: &Doc) -> Vec<String> {
    doc.note
        .frontmatter
        .as_ref()
        .map(|fm| fm.css_classes())
        .unwrap_or_default()
}

fn properties(doc: &Doc, embedded: bool, of_note: bool) -> String {
    let fm = doc.note.frontmatter.as_ref().filter(|_| of_note);
    let broken = fm.is_some_and(|f| f.error.is_some());
    let count = fm.map_or(0, |f| f.properties.len());
    let collapsed = if embedded { " is-collapsed" } else { "" };
    let mut out = format!(
        r#"<div class="metadata-container{}{collapsed}" data-property-count="{count}">"#,
        if broken { " mod-error" } else { "" }
    );
    out.push_str(&if broken {
        String::from(
            r#"<div class="metadata-error-container" style=""><div class="metadata-error-title">Invalid properties</div><div class="text-icon-button metadata-show-source-button"><span class="text-button-label">Show source</span></div></div>"#,
        )
    } else {
        String::from(r#"<div class="metadata-error-container" style="display: none;"></div>"#)
    });
    let hidden = if broken {
        r#" style="display: none;""#
    } else {
        ""
    };
    out.push_str(&format!(
        r#"<div class="metadata-properties-heading{collapsed}"{hidden}><div class="collapse-indicator collapse-icon{collapsed}">{}</div><div class="metadata-properties-title">Properties</div></div>"#,
        icon("right-triangle")
    ));
    out.push_str(&format!(
        r#"<div class="metadata-content"{}>"#,
        if embedded {
            r#" style="display: none;""#
        } else {
            ""
        }
    ));
    out.push_str(r#"<div class="metadata-properties">"#);
    if let Some(fm) = fm {
        for (key, value) in &fm.properties {
            out.push_str(&property(doc, key, value));
        }
    }
    out.push_str("</div>");
    out.push_str(&format!(
        r#"<div class="metadata-add-button text-icon-button"{hidden}><span class="text-button-icon">{}</span><span class="text-button-label">Add property</span></div></div></div>"#,
        icon("lucide-plus")
    ));
    out
}

fn property(doc: &Doc, key: &str, value: &Value) -> String {
    let declared = doc.cx.settings.property_type(key, value);
    let shown = if fits(declared, value) {
        declared
    } else if value.is_object() {
        PropertyType::Unknown
    } else {
        PropertyType::Text
    };
    let key_attr = html::attr(key);
    let mismatch = shown != declared && declared != PropertyType::Unknown;
    format!(
        r#"<div class="metadata-property" data-property-key="{key_attr}"><div class="metadata-property-key"><span class="metadata-property-icon">{}</span><input class="metadata-property-key-input" type="text" value="{key_attr}" aria-label="{key_attr}"></div><div class="metadata-property-value" data-property-type="{}">{}</div><div class="clickable-icon metadata-property-warning-icon" aria-label="Type mismatch, expected {}"{}>{}</div></div>"#,
        icon(type_icon(declared)),
        type_name(shown),
        widget(doc, key, shown, value),
        type_label(declared),
        if mismatch {
            ""
        } else {
            r#" style="display: none;""#
        },
        icon("lucide-alert-triangle"),
    )
}

/// Whether a value is one the property's type holds.
fn fits(ty: PropertyType, value: &Value) -> bool {
    let date = |s: &str, time: bool| {
        let b = s.as_bytes();
        let digits =
            |r: std::ops::Range<usize>| b.get(r).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
        digits(0..4)
            && b.get(4) == Some(&b'-')
            && digits(5..7)
            && b.get(7) == Some(&b'-')
            && digits(8..10)
            && if time {
                b.get(10) == Some(&b'T')
                    && digits(11..13)
                    && b.get(13) == Some(&b':')
                    && digits(14..16)
            } else {
                b.len() == 10
            }
    };
    match (ty, value) {
        (PropertyType::Unknown, _) => true,
        (PropertyType::Text, Value::String(_) | Value::Null) => true,
        (PropertyType::Number, Value::Number(_) | Value::Null) => true,
        (PropertyType::Checkbox, Value::Bool(_) | Value::Null) => true,
        (PropertyType::Date, Value::String(s)) => date(s, false),
        (PropertyType::DateTime, Value::String(s)) => date(s, true),
        (PropertyType::Date | PropertyType::DateTime, Value::Null) => true,
        // Obsidian shows a lone string as the one item of a list, without a warning.
        (
            PropertyType::List | PropertyType::Tags | PropertyType::Aliases,
            Value::Array(_) | Value::String(_) | Value::Null,
        ) => true,
        _ => false,
    }
}

fn type_name(ty: PropertyType) -> &'static str {
    match ty {
        PropertyType::Unknown => "unknown",
        PropertyType::Text => "text",
        PropertyType::List => "multitext",
        PropertyType::Number => "number",
        PropertyType::Checkbox => "checkbox",
        PropertyType::Date => "date",
        PropertyType::DateTime => "datetime",
        PropertyType::Tags => "tags",
        PropertyType::Aliases => "aliases",
    }
}

fn type_label(ty: PropertyType) -> &'static str {
    match ty {
        PropertyType::Unknown => "Unknown",
        PropertyType::Text => "Text",
        PropertyType::List => "List",
        PropertyType::Number => "Number",
        PropertyType::Checkbox => "Checkbox",
        PropertyType::Date => "Date",
        PropertyType::DateTime => "Date & time",
        PropertyType::Tags => "Tags",
        PropertyType::Aliases => "Aliases",
    }
}

fn type_icon(ty: PropertyType) -> &'static str {
    match ty {
        PropertyType::Unknown => "lucide-file-question",
        PropertyType::Text => "lucide-text",
        PropertyType::List => "lucide-list",
        PropertyType::Number => "lucide-binary",
        PropertyType::Checkbox => "lucide-check-square",
        PropertyType::Date => "lucide-calendar",
        PropertyType::DateTime => "lucide-clock",
        PropertyType::Tags => "lucide-tags",
        PropertyType::Aliases => "lucide-forward",
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn widget(doc: &Doc, key: &str, ty: PropertyType, value: &Value) -> String {
    match ty {
        PropertyType::Text => {
            let text = scalar(value);
            match (link_of(&text), external_url(&text)) {
                (Some(link), _) => format!(
                    r#"<div class="metadata-link"><div class="metadata-link-inner internal-link{}" data-href="{}" draggable="true">{}</div><div class="metadata-link-flair">{}</div></div>"#,
                    unresolved(doc, &link),
                    html::attr(&href(&link)),
                    html::text(&label(&link)),
                    icon("lucide-pencil"),
                ),
                (None, Some(url)) => format!(
                    r#"<div class="metadata-link"><div class="metadata-link-inner external-link" data-href="{0}">{0}</div><div class="metadata-link-flair">{1}</div></div>"#,
                    html::attr(url),
                    icon("lucide-pencil"),
                ),
                (None, None) => format!(
                    r#"<div class="metadata-input-longtext" placeholder="Empty">{}</div>"#,
                    html::text(&text)
                ),
            }
        }
        PropertyType::Number => input(
            "metadata-input metadata-input-number",
            r#" inputmode="decimal" step="any" type="number""#,
            &scalar(value),
        ),
        PropertyType::Checkbox => format!(
            r#"<input class="metadata-input-checkbox" type="checkbox" data-indeterminate="false"{}>"#,
            if value.as_bool() == Some(true) {
                " checked"
            } else {
                ""
            }
        ),
        PropertyType::Date => format!(
            "{}<div class=\"clickable-icon\" aria-label=\"Open daily note\">{}</div>",
            input(
                "metadata-input metadata-input-text mod-date",
                r#" max="9999-12-31" type="date""#,
                &scalar(value)
            ),
            icon("lucide-link")
        ),
        PropertyType::DateTime => input(
            "metadata-input metadata-input-text mod-datetime",
            r#" max="9999-12-31T23:59" type="datetime-local""#,
            &scalar(value),
        ),
        PropertyType::List | PropertyType::Tags | PropertyType::Aliases => pills(doc, ty, value),
        PropertyType::Unknown => format!(
            r#"<span class="metadata-property-value-item mod-unknown">{}</span>"#,
            html::text(&json(doc, key, value))
        ),
    }
}

fn input(class: &str, attrs: &str, value: &str) -> String {
    let value = if value.is_empty() {
        String::new()
    } else {
        format!(r#" value="{}""#, html::attr(value))
    };
    format!(r#"<input class="{class}"{attrs} placeholder="Empty"{value}>"#)
}

fn pills(doc: &Doc, ty: PropertyType, value: &Value) -> String {
    let items: Vec<&Value> = match value {
        Value::Array(a) => a.iter().collect(),
        Value::String(s) if !s.is_empty() => vec![value],
        _ => Vec::new(),
    };
    let mut out = String::from(r#"<div class="multi-select-container">"#);
    for item in items {
        let text = scalar(item);
        let (class, label, content) = if ty == PropertyType::Tags {
            // A tag pill shows the name without its `#`; one that is no tag name says so.
            let name = text.strip_prefix('#').unwrap_or(&text);
            let invalid = !valid_tag(name);
            (
                if invalid { " is-invalid" } else { "" },
                if invalid {
                    r#" aria-label="Invalid tag name""#
                } else {
                    ""
                },
                format!(
                    r#"<div class="multi-select-pill-content"><span>{}</span></div>"#,
                    html::text(name)
                ),
            )
        } else if let Some(link) = link_of(&text) {
            (
                "",
                "",
                format!(
                    r#"<div class="multi-select-pill-content internal-link{}" data-href="{}" draggable="true">{}</div>"#,
                    unresolved(doc, &link),
                    html::attr(&href(&link)),
                    html::text(&label(&link)),
                ),
            )
        } else {
            (
                "",
                "",
                format!(
                    r#"<div class="multi-select-pill-content">{}</div>"#,
                    html::text(&text)
                ),
            )
        };
        out.push_str(&format!(
            r#"<div class="multi-select-pill{class}"{label}>{content}<div class="multi-select-pill-remove-button">{}</div></div>"#,
            icon("lucide-x")
        ));
    }
    out.push_str(r#"<div class="multi-select-input" placeholder=""></div></div>"#);
    out
}

/// `https://…` and the like: a text value that is a web address and nothing else.
fn external_url(value: &str) -> Option<&str> {
    static URL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(?i:[a-z][a-z0-9+.-]*)://\S+$").unwrap());
    let value = value.trim();
    URL.is_match(value).then_some(value)
}

/// An object-valued property as JSON with its keys in the order the YAML has them (serde_json's
/// map sorts them).
fn json(doc: &Doc, key: &str, value: &Value) -> String {
    #[derive(Deserialize, Serialize)]
    #[serde(untagged)]
    enum Yaml {
        Null(()),
        Bool(bool),
        Integer(i64),
        Float(f64),
        Text(String),
        List(Vec<Yaml>),
        Map(IndexMap<String, Yaml>),
    }
    let ordered = doc.note.frontmatter.as_ref().and_then(|fm| {
        let options = serde_saphyr::options! { strict_booleans: true };
        let all: IndexMap<String, Yaml> = serde_saphyr::from_str_with_options(
            frontmatter::body(fm.span.slice(doc.source)),
            options,
        )
        .ok()?;
        serde_json::to_string(all.get(key)?).ok()
    });
    ordered.unwrap_or_else(|| value.to_string())
}

/// The link a property value consists of entirely, if it does.
fn link_of(value: &str) -> Option<Link> {
    let value = value.trim();
    let mut links = parse_note(value).links;
    let link = links.pop().filter(|_| links.is_empty())?;
    (!link.embed && link.span.start == 0 && link.span.end == value.len()).then_some(link)
}

/// The link as the property writes it, without its alias.
fn href(link: &Link) -> String {
    let fragment = link
        .fragment
        .as_ref()
        .map(Fragment::to_string)
        .unwrap_or_default();
    match link.syntax {
        LinkSyntax::Markdown => format!("{}{fragment}", link.target),
        _ => format!("{}{fragment}", link.target),
    }
}

/// What the link shows: its alias, else its target as written.
fn label(link: &Link) -> String {
    link.text.clone().unwrap_or_else(|| match link.syntax {
        LinkSyntax::Markdown => link.target.clone(),
        _ => link.target.clone(),
    })
}

fn unresolved(doc: &Doc, link: &Link) -> &'static str {
    match doc.cx.index.names().target_of(doc.path, link) {
        Target::File(_) => "",
        Target::Unresolved { .. } => " is-unresolved",
    }
}
