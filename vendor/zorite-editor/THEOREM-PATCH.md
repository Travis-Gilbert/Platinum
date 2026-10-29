# zorite-editor 0.11.2, carried by Frame

Forked from `packetThrower/zorite@6ee46f28cbe0017f7f152033227d897434ddd0ef`.
This crate's manifest and LICENSE are MIT; its upstream copyright notices
and license are retained. The GPL application is not included.

Why: Frame's Markdown editor uses the zorite crates on Frame's existing
GPUI Kit 0.8.0 / gpui-pre 0.3.9 graph.

The changes:

- `Cargo.toml`: disable publishing and replace the inherited `gpui`
  dependency with `gpui-pre = "=0.3.9"`, retaining the `gpui` import name
  and using Frame's existing workspace patches.
- `Cargo.toml`: keep `test-support` on the GPUI dev-dependency. Disable
  automatic examples and remove the explicit demo target and its
  `gpui_platform` / `os-spellcheck` dev-dependencies. The unmodified demo
  source is retained as reference; its OS spellchecker is outside this
  three-crate fork. No platform features are added to Frame's graph.
- `src/markdown_syntax.rs`: add a headless GPUI test using the existing
  Markdown test style. Open a note with a heading, list, wikilink, emoji,
  CJK and a combining mark, type via GPUI's input path, and read back the
  exact text.

No production Rust API changes were required by the initial gpui-pre 0.3.9 port.
No gpui-component/GPUI Kit API is consumed by this crate. Frame's existing
GPUI patches are unchanged; zorite's WGPU combining-mark patch is not used
on Frame's Apple text/Metal path.

Re-applying on an upstream upgrade: copy this MIT crate, carry the manifest
changes and any test additions above, and run Frame's focused offline
build, tests and clippy for all three forks. Compare the workspace GPUI
versions and features and the lockfile against the pre-upgrade graph.

## Notes authority seam (2026-10-29)

Based on Platinum `85bdc550e2d34178825d913dcf5dc9424b935c0d`, carrying the
same upstream MIT revision above. Reapply the port first, then the following
source changes; keep this file, API.md and the replay tests with the fork.

- `src/authority.rs`: `TextEdit`, `BufferEdit`, local/load origins, hosted
  undo/redo requests, and atomic generation-checked `apply_remote`. Uses GPUI
  subscriptions/events. Remote replacements shift byte selection and IME
  endpoints (right affinity), shift/drop diagnostics, and neither echo nor
  touch snapshots. Invalid/stale batches preserve all state.
- `src/lib.rs`: the 10 original `record_edit` sites finish with `emit_edit`
  after selection; helper paths thread the existing GPUI Context. Structural
  drag and code-language commands supply their final caret before emission.
  IME composition emits without changing standalone snapshot behavior;
  snapshot restore and `set_text` emit full-range replacements. Initial
  `with_text` constructs a value without an event. Hosted loads skip
  `normalize_loaded`; local snapshots are disabled while hosted.
- `src/tables.rs`: the seven original sites (alignment, insert/delete row,
  delete table, duplicate row, marker rewrite, column rewrite) finish with
  the same emission. Column rewrite returns the edit to the caller so its
  final cell selection is reported. Table resize uses marker rewrite.
- `crates/notes/src/offsets.rs`: range adapters reuse its Rope-based scalar
  conversions; the editor's UTF-16 cursor cache moves here, preserving nearby
  query scans, with clamping/round-down at partial scalars/surrogates.
- `Cargo.toml`/root lock: add the existing `frame-notes` package dependency
  rather than duplicating offset conversion in the editor, and existing
  `tempfile` as a dev-dependency for a real Vault consumer test. No new package
  or package version is introduced. This deliberately makes the carried fork
  depend on Platinum's core; an upstream standalone extraction should give the
  shared offsets their own small crate rather than copy them again.
- `src/authority_tests.rs`: replay actual mutation methods through GPUI event
  delivery; Unicode local/remote fixtures, atomic failure/history invariants,
  real Vault edit/undo/redo/save consumer, and large-note typing measurement.
  `markdown_syntax.rs` only exposes the existing test palette to sibling tests.

Data/workload (DOD-001..008): the existing String remains a transient projection
of one open authority document. Its byte offsets are positions, not document
identity. Generation plus the host's entity-scoped subscription protects stale
responses. Selection/IME/diagnostics remain editor-owned presentation state.
Each local mutation allocates its inserted payload for the existing GPUI event
system; no new retained queue, worker, index or parallel document is introduced.
Snapshots keep their upstream bound when detached and are absent when hosted.
Remote batches use one temporary String for all-or-nothing UTF-8 validation;
these are infrequent authority updates, not the typing hot path. The existing
whole-String rebuild per keystroke is deliberately unchanged.

Verification and the measured typing cost are recorded below after execution.

### Verification receipts

All commands ran from the dedicated `feat/notes-editor-seam` worktree on the
base revision above plus this patch, Rust 1.97.1, macOS 27.0.1 (26A434), Apple
M1 Max, 32 GiB RAM. `df -h "/Volumes/SSD Samsung"` ran before every build
(19 GiB initially, 18 GiB on subsequent runs). The shared script selected its
own target directory and media environment; no `cargo check`, custom target,
changes under `deps/`, or other crate-source edits were used.

Final passing editor command (35.14 s compile, 1.13 s test execution):

```sh
"/Volumes/SSD Samsung/cargo-targets/frame-canvas.sh" sh -c 'find vendor/zorite-editor -type f -exec touch {} + && cargo test --locked -p zorite-editor -- --nocapture'
```

75 tests passed, zero failed/ignored; 7 new seam tests cover the 17 original
sites, three bypasses, clipboard/deletion/formatting/drag commands, table resize,
Unicode emission/remapping, remote validation/history invariants, exact hosted
loads and a real Vault consumer. The consumer submits edits using Vault's actual
revision/operation identity, checks unchanged-save bytes, and dispatches the real
undo/redo key bindings through GPUI to `Vault::undo`/`redo` and `apply_remote`.
Session history owns grouping: the four simulated native character operations
undo and redo in four steps. Doc tests selected zero tests (not extra coverage).

```sh
"/Volumes/SSD Samsung/cargo-targets/frame-canvas.sh" cargo test --locked -p frame-notes offsets
```

10 offset tests passed (3 new); 88 library tests and all integration targets
were filtered out. Compile: 20.39 s; offset tests: 0.00 s.

```sh
cargo fmt --all -- --check
rustfmt --edition 2024 --check vendor/zorite-editor/src/lib.rs vendor/zorite-editor/src/authority.rs vendor/zorite-editor/src/authority_tests.rs vendor/zorite-editor/src/tables.rs vendor/zorite-editor/src/markdown_syntax.rs crates/notes/src/offsets.rs
git diff --check
```

The workspace-wide formatter exited 1 on existing formatting differences in
shared dependency checkouts (including browser-substrate and rustyred-thg-mcp).
It made no changes. The exact touched-file rustfmt command and diff check passed.
Existing dependency dead-code warnings remain; the editor emitted no warnings
in the final passing build.

Earlier attempts: the prescribed editor command without `--nocapture` failed
compilation because the new cursor type was hidden behind notes' private offsets
module; this was fixed by extending the already re-exported `Offsets` type, with
no change to notes' lib.rs. The first `--nocapture` run caught the consumer test's
incorrect assumption that session undo coalesced four character inputs; the
assertions now verify the actual four undo/redo steps. That same run was stopped
with SIGTERM after sampling confirmed the measurement had finished its timed
input loop and was stuck painting the full auto-height root. The measurement
harness now unloads its text after timing and before GPUI's automatic draw. The
passing results above are from a subsequent complete run, not that stopped run.

### Typing cost (storage unchanged)

`authority_tests::typing_cost_on_900_sections`, in the final command above:
900 generated sections, **600,190 UTF-8 bytes** initially. Each section contains
a heading, multilingual paragraph (emoji/combining/CJK/RTL), 20 repetitions of
body prose and two list items. Real `replace_text_in_range(None, "a", ...)`
appends one ASCII character at the end per sample, with hosted history attached,
no snapshots, and GPUI edit event delivery queued during the input update.
100 warm-up keystrokes followed by 1,000 timed keystrokes; each timer encloses
the input mutation, diagnostics remapping and event emission. Profile: Cargo
`test`, unoptimized editor with debuginfo; manifest dependency overrides remain
opt-level 2, debug 0. This is a warm in-process microbenchmark on the hardware
above, not a release, native paint, session persistence or end-to-end latency
measurement.

| Per keystroke | Microseconds |
| --- | ---: |
| Mean | 26.17 |
| Median | 16.12 |
| p95 | 79.42 |
| Maximum | 290.38 |

No storage optimization or speedup is claimed. The full String rebuild is
unchanged. The full-note paint in the earlier attempt did not complete and is
not accepted rendering/performance evidence. A mounted Platinum notes UI and
interactive demo were not run; the consumer evidence here is headless GPUI plus
the real filesystem-backed Vault. The separate notes UI lane must attach this
seam to its production host subscription.
