//! The resources Theo has open, each held in a session independent of any
//! view: its buffer, selection, undo and composition (Rune's `TextEdit`
//! keeps all four), its scroll, its loading state, and what to recover.
//!
//! A view observes a surface session; mounting or unmounting one never
//! creates, copies or discards the session. Opening a resource that is
//! already open attaches to the session it has.

use rune::TextEdit;
use serde::{Deserialize, Serialize};

/// An existing resource, named by the authority that owns it.
///
/// `kind` is open: `"conversation"` and `"note"` are the first, and an
/// authored capability names its own. Nothing here enumerates them.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SurfaceRef {
    pub kind: String,
    pub id: String,
    /// The revision the session started from, where the authority has one.
    pub revision: Option<String>,
}

impl SurfaceRef {
    pub fn new(kind: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            id: id.into(),
            revision: None,
        }
    }

    /// Whether `other` names the same resource, at whatever revision.
    pub fn same_resource(&self, other: &SurfaceRef) -> bool {
        self.kind == other.kind && self.id == other.id
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Load {
    #[default]
    Loading,
    Ready,
    /// The resource could not be loaded; the session keeps what it had.
    Failed(String),
}

/// Unsaved text and where it belongs: written on change, read on launch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    pub reference: SurfaceRef,
    pub text: String,
}

/// One open resource.
#[derive(Clone, Debug)]
pub struct SurfaceSession {
    pub reference: SurfaceRef,
    pub edit: TextEdit,
    pub load: Load,
    pub scroll: f32,
    /// The text as the authority last had it; the buffer differs when the
    /// person has unsaved changes.
    saved: String,
}

impl SurfaceSession {
    fn new(reference: SurfaceRef) -> Self {
        Self {
            reference,
            edit: TextEdit::new(""),
            load: Load::Loading,
            scroll: 0.0,
            saved: String::new(),
        }
    }

    /// The authority's text arrived. Unsaved changes are kept: a load never
    /// overwrites what the person typed.
    pub fn loaded(&mut self, text: &str, revision: Option<String>) {
        let unsaved = self.dirty();
        self.saved = text.to_owned();
        self.reference.revision = revision;
        self.load = Load::Ready;
        if !unsaved {
            self.edit.replace_text(text);
        }
    }

    pub fn failed(&mut self, reason: impl Into<String>) {
        self.load = Load::Failed(reason.into());
    }

    /// The authority accepted `text` at `revision`.
    pub fn saved(&mut self, text: &str, revision: Option<String>) {
        self.saved = text.to_owned();
        self.reference.revision = revision;
    }

    pub fn dirty(&self) -> bool {
        self.edit.text != self.saved
    }

    /// What to keep across a restart: the unsaved text, if any.
    pub fn draft(&self) -> Option<Draft> {
        self.dirty().then(|| Draft {
            reference: self.reference.clone(),
            text: self.edit.text.clone(),
        })
    }
}

/// The open surfaces, in the order they were opened, and the one in front.
#[derive(Clone, Debug, Default)]
pub struct Surfaces {
    open: Vec<SurfaceSession>,
    front: Option<usize>,
}

impl Surfaces {
    /// Opens `reference`, or attaches to the session it already has, and
    /// brings it to the front. Returns whether a session was created.
    pub fn open(&mut self, reference: SurfaceRef) -> bool {
        if let Some(index) = self.index(&reference) {
            self.front = Some(index);
            return false;
        }
        self.open.push(SurfaceSession::new(reference));
        self.front = Some(self.open.len() - 1);
        true
    }

    /// Opens a resource with a recovered draft in its buffer.
    pub fn recover(&mut self, draft: Draft) {
        self.open(draft.reference.clone());
        if let Some(session) = self.get_mut(&draft.reference) {
            session.edit.replace_text(&draft.text);
        }
    }

    fn index(&self, reference: &SurfaceRef) -> Option<usize> {
        self.open
            .iter()
            .position(|s| s.reference.same_resource(reference))
    }

    pub fn get(&self, reference: &SurfaceRef) -> Option<&SurfaceSession> {
        self.index(reference).map(|i| &self.open[i])
    }

    pub fn get_mut(&mut self, reference: &SurfaceRef) -> Option<&mut SurfaceSession> {
        self.index(reference).map(|i| &mut self.open[i])
    }

    pub fn front(&self) -> Option<&SurfaceSession> {
        self.front.map(|i| &self.open[i])
    }

    pub fn front_mut(&mut self) -> Option<&mut SurfaceSession> {
        self.front.map(|i| &mut self.open[i])
    }

    pub fn bring_to_front(&mut self, reference: &SurfaceRef) -> bool {
        match self.index(reference) {
            Some(index) => {
                self.front = Some(index);
                true
            }
            None => false,
        }
    }

    /// Closes a surface. Its unsaved draft is returned for the caller to keep
    /// or to confirm discarding; closing never drops it silently.
    pub fn close(&mut self, reference: &SurfaceRef) -> Option<Option<Draft>> {
        let index = self.index(reference)?;
        let session = self.open.remove(index);
        self.front = match self.front {
            Some(f) if f == index => self.open.len().checked_sub(1),
            Some(f) if f > index => Some(f - 1),
            other => other,
        };
        Some(session.draft())
    }

    pub fn iter(&self) -> impl Iterator<Item = &SurfaceSession> {
        self.open.iter()
    }

    pub fn len(&self) -> usize {
        self.open.len()
    }

    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// Every unsaved draft, for the recovery file.
    pub fn drafts(&self) -> Vec<Draft> {
        self.open.iter().filter_map(SurfaceSession::draft).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note() -> SurfaceRef {
        SurfaceRef::new("note", "n1")
    }

    #[test]
    fn opening_an_open_resource_attaches_to_its_session() {
        let mut surfaces = Surfaces::default();
        assert!(surfaces.open(note()));
        surfaces.front_mut().unwrap().edit.insert("hello");
        assert!(!surfaces.open(SurfaceRef {
            revision: Some("r2".into()),
            ..note()
        }));
        assert_eq!(surfaces.len(), 1);
        assert_eq!(surfaces.get(&note()).unwrap().edit.text, "hello");
    }

    #[test]
    fn a_load_never_overwrites_unsaved_text() {
        let mut surfaces = Surfaces::default();
        surfaces.open(note());
        let session = surfaces.get_mut(&note()).unwrap();
        session.loaded("from the server", Some("r1".into()));
        assert_eq!(session.edit.text, "from the server");
        session.edit.insert(" and mine");
        session.loaded("newer from the server", Some("r2".into()));
        assert!(session.edit.text.ends_with(" and mine"));
        assert!(session.dirty());
    }

    #[test]
    fn closing_returns_the_unsaved_draft() {
        let mut surfaces = Surfaces::default();
        surfaces.open(note());
        surfaces.get_mut(&note()).unwrap().edit.insert("unsent");
        let draft = surfaces.close(&note()).unwrap().unwrap();
        assert_eq!(draft.text, "unsent");
        assert!(surfaces.front().is_none());
    }

    #[test]
    fn a_recovered_draft_is_dirty_until_saved() {
        let mut surfaces = Surfaces::default();
        surfaces.recover(Draft {
            reference: note(),
            text: "kept".into(),
        });
        let session = surfaces.get_mut(&note()).unwrap();
        assert!(session.dirty());
        session.loaded("server text", Some("r1".into()));
        assert_eq!(session.edit.text, "kept", "the draft survives the load");
        session.saved("kept", Some("r2".into()));
        assert!(!session.dirty());
        assert!(surfaces.drafts().is_empty());
    }

    #[test]
    fn undo_lives_in_the_session_not_the_view() {
        let mut surfaces = Surfaces::default();
        surfaces.open(note());
        let session = surfaces.front_mut().unwrap();
        session.edit.insert("one");
        session.edit.insert(" two");
        session.edit.undo();
        assert_ne!(session.edit.text, "one two");
    }
}
