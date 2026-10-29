# Source switching, replay and video smoothness — 2026-10-27

The replay stall is fixed locally. Per-source tracing corrected the earlier
source-switch diagnosis: each replacement source was playing. The short final
clip reached EOS, then the backend rewound it without a script seek. Its time
became zero, `ended` became false, and `play()` remained pending at readyState 2.
The final patch preserves the ended position and restores playback after an
explicit replay or seek, including a seek that must remain paused.

## Changes

- Record GstPlay's stopped state before dispatching EOS to script. This prevents
  script's pause request racing the following backend state-change notification.
- Avoid calling GstPlay.pause() on the stopped player: that API automatically
  seeks back to the beginning after EOS.
- Before an explicit seek from an ended, paused element, restore the backend's
  PAUSED target. Seeking otherwise resumes its previous PLAYING target while the
  DOM still says it is paused.
- Allow Servo's HAVE_CURRENT_DATA state to recover after backend preroll, while
  leaving an element still at its end in its ended state.

These are changes to the existing Servo/GStreamer state transitions. No custom
parser, decoder, timer or new dependency was introduced. The existing display
clock and video repaint optimizations remain in place. The combined dependency
patch is `patches/servo-video-frame-pacing.patch`, applied by
`script/bootstrap-media` against the pinned Servo revision.

## Playback verification

The unchanged binary reproduced the unwanted rewind and pending replay promise.
The final engine passes all seven source/replay cases in
`tests/media/source-switch.js`: initial playback, replacement while playing,
replacement while paused, rapid replacement, explicit load(), replay after EOS,
and seek-after-end with a pause-stability check before resuming.

Three native transition runs additionally verify all three replacement sources
advance and resolve their play promises. Native rendered colors change green →
red → green across codec and size changes. The final source retains
`currentTime=duration=3`, `ended=true`, and dimensions 200×150 in all three runs.
Before the fix, all three sources advanced but the final time incorrectly reset
to zero with `ended=false`.

Standard H.264/MP4 and VP9/WebM play/pause/seek/resume and decoded-pixel checks
pass. Actual rendered screenshot pixels also match the expected red frames
(see rendered-pixels.json). MSE audio/video append, playback, repeated play, and
seek/resume pass.
62 application tests, the all-target check, and the ordinary application build
pass. The earlier strict-Clippy gate had 13 existing failures; it was not rerun
for these dependency state-transition and JavaScript fixture changes.

Evidence: `baseline-replay.log`, `baseline-regressions.json`,
`final-regressions-polled.json`, `native-before/`, `native-final/results.json`,
`standard-media.json`, `mse-media.json`, and `checks.json`.

## Supplemental compatibility limit at this checkpoint

The MP4 limitation below was subsequently fixed and verified in the
[MP4 follow-up](../browser-mp4-resize-2026-10-27/report.md).

The WebM fixture that changes intrinsic resolution during one stream passes,
including green decoded pixels at 200×150. The corresponding MP4 fixture still
stops around 0.983 seconds rather than completing its 1.964 seconds in the native
pipeline. It also fails in a standalone Apple hardware-decoder pipeline; an
explicit standalone avdec_h264 software pipeline completes it. Disabling the
VideoToolbox factories in the native diagnostic did not make that native test
pass, so a general software-decoding workaround is not established or shipped.

This is distinct from replacing a video's source URL, which passes. The full
supplemental script intentionally exits with failure for that MP4 case:
**eight of nine cases pass**, including all seven source/replay cases. No full
adaptive-playback or MSE conformance claim is made.

## Smoothness measurements

The final comparison uses the previous optimized binary (`frame-direct-candidate`)
as baseline and `frame-source-switch-final` as candidate. Three A/B pairs
alternate order, using the same local H.264 1920×1080/60 fixture, three-second
warmup and twelve-second sample. Builds and other media probes finish before
timing. Foreground validation excludes runs with inactive Page renders and
retains their raw results. Native surface presentation and Page redraw timings
are measured, not physical scanout or input-to-photon latency.

Medians across the three valid pairs:

| Metric | Before replay fix | After replay fix |
| --- | ---: | ---: |
| New video surfaces/s | 60.00 | 60.00 |
| Page redraw gap, p95 | 17.23 ms | 17.46 ms |
| New surface gap, p95 | 18.11 ms | 18.11 ms |
| Whole-process CPU | 8.84 s | 9.02 s |

The 60 fps presentation cadence is retained. Redraw p95 rose 0.23 ms (1.4%);
surface p95 was effectively unchanged; CPU rose 2.1%. This is a correctness fix,
not a further speedup. One baseline attempt lost foreground focus and was
excluded, then rerun; six valid timing runs remain. Three pairs do not establish
statistical equivalence or performance across other sites.

Paired results and medians are saved in `video-paired/results.json` and
`video-paired/summary.json`. Binary, dependency, fixture and source hashes are
saved in `environment.json`. The benchmark is a local controlled workload, not
a claim about arbitrary streaming sites.

## Reproduction

Run `script/media-env python3 tests/performance/compare.py` with the baseline
binary, an output directory, `--compare-with` the candidate, `--modes
video-only-hd --repeats 3`, and `--video-fixture` pointing to the generated
motion-1080p60.mp4 recorded in environment.json. Use `--modes video-transition`
for the source/color/size/ended-position checks with the same fixture directory.

For the standalone regression script, serve `deps/servo-mse/tests/wpt/tests`
over local HTTP. Set `FRAME_MEDIA_SCRIPT` to the absolute path of
`tests/media/source-switch.js`, then run the recorded media_check binary through
`script/media-env` with the server's `/media/` URL. Omit the script override for
standard codec playback; use `--mse` with the `/media-source/` URL for MSE.

No installation, commit, push, or deployment was performed. The isolated build
cache and frozen comparison binaries are retained for reproduction.
