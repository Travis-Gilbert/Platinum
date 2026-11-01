//! Sidebar, board and expanded views share the same editor entity and vault
//! document identity. The vault owns durable text; the editor owns selection
//! and undo. Disk/authority work runs off the UI thread except the final flush.
use frame_notes::{Document, Vault};
use frame_text_check::FieldPolicy;
use frame_text_check_editor::TextChecking;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, Subscription, Task, Window, div, px,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use zorite_editor::EditorState;

struct Store {
    vault: Vault,
    written: HashMap<String, u64>,
    heads: HashMap<String, Document>,
    submitted: HashMap<String, (u64, String)>,
}
impl Store {
    fn save(
        &mut self,
        path: &str,
        generation: u64,
        base: &Document,
        text: &str,
    ) -> anyhow::Result<Document> {
        let head = self
            .heads
            .get(path)
            .ok_or_else(|| anyhow::anyhow!("Note was not opened"))?
            .clone();
        if generation == 0 || self.written.get(path).is_some_and(|g| *g > generation) {
            return Ok(head);
        }
        if self.written.get(path).copied() != Some(generation) {
            // The queued edit names what the editor actually saw. During quit,
            // a prior save may have finished before its UI completion ran.
            let ancestor = self
                .submitted
                .get(path)
                .filter(|(revision, _)| *revision == base.revision)
                .map_or(base.text.as_str(), |(_, text)| text.as_str());
            let merged = if base.revision == head.revision {
                text.to_owned()
            } else {
                merge_typing(ancestor, text, &head.text)?
            };
            let doc = self.vault.edit(
                path,
                head.revision,
                &format!("platinum:{path}:{}:{generation}", head.revision),
                &merged,
            )?;
            self.heads.insert(path.into(), doc);
            self.submitted
                .insert(path.into(), (base.revision, text.to_owned()));
            self.written.insert(path.into(), generation);
        }
        if matches!(
            self.vault.save(path)?,
            frame_notes::ReconcileResult::Conflict { .. }
        ) {
            anyhow::bail!("The note changed on disk while saving; both versions are preserved.");
        }
        let saved = self.vault.open_document(path)?;
        self.heads.insert(path.into(), saved.clone());
        Ok(saved)
    }
}

fn merge_typing(base: &str, typing: &str, saved: &str) -> anyhow::Result<String> {
    diffy::merge(base, typing, saved).map_err(|_| {
        anyhow::anyhow!(
            "Typing overlaps external changes; the editor and saved version are preserved."
        )
    })
}

pub struct Note {
    pub id: String,
    pub path: String,
    pub editor: Entity<EditorState>,
    store: Arc<Mutex<Store>>,
    generation: u64,
    base: Document,
    saving: bool,
    status: String,
    observed_text: String,
    save: Option<Task<()>>,
    _edits: Subscription,
    _quit: Subscription,
    /// Spelling and grammar underlines and corrections for `editor`. `None`
    /// when the text-checking service could not start; the note is then
    /// written unchecked.
    _checking: Option<Entity<TextChecking>>,
}
impl Note {
    pub fn title(&self, cx: &App) -> String {
        self.editor
            .read(cx)
            .text()
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(|line| {
                line.trim_start_matches('#')
                    .trim()
                    .chars()
                    .take(80)
                    .collect()
            })
            .filter(|title: &String| !title.is_empty())
            .unwrap_or_else(|| "Untitled note".into())
    }
    fn new(
        path: String,
        doc: Document,
        store: Arc<Mutex<Store>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let observed_text = doc.text.clone();
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .with_text(doc.text.clone())
                .with_placeholder("Write a note…")
        });
        let edits = cx.observe(&editor, |this, editor, cx| {
            let text = editor.read(cx).text();
            if text != this.observed_text {
                this.observed_text = text.to_owned();
                this.generation += 1;
                this.status = "Saving…".into();
                this.queue_save(cx);
            }
        });
        let checking = frame_text_check::shared()
            .map(|service| TextChecking::attach(&editor, service, FieldPolicy::default(), cx));
        let quit = cx.on_app_quit(|this, cx| this.flush(cx));
        cx.on_release(|this, cx| this.flush(cx).detach()).detach();
        Self {
            id: doc.document_id.clone(),
            path,
            editor,
            store,
            generation: 0,
            base: doc,
            saving: false,
            status: "Saved locally".into(),
            observed_text,
            save: None,
            _edits: edits,
            _quit: quit,
            _checking: checking,
        }
    }
    fn queue_save(&mut self, cx: &mut Context<Self>) {
        // Never cancel the completion of an in-flight write. Later edits are
        // rebased onto its result before the next save is queued.
        if self.saving {
            return;
        }
        self.saving = true;
        let store = self.store.clone();
        let path = self.path.clone();
        let generation = self.generation;
        let text = self.editor.read(cx).text().to_owned();
        let submitted = text.clone();
        let base = self.base.clone();
        self.save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(300))
                .await;
            let result = cx
                .background_executor()
                .spawn(async move {
                    store
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Notes store unavailable"))?
                        .save(&path, generation, &base, &text)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                this.status = match result.and_then(|saved| {
                    let merged =
                        merge_typing(&submitted, this.editor.read(cx).text(), &saved.text)?;
                    Ok((saved, merged))
                }) {
                    Ok((saved, merged)) => {
                        let more_typing = merged != saved.text;
                        this.base = saved;
                        if this.editor.read(cx).text() != merged {
                            this.observed_text = merged.clone();
                            this.editor.update(cx, |editor, cx| {
                                let length = editor.text().len();
                                editor.replace_range(0..length, &merged, cx);
                            });
                        }
                        if more_typing {
                            this.queue_save(cx);
                            "Saving…".into()
                        } else {
                            "Saved locally".into()
                        }
                    }
                    Err(e) => format!("Not saved: {e}"),
                };
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn flush(&self, cx: &App) -> Task<()> {
        let store = self.store.clone();
        let path = self.path.clone();
        let generation = self.generation;
        let text = self.editor.read(cx).text().to_owned();
        let base = self.base.clone();
        cx.background_executor().spawn(async move {
            if let Ok(mut store) = store.lock() {
                if let Err(e) = store.save(&path, generation, &base, &text) {
                    log::error!("could not flush note: {e}");
                }
            }
        })
    }
}
impl Render for Note {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .gap_2()
            .p_3()
            .bg(cx.theme().popover)
            .text_color(cx.theme().foreground)
            .child(div().text_sm().child(self.title(cx)))
            .child(
                div()
                    .id("note-editor-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.editor.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.status.clone()),
            )
    }
}

pub enum NotesEvent {
    Promote(Entity<Note>),
    Restored(Entity<Note>),
    Expand,
}
impl EventEmitter<NotesEvent> for Notebook {}
pub struct Notebook {
    root: PathBuf,
    store: Option<Arc<Mutex<Store>>>,
    paths: Vec<String>,
    notes: HashMap<String, Entity<Note>>,
    selected: Option<String>,
    status: String,
    busy: bool,
    pending_create: Option<Option<String>>,
    pending_text: Option<String>,
    task: Option<Task<()>>,
}
impl Notebook {
    pub fn new(
        root: PathBuf,
        restore: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            root,
            store: None,
            paths: vec![],
            notes: HashMap::new(),
            selected: None,
            status: "Opening notes…".into(),
            busy: true,
            pending_create: None,
            pending_text: None,
            task: None,
        };
        let root = this.root.clone();
        this.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let documents = root.join("notes");
                    std::fs::create_dir_all(&documents)?;
                    let mut vault = Vault::open(&documents, root.join("notes-data"))?;
                    let mut restored = Vec::new();
                    let mut warnings = Vec::new();
                    for path in restore {
                        match vault.open_document(&path) {
                            Ok(doc) => restored.push((path, doc)),
                            Err(error) => {
                                warnings.push(format!("Could not restore {path}: {error}"))
                            }
                        }
                    }
                    let heads = restored
                        .iter()
                        .map(|(path, doc)| (path.clone(), doc.clone()))
                        .collect();
                    let paths = vault
                        .index()
                        .files()
                        .keys()
                        .filter(|p| p.ends_with(".md"))
                        .cloned()
                        .collect::<Vec<_>>();
                    Ok::<_, anyhow::Error>((
                        Arc::new(Mutex::new(Store {
                            vault,
                            written: HashMap::new(),
                            submitted: HashMap::new(),
                            heads,
                        })),
                        paths,
                        restored,
                        warnings,
                    ))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok((store, paths, restored, warnings)) => {
                        for (path, doc) in restored {
                            let note = cx
                                .new(|cx| Note::new(path.clone(), doc, store.clone(), window, cx));
                            this.notes.insert(path, note.clone());
                            cx.emit(NotesEvent::Restored(note));
                        }
                        this.store = Some(store);
                        this.paths = paths;
                        this.status = warnings.join("\n");
                        if let Some(text) = this.pending_text.take() {
                            this.create_text(text, window, cx);
                        } else if let Some(source) = this.pending_create.take() {
                            this.create(source, window, cx);
                        } else if let Some(path) = this.paths.first().cloned() {
                            this.open(path, None, window, cx);
                        }
                    }
                    Err(e) => this.status = format!("Could not open notes: {e}"),
                }
                cx.notify();
            });
        }));
        this
    }
    pub fn note(&self, path: &str) -> Option<Entity<Note>> {
        self.notes.get(path).cloned()
    }
    pub fn select(&mut self, note: &Entity<Note>, cx: &mut Context<Self>) {
        self.selected = Some(note.read(cx).path.clone());
        cx.notify();
    }
    pub fn selected(&self) -> Option<Entity<Note>> {
        self.selected
            .as_ref()
            .and_then(|p| self.notes.get(p))
            .cloned()
    }
    pub fn create(&mut self, source: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.is_none() {
            self.pending_create = Some(source);
            return;
        }
        let path = format!("Note {}.md", frame_profile::now());
        let text = source
            .map(|url| format!("# Note\n\nSource: {url}\n\n"))
            .unwrap_or_else(|| "# Note\n\n".into());
        self.open(path, Some(text), window, cx);
    }
    pub fn create_text(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.is_none() {
            self.pending_text = Some(text);
            return;
        }
        self.open(
            format!("Note {}.md", frame_profile::now()),
            Some(text),
            window,
            cx,
        );
    }
    fn open(
        &mut self,
        path: String,
        create: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.notes.contains_key(&path) {
            self.selected = Some(path);
            cx.notify();
            return;
        }
        let Some(store) = self.store.clone() else {
            return;
        };
        self.busy = true;
        let created = create.is_some();
        let shared = store.clone();
        let key = path.clone();
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut store = shared
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Notes unavailable"))?;
                    let doc = match create {
                        Some(text) => store.vault.create_note(&key, &text),
                        None => store.vault.open_document(&key),
                    }?;
                    store.heads.insert(key, doc.clone());
                    Ok::<_, anyhow::Error>(doc)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(doc) => {
                        let note = cx.new(|cx| Note::new(path.clone(), doc, store, window, cx));
                        if created {
                            note.read(cx).editor.clone().update(cx, |editor, cx| {
                                editor.set_cursor(editor.text().len(), cx);
                            });
                        }
                        if !this.paths.contains(&path) {
                            this.paths.push(path.clone());
                        }
                        this.notes.insert(path.clone(), note.clone());
                        this.selected = Some(path);
                        this.status.clear();
                        window.focus(&note.read(cx).editor.focus_handle(cx), cx);
                    }
                    Err(e) => this.status = format!("Could not open note: {e}"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
impl Render for Notebook {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::Disableable as _;
        let note = self.selected();
        v_flex()
            .size_full()
            .min_h_0()
            .bg(cx.theme().popover)
            .child(
                h_flex()
                    .gap_1()
                    .p_2()
                    .child(
                        Button::new("notes-new")
                            .small()
                            .ghost()
                            .label("New note")
                            .disabled(self.busy || self.store.is_none())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.create(None, window, cx)),
                            ),
                    )
                    .child(
                        Button::new("notes-promote")
                            .small()
                            .ghost()
                            .label("Place on board")
                            .disabled(note.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(note) = this.selected() {
                                    cx.emit(NotesEvent::Promote(note));
                                }
                            })),
                    )
                    .child(
                        Button::new("notes-expand")
                            .small()
                            .ghost()
                            .label("Expand")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(NotesEvent::Expand))),
                    ),
            )
            .child(
                div()
                    .id("note-list")
                    .max_h(px(130.))
                    .overflow_y_scroll()
                    .children(self.paths.iter().map(|path| {
                        let key = path.clone();
                        Button::new(gpui_kit::SharedString::from(format!("note:{path}")))
                            .small()
                            .ghost()
                            .label(path.trim_end_matches(".md").to_owned())
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open(key.clone(), None, window, cx)
                            }))
                    })),
            )
            .child(div().text_xs().px_2().child(self.status.clone()))
            .children(note.map(|note| div().flex_1().min_h_0().child(note)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typing_during_a_disk_merge_survives_completion_and_shutdown() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("notes");
        std::fs::create_dir(&folder).unwrap();
        let mut vault = Vault::open(&folder, root.path().join("data")).unwrap();
        let doc = vault
            .create_note("Note.md", "Title\n\nLocal\n\nDisk\n")
            .unwrap();
        let mut store = Store {
            vault,
            written: HashMap::new(),
            submitted: HashMap::new(),
            heads: HashMap::from([("Note.md".into(), doc.clone())]),
        };
        std::fs::write(folder.join("Note.md"), "Title\n\nLocal\n\nExternal\n").unwrap();
        let submitted = "Title\n\nFirst typing\n\nDisk\n";
        let later = "Title\n\nLater typing\n\nDisk\n";
        let saved = store.save("Note.md", 1, &doc, submitted).unwrap();
        let expected = "Title\n\nLater typing\n\nExternal\n";
        assert_eq!(
            merge_typing(submitted, later, &saved.text).unwrap(),
            expected
        );
        // Shutdown can flush before the first save's UI completion is observed.
        let flushed = store.save("Note.md", 2, &doc, later).unwrap();
        assert_eq!(flushed.text, expected);
        assert_eq!(
            std::fs::read_to_string(folder.join("Note.md")).unwrap(),
            expected
        );
        // A late older request must not remove either edit.
        store.save("Note.md", 1, &doc, submitted).unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join("Note.md")).unwrap(),
            expected
        );
        assert!(merge_typing("line\n", "typing\n", "external\n").is_err());
    }

    #[test]
    fn note_edits_survive_conflict_and_old_saves_cannot_replace_newer_edits() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("notes");
        std::fs::create_dir(&folder).unwrap();
        let mut vault = Vault::open(&folder, root.path().join("data")).unwrap();
        let doc = vault.create_note("Note.md", "initial\n").unwrap();
        let mut store = Store {
            vault,
            written: HashMap::new(),
            heads: HashMap::from([("Note.md".into(), doc.clone())]),
            submitted: HashMap::new(),
        };
        let first = store.save("Note.md", 1, &doc, "first\n").unwrap();
        let third = store.save("Note.md", 3, &first, "third\n").unwrap();
        store.save("Note.md", 2, &first, "second\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join("Note.md")).unwrap(),
            "third\n"
        );
        std::fs::write(folder.join("Note.md"), "external\n").unwrap();
        assert!(store.save("Note.md", 4, &third, "local fourth\n").is_err());
        assert_eq!(
            std::fs::read_to_string(folder.join("Note.md")).unwrap(),
            "external\n"
        );
        drop(store);
        let mut vault = Vault::open(&folder, root.path().join("data")).unwrap();
        let recovered = vault.open_document("Note.md").unwrap();
        assert_eq!(recovered.text, "local fourth\n");
        assert!(matches!(
            recovered.reconciliation,
            Some(frame_notes::ReconcileResult::Conflict { .. })
        ));
    }
}
