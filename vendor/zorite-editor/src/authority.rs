//! THEOREM PATCH: byte-based projection boundary; the host owns document history.
//!
//! No retained edit queue or shadow document: GPUI delivers each mutation in order.
//! The monotonically increasing buffer generation rejects stale remote responses, even
//! across loads. Hosts must also keep their subscription scoped to the document entity.
use std::{fmt, ops::Range};

use gpui::Context;

use crate::{EditKind, EditorEvent, EditorState};

/// One replacement. `range` addresses UTF-8 boundaries in the text BEFORE this edit.
/// For a batch, each range addresses the result of the preceding edit.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct TextEdit {
    pub range: Range<usize>,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditOrigin {
    Local,
    /// A new document load. Observe it, but do not send it back as a session edit.
    Load,
}

/// Exactly one event per buffer splice, after its selection is established.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferEdit {
    pub edit: TextEdit,
    pub selection: Range<usize>,
    pub selection_reversed: bool,
    pub origin: EditOrigin,
    /// Generation AFTER this edit; its base is `generation - 1`.
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryAction {
    Undo,
    Redo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteEditError {
    StaleGeneration {
        expected: u64,
        actual: u64,
    },
    /// Index in the supplied batch; no edits from the batch were applied.
    InvalidRange {
        index: usize,
    },
}

impl fmt::Display for RemoteEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleGeneration { expected, actual } => write!(
                f,
                "remote edit expected generation {expected}, buffer is at {actual}"
            ),
            Self::InvalidRange { index } => {
                write!(f, "remote edit {index} is not a valid UTF-8 range")
            }
        }
    }
}
impl std::error::Error for RemoteEditError {}

impl EditorState {
    /// Attach/detach host history BEFORE loading text. Subscribe to `Edited` and
    /// `HistoryRequested` first. On attachment the host must already own the current
    /// text; local snapshots are discarded on either transition. No text is changed.
    /// Hosted `with_text`/`set_text` preserve all bytes (including inline `$$` fences).
    pub fn set_hosted(&mut self, hosted: bool) {
        if self.hosted != hosted {
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.last_edit = EditKind::Other;
            self.hosted = hosted;
        }
    }

    /// Current projection generation; changes on every splice, load and remote edit.
    pub fn generation(&self) -> u64 {
        self.content_gen
    }

    pub(super) fn emit_edit(&self, edit: TextEdit, origin: EditOrigin, cx: &mut Context<Self>) {
        cx.emit(EditorEvent::Edited(BufferEdit {
            edit,
            selection: self.selected_range.clone(),
            selection_reversed: self.selection_reversed,
            origin,
            generation: self.content_gen,
        }));
    }

    /// Apply authority replacements atomically, without emitting events or changing
    /// undo/redo. The batch uses sequential old-text byte ranges. Invalid UTF-8 ranges
    /// or a stale generation leave ALL state untouched. One temporary String validates
    /// the batch; retained state remains the existing editor buffer.
    ///
    /// Selection and IME endpoints use right affinity: insertion at an endpoint moves
    /// it after the insertion; an endpoint inside replaced text lands after the new
    /// text. Direction is retained. An empty marked range ends composition. Diagnostics
    /// crossing changed text are discarded, and unaffected diagnostics shift with it.
    pub fn apply_remote(
        &mut self,
        generation: u64,
        edits: &[TextEdit],
        cx: &mut Context<Self>,
    ) -> Result<(), RemoteEditError> {
        if generation != self.content_gen {
            return Err(RemoteEditError::StaleGeneration {
                expected: generation,
                actual: self.content_gen,
            });
        }
        if edits.is_empty() {
            return Ok(());
        }
        let mut text = self.content.clone();
        for (index, edit) in edits.iter().enumerate() {
            if text.get(edit.range.clone()).is_none() {
                return Err(RemoteEditError::InvalidRange { index });
            }
            text.replace_range(edit.range.clone(), &edit.text);
        }
        for edit in edits {
            let shift = |offset| remap_offset(offset, &edit.range, edit.text.len());
            self.selected_range = shift(self.selected_range.start)..shift(self.selected_range.end);
            self.marked_range = self.marked_range.take().and_then(|range| {
                let range = shift(range.start)..shift(range.end);
                (!range.is_empty()).then_some(range)
            });
            self.remap_diagnostics(&edit.range, edit.text.len());
            self.content_gen += 1;
        }
        self.content = text;
        self.goal_x = None;
        self.last_replaced = None;
        self.last_edit = EditKind::Other;
        cx.notify();
        Ok(())
    }
}

fn remap_offset(offset: usize, range: &Range<usize>, inserted: usize) -> usize {
    if offset < range.start {
        offset
    } else if offset >= range.end {
        offset - range.len() + inserted
    } else {
        range.start + inserted
    }
}
