//! Pure rename planning. The runtime transaction will consume these edits after checking their hashes.
use crate::{
    FileIndex, FileKind, LinkSyntax, Span, Vault, VaultSettings, is_hidden, normalize, parse_note,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextEdit {
    /// UTF-8 byte offsets into the original document, never into a previously edited copy.
    pub span: Span,
    pub replacement: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentEdits {
    pub path: String,
    pub hash: String,
    pub document_id: String,
    pub revision: u64,
    pub edits: Vec<TextEdit>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenamePlan {
    pub from: String,
    pub to: String,
    pub documents: Vec<DocumentEdits>,
}

impl Vault {
    /// Compute only. No files, ids, sessions or graph records change until the authority commits a plan.
    pub fn plan_rename(&self, from: &str, to: &str) -> Result<RenamePlan> {
        ensure!(
            self.index().files().contains_key(from),
            "rename source is missing: {from}"
        );
        ensure!(
            normalize(to).as_deref() == Some(to) && !is_hidden(to),
            "invalid rename destination: {to}"
        );
        ensure!(
            !self.index().files().contains_key(to) && !self.root().join(to).exists(),
            "rename destination exists: {to}"
        );
        let mut after = self.index().names().clone();
        after.remove(from);
        after.insert(to);
        let mut documents = Vec::new();
        if self.settings().always_update_links {
            for (path, file) in self.index().files() {
                if !matches!(file.kind, FileKind::Note | FileKind::Canvas) {
                    continue;
                }
                let head = self.head(path)?;
                let text = head.text;
                let new_path = if path == from { to } else { path };
                let edits = if file.kind == FileKind::Canvas {
                    canvas_edits(
                        &text,
                        path,
                        new_path,
                        from,
                        to,
                        self.index().names(),
                        &after,
                        self.settings(),
                    )?
                } else {
                    note_edits(
                        &text,
                        path,
                        new_path,
                        from,
                        to,
                        self.index().names(),
                        &after,
                        self.settings(),
                    )?
                };
                if !edits.is_empty() {
                    documents.push(DocumentEdits {
                        path: path.clone(),
                        hash: rustyred_thg_vfs::ContentHash::of(text.as_bytes()).0,
                        document_id: head.document_id,
                        revision: head.revision,
                        edits,
                    });
                }
            }
        }
        Ok(RenamePlan {
            from: from.into(),
            to: to.into(),
            documents,
        })
    }
}

/// Apply in reverse byte order and reject overlapping/out-of-bounds edits.
pub fn apply_text_edits(text: &str, edits: &[TextEdit]) -> Result<String> {
    let mut edits = edits.to_vec();
    edits.sort_by_key(|e| e.span.start);
    let mut end = 0;
    for edit in &edits {
        ensure!(
            edit.span.start >= end && text.get(edit.span.range()).is_some(),
            "invalid or overlapping text edit"
        );
        end = edit.span.end;
    }
    let mut result = text.to_owned();
    for edit in edits.into_iter().rev() {
        result.replace_range(edit.span.range(), &edit.replacement);
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn link_edits(
    text: &str,
    path: &str,
    new_path: &str,
    from: &str,
    to: &str,
    before: &FileIndex,
    after: &FileIndex,
    settings: &VaultSettings,
) -> Vec<TextEdit> {
    parse_note(text)
        .links
        .into_iter()
        .filter_map(|link| {
            if link.in_frontmatter {
                return None;
            }
            let target = before.resolve(&link.target, path, link.syntax)?;
            let destination = if target == from { to } else { target };
            let still_resolves =
                after.resolve(&link.target, new_path, link.syntax) == Some(destination);
            if target != from && still_resolves {
                return None;
            }
            // Local heading/block references retain their local spelling when their document moves.
            if link.target.is_empty() {
                return None;
            }
            let replacement = settings.link_to(
                after,
                new_path,
                destination,
                link.fragment.as_ref(),
                link.text.as_deref(),
                link.embed,
            );
            (replacement != link.span.slice(text)).then_some(TextEdit {
                span: link.span,
                replacement,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn note_edits(
    text: &str,
    path: &str,
    new_path: &str,
    from: &str,
    to: &str,
    before: &FileIndex,
    after: &FileIndex,
    settings: &VaultSettings,
) -> Result<Vec<TextEdit>> {
    use granit_parser::ScalarStyle;
    use granit_parser::{Event, Parser};
    let mut edits = link_edits(text, path, new_path, from, to, before, after, settings);
    let note = parse_note(text);
    if let Some(fm) = &note.frontmatter {
        let yaml = crate::frontmatter::body(fm.span.slice(text));
        let base = yaml.as_ptr() as usize - text.as_ptr() as usize;
        let offsets = crate::Offsets::new(yaml);
        let source_offsets = crate::Offsets::new(text);
        let mut parser = Parser::new_from_str(yaml);
        while let Some(event) = parser.next_event() {
            let (event, span) = event?;
            let Event::Scalar(value, style, _, _) = event else {
                continue;
            };
            let start = base + offsets.scalar_to_byte(span.start.index());
            let end = base + offsets.scalar_to_byte(span.end.index());
            if !note
                .links
                .iter()
                .any(|l| l.in_frontmatter && l.span.start >= start && l.span.end <= end)
            {
                continue;
            }
            let inner = link_edits(&value, path, new_path, from, to, before, after, settings);
            if inner.is_empty() {
                continue;
            }
            let rewritten = apply_text_edits(&value, &inner)?;
            let replacement = match style {
                ScalarStyle::SingleQuoted if !rewritten.contains('\'') => format!("'{rewritten}'"),
                _ => serde_json::to_string(&rewritten)?,
            };
            edits.push(TextEdit {
                span: source_offsets.span(start, end),
                replacement,
            });
        }
        let rewritten = apply_text_edits(text, &edits)?;
        ensure!(
            parse_note(&rewritten)
                .frontmatter
                .is_some_and(|f| f.error.is_none()),
            "rename would invalidate YAML properties in {path}"
        );
    }
    Ok(edits)
}

#[derive(Deserialize)]
struct RawCanvas<'a> {
    #[serde(borrow, default)]
    nodes: Vec<&'a RawValue>,
}

#[allow(clippy::too_many_arguments)]
fn canvas_edits(
    text: &str,
    path: &str,
    new_path: &str,
    from: &str,
    to: &str,
    before: &FileIndex,
    after: &FileIndex,
    settings: &VaultSettings,
) -> Result<Vec<TextEdit>> {
    let canvas: RawCanvas<'_> = serde_json::from_str(text)?;
    let mut edits = Vec::new();
    for node in canvas.nodes {
        let fields: BTreeMap<&str, &RawValue> = serde_json::from_str(node.get())?;
        let kind = fields
            .get("type")
            .map(|v| serde_json::from_str::<String>(v.get()))
            .transpose()?;
        let field = match kind.as_deref() {
            Some("file") => "file",
            Some("text") => "text",
            _ => continue,
        };
        let Some(raw) = fields.get(field) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<String>(raw.get()) else {
            continue;
        };
        let replacement = if field == "file" {
            if before.resolve(&value, path, LinkSyntax::Path) != Some(from) {
                continue;
            }
            to.to_string()
        } else {
            let inner = note_edits(&value, path, new_path, from, to, before, after, settings)?;
            if inner.is_empty() {
                continue;
            }
            apply_text_edits(&value, &inner)?
        };
        // RawValue borrows the exact original JSON token, preserving all unrelated whitespace and keys.
        let start = (raw.get().as_ptr() as usize)
            .checked_sub(text.as_ptr() as usize)
            .context("JSON token outside source")?;
        edits.push(TextEdit {
            span: crate::Offsets::new(text).span(start, start + raw.get().len()),
            replacement: serde_json::to_string(&replacement)?,
        });
    }
    Ok(edits)
}
