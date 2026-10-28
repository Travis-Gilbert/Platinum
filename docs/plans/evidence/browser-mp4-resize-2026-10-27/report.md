# MP4 midstream resolution changes — 2026-10-27

The remaining MP4 failure is fixed. The native browser now plays the full
1.964-second fixture, changes from 400×300 red frames to 200×150 green frames,
and seeks back and forward across the change. Apple VideoToolbox hardware
decoding remains enabled.

## Root cause and change

GStreamer's h264parse correctly parsed the new SPS dimensions but replaced them
with the MP4 container's original width/height when emitting new caps. The
result paired 200×150 codec data with 400×300 dimensions. The hardware decoder
stopped around 0.983 seconds. The parser now emits its parsed, cropped dimensions
with the matching codec data; missing-SPS handling still uses the incoming caps.
This fixes the existing parser and preserves its stream parsing and negotiation.

The patch is included in `patches/gstreamer-mse.patch` against the existing
pinned GStreamer revision. Bootstrap now builds the videoparsers plugin. Servo
loads plugin files beside the executable, so `script/setup-media` refreshes
recognized development symlinks when a private patched plugin replaces the
system version. It validates every path before changing links and still refuses
to replace real files or arbitrary symlinks. No system GStreamer files changed.

## Verification

- All 42 checks in the pinned upstream H.264 parser suite pass, including the
  added stale-container-dimensions regression. The original parser fails that
  regression (400 versus the SPS's cropped width 32) and passes the others.
  Run with `script/media-env python3 tests/media/check_h264.py`; it uses GstCheck
  and upstream test helpers directly, without requiring unrelated GstValidate.
- All nine source/replay/resize checks pass. The strengthened MP4 and WebM cases
  require progress beyond the size change, EOS, and seeking across it again.
- Three native GPUI browser runs show both dimensions, the green final surface,
  and `currentTime=duration=1.964` with `ended=true`.
- Standard MP4/WebM play/pause/seek/resume and decoded pixels pass. MSE append,
  playback, repeated play and seek/resume pass. All 62 application tests pass.
- Plugin setup checks cover preserving real files, preserving unknown symlinks,
  upgrading a recognized system link, and repeating setup idempotently.
- The full bootstrap rebuild/install succeeds. Dependency patches are checked
  against their pinned source. Python lint, JavaScript syntax and diff checks pass.

Raw results are in `native-after.json`, `native-resize/results.json`,
`parser-suite-before.log`, `parser-suite-final.log`, `standard-media.json`,
`mse-media.json`, `plugin-setup-checks.json`, and `unit-tests.log`.
The initial `parser-candidate.log` used the unchanged executable-adjacent plugin
and exposed the loader issue; it is retained as diagnostic evidence.

## Performance method

Three paired 1080p/60 H.264 video-only runs use identical frozen browser binaries
with separate plugin directories: h264-before loads the original Homebrew parser;
h264-after loads the patched parser. Other executable-adjacent plugins match.
The harness first requires readyState >= 3 and an advancing video clock beyond
0.25 seconds, then uses three seconds of warmup and twelve sampled seconds. A/B order
alternates, with separate synthetic profiles. Native redraw/surface intervals
and whole-process CPU are recorded. Inactive attempts are retained and excluded.
All compilation and media probes finish before timing; other applications remain
running, and each run records host load. These are native presentation metrics,
not physical scanout measurements or a broad streaming-site benchmark.

`environment.json` records binary, plugin, source and fixture hashes.
`video-ready-paired/results.json` retains per-run measurements and
`video-ready-paired/summary.json` reports medians. Reproduce with
`script/media-env python3 tests/performance/compare.py`, the recorded before/after
binary paths, `--compare-with`, `--modes video-only-hd --repeats 3`, and the recorded
`--video-fixture`. The `video-resize` mode verifies the native resizing fixture.

Historical benchmark binaries in the parent debug directory share its updated
plugin links. Use the recorded h264-before directory to reproduce the old parser;
its independent links retain the original system plugin. Both variants use the
same optimized Rust build; the patched native plugin is built with Meson
`debugoptimized` (optimization 2).

## Measured results

All six runs passed playback and foreground validity checks. Medians across the
three pairs (original parser → patched parser):

| Metric | Before | After |
| --- | ---: | ---: |
| New video surfaces/s | 60.00 | 60.00 |
| p95 native surface gap | 20.94 ms | 20.90 ms |
| p95 Page redraw gap | 25.01 ms | 24.45 ms |
| Whole-process CPU | 11.40 s | 11.45 s |
| Video ready before warmup | 1.690 s | 1.638 s |

Every measured presentation window spans 12.00–12.12 seconds. These results show
that the compatibility fix preserves steady playback; no speedup is claimed.
CPU includes startup, readiness waiting, warmup and sampling. The baseline uses
the Homebrew plugin build while the candidate uses the private Meson build, so
this measures the shipped plugin combination, not a compiler-identical microtest.

Earlier `video-paired`, `video-paired-validated`, and `video-final-pair` attempts
are retained as diagnostics, not included in these medians. Under host load the
old fixed warmup sometimes expired before video started; other attempts lost
foreground focus. The final playback-readiness gate resolves that measurement
problem. All final samples used the same gated executable and required active
focus throughout. Other applications remained running.

The bootstrap, parser suite, playback checks and optimized benchmark build pass.
Strict Clippy still has the 13 preexisting findings documented in the earlier
performance report. This is local controlled-fixture acceptance, not a claim of
full streaming-site conformance or physical display scanout measurement.
