# Frame performance results — 2026-10-27

All three requested changes are implemented. Search improved substantially.
Tint sampling became much cheaper and visual updates no longer build session
snapshots. These measurements do **not** establish a reliable improvement in
scrolling or video smoothness, or in whole-process CPU consumption for those
workloads.

## Paired comparison

Three independent processes per variant per workload, alternating baseline-first
and candidate-first order. Numbers below are medians across processes. Each
native workload uses a 1280 × 820 logical-point browser window, three seconds of
startup warmup, and twelve seconds of native measurements. The scrolling page
contains 100 text sections and an animated colored header. Video uses Servo's
`movie_5.webm`, looping and muted, with the same animated header. This intentionally
exercises the dynamic tint path. Profiles and HTTP fixtures are isolated.

| Metric | Baseline | Candidate | Interpretation |
| --- | ---: | ---: | --- |
| Cold local-provider search | 979 ms | 843 ms | 13.9% lower |
| Warm local-provider search | 756 ms | 609 ms | 19.4% lower |
| Three simultaneous searches, batch completion | 2,371 ms | 615 ms | 74.1% lower; requests overlap |
| Scroll: tint sampling time over measured interval | 595.3 ms | 10.1 ms | 98.3% lower |
| Scroll: tint reads | 576 | 84 | 85.4% fewer |
| Scroll: visual-only session snapshots | 367 | 0 | Removed |
| Scroll: Servo frame-interval p95 | 51.89 ms | 50.49 ms | Too variable to establish smoother scrolling |
| Scroll: document rAF interval p95 | 28.54 ms | 28.10 ms | Essentially unchanged |
| Scroll: whole-process CPU time | 5.73 s | 5.67 s | Essentially unchanged |
| Video: tint sampling time over measured interval | 666.4 ms | 16.1 ms | 97.6% lower |
| Video: tint reads | 571 | 85 | 85.1% fewer |
| Video: visual-only session snapshots | 365 | 0 | Removed |
| Video: Servo frame-interval p95 | 51.05 ms | 50.10 ms | Too variable to establish smoother video |
| Video: document rAF interval p95 | 30.56 ms | 28.61 ms | Small change within run-to-run variation |
| Video: whole-process CPU time | 5.76 s | 5.95 s | 3.3% higher median; no CPU improvement established |

The controlled provider is an actual local HTTP server with a fixed 600 ms
response delay, accessed through the production SearXNG provider, query planner,
ranking and session store. It is not a public-web latency prediction. The search
batch also captures a page after 150 ms. Capture timing was variable (244 ms vs
271 ms median), because capture sometimes acquired the baseline mutex before
provider work began. No capture speedup is claimed from that timing. A separate
gated-provider regression proves capture completes while both searches remain
pending on provider responses.

Captured pages affect subsequent searches. Already prepared concurrent searches
use their original graph/index snapshot; this explains the baseline's occasional
additional local hit during the batch. The regression verifies the captured
page is discoverable by the next search.

## Live provider check

One additional paired run used the real keyless GDELT and Bluesky providers:

- Warm single query: 4,284 ms → 4,004 ms.
- Three simultaneous queries: 12,530 ms → 4,008 ms.
- Both builds returned results. GDELT reported a provider error in both; the
  roughly four-second provider deadline remained visible in both variants.

This confirms the overlap through real network providers, including degraded
provider handling. One pair, with a degraded provider, is not a general-web
performance or coverage benchmark. Raw results are in `live-search/`.

## Correctness and limits

- Frame: **61 tests passed**. Search substrate: **30 tests passed**.
- Regression coverage includes concurrent provider waits, capture during those
  waits, latest-session ordering, retrying an identical snapshot after a failed
  write, closing before debounce, and visual updates not scheduling persistence.
- Substrate tests verify query snapshots survive newer index generations,
  out-of-order completion, generation reclamation, and completion refusal by a
  different store even when it uses the same profile label.
- Actual native scrolling and WebM playback passed all paired runs; the media
  clock advanced and `readyState` reached 4. Servo did not expose video playback
  quality counters in this fixture, so decoder dropped-frame counts are unknown.
- A stopped-animation fixture reached the exact final green tint (30,160,70).
  A redirect fixture saved the final URL. Session database readback after process
  exit passed for all final native workloads.
- Tint access is no longer inside the frame event handler. The surfman 0.14 CGL
  swap-chain still unbinds and flushes its producer surface before handing it off;
  its documented synchronization path was inspected and left unchanged.
- macOS screen capture failed despite permission preflight succeeding. No
  screenshot-based visual acceptance is claimed; final tint was read from the
  actual Page state consumed by native chrome.
- The ordinary optimized build and `cargo check --all-targets --features
  performance-check` passed.
- Strict Clippy is blocked by pre-existing warnings in untouched code. The one
  warning introduced in the measurement helper was corrected without suppressing
  checks or changing unrelated code. See `clippy-final.log` for the final result.

Frame presentation timings measure intervals between Servo paint/present
callbacks, not GPU scanout or input-to-photon latency. Document rAF measures
JavaScript animation scheduling, not decoded video frames. Process CPU includes
startup and shutdown, not just the twelve-second sample. Other user builds and
apps were running; per-process host load is retained in each result. The small
frame-pacing differences are not treated as statistically established wins.

## Reproduction and source binding

Hardware: Apple M1 Max, 10 logical CPUs, 32 GiB RAM; macOS 27.0.1.
Both binaries use the same opt-level 2 dev profile, with debug info disabled.
They are **not release builds**. Exact source revisions, toolchain, source hashes
and executable hashes are in `environment.json`.

The configured target was busy in another checkout. This task used the isolated
copy-on-write target `/Volumes/SSD Samsung/cargo-targets/frame-performance`.
Baseline and candidate executables remain there as `debug/frame-baseline` and
`debug/frame-candidate`, beside their required media-library links. The frozen
baseline source archive is `/Volumes/SSD Samsung/frame-performance/baseline-source.tar.gz`.
The staging source directory was subsequently reused for the candidate; its
name `baseline-source` is historical, not its current contents.

From the Frame checkout:

```sh
script/media-env python3 tests/performance/compare.py \
  '/Volumes/SSD Samsung/cargo-targets/frame-performance/debug/frame-baseline' \
  /path/to/new-results \
  --compare-with '/Volumes/SSD Samsung/cargo-targets/frame-performance/debug/frame-candidate' \
  --repeats 3
python3 tests/performance/summarize.py /path/to/new-results/results.json
```

Build environment: `CARGO_TARGET_DIR` points at that isolated target,
`CARGO_BUILD_JOBS=2`, `CARGO_PROFILE_DEV_OPT_LEVEL=2`, and
`CARGO_PROFILE_DEV_DEBUG=0`. The opt-in `performance-check` feature adds the
harness; ordinary builds contain none of its timers/counters/entry point.
After measurement, equivalent lint-only cleanup collapsed a nested helper `if`
and removed unused benchmark-helper export warnings in source-sharing examples.
This does not change measurement behavior. Production behavior is unchanged
from the candidate exercised above.

Theorem's installed revision was newer than Frame's prior bootstrap pin. The
bootstrap now pins the measured installed revision and applies
`patches/browser-search-concurrency.patch` idempotently. The patch preserves the
existing search implementation instead of duplicating its ranking, synthesis,
privacy or persistence logic. No branch was pushed or application deployed.
