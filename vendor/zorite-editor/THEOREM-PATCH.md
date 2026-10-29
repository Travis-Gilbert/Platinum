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
- `crates/notes/src/offsets.rs`: the four documented range adapters live in
  its existing impl and reuse the host's Rope-based scalar/UTF-16 conversions.
  No editor scanner or conversion cache lives in the notes core.
- `src/lib.rs`: use `str_indices::utf16::from_byte_idx` / `to_byte_idx` for
  all byte/UTF-16 conversion. Remove the former generation anchor and resume
  scanner. Clamping and rounding down partial scalars/surrogates are supplied
  by the library. Keep the buffer generation for authority and layout staleness.
- `Cargo.toml`/root lock: pin normal dependency `str_indices = "=0.4.4"`,
  already locked for ropey, MIT OR Apache-2.0, with no dependencies of its own.
  `frame-notes` is a **dev-dependency only** for offset fixtures and real Vault
  integration tests, alongside `tempfile`; the leaf UI crate has no normal
  dependency on Platinum's notes core, RedCore, VFS or Loro. The lockfile adds
  the editor's edge to the existing str_indices package; no new crate/version
  enters the graph. Both editor and host use the library already used by ropey.
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

### Initial seam verification receipts (before the layering revision)

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

### Initial typing cost (before the layering revision; storage unchanged)

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

## Layering revision after 297352a (2026-10-29)

The initial seam made the vendored UI crate depend normally on the notes core
just for the editor's former UTF-16 resume scanner. This revision removes that
layering dependency and the scanner. `frame-notes` remains a **dev-dependency**
for the real Vault consumer and host offset fixtures. Production conversions
use exactly pinned `str_indices` 0.4.4's `utf16::from_byte_idx` / `to_byte_idx`,
which ropey 1.6.1 already uses. The MIT OR Apache-2.0 crate has no dependencies
of its own; its library already owns chunked UTF-8/UTF-16 indexing, partial
character rounding and clamping. No new package/version enters Cargo.lock.

`utf16_anchor` and all generation-based conversion caching are removed. The
three scanner helpers are removed from `crates/notes/src/offsets.rs`; its four
range helpers are documented in the existing `impl Offsets`. Generation still
protects authority responses and editor layout caches. No persistent offset
index, queue, alternate buffer or new scanner is introduced. Conversions now
scan the queried String prefix without allocation; their cost is measured
below. Cache-specific fixture tests were adapted to test the surviving Rope
API's boundary rounding and independent documents; all other fixtures remain.
An additional editor test compares the actual input conversions to host Offsets
at every byte/unit, including partial scalars/surrogates and `usize::MAX`, for
ASCII, emoji, ZWJ sequences, combining marks, CJK and right-to-left text.

### Revision verification

Worktree: `feat/notes-editor-seam` at `297352a` plus this revision. Environment
reconfirmed: Rust 1.97.1, macOS 27.0.1 (26A434), Apple M1 Max, 32 GiB RAM.
`df -h "/Volumes/SSD Samsung"` ran before both builds (17 and 16 GiB free).
Both builds used the shared lock/media script and its selected target directory:

```sh
"/Volumes/SSD Samsung/cargo-targets/frame-canvas.sh" sh -c 'find vendor/zorite-editor -type f -exec touch {} + && cargo test --locked -p zorite-editor -- --nocapture'
"/Volumes/SSD Samsung/cargo-targets/frame-canvas.sh" cargo test --locked -p frame-notes offsets
```

Offsets: **10 passed**, zero failed/ignored; 88 library tests and all integration
targets were filtered out. Compile: 26.70s; offset tests: 0.00s.

Editor: **76 passed**, zero failed/ignored, including all original seam and
real-Vault tests plus the new conversion boundary test. Compile: 1m 07s;
tests: 15.78s. Doc tests selected zero tests. Existing dependency dead-code
warnings remain; the editor emitted no warnings.

```sh
cargo tree --locked -p zorite-editor --edges normal --depth 1
cargo tree --locked -p zorite-editor --edges normal --prefix none
rustfmt --edition 2024 --check vendor/zorite-editor/src/lib.rs vendor/zorite-editor/src/authority_tests.rs crates/notes/src/offsets.rs
git diff --check
```

Both normal-dependency tree views confirm that frame-notes, Theorem, RedCore,
VFS and Loro are absent. The direct dependencies are gpui-pre, gpui-bidi,
zorite-markdown, unicode-segmentation and str_indices. The full tree was also
checked programmatically for those excluded package prefixes. Touched-file
formatting and diff checks passed. The initial workspace-wide formatting
limitation above is unchanged; shared dependency sources were not edited.

### Revised typing and IME conversion measurements

`authority_tests::typing_cost_on_900_sections` retains the same **600,190-byte,
900-section** multilingual fixture, 100 warm-up and 1,000 measured iterations
per workload. Typing still appends `a` through the real input mutation method,
with hosted history and queued GPUI edit delivery, then asserts text length.
Conversions restore the original fixture before timing. Each direction measures
one actual editor conversion at the document end, traversing the full prefix;
`black_box` guards offsets and results. The IME workload queries the caret
selection, marked range and requested text through `EntityInputHandler`, with
the final section's emoji as the composition span: eight conversions per batch
near the tail, plus returning the emoji String. Assertions check returned units,
marked range and text. The text is unloaded after timing, before automatic
root painting, exactly as in the corrected initial benchmark.

| Workload | Mean (us) | Median (us) | p95 (us) | Maximum (us) |
| --- | ---: | ---: | ---: | ---: |
| Input mutation / keystroke | 27.56 | 27.12 | 54.08 | 176.50 |
| Byte to UTF-16, full prefix | 1447.48 | 1425.46 | 1577.17 | 2775.71 |
| UTF-16 to byte, full prefix | 1156.28 | 1127.83 | 1287.54 | 2843.79 |
| IME query batch, eight conversions | 11589.62 | 10988.29 | 13915.83 | 116861.08 |

These are warm in-process measurements in the same Cargo test profile:
unoptimized editor with debuginfo, unchanged opt-level 2/debug 0 dependency
overrides, on a shared machine. Timers exclude layout/paint, session persistence
and filesystem work. The 11.59 ms mean / 13.92 ms p95 IME batch is a material
measured cost, not evidence of native/release input latency. The maximum outlier
is retained in the table. No conversion cache is retained and no speedup or
latency acceptance is claimed; a native input budget and native/release profile
would be needed to judge whether an IME-specific cache is warranted. The String
storage and seam/history contracts are unchanged. Mounted UI/demo rendering
remains unverified.
