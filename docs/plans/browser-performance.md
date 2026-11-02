# Browser hot-path performance

Authorized scope: sample toolbar tint outside the per-frame hot path; separate
session persistence from visual updates and write off the UI thread; reuse search
runtime/providers and release the store during network work. Compare search,
scrolling and video against the same instrumented baseline. Preserve other work.

Baseline Frame: 2e135c046d2feba70b57980c8d70e5bb7667822d.
Installed Theorem: 6e57dbb39124f472d8619fb20a9984e89eb225e8 (clean at start;
bootstrap currently names an older revision). Servo and native media stay pinned.

Plan:
1. Add opt-in measurements and reproducible native/browser and search workloads;
   retain baseline executable and results before changing behavior.
2. Implement the three changes using GPUI tasks, existing store/query types and
   Tokio. Preserve ordered saves, shutdown flush and search retention semantics.
3. Run focused regression tests and identical before/after workloads. Record
   distributions and correctness, including unavailable or inconclusive gates.

Measurements use isolated synthetic profiles, a local HTTP fixture and actual
Servo/GPUI/GStreamer. Controlled search transport is reported separately from
live web search. Host build contention is recorded; no whole-browser speedup
will be inferred from a microbenchmark. Evidence lives in
`docs/plans/evidence/browser-performance-2026-10-27/`.

Implementation decisions:
- Tint sampling uses one GPUI trailing timer at 125 ms per page. Hiding a page
  cancels its pending sample; a stopped animation still receives its last tint.
  TintChanged invalidates the affected panel and dock tab chrome without syncing
  title/address/session state.
- Session snapshots are scheduled on navigation, selection and dock/content
  changes, debounced 200 ms. Tokio watch keeps just the newest queued snapshot;
  one GPUI background consumer serializes writes and deduplicates only successes.
  Closing/quitting synchronously drains that worker: GPUI's asynchronous quit
  allowance is only 200 ms, shorter than the profile DB's 3 s busy timeout.
  Ordinary interaction never waits for session disk I/O.
- Prepared search owns graph/profile/index snapshots; provider/model I/O runs
  without the browser-store mutex. Completion is bound to its originating store.
  Index generations still used by queries are protected from pruning. Frame
  shares a current-thread Tokio runtime and provider clients; a semaphore limits
  executing searches to four. This reuses the existing query/ranking/synthesis
  implementations and the substrate's prepare/execute/finish pattern.
- The baseline uses opt-level 2 for the application and dependencies, debug info
  disabled, with the same settings for the candidate. This is an optimized dev
  build, not a release build. The native target was copy-on-write cloned to
  `/Volumes/SSD Samsung/cargo-targets/frame-performance` because another checkout
  was using the configured target. No other build was interrupted.

Validation so far:
- Optimized instrumented baseline and candidate binaries built successfully.
- `cargo test --locked --bin frame --features performance-check -- --test-threads=1`:
  61 passed. Includes immediate-close flush, latest-value writer drain, failed
  write retry, visual updates not scheduling persistence, concurrent searches
  and capture while provider futures remain pending.
- `cargo test --locked -p theorem-browser-substrate --lib -- --test-threads=1`:
  30 passed, including leased index generations, out-of-order search completion
  and same-label/different-store completion refusal.
- Native acceptance: stopped-frame tint reached RGB (30,160,70); redirect state
  persisted the final URL; profile session readback after exit passed. Actual
  scrolling and WebM media clock advancement passed in the smoke workloads.
- Desktop screen capture returned errors despite permission preflight succeeding;
  no screenshot-based visual claim is made. The native probe reads the actual
  Page tint used by the card and selected-tab renderers.
- Strict Clippy initially reported 14 warnings as errors: one new instrumentation
  `collapsible_if` was fixed; the other 13 occur in unchanged profile UI,
  bookmark/history/suggestion code, engine, navigator, and share code. No lint
  suppression or unrelated cleanup was added. The remaining strict gate is
  recorded separately from behavioral tests.

Completed paired measurements: see
[evidence report](evidence/browser-performance-2026-10-27/report.md) and its raw
per-process JSON/logs. Three repeated A/B pairs establish lower search latency
and tint-sampling overhead; they do not establish smoother scrolling/video or
lower rendering-workload process CPU. The additional live-provider pair also
shows concurrent search overlap, with GDELT degraded in both builds. No public
provider coverage or decoder-drop-count claim is made.


## Smoothness follow-up (2026-10-27)

Authorized: investigate remaining scrolling/video bottlenecks, fix and remeasure;
remove GDELT and identify the search execution path.

The new baseline includes all three previous optimizations. Additional opt-in
probes separate Servo event-loop time, frame-ready callbacks, Page render calls,
and IOSurface presentation. Keep the original comparison artifacts unchanged.
Servo's embedding API recommends painting during window repaint. Frame instead
painted immediately in the frame-ready callback. Test coalescing ready frames
into one paint when GPUI renders Page, using the existing render invalidation.
No new scheduler or rendering dependency is needed.

GDELT is removed from default/fallback providers and rejected even in an explicit
Frame provider list. Bluesky remains the keyless fallback; general web coverage
requires a configured general web provider. Search runs RustyWebQueryService
in-process through theorem-browser-substrate; it does not use the Theorem SDK.

Completed: three alternating pairs for native scrolling and video-only playback,
62 unit tests, all-target check, production build, media and live-search smoke
checks. Final results and limitations are recorded in
[evidence report](evidence/browser-performance-2026-10-27/smoothness-report.md).


Follow-up findings and implementation:
- Immediate paint coalescing alone was rejected: Servo's independent 8 ms timer
  plus GPUI's display tick reduced cadence. The final candidate implements
  Servo's existing RefreshDriver hook using GPUI's one-shot on_next_frame.
  The adapter follows servoshell's VsyncRefreshDriver observer queue; no additional
  timing thread or fixed frame-rate limit is introduced. At the display callback,
  Servo is pumped and its Page events drained before GPUI draws the dirty page.
- Static pages request no more refresh callbacks. Hidden tabs keep Servo's normal
  visibility handling; native hide/resume acceptance passed with zero hidden-tab
  surface presentations.
- Foreground focus matters: GPUI deliberately throttles inactive animation
  callbacks. Harness now activates the application, records inactive Page renders,
  and rejects inactive/occluded runs. Temporary caffeinate keeps the display awake
  during timing only. Raw excluded runs are retained, not mixed into medians.
- Native wheel input enters through Window::dispatch_event and the production
  Page wheel handler. A generated H.264 1920x1080/60 fixture supplements the original
  320x240/24 VP9 clip. Video-only workload disables the animated banner/rAF loop,
  so native presentation cadence can be measured separately from page animation.
- macOS Servo media currently uses the CPU-backed GStreamer video path: BGRA data
  is copied from the decoded buffer and uploaded to WebRender. This is a remaining
  architectural cost; inspect decoder diagnostics before attributing dropped
  frames or promising a zero-copy improvement. The existing OS/GL synchronization
  contract and surface lifetime scheme are preserved.


Final selection supersedes the exploratory deferred-paint candidate: keep
painting ready frames immediately, while Servo animations use GPUI's shared
refresh driver. Deferring every paint to the next GPUI render introduced an
extra display-tick boundary for video-only content. Direct painting retained the
scrolling improvement and reduced that video jitter. Stable video frames also
use the existing Paint API repaint path instead of invalidating style/layout.
This Servo change began as patches/servo-video-frame-pacing.patch and is now
commit efdf7370 in the pinned Servo fork.

Final paired medians: scrolling p95 redraw gap 16.74 → 9.35 ms; video-only
p95 redraw gap 25.53 → 17.47 ms and new surfaces/s 46.8 → 60.0. Whole-process
CPU changed +3% for scrolling and -5% for video. Source-switch playback stalls
remain a separate compatibility limitation. Strict Clippy retains 13 existing
errors. No installation, push or deployment was performed.

## Source-switch / end-of-stream follow-up (2026-10-27)

Authorized: fix the reported source-switch stall and reverify video smoothness.
Event tracing corrected the earlier diagnosis: all replacement sources play;
the final short clip ends, then GstPlay.pause() implicitly rewinds it to zero.
The resulting HAVE_CURRENT_DATA state leaves later play() promises pending.
Investigating preservation of the ended position and ready-state recovery on
replay, with per-source progress, promise, pause, seek and resize regression checks.


Final source-switch fix: mark the GStreamer player stopped before dispatching
EOS; avoid GstPlay.pause()'s implicit rewind when already stopped; restore the
PAUSED pipeline target before an explicit seek from an ended, paused element.
Servo readiness can recover from HAVE_CURRENT_DATA after preroll without
reopening an element still at its end. No new parser, decoder, timer, or dependency.

Validation: seven source/replay checks pass, including replacement while playing
and paused, rapid replacement, load(), replay and paused seek after EOS. Three
native transition runs show every source advancing, play promises resolving,
rendered green/red/green frames and final position 3/3 seconds with ended=true.
Standard MP4/WebM and MSE playback, 62 unit tests, all-target check and the ordinary
build pass. Supplemental WebM intrinsic resize passes; MP4 intrinsic resize
still ends early in the native pipeline. This separate compatibility check is
retained as a failure, not counted as a successful source-switch check.

The new paired video measurements and final evidence are recorded in
[evidence report](evidence/browser-source-switch-2026-10-27/report.md).

Final smoothness medians across three valid A/B pairs: 60.00 → 60.00 new video
surfaces/s; p95 Page redraw gap 17.23 → 17.46 ms; p95 surface gap 18.11 →
18.11 ms; whole-process CPU 8.84 → 9.02 s. One inactive baseline attempt was
retained and excluded, then rerun. No further speedup is claimed. Bootstrap patch
idempotency, focused script checks and actual rendered MP4/WebM pixel checks pass.
No installation, commit, push or deployment was performed.


## MP4 midstream resolution follow-up (2026-10-27)

Authorized: fix the remaining MP4 issue, verify playback and smoothness, then
commit and push if acceptance passes. h264parse was emitting stale container
width/height with updated SPS codec data. Emit the parsed cropped dimensions;
build the private videoparsers plugin and refresh recognized setup-media links
so Servo's executable-adjacent loader selects it. Existing system files and
unrecognized links remain protected.

All 42 parser checks and nine native source/replay/resize cases pass, including
playing to EOS and seeking backward and forward across the size change. Three
native browser runs end at 1.964 seconds with a green 200×150 surface. MP4/WebM,
MSE and 62 application tests pass. Evidence and paired smoothness results:
[evidence report](evidence/browser-mp4-resize-2026-10-27/report.md).

Final readiness-gated A/B acceptance: all six foreground runs pass, with three
pairs at 60.00 → 60.00 surfaces/s; p95 surface interval 20.94 → 20.90 ms;
p95 Page redraw gap 25.01 → 24.45 ms; whole-process CPU 11.40 → 11.45 s.
Every sampled presentation window spans at least 12 seconds. Earlier fixed-warmup
attempts are retained as diagnostics and excluded. This preserves smoothness;
no additional speedup is claimed. Verification satisfies the user's commit/push
gate; this changeset packages the authorized search, scrolling, source-switch
and MP4 fixes with reproducible dependency patches and evidence on
`feat/liquid-glass-chrome`. No application deployment or system-plugin change.
