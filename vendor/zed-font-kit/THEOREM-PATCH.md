# zed-font-kit 0.14.1-zed, carried by the desktop host

This is the published `zed-font-kit` 0.14.1-zed with two version requirements
raised, applied through `[patch.crates-io]` in `desktop/Cargo.toml`:

- `freetype` 0.7 -> 0.8
- `freetype-sys` 0.20 -> 0.23

Why: `freetype-sys` declares `links = "freetype"`, and Cargo allows one such
package per graph across every target. GPUI reaches `freetype-sys` 0.20
through this crate. Servo's `servo-fonts` requires 0.23. Neither side links
FreeType on macOS, but the resolver refuses the graph anyway.

`open-gpui-font-kit` 0.14.3 made the same move. Its FreeType loader changes
(bitmap sizes per pixel format) are not taken here, because macOS never
compiles that loader. Take them before building the host on Linux.
