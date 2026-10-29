# Experimental MSE integration

`script/bootstrap-media` checks out Servo commit
`efdf7370da024f467f7526798fe5c6e50e784b52` from `Travis-Gilbert/servo`
(branch `main`)
and applies the GStreamer patch without discarding local work. That commit
also rounds text selection highlights the way code editors draw them
(`components/layout/display_list/selection.rs`).
`script/media-env` selects the native pkg-config files and plugin directory.
The development runtime depends on the installed Homebrew GStreamer libraries;
it is not a redistributable application bundle.

- `servo-mse.patch`: archival diff against fork revision
  `ebc89e684b721f0a8e2407aad20b121a61187779`. These changes are included in
  the pinned Servo commit; bootstrap does not reapply this patch.
  Based on the existing in-tree media abstractions and the approaches in
  Servo PR #40423 and servo/media PR #458. Containers are parsed by GStreamer.
  The patch also roots coalesced JavaScript timers across GC and keeps repeated
  playback state updates from pausing an already-playing video. The Rust binding lacks `SourceBuffer.buffered`, so a small ownership adapter
  calls its native GArray API. DOM range aggregation follows MSE's intersection
  rule using Servo's TimeRanges representation.
  Inline SVG paint is serialized using Stylo's computed values so CSS colors
  survive rasterization, including changes to ancestor and descendant styles.
  This bridges the existing style and SVG engines; it adds no CSS parser.
- `gstreamer-mse.patch`: GStreamer 1.28.8, revision
  `1b2a95a1b72787cda146b46df9605b168e5b683b`. Repairs removal, completion timing,
  untimed codec-header handling and a lock leak while awaiting the second
  initialization segment. It resets per-pad flow results after flushing a seek.
  The patched library has a distinct filename to
  prevent the system library from winning Cargo's library-search order.

Original source license headers are retained. Servo changes are MPL-2.0;
GStreamer changes retain the source files' LGPL terms. See the pinned source
repositories for full licenses. No new container parser is introduced.

See `docs/plans/youtube.md` for live evidence and known gaps. These patches are
under validation and do not establish complete MSE conformance or Chrome parity.

## Browser search concurrency

`browser-search-concurrency.patch` is applied by `script/bootstrap` to Theorem
`7eb1ba024b6ccab8146f7857535dcdc6334eb39b`. It was measured against `6e57dbb3`,
the revision installed for the performance baseline, and applies unchanged to
the pinned revision. It separates preparation and retention under
the browser-store lock from provider/model I/O without that lock, preserving
existing query, privacy and retention owners. Active index generations are
leased until their queries complete. No search engine or HTTP client is copied.
See `docs/plans/browser-performance.md` for validation and measurement evidence.


## Video frame pacing

This is commit `efdf7370` in the pinned Servo; it was a separate patch until
the fork's branches were consolidated. Stable-size video frames with the same WebRender image key request a repaint
through the existing Paint API instead of invalidating style/layout each time.
New image keys and natural-dimension changes still rebuild layout, preserving
source/poster transitions and resize events. The patch does not change decoding
or media buffering. It also prevents GstPlay.pause() from rewinding after EOS
and lets HAVE_CURRENT_DATA recover after an explicit replay/seek. The stopped
state is recorded before sending EOS to script, avoiding a callback race. Native
transition and pacing evidence is recorded in
`docs/plans/evidence/browser-performance-2026-10-27/smoothness-report.md`.

End-of-stream/replay and per-source playback regressions are documented in
`docs/plans/evidence/browser-source-switch-2026-10-27/report.md`.


## H.264 resolution changes

`gstreamer-mse.patch` also fixes h264parse's output dimensions after an in-band
SPS change. Container caps can retain the initial size; parsed cropped dimensions
must accompany the updated codec data so the decoder renegotiates correctly.
The existing GStreamer parser and hardware decoder are reused. Bootstrap builds
the videoparsers plugin, and setup-media refreshes recognized development
symlinks to use it while refusing to replace real files or arbitrary symlinks.
The upstream GstCheck regression and native playback/smoothness evidence are in
`docs/plans/evidence/browser-mp4-resize-2026-10-27/report.md`.
