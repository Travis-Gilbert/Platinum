# R1: a viewport that holds still

Uncommitted implementation in `feat/viewport-css-size` (Servo, base `2b522612c4d`)
and `feat/canvas-viewport` (Frame, base `89264e4`). The orchestrator owns review,
commits, and native acceptance. Frame's existing Cargo path to `deps/servo-r1`
is preserved.

## Contract and implementation

```rust
pub fn set_viewport(
    &self,
    css_size: Size2D<f32, CSSPixel>,
    hidpi_scale_factor: Scale<f32, DeviceIndependentPixel, DevicePixel>,
)
```

The new call supplies an explicit CSS layout size and raster scale together.
The renderer stores both before sending one `ChangeViewportDetails` message;
the surface is `ceil(css_size * scale)`, at least one physical pixel per axis.
The CSS size is never recovered from those rounded dimensions. Repeated identical
updates are no-ops, and updates still work when the rounded device size stays equal.
Legacy `resize` and `set_hidpi_scale_factor` restore device-derived sizing.
Explicit sizing takes precedence over page zoom and viewport-meta initial scale.
Visual-viewport size also uses the explicit CSS size divided by pinch zoom,
so fractional surface padding cannot cause a visual resize on a DPR change.
No new dependency is needed: this extends the existing Servo viewport/painter
owners and uses euclid for typed size arithmetic.

Read [CSSOM View resizing viewports](https://drafts.csswg.org/cssom-view/#resizing-viewports)
on 2026-10-27. Its window event criterion is a change in viewport width or height
since the previous resize steps. Servo still applies the entire viewport record
atomically in `Window::add_resize_event`. At resize steps it compares CSS `size`
for event dispatch; it retains the full-record change result for media-query and
image-environment updates. This preserves DPR/resolution updates without a window
resize for scale alone. Coalesced updates compare against the last resize steps,
not against the previous incoming message.

Frame caches CSS size rather than rounded raster size. Canvas uses card world
size plus the uniformly capped settled scale; Tabs uses bounds plus display
scale. Nonpositive bounds are skipped. The probe retains every resize event's
size and DPR and records animation-frame samples between baseline and completion.
`zoom.json` includes the entire session trace. Every gesture/settled sample and
trace entry must keep the baseline inner size, and no session resize may occur.
The obsolete `expected_blocker` field is removed.

## Files changed

Servo checkout:

- `components/servo/webview.rs`: typed public API and legacy setter forwarding.
- `components/paint/paint.rs`: route the atomic update and legacy resize.
- `components/paint/painter.rs`: round and resize the device surface; preserve shared-context behavior.
- `components/paint/webview_renderer.rs`: retain explicit CSS size and publish complete viewport updates.
- `components/script/dom/window/window.rs`: dispatch window resize only for CSS dimension changes.
- `components/servo/tests/webview.rs`: scale-only, CSS-size, and legacy compatibility integration tests.

Frame checkout:

- `src/page.rs`: use the new call and cache CSS size, including the settle path.
- `src/canvas/surface.rs`: current cap documentation and an asymmetric-cap test.
- `tests/canvas/probe.js`: complete resize/animation-frame trace.
- `examples/canvas_check.rs`: enforce every sample/trace entry and retain the trace in `zoom.json`.
- This evidence directory: receipts, logs, dry-run plan and retained probe regression script.

`Cargo.toml` was already modified for the lane; its content is unchanged by this work.
No dependency manifests or lockfiles were changed in Servo. Nothing under Frame's
`deps/` or `vendor/` was content-edited. Both original checkouts remain in place
with the changes uncommitted for the orchestrator.
