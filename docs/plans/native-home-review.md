# Native Home and SERP: PR #4 review

Review date: 2026-10-29. Original head: `40a50b8fd62a13b19223cba30d21aff8a1d2dc3c`.
Integrated main: `37da98b5a800f8241ace79b448a201e3423d12cc`.
Scope follows step 1 of [the native workspace vision](../vision/native-workspace.md).

## Spec findings and fixes

- Record visits on `PageEvent::Visited`; title updates preserve timestamps and
  cannot restore a cleared Continue entry.
- Show search-wide notes and provider degradation messages with populated and
  empty results. Local provenance says pages were kept locally, without claiming
  the user visited automatically fetched pages.
- Preserve Command-click and Command-Return search destinations on workspace
  Home. Consume the backing tab when it is the new destination.
- Downloading a workspace result preserves its route and Back history.
- Opening a new tab preserves existing workspace results in their own tab.

## Standards findings and fixes

- `CON-003`, `DOD-007`: replace recursive external HTML traversal with explicit
  stacks; bound preview input to 64 KiB and favicon discovery to 512 KiB plus a
  five-second request/body deadline. Only HTTP(S) declared icon URLs are used.
  html5ever remains the HTML parser; no replacement parser was introduced.
- `CON-004`, `DATA-002`: serialize recents writes and reject older snapshots,
  using the existing session store's generation/mutex pattern. Flush the latest
  snapshot on quit. The record contains at most eight pages and eight searches.
- `UI-002`: preserve degraded search notices, including the empty state.

## Main integration

Keep main's profile import, tab tree, pins, session restoration, spatial canvas,
search concurrency and dependency pins. Recents live in the selected profile;
legacy application-level recents migrate only to the default profile. Empty
workspace Home does not restore a phantom tab; workspace search queries remain
in the saved session. Placeholder places remain explicitly unavailable.

## Validation

The commands below use the repository Rust 1.97.1 toolchain on macOS arm64.
`/Volumes/SSD Samsung/cargo-targets/frame-canvas.sh` serializes access to the
existing build cache, refreshes first-party source timestamps and invokes
`script/media-env`. It was used for the Cargo commands.

- `cargo test --locked --offline --bin frame`: 182 passed, 0 failed, 1 ignored.
  The ignored test is the existing explicit model-download/inference check.
- `cargo build --locked --offline --features snapshot --bin frame --example serp_check`:
  built successfully. Existing dependency and unused-code warnings remain.
- `python3 script/check-pins.py --require-deps --json`: passed; exact dependency
  identities are in [pins.json](evidence/native-home-pr4-2026-10-29/pins.json).
- `git diff --check origin/main`: passed.
- Scoped `rustfmt --edition 2024 --config skip_children=true --check` on changed
  Rust files: passed. The repository-wide formatting command also reports
  pre-existing formatting in untouched main files; those were not reformatted.
- Native visual/navigation acceptance: passed (exit 0). Inspected workspace Home,
  tab Home, populated light/dark results, empty results with notices, and the Servo
  destination. Back returned to the native results view. See the
  [native report](evidence/native-home-pr4-2026-10-29/report.json) and adjacent PNGs.

The native harness is `FRAME_WORKSPACE_CHECK=<output> script/media-env <frame>`
with the `snapshot` feature. It draws production Home/SERP in light and dark,
uses labeled search fixtures, navigates a real Servo page and returns to native
results with Back. It uses no real browsing profile or search credentials.
Screenshots contain GPUI's rendered layer; native glass behind that layer is not
included. This is not live-provider, model, media, or release-bundle acceptance.

The local cache had two dangling `libgstmse.dylib` links to a removed worktree.
They were repaired to the existing pinned runtime before native validation;
no dependency source or pin was changed.
