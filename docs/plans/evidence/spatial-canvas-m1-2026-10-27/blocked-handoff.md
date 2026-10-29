# Spatial canvas M1 continuation

**M1 is incomplete.** The draft now compiles and its 61 binary tests pass.
Native acceptance and measurements are blocked by the locked desktop, not by
filesystem or port permissions. Nothing is committed.

Worktree: `/Volumes/SSD Samsung/frame-worktrees/spatial-canvas`.
Branch/base: `feat/spatial-canvas`, `b4695cfb0d076c6ef042cb0fdfa14162dd71486f`.
The prior draft and its sandbox receipts remain in
[initial-handoff.md](initial-handoff.md). The external spatial plan was read,
not edited or copied into this branch.

## Acceptance state

| Item | Current result |
| --- | --- |
| Required binary tests | **61 passed**, 0 failed; 1.14 s test execution, 14.915675 s command wall time |
| Required example build | **Passed**; latest command 9.429677 s |
| A1 text, caret, scroll, cookie, document/webview identity, continued typing | Unverified: native window is hidden |
| A1 settle resize gate | **Blocked**, no valid resize counts or viewport drift; zero-resize remains the expected R1 exception |
| A2 input, visible menu dismissal, picker dismissal, modal focus/reveal | Driver expanded and compiled; not accepted in a visible window |
| A3 gestures and editable Space matrix | Driver expanded and compiled; not accepted in a visible window |
| Live/hidden/disposed measurements | Driver implemented and compiled; **not measured** |
| Notes plan agreement | Live-card paragraph matches settle resize, pixel cap, R1 caveat, caption drag and scroll restoration |
| Human-only checks | Listed in `tests/canvas/MANUAL.md`; none performed |

The latest native attempts report GPUI `visibility: Hidden`, `visible: false`,
`active: false` in `zoom/window.json`, `signin/window.json` and
`measure/window.json`. A separate read-only macOS query reports
`CGSSessionScreenIsLocked: true`; see [display-state.json](display-state.json).
GPUI's macOS backend stops its display link for an occluded window. Activating
the application cannot unlock the screen. The user was asked to unlock it;
no system settings or screen-lock state were changed.

| Native command | Result | Measured seconds |
| --- | --- | ---: |
| Zoom probe, served on 127.0.0.1 | Rejected at visible-window preflight | 3.780718 |
| Sign-in/input probe, served on 127.0.0.1 | Rejected at visible-window preflight | 1.480136 |
| Multi-card measurement | Rejected at visible-window preflight | 1.480289 |

Each has a `result.json` with `passed: false` and the exact blocker. AppKit's
`terminate:` exits with process code 0 even after a probe error, so process exit
alone is **not** acceptance. The runner now requires the current process's
explicit passing result, and both the probes runner and measurement runner
exit 1 for these attempts. Binary SHA-256 is retained with each command.

## Gate and measurement records

[zoom-gate.json](zoom-gate.json) contains a blocked record with null counts and
null CSS drift. It names Servo revision
`2b522612c4db75a7f3214a0f17ef8abe5a8c47a1` and the sequence 1, 0.35, 2, 1.
No real R1 baseline has been obtained. Expected resize failures are not asserted
as measured failures, and stale zero values are not accepted.

[measurements.json](measurements.json) records the blocker with all requested
metrics null: display rate, live-card capacity, memory per live/hidden/disposed
card, hidden-to-live and disposed-restore costs, and retained-frame validity.
There is no measured budget and no colour-buffer estimate.

The driver runs 1 through 16 production Page/SpatialCanvas instances with an
animated Canvas2D fixture, in a 4 by 4 grid fitted to the actual window. It takes
three two-second samples per population, comparing every page's presented
frames and GPUI frame callbacks against 95% of the display refresh rate.
It samples process RSS, including allocator retention, against an empty-engine
baseline; hashes retained CVPixelBuffer bytes before/after hiding; captures the
retained hidden frames through GPUI; records time to the first new frame on
show; checks released entities after disposal; and times recreated pages to
load completion and a first frame. These are workload-specific measurements,
not a universal browsing budget. The driver has not run beyond preflight.

Earlier artifacts in `zoom-inactive-window/`, `signin-inactive-window/`,
`zoom-screen-locked/` and `signin-screen-locked/` are **invalid attempts**.
Inspection of their PNGs showed only the initial Loading card. The page's
rendered zoom stayed at 1 while the requested camera changed. Those directories
are retained for diagnosis; their zero resize counts and any apparent pass
must never be used as acceptance. One sign-in attempt also exposed and fixed a
reentrant typed Root borrow; another was terminated after diagnosing the lock.

## Files and implementation

The existing draft was continued, not replaced:

- `Cargo.toml`, `Cargo.lock`: original one-edge dependency on the existing
  `gpui-whiteboard` workspace member.
- `src/canvas/mod.rs`, `input.rs`, `surface.rs`: hosted whiteboard camera,
  history, existing Page layer, settle resize and inverse transform. Added
  modal reveal, cached-card input checks, pending Space arbitration and popup
  ownership. Existing whiteboard hit testing, gestures and painters are reused.
- `src/page.rs`: retained camera/raster separation and input mapping; Space
  queries actual editable DOM focus and buffers events until the answer arrives.
  IME-control presence alone does not cover contenteditable or designMode.
- `src/controls.rs`: canvas select/context menus use GPUI Kit PopupMenu, and
  canvas dialogs use its Dialog/Input components. The native menu API exposes
  no dismissal handle. Camera movement clears rendered popups and requests
  owned picker dismissal; dialogs focus/reveal the page. These behaviors still
  require live validation; responder cleanup is not reported as visual proof.
- `src/delegate.rs`: navigation-start and presented-frame counters.
- `src/actions.rs`, `src/main.rs`, `src/browser.rs`: original Ctrl-Cmd-B
  developer entry/return with the same Page, plus the modal reveal event match.
- `examples/canvas_check.rs`: production mount, native input, navigation count
  with document/webview identity, session continuation, settle samples, GPUI
  PNGs, visible-window guard and explicit result records.
- `examples/canvas_check/acceptance.rs`: A2/A3 automation at 0.35, 1 and 2;
  reset selection/scroll state per zoom; select/context menu before/after images;
  real AppKit picker visibility; modal reveal/responses/focus; editable Space
  matrix including contenteditable and designMode; caption/cached-card drag,
  empty-board pan/marquee, synthetic pinch, Command-wheel and page-first Escape.
- `examples/canvas_check/measurements.rs`: the multi-card lifecycle driver.
- `tests/canvas/signin/index.html`, `zoom-probe.html`, `probe.js`, and new
  `benchmark.html`: local session, editable/control, resize and animated fixtures.
- `tests/canvas/run-checks.py`: shared wrapper/target, loopback fixture server,
  disk guard, measured receipts, executable hashes, explicit verdict checking,
  and temporary native scratch/profile cleanup after process exit.
- `tests/canvas/MANUAL.md`: VoiceOver, IME candidate window, physical pinch,
  and HTML drag/drop after R3. No manual pass is claimed.
- `docs/plans/notes-app.md`: the aligned live-card paragraph.
- `vendor/gpui-whiteboard/src/lib.rs`, `paint.rs`: only the original marked
  THEOREM PATCH exports of existing selection/marquee painters.
- This evidence folder: source/runtime records, logs, invalid captures,
  environment, gate and measurement status.

Outside the worktree, `/Volumes/SSD Samsung/cargo-targets/frame-canvas.sh` was
repaired: its unconditional `find build.rs` exited under `set -e` because that
file is absent. It now includes build.rs only if present and touches
`crates/profile` with the other lane sources inside the existing lock. The
shared target, lock, TMPDIR and media environment are unchanged.

## Commands and checks

[commands.json](commands.json) contains exact arguments and measured runtimes;
[command-runtimes.md](command-runtimes.md) lists every recorded validation run,
including failures and shared-lock waits. Read-only investigation and editing
calls were not individually timed. No runtimes are invented for them.

Notable earlier receipts: wrapper failure 0.170483 s; first compile failure
1499.930624 s (five continuation errors, fixed); first passing tests
451.345789 s; first passing example build 1200.624678 s. Missing app-adjacent
GStreamer links initially prevented both probes starting. Existing
`script/setup-media`, run through the shared wrapper, linked 39 installed
plugins in 474.914795 s. It changed no dependencies or installed plugins.
Later harness compile errors were corrected; final tests and build above pass.

Final new-Rust formatting, Python and fixture-JavaScript syntax, diff whitespace
and protected-path checks all pass; individual runtimes are in the receipts.
SSD free space was 74 GiB at the final check, above the 6 GiB stop gate. No
Servo/deps changes, pin changes, other vendor edits, cargo check, cargo clean,
new target directory, commit, push or branch switch were performed. All check
windows and loopback servers are closed. The original worktree is retained
with the unfinished work uncommitted, as requested.

GPUI `render_to_image` captures its scene only. It cannot show the desktop,
macOS IME candidate window, AppKit system panels or physical gesture delivery.
The driver queries actual AppKit panel visibility separately, but those checks
have not yet reached a valid visible run. No Screen Recording was attempted.

## Next concrete step

Unlock the Mac and leave the desktop visible, then rerun:

```sh
python3 -I tests/canvas/run-checks.py --only probes --evidence docs/plans/evidence/spatial-canvas-m1-2026-10-27
python3 -I tests/canvas/run-checks.py --only measure --evidence docs/plans/evidence/spatial-canvas-m1-2026-10-27
```

Keep this branch and current draft. Fix any failures exposed by the visible
run, inspect actual menu open/closed and modal captures, record the real resize
counts/CSS drift, and replace the blocked measurement record with the measured
samples. Rebuild any source changes through the shared wrapper. A1-A3, visual
popover dismissal, modal focus/reveal, editable Space behavior, and all lifecycle
metrics remain live acceptance work, not finished features certified here.
