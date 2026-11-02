# Spatial canvas M1: on-screen checks and measurements pass

Surface, zoom, the whole sign-in and input run, and the measurements pass on
screen. The measurements are in
[native-measure-paced](native-measure-paced-2026-10-27/measure/measurements.json).
Still open for M1: the manual checks in `tests/canvas/MANUAL.md`, and the
resize gate, which waits for R1. Everything remains uncommitted on
`feat/spatial-canvas` at base `b4695cf`.

## On-screen runs, 2026-10-27

Five runs of the native sequence and three of the measurements alone, with
the person away from the Mac. Each is a folder here with `commands.json`,
per-case logs and records.

| Run | Surface | Zoom | Sign-in and input | Measurements |
| --- | --- | --- | --- | --- |
| [native-ready](native-ready-2026-10-27) | pass, 7.85 s | pass, 5.45 s | stopped at 13.48 s | not reached |
| [native-panel-focus](native-panel-focus-2026-10-27) | pass, 7.42 s | pass, 5.57 s | stopped at 13.71 s | not reached |
| [native-key-return](native-key-return-2026-10-27) | pass, 7.32 s | pass, 5.54 s | stopped at 24.99 s | not reached |
| [native-final](native-final-2026-10-27) | pass, 7.35 s | pass, 5.63 s | stopped at 43.37 s | not reached |
| [native-complete](native-complete-2026-10-27) | pass, 6.16 s | pass, 5.78 s | **pass, 79.93 s** | stopped at 25.77 s |
| [native-measure](native-measure-2026-10-27) | not run | not run | not run | stopped at 14.95 s |
| [native-measure-idle](native-measure-idle-2026-10-27) | not run | not run | not run | failed at 518.80 s |
| [native-measure-paced](native-measure-paced-2026-10-27) | not run | not run | not run | **pass, 519.19 s** |

Zoom passes with its expected blocker: `zoom.json` records the resize gate
failing until R1, as before.

**What each stop was.**

- native-ready: a product bug, in tabs and canvas alike. A focused colour
  input's NSColorPanel takes key; the page's blur set Servo focus to false;
  Servo hides an input's controls on blur, so the panel closed as it opened;
  and AppKit does not give key back when Frame orders a panel out, so no
  window had the keyboard. Fixed in `src/controls.rs` and `src/page.rs`: the
  page keeps Servo focus while its own colour or file panel is open
  (`Controls::owns_native_panel`), and `key_owner` hands key back to the
  window that had it when Frame closes the panel. The picker probe now also
  requires the input to stay focused and the window to be key again.
- native-panel-focus: the guard's own lag. GPUI's `is_window_active()` is a
  cached flag set by a deferred task, so it read inactive while AppKit
  already had the window key (`active:false, appkit_key:true` in
  `foreground.json`). The guard in `examples/canvas_check/foreground.rs` now
  reads AppKit's `isKeyWindow`.
- native-key-return: another agent's `tests/performance/compare.py` run
  launched Frame performance binaries that took key.
- native-final: a second Frame, built in `cargo-targets/frame` and started
  through launchd, took key.
- native-complete: keyboard focus went to Universal Control as an outbound
  target at 15:40:51.7 (WindowServer log).
- native-measure, measurements alone: Firefox (pid 18869) took keyboard
  focus at 15:46:36.6, and a modifier key event went to it at 15:46:44.1.
  Codex's computer-use service was recording window changes at the time;
  the log does not show whether a person or an agent brought Firefox
  forward. One population was sampled before the stop.
- native-measure-idle: the window kept key throughout, so this was the
  probe. After hiding the 16 cards it showed them again one at a time and
  unpaused each, so all 16 ended up animating. That saturated the main
  thread, and one page's script reply missed its 10 s timeout ("probe
  evaluation timed out"). The lifecycle never has more cards live than its
  budget, so the probe now pauses each shown or restored card again after
  its first frame, and times each one with a single page animating
  (`examples/canvas_check/measurements.rs`). Its capacity and its live and
  hidden memory records stand, and agree with the paced run.

**Verified on screen.**

- Select anchor, all three zooms within a point of the control: 0.35 at
  (439, 330) for (438.9, 330.1), 1 at (419, 338) for (419.0, 338.6), 2 at
  (388, 352) for (388.1, 352.6). An earlier failure was the probe, not the
  menu: the bounds canvas had no inset, so it sat at the menu's bottom edge.
  It now has `top_0().left_0()`.
- Hidden-card retention: the same IOSurface and hash across the hide, no
  present while hidden, presenting again when shown, `hidden_copy_bytes` 0.
  The design assumption holds with the paint and present stop; no copy at
  hide is needed.
- Input, 66 of 66 rows in native-complete, including the colour and file
  panels, dialogs, Space drag and caption drag.

## Measurements, native-measure-paced

The workload is the fixture's animated Canvas2D page, 1280 by 800 CSS per
card, in a 4 by 4 grid at zoom 0.25 or less, on the 120 Hz display. It runs
the debug example build: the frame crate is unoptimized, and dependencies,
Servo included, are opt-level 2. The window stayed key through every sample.
A `sample` of the process at 11 live cards is in
[sample-10-cards.txt](native-measure-paced-2026-10-27/sample-10-cards.txt),
named for the tenth population record that started it. It ran from
16:06:00 to 16:06:04, during the 11-card population, so treat that row as
disturbed.

**Capacity.**

| Live cards | RSS | Per-card presented fps, median (range) | GPUI fps |
| ---: | ---: | --- | --- |
| 1 | 318.9 MB | 90.0 (87.3 to 90.6) | 120.0 to 120.0 |
| 2 | 350.8 MB | 89.2 (88.2 to 90.3) | 119.7 to 120.1 |
| 3 | 384.7 MB | 89.5 (87.1 to 90.6) | 119.8 to 120.0 |
| 4 | 413.3 MB | 87.7 (85.4 to 88.9) | 119.9 to 120.3 |
| 5 | 442.8 MB | 86.3 (84.3 to 88.8) | 119.8 to 120.2 |
| 6 | 480.8 MB | 85.6 (83.7 to 88.7) | 119.2 to 120.3 |
| 7 | 506.2 MB | 84.5 (81.4 to 87.4) | 117.6 to 119.0 |
| 8 | 541.3 MB | 81.7 (78.1 to 85.0) | 98.4 to 109.1 |
| 9 | 575.1 MB | 79.8 (76.3 to 84.1) | 75.8 to 76.8 |
| 10 | 608.7 MB | 79.1 (73.4 to 83.3) | 41.9 to 48.1 |
| 11 | 730.6 MB | 76.9 (72.1 to 83.2) | 11.3 to 19.0 |
| 12 | 757.7 MB | 75.2 (69.4 to 81.1) | 1.0 to 1.9 |
| 13 | 695.1 MB | 71.1 (65.3 to 79.0) | 0.1 to 0.4 |
| 14 | 696.9 MB | 68.7 (63.2 to 77.4) | 0.0 to 0.1 |
| 15 | 748.8 MB | 64.7 (56.3 to 75.7) | 0.1 to 0.3 |
| 16 | 804.3 MB | 55.7 (48.3 to 69.7) | 2.8 to 15.2 |

- By the probe's criterion (every card and GPUI at 95% of the display rate
  in all three samples), capacity is 0. No card reaches 114 fps even alone:
  one card presents a median of 90. Other runs put one card at 74 and 95, so
  Servo's own pacing caps a card below the display rate, whatever the load.
- GPUI holds 114 fps or more through 7 animating cards here, and through 6
  in native-measure-idle. Past that it slows, and at 12 cards and up it
  draws about one frame a second or less; the guard's 16 ms poll once went
  106.7 s without running. Cards keep presenting meanwhile.
- The cost is Servo's, on the main thread. 148 of the 156 main-thread
  samples are in Servo's `spin_event_loop` calling Frame's
  `notify_new_frame_ready`: 103 in `webview.paint()`, which is WebRender
  rendering, mostly `update_texture_cache` uploading each card's
  CPU-rendered canvas (`vello_cpu`) through `glTexSubImage2D`, and 37 in
  `present()`, which is surfman's `swap_buffers` flushing each card's GL
  context to Metal. Frame's own code hardly appears, so the debug build of
  the frame crate is not the limit. Every new frame from every card is
  painted and presented synchronously as it arrives, and GPUI gets what is
  left.

**Memory**, from `vmmap` physical footprint, minus the empty engine, divided
by 16. vmmap rounds 1.5G to a tenth of a gigabyte, so the live and hidden
figures are good to about 3 MB per card.

| State | RSS | Footprint | IOSurface | Per card, footprint | Per card, RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| baseline | 212.7 MB | 121.4M | 18.4M | | |
| 16 live | 827.9 MB | 1.5G | 63.9M | 92.7 MB | 38.5 MB |
| 16 hidden | 866.5 MB | 1.5G | 63.9M | 92.7 MB | 40.9 MB |
| 16 disposed | 393.3 MB | 273.3M | 48.5M | 10.0 MB | 11.3 MB |

- RSS misses more than half the footprint. Most of a live card is GPU
  memory: IOAccelerator (graphics) goes from 5.4 MB dirty to 191.4 MB dirty
  plus 343.8 MB swapped, about 33 MB per card. IOSurface is about 2.8 MB
  per card.
- Hiding frees nothing: a hidden card keeps its whole webview and costs what
  a live one does. Only disposal gives the memory back.
- After disposing all 16, IOSurface still holds 30.1 MB above the baseline,
  most of the 45.5 MB the cards added. Whether something keeps those
  surfaces or frees them later is open.

**Lifecycle**, each card timed with a single page animating.

- Hidden to live, from showing the card to its first new present: median
  4.3 ms (2.1 to 10.5) over 16 cards.
- Disposed to restored, from adding the card to its load completing with a
  frame: median 46.2 ms (40.7 to 80.9) over 16 cards. That is a page from
  the local fixture server, so real sites take longer.
- Hidden frames valid: the same surface ids and present counts across the
  hide for all 16, no copy, and
  [hidden-retained-16.png](native-measure-paced-2026-10-27/measure/hidden-retained-16.png)
  paints every card's newest frame. Every disposed card's objects were
  released.

To repeat the measurements:

```sh
python3 -I tests/canvas/run-checks.py --only measure --evidence docs/plans/evidence/spatial-canvas-m1-2026-10-27/native-measure-<date>
```

The sections below are the report written before any window opened.

## One runner command

From `/Volumes/SSD Samsung/frame-worktrees/spatial-canvas`, once the person has
stepped away and the Mac is unlocked:

```sh
python3 -I tests/canvas/run-checks.py --only native --evidence docs/plans/evidence/spatial-canvas-m1-2026-10-27/native-ready-2026-10-27
```

This uses the example already built in
`/Volumes/SSD Samsung/cargo-targets/frame-canvas/debug/examples/canvas_check`.
It runs **surface diagnostic, zoom, sign-in/input, full measurements**, in that
order, with one small window at a time, temporary profiles, and a fixture server
bound only to `127.0.0.1`. It starts `caffeinate -d -w <runner-pid>` for the native
sequence only and terminates it in cleanup. The process watch also releases the
assertion if the runner dies. No settings are changed.

The runner stops on the first failed process or probe verdict, including
activation refusal, visibility/occlusion failure or failed retention. There are
no retries. It checks the display lock first and refuses an existing native
case directory, preventing stale evidence from being reused. Ctrl-C or SIGTERM
closes the current child, server, temporary directory and caffeinate process.
Each command retains wall time, binary hash, output, process result and explicit
probe verdict. Process exit zero alone is insufficient.

For a no-window rehearsal, add `--dry-run` and choose a different evidence
folder. Dry run starts no native binary, server, build or caffeinate process.
The executed [dry-run record](offline-ready/dry-run/dry-run.json) confirms the
case order and the binary's existence.

## Changes and checks

| Item | Implemented | Verification now | Native proof pending |
| --- | --- | --- | --- |
| Select anchor | Only select CSS coordinates are converted with `device_pixels_per_css_pixel()` before the existing device-to-window transform; shared by Tabs and Canvas | Conversion test passes at tab scale 2, canvas scales, and page zoom 1.25 | Three new 2-point geometry rows and captures |
| Hidden frames | Page sets delegate visibility before Servo show/hide; hidden frame-ready notifications skip both paint and present | Test verifies no rendering callback, frame-count increment or frame event while hidden, and resumption when shown | Same IOSurface, same hash, no new presents after hide; resumed presentation after show |
| Memory | `vmmap --summary` alongside `ps` RSS at baseline, live, hidden and disposed | Compiled | Actual physical footprint and IOSurface rows |
| Capacity | Normal key-capable window; activation requested once, then checked | Compiled | Fully foreground capacity samples |
| Runner | Ordered native sequence, temporary resources, scoped caffeinate, first-failure stop | Parses and dry run passes | Foreground execution |

The existing input matrix had 63 rows; the new select geometry rows make **66**.
The harness mounts production Page and SpatialCanvas; it does not currently
open ordinary Tabs with their native select menu. Tabs coordinate conversion is
covered by the shared-path unit test, not a native Tabs acceptance claim.

The hidden-frame fix is entirely in Frame's delegate path. Source inspection
shows Servo rebuilding its display list without the hidden webview, then
clearing the context, while the old delegate painted and presented every
notification. The gate now prevents that paint/present sequence. The unit test
checks the actual gate used by `notify_new_frame_ready`, but does not simulate
Servo or IOSurface ownership. The retained native diagnostic is the acceptance
proof. No frame copy was added: `hidden_copy_bytes` is recorded as zero. Copying
at hide remains the fallback only if the native proof fails.

The surface diagnostic writes live/hidden captures and records the held buffer
and newest buffer identities, hashes and present counts; unequal or missing
values fail the verdict. It then shows the page and requires a new present.
The 16-card lifecycle driver also checks every card's identity, hash and present
count across hide. This protects against confusing a new blank frame with
valid retention.

## Foreground and memory evidence

The measurement window is now `WindowKind::Normal`, allowing ordinary key-window
activation; no assumption is made that macOS grants the request. Startup requires
both GPUI active and visible. Refusal returns **needs the foreground**, closes
the window and stops the runner without retry.

The foreground guard observes GPUI activation and visibility events continuously,
polls every 16 ms, records transitions and maximum actual polling gap, and
latches a failure. The watchdog cancels work on failure; a final guard check
also rejects a result if a transition arrived as the work completed. Monitoring
covers fixture warm-up, capacity samples, memory collection and lifecycle work,
not just sample endpoints. No pacing override or reactivation loop is used.
GPUI's own inactive-window throttle explains a limit of the earlier run;
Frame's Page visibility policy is not called during the live capacity samples.

Zoom and input probes use the same guard. The input probe permits a key
NSColorPanel/NSOpenPanel owned by its still-active application during its
intentional native-picker checks; it does not permit another app taking focus.
Measurement and surface diagnostics allow no such focus handoff. Visibility is
what GPUI/macOS reports; AppKit's Visible bit alone only means some portion is
visible and is not independent proof of an unobscured window.

`window.json` records the display refresh even if activation is refused.
`foreground.json` records monitoring and any stop. Startup conditions, capacity
progress and each memory state are saved independently so an interrupted run
keeps completed evidence. Memory collection runs `vmmap` on a background worker,
outside capacity sampling, and saves `vmmap-<state>.txt` plus
`memory-<state>.json`. Each memory record includes RSS, physical footprint,
original IOSurface region lines and collection runtime. Footprint bytes preserve
vmmap's reported rounding, and RSS/footprint are nearby observations rather than
an atomic sample. An absent IOSurface row is explicitly marked absent, never
reported as zero. Final per-card RSS and footprint deltas use the empty-engine
baseline; allocator retention is included. No colour-buffer arithmetic is used
as a substitute for measurement.

## Measured command runtimes

| Check | Result | Wall seconds |
| --- | --- | ---: |
| Wrapper `cargo test --locked --bin frame` | 63 passed; suite execution 3.87 s | 35.462145 |
| Wrapper `cargo build --locked --example canvas_check` | Passed | 11.874249 |
| Runner native-sequence dry run | Passed; zero windows | 0.069024 |
| New Rust files formatting check | Passed | 0.089215 |
| Python AST parse | Passed | 0.001768 |
| `git diff --check` | Passed | 0.017999 |
| Protected-path inspection | Only the two pre-existing whiteboard files | 0.010593 |
| Disk check | 72 GiB free | 0.007721 |

Exact commands are in [offline-ready/commands.json](offline-ready/commands.json)
and [verification.json](offline-ready/verification.json). The built example's
SHA-256 is `9f87f2ef17e543e3597d47eeceac09ce6c90af0618a81132287cb5599bba32ef`.
The bin suite uses headless GPUI test windows and offscreen scene contexts; no
native application windows or fixture servers were launched here.

## Scope and preserved evidence

This continuation changed `src/delegate.rs`, `src/page.rs`,
`examples/canvas_check.rs`, `examples/canvas_check/measurements.rs`, added
`examples/canvas_check/foreground.rs`, and updated `tests/canvas/run-checks.py`,
`tests/canvas/MANUAL.md` and evidence. It built the prior untested changes in
`src/controls.rs`, `src/canvas/mod.rs` and
`examples/canvas_check/acceptance.rs`. No deps, pins or vendor edits were added.
The two original whiteboard patch files remain the only vendor changes.

**R1 behavior is unchanged.** [zoom-gate.json](zoom-gate.json) retains the earlier
baseline, including the 637 by 398 transient. [a2-final/gate.json](a2-final/gate.json)
retains the later 0,2,4,4 event counts. No fresh native resize result is claimed.
The old [measurements.json](measurements.json) remains the pre-fix run, with
failed hidden-frame validity and inactive capacity samples; it is not a new
budget. Prior reports are preserved in
[retention-diagnosis/handoff.md](retention-diagnosis/handoff.md),
[a2-final/handoff.md](a2-final/handoff.md), [resume/handoff.md](resume/handoff.md),
[blocked-handoff.md](blocked-handoff.md) and [initial-handoff.md](initial-handoff.md).

VoiceOver, IME candidate windows, physical trackpad pinch and HTML drag/drop
remain human-only checks in [MANUAL.md](../../../../tests/canvas/MANUAL.md).
HTML drag/drop waits for R3. GPUI captures cannot show system picker windows,
IME candidates or the desktop. No screencapture, settings changes, sign-ins,
commits, pushes, cargo check, cargo clean or alternative target were used.
