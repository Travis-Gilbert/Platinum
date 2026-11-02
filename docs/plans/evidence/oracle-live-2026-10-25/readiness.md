# Frame oracle readiness - READY

- Receipt: `d01ed61b31394057b87e41e2be5827cd` mode `live`, 2026-10-25T23:24:35+00:00 .. 2026-10-25T23:25:50+00:00
- Frame: `651b9a36dcb897289edef7db95a5e53be684642c` (dirty)
- Platform: Darwin 27.0.0 arm64, rustc 1.97.1 (8bab26f4f 2026-07-14)

## Pins

| Source | Revision |
|---|---|
| Theorem substrate | `6e57dbb39124f472d8619fb20a9984e89eb225e8` |
| Servo fork | `81d8e2999569a8a496142651a1d129aed22fda77` |
| Servo archival base | `ebc89e684b721f0a8e2407aad20b121a61187779` |
| GStreamer | `1b2a95a1b72787cda146b46df9605b168e5b683b` |
| Rust toolchain | `1.97.1` |

## CI rows

| Row | Outcome | Exit | Seconds | Item |
|---|---|---|---|---|
| P1 | demonstrated | 0 | 0.1 | Pin unity: the bootstrap scripts, patches/README.md, the nightly workflow, this spec, Cargo.toml and the deps/ checkouts name one Theorem, one Servo and one GStreamer revision |
| P2 | demonstrated | 0 | 1.7 | The archival patches/servo-mse.patch covers exactly the base..pin diff of the fork and reproduces the pinned tree byte-for-byte |
| B1 | demonstrated | 0 | 0.3 | The patched GStreamer MSE runtime loads: gst-inspect sees the mse plugin through script/media-env |
| B2 | demonstrated | 0 | 10.1 | cargo build --locked of frame and every example against the pinned Servo fork, Theorem substrate and patched MSE runtime |
| B3 | demonstrated | 0 | 0.9 | script/setup-media resolves every GStreamer plugin the pinned Servo expects beside the built binaries |
| T1 | demonstrated | 0 | 0.4 | Native MSE backend regression: buffered-range removal and refill for VP8 and Vorbis against the pinned WPT fixtures |
| T2 | demonstrated | 0 | 9.9 | Chrome UI integration tests in headless windows (src/browser/tests.rs) |

## Live rows (never claimed by a hosted runner)

- **L1** demonstrated - Ordinary MP4/WebM playback: clock advance, dimensions, decoded red pixels, pause, seek, resume (muted; audible output is L7)
  - command: `script/media-env cargo run --locked --example media_check -- http://127.0.0.1:8766/media/`
- **L2** demonstrated - MSE in the browser fixture: chunked separate audio/video appends, end-of-stream, rendered frames, repeated play, seek/resume
  - command: `script/media-env cargo run --locked --example media_check -- --mse http://127.0.0.1:8766/media-source/`
- **L3** demonstrated - Timer GC regression: 256 coalesced timer callbacks survive allocation pressure (checksum 32640; exit 139 before the fix)
  - command: `script/media-env cargo run --locked --example media_check`
- **L4** demonstrated - Inline SVG paint pixels: CSS-inherited and stylesheet-selected paint survive rasterization, before and after restyle (needs Pillow)
  - command: `script/media-env python3 tests/rendering/check_svg_paint.py {target_dir}/debug/examples/media_check --evidence {output}/evidence`
- **L5** demonstrated - Public YouTube watch acceptance: video identity, no ad, playback, pause, seek and 1.5x speed on aqz-KE-bpKQ in a temporary profile
  - command: `script/media-env cargo run --locked --example media_check -- --youtube https://www.youtube.com/watch?v=aqz-KE-bpKQ`
- **L6** not-run - media-source WPT conformance suite against the built fork with retained pass/fail evidence (docs/plans/youtube.md step 4)
  - why not runnable here: Frame has no WPT runner path yet; the suite is open implementation work
- **L7** not-run - Audible output and full visible-window playback in the rebuilt Frame app
  - why not runnable here: requires a human at a Mac with audio
- **L8** not-run - Chrome parity measurements on the same machine and network: startup, seek recovery, stalls, dropped frames, sync, CPU/GPU, memory, power
  - why not runnable here: requires Chrome and human measurement; performance thresholds come from those measurements, not from invention

## Artifacts

- `evidence/svg-initial.png` sha256 `652c9d0cded7600a...`
- `evidence/svg-paint.json` sha256 `71666155e76f833d...`
- `evidence/svg-restyled.png` sha256 `0779b83dfc4a2147...`
- `logs/B1.log` sha256 `e8c34219181d0551...`
- `logs/B2.log` sha256 `b4f0ab15c07ce856...`
- `logs/B3.log` sha256 `f9f2c1b8efefb441...`
- `logs/L1.log` sha256 `f887b9bccd88f2bf...`
- `logs/L2.log` sha256 `c7e0b879ae4deb5e...`
- `logs/L3.log` sha256 `c897548bb315c0e1...`
- `logs/L4.log` sha256 `186318d810ae8943...`
- `logs/L5.log` sha256 `b2c6311f380d6e4a...`
- `logs/P1.log` sha256 `02c1bd59ba1082d0...`
- `logs/P2.log` sha256 `008ad51711305b09...`
- `logs/T1.log` sha256 `fa36c3c95fdf6550...`
- `logs/T2.log` sha256 `a217461a8472e54b...`
