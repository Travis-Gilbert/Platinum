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
