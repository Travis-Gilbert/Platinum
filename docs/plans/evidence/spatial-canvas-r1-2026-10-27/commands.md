# R1 command receipts

All commands use `TMPDIR="/Volumes/SSD Samsung/tmp"`. Servo commands use `CARGO_TARGET_DIR="/Volumes/SSD Samsung/cargo-targets/servo-r1"`, `CARGO_PROFILE_DEV_DEBUG=0`, and `CARGO_PROFILE_TEST_DEBUG=0`. All but the initial `servo-webview` attempt also use `CARGO_PROFILE_DEV_STRIP=none` and `CARGO_PROFILE_TEST_STRIP=none`. Frame commands use the required wrapper from the Frame worktree root. The retained Node script is `probe-unit.cjs`; its original timed invocation used the identical temporary copy.

## servo-webview

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **101**. Wall time: **5.165 s**. [Output](servo-webview.log).

```sh
cargo test -p servo --test webview -- test_set_viewport test_resize test_negative_resize test_webview_resize test_page_zoom test_viewport test_pinch_zoom --test-threads=1
```

## probe-unit

Directory: `/Volumes/SSD Samsung/frame-worktrees/canvas-viewport`. Exit: **0**. Wall time: **0.046 s**. [Output](probe-unit.log).

```sh
node '/Volumes/SSD Samsung/tmp/r1-probe-test.cjs'
```

## servo-webview-unstripped

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **101**. Wall time: **254.853 s**. [Output](servo-webview-unstripped.log).

```sh
cargo test -p servo --test webview -- test_set_viewport test_resize test_negative_resize test_webview_resize test_page_zoom test_viewport test_pinch_zoom --test-threads=1
```

## servo-scale-only

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **24.172 s**. [Output](servo-scale-only.log).

```sh
cargo test -p servo --test webview test_set_viewport_scale_only -- --exact
```

## test_set_viewport_css_resize

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **2.459 s**. [Output](test_set_viewport_css_resize.log).

```sh
cargo test -p servo --test webview test_set_viewport_css_resize -- --exact
```

## test_set_viewport_then_legacy_resize_and_scale

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **1.425 s**. [Output](test_set_viewport_then_legacy_resize_and_scale.log).

```sh
cargo test -p servo --test webview test_set_viewport_then_legacy_resize_and_scale -- --exact
```

## test_negative_resize_to_request

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **1.724 s**. [Output](test_negative_resize_to_request.log).

```sh
cargo test -p servo --test webview test_negative_resize_to_request -- --exact
```

## test_resize_webview_zero

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **1.739 s**. [Output](test_resize_webview_zero.log).

```sh
cargo test -p servo --test webview test_resize_webview_zero -- --exact
```

## test_webview_resize_interactivity

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **1.780 s**. [Output](test_webview_resize_interactivity.log).

```sh
cargo test -p servo --test webview test_webview_resize_interactivity -- --exact
```

## test_page_zoom

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **1.378 s**. [Output](test_page_zoom.log).

```sh
cargo test -p servo --test webview test_page_zoom -- --exact
```

## test_viewport_meta_tag_initial_scale

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **1.564 s**. [Output](test_viewport_meta_tag_initial_scale.log).

```sh
cargo test -p servo --test webview test_viewport_meta_tag_initial_scale -- --exact
```

## test_pinch_zoom_update_dom_visual_viewport

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **1.825 s**. [Output](test_pinch_zoom_update_dom_visual_viewport.log).

```sh
cargo test -p servo --test webview test_pinch_zoom_update_dom_visual_viewport -- --exact
```

## zoom-dry-run

Directory: `/Volumes/SSD Samsung/frame-worktrees/canvas-viewport`. Exit: **0**. Wall time: **0.179 s**. [Output](zoom-dry-run.log).

```sh
python3 tests/canvas/run-checks.py --only zoom --dry-run --evidence docs/plans/evidence/spatial-canvas-r1-2026-10-27/zoom-dry-run
```

## frame-tests

Directory: `/Volumes/SSD Samsung/frame-worktrees/canvas-viewport`. Exit: **0**. Wall time: **849.342 s**. [Output](frame-tests.log).

```sh
'/Volumes/SSD Samsung/cargo-targets/frame-canvas.sh' cargo test --locked --bin frame
```

## canvas-build

Directory: `/Volumes/SSD Samsung/frame-worktrees/canvas-viewport`. Exit: **0**. Wall time: **1500.973 s**. [Output](canvas-build.log).

```sh
'/Volumes/SSD Samsung/cargo-targets/frame-canvas.sh' cargo build --locked --example canvas_check
```

## servo-visual-viewport-regression

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **101**. Wall time: **21.066 s**. [Output](servo-visual-viewport-regression.log).

```sh
cargo test -p servo --test webview test_set_viewport_scale_only -- --exact
```

## final-test_set_viewport_scale_only

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **20.909 s**. [Output](final-test_set_viewport_scale_only.log).

```sh
cargo test -p servo --test webview test_set_viewport_scale_only -- --exact
```

## final-test_set_viewport_css_resize

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **1.286 s**. [Output](final-test_set_viewport_css_resize.log).

```sh
cargo test -p servo --test webview test_set_viewport_css_resize -- --exact
```

## final-test_set_viewport_then_legacy_resize_and_scale

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **1.005 s**. [Output](final-test_set_viewport_then_legacy_resize_and_scale.log).

```sh
cargo test -p servo --test webview test_set_viewport_then_legacy_resize_and_scale -- --exact
```

## final-test_negative_resize_to_request

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **0.781 s**. [Output](final-test_negative_resize_to_request.log).

```sh
cargo test -p servo --test webview test_negative_resize_to_request -- --exact
```

## final-test_resize_webview_zero

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **0.766 s**. [Output](final-test_resize_webview_zero.log).

```sh
cargo test -p servo --test webview test_resize_webview_zero -- --exact
```

## final-test_webview_resize_interactivity

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **0.851 s**. [Output](final-test_webview_resize_interactivity.log).

```sh
cargo test -p servo --test webview test_webview_resize_interactivity -- --exact
```

## final-test_page_zoom

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **0.754 s**. [Output](final-test_page_zoom.log).

```sh
cargo test -p servo --test webview test_page_zoom -- --exact
```

## final-test_viewport_meta_tag_initial_scale

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **0.767 s**. [Output](final-test_viewport_meta_tag_initial_scale.log).

```sh
cargo test -p servo --test webview test_viewport_meta_tag_initial_scale -- --exact
```

## final-test_pinch_zoom_update_dom_visual_viewport

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r1`. Exit: **0**. Wall time: **0.866 s**. [Output](final-test_pinch_zoom_update_dom_visual_viewport.log).

```sh
cargo test -p servo --test webview test_pinch_zoom_update_dom_visual_viewport -- --exact
```

## frame-tests-final

Directory: `/Volumes/SSD Samsung/frame-worktrees/canvas-viewport`. Exit: **0**. Wall time: **37.542 s**. [Output](frame-tests-final.log).

```sh
'/Volumes/SSD Samsung/cargo-targets/frame-canvas.sh' cargo test --locked --bin frame
```

## canvas-build-final

Directory: `/Volumes/SSD Samsung/frame-worktrees/canvas-viewport`. Exit: **0**. Wall time: **14.445 s**. [Output](canvas-build-final.log).

```sh
'/Volumes/SSD Samsung/cargo-targets/frame-canvas.sh' cargo build --locked --example canvas_check
```
