# gpui-pre-apple 0.3.9, carried by the desktop host

This is the published `gpui-pre-apple` 0.3.9 with a BGRA path in its Metal
renderer, applied through `[patch.crates-io]` in `desktop/Cargo.toml`. It is
not offered upstream (plan D2). Every change is marked `THEOREM PATCH`.

Why: `gpui::surface()` paints a `CVPixelBuffer` inside GPUI's scene, but the
renderer asserted that the buffer is `420YpCbCr8BiPlanarFullRange`, the format
of screen capture. Servo's frames are `IOSurface`s in `32BGRA`, written by
OpenGL, so the page could not be drawn as an element.

The changes:

- `src/shaders.metal`: `surface_bgra_fragment` samples one BGRA texture. It
  flips the rows, because OpenGL writes the bottom row first, and divides the
  premultiplied colors by alpha, because the surface pipeline blends straight
  alpha.
- `src/metal_renderer.rs`:
  - a `surfaces_bgra` pipeline with that fragment and GPUI's `surface_vertex`;
  - `draw_surfaces` picks the pipeline per surface by pixel format, and keeps
    asserting on any format that is neither;
  - texture sizes are passed to `plane_texture`, since a buffer with one plane
    reports no plane sizes;
  - the test `render_bgra_surface`.

A BGRA buffer is assumed to be in OpenGL's row order. Nothing else in the app
paints BGRA surfaces, and a top-down producer would need its own path.

Re-applying it on a GPUI Kit upgrade: copy the new `gpui-pre-apple` from the
registry, carry the marked changes, and run, in `desktop/`:

```bash
cargo test -p gpui-pre-apple --lib render_bgra_surface
```
