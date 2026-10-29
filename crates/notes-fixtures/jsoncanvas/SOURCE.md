# Source

`sample.canvas` is copied byte for byte from
[obsidianmd/jsoncanvas](https://github.com/obsidianmd/jsoncanvas), the reference repository of the
JSON Canvas format, which is MIT licensed. `LICENSE` is that repository's licence file, copied
unchanged.

- Commit: `456f843cb293df4f4ab1763e22ccb46a80b307c8` (2026-07-24)
- Path in that commit: `sample.canvas`
- It is the only `.canvas` file in the repository. The spec (`spec/1.0.md`) and the apps list are
  prose, so they are not copied.
- Its `file` nodes name files from that repository (`readme.md`, `_site/logo.svg`) that are not
  here, so the canvas parses but its file cards do not resolve inside this crate.

The canvases that exercise Frame's own parity rows are hand-written in `../vault/canvases/`.

Obsidian's help vault (obsidian-help) carries no licence. It is never committed here; see the
crate README for how to point the reference harness at an uncommitted local clone.
