# gpui-pre 0.3.9, carried by Frame

This is the published `gpui-pre` 0.3.9 with its `surface()` element's
rounded corners carried to the renderer, applied through `[patch.crates-io]`
in `Cargo.toml`. Every change is marked `THEOREM PATCH`. The crate's
`examples/` are left out, and with them the `[[example]]` targets in
`Cargo.toml`.

Why: a page is a `surface()` inside a rounded card. With Liquid Glass under
the window's frame (`src/glass.rs`), the card's corners can no longer be
masked by painting the frame's color over the page, since that color is now
partly clear. `Surface` already took a style, corner radii included, but its
`paint` ignored them (upstream's `TODO: Add support for corner_radii`).

The changes:

- `src/scene.rs`: `PaintSurface::corner_radii`.
- `src/window.rs`: `Window::paint_surface` takes the corner radii, as
  `paint_quad` does, and scales them.
- `src/elements/surface.rs`: `Surface::paint` passes its style's corner radii,
  clamped to its size, as an image's are.

The renderer draws them with `vendor/gpui-pre-apple`'s matching patch.

Local dictation additionally uses `Window::with_focused_text_input` and the
private `PlatformInputHandler::edit` bridge. They validate the captured focus
before calling the mounted input's existing IME methods. This avoids synthetic
keystrokes, clipboard mutation, and a separate insertion implementation per
native editor. The caller owns destination invalidation on navigation and
user input; no method submits the field. Carry these marked methods forward
when updating GPUI and test stale focus and selection with dictation.

Re-applying it on a GPUI Kit upgrade: copy the new `gpui-pre` from the
registry without `examples/`, drop its `[[example]]` targets, carry the
marked changes, and run:

```bash
cargo test -p gpui-pre-apple --lib render_
```
