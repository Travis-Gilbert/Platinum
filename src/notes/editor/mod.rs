//! The editor tab of one note: the zorite editor, hosted, on the vault's
//! authority.
//!
//! Ownership, by the data-oriented rules:
//! - the vault owns the note's text and its history (undo and redo are its,
//!   which is why an undo never reaches a change that came from disk);
//! - this view owns the buffer (`EditorState`) and `head`, a copy of the head
//!   it last saw, which is the base every rebase starts from;
//! - nothing else is stored. Whether the note is saved, unsaved or in
//!   conflict is derived from the head and the buffer's generation.
//!
//! Every exchange with the vault is one flight, and there is one flight at a
//! time (`pump`). A flight carries the whole buffer, so keystrokes made while
//! one is out simply wait for the next: the queue is one bit, in the order the
//! vault must see them. When a flight returns a head that is not the one it
//! was built on (the file changed outside, or another view wrote), the unsent
//! keystrokes are merged onto it by three-way merge and the result is applied
//! to the buffer as a minimal edit, so the caret stays where it was. An
//! overlap keeps both versions between conflict markers: nothing is dropped.
//!
//! The vault never overwrites a change from outside on save; the tab says so
//! in its header (`Status::Conflict`). Resolving it for good needs the vault
//! to offer a way to keep the buffer, which `frame-notes` does not yet have.

mod design;
mod rebase;

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use frame_notes::{ReconcileResult, Vault, stem};
use futures::FutureExt as _;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    Global, InteractiveElement as _, IntoElement, KeyBinding, ParentElement, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Task, Window, actions,
    div, px, relative,
};
use zorite_editor::{EditOrigin, EditorEvent, EditorState, HistoryAction};

use super::host::service::{Error as ServiceError, Service};
use rebase::{edits_between, merge3};

actions!(notes_editor, [Save]);

/// The key context of the tab; its bindings reach only a focus inside it.
const KEY_CONTEXT: &str = "NoteEditor";
/// The vault's name for the one workspace it holds.
const WORKSPACE: &str = "local";
/// A note is saved this long after the last keystroke.
const AUTOSAVE: Duration = Duration::from_secs(2);
/// A request the vault's full queue refused is made again after this.
const BUSY_RETRY: Duration = Duration::from_millis(250);
/// Undo and redo presses that may wait for the vault.
const MAX_HISTORY: usize = 64;

/// What a person is told about the note in the tab's header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// The vault has not answered yet.
    Opening,
    /// The text on screen is the text on disk.
    Saved,
    /// There is text that is not on disk yet.
    Unsaved,
    /// The text changed elsewhere in a place that this tab's text also
    /// changed. Nothing has been overwritten on either side.
    Conflict,
    /// The vault refused or could not do something. Typing again tries again.
    Failed(String),
}

impl Status {
    fn label(&self) -> SharedString {
        match self {
            Self::Opening => "Opening".into(),
            Self::Saved => "Saved".into(),
            Self::Unsaved => "Unsaved".into(),
            Self::Conflict => "Conflict: changed elsewhere, not saved".into(),
            Self::Failed(reason) => format!("Not saved: {reason}").into(),
        }
    }
}

/// What a window that holds the tab can follow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoteEditorEvent {
    /// The vault's head of the note is now `revision`, and is `saved` to
    /// disk or not. The vault's changes are announced as well, which is what
    /// the reading panel follows; this is for a window that wants the numbers.
    Synced { revision: u64, saved: bool },
    /// A link in the text was clicked: the window opens it.
    OpenLink(SharedString),
    /// A `[[wiki link]]` or `#tag` was clicked: the window opens that note.
    OpenWikiLink(SharedString),
}

/// The vault's head of a note as the tab last saw it. Derived from the
/// vault's authority; replaced whole by every flight.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Head {
    revision: u64,
    /// The text equals the file's, as far as the vault last observed it.
    saved: bool,
    text: Arc<str>,
}

/// One exchange with the vault.
enum Job {
    /// Open the note at the vault's head.
    Load,
    /// Make the buffer, as it was at `generation`, the head.
    Push {
        text: Arc<str>,
        generation: u64,
        revision: u64,
        operation: String,
    },
    Save,
    /// Look at the head again, because the vault announced a change.
    Sync,
    History(HistoryAction),
}

/// What a flight brings back.
struct Reply {
    head: Head,
    /// The vault refused the request (a push of a stale revision, an undo
    /// with nothing to undo). The head is still read.
    refused: Option<String>,
    /// A save found the file changed in a way that overlaps the text.
    conflict: bool,
}

/// The text a buffer was in step with, and when.
struct Base {
    text: Arc<str>,
    generation: u64,
}

/// Keys bound for every tab of the app, once.
struct KeysBound;
impl Global for KeysBound {}

/// The editor tab of one note.
pub struct NoteEditor {
    service: Service,
    path: String,
    editor: Entity<EditorState>,
    scroll: ScrollHandle,
    /// Colour scheme the editor's markdown style was last made for.
    dark: bool,
    /// `None` until the vault has opened the note.
    head: Option<Head>,
    /// The generation of the buffer whose text is `head.text`. The buffer is
    /// ahead of the vault exactly when its generation is another one.
    synced_generation: u64,
    /// A flight is out.
    busy: bool,
    /// The text of the push that is out, if one is: the vault may already hold
    /// it, which is this tab's own doing and not a change from elsewhere.
    pushing: Option<Arc<str>>,
    load_wanted: bool,
    save_wanted: bool,
    sync_wanted: bool,
    history: VecDeque<HistoryAction>,
    /// Text overlapped a change from elsewhere; cleared by a clean save.
    conflict: bool,
    /// Set by a refusal that moved nothing; cleared by typing or saving.
    error: Option<String>,
    /// Names this view's operations to the vault, which refuses a repeat.
    session: u128,
    operations: u64,
    autosave: Option<Task<()>>,
    retry: Option<Task<()>>,
    _watching: Task<()>,
    _editor_events: Subscription,
}

impl EventEmitter<NoteEditorEvent> for NoteEditor {}

impl Focusable for NoteEditor {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.read(cx).focus_handle(cx)
    }
}

impl NoteEditor {
    /// Opens `path`, a note of the vault `service` serves, for editing.
    pub fn new(
        service: Service,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        bind_keys(cx);
        let dark = cx.theme().is_dark();
        let editor = cx.new(|cx| {
            let mut editor = EditorState::new(window, cx);
            editor.set_hosted(true);
            editor.set_markdown_style(design::syntax(dark), cx);
            editor
        });
        let _editor_events = cx.subscribe_in(&editor, window, Self::on_editor_event);
        let mut changes = service.changes();
        let _watching = cx.spawn(async move |this, cx| {
            while changes.changed().await.is_ok() {
                let seen = this.update(cx, |this, cx| {
                    this.sync_wanted = true;
                    this.load_wanted |= this.head.is_none();
                    this.pump(cx);
                });
                if seen.is_err() {
                    break;
                }
            }
        });
        // What is typed or undone before the vault has answered is kept, not
        // lost: the load merges onto it.
        let synced_generation = editor.read(cx).generation();
        editor.update(cx, |editor, cx| editor.focus(window, cx));
        cx.on_release(|this, cx| this.flush(cx)).detach();
        let mut tab = Self {
            service,
            path,
            editor,
            scroll: ScrollHandle::new(),
            dark,
            head: None,
            synced_generation,
            busy: false,
            pushing: None,
            load_wanted: true,
            save_wanted: false,
            sync_wanted: false,
            history: VecDeque::new(),
            conflict: false,
            error: None,
            session: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos()),
            operations: 0,
            autosave: None,
            retry: None,
            _watching,
            _editor_events,
        };
        tab.pump(cx);
        tab
    }

    /// The revision of the vault's head, once the note is open.
    pub fn revision(&self) -> Option<u64> {
        self.head.as_ref().map(|head| head.revision)
    }

    /// The text on screen.
    pub fn text<'a>(&self, cx: &'a App) -> &'a str {
        self.editor.read(cx).text()
    }

    /// What the header says.
    pub fn status(&self, cx: &App) -> Status {
        if let Some(reason) = &self.error {
            return Status::Failed(reason.clone());
        }
        let Some(head) = &self.head else {
            return Status::Opening;
        };
        if self.conflict {
            Status::Conflict
        } else if head.saved && !self.ahead(cx) {
            Status::Saved
        } else {
            Status::Unsaved
        }
    }

    /// Saves now (⌘S).
    pub fn save(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.save_wanted = true;
        self.pump(cx);
    }

    /// The buffer has text the vault has not been given.
    fn ahead(&self, cx: &App) -> bool {
        self.editor.read(cx).generation() != self.synced_generation
    }

    fn on_editor_event(
        &mut self,
        _: &Entity<EditorState>,
        event: &EditorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            EditorEvent::Edited(edit) if edit.origin == EditOrigin::Local => {
                self.error = None;
                self.autosave = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(AUTOSAVE).await;
                    this.update(cx, |this, cx| {
                        if !matches!(this.status(cx), Status::Saved) {
                            this.save(cx);
                        }
                    })
                    .ok();
                }));
                self.reveal_caret(window);
                self.pump(cx);
                cx.notify();
            }
            EditorEvent::HistoryRequested { action, .. } => {
                if self.history.len() < MAX_HISTORY {
                    self.history.push_back(*action);
                }
                self.pump(cx);
            }
            EditorEvent::SelectionChanged => self.reveal_caret(window),
            EditorEvent::OpenLink(link) => cx.emit(NoteEditorEvent::OpenLink(link.clone())),
            EditorEvent::OpenWikiLink(link) => {
                cx.emit(NoteEditorEvent::OpenWikiLink(link.clone()));
            }
            _ => {}
        }
    }

    /// Scrolls so the caret stays clear of the tab's edges, once the editor
    /// has laid out the text it just changed.
    fn reveal_caret(&self, window: &mut Window) {
        let (editor, scroll) = (self.editor.clone(), self.scroll.clone());
        window.on_next_frame(move |window, cx| {
            let Some(caret) = editor.read(cx).caret_screen_bounds() else {
                return;
            };
            let view = scroll.bounds();
            let (top, bottom) = (
                view.top() + design::CARET_MARGIN,
                view.bottom() - design::CARET_MARGIN,
            );
            let mut offset = scroll.offset();
            if caret.bottom() > bottom {
                offset.y -= caret.bottom() - bottom;
            } else if caret.top() < top {
                offset.y += top - caret.top();
            } else {
                return;
            }
            scroll.set_offset(offset);
            window.refresh();
        });
    }

    /// Starts the next flight, if none is out and there is one to make.
    fn pump(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(job) = self.next_job(cx) else {
            return;
        };
        self.busy = true;
        self.pushing = match &job {
            Job::Push { text, .. } => Some(text.clone()),
            _ => None,
        };
        let (service, path) = (self.service.clone(), self.path.clone());
        cx.spawn(async move |this, cx| {
            let reply = fly(&service, path, &job).await;
            this.update(cx, |this, cx| {
                this.landed(job, reply, cx);
                this.pump(cx);
            })
            .ok();
        })
        .detach();
    }

    /// What the vault should be asked next. Text goes first, so that
    /// whatever follows (an undo, a save) works on all of it.
    fn next_job(&mut self, cx: &App) -> Option<Job> {
        let Some(head) = &self.head else {
            return std::mem::take(&mut self.load_wanted).then_some(Job::Load);
        };
        if self.error.is_none() && self.ahead(cx) {
            let editor = self.editor.read(cx);
            self.operations += 1;
            return Some(Job::Push {
                text: editor.text().into(),
                generation: editor.generation(),
                revision: head.revision,
                operation: format!("note-editor-{:x}-{}", self.session, self.operations),
            });
        }
        if let Some(action) = self.history.pop_front() {
            return Some(Job::History(action));
        }
        if std::mem::take(&mut self.save_wanted) {
            return Some(Job::Save);
        }
        std::mem::take(&mut self.sync_wanted).then_some(Job::Sync)
    }

    /// A flight has returned.
    fn landed(&mut self, job: Job, reply: Result<Reply, ServiceError>, cx: &mut Context<Self>) {
        self.busy = false;
        self.pushing = None;
        match reply {
            Ok(reply) => {
                let base = self.base_of(&job, reply.refused.is_some());
                if matches!(job, Job::Save) {
                    self.conflict = reply.conflict;
                }
                let moved = self.absorb(base, reply.head, cx);
                if let (Job::Push { .. }, Some(reason), false) = (&job, reply.refused, moved) {
                    self.error = Some(reason);
                }
            }
            Err(ServiceError::Busy) => self.again(job, cx),
            Err(error) => self.error = Some(error.to_string()),
        }
        cx.notify();
    }

    /// The vault's queue was full: make the request again shortly.
    fn again(&mut self, job: Job, cx: &mut Context<Self>) {
        match job {
            Job::Load => self.load_wanted = true,
            Job::Save => self.save_wanted = true,
            Job::Sync => self.sync_wanted = true,
            Job::History(action) => self.history.push_front(action),
            Job::Push { .. } => {}
        }
        self.retry.get_or_insert_with(|| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(BUSY_RETRY).await;
                this.update(cx, |this, cx| {
                    this.retry = None;
                    this.pump(cx);
                })
                .ok();
            })
        });
    }

    /// The text the buffer was in step with when `job` went out. A push the
    /// vault took is the new head, so it is its own base; one it refused
    /// changed nothing, and what the buffer grew from is the head before it.
    fn base_of(&self, job: &Job, refused: bool) -> Base {
        match job {
            Job::Push {
                text, generation, ..
            } if !refused => Base {
                text: text.clone(),
                generation: *generation,
            },
            _ => Base {
                text: self
                    .head
                    .as_ref()
                    .map_or_else(|| Arc::from(""), |head| head.text.clone()),
                generation: self.synced_generation,
            },
        }
    }

    /// Brings the buffer onto `head`, which was made from `base`, keeping
    /// what was typed since. True when `head` is a revision not seen before.
    fn absorb(&mut self, base: Base, head: Head, cx: &mut Context<Self>) -> bool {
        let opening = self.head.is_none();
        let (synced_generation, marked, applied) = self.editor.update(cx, |editor, cx| {
            let generation = editor.generation();
            if opening && generation == base.generation {
                editor.set_text(head.text.to_string(), cx);
                editor.set_cursor(0, cx);
                return (editor.generation(), false, true);
            }
            let buffer = editor.text();
            let (target, marked) = if head.text == base.text {
                (Cow::Borrowed(buffer), false)
            } else if generation == base.generation {
                (Cow::Borrowed(&*head.text), false)
            } else {
                match merge3(&base.text, buffer, &head.text) {
                    Ok(merged) => (Cow::Owned(merged), false),
                    Err(marked) => (Cow::Owned(marked), true),
                }
            };
            let edits = edits_between(buffer, &target);
            let matches_head = *target == *head.text;
            let applied = editor.apply_remote(generation, &edits, cx).is_ok();
            let synced = if matches_head {
                editor.generation()
            } else {
                base.generation
            };
            (synced, marked, applied)
        });
        if !applied {
            self.error = Some("the text could not be brought up to date".into());
        }
        self.synced_generation = synced_generation;
        self.conflict |= marked;
        let moved = self
            .head
            .as_ref()
            .is_none_or(|before| before.revision != head.revision);
        let changed = self
            .head
            .as_ref()
            .is_none_or(|before| (before.revision, before.saved) != (head.revision, head.saved));
        let event = NoteEditorEvent::Synced {
            revision: head.revision,
            saved: head.saved,
        };
        self.head = Some(head);
        if changed {
            cx.emit(event);
        }
        moved
    }

    /// The tab is closing: leave the vault with everything typed, saved.
    /// Fire and forget: the request is queued before this returns, and the
    /// vault's thread works through its queue in order.
    fn flush(&mut self, cx: &mut App) {
        let Some(head) = &self.head else {
            return;
        };
        let editor = self.editor.read(cx);
        if head.saved && editor.generation() == self.synced_generation {
            return;
        }
        let (base, ours, pushing) = (
            head.text.clone(),
            editor.text().to_owned(),
            self.pushing.clone(),
        );
        let (path, operation) = (
            self.path.clone(),
            format!("note-editor-{:x}-close", self.session),
        );
        drop(
            self.service
                .write(move |vault| {
                    let current = head_of(vault, &path)?;
                    // What came in meanwhile stays: the typed text is merged
                    // onto it, never put over it. A head that is only this
                    // tab's push of an earlier state of its text came from
                    // nowhere else, and the text now supersedes it.
                    let merged =
                        if *current.text == *ours || Some(&current.text) == pushing.as_ref() {
                            ours
                        } else {
                            merge3(&base, &ours, &current.text).unwrap_or_else(|marked| marked)
                        };
                    if merged != *current.text {
                        vault.edit(&path, current.revision, &operation, &merged)?;
                    }
                    vault.save(&path)?;
                    Ok(())
                })
                .now_or_never(),
        );
    }
}

/// Runs `job` on the vault's thread.
async fn fly(service: &Service, path: String, job: &Job) -> Result<Reply, ServiceError> {
    match job {
        Job::Load => {
            service
                .write(move |vault| {
                    vault.open_document(&path)?;
                    head_of(vault, &path).map(Reply::of)
                })
                .await
        }
        Job::Push {
            text,
            revision,
            operation,
            ..
        } => {
            let (text, revision, operation) = (text.clone(), *revision, operation.clone());
            service
                .write(move |vault| {
                    let refused = vault
                        .edit(&path, revision, &operation, &text)
                        .err()
                        .map(|error| format!("{error:#}"));
                    Ok(Reply {
                        head: head_of(vault, &path)?,
                        refused,
                        conflict: false,
                    })
                })
                .await
        }
        Job::Save => {
            service
                .write(move |vault| {
                    let conflict = matches!(vault.save(&path)?, ReconcileResult::Conflict { .. });
                    Ok(Reply {
                        head: head_of(vault, &path)?,
                        refused: None,
                        conflict,
                    })
                })
                .await
        }
        Job::Sync => {
            service
                .read(move |vault| head_of(vault, &path).map(Reply::of))
                .await
        }
        Job::History(action) => {
            let action = *action;
            service
                .write(move |vault| {
                    let stepped = match action {
                        HistoryAction::Undo => vault.undo(&path),
                        HistoryAction::Redo => vault.redo(&path),
                    };
                    Ok(Reply {
                        head: head_of(vault, &path)?,
                        refused: stepped.err().map(|error| format!("{error:#}")),
                        conflict: false,
                    })
                })
                .await
        }
    }
}

impl Reply {
    fn of(head: Head) -> Self {
        Self {
            head,
            refused: None,
            conflict: false,
        }
    }
}

/// The vault's head of `path`, from its authority.
fn head_of(vault: &Vault, path: &str) -> Result<Head> {
    let authority = vault.authority();
    let id = authority
        .resolve(WORKSPACE, path)
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .with_context(|| format!("{path} is not open in the vault"))?;
    let head = authority
        .head(&id)
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .with_context(|| format!("{path} has no head in the vault"))?;
    Ok(Head {
        revision: head.revision,
        saved: head.disk_hash.as_deref() == Some(head.content_hash.as_str()),
        text: head.text.into(),
    })
}

/// Binds the editor's keys and the tab's own, once for the app. All of them
/// are scoped to a key context, so none reaches the browser's shortcuts.
fn bind_keys(cx: &mut App) {
    if cx.has_global::<KeysBound>() {
        return;
    }
    cx.set_global(KeysBound);
    zorite_editor::bind_keys(cx);
    cx.bind_keys([KeyBinding::new("cmd-s", Save, Some(KEY_CONTEXT))]);
}

impl Render for NoteEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = cx.theme().is_dark();
        if dark != self.dark {
            self.dark = dark;
            self.editor.update(cx, |editor, cx| {
                editor.set_markdown_style(design::syntax(dark), cx)
            });
        }
        let theme = cx.theme();
        let (text, faint) = (theme.foreground, theme.foreground.opacity(design::FAINT));
        let status = self.status(cx);
        let status_color = match status {
            Status::Conflict => design::conflict(dark),
            Status::Failed(_) => design::failure(dark),
            _ => faint,
        };
        let viewport = window.viewport_size();
        let side = if viewport.width < px(design::COLUMN_MAX_WIDTH) {
            design::SIDE_PADDING_NARROW
        } else {
            design::SIDE_PADDING
        };
        let name = stem(&self.path)
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned();

        v_flex()
            .size_full()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(|this, _: &Save, _, cx| this.save(cx)))
            .child(
                h_flex()
                    .flex_none()
                    .h(px(design::HEADER_HEIGHT))
                    .px(px(design::HEADER_PADDING))
                    .items_center()
                    .gap(px(design::HEADER_PADDING))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(design::STATE_SIZE))
                            .text_color(status_color)
                            .child(status.label()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(design::COLUMN_MAX_WIDTH))
                            .truncate()
                            .text_size(px(design::NAME_SIZE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(text)
                            .child(name.clone()),
                    )
                    .child(div().flex_1()),
            )
            .child(
                div()
                    .id("note-scroll")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        v_flex()
                            .mx_auto()
                            .w_full()
                            .max_w(px(design::COLUMN_MAX_WIDTH))
                            .px(px(side))
                            .pt(px(design::TOP_PADDING))
                            .text_size(px(design::TEXT_SIZE))
                            .text_color(text)
                            .child(
                                div()
                                    .mb(px(design::TITLE_GAP))
                                    .text_size(px(design::TITLE_SIZE))
                                    .font_weight(FontWeight::BOLD)
                                    .line_height(relative(design::TITLE_LINE_HEIGHT))
                                    .child(name),
                            )
                            .child(self.editor.clone())
                            .child(div().h(viewport.height * design::BOTTOM_SPACE)),
                    ),
            )
    }
}
