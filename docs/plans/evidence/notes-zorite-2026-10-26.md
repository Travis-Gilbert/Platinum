# Zorite fork port into Frame

Branch: `feat/notes-zorite`, 2026-10-26. Upstream:
`packetThrower/zorite@6ee46f28cbe0017f7f152033227d897434ddd0ef`.
Frame base: `e8a21b13001e07f9b61e0be6827d1a83929bcb9a`.
This record covers only the three-crate fork in foundation step 1.

## Dependency graph

`cargo tree --offline -d` passed before the import. After adding the crates,
`cargo tree --offline -d --workspace` reports no duplicate gpui-pre family,
gpui-kit or gpui-component package. Comparing the unique package/version/
feature lines of `cargo tree --offline --format '{p} {f}' --prefix none`
before the import with `--workspace` afterward shows no GPUI changes.
Frame stays on GPUI Kit/component 0.8.0 and gpui-pre 0.3.9. Cargo.lock adds
only the three local packages; no existing package entry changes.

All upstream tests are retained. The standalone editor demo source is also
retained, but its target and dev-only platform/spellchecker dependencies are
disabled: `os-spellcheck` is a fourth crate outside this fork. No dependency
on the forks is added to the Frame binary.

## Combining marks

Zorite's `vendor/gpui-pre-wgpu/ZORITE-PATCH.md` identifies a cosmic-text bug:
`layout_line` omitted glyph x/y offsets on Linux/web. Frame's macOS backend
selects `gpui_macos::MacPlatform` (`gpui-pre-platform` 0.3.9,
`src/gpui_platform.rs:68`), which creates `gpui_apple::AppleTextSystem`
(`gpui-pre-macos` 0.3.9, `src/platform.rs:234`) with the already-enabled
`font-kit` feature. In Frame's `vendor/gpui-pre-apple/src/text_system.rs`,
`layout_line` uses Core Text `CTLine`/glyph runs and copies both coordinates
from `run.positions()` into each `ShapedGlyph` (lines 583-629).

`gpui-pre-macos/src/gpui_macos.rs:17` selects `gpui_apple::metal_renderer`.
The cosmic-text patch does not apply to this Apple/Metal path and is not
added. This is a source-level backend finding, not visual proof of every
combining mark. The new headless test checks exact Unicode preservation
through editing; it does not validate Core Text shaping or Metal pixels.

## API changes

None in production Rust. All three libraries and all existing tests compile
on gpui-pre 0.3.9 unchanged. Manifest changes replace inherited GPUI specs
with the exact Frame version under the existing `gpui` alias. GPUI Kit stays
at `=0.8.0`; none of the three crates consumes its component API.

## Validation

| Command | Result |
| --- | --- |
| `cargo build --offline -p zorite-editor -p zorite-markdown -p gpui-bidi` | Exit 0 on the initial build and repeat; repeat finished in 2.87s. |
| `cargo test --offline -p zorite-editor -p zorite-markdown -p gpui-bidi` | First attempt exited 101 while compiling the newly added test; corrected test setup and reran: exit 0, 144 tests passed, none failed/ignored/filtered. |
| `cargo clippy --offline -p zorite-editor -p zorite-markdown -p gpui-bidi` | Exit 0, no warnings; finished in 1m 31s. |
| `cargo tree --offline -d` | Exit 0 before and after; no duplicate GPUI family, kit or component. |
| `cargo tree --offline -d --workspace` | Exit 0; also checks all three forks, no duplicate GPUI packages. |
| `git diff --check` | Passed. |
| `rustfmt --check --edition 2024 vendor/zorite-editor/src/markdown_syntax.rs` | Passed. |

Test totals: gpui-bidi 19, zorite-editor 68 (67 upstream plus the new headless
test), zorite-markdown 57. All three doc-test targets ran with zero tests.
The successful test command finished compilation in 25.02s; the test suites
reported 0.00s, 0.01s and 0.02s respectively.

The first test compilation reported E0277/E0599 because the new test passed
an Entity to `TestAppContext::open_window`, whose closure returns a Render
value. The correction uses the typed application `open_window` through
`cx.update`, as the upstream GPUI tests do, then attaches a
`VisualTestContext` for input. This was test setup, not an upstream API break.

Before/after trees, feature lists, `graph-check.txt`, `build.log`, `test.log`
(the failed attempt), `test-retry.log` (passing), and `clippy.log` are under
`target/notes-port-evidence/` (ignored build output). All existing lockfile
package entries compare equal to the base; only the three local packages
are added. All 24 upstream files are retained, and all three LICENSE files
are byte-for-byte identical to upstream. Only manifests and the added test
change upstream files; each fork also adds THEOREM-PATCH.md.

The original demo target is disabled as described above. The Frame binary
and Servo were not built, per the task scope. Native Apple/Metal rendering
was not visually tested. No existing GPUI vendor source or shared dependency
checkout was edited. No missing cached crate or Cargo sandbox failure was
encountered.
