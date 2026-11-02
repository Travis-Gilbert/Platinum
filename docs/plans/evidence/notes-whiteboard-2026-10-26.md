# Whiteboard rebase and Frame port

Branch `feat/notes-zorite`, starting from
`4e5422511c59288a829021b5c1bc6a55504c3ac7` (Theorem's fork imported
verbatim). The work lands as two commits: the source-only rebase onto
upstream zorite `6ee46f2`, then the Frame port of the manifest and font
source. The rebase commit still names Theorem's workspace dependencies, so
the crate builds in Frame only from the port commit. All code was compiled
and tested after the port.

## Upstream history

The upstream commits were applied in this order:

1. `7c11533`: bounded dash iteration and regression test.
2. `3586a48`: wheel-line clamp and camera-pan bounds.
3. `077cd9a`: one-pixel minimum strokes.
4. `37cf7b3`, `b44dfa1`, `bba7056`: merges combining those fixes; no distinct
   implementation changes beyond the combinations already applied.
5. `c963c3c`: glyph mesh caching and one path for the dot grid.
6. `b5e9121`: pen-stroke mesh caching and translation on pan.

Each intermediate `lib.rs`, `paint.rs` and `scene.rs` was checked against its
Git blob SHA-1 in the corresponding upstream commit's tree before it was
merged into the fork. `paint.rs` after the rebase equals upstream `6ee46f2`
byte for byte (checked with `cmp` against
`git show 6ee46f2:crates/gpui-whiteboard/src/paint.rs`).

## Conflicts and preserved fork behavior

One textual conflict occurred, in `scene.rs` for 3586a48: upstream bounded
pan in `from_json`; the fork calls a shared `sanitize_camera` from both
`from_json` and recoverable `parse`. Keeping that helper and bounding pan
there preserves the recoverable loader, retained elements and camera errors
while protecting both load paths.

The hosted camera setter is absent upstream. It now calls the same bound,
with `Camera::bound_pan` visible within the crate. It still rejects
non-finite inputs, emits no event, and causes no persistence write. The
mounted camera regression now tests extreme host-supplied pan and these
ownership properties. The upstream pan-load test uses `Scene::parse`, and
the dash sweep starts at the fork's 0.02 minimum zoom; the shared 4.0 maximum
is preserved.

Other hunks merged cleanly. The fork's history, camera-request dispatch,
context-menu seam, retained content and ids, constraints, rotation
capability, mechanics, gesture engine, Space adapter and safety lint remain
intact. No upstream change introduced a new editing command requiring a new
hosted-history route.

## Port and API changes

- Added `vendor/gpui-whiteboard` as a workspace member, with `publish = false`.
- Replaced inherited GPUI dependencies with
  `gpui = { package = "gpui-pre", version = "=0.3.9" }`, retaining the
  dev-only `test-support` feature and Frame's existing patches.
- No GPUI Rust API changes were needed. The crate does not use gpui-component,
  so there was no component API migration. GPUI Kit stays at `=0.8.0`.
- Retained default-enabled, optional `space`, pointing it at
  `deps/theorem-rustyweb-pw2/rustyredcore_THG/crates/theorem-space` with default
  features disabled. No `canonical` feature was introduced.
- Removed `theorem-design-gpui`; restored upstream 6ee46f2's exact JetBrains
  Mono Regular asset and OFL text. Copied the MIT design crate's native CJK
  candidate search into `font.rs`, keeping its order, face validation,
  cached bytes, wasm32 stub, and provider -> system CJK -> bundled fallback.
- Strict clippy found ten existing test-only lints: nine clones of Copy
  geometry and one complex callback-log annotation. Removed the clones and
  used type inference for the log. No production refactor or lint suppression.

FORK.md records the origin, all retained divergences, conflict resolutions,
port changes and validation procedure. The MIT license, font asset and OFL
text compare byte-for-byte equal to upstream; the font's SHA-256 is
`a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f`.

## Validation

All commands ran offline in this worktree on 2026-10-26.

| Command | Result |
| --- | --- |
| `cargo build --offline -p gpui-whiteboard` | Exit 0. |
| `cargo test --offline -p gpui-whiteboard` | 130 passed, none failed, ignored or filtered. Includes 23 Space tests, mounted hosted behavior, font and provider tests and the upstream mesh and dash regressions. |
| `cargo test --offline -p gpui-whiteboard --no-default-features` | 107 passed, none failed, ignored or filtered. |
| `cargo test --offline -p zorite-editor -p zorite-markdown -p gpui-bidi` | 68 + 57 + 19 = 144 passed, none ignored. |
| `cargo clippy --offline -p gpui-whiteboard --all-targets --all-features -- -D warnings` | Exit 0 after the ten test-only lint fixes above. |
| `cargo tree --offline -d --workspace` | Includes the new member; no duplicate gpui-pre family, gpui-kit or gpui-component package. |
| `git diff --check` | Passed. |

The unique GPUI package, version and feature lines from
`cargo tree --offline --workspace --prefix none --format '{p} {f}'` match
before and after. Cargo.lock adds only gpui-whiteboard and its dependency
references; every existing package entry is unchanged. Default features on
the other three crates and Frame's own dependencies were not edited.

## Not verified

- The old Theorem graph was not built.
- Native Metal pixels and real-window behavior were not tested. Mounted GPUI
  tests exercise headless render and input dispatch, not a real GPU.
- Frame and Servo were not built for this scoped port.
- The CJK candidates are fixed paths. On this macOS (Darwin 27),
  `/System/Library/Fonts/PingFang.ttc` does not exist; the first candidate,
  `/Library/Fonts/Arial Unicode.ttf`, does. When the canvas is wired into
  Frame, Frame should supply its CJK face through `set_font_provider`, found
  through its existing font-kit, rather than relying on these paths.
