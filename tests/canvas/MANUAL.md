# M1 checks requiring a person

The automated checks use a temporary profile and local fixtures. Run them with:

```sh
python3 -I tests/canvas/run-checks.py --only native --evidence docs/plans/evidence/spatial-canvas-m1-2026-10-27/native-ready-2026-10-27
```

Run once the person has stepped away. Keep the desktop unlocked and leave the
check window in the foreground. The runner holds `caffeinate -d` only during its
native sequence, checks activation rather than assuming it succeeded, and stops
at the first failure without retry. It requires a passing `result.json`; AppKit
process exit 0 alone does not certify a check. Choose a fresh evidence directory.
To rehearse without opening any window, add `--dry-run`.

Every Cargo command goes through `/Volumes/SSD Samsung/cargo-targets/frame-canvas.sh`.
The runner serves fixtures on an ephemeral `127.0.0.1` port, runs the surface diagnostic, zoom probe, sign-in/input probe and full
lifecycle measurement in order, and closes its server. The native probes close
their windows on completion. JSON records and GPUI `render_to_image` captures
are in the evidence directory. Those captures do not include the macOS IME
candidate window, system file/colour panels, the desktop, or physical gestures.

For the following checks, serve `tests/canvas` on loopback, open its `signin/`
fixture in a temporary Frame profile, and use Ctrl-Cmd-B to enter the board.
Do not use a real profile, sign into a real site, or change system settings.
Use an already configured input source and existing accessibility permissions.
Repeat at zoom 0.35, 1 and 2. Record the date, display, input source, outcome,
and any failure in the evidence README; do not treat automation as these proofs.

- **IME candidate window:** With an already configured composing input source,
  compose in the textarea and contenteditable field. Confirm the candidate
  window stays beneath the caret after pan and zoom, and choosing a candidate
  inserts it once. If no such input source is configured, record that blocker.
- **Physical trackpad pinch:** Pinch over both empty board and the live page.
  The board must zoom about the gesture location; page zoom must not change.
  Continue typing and scrolling afterward. Synthetic pinch dispatch verifies
  routing only, not the operating system's trackpad event delivery.
- **VoiceOver:** With VoiceOver already available, navigate chrome and page
  content and inspect focus/bounds after pan and zoom. The baseline has no
  Servo accessibility-tree bridge, so page accessibility is an explicit M4
  blocker. Native chrome accessibility cannot stand in for web accessibility.
- **HTML drag and drop:** Waiting for R3. After R3 lands, test a page's HTML
  draggable and a Finder file drop at each zoom, including after panning.
  Current pointer-capture drag automation is a different API and does not
  prove HTML drag/drop. This Servo has no HTML drag/drop path in Tabs or Canvas.

No human results have been recorded by this continuation unless explicitly
listed as such in the evidence README.

## Native resume on 2026-10-27

The visible 900 by 650 point zoom probe completed in 8.007748 seconds. The
sign-in/input probe completed in 79.610780 seconds: session retention and all
A3 checks passed, but 11 of 63 input checks failed (select opening/selection,
colour-picker opening, and repeated hover at zoom 1 and 2). These failures are
not moved into the human-only checklist. The measurement run stopped after
62.256221 seconds because its window became occluded; no reactivation or retry
followed. macOS remained unlocked. See the evidence README for the valid resize
gate, partial samples, failure details and image-inspection limits.

For subsequent authorized runs, use `--only zoom`, `--only signin` and
`--only measure` separately. Keep the check window unobscured on the active
Space. Stop on an occlusion result; do not interpret process exit 0 as success
or reuse a stale image. No human-only result has been recorded by this resume.

## A2 repair and single final pass on 2026-10-27

The final example build passed in 16.989335 seconds; 61 bin tests passed in
23.981168 seconds (2.40 seconds test execution). The zoom probe took 7.797161
seconds and the sign-in/input probe took 81.217115 seconds, passing all 63
matrix rows. Dialog footer buttons are visible and native picker borrow errors
are absent. R1 still has resize events, including the preserved 637 by 398
transient; no resize implementation was changed.

Visual review found a remaining select-menu anchor offset that the matrix does
not assert. The complete measurement took 128.151413 seconds and failed hidden
frame retention: all 16 retained cards became blank. The popup measurement
window stayed visible but was inactive at 42 of 48 sample endpoints, so its
observed capacity of zero at 120 Hz is not a foreground budget. These are
implementation/measurement failures, not items delegated to this human-only
checklist. No additional native retry followed the requested final pass.

See `docs/plans/evidence/spatial-canvas-m1-2026-10-27/README.md` for the complete
matrix, diagnoses, RSS and lifecycle timings, retained earlier runs, and capture
limits. No VoiceOver, IME candidate-window, physical trackpad or HTML-drop result
was recorded. Future automated runs must use a fresh evidence subdirectory.

## Subsequent visibility stop on 2026-10-27

The surface-only diagnostic stopped at preflight after 4.170938 seconds: GPUI
reported Hidden/inactive, while AppKit reported shown, on the active Space,
not minimized, with its Visible occlusion bit. macOS remained unlocked. No
retry or final acceptance pass ran. The select coordinate conversion and
geometry rows are drafted but unbuilt. The earlier 63-row pass predates these
edits and is not validation of them. No new human-only result was recorded.

## Offline preparation after the visibility stop

No native windows were opened in this continuation. The shared Tabs/Canvas
select conversion and hidden paint/present gate pass their unit tests; all 63
bin tests passed in 35.462145 seconds (3.87 seconds suite execution). The example
built in 11.874249 seconds. The one-command runner dry run passed in 0.069024
seconds without starting a native binary, server or caffeinate process.

The upcoming input matrix includes three select geometry rows (66 total).
The surface diagnostic requires the same IOSurface and hash with no new present
while hidden, then resumption after show. The measurement records vmmap physical
footprint/IOSurface regions beside RSS and uses continuous activation/visibility
observers plus polling. A normal key-capable window is used, but refused
activation produces a clear needs-the-foreground result. Do not reuse the old
inactive capacity figures as a budget. These native proofs remain pending.
