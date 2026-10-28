# R1 verification — 2026-10-27

**Implemented and left uncommitted** in Servo `feat/viewport-css-size` (base
`2b522612c4d`) and Frame `feat/canvas-viewport` (base `89264e4`). Frame's existing
Cargo path change is preserved. No native window checks ran.

[API contract and changed files](implementation.md) ·
[Every command, environment, result and runtime](commands.md) ·
[Zoom dry-run plan](zoom-dry-run/dry-run.json) · [Built artifact identity](artifact.json)

The new `WebView::set_viewport(css_size, hidpi_scale_factor)` sends the complete
viewport in one update. The device surface rounds up; CSS size stays exact.
Window resize steps compare CSS dimensions, while full viewport changes still
drive media queries and image-environment updates. Visual-viewport size also
excludes the rounded surface padding. Frame uses card world size plus capped
scale on Canvas, or bounds plus display scale in Tabs. The zoom gate records and
checks every resize event, animation-frame sample and gesture/settled snapshot.

## Final checks

Servo: `cargo test -p servo --test webview NAME -- --exact`, one process per test.
Wall times include Cargo overhead. Only these nine Servo tests were selected.

| Test | Result | Seconds |
| --- | --- | ---: |
| `test_set_viewport_scale_only` | Pass | 20.909 |
| `test_set_viewport_css_resize` | Pass | 1.286 |
| `test_set_viewport_then_legacy_resize_and_scale` | Pass | 1.005 |
| `test_negative_resize_to_request` | Pass | 0.781 |
| `test_resize_webview_zero` | Pass | 0.766 |
| `test_webview_resize_interactivity` | Pass | 0.851 |
| `test_page_zoom` | Pass | 0.754 |
| `test_viewport_meta_tag_initial_scale` | Pass | 0.767 |
| `test_pinch_zoom_update_dom_visual_viewport` | Pass | 0.866 |

- [x] Servo: **9/9 pass**, including 3 new integration tests. Scale-only checks
  cover Window and VisualViewport events/sizes, DPR, resolution queries, ceil
  surface sizing, and two scales with the same rounded surface size.
- [x] `frame-canvas.sh cargo test --locked --bin frame`: **64/64 pass**,
  **37.542 s** wall time; **2.32 s** test execution. Includes the new asymmetric-cap test.
- [x] `frame-canvas.sh cargo build --locked --example canvas_check`: **passed**,
  **14.445 s** wall time.
- [x] Probe trace regression: **0.046 s**, passed in Node without a window.
  Retained `probe-unit.cjs` covers a transient 637×398 event and a stable scale-only trace.
- [x] Requested zoom dry-run: **0.179 s**, passed. Only `zoom-probe.html`,
  `127.0.0.1`, shared example binary, zooms `1,0.35,2,1`, a fresh evidence directory;
  stop on first failure. Zero windows; no server or caffeinate started.
- [ ] Native zoom, surface, sign-in and measurement: **not run**, orchestrator-owned.

Earlier receipts are retained: initial Servo loader failure **5.165 s**;
combined Servo invocation **254.853 s** (one pass, eight process-global
`Already initialized` failures); first successful Frame tests **849.342 s** and
example build **1500.973 s**, including long shared-lock waits. Final review
strengthened the scale-only test to cover VisualViewport and reproduced its
rounding bug in **21.066 s** before fixing it and rerunning all checks above.

Servo uses only `cargo-targets/servo-r1`, with command-local debug=0 and strip=none
(the latter fixes the confirmed macOS `mis-aligned LINKEDIT string pool` loader
failure). Frame uses the required shared wrapper. TMPDIR is on the SSD; free
space stayed above 6 GiB and the final check showed **52 GiB**. No cargo check,
commit, push, window, screencapture, system-setting change or server was run.

The dry-run proves command planning, not native behavior. Next: the orchestrator
reviews both uncommitted checkouts and runs the native zoom gate against the R1
binary in a **fresh** case directory. `artifact.json` identifies this build in
the shared target. Native R1 acceptance remains open until that trace passes.
