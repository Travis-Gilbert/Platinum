# Spatial canvas M1 native continuation

**M1 remains incomplete.** The unlocked resume produced valid zoom and sign-in
runs, exposing eleven input-check failures. The measurement window later became
occluded, triggering the user's explicit stop rule. It was closed without
reactivation or retry. No code fix or further native run followed that stop.

Worktree `/Volumes/SSD Samsung/frame-worktrees/spatial-canvas`, branch
`feat/spatial-canvas`, base `b4695cf`. All work remains uncommitted.
The earlier blocked report and records are preserved in
[blocked-handoff.md](blocked-handoff.md),
[blocked-zoom-gate.json](blocked-zoom-gate.json) and
[blocked-measurements.json](blocked-measurements.json). The original draft
handoff remains in [initial-handoff.md](initial-handoff.md).

## Run validity and measured runtimes

| Check | Result | Wall seconds |
| --- | --- | ---: |
| Initial ioreg display-state check | IOConsoleLocked false | 0.039315 |
| Wrapper example build | Passed | 243.190892 |
| Zoom probe | Valid visible run; accepted apart from expected resize exception | 8.007748 |
| Sign-in/input probe | Valid visible run; session passed, 52/63 input checks passed | 79.610780 |
| Measurement driver | Incomplete; stopped when window became occluded | 62.256221 |
| Display-state check after stop | IOConsoleLocked still false | 0.039020 |

Exact commands, binary hashes and process/verdict results are in
[resume/commands.json](resume/commands.json). Windows were reduced to 900 by
650 points; that source change required the wrapper build. Each native command
used a separate temporary profile/scratch directory and loopback server, all
closed/removed after its run. Earlier binary tests passed 61/61 in 1.14 seconds
(14.915675 seconds command wall time); production sources have not changed in
this resume, so those tests were not rerun.

All three windows initially reported Visible and active on the active Space,
shown and not minimized. See each `resume/*/window.json`. The measurement
result reports `measurement window became occluded; frame-rate sample rejected`.
The post-stop macOS query still reports unlocked. Final native Space/occlusion
flags were not captured before the driver closed its window, so covering by
another window versus moving to another Space cannot be distinguished. No
attempt was made to reactivate, uncover, move Spaces, change settings or retry.

## Acceptance results

- **A1 session passed:** text, caret, scroll, cookie, document identity and
  webview identity survived the camera sequence; continued typing inserted at
  the retained caret. Navigation-start counter stayed 0 throughout. Initial
  Page::open navigation and document/webview identities are in
  `resume/signin/navigation.json`, with per-step state in `zoom.json`.
- **A1 zero-resize failed as expected until R1.** The measured gate below is
  valid and supersedes the earlier blocked record.
- **A2 remains failing.** Select popup opening and keyboard selection failed at
  all three zooms (the value stayed First). Colour-picker visibility was false
  at all three zooms. At zoom 1 and 2, the combined pointer/wheel check failed
  because hover count did not increase; click, pointer drag and scrolling did
  occur. The log also contains `RefCell already borrowed` errors near picker
  operations. Their exact cause has not been established; it is not attributed
  to Servo or declared fixed without a reproducing diagnosis.
- Text selection, context-menu open/close, actual file-panel visibility and
  camera dismissal, and alert/confirm/prompt reveal, response and focus checks
  passed at 0.35, 1 and 2. Inspection of
  `resume/signin/menu-0.35-#selection-open.png` and its `-closed.png` counterpart
  visibly confirms that the drawn context menu disappears when the camera
  moves; this is not inferred solely from responder state. The zoom-1 prompt
  capture shows the revealed card and its modal input. The dialog has no
  visible footer buttons in that capture, despite keyboard response passing;
  inspect the Dialog configuration before claiming complete visual acceptance.
- **A3 automated checks passed:** all 15 editable selectors plus designMode,
  pending-query Space drag, Space-pan/non-drag delivery, caption and cached-card
  dragging, empty-board pan/marquees, Command-wheel, synthetic pinch and
  page-first Escape. Physical gesture delivery remains manual.

The full 63-row matrix and 11 failures are in `resume/signin/input.json` and
[resume/summary.json](resume/summary.json). A valid run can contain failing
checks; validity is not a claim that A1-A3 all pass. Captures also show poor
fixture text contrast on the page background; rendered appearance is not fully
accepted. No human VoiceOver, IME candidate, trackpad or HTML-drop check ran.

## Settle gate for R1

Both runs used baseline CSS viewport 1280 by 800 and camera sequence
1, 0.35, 2, 1. DPR was 1.40625, approximately 0.7, 1.40625, 1.40625 because of
the window pixel cap. The captured 60 ms pre-settle samples retained the prior
viewport; resize events occurred during settle.

| Fixture | Cumulative resize counts relative to baseline | Settled width/height drift | Transient event viewport |
| --- | --- | --- | --- |
| Zoom | 0, 1, 2, 2 | 0 / 0 at every step | 1280 by 800 only |
| Sign-in | 0, 1, 3, 3 | 0 / 0 at every step | **637 by 398**, then 1280 by 800 |

The sign-in transient drift is **-643 CSS pixels wide and -402 high**. This is
an observed two-message resize effect, not merely a rounding estimate. Event
counts differ between the two runs, so R1 must check the complete event trace,
not only the settled endpoint. [zoom-gate.json](zoom-gate.json) retains both
runs and links the raw samples. Zero-resize is explicitly false.

## Partial measurements

Eight populations completed three samples each before occlusion stopped the
next population. Their endpoint visibility checks passed. None met the driver's
95%-of-display-rate criterion; this is not a completed or accepted capacity
budget. The driver only writes its queried display refresh value at final
completion, so that exact value was lost on early stop and remains null. GPUI
callback frequency must not be relabeled as the display refresh rate.

For one live card, Servo presented **88.58–90.00 frames/s** while GPUI callbacks
were **119.99–120.02/s**. Process RSS was **304.78 MiB**, or **102.33 MiB above
the empty-engine baseline**. At eight live cards, RSS was **423.88 MiB**, an
amortized **27.68 MiB/card** above that baseline; rates varied widely.
[resume/partial-measurements.md](resume/partial-measurements.md) lists all eight
populations. These are process deltas, including allocator retention, not
individual card allocations or colour-buffer estimates.

Hidden/disposed memory, hidden-to-live cost, disposed restoration cost and
hidden-frame validity were **not reached**. [measurements.json](measurements.json)
contains the partial samples, their limits, and nulls for unmeasured metrics.
The interrupted population is excluded. No lifecycle result or budget is
inferred from the partial data.

## Changes in this resume and remaining work

Changed `examples/canvas_check.rs` and `examples/canvas_check/measurements.rs`
to use small windows; the main probe additionally records AppKit active-Space,
shown/minimized and occlusion state. `tests/canvas/run-checks.py` now supports
individual `--only zoom` and `--only signin` commands and stops a combined run
on an occlusion result. `tests/canvas/MANUAL.md` and this evidence folder record
the resume. The full inherited source inventory is in the blocked handoff.
No production, deps, pins or vendor changes were made during this resume.

The stop rule leaves the visible failures unfixed. Next work is to diagnose
select/colour-control dismissal and focus, inspect the dialog footer and hover
sequence, then rerun the affected check when visible execution is authorized
again. The measurement driver should preserve display/condition metadata at
startup so interrupted runs retain it, then complete all lifecycle phases in
an unobscured window. Retain the valid resize baseline above.

All windows and servers are closed. No commits, pushes, branch switches,
settings changes, screencapture, cargo check, cargo clean or alternative target
were used. GPUI images cannot show system picker windows, the desktop, IME
candidate windows or physical gesture delivery; actual AppKit picker visibility
was checked separately. Earlier stale Loading images remain explicitly invalid.
