# YouTube in Frame's Servo engine

## Target and acceptance

The requested outcome is YouTube working as well as Chrome inside Frame's
Servo engine. This includes ordinary and live playback, audio/video sync,
seeking, adaptive and manually selected quality, captions, playback speed,
fullscreen, navigation between videos, ad transitions when served, sign-in,
background/foreground transitions, and sustained playback without unbounded
memory growth. Protected content requires a separate EME/CDM integration and
distribution assessment; it is still part of evaluating the broad parity goal.

Do not claim parity from compilation, a short video, codec reporting, or a
compatibility engine replacing Servo. Compare the same videos and actions in
Chrome on the same machine/network. Record startup, seek recovery, stalls,
dropped frames, sync, CPU/GPU, memory and power measurements. Set performance
thresholds against those measured Chrome results, rather than inventing them.

## Verified baseline — 2026-10-25

- Frame base: `74e432f`, branch `feat/liquid-glass-chrome`, no upstream set.
  Preserve unrelated `.theorem/` and existing untracked learning documents.
- Servo pin: `ebc89e684b721f0a8e2407aad20b121a61187779`, browser branch
  `Travis-Gilbert/servo07-browser-20261013`.
- The original Frame feature graph selects DummyBackend. The native
  `media_check` probe returned empty `canPlayType()` for H.264/AAC, VP8/Vorbis,
  VP9/Opus and AV1; both `MediaSource` and `SourceBuffer` were absent.
- GStreamer was absent from pkg-config and the standard macOS framework
  location. Installed Homebrew GStreamer 1.28.8 and libnice-gstreamer 0.1.24.
- Servo's macOS initializer explicitly loads 38 plugins from `lib/` beside
  the executable. The setup script reads that list from Cargo's resolved
  Servo source. Homebrew puts the NICE plugin in a separate formula's
  `libexec/gstreamer-1.0` directory.
- `gst-inspect-1.0 mse` confirms the installed `msesrc` plugin. A native
  GStreamer probe appending existing WPT files in 32 KiB chunks reproduced:

  ```text
  video/webm; codecs="vp8": before=1 after=1 remove_returned=1 passed=0
  audio/webm; codecs="vorbis": before=1 after=1 remove_returned=1 passed=0
  ```

  Exit status: 1. Audio append also emitted invalid-DTS assertions from
  `gst_media_source_sample_map_add`; investigate before treating the audio
  path as correct. This probe tests buffer behavior, not synchronized output.
- The rebuilt Frame binary and native example compile successfully. The
  example passes MP4 and WebM decoding, pause, seek and resume with 320x240
  frames and one audio track each. Pixel checks return `[255,0,0,255]` and
  `[254,0,1,255]`, matching the WPT clips' red frames. Inspected screenshots
  also show the video. The clips are muted; audible output is not established.
- A missing-fixture control produces MediaError 4 for both clips and exit 1;
  successful playback exits 0. Re-running setup preserves the same 38 links.
- A fresh-profile live watch of Blender's Big Buck Bunny loads the page and
  player but displays "Your browser can't play this video." The diagnostic
  returns exit 1, currentTime 0, readyState 0, dimensions 0x0, and no source.
  `MediaSource` and `SourceBuffer` remain absent. This is consistent with
  the known MSE gap; it does not rule out additional YouTube incompatibilities.
- Evidence: [before](evidence/youtube-2026-10-25/before.json),
  [playback](evidence/youtube-2026-10-25/playback.json),
  [negative control](evidence/youtube-2026-10-25/missing-fixture.json),
  [MSE removal failure](evidence/youtube-2026-10-25/mse-remove.txt),
  [YouTube diagnostic](evidence/youtube-2026-10-25/youtube.json), and
  [YouTube screenshot](evidence/youtube-2026-10-25/youtube.png).
  The native run emits a GL texture warning despite the successful pixel and
  screenshot checks; keep it visible during further GPU acceptance.

## Implementation sequence

1. **Frame media baseline.** Enable `servo/media-gstreamer`, resolve plugins,
   and run `media_check` with the existing WPT MP4/WebM clips. Verify actual
   audible output and GPU presentation in a visible Frame window separately.
   The automated clips are muted, so a pass cannot establish audio output.
   Package and relocate transitive dylibs/plugins for a portable `.app`, then
   test on a machine without Homebrew. Existing development app copies are
   separate binaries and must not be mistaken for the rebuilt Cargo binary.
2. **Reusable MSE backend.** Use GStreamer's demuxers, decoder pipeline and
   MSE library rather than writing MP4/WebM parsers in Servo. Repair and test
   range removal, append completion, timestamp handling and memory eviction
   against the actual installed runtime. Exercise separate audio/video,
   chunked appends, overlapping/replaced ranges, append windows, quality
   switches, end-of-stream, seek/refill and bounded buffering. The removal
   probe above currently fails: this is a concrete backend blocker.
3. **Servo integration.** Port useful portions of upstream Servo #40423 and
   servo/media #458 into the fork's current in-tree media crates. Complete
   MediaSource, SourceBuffer, SourceBufferList, object URL attachment and
   revocation, stable DOM object identity, spec exceptions, async events on
   Servo task sources, and capability reporting. Handle aborts, navigation,
   tab closure and detachment without stale callbacks or leaked pipelines.
   Keep incomplete APIs gated. Do not transplant TODOs or the closed PR's
   handwritten container parsers into the product.
4. **Conformance.** Run Servo's existing `media-source` WPT suite against the
   built fork, retaining exact pass/fail evidence. Add backend tests for the
   repair and focused WPT regressions where needed. Worker MSE support must
   either work or be accurately reported as unavailable; false capability
   claims must not steer YouTube into unsupported paths.
5. **Live YouTube and Chrome comparison.** Capture the first failing script
   or media operation from a direct watch page, then fix page compatibility
   independently. Cover every user journey in the target above, including
   long playback and live seek windows. Retain sanitized console/network
   evidence, build hashes and comparison measurements. Do not infer success
   for login, ads, live or protected content from an ordinary watch video.

## Upstream review

- [Servo #40423](https://github.com/servo/servo/pull/40423) is open and
  unmerged at `cfc451633ba893a93ae708f9c337c688acaa3ff9`. Its MSE DOM draft has
  commented-out source events, TODO buffered ranges, track lists and
  `changeType`, and incomplete codec checks. Reuse requires substantive work.
- [servo/media #458](https://github.com/servo/media/pull/458) is open and
  unmerged at `3f9382a0d0ca6511281fd8c16f1787a0abdf5f13`. It wraps
  `gstreamer-mse`; errors, asynchronous state and cleanup require review.
- [Servo #47269](https://github.com/servo/servo/pull/47269) remains closed,
  unmerged. Its own description documents one source buffer, incomplete
  coded-frame processing and removal that does not drop backend data.
- [GStreamer source](https://github.com/GStreamer/gstreamer/blob/main/subprojects/gst-plugins-bad/gst-libs/gst/mse/gstsourcebuffer.c)
  still implements `gst_source_buffer_remove()` as a type check and success
  return. The installed-library probe independently confirms the defect.

## Reproducing the checks

```bash
cargo build --locked --example media_check --bin frame
script/setup-media
cargo run --locked --example media_check
```

The no-argument probe checks capability reporting only. Its `passed` field
does not include MSE or YouTube acceptance. For actual ordinary playback,
serve the resolved Servo checkout's `tests/wpt/tests/media/` directory:

```bash
python3 -m http.server 8766 --bind 127.0.0.1 --directory "$SERVO_SOURCE/tests/wpt/tests/media"
```

In another terminal:

```bash
cargo run --locked --example media_check -- http://127.0.0.1:8766/
```

The native probe reuses `src/engine.rs` and `src/scene.rs`, creates its own
temporary profile, loads the WPT `movie_5.mp4` and `movie_5.webm`, and checks
time advancement, dimensions, decoded red pixels, pause, seek and resume. It exits nonzero on
failure or timeout. It does not use Frame's browsing profile or search index.
Screenshots are written to the system temporary directory and their paths
are printed to stderr. The live YouTube smoke diagnostic runs as follows:

```bash
cargo run --locked --example media_check -- \
  --youtube 'https://www.youtube.com/watch?v=aqz-KE-bpKQ'
```

Even a passing smoke check would only establish an advancing video clock;
it could be an ad and does not establish target-video identity or parity.

Compile the independent MSE backend check using the installed headers:

```bash
cc -DGST_USE_UNSTABLE_API -Wall -Wextra tests/media/mse_remove.c \
  -o /tmp/frame-mse-remove $(pkg-config --cflags --libs gstreamer-mse-1.0)
/tmp/frame-mse-remove \
  "$SERVO_SOURCE/tests/wpt/tests/media-source/webm/test-v-128k-320x240-30fps-10kfr.webm" \
  "$SERVO_SOURCE/tests/wpt/tests/media-source/webm/test-a-192k-44100Hz-1ch.webm"
```

`SERVO_SOURCE` is the directory containing the resolved Servo workspace
`Cargo.toml`, not Frame's directory. Find it through `cargo metadata` rather
than assuming a Cargo cache hash or editing Cargo's cached checkout.

## Status

Media backend activation, local runtime preparation, and automated ordinary
playback verification are complete. MSE integration, backend repair, WPT,
audible output, portable packaging and live YouTube/Chrome acceptance remain
incomplete. No claim of YouTube playback or Chrome parity has been established.

Next concrete work: repair GStreamer's buffered-range removal and investigate
the audio DTS assertions using the native backend probe, then integrate the
working backend into Servo's object URL/media-element and DOM task machinery.
Neither upstream draft is ready to land as-is. This work requires changes to
the media library and the Servo fork, beyond the Frame feature activation.
Current Frame changes are local and uncommitted; nothing was published.

## MSE continuation — 2026-10-25

The development dependency now lives at `deps/servo-mse`, based on the same
`ebc89e6` fork revision. GStreamer 1.28.8 source is in `deps/gstreamer-mse`;
its isolated install is `deps/gstreamer-mse-runtime`. Neither checkout is
published. Preserve them while this implementation is under validation.

The native backend regression now passes for separate VP8 and Vorbis files:
range removal changes one buffered range to zero, and untimed codec headers
no longer cause invalid-DTS assertions. Append completion is dispatched after
the demuxer consumes samples instead of immediately after queueing input.
See [native regression](evidence/youtube-2026-10-25/mse-remove-fixed.txt).
This does not yet establish full removal, reset or playback conformance.

Servo integration adds gated MediaSource, SourceBuffer and SourceBufferList
bindings, GStreamer-backed buffers, asynchronous completion events and object
URL attachment. Frame enables the preference. The integration is currently
being compiled and tested; no YouTube playback claim follows from this work.
A new `media_check --mse` mode tests chunked separate audio/video appends,
end-of-stream, clock advancement and seek/resume with existing WPT fixtures.

### First live playback evidence

The first successful native watch-page run advanced to 7.450083333 seconds,
reported HAVE_ENOUGH_DATA, and rendered an 854×480 Big Buck Bunny frame. This
probe used an executable copy whose MSE install name was corrected with
`install_name_tool`, then ad-hoc signed; the normal Cargo link is being verified
separately. This distinction matters: a manually corrected probe is not the
finished application build. Evidence: [result](evidence/youtube-2026-10-25/youtube-mse-first.json)
and [rendered frame](evidence/youtube-2026-10-25/youtube-mse-first.png).

Two additional native fixes were necessary: a distinct patched MSE dylib name
prevents Homebrew's library from winning linker search order, and the
initialization-segment readiness check now releases its mutex when the second
stream has not arrived. YouTube selected separate Opus audio and AV1 video;
the lock leak prevented the video stream from being attached after audio.
The native remove/refill test passes for both VP8 and Vorbis.

The first two-second MSE fixture played, but the initial seek assertion targeted
the end of the clip and incorrectly required playback beyond its duration. The
test now seeks to 0.75 seconds and requires resumed playback past 1.2 seconds.
The browser fixture creates both SourceBuffers before appending, exercising
initialization arriving on separate streams.

Remaining acceptance includes an ordinary Cargo-built executable, pause/seek/
speed on the live watch page, ordinary-media regression, audible output and
full app presentation. Broader gaps remain in abort/parser reset semantics,
negative timestamp offsets, live duration/ranges, source track-list mapping,
same-origin cross-global object URLs, WPT conformance, protected content,
portable packaging, sustained playback and the Chrome comparison. Do not
interpret this first successful watch as completion of those requirements.

### Browser runtime regressions uncovered by live playback

- A coalesced batch of JavaScript timers was moved from its traced queue into
  an unrooted Rust vector before invoking callbacks. An earlier callback's
  allocations could collect a later callback. A 256-callback allocation test
  reproduced exit 139 before the fix. The pending queue and current JS timer
  task now use the existing RootedTraceableBox; the same test passes with all
  256 callbacks and checksum 32640. This also preserves interval continuations
  and callback arguments during invocation.
- Repeating `video.play()` while the backend was already playing incorrectly
  entered `update_media_state`'s pause branch. The focused MSE test reproduced
  a clock stall at approximately 0.54 seconds while `video.paused` was false.
  The branch now pauses only when the element should not be playing. The
  final rebuild and live pause/seek/speed verification are in progress.
- The ordinary MP4/WebM pixel, pause and seek tests still pass with MSE enabled.
- Chrome was not available in the connected browser inventory (only the
  in-app browser and MCP Apps were exposed), so a Chrome comparison has not
  been performed. The running `FrameHome.app` is a separate existing build;
  native media probes use fresh temporary profiles and the current engine.

## Verified playback milestone — final local build

The normal Cargo build of both `frame` and `media_check` succeeded. Both link
the distinct `libgstmse-frame-1.0.0.dylib` directly; no install-name editing is
required. Plugin setup finds all 39 plugins and is idempotent.

| Check | Result |
| --- | --- |
| Big Buck Bunny, `aqz-KE-bpKQ` | Correct video ID, no ad, 854×480; pause held; seek to 60 s resumed at 61.003 s; measured 1.523× over a 4.002 s steady interval at requested 1.5× |
| Me at the zoo, `jNQXAC9IVRw` | Correct video ID, no ad, 320×240; pause held; backward seek to 9.5105 s resumed at 11.015 s; measured 1.524× at requested 1.5× |
| Separate MSE audio/video fixture | Chunked appends, rendered frames, repeated play and seek/resume pass |
| Ordinary MP4/WebM | Correct red pixels, dimensions, audio-track metadata, pause, seek and resume pass |
| Missing files | Both clips fail with MediaError 4; probe exits 1 as expected |
| Native removal/refill | VP8 and Vorbis buffered ranges are removed and successfully refilled |
| Timer allocation regression | 256 callbacks complete with checksum 32640; pre-fix run crashed with exit 139 |

Evidence: [first live acceptance](evidence/youtube-2026-10-25/youtube-acceptance.json),
[frame](evidence/youtube-2026-10-25/youtube-acceptance.png),
[second video](evidence/youtube-2026-10-25/youtube-second.json),
[second frame](evidence/youtube-2026-10-25/youtube-second.png),
[MSE](evidence/youtube-2026-10-25/mse-acceptance.json),
[ordinary playback](evidence/youtube-2026-10-25/playback-final.json),
[negative control](evidence/youtube-2026-10-25/negative-final.json),
[timers](evidence/youtube-2026-10-25/timer-gc-acceptance.json).

Rate changes involve an internal seek. The speed check waits for resumed
movement before measuring a steady four-second interval; counting the seek
latency as playback speed produced a false failure in the earlier probe.
The public watch tests use temporary profiles and do not establish audible
output, full app interaction, login, ad transitions, live streams, DRM,
long-session stability, quality switching, or Chrome performance parity.
The broader acceptance scope and implementation gaps above remain open.

The running `FrameHome.app` is a separate build. To use this checkout's rebuilt
browser, quit that app first and launch from this repository:

```bash
script/media-env cargo run --locked --bin frame
```

Fresh setup and focused reproduction:

```bash
script/bootstrap
script/bootstrap-media
script/media-env cargo build --locked --bin frame --example media_check
script/setup-media
script/media-env python3 tests/media/check_native.py
```

Serve `deps/servo-mse/tests/wpt/tests` on localhost:8766. Then:

```bash
script/media-env cargo run --locked --example media_check -- --mse http://127.0.0.1:8766/media-source/
FRAME_MEDIA_SCRIPT=tests/media/youtube.js script/media-env cargo run --locked --example media_check -- --youtube 'https://www.youtube.com/watch?v=aqz-KE-bpKQ'
FRAME_MEDIA_SCRIPT=tests/media/timer-gc.js script/media-env cargo run --locked --example media_check
```

The native dependency checkouts remain under the ignored `deps/` directory;
the complete patches and pinned revisions are preserved in `patches/`.
No fork branch has been pushed or published. The next concrete acceptance step
is playback in the rebuilt visible Frame window with audible sound, followed
by a longer session and quality changes; the existing FrameHome process was
left running because it belongs to concurrent work.

### Launchable local development bundle

The media build is saved at
`/Volumes/SSD Samsung/frame-deps/FrameYouTube.app`. Its executable comes from
the matching Cargo artifact `frame-f453e6e3785a2645`, not the shared top-level
`debug/frame` path, which another local build subsequently replaced. It has
39 resolved plugin links, valid Info.plist, and a verified ad-hoc signature.
[Build manifest and hashes](evidence/youtube-2026-10-25/build-manifest.json)
record the bundle, probe, native library and patches. Both patches were applied
to temporary copies of their pinned base files and compared byte-for-byte with
the working sources; both checks passed.

Quit the existing FrameHome app before opening this development build because
both use Frame's existing profile directory:

```bash
open "/Volumes/SSD Samsung/frame-deps/FrameYouTube.app"
```

The bundle uses the local native runtime and Homebrew libraries. It has not
been notarized or tested as a portable distribution, and the full visible
Frame window still needs acceptance. The media probe uses Frame's actual
engine and rendering context; all recorded playback results are from that
native probe, not a replacement browser engine.


### Follow-up: visible YouTube window and surrounding rendering

The user confirmed video playback in FrameYouTube and clarified that the earlier
quit may have been another test Frame instance. The packaged app remained alive
for more than 17 minutes during inspection; there was no new matching crash
report. No startup-crash fix is claimed. FrameHome was also running and held the
search-store lock; the test app logged its existing in-memory fallback.

The user's screenshot showed black header icons and missing recommendation
thumbnails. A reduced native-rendering fixture reproduces the SVG problem:
CSS-inherited white and stylesheet-selected green both rasterize black; a blue
presentation attribute remains blue. Servo's inline SVG serialization clones
markup without the host document's computed CSS paint. The repair passes Stylo's
computed paint declarations to the existing SVG renderer and keys the serialized
cache on those declarations, including descendant style changes. Layout must
rebuild the replaced SVG box when the serialized paint becomes stale.

`tests/rendering/check_svg_paint.py` uses Pillow to check screenshot pixels before
and after changing an ancestor's color and a stylesheet rule. A literal blue
fill and a local red gradient are controls. The previous build fails the CSS
paint assertions; the final build passes all eight pixel checks, including the
local gradient before and after restyling. The bridge also avoids empty leading
CSS declarations rejected by the SVG parser and preserves same-document paint
server fragments when serializing computed absolute URLs.

Fresh public watch-page probes at 1280x820 do load visible recommendation images,
including in dark mode. The reported thumbnail issue is not yet reproduced.
The probe now supports physical viewport dimensions, display scale, and dark
appearance to match the user's large Retina window without accessing their
profile. Watch diagnostics wait for DOMContentLoaded and track document loads,
so they survive YouTube's observed initial document reload without waiting on
unrelated resources that can keep the load event pending.

Final native validation and delivery:

- The Openprinter watch page at 4992x2672 physical pixels, display scale 2,
  dark appearance, renders readable SVG header icons and all six visible
  recommendation thumbnails. This is a fresh profile, not the user's signed-in
  session; the screenshot's missing thumbnails were not reproduced there.
- Clicking a recommendation navigates successfully; all six visible thumbnails
  load before and after the navigation in the Retina probe. This final run has
  DOM evidence only: its asynchronous screenshot did not finish before exit.
- Big Buck Bunny identity/no-ad, playback, pause, seek to 60 seconds and 1.5x
  speed pass again. The measured steady playback ratio was 1.526 over 4.001s.
- The complete Servo patch applies to its pinned base and reproduces all 28
  changed/new dependency source files byte-for-byte.
- FrameYouTube.app was updated atomically and re-signed. Its Info.plist and
  ad-hoc signature verify, and all 39 plugin links resolve. Bundle version is
  20261025.2. The running app retains its previous executable until restarted.

Evidence is in `evidence/youtube-2026-10-25/`: `svg-before/`, `svg-after/`,
`youtube-retina-rendering.*`, `youtube-spa-rendering.json`, `rendering-playback.*`,
`rendering-patch-check.json`, and `rendering-build-manifest.json`.

Reproduce the pixel check with a Python environment containing Pillow:

```bash
script/media-env python3 tests/rendering/check_svg_paint.py /path/to/media_check
FRAME_MEDIA_SCRIPT=tests/rendering/youtube.js script/media-env /path/to/media_check --width 4992 --height 2672 --scale 2 --dark --youtube 'https://www.youtube.com/watch?v=wSV5XT4sl2A'
```

The next account-specific check is the same page in the user's signed-in window
after restarting the updated bundle. Chrome parity and the earlier broader
acceptance gaps remain open. No remote branch was pushed.
