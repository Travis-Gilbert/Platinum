# Spatial canvas M1: A2 repair and one final pass

**M1 remains incomplete.** The final visible input matrix passes **63/63**, with
visible dialog buttons and no `RefCell already borrowed` errors. Image review
exposes a select-menu anchor offset that the matrix does not assert. The full
measurement run completed, but hidden newest frames became blank. These are
remaining failures, not R1 exceptions.

Worktree `/Volumes/SSD Samsung/frame-worktrees/spatial-canvas`, branch
`feat/spatial-canvas`, base `b4695cf`; everything remains uncommitted.

## Validity and runtimes

The initial `ioreg -n Root -d1 -a` check reported `IOConsoleLocked: false`
in **0.054949 seconds**; see [display state](a2-repair/display-state.json).
The final probes ran sequentially in 900 by 650 point windows, with temporary
profiles and individual loopback fixture servers. All reached completion
without a visibility/occlusion stop. Each window and server closed afterward.
Only one final run of each requested check was made after the repairs.

| Final check | Result | Measured wall seconds |
| --- | --- | ---: |
| Wrapper example build | Passed | 16.989335 |
| Wrapper bin tests | 61 passed; test execution 2.40 s | 23.981168 |
| Zoom probe | Passed apart from R1 zero-resize exception | 7.797161 |
| Sign-in/input probe | Session passed; 63/63 matrix rows passed | 81.217115 |
| Full measurement driver, 1 through 16 cards | Completed; verdict false because hidden-frame validity failed | 128.151413 |

[Final commands](a2-final/commands.json) retain exact arguments, runtimes,
process results and binary SHA-256. AppKit returned process exit 0 even for the
failed measurement verdict; the runner correctly rejected its `result.json`.
The final binary hash is
`152ceb37d1f085ecc63c8d479b1f1de2a4ff947c93f25f8912142628c6b72f83`.

Diagnostic commands are retained separately, not represented as final passes:
first diagnostic build **275.202324 s**, targeted diagnosis **14.964778 s**;
second diagnostic build failed **19.666803 s** because `on_hover` needed the
`StatefulInteractiveElement` import, repaired build **15.586449 s**, targeted
focus/hover diagnosis **15.083137 s**. See
[a2-repair](a2-repair/command-runtimes.md),
[a2-focus-diagnosis](a2-focus-diagnosis/command-runtimes.md) and
[final runtimes](a2-final/command-runtimes.md).

Earlier blocked runs remain invalid; no old capture is reused as fresh proof.
The previous unlocked run and its occlusion stop are preserved in
[resume/handoff.md](resume/handoff.md), with its original partial
[measurement summary](resume/measurements-summary.json).
[blocked-handoff.md](blocked-handoff.md), [blocked-zoom-gate.json](blocked-zoom-gate.json),
[blocked-measurements.json](blocked-measurements.json), and
[initial-handoff.md](initial-handoff.md) remain available.

## Diagnoses and repairs

1. **Dialog footer confirmed.** The earlier prompt capture has no buttons.
   GPUI Kit's plain Dialog renders only its explicit footer; AlertDialog builds
   the button footer. Canvas dialogs now use `open_alert_dialog`, with
   `.confirm()` for confirm and prompt. The final
   [prompt capture](a2-final/signin/dialog-0.35-prompt.png) visibly shows Cancel
   and OK. Alert, confirm and prompt responses, focus return and card reveal
   pass at all three zooms. Existing Kit primitives are reused.
2. **Native panel reentrancy confirmed, with a timing correction.** The first
   trace reports colour opening at 1793120925546 ms, returning visible at
   1793120925563, and Servo Hide at 1793120925594. Thus `opened: false` was an
   already-dismissed panel, not proof that it never opened or a premature
   visibility read. Borrow-error pairs follow synchronous colour dismissal
   and file cancellation (camera dismissal at 1793120928583). File opening
   itself was already asynchronous. Colour open now runs in the existing
   foreground task outside an App update; colour close and native file cancel
   also run outside updates. The final log has zero borrow-error occurrences;
   AppKit visibility is true before and false after camera dismissal for both
   panels at zoom 0.35, 1 and 2.
3. **Select delivery and focus failure reproduced.** Servo delivered Select
   and Frame mounted it at 1793120923911 ms; Hide arrived 11 ms later. The
   second diagnostic trace shows Page blur setting Servo focus false, followed
   by DOM blur/focusout and Hide. Frame now retains Servo focus while its own
   popup owns GPUI focus. Opening, camera dismissal and keyboard selection of
   Second pass at every zoom. Tracing is gated by `FRAME_CANVAS_TRACE`.
4. **Hover had a forwarding defect and an ambiguous probe.** Moving off the
   card left CSS `:hover` true: `on_mouse_exit` handled window exit, not the
   element boundary. `on_hover(false)` now sends Servo MouseLeftViewport.
   The probe pans the card away, waits, verifies `:hover` clears, then enters a
   non-target page element before the target. This also avoids the pinned
   Servo boundary-event case when the previous target is null. Pointer,
   hover, drag and wheel checks pass at all zooms. No Servo changes were made.
5. **Measurement conditions survive a stopped run.** Refresh rate, layout,
   baseline RSS and compiler activity are written at startup. The window uses
   `WindowKind::PopUp`: pinned gpui-pre-macos 0.3.9 maps it to NSPanel at popup
   window level, without a separate frame clock. No frame-pacing override was
   added. Sample endpoint activation is recorded because visible popup windows
   can still be inactive and GPUI then throttles callbacks.

## Final acceptance matrix and visual limits

- **A1 session passes:** text, caret, scrolling, cookie, document and webview
  identities survive zoom 1, 0.35, 2, 1; continued typing uses the retained
  caret. Navigation starts remain 0 during the camera sequence. Initial open
  and identity records are in [navigation.json](a2-final/signin/navigation.json).
- **A1 zero-resize fails as expected until R1.** See the separate gate below.
- **A2 automated rows pass:** pointer/wheel, text selection, select opening and
  keyboard response, context menu, native panels and dialogs at all three
  zooms. Inspection of the open/closed `#selection` and `#choice` capture pairs
  at 0.35 confirms the drawn menus disappear after camera motion; this was not
  inferred solely from responder cleanup.
- **A2 visual failure remains:** the now-visible select menu is displaced from
  its HTML control in [this capture](a2-final/signin/menu-0.35-%23choice-open.png).
  Servo's `document_embedder_controls.rs` returns the no-point control border
  rectangle in CSS pixels (with an explicit device-pixel FIXME). Frame's
  `Page::below` treats that rectangle as device pixels. The popup position
  therefore applies the raster conversion twice. The matrix checks lifetime
  and response, not anchor geometry. This newly exposed failure was recorded
  without another native rerun after the requested single final pass.
- **A3 automated rows pass:** Space on all 15 editable selectors and designMode,
  pending-query drag, Space pan/non-drag delivery, caption and cached-card
  dragging, empty-board pan/marquee, Command-wheel, synthetic pinch, page-first
  Escape. Physical gesture delivery remains manual.

The complete matrix is [input.json](a2-final/signin/input.json); the summary is
[a2-final/summary.json](a2-final/summary.json). Poor fixture text contrast also
remains visible. `render_to_image` captures GPUI composition; it cannot show
system panels, the desktop, IME candidates or physical gesture delivery.
Panel visibility was observed separately through AppKit. VoiceOver, IME,
physical trackpad pinch and HTML drop have no human results; see
[MANUAL.md](../../../../tests/canvas/MANUAL.md). HTML drop waits for R3.

## R1 settle gate: baseline preserved, new observation appended

The earlier [zoom-gate.json](zoom-gate.json) is unchanged, including its
**637 by 398** transient (-643 width, -402 height relative to 1280 by 800).
R1 code and resize behavior were left alone.

Both final fixtures again use baseline 1280 by 800, zoom 1, 0.35, 2, 1 and DPR
1.40625, approximately 0.7, 1.40625, 1.40625. Both now record cumulative resize
counts **0, 2, 4, 4** relative to baseline. Settled width/height drift is **0/0**
at every step. Event traces show **2571 by 1607** (+1291/+807) then 1280 by 800
at zoom 0.35, and **637 by 398** (-643/-402) then 1280 by 800 at zoom 2.
The pre-settle samples retain the prior viewport. Timing-dependent event
counts differ from the previous run; settled dimensions alone are insufficient.
[New gate](a2-final/gate.json) contains both complete final traces without
replacing the previous baseline. Zero-resize is explicitly false.

## Complete measurements and their limits

[Raw measurements](a2-final/measure/measurements.json),
[startup conditions](a2-final/measure/conditions.json), and
[capacity progress](a2-final/measure/capacity-progress.json) retain all samples.
The root [measurements.json](measurements.json) is a copy of the complete record.

Display refresh was **120.000005 Hz**, scale 2. The workload is animated
Canvas2D, 1280 by 800 CSS pixels per card, laid out in a 4 by 4 grid in a
900 by 650 point window, using production Page, SpatialCanvas and IOSurfaces.
No compiler was running at startup. Each population from 1 through 16 has
three measured samples of about two seconds.

**Observed capacity: 0 of the tested populations met the criterion** (every
card and GPUI at least 95% of display refresh in all three samples). This is
not an accepted foreground browsing budget: **42/48 sample endpoints were
inactive**, and GPUI's existing background throttle affects callback rates.
The two active one-card samples still showed only **93.33 and 93.01 Servo
frames/s**, with **120.00 and 118.68 GPUI callbacks/s**. Activation was measured
at endpoints, not continuously. No reactivation or pacing override was used.

RSS is process resident memory from `ps`, including engine/allocator retention,
not per-card allocations or colour-buffer estimates. Baseline was **194.6875 MiB**.
At the 16-card lifecycle phase:

| State | Process RSS MiB | Delta from baseline per card MiB |
| --- | ---: | ---: |
| Live | 601.7031 | 25.4385 |
| Hidden | 615.1719 | 26.2803 |
| Disposed | 361.4375 | 10.4219 |

These lifecycle RSS readings follow image capture; the per-population samples
were taken earlier and remain separately recorded. All **16 Page weak entities
expired** after disposal. Residual RSS must not be labeled 16 leaked pages.

| Operation, 16 sequential observations | Minimum ms | Median ms | Maximum ms |
| --- | ---: | ---: | ---: |
| Hidden to live, through first new frame | 14.193 | 27.779 | 55.987 |
| Disposed restoration, through load complete and first frame | 39.521 | 92.881 | 218.773 |

**Hidden newest-frame validity fails for all 16 cards.** After pausing animation,
waiting 500 ms, hiding for two seconds, and showing the retained surfaces before
reactivation, every signature differs and converges to the same blank-frame
hash. The [live image](a2-final/measure/live-16.png) shows distinct animated
pages; [hidden retained image](a2-final/measure/hidden-retained-16.png) shows 16
white cards. Dimensions remain 419 by 262 pixels. This establishes that the
current hide path does not preserve a useful newest frame; it does not by
itself distinguish in-place IOSurface mutation from replacement of the cached
buffer. Ownership/lifetime tracing is the next diagnostic step. The failure
is retained in the measurement verdict, not waived.

## Changed files and remaining work

This repair changed `src/controls.rs`, `src/page.rs`,
`examples/canvas_check.rs`, `examples/canvas_check/acceptance.rs`,
`examples/canvas_check/measurements.rs`, `tests/canvas/run-checks.py`,
`tests/canvas/MANUAL.md` and this evidence folder. The full inherited draft
inventory remains in the initial/blocked handoffs. `notes-app.md` already agrees
with the plan's top-level webviews, settle resize, focus/input and measured
live-card policy; no numeric budget was invented or inserted.

Remaining M1 work is the select anchor conversion and an assertion for its
geometry, hidden-frame ownership/retention diagnosis and repair, and a capacity
measurement with recorded continuous foreground conditions. The requested
single final pass is complete; no further native retry was made. R1 remains
its own zero-resize exception. Human-only checks are explicitly unperformed.

All windows/servers and temporary profiles were closed/removed. SSD free space
was 72 GiB. No deps, pins or vendor edits were added in this repair; the two
existing whiteboard patch files remain the only vendor changes. No commits,
pushes, branch switches, settings changes, screencapture, cargo check, cargo
clean or alternative target directory were used.

Final non-native verification is in [verification.json](a2-final/verification.json):
`git diff --check` passed in 0.019875 s; Python parsing passed in 0.001996 s;
scoped Rust formatting passed in 0.065604 s. An initial recursive formatting
check (0.136663 s) reported formatting differences in imported production
modules; no broad formatting was applied. The subsequent check uses
`skip_children=true` and covers every new Rust file. Protected-path inspection
(0.011457 s) lists only the two pre-existing whiteboard files. Process, port and
temporary-directory cleanup verification took 0.000765 s; the final display
query took 0.039691 s and still reported unlocked.
