# Rust review: Platinum PR #17

Reviewed the application changes on `claude/platinum-theorem-interface-990ec3`
starting at `ef0e603`, including the merged Index sidebar work. This is a Rust
2024 application using the repository's pinned Rust 1.97.1 toolchain, not a
published library. BeautifulRust's `rust-review` and `rust-best-practices`
guided the review, subject to `RUST-006` in `CODING_STANDARDS.md`. UBMode was not
available in this session. The most important finding was untrusted archive
expansion before resource limits were enforced. The findings below are fixed;
verification results are recorded separately below.

The review followed the changed paths and their consumers: speech shortcut and
capture lifecycle, note reconciliation and promotion, board/session identity,
visit persistence and previews, decision routing and receipts, and Index file
import and reading. It does not certify the entire browser or its dependencies.

## Must fix — addressed

### 1. P1: Validate ZIP contents before admitting an import

**Location:** `src/crm/archive.rs:21`, `src/crm/shelf.rs:358`.
**Rules:** Platinum `DOD-007`, `DATA-001`, `RUST-003`; BeautifulRust review's
correctness and expected-error handling guidance.

**Problem:** The custom ZIP reader expanded entries before the aggregate budget
was checked, did not verify CRCs, and allowed archive paths to bypass the
directory recursion bound. Corrupt or highly compressed input could become
partial sidebar contents or consume excessive memory/stack.

**Fix:** Delegate ZIP headers, compression and CRC validation to `zip`; retain
only application-specific admission limits. Check the compressed file snapshot
(64 MiB), entry count (2,000), remaining depth, declared size, actual bytes read
(8 MiB per file and 32 MiB total), enclosed paths and symlink policy. A failed
import remains a visible error item referring to the unchanged source archive.

```rust
let limit = ENTRY_BYTES.min(TOTAL_BYTES - total);
ensure!(file.size() <= limit, "ZIP exceeds its 8 MiB entry or 32 MiB total limit");
file.by_ref().take(limit + 1).read_to_end(&mut data)
    .context("corrupt ZIP entry")?;
ensure!(data.len() as u64 <= limit, "ZIP exceeds its decompressed size limit");
```

### 2. P1: Do not publish failed extractions or overwrite previous files

**Location:** `src/crm/shelf.rs:489`.
**Rules:** Platinum `DATA-002`, `RUST-003`; BeautifulRust expected-error handling.

**Problem:** Extraction discarded directory/write failures and returned a path
as though the file existed. Names derived from sidebar IDs could overwrite a
previous extraction after IDs were reused or imports were prepared concurrently.

**Fix:** Propagate extraction errors to the archive error item and use the
existing `tempfile` dependency for unique retained files. Keep the extension for
the file reader. Prepared imports receive fresh sidebar IDs only when admitted
to the current owner, preserving edits made while the worker was running.

```rust
let mut file = tempfile::Builder::new()
    .prefix("import-").suffix(&suffix).tempfile_in(dir)?;
file.write_all(data)?;
file.as_file().sync_all()?;
let (_, path) = file.keep()?;
```

### 3. P2: A stale drop target must not remove the dragged item

**Location:** `src/crm/shelf.rs:218`, `src/crm/shelf.rs:244`.
**Rules:** Platinum `DOD-005`, `DATA-002`; BeautifulRust correctness guidance.

**Problem:** `drop_node` removed the source before discovering that the target
was absent. A stale drag could therefore lose the source from the shelf.

**Fix:** Reject a missing destination before mutation. Background imports also
revalidate their destination at admission and visibly reject a deleted target.

```rust
if dragged == target || dragged == self.root.id
    || self.node(target).is_none() || contains(&self.root, dragged, target)
{
    return false;
}
```

### 4. P2: File import and reading must leave the UI thread

**Location:** `src/crm/workspace.rs:1079`, `src/crm/workspace.rs:1137`.
**Rules:** Platinum `CON-001`, `DOD-007`, `UI-002`.

**Problem:** Drop handling performed directory traversal and ZIP extraction in
the UI callback. Opening PDFs synchronously waited for Quick Look, and Markdown
was reread from disk on every render.

**Fix:** Prepare imports and read/rasterize files on the background executor.
Allow one import per Index view, with visible feedback for an additional drop.
Cache the loaded page and refresh it on reopen. Cancel old read tasks and verify
the active node and its continued existence before publishing a result. Quick
Look receives a unique temporary directory to avoid same-name preview collisions.

```rust
let imported = cx.background_executor()
    .spawn(async move { super::shelf::Import::read(&paths, directory) }).await;
// Back on the owning view:
this.shelf.finish_import(target, imported)?;
```

### 5. P2: Note promotion must restore the board composer

**Location:** `src/browser/place.rs:56`.
**Rules:** Platinum `UI-003`, `DOD-005`.

**Problem:** Promoting a note from the expanded Notes place cleared `place`
directly. When the board was already open behind that place, the shared cleanup
was skipped and the full workbench remained mounted over the board.

**Fix:** Use the same place-exit operation as other navigation paths. The note's
existing document/editor entity remains the board card's owner.

```rust
browser.leave_place(window, cx);
```

### 6. P1: Website fields swallow modifier-release handling

**Location:** `src/browser.rs`, the `intercept_keystrokes` subscription.
**Rules:** Platinum `UI-003`, `CON-002`.

**Problem:** GPUI synthesizes a modifier-only keystroke on release. A focused
`Page` consumes it before the bubbling modifier callback, leaving dictation
recording or its delayed start armed after the user releases Command.

**Fix:** The existing window-scoped keystroke interceptor forwards modifier-only
release state to the speech lifecycle before the page can consume it. It still
allows page dispatch and does not introduce a global key hook. The native probe
now targets a focused production Page, checking both a quick tap and hold/release.

```rust
this.voice_modifiers(event.keystroke.modifiers, false, false, window, cx);
```

### 7. P1: Shared session preferences broke native example compilation

**Location:** `examples/canvas_check.rs`, `examples/site_panel_check.rs`.
**Rules:** Platinum `VER-004`, `DEP-002`.

**Problem:** Both examples include `spoken.rs` under their own crate roots but
had no `voice::shortcut` module for its new preference parser references.

**Fix:** Include and re-export the production shortcut module in each example,
without pulling the microphone/recognition owner into these probes.

```rust
#[path = "../src/voice/shortcut.rs"]
pub(crate) mod voice_shortcut;
mod voice {
    pub(crate) use crate::voice_shortcut as shortcut;
}
```

### 8. P1: Summary and proposal actions were unreachable

**Location:** `src/crm/workspace.rs`, `render_ask` and `Crm::render`.
**Rules:** Platinum `UI-001`, `VER-004`.

**Problem:** The ask input had an Enter subscription, but neither it nor proposal
acceptance was rendered. Tests called methods directly, hiding the missing path.

**Fix:** Mount the existing input, a summary action, a readable proposal with
its evidence count and proposed changes, and Accept/Dismiss controls when a
company is in scope. The integration test now clicks the evidence suggestion,
types into the ask field, requests a proposal and clicks acceptance.

```rust
.when(self.ask_company().is_some(), |card| card.child(self.render_ask(cx)))
```

### 9. P1: Attachment evidence and displayed rows came from different reads

**Location:** `src/crm/files.rs`, `src/crm/workspace.rs::attach_file`.
**Rules:** Platinum `DATA-002`, `DOD-005`, `CON-001`.

**Problem:** Reading the source and copying it later allowed another application
to change the file between operations. Reopened rows then described different
text from the saved passages used for highlighting.

**Fix:** Copy into a unique owned attachment directory on the worker first,
including supported database sidecars, then derive passages and rows from that
same copy. Admit its relative path and reading together. Copy/read failures are
visible and cannot publish a successful attachment. A regression changes the
original after preparation and verifies the owned reading remains unchanged.

```rust
fs::copy(source, &path)?;
let reading = material::open(&path).map_err(anyhow::Error::msg)?;
```

### 10. P2: Refreshing a table discarded the user's layout

**Location:** `src/crm/workspace.rs::Rows::same_columns` and `Crm::sync_rows`.
**Rules:** Platinum `DOD-005`, `UI-002`.

**Problem:** Comparing columns positionally treated a reordered projection as a
new schema. Returning to the list reset order, widths and sorting.

**Fix:** Compare key membership, remap incoming cells to the retained key order,
and reuse the existing row/sort update. A view-level regression reorders,
resizes and sorts, opens a company and returns to the list.

```rust
self.columns.iter().all(|held| columns.iter().any(|next| held.key == next.key))
```

Findings 1 and 4 overlap the latest Greptile archive and PDF findings; findings
6–10 address the other five issues in that review. Each was checked against the
current source rather than assumed valid from the external comment.

## Dependency decision

`zip` is pinned to **9.0.4**, with default features disabled and only
`deflate-flate2-zlib-rs` enabled. Its MIT license and Rust 1.88 minimum are
compatible with this repository. Its parser and CRC checks replace the custom
ZIP implementation; unsupported codecs/encryption fail visibly. The lockfile
adds `zip` and its `nix` dependency, without upgrading the existing graph.
`tempfile` was already used by tests and is now also a runtime dependency.

The remaining adapter implements product limits, sidebar placement and async
ownership, which the archive library cannot supply. ZIP limits apply per archive;
this change does not claim a global disk quota or cancellation of a Quick Look
process that is already executing.

## What's already good

- The note store uses a shared document/editor identity for sidebar and board
  cards, and queued edits retain their base revision during reconciliation.
- Visit metadata has independent admission from optional bounded previews;
  persistent tab allocation prevents closed tabs from lending their trails to
  new tabs.
- Speech capture has explicit hold, release and cancellation paths, and refuses
  stale insertion destinations.
- Typed-decision receipts distinguish unknown paid outcomes and record attempts
  before dispatch. This review did not make a live paid provider call.

## Verification

The locked application suite passed **303 tests** with **2 ignored**. The native
app, `canvas_check` and `site_panel_check` built with `snapshot`. Scoped pedantic
Clippy completed with **599 warnings**, including existing code and style
suggestions; this is not a warning-free result. The formatting check reported
existing broad differences; unrelated formatting was not rewritten.

The native flow and fresh-process restore passed. A focused production Page
received modifier-release dispatch and committed the supplied transcript exactly
once; a quick tap did not start capture. The Index proposal and board-note
screenshots were inspected. See the [commands, reports, screenshots and limits](evidence/beautifulrust-review-2026-10-31/checks.md).

Per `RUST-006`, validation targets the application and its changed consumer paths
instead of enabling every optional feature or imposing a new BeautifulRust lint
policy. Pedantic lints cannot establish the archive budget, stale destination or
view-lifecycle properties found here. Physical keys, live microphone recognition
and paid provider behavior were not verified by this run.
