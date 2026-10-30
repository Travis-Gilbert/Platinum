# FORK.md: gpui-whiteboard 0.5.1, Theorem hard fork

## Origin

- Upstream: https://github.com/packetThrower/zorite, directory `crates/gpui-whiteboard`
- Revision: `6ee46f28cbe0017f7f152033227d897434ddd0ef` (crate version 0.5.1, edition 2024)
- Initially based on upstream `4aad847f57a5ac3e8ec0b6220e25d9db7e5264b4`, imported verbatim in `1683bb450 chore(theoremweb): vendor zorite gpui-pdf and gpui-whiteboard as hard forks`. Frame imported Theorem `6e57dbb39` in `4e54225` and rebased it to the revision above. Every retained difference from that upstream revision is listed below; anything not listed is upstream.

## Licenses

- The crate is MIT (`LICENSE`, unchanged). Keep the upstream copyright line; add nothing that narrows it.
- The default face, JetBrains Mono Regular, is under the SIL Open Font License 1.1 (`assets/JetBrainsMono-OFL.txt`, unchanged). Frame restores upstream 6ee46f2's exact `assets/JetBrainsMono-Regular.ttf` and OFL text. Theorem's shared design-crate font source is not a Frame dependency; `Font::bundled` reads this asset instead. The OFL text ships beside the face.
- `assets/icons/LICENSE` covers the toolbar icons, unchanged.

## Why this fork exists

Theorem's spatial canvas uses the whiteboard for **mechanics only**: drawing tools, hit-testing, marquee, z-order, thumbnails, the editing UI. Theorem's Space and Placement model is the truth, and Theorem owns the one gesture, camera, hit-test and undo owner. The fork therefore has to be hostable (the host can take history, camera and the context menu) and must never silently destroy content (a board the loader cannot fully read has to survive a save). **Upstream standalone behavior is the default** in every case below: a host that changes nothing gets the upstream crate.

## Divergences

1. **Recoverable parse** (`src/scene.rs`). New `Scene::parse(&str) -> Result<ParsedScene, SceneParseError>`. `ParsedScene { scene, retained: Vec<RetainedElement>, camera_error }`; `RetainedElement { index, raw: serde_json::Value, error, source }`. An element that does not deserialize (unknown `kind`, bad geometry) is kept verbatim instead of dropped. A whole-document syntax error returns `Err(SceneParseError::Syntax { .. })` carrying the original text, and valid JSON that is not a scene returns `Err(SceneParseError::Shape { .. })` likewise, so the host can keep the text. `Scene::to_json_with_retained` writes retained elements back at their original positions and in order. The retained bytes are written exactly as read (a small span scanner, `element_spans`, because `serde_json::Value` would re-sort keys and reformat numbers); the readable elements are re-serialized as before. *Reason:* upstream `from_json` turned any parse failure into an empty board, and the next autosave overwrote the saved board with it.
2. **`Scene::from_json` deprecated** (`src/scene.rs`). Behavior unchanged (lossy, blank board on failure) and kept for hosts that want it; marked `#[deprecated]` and documented. Internal callers use `parse`. *Reason:* point new code at the non-destructive path without breaking a standalone host.
3. **Retained elements are surfaced** (`src/hosted.rs`, `src/lib.rs`). `WhiteboardView::from_parsed(ParsedScene, style, cx)`, `retained()`, and `to_json()`. `on_change` is handed the retained-aware JSON, new element ids are reserved above every retained id, and a HUD line ("N unreadable elements kept, not shown") is drawn when `retained` is non-empty, in both the editable and read-only views. The HUD line carries a `debug_selector` of its own text (a no-op outside test builds), so a test reads exactly what is drawn. *Reason:* keeping content the user cannot see is only safe if the user is told, and an id collision with a held element would corrupt it on the next load.
4. **Pure mechanics module** (`src/mechanics.rs`, public). Topmost-first `hit_test` / `hit_test_where` / `hit_test_all`; `marquee_select` with `MarqueeMode::{Intersecting, Contained}`; `ZOrder` and `reorder` / `reorder_by` / `reorder_elements` returning the new order; `element_bounds`, `selection_bounds`, `scene_bounds`, `union_rect`; `fit_thumbnail` / `fit_scene_thumbnail` returning a `ThumbnailTransform`; `RotationSupport`, `rotation_support`, `supports_rotation`. Reuses `geometry.rs`. `lib.rs` now calls these where it previously inlined the same logic (`element_bbox`, `selection_bbox`, `text_at`, `shape_at`, the two hit-test sites, the marquee finish, `reorder_selection`); the private `enum ZOrder` moved here and became public. `scene_bbox_for_local_thumbnail` delegates to `mechanics::scene_bounds`. *Reason:* the host's canvas must run the same hit-test, marquee and z-order the view does, from outside a window. A reorder that would change nothing is decided before a snapshot is taken, so a no-op no longer pushes an undo entry.
5. **Hosted history** (`src/hosted.rs`, `src/lib.rs`). `HistoryMode::{Local (default), Hosted}`, `set_history_mode`, `set_on_commit`, `apply_external`, and `WhiteboardEvent::{Commit { before, after, label }, UndoRequested, RedoRequested, CameraRequested}`. Hosted mode keeps **no** local stack. `push_undo(label)` (now labeled at all 18 call sites) takes a lazy "before" snapshot (first one wins); the Commit is emitted once the gesture has ended and only if the elements actually changed. A pen stroke, drag, resize, rotate, marquee or text edit therefore emits nothing until it completes, and a click that changes nothing emits nothing. Undo and redo keys emit `UndoRequested` / `RedoRequested` and change nothing. `apply_external(scene)` replaces the elements without a Commit and without `on_change`, cancels in-flight gestures, and leaves the camera alone. Events reach both the `set_on_commit` hook (every event, including requests) and `cx.subscribe`. The host-called `add_*` APIs, which never flush, settle themselves. *Reason:* one undo owner. A view that keeps its own stack beside the host's would undo the same edit twice or diverge.
6. **Context-menu seam** (`src/hosted.rs`, `src/lib.rs`). `set_on_context_menu(Fn([f32; 2], Option<u64>, Point<Pixels>, &mut Window, &mut App))` receives the scene point, the topmost element under it, and the window position, and suppresses the built-in menu. A right-click inside the open color picker still belongs to the picker. *Reason:* the host owns commands and menus.
7. **Camera seam** (`src/hosted.rs`, `src/lib.rs`, `src/scene.rs`). `CameraMode::{Local (default), Hosted}`, `set_camera_mode`, `camera()`, `set_camera(Camera)`, `CameraIntent::{Pan, ZoomAbout, Reset}`. In Hosted mode wheel, pinch, pan-drag and reset emit `CameraRequested { camera, intent }` (the camera the view would have used) and change nothing; the host answers with `set_camera`, which clamps zoom and pan to the interactive ranges, refuses non-finite values, and does not mark the board dirty (the host owns the camera). `Camera::{screen_to_world, pan_by, zoom_about}` are now `pub`. *Reason:* one camera owner, so the host's spatial canvas and the board never disagree about where the viewport is.
8. **Font seam** (`src/font.rs`). `Font::default()` is now: registered provider, then (native only) the system CJK face, then the bundled JetBrains Mono. `set_font_provider(Option<impl Fn() -> Option<Font> + Send + Sync + 'static>)` installs or removes the provider; `Font::bundled()` exposes the cached bundled face. The `std::fs` search for a system CJK face is behind `#[cfg(not(target_arch = "wasm32"))]` with a wasm32 stub that returns `None`. *Reason:* upstream read font files from disk on every target, which cannot work in the browser build and must not panic there.
9. **Rotation capability** (`src/mechanics.rs`). `supports_rotation(&ElementKind) -> bool` and the finer `rotation_support` (`Free`, `Baked`, `QuarterTurns`, `None`) come from an exhaustive `match` with no wildcard, so a new `ElementKind` fails to compile until classified, and a test rotates every kind and compares the result with the declared capability. `supports_rotation` is true for `Free` (box shapes, text) and `Baked` (line, arrow, draw: the points are rotated, no angle is stored). Images snap to quarter turns (`QuarterTurns`) and embeds do not rotate (`None`); both report false. Hit-testing a rotated shape uses the axis-aligned box of its rotated bounds, which is conservative (it can accept a click just outside a rotated corner). *Reason:* a host offering a rotate command must not offer it for something that will not rotate.
10. **Model changes** (`src/scene.rs`). `PartialEq` derived on the model types so a Commit can be skipped when nothing changed; `TextGeom` compares by hand and ignores the cached `measured_w` / `measured_h`. Wire format is unchanged.
11. **Tests** (`src/hosted_tests.rs`, plus `mechanics`, `scene`, `font` test modules; one in `lib.rs` for `Template`). `Cargo.toml` gains a dev-dependency feature, `gpui = { package = "gpui-pre", version = "=0.3.9", features = ["test-support"] }`, on the existing `gpui` edge: no new package, `Cargo.lock` unchanged. The view tests drive the mounted view through GPUI's real event dispatch (`VisualTestContext::simulate_*`), not by calling handlers directly.
12. **`Template::from_json` is per-element tolerant** (`src/lib.rs`). One unreadable element is logged (`log::warn`) and skipped instead of emptying the whole template. The host still owns the stored row.
13. **Constraint seam** (`src/hosted.rs`, `src/lib.rs`). `set_constraints(held, axis_aligned)`, `is_held`, `is_axis_aligned`. A held element is never moved by a gesture: a selection drag moves only its unheld members, and a resize, endpoint or rotate handle is not offered while a held element is selected. An axis-aligned element is never offered rotation (the rotate button and handle, alone or in a group). Both sets are empty by default. *Reason:* Canvas 2.5 7.6, pinned geometry is a hard constraint and live foreign surfaces are axis-aligned only; the board must not offer a gesture the host will refuse.
14. **Embeds take connectors** (`src/geometry.rs`). `connector_capable` includes `Embed`, and `connector_points` returns its four edge midpoints (top, right, bottom, left; an embed never rotates). *Reason:* a Space's cards are drawn as embeds, and a connection drawn between two cards has to anchor to them.
15. **Space adapter** (`src/space.rs`, `src/space_tests.rs`, feature `space`, on by default; path dependency on `deps/theorem-search/rustyredcore_THG/crates/theorem-space` with its `canonical` feature off, so it stays wasm-safe). `SpaceBoard` projects a `theorem_space::Space` into a `Scene` (groups as regions, object/note/portal placements as embed cards, `PlacementTarget::Ink` content expanded element by element, the three connection classes as differently styled arrows) with element ids that stay stable across projections and undo, and converts each hosted `Commit` back into at most one Space-history `SpaceCommand` plus owner commands (`Unrelate`, `Unbind`) for relationships and bindings. `route` maps undo/redo to the Space's own and a camera request to device-local view state with no command; `apply` executes a route once; `hit` and `open` answer with Space identities; `import_board` turns a legacy board document into a `PlaceInk` through the recoverable parse; `WhiteboardView::present` installs a projection (hosted history and camera, constraints, id floor). Ink unknown to this build (an unreadable element or an unknown format) is kept verbatim and written back unchanged. *Reason:* Canvas 2.5 8.2, the board is the Space's mechanical engine and never a second store. Build without it with `--no-default-features`.
16. **Gesture engine without a view** (`src/engine.rs`, `src/engine_tests.rs`, public, no feature). `Engine` holds one presented scene with a `Role` per element (`Card { connectable }`, `Region { members }`, `Inert`) and the held set, and answers `hit` (topmost card, else the smallest region), `hover` (corner and side handles of selected and hovered cards first), `press` / `drag_to` / `release` over a host-kept `Option<Gesture>` (move, region move carrying the stored members, resize, marquee wholly-inside, side-dot connect, secondary press resolving on release to a context request or a pan, middle / alt / pan-key pan), `wheel`, `zoom_step`, `zoom_reset`, `pan_step`, `nudge`, and the frame previews (`preview`, `marquee`, `pending_connection`). Output is `EngineEvent`: a finished gesture is exactly one `Commit(WhiteboardEvent::Commit)`, camera moves are `Camera(View)` in `f64` and never a commit, and a gesture that includes a held element moves nothing and reports `Held` (all or nothing). *Reason:* Canvas 2.5 8.2 and review C20: a host that paints its own live surfaces (the Space canvas) needs the board's mechanics without its view, so one engine owns each gesture, camera move, hit test and gesture commit.
17. **One zoom range** (`src/engine.rs`, `src/lib.rs`). `engine::{MIN_ZOOM, MAX_ZOOM}` are 0.02 and 4.0, equal to `theorem_space::geom`; the view's `f32` `MIN_ZOOM` / `MAX_ZOOM` (now `pub`) are derived from them. Upstream was 0.1 to 8. Tests hold the three in agreement. *Reason:* Canvas 2.5 decision (docs/records/2026-10-04-canvas-2-5-implementation.md): a board and the Space it presents must not disagree about the camera.
18. **Space adapter for the engine** (`src/space.rs`). `Projection::roles` and `Projection::present(&mut Engine)`; `SpaceBoard::project_cards` projects every placement as one card (ink included) for a host that paints cards; `view_of` / `space_view` convert the device-local camera at full precision; a compound gesture compiles to `SpaceCommand::Gesture` (one transaction, one undo entry, refused for agents) instead of `ApplyProposal`; a placement turn declared in the Space (`Space::rotations`, Free ink only) is shown about the frame centre and taken off again before content is stored. `unreadable(&InkContent)` lists what a drawing keeps without showing (each unread element's index and error, or the whole drawing in an unknown format), so a host that paints its own cards can say so on the card.
19. **Safety lint** (`Cargo.toml`). `[lints.rust] unsafe_code = "forbid"`; the crate has no `unsafe` block. *Reason:* keep it that way across a rebase: an upstream change that adds `unsafe` fails to compile until it is reviewed.

## Known limits (not fixed here)

- Unknown **top-level** fields, and unknown fields inside an element that otherwise parses, are dropped when the board is re-saved. Only an element that fails to parse as a whole is retained verbatim.
- Hit-testing a rotated shape is the axis-aligned bound of the rotated box (see 9).
- In Hosted mode a text edit on an *existing* text element now produces a Commit (label `edit text`). Upstream Local mode never made that undoable.
- `apply_external` does not touch the camera; the host sets it with `set_camera`.
- A hosted event is delivered twice by design, to the `set_on_commit` hook and to `cx.subscribe` listeners. Use one of them, not both.
- Space adapter: a style change on a card or region (color, stroke) is reported as ignored and snaps back; style is set through the Space. A new embed drawn on the board is ignored (objects are placed through the Space).
- Space adapter: the board's element geometry is `f32`. A move read back from a commit keeps every untouched component at the Space's `f64` precision and adds the `f32` delta, so a move far from the origin can carry a sub-thousandth rounding in the moved coordinate. The camera never passes through `f32` on the engine path (`view_of` / `space_view`).
- Space adapter: a content edit that changes the bounds of a turned drawing re-frames it, so the turn is then shown about the new frame centre.
- Space adapter: reordering elements inside one drawing does not change the stored element order.

## Frame rebase to 6ee46f2

Applied in this order, preserving all 19 divergences above:

1. `7c11533`: bounded dash iteration and its zoom-sweep regression, first.
2. `3586a48`: wheel-line limits and bounded camera pan.
3. `077cd9a`: minimum one-screen-pixel strokes.
4. `37cf7b3`, `b44dfa1`, `bba7056`: merge commits combining the same dash,
   pan and stroke fixes; their combined result is retained without applying
   those fixes twice.
5. `c963c3c`: cached glyph tessellation and a single dot-grid path.
6. `b5e9121`: cached pen-stroke tessellation, translated on pan.

Each intermediate `lib.rs` and `paint.rs` was checked against its Git blob
hash in the corresponding upstream commit's tree before it was applied.

Conflicts and adaptations:

- `3586a48`, `src/scene.rs`: upstream bounded pan inside `from_json`; the fork
  shares `sanitize_camera` between `from_json` and recoverable `parse`. Kept
  that shared path and added `bound_pan` there, preserving retained elements
  and camera errors while protecting both loaders.
- The hosted `set_camera` is another entry point absent upstream. Made
  `Camera::bound_pan` crate-visible and call it from that setter too, retaining
  non-finite refusal and the rule that host camera updates emit no event or
  persistence write. Extended the mounted camera test to cover extreme pan.
- Upstream's new camera-load regression uses the fork's recoverable `parse`,
  not deprecated `from_json`. The dash sweep starts at the fork's `MIN_ZOOM`
  (0.02) and keeps its shared maximum (4.0).
- All other upstream hunks merged cleanly, including cache fields and render
  paths. `paint.rs` matches upstream 6ee46f2 byte for byte. The fork's history,
  camera requests, constraints, id retention, mechanics, engine, Space
  adapter, font seam and unsafe-code prohibition remain in place.

The rebase is its own commit. It still names Theorem's workspace
dependencies, so the crate builds in Frame only from the port commit after it.

## Frame port

- `Cargo.toml`: `publish = false`; replace the inherited Theorem `gpui` specs
  with `gpui-pre = "=0.3.9"` under the same `gpui` import name, keeping
  `test-support` on the dev-dependency. Frame supplies the existing GPUI
  patches. The crate has no gpui-component API or dependency to migrate;
  Frame remains on GPUI Kit 0.8.0.
- Keep the optional, default-enabled `space` feature and point it at Frame's
  existing `deps/theorem-search/rustyredcore_THG/crates/theorem-space`,
  explicitly disabling default features. Keep `unsafe_code = "forbid"`.
- Remove `theorem-design-gpui`. `src/font.rs` embeds upstream's restored
  JetBrains Mono asset, preserving the provider -> system CJK -> bundled
  fallback order and both byte caches. Copy the native system candidate
  search from Theorem's MIT `apps/theoremweb/crates/gpui/design/src/fonts.rs`
  into `load_system_cjk_fallback`, preserving candidate order and face
  validation. The wasm32 fallback remains `None`; `set_font_provider` is
  unchanged so Frame can supply its own face later.
- `src/mechanics.rs` and `src/hosted_tests.rs`: remove nine redundant clones
  of Copy geometry and let the context-menu test's callback-log type be
  inferred. These existing test-only lints fail Frame's strict all-targets
  clippy check; test behavior is unchanged.
- No GPUI Rust API changes were needed to build the rebased source.
- `src/paint.rs`, `src/lib.rs`: `paint_board` is `pub` and re-exported, so
  Frame's cards (`src/bump.rs`) draw the board's dot grid rather than a
  second one. Its behavior is unchanged.

## Rebase procedure

1. Fetch upstream and list what moved in the crate: `git -C <zorite checkout> log --oneline 6ee46f28cbe0017f7f152033227d897434ddd0ef..origin/main -- crates/gpui-whiteboard`. If nothing changed, stop.
2. Produce the upstream delta only: `git -C <zorite checkout> diff 6ee46f28cbe0017f7f152033227d897434ddd0ef <new rev> -- crates/gpui-whiteboard > upstream.patch`.
3. Apply it on a clean tree of this crate with `git apply --3way` run from the crate directory (use `-p3` to strip `a/crates/gpui-whiteboard/`). Resolve conflicts against the divergence list above, in this order of risk: `src/lib.rs` (`push_undo` call sites, `on_right_down`, the wheel / pinch / pan handlers, `flush`, `reorder_selection`, the inlined hit-tests), then `src/scene.rs`, then `src/font.rs`.
4. Re-check by hand every place upstream added a **new mutation entry point** (a new tool, a new `add_*`, a new edit path): it must call `push_undo(label)` before it mutates and reach `flush` or `settle_hosted`, or Hosted mode will silently miss that edit. Likewise any new camera mutation must go through `request_camera`, and any new `ElementKind` must be classified in `rotation_support`, `element_bounds` and the hit-test (the exhaustive matches will say where).
5. Run from Frame, offline, building only this crate:
   - `cargo build --offline -p gpui-whiteboard`
   - `cargo test --offline -p gpui-whiteboard --all-features`
   - `cargo test --offline -p gpui-whiteboard --no-default-features`
   - `cargo clippy --offline -p gpui-whiteboard --all-targets --all-features -- -D warnings`
   - Run the other three vendored note crates' tests, then compare
     `cargo tree --offline -d --workspace`, GPUI feature sets and Cargo.lock
     against the base. Do not build Frame/Servo for this scoped port.
6. Update the revision in "Origin" and add a line per conflict resolved. If upstream adopted one of the seams above, delete our version and its entry.
7. Do not run `cargo fmt` at package or workspace level; `rustfmt --edition 2024 <file>` only the files you changed.
