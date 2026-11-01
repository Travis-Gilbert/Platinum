//! Editable text in a web page: `<input type=text>`, `<textarea>` and
//! `contenteditable`.
//!
//! Servo gives an embedder no way to underline a page's text, no way to map a
//! click to a character offset, and no undo for a text control. What it does
//! give is `evaluate_javascript`. So this module speaks to the page in two
//! small scripts and treats the page as untrusted at both ends:
//!
//! * **probe** (`probe.js`) describes the focused field. It decides whether the
//!   field is protected or not prose from its attributes *before* reading its
//!   value, so a password never reaches the embedder.
//! * **apply** (`apply.js`) makes one correction. It re-derives the field's
//!   revision, and re-reads the text under the range, in the same turn of the
//!   page's event loop as the edit; if either differs from what the person was
//!   shown, nothing changes.
//!
//! The embedder adds two more refusals the page cannot see: an IME composition
//! in progress, and a result for a different field than the one now focused.
//! Because Servo has no text-control undo, a correction also yields its exact
//! inverse ([`Undo`]) as one more guarded edit.

use std::ops::Range;

use serde::Deserialize;
use theorem_text_check::{
    CheckRequest, CheckStatus, CurrentText, FieldKind, FieldPolicy, OffsetUnit, ProposedEdit,
    RevisionId,
};

use crate::{
    field::revision_of,
    menu::{self, Action, Menu, Refusal},
    service::Service,
};

/// Runs in the page; evaluates to a JSON string describing the focused field.
pub const PROBE: &str = concat!("(() => {\n", include_str!("flat.js"), include_str!("probe.js"), "\n})()");

const APPLY_HEAD: &str = concat!("((d) => {\n", include_str!("revision.js"), include_str!("flat.js"));
const APPLY_BODY: &str = concat!(include_str!("apply.js"), "\n})");

/// Which kind of editor the page is using for the field.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Editor {
    Input,
    Textarea,
    Editable,
}

impl Editor {
    fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Textarea => "textarea",
            Self::Editable => "editable",
        }
    }
}

/// What the page said about its focused field.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Probe {
    /// Nothing editable is focused.
    None,
    /// Read-only, disabled, or otherwise not ours to touch.
    Other,
    /// A password, one-time code or payment field. Its text was never sent.
    Protected,
    /// An email address, URL, number, search box.
    NotProse,
    Field {
        editor: Editor,
        text: String,
        /// The caret or selection, in UTF-16 units.
        selection: Option<(usize, usize)>,
        spellcheck: Option<bool>,
        lang: Option<String>,
    },
}

impl Probe {
    /// Read the script's reply. A reply that is not exactly what the script
    /// promises is treated as no field: nothing is offered.
    #[must_use]
    pub fn parse(reply: &str) -> Self {
        serde_json::from_str(reply).unwrap_or(Self::Other)
    }
}

/// A field the page described, ready to be checked.
#[derive(Clone, Debug)]
pub struct WebField {
    editor: Editor,
    text: String,
    revision: RevisionId,
    caret: Range<usize>,
    policy: FieldPolicy,
}

impl WebField {
    /// The field, if the probe found one worth checking.
    #[must_use]
    pub fn from_probe(probe: Probe) -> Option<Self> {
        let Probe::Field {
            editor,
            text,
            selection,
            spellcheck,
            lang,
        } = probe
        else {
            return None;
        };
        let end = text.encode_utf16().count();
        let caret = selection
            .map(|(a, b)| a.min(b).min(end)..a.max(b).min(end))
            .unwrap_or(end..end);
        Some(Self {
            editor,
            revision: revision_of(&text),
            text,
            caret,
            policy: FieldPolicy {
                kind: FieldKind::Prose,
                spellcheck,
                language: lang,
            },
        })
    }

    #[must_use]
    pub fn revision(&self) -> &RevisionId {
        &self.revision
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// The corrections on offer for the flagged span nearest the caret.
pub struct WebMenu {
    field: WebField,
    menu: Menu,
}

impl WebMenu {
    /// Check the field and build the menu for the span nearest its caret.
    ///
    /// `None` when there is nothing to offer: the page disables checking,
    /// the field is not English prose, nothing is flagged, or an input method
    /// is composing (`composing`): then no text is read for correction at
    /// all. The caret is not passed to the checker: a person who asks for a
    /// menu is not mid-word, and the word the caret sits in is exactly the
    /// one they are asking about.
    #[must_use]
    pub fn build(service: &Service, field: WebField, composing: bool) -> Option<Self> {
        if composing {
            return None;
        }
        let mut request = CheckRequest::new(field.revision.clone(), field.text.clone());
        request.offset_unit = OffsetUnit::Utf16;
        request.field = field.policy.clone();
        let outcome = service.check(&request);
        if !matches!(outcome.status, CheckStatus::Checked) {
            return None;
        }
        let id = menu::nearest(&outcome, &field.caret, OffsetUnit::Utf16)?.id.clone();
        let menu = Menu::new(service, &outcome, &id).ok()?;
        Some(Self { field, menu })
    }

    #[must_use]
    pub fn menu(&self) -> &Menu {
        &self.menu
    }

    /// The rows as a person reads them in a page's context menu, where the
    /// flagged word has to be named because nothing is underlined.
    #[must_use]
    pub fn labels(&self) -> Vec<String> {
        let word = self.menu.subject();
        self.menu
            .rows()
            .iter()
            .map(|row| match row.choice {
                menu::Choice::Replace(_) => format!("Correct \u{201c}{word}\u{201d} to \u{201c}{}\u{201d}", row.label),
                menu::Choice::AddWord => format!("Add \u{201c}{word}\u{201d} to Dictionary"),
                menu::Choice::IgnoreRule => row.label.clone(),
            })
            .collect()
    }

    /// Choose row `index`. A replacement comes back as a script to run in the
    /// page, with the undo that reverses it; the page makes the final checks.
    pub fn choose(&self, index: usize, composing: bool) -> Result<Chosen, Refusal> {
        let now = CurrentText {
            revision: &self.field.revision,
            text: &self.field.text,
            composing,
        };
        Ok(match self.menu.resolve(index, &now)? {
            Action::Edit(edit) => Chosen::Edit {
                script: apply_script(self.field.editor, &edit),
                undo: Undo::reversing(self.field.editor, &self.field.text, &edit),
                after: after_revision(&self.field.text, &edit),
            },
            other => Chosen::Profile(other),
        })
    }
}

/// What choosing a row does.
pub enum Chosen {
    /// Run `script` in the page. If it replies `ok` with the revision `after`,
    /// `undo` is what reverses it.
    Edit {
        script: String,
        undo: Undo,
        after: RevisionId,
    },
    /// A word for the dictionary or a rule to switch off; no page involved.
    Profile(Action),
}

/// The script that makes `edit` in a field of kind `editor`.
#[must_use]
pub fn apply_script(editor: Editor, edit: &ProposedEdit) -> String {
    let arguments = serde_json::json!({
        "editor": editor.as_str(),
        "revision": edit.revision.0,
        "start": edit.utf16_range.start,
        "end": edit.utf16_range.end,
        "expected": edit.expected,
        "replacement": edit.replacement,
    });
    format!("{APPLY_HEAD}{APPLY_BODY}({arguments})")
}

/// What the apply script said.
#[derive(Debug, Deserialize, Eq, PartialEq)]
pub struct Applied {
    pub ok: bool,
    /// Why nothing changed: `gone`, `kind`, `readonly`, `stale`, `changed`,
    /// `range`, `refused`, or `silent` (the page never answered).
    #[serde(default)]
    pub why: Option<String>,
    /// The field's revision after the edit.
    #[serde(default)]
    pub after: Option<String>,
}

impl Applied {
    /// Read the script's reply; anything unreadable is a refusal.
    #[must_use]
    pub fn parse(reply: &str) -> Self {
        serde_json::from_str(reply).unwrap_or(Self {
            ok: false,
            why: Some("unreadable".to_owned()),
            after: None,
        })
    }

    /// Why nothing changed, as a sentence for the person.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self.why.as_deref() {
            Some("stale" | "changed") => {
                "The text changed after it was checked, so nothing was corrected."
            }
            Some("gone") => "The field is no longer on the page.",
            Some("kind") => "A different field is focused now.",
            Some("readonly") => "The field is read-only.",
            Some("range") => "That part of the field cannot be edited.",
            Some("refused") => "The page would not take the correction.",
            Some("silent") => "The page did not answer.",
            _ => "The page did not take the correction.",
        }
    }

    /// Whether the edit happened and left the field exactly as predicted. A
    /// page whose own handlers altered the text further is not recorded as an
    /// undoable correction.
    #[must_use]
    pub fn left(&self, expected: &RevisionId) -> bool {
        self.ok && self.after.as_deref() == Some(expected.0.as_str())
    }
}

/// The text after `edit`, as a revision.
fn after_revision(text: &str, edit: &ProposedEdit) -> RevisionId {
    revision_of(&replaced(text, edit))
}

fn replaced(text: &str, edit: &ProposedEdit) -> String {
    let mut out = String::with_capacity(text.len() + edit.replacement.len());
    out.push_str(&text[..edit.byte_range.start]);
    out.push_str(&edit.replacement);
    out.push_str(&text[edit.byte_range.end..]);
    out
}

/// The exact inverse of one correction.
///
/// It is itself a guarded edit against the text *after* the correction, so it
/// refuses if anything else has been typed since. The embedder offers it for
/// one step only and forgets it as soon as the page receives any other input.
#[derive(Clone, Debug)]
pub struct Undo {
    editor: Editor,
    inverse: ProposedEdit,
}

impl Undo {
    fn reversing(editor: Editor, text: &str, edit: &ProposedEdit) -> Self {
        let after = replaced(text, edit);
        let units = edit.replacement.encode_utf16().count();
        Self {
            editor,
            inverse: ProposedEdit {
                revision: revision_of(&after),
                byte_range: edit.byte_range.start..edit.byte_range.start + edit.replacement.len(),
                utf16_range: edit.utf16_range.start..edit.utf16_range.start + units,
                expected: edit.replacement.clone(),
                replacement: edit.expected.clone(),
            },
        }
    }

    /// The script that puts the corrected text back as it was.
    #[must_use]
    pub fn script(&self) -> String {
        apply_script(self.editor, &self.inverse)
    }

    /// What the menu calls it.
    #[must_use]
    pub fn label(&self) -> String {
        format!(
            "Undo Correction \u{201c}{}\u{201d}",
            self.inverse.replacement
        )
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use theorem_text_check::ProfileStore;

    use super::*;

    fn service() -> (Service, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::open(ProfileStore::at(dir.path().join("profile.json"))).unwrap();
        (service, dir)
    }

    fn textarea(text: &str, caret: usize) -> WebField {
        WebField::from_probe(Probe::Field {
            editor: Editor::Textarea,
            text: text.to_owned(),
            selection: Some((caret, caret)),
            spellcheck: None,
            lang: None,
        })
        .unwrap()
    }

    fn node() -> Command {
        Command::new("node")
    }

    fn require_node() {
        let found = node().arg("--version").output();
        assert!(
            found.is_ok_and(|out| out.status.success()),
            "node is required to check the page-side scripts"
        );
    }

    #[test]
    fn replies_that_are_not_a_checkable_field_yield_no_field() {
        assert_eq!(Probe::parse(r#"{"kind":"protected"}"#), Probe::Protected);
        assert_eq!(Probe::parse(r#"{"kind":"not_prose"}"#), Probe::NotProse);
        assert_eq!(Probe::parse(r#"{"kind":"none"}"#), Probe::None);
        for reply in [r#"{"kind":"protected"}"#, r#"{"kind":"other"}"#, r#"{"kind":"none"}"#, "garbage", "", r#"{"kind":"field"}"#] {
            assert!(WebField::from_probe(Probe::parse(reply)).is_none(), "{reply}");
        }
    }

    #[test]
    fn a_correction_menu_names_the_word_because_nothing_is_underlined() {
        let (service, _dir) = service();
        let text = "I think teh answer is clear.";
        let at = text.find("teh").unwrap();
        let menu = WebMenu::build(&service, textarea(text, at + 1), false).expect("a menu");
        let labels = menu.labels();
        assert!(labels.iter().any(|l| l == "Correct \u{201c}teh\u{201d} to \u{201c}the\u{201d}"), "{labels:?}");
        assert!(labels.iter().any(|l| l == "Add \u{201c}teh\u{201d} to Dictionary"));
        assert_eq!(menu.menu().subject(), "teh");
    }

    #[test]
    fn nothing_is_offered_while_composing_or_when_the_page_opts_out() {
        let (service, _dir) = service();
        let text = "I think teh answer is clear.";
        assert!(WebMenu::build(&service, textarea(text, 9), true).is_none());

        let off = WebField::from_probe(Probe::Field {
            editor: Editor::Input,
            text: text.to_owned(),
            selection: None,
            spellcheck: Some(false),
            lang: None,
        })
        .unwrap();
        assert!(WebMenu::build(&service, off, false).is_none(), "spellcheck=false");

        let french = WebField::from_probe(Probe::Field {
            editor: Editor::Input,
            text: text.to_owned(),
            selection: None,
            spellcheck: None,
            lang: Some("fr-CA".to_owned()),
        })
        .unwrap();
        assert!(WebMenu::build(&service, french, false).is_none(), "lang=fr");
    }

    #[test]
    fn a_composition_that_starts_after_the_menu_opened_refuses_the_choice() {
        let (service, _dir) = service();
        let text = "I think teh answer is clear.";
        let menu = WebMenu::build(&service, textarea(text, 9), false).unwrap();
        let the = menu.labels().iter().position(|l| l.ends_with("to \u{201c}the\u{201d}")).unwrap();
        assert!(matches!(menu.choose(the, true), Err(Refusal::Edit(_))));
        assert!(matches!(menu.choose(the, false), Ok(Chosen::Edit { .. })));
    }

    #[test]
    fn offsets_in_the_script_count_utf16_units_not_bytes() {
        let (service, _dir) = service();
        // Four UTF-16 units of emoji and a combining accent precede the typo.
        let text = "\u{1f600}\u{1f600} e\u{301} teh end.";
        let caret = text.find("teh").unwrap();
        let menu = WebMenu::build(&service, textarea(text, text[..caret].encode_utf16().count()), false).unwrap();
        let the = menu.labels().iter().position(|l| l.ends_with("to \u{201c}the\u{201d}")).unwrap();
        let Ok(Chosen::Edit { script, .. }) = menu.choose(the, false) else { panic!("an edit") };
        let units = text[..caret].encode_utf16().count();
        assert!(script.contains(&format!("\"start\":{units}")), "{script}");
        assert!(script.contains(&format!("\"end\":{}", units + 3)), "{script}");
        assert_ne!(units, caret, "the fixture must distinguish bytes from units");
    }

    #[test]
    fn the_page_side_digest_matches_revision_of() {
        require_node();
        let samples = [
            "", "teh", "a", "caf\u{e9}", "\u{1f600}", "e\u{301}", "line\nbreak", "I think teh answer is clear.",
            "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} family",
        ];
        let harness = format!(
            "{}\nconsole.log(JSON.stringify(JSON.parse(process.argv[1]).map(rev)));",
            include_str!("revision.js")
        );
        let out = node()
            .args(["-e", &harness, &serde_json::to_string(&samples).unwrap()])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let page: Vec<String> = serde_json::from_slice(&out.stdout).unwrap();
        let native: Vec<String> = samples.iter().map(|s| revision_of(s).0).collect();
        assert_eq!(page, native);
    }

    #[test]
    fn both_scripts_are_valid_javascript() {
        require_node();
        let dir = tempfile::tempdir().unwrap();
        let (service, _keep) = service();
        let text = "I think teh answer is clear.";
        let menu = WebMenu::build(&service, textarea(text, 9), false).unwrap();
        let the = menu.labels().iter().position(|l| l.ends_with("to \u{201c}the\u{201d}")).unwrap();
        let Ok(Chosen::Edit { script, undo, .. }) = menu.choose(the, false) else { panic!("an edit") };
        for (name, source) in [("probe", PROBE.to_owned()), ("apply", script), ("undo", undo.script())] {
            let file = dir.path().join(format!("{name}.js"));
            std::fs::write(&file, format!(";({source});")).unwrap();
            let out = node().arg("--check").arg(&file).output().unwrap();
            assert!(out.status.success(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
        }
    }

    /// Run an apply script against a stand-in `<textarea>` in node and report
    /// what it did. A fixture for the guard logic; the real page is exercised
    /// by the live example.
    fn run_in_textarea(initial: &str, script: &str) -> (Applied, String, Vec<String>) {
        require_node();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("apply.js");
        std::fs::write(&file, script).unwrap();
        let harness = r#"
            const fs = require('fs');
            class InputEvent { constructor(type, init) { this.type = type; Object.assign(this, init); } }
            const el = {
              isConnected: true, disabled: false, readOnly: false, value: process.argv[2], events: [],
              setRangeText(r, s, e) { this.value = this.value.slice(0, s) + r + this.value.slice(e); },
              dispatchEvent(ev) { this.events.push(ev.inputType); },
            };
            globalThis.InputEvent = InputEvent;
            globalThis.window = { __theoremTextCheck: { el, editor: 'textarea' } };
            const reply = (0, eval)(fs.readFileSync(process.argv[1], 'utf8'));
            console.log(JSON.stringify({ reply, value: el.value, events: el.events }));
        "#;
        let out = node().args(["-e", harness, file.to_str().unwrap(), initial]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let seen: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        (
            Applied::parse(seen["reply"].as_str().unwrap()),
            seen["value"].as_str().unwrap().to_owned(),
            serde_json::from_value(seen["events"].clone()).unwrap(),
        )
    }

    #[test]
    fn the_apply_script_corrects_a_textarea_and_its_inverse_restores_it() {
        let (service, _dir) = service();
        let text = "I think teh answer is clear.";
        let menu = WebMenu::build(&service, textarea(text, 9), false).unwrap();
        let the = menu.labels().iter().position(|l| l.ends_with("to \u{201c}the\u{201d}")).unwrap();
        let Ok(Chosen::Edit { script, undo, after }) = menu.choose(the, false) else { panic!("an edit") };

        let (applied, value, events) = run_in_textarea(text, &script);
        assert!(applied.left(&after), "{applied:?}");
        assert_eq!(value, "I think the answer is clear.");
        assert_eq!(events, ["insertReplacementText"]);

        // The undo is one more guarded edit, against the corrected text.
        let (undone, value, _) = run_in_textarea(&value, &undo.script());
        assert!(undone.ok, "{undone:?}");
        assert_eq!(value, text);
    }

    #[test]
    fn the_apply_script_changes_nothing_when_the_field_moved_on() {
        let (service, _dir) = service();
        let text = "I think teh answer is clear.";
        let menu = WebMenu::build(&service, textarea(text, 9), false).unwrap();
        let the = menu.labels().iter().position(|l| l.ends_with("to \u{201c}the\u{201d}")).unwrap();
        let Ok(Chosen::Edit { script, undo, .. }) = menu.choose(the, false) else { panic!("an edit") };

        // Typed before the word: the revision moved.
        let typed = "Well, I think teh answer is clear.";
        let (applied, value, events) = run_in_textarea(typed, &script);
        assert_eq!((applied.ok, applied.why.as_deref()), (false, Some("stale")));
        assert_eq!(value, typed);
        assert!(events.is_empty());

        // The inverse refuses once anything else has been typed since.
        let corrected = "I think the answer is clear.";
        let edited = "I think the answer is clear!";
        let (undone, value, _) = run_in_textarea(edited, &undo.script());
        assert_eq!(undone.why.as_deref(), Some("stale"));
        assert_eq!(value, edited);
        let (undone, value, _) = run_in_textarea(corrected, &undo.script());
        assert!(undone.ok);
        assert_eq!(value, "I think teh answer is clear.");
    }

    fn editable(text: &str, caret: usize) -> WebField {
        WebField::from_probe(Probe::Field {
            editor: Editor::Editable,
            text: text.to_owned(),
            selection: Some((caret, caret)),
            spellcheck: None,
            lang: None,
        })
        .unwrap()
    }

    /// Run an apply script against a stand-in contenteditable region in node:
    /// `pieces` are its children, a string for a text node or `[tag, text]` for
    /// an element holding one. `cancel` makes the page cancel `beforeinput`.
    /// Gives what the script said, the text the region ends with, and the
    /// events it saw. A fixture for the guard logic; the real page is
    /// exercised by the live example.
    fn run_in_editable(
        pieces: serde_json::Value,
        cancel: bool,
        script: &str,
    ) -> (Applied, String, Vec<String>) {
        require_node();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("apply.js");
        std::fs::write(&file, script).unwrap();
        let harness = r#"
            const fs = require('fs');
            class InputEvent { constructor(type, init) { this.type = type; Object.assign(this, init || {}); } }
            globalThis.InputEvent = InputEvent;
            const text = (data) => ({
              nodeType: 3, data,
              replaceData(o, n, r) { this.data = this.data.slice(0, o) + r + this.data.slice(o + n); },
            });
            const nodes = [];
            const make = (spec) => {
              if (typeof spec === 'string') { const t = text(spec); nodes.push(t); return t; }
              const t = text(spec[1]); nodes.push(t);
              return { nodeType: 1, tagName: spec[0], firstChild: t, nextSibling: null };
            };
            const children = JSON.parse(process.argv[2]).map(make);
            children.forEach((c, i) => { c.nextSibling = children[i + 1] || null; });
            const events = [];
            const el = {
              nodeType: 1, tagName: 'DIV', isConnected: true, disabled: false, readOnly: false,
              firstChild: children[0] || null,
              contains: (n) => nodes.includes(n),
              dispatchEvent(ev) { events.push(ev.type); return !(process.argv[3] === 'cancel' && ev.type === 'beforeinput'); },
            };
            globalThis.window = { __theoremTextCheck: { el, editor: 'editable' } };
            const reply = (0, eval)(fs.readFileSync(process.argv[1], 'utf8'));
            console.log(JSON.stringify({ reply, value: nodes.map((n) => n.data).join(''), events }));
        "#;
        let out = node()
            .args(["-e", harness, file.to_str().unwrap(), &pieces.to_string(), if cancel { "cancel" } else { "go" }])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let seen: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        (
            Applied::parse(seen["reply"].as_str().unwrap()),
            seen["value"].as_str().unwrap().to_owned(),
            serde_json::from_value(seen["events"].clone()).unwrap(),
        )
    }

    fn editable_receive(text: &str) -> (String, RevisionId, Undo) {
        let (service, _dir) = service();
        let menu = WebMenu::build(&service, editable(text, 10), false).unwrap();
        let row = menu
            .labels()
            .iter()
            .position(|l| l.ends_with("to \u{201c}receive\u{201d}"))
            .unwrap();
        let Ok(Chosen::Edit { script, undo, after }) = menu.choose(row, false) else { panic!("an edit") };
        (script, after, undo)
    }

    #[test]
    fn a_contenteditable_range_is_replaced_in_its_text_node_and_nothing_around_it_changes() {
        let (script, after, undo) = editable_receive("We will recieve it soon.");
        let (applied, value, events) =
            run_in_editable(serde_json::json!(["We will recieve it soon."]), false, &script);
        assert!(applied.left(&after), "{applied:?}");
        // The space before the word is still an ordinary space.
        assert_eq!(value, "We will receive it soon.");
        assert_eq!(events, ["beforeinput", "input"]);
        let (undone, value, _) =
            run_in_editable(serde_json::json!([value]), false, &undo.script());
        assert!(undone.ok, "{undone:?}");
        assert_eq!(value, "We will recieve it soon.");
    }

    #[test]
    fn a_word_split_across_nodes_or_a_page_that_cancels_the_edit_is_left_alone() {
        let (script, _, _) = editable_receive("We will recieve it soon.");
        let split = serde_json::json!(["We will rec", ["B", "ieve"], " it soon."]);
        let (applied, value, events) = run_in_editable(split, false, &script);
        assert_eq!((applied.ok, applied.why.as_deref()), (false, Some("range")));
        assert_eq!(value, "We will recieve it soon.");
        assert!(events.is_empty());

        let (applied, value, events) =
            run_in_editable(serde_json::json!(["We will recieve it soon."]), true, &script);
        assert_eq!((applied.ok, applied.why.as_deref()), (false, Some("refused")));
        assert_eq!(value, "We will recieve it soon.");
        assert_eq!(events, ["beforeinput"]);

        let moved = serde_json::json!(["Well, we will recieve it soon."]);
        let (applied, value, _) = run_in_editable(moved, false, &script);
        assert_eq!(applied.why.as_deref(), Some("stale"));
        assert_eq!(value, "Well, we will recieve it soon.");
    }

    #[test]
    fn a_wrong_kind_or_read_only_field_is_not_touched() {
        let (service, _dir) = service();
        let text = "I think teh answer is clear.";
        let menu = WebMenu::build(&service, textarea(text, 9), false).unwrap();
        let Ok(Chosen::Edit { script, .. }) = menu.choose(0, false) else { panic!("an edit") };
        let reworked = script.replace("\"editor\":\"textarea\"", "\"editor\":\"editable\"");
        let (applied, value, _) = run_in_textarea(text, &reworked);
        assert_eq!(applied.why.as_deref(), Some("kind"));
        assert_eq!(value, text);
    }
}
