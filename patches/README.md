# Experimental MSE integration

`script/bootstrap-media` checks out Servo commit
`5c8325a389555f3b1adc476106d8529791afe13e` from `Travis-Gilbert/youtube-mse`
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
