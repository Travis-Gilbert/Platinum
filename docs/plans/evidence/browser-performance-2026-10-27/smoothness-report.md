# Browser smoothness follow-up — 2026-10-27

Implemented locally: synchronize Servo animation requests with GPUI's display
clock; repaint stable video images without invalidating style/layout; remove
GDELT from Frame provider selection. The selected candidate improves scrolling
cadence and raises steady 1080p/60 video presentation from about 47 to 60 frames/s.
No branch push, application installation, or deployment was performed.

## Selected measurements

Medians from **three alternating A/B pairs per workload**, all with active
foreground rendering. Baseline includes the earlier tint, persistence, and
concurrent-search optimizations. Both variants use identical instrumentation.

| Workload | GPUI page redraw gap, p95 | New surface gap, p95 | New surfaces/s | Whole-process CPU |
| --- | ---: | ---: | ---: | ---: |
| Wheel scrolling + animated page | 16.74 → 9.35 ms | 12.39 → 9.28 ms | 98.7 → 120.0 | 8.88 → 9.15 s |
| 1080p/60 H.264, video only | 25.53 → 17.47 ms | 23.25 → 17.93 ms | 46.8 → 60.0 | 9.03 → 8.55 s |

Scrolling redraw p95 improved 44%; video redraw p95 improved 32%. New-video-surface
p95 improved 23%. Video presentations increased 28%, reaching the 60 fps source
rate. Scrolling process CPU rose 3%, while video process CPU fell 5%. This is a
cadence improvement, not a blanket claim of lower CPU or faster arbitrary sites.

Raw selected runs: [direct-paired/results.json](direct-paired/results.json).
Distributions: [direct-paired/summary.json](direct-paired/summary.json).
Each raw run retains host load, foreground status, native counters, fixture state,
and session readback. No selected timing run lost foreground focus.

Controlled search remained unchanged in the earlier three-pair follow-up:
warm search 608 → 608 ms, three concurrent searches 615 → 609 ms against a 600 ms
local provider. Rendering changes do not run in the search-only harness branch.
Those results are in [smoothness-awake/summary.json](smoothness-awake/summary.json).

## Bottlenecks and final changes

1. Servo's default refresh driver starts an independent 8 ms timer after painting.
   Its timing drifts against GPUI's display clock. Window-backed contexts now
   implement Servo's RefreshDriver observer hook using GPUI's one-shot
   on_next_frame callback. Servo is pumped before the window draws. The adapter
   follows Servo's existing VsyncRefreshDriver pattern and adds no timing thread
   or fixed-rate poller. Static pages stop requesting ticks. Standalone media and
   profile probes retain the default timer because they have no GPUI window.
2. HTMLMediaElement marked the video node dirty on every frame. For unchanged
   dimensions and image identity, the new path uses the existing Paint API to
   generate a frame after UpdateImage, reusing the display list. New image keys
   and natural-size changes still invalidate layout and preserve resize events.
   The dependency change is reproducible via
   [servo-video-frame-pacing.patch](../../../../patches/servo-video-frame-pacing.patch)
   and script/bootstrap-media; bootstrap idempotency was checked.
3. GDELT is excluded from default, fallback, and explicit Frame provider lists.
   Bluesky is the remaining keyless fallback. Configured general-web providers
   remain available, and the UI reports when none are configured. Existing local
   browsing/search history is preserved.

The final implementation keeps immediate ready-frame painting. Deferring every
paint to the next GPUI render reduced work but added a display-tick boundary to
video-only playback. It was tested and rejected. The shared clock plus immediate
painting retained the scrolling benefit and reduced video jitter consistently.

## Search execution path

Address bar → Frame SearchService → theorem-browser-substrate
prepare_programmable_search → **RustyWebQueryService in the same process** →
provider HTTP clients → retained local search session. The Theorem SDK and a
hosted RustyRed search endpoint are not involved. In-process search still uses
network HTTP for external providers. The store lock is released during provider
I/O, preserving the earlier concurrency improvement.

## Validation

- Selected application: 62 unit tests passed; all-target check and ordinary
  production build passed. Strict Clippy still reports the same 13 pre-existing
  issues in profile UI, browser, engine, navigator, and share code; no new
  application lint failures were introduced.
- Standalone H.264/MP4 and VP9/WebM: play, pause, seek, resume, metadata, decoded
  pixel checks and actual rendered screenshot pixel checks passed. Both rendered
  images contain the expected red video frame.
- MSE smoke: audio/video append, playback, repeated play, seek and resume passed.
- Live default-provider search returned ten hits per query using only Bluesky;
  GDELT was absent from every provider receipt. Warm query was 129 ms and three
  concurrent queries completed in 419 ms. This is a smoke check, not a paired
  network benchmark. Evidence: selected-live-search/results.json.
- Native lifecycle: hidden tabs produce zero new surfaces and resume rendering;
  stopped animation reaches its final tint; redirected final URL persists.
- Source/image identity checks: native rendered tint changes green → red → green
  across same-size and different-size source replacements; final dimensions are
  200×150. This verifies first-frame display-list updates, not resumed playback
  after changing source (see the limitation below).
- Compiler/test details are in selected-checks.json, validation.json, and the
  associated logs. Native correctness runs are separate from timing runs and may
  run while unfocused; their correctness assertions still require changed
  rendered output. Performance comparisons require foreground rendering.

## Method and limits

Actual native Servo + GPUI + GStreamer, optimized dev build (opt-level 2, debug
info disabled), isolated synthetic profiles, 1280×820 logical window, three
seconds warmup and twelve seconds sampled. A/B order alternates across repeats.
Wheel events enter through Window::dispatch_event, exercising production hit
testing and Page input routing. The scrolling fixture contains text and an
animated banner. Video-only disables the banner/rAF loop. The HD test clip is
locally generated H.264 at 1920×1080/60. Temporary caffeinate prevents display
sleep during timing; no power settings are changed. Other applications remain
running. CPU includes startup/shutdown, not only the sample window.

These are browser redraw and surface-presentation measurements, **not physical
screen scanout or input-to-photon measurements**. Three pairs show direction and
repeatability, not a broad statistical or real-site benchmark. Servo exposes
neither getVideoPlaybackQuality nor requestVideoFrameCallback in this pinned
build, so decoder drops are not inferred from rAF or presentation counts.

A separate GStreamer diagnostic selected Apple GstVtdecHw decoding to NV12 at
1920×1080/60 and reported 862 appsink buffers rendered, 0 dropped. Those are sink
counters, not screen scanout counts. Extra diagnostic logging is excluded from
timing medians. The macOS media path still copies decoded pixels into BGRA data
before WebRender upload; zero-copy decoding/compositing was not introduced.

**Follow-up correction:** subsequent per-source tracing showed that each source
played. The final short clip ended, then an unwanted backend rewind made it look
stalled. The EOS/replay fault is fixed in the
[source-switch follow-up](../browser-source-switch-2026-10-27/report.md). The
MP4 midstream-resolution limitation remains separate. The original observation
below is retained for context.

**Observation at this checkpoint:** source switching can leave the next video at
readyState 2/currentTime 0 despite showing its first frame. The WPT MP4 fixture
with an internal resolution change stalled the same way in the unchanged
baseline and candidate. The dynamic-resolution WebM fixture also stalled in the
candidate. This was not counted as a passing playback/resize check. The focused
source-replacement checks above verify first-frame colors and dimensions only.
This performance change does not claim to fix source-switch/adaptive-playback
compatibility or complete MSE conformance.

## Reproduction and evidence

Use script/media-env with tests/performance/compare.py. Selected binaries,
source/patch hashes, fixture hashes and build settings are recorded in
[smoothness-environment.json](smoothness-environment.json). Baseline binaries are
retained beside their runtime libraries in the isolated frame-performance target.

```
ffmpeg -f lavfi -i testsrc2=size=1920x1080:rate=60:duration=16 \
  -c:v libx264 -preset ultrafast -crf 23 -pix_fmt yuv420p -threads 2 \
  -an -movflags +faststart motion-1080p60.mp4
```

Source-transition fixtures use the same encoder with
color=c=green:size=320x240:rate=30:duration=3 and size=200x150, named green.mp4 and
green-small.mp4 beside the HD fixture. Those generated files stay outside the
repository; their hashes are recorded.

Exploratory snapshots are retained rather than rewritten: smoothness-probe
(coalescing alone), refresh-probe/foreground-probe (clock/focus diagnosis),
smoothness-awake (clock + deferred painting), video-only-paired (its video-only
regression), video-diagnostics (decoder selection/sink counters), final-paired
(repaint optimization with deferred painting), and direct-paired (selected).
The initial smoothness-paired run was aborted when display idle occluded the
window. Inactive timing runs are excluded and retained with their status.
