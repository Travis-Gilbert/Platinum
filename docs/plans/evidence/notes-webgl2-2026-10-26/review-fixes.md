# WebGL2 review fixes, 2026-10-26

The fixes are committed in two places:

- **Servo:** nine commits on `feat/webgl2-cosmos` in `servo/`.
- **Frame:** two commits on `feat/notes-webgl2` in `frame/`, which depend on the last Servo commit.

No dependencies were added. The binaries in `servo/target/debug/` are:

- `servoshell.base`: the branch before the fixes.
- `servoshell.mine3`: the first complete build.
- `servoshell.mine5`: the tip's source.

## Commits

| Servo commit | Contents |
| --- | --- |
| `4df3cd1e6f4` chore(deps): record gstreamer-mse in Cargo.lock | The lock entry that 81d8e299956 left out. |
| `b1b8287005a` fix(webgl): validate mip levels and array texture storage limits | The tex_image validators, the mip helpers and their tests in `shared/canvas/pixel_storage.rs`, and the module declaration. |
| `3e554d69d95` fix(webgl): check pixel storage and source offset arithmetic | `pixelunpack.rs`, `required_bytes` and its tests, compressed lengths, texture accounting and indexing, the allocation, offset, PBO, unpack, buffer-view and readback-size hunks in both contexts, and the GL-thread upload, conversion and flip guards. |
| `e3ff7456e7a` fix(webgl): classify integer external formats as unsized | `TexFormat::is_sized`, its unit test, and the new `tests/wpt/tests/webgl/texture-validation-overflow.html`. |
| `5b9eb41fe64` fix(webgl): derive float extensions from native capabilities | The extension manager and its unit tests, the four extension files, framebuffer and renderbuffer renderability, and the native GL version plumbing. |
| `655079a06f1` fix(webgl): accept implementation color readback pairs | The implementation-pair command, `ColorBufferKind`, the WebGL2 readPixels and getParameter hunks, packed read sizing, and the new `float-extensions-and-read-pixels.html`. |
| `8991062fd9a` fix(webgl): read float color buffers back in WebGL 1 | `read_pixels_type` and its use in WebGL1 `readPixels` and `IMPLEMENTATION_COLOR_READ_TYPE`. |
| `a1e885a67ba` docs(webgl): note the allowlisted WebGL2 interface exposure | The `is_webgl2_enabled` comment only. |
| `2b522612c4d` fix(net): preserve trusted initiators for embedder scheme gates | `shared/net/request.rs` and its tests, the Constellation message, navigation and RequestClient construction, the LoadData call sites, protocol-registry `remove`, and the protocol API re-exports. |

The Frame commits are:

- `chore(media): pin Servo at the WebGL2 review fixes`, which moves the `servo-mse` revision in `script/bootstrap-media` to the Servo tip.
- `fix(engine): restrict notes protocols to trusted initiators`, which holds the `src/engine.rs` gate, the automatic handler wrapper, the unit tests, this report and the notes plan addendum.

The split was cut hunk by hunk from one working tree. The tip's tree is the tree that built `servoshell.mine5`, plus the regenerated WPT manifest and two lines of test setup in `extensions.rs`. Those lines are test-only, so they do not change `servoshell`. Each commit compiles on its own (see Verification).

## Changes and correctness

1. **Mip levels and texture limits.**
   - `validations/tex_image_3d.rs` rejects negative and excessive signed levels before any size arithmetic.
   - Both image validators use the checked shift in `shared/canvas/pixel_storage.rs`, so negative levels and shift counts of 32 or more cannot wrap or panic.
   - The 3D validator separates the 3D size limit from the array width, height and layer limits.
   - `TexStorageValidator` uses depth for TEXTURE_3D only.
   - Excess mip counts remain INVALID_OPERATION.
2. **Pixel storage and offsets.**
   - **Layouts and allocation:**
     - `pixelunpack.rs` computes source layouts with checked u64 arithmetic and checked conversion to usize.
     - Null 2D and 3D uploads use that layout and a fallible zeroed allocation.
     - Repacking checks its allocation size and keeps INVALID_OPERATION distinct from OUT_OF_MEMORY.
   - **What this replaces:** the previous 2D path used unchecked u32 lengths and the infallible `GenericSharedMemory::from_byte`.
   - **Error codes:**
     - A recoverable allocation failure gives OUT_OF_MEMORY.
     - Dimensions beyond GL limits still give INVALID_VALUE.
     - Texture offset failures give INVALID_OPERATION, while clearBuffer offsets keep INVALID_VALUE.
     - Buffer-data and uniform offset overloads keep their own INVALID_VALUE rules, and now check products, sums and detached slices.
   - **Byte counts and lookups:**
     - Compressed image byte counts use the same checked helper.
     - Texture accounting saturates instead of wrapping.
     - Out-of-range level indices return no image.
   - **GL-thread checks:**
     - The GL thread checks the prepared upload's byte length before every client-memory texture upload.
     - Conversion capacity, vertical-flip storage and native readback lengths are checked, including packed pixel sizes.
     - If the GL thread cannot allocate for a conversion, it refuses the upload, because that asynchronous command has no error reply to script.
     - PBO byte offsets are checked before they are narrowed to glow's u32 offsets.
   - **2D offset upload:**
     - It takes its suffix with `get`, so a detached view cannot panic.
     - Its redundant post-unpack length check is gone; element-type validation stays.
   - **Limits:**
     - Shared-memory transport still uses Servo's infallible IPC allocation.
     - OS-level exhaustion that kills the process is not something `try_reserve_exact` can recover from.
   - **Overloads not touched:**
     - The WebIDL still comments out the texImage3D and texSubImage2D srcOffset overloads, and none was added.
     - The implemented texImage2D and texSubImage3D overloads are hardened.
3. **Integer external formats.**
   - `TexFormat::is_sized` treats RED_INTEGER, RG_INTEGER, RGB_INTEGER and RGBA_INTEGER as unsized.
   - texStorage validation still rejects them as internal formats.
   - The subimage debug assertion now accepts them, as it should.
4. **Float extensions.**
   - The extension manager gets the native GL version from the GL thread, independently of GLSL.
   - Desktop GL core or ARB support, or GLES EXT support, decides float rendering and blending.
   - WebGL1 linear float filtering and half-float rendering require their OES texture extensions.
   - WebGL2 keeps exposure.
   - RGB16F is not color-renderable under WebGL2 half-float. R16F, RG16F and RGBA16F are, including renderbuffer allocation with only the half-float extension.
5. **Interface exposure.**
   - The `WEBGL2_ORIGINS` exception is kept for the context and its object interfaces together.
   - A comment in `is_webgl2_enabled` records that default preferences still admit the allowlisted host.
6. **WebGL2 implementation readback pair.**
   - Accepted pairs:
     - Both the client-array and PBO readPixels paths accept the pair `getParameter` reports, as well as the required base pairs.
     - For RGB10_A2 that covers both the unsigned-byte read and the packed read.
   - Which pair `getParameter` reports:
     - GLES, and desktop GL that supports the query, report the native pair.
     - Older desktop GL, or a native answer outside WebGL's enums, gets Servo's supported fallback. For RGB10_A2 that is RGBA/UNSIGNED_INT_2_10_10_10_REV.
   - Validation order and sizing:
     - A packed pixel occupies one element, not four.
     - Invalid format and type enums are checked before the no-read-buffer INVALID_OPERATION.
7. **WebGL1 float readback.**
   - WEBGL_color_buffer_float and EXT_color_buffer_half_float allow one RGBA read type per buffer: FLOAT from a floating-point color buffer, UNSIGNED_BYTE from any other.
   - `read_pixels_type` picks that type from the first color attachment of the bound framebuffer. WebGL1 binds one framebuffer, kept in the draw slot.
   - Before this commit, WebGL1 readPixels refused FLOAT from every buffer.
8. **Notes request boundary.**
   - `Engine::start_with_protocols` removes the registered notes handler and registers it again wrapped in one `NotesProtocol` gate. The app handler and the graph probe therefore share one boundary.
   - A refused request returns a network error before the underlying handler can inspect a file.
   - Any notes handler passed to this entry point is wrapped. This checkout has only the graph probe handler.

## Known gap: upload-time fake black

`validate_filterable_texture` (`webglrenderingcontext.rs`, about lines 621-674) predates these fixes and is still on the fork's `origin/master` as of 2026-10-24.

- **When it fires:** a float or half-float texture is not filterable (OES_texture_float_linear is not enabled) and uses a linear filter. The default MIN_FILTER, NEAREST_MIPMAP_LINEAR, counts as linear.
- **What it does:** it replaces the image with RGBA8 black, [0, 0, 0, 255], with data type UnsignedByte.
- **Where it runs:** from both TexImage2D overloads and from texParameter.

The specs instead sample a texture that is incomplete at draw time as black and leave its data alone. The replacement has two effects:

- It destroys data.
- It turns a float render target into an RGBA8 one whenever the filters are set after the upload.

Commit 7 reads the attachment's real type. That means a texture that went through this path reads back as RGBA8: FLOAT is refused and UNSIGNED_BYTE returns black. Debug output confirmed the stored data type was UnsignedByte.

This one path explains every remaining float failure below. The fix substitutes a black texture per affected unit at draw time and removes the replacement. It is left for its own change.

## Initiator tracing

**Why the existing request fields cannot identify the initiator:**

- **Navigation clients:**
  - `script/navigation.rs::request_builder` builds a synthetic RequestClient for navigations.
  - Script navigation carries its source origin; embedder navigation uses an opaque origin.
  - Destination is Document or IFrame, and mode is Navigate.
- **Subresource clients:**
  - GlobalScope and Window create the clients for subresources and fetch.
  - Their modes include NoCors and CorsMode.
  - Their destinations include Image, Script and None.
- **Fetch:** `net/fetch/methods.rs` fills in the origin from the client and applies referrer policy before calling a protocol handler.
- **Custom schemes:** `servo_url::ImmutableOrigin::new` gives them opaque origins.
- So neither an opaque origin nor a missing referrer authenticates an embedder request, and a referrer cannot reliably identify a notes document.

**What Servo now carries:**

- The trusted initiating document URL, kept independent of referrer policy, from document navigation through LoadData to RequestClient.
- An explicit mark on direct Constellation top-level navigations.
- Missing serialized fields default to no authority.

**What Frame allows:**

- A request initiated by a frame-notes document.
- A marked top-level Document/Navigate embedder load whose whole redirect URL list stays on frame-notes.

**What Frame denies:**

- HTTP(S), data: and about:blank initiators.
- Requests without a client.
- Iframe-shaped embedder requests and cross-scheme embedder redirects.
- Workers without document metadata, which fail closed.

## Verification

These ran outside the sandbox unless noted. mach was run from the real checkout, through the space-free alias `.../scratchpad/webgl2-servo`.

**Builds:**

- `mach build --dev --media-stack=dummy` passed for mine3, mine4 and mine5. mine5 is the tip's source. Logs: `servo/target/build-mine5.log`.
- `mach check --media-stack=dummy` passed at each of the nine Servo commits, with no new warnings in the changed components.
  - The first full check took 573 s. Later ones took 28-316 s.
  - The first cut of the split failed at the storage commit. One hunk there moved the read-buffer lookup that only the readback-pair commit needs. That hunk is now cut in two, and the tip's tree is unchanged.

**Servo tests:**

- **Standalone `rustc --test`, in the sandbox:** all 16 tests passed, in both debug and `-O`. That is 9 pixel-unpack, 3 pixel-storage and 4 existing shader-isnan tests. Logs: `servo/target/mine3-*-{debug,release}.log`. The new tests are:
  - `page_supplied_levels_cannot_overflow`
  - `volumes_larger_than_u32_do_not_wrap`
  - `array_layers_do_not_add_mip_levels`
  - `volume_length_does_not_wrap_at_four_gibibytes`
  - `unrepresentable_volume_and_allocation_fail`
  - `empty_volume_does_not_allocate_or_copy_skipped_rows`
- **`cargo test --offline -p servo-net-traits --test request_client`, from Servo's workspace:** 5 passed, including the new `missing_protocol_initiator_metadata_fails_closed`.
- **`cargo test -p servo-config --lib`, in the sandbox:** 4 passed.
- **`cargo test -p servo-canvas-traits --lib`, in the sandbox:** 4 passed, including `integer_external_formats_are_unsized`.
- **`script/media-env cargo test --offline -p servo-script --lib dom::webgl`, from Frame's workspace:** 18 passed, 0 failed, in 1:38. That covers the extension, pixel-unpack, shader-isnan and WebGL2 context tests, including `compressed_source_offsets_do_not_wrap` and `rgb10_a2_accepts_both_required_read_types`.
  - The test target only compiles with one unchanged module switched off. The IndexedDB `mod tests` at `idbdatabase.rs:528` has no `#[cfg(test)]`, and it compares types that lack `PartialEq` and `Debug`. It was switched off for the run and restored afterwards.
  - The first run failed 3 of 18. The extension tests borrowed a `DomRefCell` outside a script thread. Their helper now marks the test thread as a script thread, and that change is folded into `5b9eb41fe64`. It only touches `#[cfg(test)]` code, so the per-commit `mach check` results stand.
  - From Servo's own workspace, without mach, `mozjs_sys` fails to build from source.
- **`mach test-tidy`:** the config, directory and WPT lint stages ran, then tidy stopped because `cargo-deny` is not installed. The Rust file checks report only after that stage, so tidy did not complete. Changed Rust code is rustfmt-formatted, and `git diff --check` passes in both trees.

**Frame:**

- **`script/media-env cargo test --offline -p frame`:** 59 passed, 0 failed. That includes the three gate tests:
  - `own_navigation_and_notes_subresources_are_allowed_without_a_referrer`
  - `web_and_opaque_initiators_cannot_probe_notes`
  - `embedder_authority_is_limited_to_direct_top_level_notes_loads`
  - The two scene tests and the local-search test that failed in the sandbox pass here.
- **`script/media-env cargo clippy --offline -p frame --all-targets -- -D warnings`:** fails, but only on lines this change does not touch:
  - suspicious assignment formatting in `profile_ui.rs`
  - collapsible `if`s in `browser.rs`
  - type complexity in `engine.rs`, at the unchanged `after_spin` field, and in `navigator.rs`
  - default-field reassignment in the unchanged `Opts` setup in `engine.rs`
  - a unit binding in `share.rs`
  - dead search code when compiled into examples
- **`script/media-env cargo build --offline --example graph_probe`:** passed.

All three Frame commands are in `servo/target/review-rerun.log`.

**What was not tested:**

- Live GPU acceptance beyond WPT.
- A 32-bit target. Checked conversions and boundary tests cover representability on this host.
- The full null upload. The large-volume WPT case uses an undersized array instead of asking for 32 GiB of zeroed memory, and the Rust unit tests cover the exact null-upload length and the allocation failure.

## WPT comparison

The expectations in `tests/wpt/meta` describe Linux CI with WebGL2 disabled. These runs are macOS with `--pref dom_webgl2_enabled=true`, so mach exits nonzero on unexpected results against that metadata. The metadata is left unchanged. The comparison counts PASS subtests per file.

### Full run, base against mine3

- **What ran:** every conformance and conformance2 area plus the two new files, 1974 tests, with six processes, using [`review-rerun.sh`](review-rerun.sh).
- **Time:** 241.9 s for base, 186.1 s for mine3.
- **Logs:** `servo/target/wpt-{base,mine3}-review.raw.log`.
- **mine5:** this run predates commit 7. The serial float run below measures commit 7 separately.

| Area | base | mine3 |
| --- | ---: | ---: |
| conformance/attribs | 6576 | 6576 |
| conformance/buffers | 345 | 345 |
| conformance/canvas | 225 | 225 |
| conformance/context | 1191 | 1191 |
| conformance/extensions | 3351 | 3294 |
| conformance/glsl | 7833 | 7833 |
| conformance/limits | 36 | 36 |
| conformance/misc | 2725 | 2725 |
| conformance/more | 182 | 182 |
| conformance/offscreencanvas | 24 | 24 |
| conformance/programs | 263 | 263 |
| conformance/reading | 365 | 365 |
| conformance/renderbuffers | 732 | 732 |
| conformance/rendering | 1141 | 1140 |
| conformance/state | 836 | 836 |
| conformance/textures | 29181 | 29172 |
| conformance/typedarrays | 1411 | 1411 |
| conformance/uniforms | 963 | 963 |
| conformance2/attribs | 2764 | 2764 |
| conformance2/buffers | 1881 | 1881 |
| conformance2/canvas | 10 | 10 |
| conformance2/context | 153 | 153 |
| conformance2/extensions | 608 | 774 |
| conformance2/glsl3 | 933 | 1008 |
| conformance2/misc | 689 | 5341 |
| conformance2/offscreencanvas | 12 | 12 |
| conformance2/programs | 48 | 48 |
| conformance2/query | 43 | 43 |
| conformance2/reading | 378 | 590 |
| conformance2/renderbuffers | 581 | 593 |
| conformance2/rendering | 3138 | 4203 |
| conformance2/samplers | 172 | 172 |
| conformance2/state | 265 | 134 |
| conformance2/sync | 12 | 15 |
| conformance2/textures | 2580 | 6200 |
| conformance2/transform_feedback | 196 | 196 |
| conformance2/uniforms | 1644 | 1644 |
| conformance2/vertex_arrays | 39 | 39 |
| conformance2/wasm | 66 | 66 |
| **Total** | **73592** | **83199** |

Five files had fewer passes or a worse end status in mine3. Each was rerun serially, twice on each binary.

| File | Parallel, base then mine3 | Serial base | Serial mine3 | Reading |
| --- | --- | --- | --- | --- |
| conformance/extensions/oes-standard-derivatives.html | 80 OK, 0 CRASH | 80/0, 80/0 | 80/0, 80/0 | A crash under parallel load, not a regression. |
| conformance/textures/misc/texture-fakeblack.html | 4 OK, 0 CRASH | 4/2, 4/2 | 4/2, 4/2 | The same. |
| conformance2/state/gl-object-get-calls.html | 131 TIMEOUT, 0 CRASH | 86/1, 86/1 TIMEOUT | 131/1, 148/1 TIMEOUT | Times out on both. mine3 gets further. |
| conformance/rendering/blending.html | 8, 7 | 8/5, 8/5 | 7/7, 7/7 | The fake-black path. See the float run below. |
| conformance/textures/misc/texture-upload-size.html | 70, 62 | 74/4, 70/0 | 54/0, 54/0 | Timing-dependent on both builds. See below. |

**texture-upload-size:** the number of subtests varies on both builds.

- **Four more serial runs per binary:**
  - base: 54, 62, 86 and 54 passes, no failures.
  - mine5: 70, 54, 54 passes, then 58 passes with 4 failures.
  - Logs: `servo/target/wpt-{base,mine5}-upload.raw.log`.
- **Why:** the console output shows image uploads running again after the video cases have started. So the count depends on how many extra image load events arrive before the test finishes.
- **The failures:** base showed the same 4 in its first serial run.

Both builds crashed some files under parallel load: 8 on base and 9 on mine3. The two files that crashed only on mine3 match base exactly when run serially. gl-object-get-calls times out on both builds in every run.

### Serial float and readback run, base against mine5

- **What ran:** `--processes 1 --repeat 2`. Both runs of each binary gave identical results.
- **Time:** 184 s in total.
- **Logs:** `servo/target/wpt-{base,mine5}-float.raw.log`.

| File | base pass/fail | mine5 pass/fail |
| --- | ---: | ---: |
| conformance/extensions/ext-color-buffer-half-float.html | 90/15 | 98/7 |
| conformance/extensions/ext-float-blend.html | 8/2 | 16/0 |
| conformance/extensions/oes-texture-float.html | 94/16 | 113/7 |
| conformance/extensions/oes-texture-half-float.html | 114/8 | 122/0 |
| conformance/reading/fbo-remains-unchanged-after-read-pixels.html | 5/0 | 5/0 |
| conformance/reading/read-pixels-pack-alignment.html | 166/0 | 166/0 |
| conformance/reading/read-pixels-test.html | 194/16 | 194/16 |
| conformance/textures/misc/texture-fakeblack.html | 4/2 | 4/2 |
| conformance/rendering/blending.html | 8/5 | 7/7 |
| webgl/float-extensions-and-read-pixels.html (new) | 1/2 | 3/0 |
| webgl/texture-validation-overflow.html (new) | CRASH | 6/0 |

**Commit 7's share:** mine3, without it, scored 97/15 on oes-texture-float and 114/8 on oes-texture-half-float in the full run. So commit 7 accounts for 16 and 8 of those passes.

**blending.html:**

- Subtests 3, 4, 6, 7 and 9 read black on both builds, because each draws into a float target that the fake-black path replaced.
- **Subtest 10** draws to RGBA32F and reads it back as FLOAT:
  - **Base:** it did not expose EXT_float_blend, so the test expected INVALID_OPERATION. It got one, but from readPixels refusing FLOAT, not from blend validation, so it passed by accident.
  - **mine5:** it exposes EXT_float_blend, so the test expects no error. readPixels now refuses FLOAT because the attachment became RGBA8, so subtests 10 and 11 fail.
- Removing the fake-black path should turn all of these into passes.

**ext-float-blend:** this file treats "supported" as "blending allowed". Servo has no draw-time float32 blend validation for when the extension is missing. That gap predates these fixes, and the Linux metadata expects its subtests 2 and 4 to fail.
