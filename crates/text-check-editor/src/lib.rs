//! Platinum's shared text checking, attached to a `zorite-editor`.
//!
//! [`TextChecking`] watches one editor. After the person pauses it checks the
//! text off the UI thread, underlines what it found, and on a right-click on
//! an underline supplies the menu rows. Choosing a correction replaces the
//! span through the editor's own `replace_range`, so it is one ordinary undo
//! step, and only after proving the text is still what was checked.
//!
//! What it will not do: apply anything without a choice, apply anything while
//! an input-method composition is active, apply a result computed for text
//! that has since changed, or touch a character outside the flagged span.

use std::{cell::RefCell, ops::Range, rc::Rc, sync::Arc, time::Duration};

use frame_text_check::{
    Action, CheckOutcome, CurrentText, Field, FieldPolicy, Menu, RevisionId, Service,
    check_note_in, revision_of,
};
use gpui::{AppContext as _, Context, Entity, Subscription, Task};
use zorite_editor::{Diagnostic, EditorEvent, EditorState, SpanContext};

/// How long the text must be still before it is checked.
pub const DEBOUNCE: Duration = Duration::from_millis(400);

/// What the editor's provider and the event handler share.
#[derive(Default)]
struct Shared {
    field: Field,
    /// The menu most recently shown, awaiting the person's choice.
    menu: Option<Menu>,
}

/// What a check was asked about; a repeat is not run again.
type Question = (RevisionId, Range<usize>, Option<Range<usize>>);

pub struct TextChecking {
    editor: Entity<EditorState>,
    service: Arc<Service>,
    policy: FieldPolicy,
    shared: Rc<RefCell<Shared>>,
    asked: Option<Question>,
    pending: Option<Task<()>>,
    /// Why the last chosen row changed nothing, if it did not.
    refusal: Option<String>,
    _watch: Subscription,
    _events: Subscription,
}

impl TextChecking {
    /// Check `editor`'s text as prose under `policy`.
    pub fn attach(
        editor: &Entity<EditorState>,
        service: Arc<Service>,
        policy: FieldPolicy,
        cx: &mut gpui::App,
    ) -> Entity<Self> {
        let editor = editor.clone();
        cx.new(|cx: &mut Context<Self>| {
            let shared = Rc::new(RefCell::new(Shared::default()));
            editor.update(cx, |editor, _| {
                editor.on_span_actions(rows(service.clone(), shared.clone()));
            });
            let watch = cx.observe(&editor, |this, _, cx| this.schedule(cx));
            let events = cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| {
                if let EditorEvent::SpanAction { range, index } = event {
                    this.choose(range.clone(), *index, cx);
                }
            });
            let mut this = Self {
                editor,
                service,
                policy,
                shared,
                asked: None,
                pending: None,
                refusal: None,
                _watch: watch,
                _events: events,
            };
            this.schedule(cx);
            this
        })
    }

    /// Why the last chosen correction was not made, if it was not.
    #[must_use]
    pub fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }

    /// Change what is checked: switch checking off for this editor, or mark
    /// the field protected. Underlines update at the next check.
    pub fn set_policy(&mut self, policy: FieldPolicy, cx: &mut Context<Self>) {
        self.policy = policy;
        self.asked = None;
        self.schedule(cx);
    }

    /// Ask for a check once the text has been still for [`DEBOUNCE`].
    fn schedule(&mut self, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        let question = (
            revision_of(editor.text()),
            editor.selected_range(),
            editor.marked_range(),
        );
        if self.asked.as_ref() == Some(&question) {
            return;
        }
        let text = editor.text().to_owned();
        let (_, selection, composition) = question.clone();
        self.asked = Some(question);
        let service = self.service.clone();
        let policy = self.policy.clone();
        // Replacing the task drops the previous one, which cancels it.
        self.pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    check_note_in(&service, &text, Some(selection), composition, &policy)
                })
                .await;
            let _ = this.update(cx, |this, cx| this.finish(outcome, cx));
        }));
    }

    /// Show a result, if it still describes the text.
    fn finish(&mut self, outcome: CheckOutcome, cx: &mut Context<Self>) {
        let underlines: Vec<Diagnostic> = {
            let text = self.editor.read(cx).text();
            let mut shared = self.shared.borrow_mut();
            // The person kept typing: the newer check is already scheduled.
            if !shared.field.accept(outcome, text) {
                return;
            }
            shared
                .field
                .outcome_for(text)
                .into_iter()
                .flat_map(|found| &found.diagnostics)
                .map(|d| Diagnostic {
                    range: d.byte_range.clone(),
                })
                .collect()
        };
        self.editor
            .update(cx, |editor, cx| editor.set_diagnostics(underlines, cx));
    }

    /// The person chose row `index` of the menu for the span `range`.
    fn choose(&mut self, range: Range<usize>, index: usize, cx: &mut Context<Self>) {
        let menu = self.shared.borrow_mut().menu.take();
        let Some(menu) = menu.filter(|menu| menu.byte_range() == range) else {
            return;
        };
        let (text, composing) = {
            let editor = self.editor.read(cx);
            (editor.text().to_owned(), editor.marked_range().is_some())
        };
        let revision = revision_of(&text);
        let now = CurrentText {
            revision: &revision,
            text: &text,
            composing,
        };
        self.refusal = None;
        match menu.resolve(index, &now) {
            Ok(Action::Edit(edit)) => {
                self.editor.update(cx, |editor, cx| {
                    editor.replace_range(edit.byte_range.clone(), &edit.replacement, cx);
                    cx.emit(EditorEvent::Changed);
                });
            }
            Ok(profile) => {
                if let Err(error) = profile.run(&self.service) {
                    log::warn!("text check profile not saved: {error}");
                    self.refusal = Some(error.to_string());
                }
                // A new word or a rule switched off changes what is flagged.
                self.asked = None;
                self.schedule(cx);
            }
            Err(refusal) => {
                self.refusal = Some(refusal.to_string());
                self.asked = None;
                self.schedule(cx);
            }
        }
        cx.notify();
    }
}

/// The editor's right-click provider: the rows for a flagged span, from the
/// last result if, and only if, it still describes the text.
fn rows(
    service: Arc<Service>,
    shared: Rc<RefCell<Shared>>,
) -> impl Fn(SpanContext<'_>) -> Vec<String> + 'static {
    move |span| {
        let mut shared = shared.borrow_mut();
        shared.menu = None;
        if span.composing {
            return Vec::new();
        }
        let Some(outcome) = shared.field.outcome_for(span.text).cloned() else {
            return Vec::new();
        };
        let Some(id) = outcome
            .diagnostics
            .iter()
            .find(|d| d.byte_range == span.range)
            .map(|d| d.id.clone())
        else {
            return Vec::new();
        };
        let Ok(menu) = Menu::new(&service, &outcome, &id) else {
            return Vec::new();
        };
        let labels = menu.rows().iter().map(|row| row.label.clone()).collect();
        shared.menu = Some(menu);
        labels
    }
}

#[cfg(test)]
mod tests;
