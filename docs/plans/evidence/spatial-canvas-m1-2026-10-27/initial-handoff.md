# Spatial canvas M1 — incomplete, uncommitted draft

Worktree: `/Volumes/SSD Samsung/frame-worktrees/spatial-canvas`.
Branch/base: `feat/spatial-canvas`, `b4695cf`.

## Blocker

Both required native commands stop **before compilation**:

```
error: failed to open: /Volumes/SSD Samsung/cargo-targets/frame/debug/.cargo-build-lock
Caused by:
  Operation not permitted (os error 1)
```

This session can write only the task worktree and temporary directories. The
required existing target is outside those writable roots; escalation is not
available. It is not Cargo's “Blocking waiting for file lock” message. The SSD
had **88 GiB available**. No new target was created, and no cargo check was run.
No native build, screenshot, session/input acceptance or performance result
was obtained. The source has not been type-checked. M1 is **not done**.

## Draft in the worktree

- `Cargo.toml`, `Cargo.lock`: root dependency on the existing whiteboard
  workspace member. `cargo update -p gpui-whiteboard --offline` succeeded;
  the entire lock diff is the one dependency edge in Frame's package.
- `src/canvas/mod.rs`: one existing Page, whiteboard Engine, host camera and
  gesture commits, board painter, page layer, selection/marquee overlay,
  settle debounce, developer probe camera operations.
- `src/canvas/input.rs`: host arbitration around the whiteboard's press,
  drag, release and wheel, with H/V tool selection and buffered Space state.
- `src/canvas/surface.rs`: inverse input/control transform and uniform raster
  scale cap; two pure tests are written but **not executed**.
- `src/page.rs`: current camera zoom separate from raster scale; page input,
  wheel and IME mapping; settle-only resize; buffered Space and page-first
  Escape. The existing Page `surface()` renderer is reused unchanged.
- `src/controls.rs`: uses Page's shared inverse transform for select and
  context-menu anchors. Native colour/file panels and dialogs retain their
  existing window-owned behavior; see the unfinished work below.
- `src/actions.rs`, `src/main.rs`, `src/browser.rs`: Ctrl–Cmd–B developer entry
  and return, retaining the same Page. No M2 mode-switch chrome.
- `examples/canvas_check.rs`: production Page/SpatialCanvas mounted in a
  native window with a temporary profile; before/after-settle samples,
  per-step PNGs, a session-retention driver and click/hover/wheel/pointer-drag
  subset. It intentionally cannot certify full sign-in acceptance while
  automated coverage is incomplete.
- `tests/canvas/zoom-probe.html`, `tests/canvas/probe.js`,
  `tests/canvas/signin/index.html`: resize/viewport log and local session,
  textarea, pointer/scroll/control fixtures.
- `tests/canvas/MANUAL.md`: developer entry and manual matrix, explicitly
  pending VoiceOver bridge and R3 HTML drag/drop.
- `tests/canvas/run-checks.py`: required builds and probe runs, with command,
  exit code, elapsed wall time and output receipts. Takes `--evidence`.
- `docs/plans/notes-app.md`: only the live-card paragraph changed: settle,
  scale/cap, R1 caveat, caption drag and scroll restoration. The external
  spatial plan was read in place, not copied or edited.

## Minimal whiteboard patches

Every patch is marked `// THEOREM PATCH:`:

- `vendor/gpui-whiteboard/src/paint.rs`: make the existing `paint_selection`
  and `paint_marquee` functions public.
- `vendor/gpui-whiteboard/src/lib.rs`: re-export those two functions.

The existing `BoardThumbnailView` supplies board content. Its `Engine` is
intrinsically hosted: it returns `EngineEvent::Camera` and a completed
`WhiteboardEvent::Commit`; `HistoryMode`/`CameraMode` setters are APIs on
`WhiteboardView`, not on Engine. No duplicate camera, gesture, hit-test or
selection-paint implementation was added. Frame's custom code is the missing
Servo/GPUI transform and page-versus-board arbitration. Caption hit testing
is a subregion of the engine-hit card. The scale cap reduces raster scale
uniformly rather than distorting the CSS aspect ratio.

## Verification and commands

[commands.json](commands.json) records each timed validation command, exact
arguments, exit status and wall time. The corresponding `.log` files retain
output. Read-only investigation also used `pwd`, `git status --short`,
`git branch -vv`, `rg`, `cat`, and `sed` for the supplied plans, skills,
app code, vendored APIs and read-only Servo sources; the recording grid was
visually inspected. Source edits used Python, here-documents and rustfmt.
Those investigation/editing batches were not individually instrumented;
there are no invented command runtimes for them.

| Check | Result | Measured seconds |
| --- | --- | ---: |
| `df -g "/Volumes/SSD Samsung"` | 88 GiB available | 0.003233 |
| `touch crates/profile/src/lib.rs` | succeeded before build | 0.003067 |
| required `cargo test --locked --bin frame` | denied before compilation, exit 101 | 0.583794 |
| required `cargo build --locked --example canvas_check` | denied before compilation, exit 101 | 0.543868 |
| new Rust formatting check | passed | 0.049556 |
| edited existing Rust modules, rustfmt parse only | passed; not type checking | individual times in JSON |
| `node --check tests/canvas/probe.js` | passed | 0.027637 |
| `node --check` for inline sign-in script | passed | 0.028271 |
| Python runner syntax | passed | 0.022518 |
| `git diff --check` | passed | 0.029825 |
| lockfile diff | only the requested edge | 0.010151 |
| protected paths diff | empty | 0.007105 |

The build commands used exactly
`TMPDIR="/Volumes/SSD Samsung/tmp" CARGO_TARGET_DIR="/Volumes/SSD Samsung/cargo-targets/frame" script/media-env ...`.
An earlier uninstrumented attempt of the same test command hit the same denial.
The offline dependency update was repeated for a measured receipt in
`commands.json`; its first run also succeeded with the one-edge diff.

## Acceptance and remaining work

| Item | State | Evidence |
| --- | --- | --- |
| Dependency integration | static pass | `lock-diff.log` |
| Notes/spatial description | paragraph aligned | notes diff; no claim of implemented lifecycle |
| A1 session, caret, scroll, identity | untested | native build blocked |
| A1 zero-resize / stable inner size | untested; expected R1 failure is not a measured failure | `zoom-gate.json` |
| A2 input and controls | incomplete and untested | subset driver; manual matrix |
| A3 gestures | draft, untested | engine routing; no native receipt |
| Rounded clipping and rendered layer order | untested | no native screenshot |
| Lifecycle/performance measurements | unimplemented and unmeasured | `measurements.json` |
| VoiceOver | baseline not wired | manual matrix |
| HTML drag/drop | waiting for R3 | manual matrix |

Next work, still in M1 scope:

1. Run the two required native commands from a session allowed to write the
   existing shared target; fix compilation and run the new tests.
2. Verify the draft in a real window. Complete modal focus/reveal and actual
   camera-motion dismissal of native/browser popovers. Current responder
   cleanup alone does not prove that a visible menu has closed. Verify Space
   against all editable types; it currently relies on Servo's IME control.
3. Complete automated A2 text selection, context menu, select and dialogs,
   and A3 gesture checks. The example's current subset does not certify A2/A3.
   Record navigation starts as well as the current document/webview identity.
4. Finish the live/hidden/disposed multi-card measurement driver, then record
   frame-rate capacity, memory deltas, activation/restoration latency and
   retained hidden-frame validity. Do not infer a budget from colour-buffer
   arithmetic or from this single-card draft.
5. Serve the fixtures, run both native probes and all manual checks. Retain
   real resize counts and CSS drift for R1. No zero values are asserted here.

All changes remain uncommitted. No push, branch creation, Servo modification,
`deps/` edit, `vendor/gpui-pre` edit or bootstrap change was made.
