# Verification evidence

The PNGs and JSON reports were produced by native run 9 and its fresh-process
restore. The stale-context toast in some screenshots is the expected refusal
from the negative dictation test; its copy was subsequently changed to plain
language. The native note test ran with an active AppKit window, as recorded
in `native-focus.json`. Supplied transcripts test insertion separately from ASR.

| Check | Result |
| --- | --- |
| Platinum binary tests | 243 passed, 2 ignored |
| Individual visit integration tests | 2 passed |
| Explicit real Parakeet WAV oracle | 1 passed |
| Theorem organization policy | 13 passed |
| Shared role-routing contract | 1 passed |
| Jev adapter/observation/abstention checks | 3 passed |
| Native app + canvas_check + site_panel_check build | Passed |
| Theorem server including test code | Checked; server tests not executed |
| Native board/notes/web dictation/public browsing | Passed |
| Fresh-process board/note restore | Passed |
| Companion patches against clean pinned indexes | Both passed |
| Companion reverse-apply checks against working dependencies | Both passed |
| Bootstrap on already-patched dependency | Passed |
| Source diff whitespace check | Passed; patch context blank lines excluded |

The two ignored binary tests were not represented as passing. The pinned ASR
oracle was explicitly selected in a separate invocation and passed; no live
microphone audio was recorded. Live Jev, hosted attachments/model routing,
website Undo and audible playback are not established by these checks.

## Reproducing the native flow

Build the app with `script/media-env cargo build --locked --features snapshot`.
Serve this directory on loopback port 8768:

```sh
python3 -m http.server 8768 --bind 127.0.0.1 --directory docs/plans/evidence/zephium-spatial-2026-10-30
```

In another terminal, choose a new output directory; the harness creates an
isolated profile there. Replace the executable path if Cargo uses another target.

```sh
FRAME_SPATIAL_CHECK=/tmp/platinum-spatial-check \
FRAME_SPATIAL_PUBLIC_URL=https://example.com \
script/media-env target/debug/frame

FRAME_SPATIAL_CHECK=/tmp/platinum-spatial-check \
FRAME_SPATIAL_RESTORE=1 script/media-env target/debug/frame
```

Inspect both report JSON files and `native-focus.json`; an inactive native window
explicitly records that native insertion was not exercised. A public-site text
change can fail its expectation independently of application behavior. Stop the
fixture server after the run. Do not reuse a personal browsing profile.

## PR #17 repair verification

The six review findings are repaired on `claude/platinum-theorem-interface-990ec3`.
This run uses the same Rust/media environment with SSD-backed Cargo targets,
four build jobs and incremental compilation disabled. Cargo's automatic cache
GC is disabled with `--config 'cache.auto-clean-frequency="never"'`; the first
attempt was interrupted while Cargo was doing unrelated global cache cleanup.

| Check | Repair result |
| --- | --- |
| `script/media-env cargo test --locked -p frame --bin frame -- --test-threads=1` | 248 passed, 2 ignored |
| `script/media-env cargo build --locked --features snapshot --bin frame` | Passed |
| Native board, notes, public browsing and supplied-transcript insertion | Passed; `review-report.json` |
| Fresh-process board/note restore | Passed; `review-restore-report.json` |
| Native dictation window active | Confirmed; `review-native-focus.json` |
| `cargo test --locked --manifest-path rustyredcore_THG/Cargo.toml -p agent --no-default-features --lib provider::decision` | 4 passed, including dropped/truncated HTTP replies |
| `cargo test --locked --manifest-path rustyredcore_THG/Cargo.toml -p rustyred-thg-server --lib typed_decisions::tests` | 2 passed: unknown outcome/replay and interrupted fallback receipts |
| Theorem patch clean-index forward/reverse checks and bootstrap | Passed |
| Source whitespace check | Passed, excluding unified patch context lines |
| `cargo fmt -p frame -- --check` | Existing broad formatting differences remain; no unrelated reformat applied |

The five new Platinum regressions cover typing during a disk merge (including
shutdown before UI completion), board composer restoration through tab shortcuts,
closed-tab identity across restart, imported-tab allocation, and twelve queued
visits with only four preview slots. The separate backend checks below exercise
the receipt/effect boundary and actual dropped/truncated loopback HTTP replies.
No live paid provider call is used by these regression checks.

The server test build encountered an internal-volume space error before test
execution. Retrying with the log and temporary/build files on the external SSD
completed successfully (two build jobs for the final retry). Both receipt tests
then executed and passed; they use the real graph receipt path with an injected
provider effect, not a live paid provider or a deployed server.
