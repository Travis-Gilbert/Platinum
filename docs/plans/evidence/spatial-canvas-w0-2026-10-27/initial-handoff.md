# Spatial canvas W0 — blocked before compilation

2026-10-27 · `spike/web-viewer` · base `b4695cf`.

**W0 is incomplete. Neither renderer was compiled or exercised in a browser.**
Both standalone dependency graphs resolve offline, with `space` enabled, but
the sandbox refuses writes to the required shared Cargo target. It also refuses
the local HTTP listener. No browser tabs were opened; no server remains running.
No system/browser security settings were changed.

## Blockers and decision 3

1. Both release builds stop before rustc with:

   ```text
   error: failed to create directory `/Volumes/SSD Samsung/cargo-targets/frame/release`
   Caused by:
     Operation not permitted (os error 1)
   ```

   The writable roots supplied to this session include the worktree and temporary
   directories, but do not include the required target or TMPDIR on the SSD.
   No alternative target was created. This is an environment failure, not
   evidence that the vendored GPUI cannot compile for wasm.

2. `python3 spikes/web-viewer/run.py serve --port 8765` fails at
   `socket.bind` with `PermissionError: [Errno 1] Operation not permitted`.
   The page/server source is prepared, but a runnable bundle is not available.

3. Online Cargo metadata could not resolve `index.crates.io` when fetching
   `config.json`. This was worked around using the existing cache and a
   wasm-bindgen version matching the installed CLI. **No missing crate or Git
   dependency is currently identified:** both target-filtered metadata commands
   subsequently passed offline. Further build-time downloads remain untested.

**Recommendation:** retain gpui-pre-web 0.3.9 as the preferred candidate because
it uses Frame's actual GPUI and board. Do not approve it as the published viewer
yet, and do not choose Zed based on these failures. Decision 3 remains unresolved
until this fixture runs in all three browsers. The sandbox prevented the
comparison needed to settle it; claiming a final stack choice would invent
evidence. Resume with this worktree, the existing SSD target and TMPDIR writable,
and permission to bind loopback. This session cannot request sandbox escalation.

## What is prepared

- `spikes/web-viewer/Cargo.toml` and its own `Cargo.lock`: standalone workspace,
  exact gpui-pre and gpui-pre-web 0.3.9, gpui-pre patched to Frame's vendored copy,
  Frame's path whiteboard with its default `space` feature. 466 resolved packages.
- `spikes/web-viewer/src/lib.rs`: 200 elements (67 rectangles, 67 page cards,
  66 editable text elements), native `WhiteboardView`, forced `WebGl`,
  single-threaded platform, retained `ApplicationHandle`, and readback from the
  actual mounted scene. No DOM replacement editor or custom gesture engine.
- `spikes/web-viewer-zed/Cargo.toml` and its own `Cargo.lock`: Zed gpui 0.2.2 and
  gpui_web at `6d4d90754f7dde3e62afb6fe74a632c0b4660396`, matching theoremweb's
  source pin. 461 resolved packages.
- `spikes/web-viewer-zed/whiteboard/Cargo.toml` points its library at Frame's
  existing board source and changes only dependency identity to Zed. This avoids
  theoremweb's design/application dependency graph pulled through its font crate.
  The fallback shares the fixture source and browser checks, isolating the GPUI
  stack comparison. It does **not** validate theoremweb's whole application or
  its font provisioning. Theoremweb's manifest, browser bootstrap and board
  manifest were read, never edited.
- `index.html` and `checks.js`: scripted pointer pan, wheel pan and zoom,
  selection checked through delete/undo, drag checked through geometry, typing
  and Japanese composition checked through scene text, focus probes checked by
  actual text mutation, and Tab/Escape dispatch. Snapshots accompany results.
  The existing board enters text editing on **double-click**. The fixture
  preserves that behavior and records the entry gesture.
- `run.py`: Python standard-library server and command logger; no new server
  framework. POST results go to this evidence directory. Canvas PNG readback is
  attempted and recorded separately from visual review. Browser user agent,
  viewport, DPR and errors are included. The ten-second pan records RAF median,
  p95 and every interval, rejecting a run that becomes backgrounded. RAF cadence
  is not GPU presentation latency, and snapshots prove camera movement rather
  than merely successful event dispatch.
- Build utility uses the exact requested TMPDIR/target, checks `df -g` and the
  6 GiB floor, uses installed wasm-bindgen 0.2.127, runs wasm-opt, and records
  pre/post-optimization WASM, gzip WASM, JS and combined gzip sizes separately.

The hand-written code is fixture generation, instrumentation, build invocation
and evidence transport, which the board/platform do not provide. Rendering,
gestures, selection, editing and composition use their existing implementations.
The small coordinate conversion in the script inverts `Camera::screen_to_world`
to target DOM events. It does not implement board mechanics.

All fixture code is **uncompiled**. Rustfmt accepted the Rust syntax and Node
accepted the JavaScript syntax; those are not runtime or compilation receipts.
Neither the server's POST path nor the bundle/optimizer path could be run.

## Per-browser acceptance

Versions read from installed application bundle plists:

| Browser | Installed version | Browser run |
| --- | --- | --- |
| Firefox | 158.0.1 | Not started |
| Safari | 27.0.1 | Not started |
| Arc (Chromium lane) | Arc 1.171.0; Chromium/ArcCore 156.0.8078.48 | Not started |

ArcCore version source:
`/Applications/Arc.app/Contents/Frameworks/ArcCore.framework/Versions/A/Resources/Info.plist`,
`CFBundleShortVersionString`. This is installed-package evidence, not a browser
user-agent receipt. The page will also record `navigator.userAgent` when run.

The following matrix applies independently to **both** stacks. `Fail (blocked)`
means the acceptance item could not be demonstrated due to the environment;
it does **not** mean an observed Firefox/Safari/Chromium rendering failure.
Machine-readable provenance is in [blocked-status.json](blocked-status.json).

| Item | Mode | Firefox | Safari | Arc |
| --- | --- | --- | --- | --- |
| WebGL2 initialization / 200-element board | Automated | Fail (blocked) | Fail (blocked) | Fail (blocked) |
| Pointer pan and wheel pan | Automated | Fail (blocked) | Fail (blocked) | Fail (blocked) |
| Zoom | Automated | Fail (blocked) | Fail (blocked) | Fail (blocked) |
| Select and drag | Automated | Fail (blocked) | Fail (blocked) | Fail (blocked) |
| Typing | Automated | Fail (blocked) | Fail (blocked) | Fail (blocked) |
| Synthetic Japanese composition | Automated | Fail (blocked) | Fail (blocked) | Fail (blocked) |
| Click focus into text and back | Automated | Fail (blocked) | Fail (blocked) | Fail (blocked) |
| Synthetic Tab / Escape behavior | Automated | Fail (blocked) | Fail (blocked) | Fail (blocked) |
| Ten-second RAF pan measurement | Automated | Fail (blocked) | Fail (blocked) | Fail (blocked) |
| IME candidate window beside caret | Manual | Manual | Manual | Manual |
| Trackpad pinch | Manual | Manual | Manual | Manual |
| Real Tab and Escape | Manual | Manual | Manual | Manual |
| Text quality, Latin and Japanese | Manual | Manual | Manual | Manual |

Frame-time medians/p95: **unavailable** for every stack/browser. Raw WASM,
wasm-opt, gzip and total bundle sizes: **unavailable**. No rendered frames were
captured. JSON in this folder is an environment status record, not a fabricated
browser result. The harness writes distinct, timestamped browser result files
only after it actually runs.

## Source findings to verify next

- gpui-pre-web's `platform.rs:151` accepts an explicit backend. Its window
  creates the document-owned canvas and hidden textarea; W0 uses
  `Application::with_platform(...).run_embedded(...)`, as theoremweb does.
- `gpui-pre-wgpu-0.3.9/src/wgpu_context.rs:350-352` maps `WebGl` exclusively
  to `wgpu::Backends::GL`. Its renderer has a dedicated WebGL2 shader/texture
  path because WebGL2 has no storage buffers (`wgpu_renderer.rs:30,171`).
- `gpui-pre-web-0.3.9/src/events.rs:1129-1170` routes composition updates to
  `replace_and_mark_text_in_range`, commits marked text on composition end and
  unmarks it. `ime_mirror.rs` maintains the hidden textarea. These are source
  capabilities, not proof of real Japanese input or candidate placement.
- Frame's `vendor/gpui-whiteboard/src/input.rs` already implements the GPUI
  input handler, UTF-16 range conversion and caret bounds through the camera.
  No substitute GPUI text input was needed.
- `vendor/gpui-whiteboard/src/font.rs:104` explicitly describes the bundled
  font as Latin-only. Its wasm system-CJK loader returns `None`, and glyph
  lookup falls back to glyph zero. The unmodified wasm board therefore lacks
  a Japanese face even if composition updates its stored text successfully.
  Validate text appearance and provide a suitable licensed font through the
  existing font-provider seam before viewer acceptance. Switching renderer
  alone does not fix the board's vector-outline font choice.
- `vendor/gpui-whiteboard/src/lib.rs:5066` propagates Tab for ordinary text;
  the board shortcut handler has no board-to-text Tab traversal. Escape commits
  text and restores the board's editing mode. The prepared host adds no Tab
  traversal. Expect the Tab checks to expose this gap; only browser execution
  can establish final behavior. Fixes beyond cfg gates to this vendor are
  outside this lane's permitted changes.

**Vendor patches:** none. Neither `vendor/gpui-whiteboard`, `vendor/gpui-pre`,
`deps/`, root Cargo.toml nor root Cargo.lock was changed. No Frame-workspace
`cargo check`, commit, branch creation or push was run.

## Resume commands

From this worktree, with the required SSD paths writable and loopback allowed:

```sh
cd '/Volumes/SSD Samsung/frame-worktrees/web-viewer-spike'
python3 spikes/web-viewer/run.py build --stack gpui-pre
python3 spikes/web-viewer/run.py build --stack zed
python3 spikes/web-viewer/run.py serve --port 8765
```

In another terminal, run each browser sequentially and keep its test tab visible
until the page reports that evidence was saved. Close only the test tab opened
for that run before opening the next. Do not run the timing tests concurrently.

```sh
open -a Firefox 'http://127.0.0.1:8765/?stack=gpui-pre&browser=firefox'
open -a Safari 'http://127.0.0.1:8765/?stack=gpui-pre&browser=safari'
open -a Arc 'http://127.0.0.1:8765/?stack=gpui-pre&browser=arc'
open -a Firefox 'http://127.0.0.1:8765/?stack=zed&browser=firefox'
open -a Safari 'http://127.0.0.1:8765/?stack=zed&browser=safari'
open -a Arc 'http://127.0.0.1:8765/?stack=zed&browser=arc'
```

These `open` commands are handoff commands, **not commands executed here**.
The build utility keeps the required target directory, waits normally for Cargo
locks, uses no browser/driver downloads, and performs no workspace build.

## Manual checks (not executed)

Run each stack in each browser with `&manual=1` appended to its URL. That mode
mounts the same 200-element fixture without the automated mutations.

1. **Manual — Japanese candidate window:** use an already installed Japanese
   input source, double-click `Text 2`, compose `にほんご`, invoke conversion and
   choose `日本語`. Confirm a single committed string and readable glyphs. Check
   the candidate window beside the caret at the initial zoom, after pan and
   after zoom. Capture the whole browser/candidate UI with a screenshot;
   canvas readback cannot capture an OS candidate window.
2. **Manual — trackpad pinch:** pinch in and out over blank board and text;
   verify that the board camera changes about the gesture and the browser page
   itself does not zoom. Pan again and confirm subsequent selection aligns.
3. **Manual — real Tab and Escape:** click blank board, press Tab and attempt
   typing in the text. From the editing text press Tab to leave it; return by
   double-click and press Escape. Verify subsequent typing does not alter the
   text when board focus is intended. Record actual traversal and failures.
   Synthetic KeyboardEvents cannot prove trusted browser focus defaults.
4. **Manual — text quality:** inspect Latin and Japanese at several zoom levels,
   including caret alignment and missing-glyph boxes. Review each saved PNG for
   a real, nonblank frame; framebuffer retention is backend-dependent. Use an
   ordinary browser screenshot if canvas capture is empty. Close test tabs and
   stop the local server with Ctrl-C when finished.

## Execution receipts and files

- [commands.jsonl](commands.jsonl): real elapsed wall times measured with
  `time.perf_counter` for both instrumented build attempts and their disk checks.
  gpui-pre build: **0.211 s**, exit 101; Zed build: **0.313 s**, exit 101.
  Both had 88 GiB free before starting.
- [command-index.md](command-index.md): every shell command issued in the session,
  its exit result and tool-reported elapsed time. Earlier shell calls use the
  execution tool's reported timing, which often rounds to near zero; they are
  not substituted for the measured build runtimes above.
- [shell-commands.json](shell-commands.json): command output receipts, including
  metadata failures, successful offline resolution and the denied server bind.
  Some long source inspections were output-limited; unrelated private memory
  output is omitted.
- [blocked-status.json](blocked-status.json): per-stack/per-browser acceptance
  status with `observed: false`, installed browser versions and null measurements.

Changed files are all new, uncommitted files under `spikes/web-viewer/`,
`spikes/web-viewer-zed/`, and this evidence directory. The final inventory is
in [files.txt](files.txt). The requested worktree is retained for the orchestrator;
it contains unfinished, uncommitted work and has not been removed.

Checklist: standalone manifests/locks and source inspection complete; fixture
and harness authored; syntax checks passed; wasm compilation, bundling, browser
execution, manual checks, performance/size figures and final stack decision
blocked. Resume at the first build command, then repair any real compiler or
runtime findings before treating this harness as validated.
