# zorite-editor 0.11.2, carried by Frame

Forked from `packetThrower/zorite@6ee46f28cbe0017f7f152033227d897434ddd0ef`.
This crate's manifest and LICENSE are MIT; its upstream copyright notices
and license are retained. The GPL application is not included.

Why: Frame's Markdown editor uses the zorite crates on Frame's existing
GPUI Kit 0.8.0 / gpui-pre 0.3.9 graph.

The changes:

- `Cargo.toml`: disable publishing and replace the inherited `gpui`
  dependency with `gpui-pre = "=0.3.9"`, retaining the `gpui` import name
  and using Frame's existing workspace patches.
- `Cargo.toml`: keep `test-support` on the GPUI dev-dependency. Disable
  automatic examples and remove the explicit demo target and its
  `gpui_platform` / `os-spellcheck` dev-dependencies. The unmodified demo
  source is retained as reference; its OS spellchecker is outside this
  three-crate fork. No platform features are added to Frame's graph.
- `src/markdown_syntax.rs`: add a headless GPUI test using the existing
  Markdown test style. Open a note with a heading, list, wikilink, emoji,
  CJK and a combining mark, type via GPUI's input path, and read back the
  exact text.

No production Rust API changes were required by the gpui-pre 0.3.9 build.
No gpui-component/GPUI Kit API is consumed by this crate. Frame's existing
GPUI patches are unchanged; zorite's WGPU combining-mark patch is not used
on Frame's Apple text/Metal path.

Re-applying on an upstream upgrade: copy this MIT crate, carry the manifest
changes and any test additions above, and run Frame's focused offline
build, tests and clippy for all three forks. Compare the workspace GPUI
versions and features and the lockfile against the pre-upgrade graph.
