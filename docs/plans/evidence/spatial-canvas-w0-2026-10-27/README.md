# Spatial canvas W0 — builds pass; Safari foreground checks blocked

2026-10-27 · `spike/web-viewer` · unchanged base `b4695cf` · all work uncommitted.

Both standalone stacks now build and run. The full automated interaction suite
ran on Firefox 158.0.1 and Arc 1.171.0 (Chromium 156.0.8078.48), for both stacks.
Safari 27.0.1 initialized WebGL2 and mounted 200 scene elements for both stacks,
but its test tabs remained hidden, preventing foreground interaction/timing
acceptance. **The six-cell interaction matrix is incomplete.**

## Decision 3

**Settled on 2026-10-27:** gpui-pre-web 0.3.9. The user accepted the Firefox
and Arc results as enough, so the Safari rows below are outside W0's
acceptance. The recommendation that follows was written before that ruling.

**Retain Frame's vendored gpui-pre with gpui-pre-web 0.3.9 as the preferred
candidate; do not mark the published viewer accepted yet.** Firefox and Arc
show equivalent functional results on both stacks. Zed does not close the
shared Tab gap, and its measured bundle is essentially the same size. Reusing
Frame's native GPUI and the same board crate remains the simpler integration.
There is no evidence here that justifies changing to Zed.

A final acceptance decision is blocked by Safari foreground execution, trusted
input checks, and the board's missing Tab traversal. The next concrete step is
to select a W0 Safari test tab, run each stack separately while visible, then
perform the manual checks below. Browser controls in this session exposed no
native apps or tabs. AppleScript authorization was requested and remained
pending; no AppleScript, driver, settings change, or browser download was used.

## Observed matrix

`Unavailable` means Safari stopped at the foreground precondition, not an
observed stack failure. The 200-element check reads mounted scene state; it
does not claim all 200 elements are simultaneously visible. Pointer checks use
explicit CSS-local **synthetic** offsets; see the coordinate limitation below.

| Check | Pre / Firefox | Pre / Safari | Pre / Arc | Zed / Firefox | Zed / Safari | Zed / Arc |
| --- | --- | --- | --- | --- | --- | --- |
| WebGL2 context | Pass | Pass | Pass | Pass | Pass | Pass |
| 200 scene elements | Pass | Pass | Pass | Pass | Pass | Pass |
| Synthetic pointer pan | Pass | Unavailable | Pass | Pass | Unavailable | Pass |
| Wheel pan / 10-second RAF sample | Pass | Unavailable | Pass | Pass | Unavailable | Pass |
| Synthetic wheel zoom | Pass | Unavailable | Pass | Pass | Unavailable | Pass |
| Select (delete and undo) | Pass | Unavailable | Pass | Pass | Unavailable | Pass |
| Drag (scene geometry) | Pass | Unavailable | Pass | Pass | Unavailable | Pass |
| Double-click into text | Pass | Unavailable | Pass | Pass | Unavailable | Pass |
| Typing | Pass | Unavailable | Pass | Pass | Unavailable | Pass |
| Japanese composition updates/commit | Pass | Unavailable | Pass | Pass | Unavailable | Pass |
| Synthetic Escape to board | Pass | Unavailable | Pass | Pass | Unavailable | Pass |
| Click back to board | Pass | Unavailable | Pass | Pass | Unavailable | Pass |
| Synthetic Tab into text | Fail | Unavailable | Fail | Fail | Unavailable | Fail |
| Synthetic Tab back to board | Fail | Unavailable | Fail | Fail | Unavailable | Fail |

All four completed suites had no JavaScript/Rust runtime errors. They warned
that dual-source blending was unavailable (subpixel antialiasing disabled), and
reported a device-poll timeout during resize. The checks and canvas readbacks
still completed; these warnings are retained in each JSON's `console` field.

### Safari evidence and blocker

The first Safari page loaded the WASM/font but produced no result. Later
watchdog runs on both stacks reported `visibilityAtStart: "hidden"`, successful
GL initialization, then a timeout at `mount-called`. The final diagnostic uses
timer-based readiness polling and reads the mounted scene before waiting for a
frame: both pass WebGL2 and 200 elements, then report:

```text
Error: Tab must be foreground for frame timings
```

`window.focus()` did not make these pages visible. No animation-frame timings
were invented or measured in a background tab. Some `open -a Safari` navigations
were delayed; `safari-runs.json` records the launch wait timeouts, and the final
browser JSON files below supersede those timeouts with actual initialization
results. This is a foreground/tab-control blocker, not evidence that Safari's
WebGL2 renderer is broken. Safari also explicitly refused script close for some
test tabs. W0 tab cleanup is therefore incomplete; close only tabs whose URL
starts with `http://127.0.0.1:8765/`. Other browser close attempts were issued,
but not every attempt has a `pagehide` receipt. No user tab or window was closed.

## Frame intervals

| Stack | Browser | Median ms | p95 ms | Samples | Viewport, CSS px |
| --- | --- | ---: | ---: | ---: | --- |
| gpui-pre | firefox | 8.34 | 26.18 | 837 | 1783 × 1324 |
| gpui-pre | safari | Unavailable | Unavailable | — | Hidden tab |
| gpui-pre | arc | 8.30 | 9.80 | 1200 | 1102 × 590 |
| zed | firefox | 24.04 | 33.66 | 460 | 1783 × 1324 |
| zed | safari | Unavailable | Unavailable | — | Hidden tab |
| zed | arc | 8.30 | 9.90 | 1200 | 1102 × 590 |

These are requestAnimationFrame intervals during ten seconds of wheel pan,
not GPU presentation latency. All four samples remained visible and confirmed
camera movement. DPR was 2. Existing browser window sizes were preserved; the
viewports differ. Other lanes were active on the machine. These single runs
are acceptance evidence, not a controlled performance ranking. Raw intervals
are retained in each JSON.

## Builds and bundle sizes

Both release builds, wasm-bindgen 0.2.127, and `wasm-opt -Oz --all-features`
passed. Initial Rust builds took 387.441 s (gpui-pre) and 461.846 s (Zed).
After the host-font bootstrap fix, Rust rebuilds took 148.884 s and 145.705 s;
wasm-opt took 28.762 s and 9.234 s respectively. No Rust compile errors occurred.
All crates were available offline. Both builds use only:

```text
CARGO_TARGET_DIR=/Volumes/SSD Samsung/cargo-targets/frame-canvas
TMPDIR=/Volumes/SSD Samsung/tmp
```

Exact byte counts measured by `run.py sizes`:

| Artifact | gpui-pre | Zed |
| --- | ---: | ---: |
| Compiler WASM | 13,946,946 | 13,906,565 |
| Bindgen WASM, before wasm-opt | 12,010,600 | 11,953,320 |
| Optimized WASM | 8,906,529 | 8,908,273 |
| Gzip WASM | 3,470,036 | 3,473,492 |
| JS bindings | 149,916 | 151,647 |
| Gzip JS bindings | 23,799 | 23,713 |
| Noto Sans JP asset | 9,589,900 | 9,589,900 |
| Gzip font asset | 5,863,494 | 5,863,494 |
| Raw WASM + JS + font | 18,646,345 | 18,649,820 |
| Gzip WASM + JS, font excluded | 3,493,835 | 3,497,205 |
| Gzip WASM + JS + font | 9,357,329 | 9,360,699 |

Bundle totals cover optimized WASM, generated JS bindings, and the separately
loaded font. They exclude the test HTML/check script and the unrequested licence
file transfer. Gzip totals sum individually compressed assets; the local server
serves uncompressed files. The font adds exactly 9,589,900 raw / 5,863,494 gzip
bytes to each otherwise unchanged WASM/JS bundle. Original pre-bootstrap sizes
are retained separately and are not the final bundle figures.

## Font and shared-board findings

The board's `font.rs` explicitly identifies its bundled JetBrains Mono as
Latin-only, returns no system CJK fallback on wasm, and maps absent characters
to glyph zero. The baseline Firefox scene accepted `日本語` while its
[canvas readback](gpui-pre-firefox-1793115960152703000.png) showed three missing
glyphs. This confirms the handoff's font finding.

The spike now fetches **Noto Sans JP** from its own `assets/` directory and calls
`Font::from_bytes` / `set_font_provider` before creating the board. It uses the
unmodified Google Fonts variable font at its default face coordinates. Its
SIL OFL licence is beside it. The source is pinned to Google Fonts revision
`66a36c8c94b1a5d992ee4e7f392fccfe4945767c`; exact official download URLs, hashes,
and sizes are in
[font provenance](../../../../spikes/web-viewer/assets/provenance.json).
The final Firefox/Arc readbacks contain Japanese glyphs. This is not acceptance
of text quality at every zoom or of an OS IME candidate window.

Both stacks fail Tab into ordinary text and Tab back to the board. This is
**board behavior**, not a difference between GPUI stacks: the shared
`vendor/gpui-whiteboard/src/lib.rs` shortcut handler has no board-to-text Tab
traversal and ordinary editing propagates Tab (around line 5066). Closing this
gap requires an explicit board/host keyboard traversal policy and entry/commit
hooks: Tab/Shift-Tab select an eligible text target and focus its editor, Tab
leaves/advances, and Escape commits/restores board focus. Verify it using
trusted keys as well as scene mutation probes. No vendor fix was made in W0.

### Fixture repairs and evidence limits

- The original host did not register a GPUI UI font. The first Firefox run
  panicked resolving `.SystemUIFont`. The spike now registers the board's
  existing JetBrains Mono bytes and wraps the unchanged board in a full-size
  host with that text family. The original panic receipts are preserved.
- GPUI counts double-clicks by time/position, ignoring `event.detail`. Separate
  synthetic gestures now wait beyond its 400 ms click window. Focus probes
  collapse word selections before inserting/deleting a probe character and
  include positive entry controls before testing focus exit.
- On Arc at DPR 2, unmodified synthetic PointerEvents reported `offsetX/Y` at
  half `clientX/Y` despite a canvas origin of (0,0). This caused the initial
  selection/drag/text failures. The diagnostic JSON stores both coordinates.
  The final fixture supplies explicit CSS-local offsets on synthetic events;
  it does not alter GPUI or real browser events. **Trusted pointer coordinate
  mapping is unverified.** Use `&rawpointer=1` to reproduce the unmodified path.
- Reading WebGL on a later RAF produced a blank PNG despite nonzero bytes.
  Readback now observes wgpu's presentation draw/blit and synchronously calls
  `canvas.toDataURL`, restoring the methods after capture. This runs after the
  timing sample and changes no context flags. Nonblank board/Japanese output
  was confirmed in the saved frames. Canvas readback cannot show browser
  chrome, native focus, trackpad gestures, or the OS candidate window.
- The fallback is the same fixture and exact Frame whiteboard source, compiled
  against Zed gpui/gpui_web at `6d4d90754f7dde3e62afb6fe74a632c0b4660396`.
  This does not validate the entire theoremweb application.

### Trusted input in Chromium (checked after the run)

After Codex finished, the orchestrator served the gpui-pre stack with
`&manual=1` and drove it with trusted input from the Claude desktop app's
built-in browser: Chromium 152.0.7977.167, DPR 2, a 1024 × 768 CSS-px viewport,
and a canvas at (0, 0). Nothing was posted to `/result`.

- A trusted `pointerdown` at client (150, 160) reported `offsetX/Y` (150, 160).
  The half-size offsets are specific to untrusted synthetic events, and the
  board maps trusted Chromium pointers correctly at DPR 2.
- The click selected the rectangle under the pointer, and dragging moved it.
- Double-clicking `Text 2` entered editing with the word selected. Typed keys
  replaced it, and Escape committed the edit.
- From a click on blank board, a trusted Tab did not enter any text. This
  confirms the board's missing Tab traversal with a real key.

Arc and Safari with trusted input, the IME candidate window, pinch and text
quality remain the manual checks below.

## Receipts

- gpui-pre / firefox: [gpui-pre-firefox-1793116604891970000.json](gpui-pre-firefox-1793116604891970000.json); [canvas readback](gpui-pre-firefox-1793116604891970000.png).
- gpui-pre / safari: [gpui-pre-safari-1793116929964430000.json](gpui-pre-safari-1793116929964430000.json); no rendered-frame capture.
- gpui-pre / arc: [gpui-pre-arc-1793116623948022000.json](gpui-pre-arc-1793116623948022000.json); [canvas readback](gpui-pre-arc-1793116623948022000.png).
- zed / firefox: [zed-firefox-1793116643492473000.json](zed-firefox-1793116643492473000.json); [canvas readback](zed-firefox-1793116643492473000.png).
- zed / safari: [zed-safari-1793116990944898000.json](zed-safari-1793116990944898000.json); no rendered-frame capture.
- zed / arc: [zed-arc-1793116662597785000.json](zed-arc-1793116662597785000.json); [canvas readback](zed-arc-1793116662597785000.png).

The selected results are summarized in [blocked-status.json](blocked-status.json).
Earlier attempts are diagnostic history, not additional acceptance runs.
`browser-runs.json`, `safari-runs.json`, `server.log`, and `lifecycle-*.json`
record launches, transport, timeout and close attempts. `commands.jsonl` and
`*-build.log` record actual build output and elapsed times. The original handoff
and environment blockers are preserved in `initial-handoff.md` and
`initial-blocked-status.json`.

## Resume and manual checks for the user

The server is stopped at session end. From this retained worktree:

```sh
python3 spikes/web-viewer/run.py serve --port 8765
open -a Safari 'http://127.0.0.1:8765/?stack=gpui-pre&browser=safari'
```

Select only that test tab and keep it visible until evidence is saved. Close it,
then repeat with `stack=zed`. For manual checks, append `&manual=1` to a stack
and browser URL; do these checks in Firefox, Safari and Arc, for both stacks.
Do not append `autoclose=1` for manual review.

1. **Japanese IME candidate window:** using an already installed Japanese input
   source, double-click `Text 2`, compose `にほんご`, convert and commit `日本語`.
   Check one committed string and the candidate window beside the caret before
   and after pan/zoom. Canvas PNGs cannot prove candidate-window placement.
2. **Trackpad pinch and trusted pointer mapping:** pinch in/out over blank board
   and text. Confirm the board zooms about the gesture rather than the browser
   page zooming. Click a rectangle, drag it, and double-click text at the visible
   position before/after zoom; pay particular attention to Arc's coordinate
   mapping, which was normalized only for synthetic events.
3. **Real Tab and Escape:** click blank board, press Tab and try typing. Enter
   text by double-click, press Tab to leave/advance, then return and press
   Escape. Verify typing stops editing when board focus is intended. Record
   the current Tab failure rather than assuming synthetic events prove browser
   focus defaults. Repeat Shift-Tab when traversal is implemented.
4. **Text quality:** inspect Latin and Japanese at several zoom levels, including
   caret alignment, thin strokes, missing glyphs and page-card symbols. This
   spike uses vector-outline board text and a full variable CJK asset; neither
   text quality nor production font-size optimization is accepted by W0 yet.
5. Close only the W0 localhost tabs and stop this server with Ctrl-C.

## Scope and completion

- [x] Resume existing uncommitted fixture; preserve branch/base.
- [x] Build both standalone stacks using the canvas target and SSD TMPDIR.
- [x] Measure final WASM, JS and font sizes.
- [x] Run complete automated Firefox and Arc checks for both stacks.
- [x] Observe Safari WebGL2 initialization and mounted 200-element scenes.
- [ ] Complete foreground Safari interaction/timing checks for both stacks.
- [x] Verify the missing-font and missing-Tab source findings; add licensed CJK.
- [x] Record manual steps for the user and a conditional decision 3 recommendation.
- [ ] Confirm closure of all W0 tabs (Safari refused some script closes).

No `vendor/`, `deps/`, root Cargo.toml or root Cargo.lock edits; no Frame-workspace
`cargo check`; no commit, push, branch switch, cargo clean, browser/driver
installation, Screen Recording attempt, safaridriver enablement, or settings
change. SSD free space remained above 6 GiB (83 GiB initially, 75 GiB at the
last check). The worktree is retained because all requested work remains
uncommitted and Safari acceptance is unfinished.

Files changed in this continuation: `spikes/web-viewer/run.py`, `checks.js`,
`src/lib.rs`, new font/licence/provenance under `assets/`, this README,
`blocked-status.json`, command index, file inventory, and new execution
receipts/readbacks. Existing standalone manifests and locks were reused.
The complete uncommitted inventory is [files.txt](files.txt).
