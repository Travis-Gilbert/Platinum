# frame-text-check

Platinum's local text checking: spelling and grammar underlines and on-demand
corrections for native notes, and the same corrections for editable web text.
It is a thin host over Theorem's `theorem-text-check` (the dictionary and
`englishc` engines, revisioned diagnostics, the personal profile); everything
here is what a host needs and the engine does not know about.

| Module | Owns |
| --- | --- |
| `service` | One `Service`: the checker plus the persistent personal profile (custom words, suppressed rules). `install` / `shared` give the app one instance. An unreadable profile is reported, never overwritten. |
| `markdown` | `prose_view`: the same bytes with code, front matter, HTML, links, tags, comments, identifiers and numbers blanked, so only prose is judged and every diagnostic offset is already an offset in the note. |
| `field` | The revision of a note (`revision_of`), `check_note` (anchors each diagnostic back onto the real text and drops any whose bytes differ), and `Field`, the "latest result for this text" cell that refuses a stale result. |
| `menu` | The on-demand correction menu: replacement rows, "Add to Dictionary", "Ignore: <rule>", `Menu::resolve` (validates the edit against the text as it is now) and `Action::run`. |
| `web` | The editable-web-text adapter: page scripts (`probe.js`, `apply.js`, `flat.js`, `revision.js`) run through Servo's `evaluate_javascript`, and `WebMenu`, which builds corrections from a probed field and turns a pick into a guarded apply script plus its guarded inverse. |

## Rules it keeps

- Nothing is rewritten on its own. A correction is a row a person picks.
- A result is only ever shown or applied for the exact text it was computed
  from. The revision is a hash of the text, so a result for text that was
  edited and then restored is still correct, and one for text that merely has
  the same length is refused.
- No correction while an input-method composition is active, and the word
  being typed is not judged.
- Protected fields (password, one-time code, payment), non-prose fields,
  `spellcheck="false"` and non-English `lang` are never read.
- Web corrections are made after the script re-reads the field and compares
  its revision and the text at the range: `setRangeText` and an `input` event
  for `<input>` and `<textarea>`; for contenteditable, a cancelable
  `beforeinput`, then `replaceData` on the one text node holding the range, then
  `input`. A page that cancels `beforeinput`, and a word split across nodes, are
  left alone. `execCommand('insertText')` is not used: Servo turns the space
  before the inserted text into a no-break space. The undo is a second guarded
  edit.

## What it cannot do yet

- Servo exposes no layout rectangles or custom highlights for web text, so a
  web field has no underlines; corrections are offered from the context menu
  for the word nearest the caret.
- Servo has no undo for text controls, so the web undo is this adapter's own
  one-step inverse, cleared by any other input.

## Proposed CLAUDE.md row

| `frame-text-check` | Platinum's host for `theorem-text-check`: the shared `Service` and persistent personal dictionary, `prose_view` (Markdown to prose-only bytes at the same offsets), `Field` (stale results refused), the correction `Menu`, and the editable-web-text adapter (page probe and apply scripts, guarded undo). |
