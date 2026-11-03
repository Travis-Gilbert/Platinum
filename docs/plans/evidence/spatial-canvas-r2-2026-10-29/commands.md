# R2 command receipts

Every Servo command ran from `/Volumes/SSD Samsung/frame-deps/servo-r2` with
`TMPDIR="/Volumes/SSD Samsung/tmp"`, `CARGO_TARGET_DIR="/Volumes/SSD Samsung/cargo-targets/servo-r1"`,
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_PROFILE_DEV_STRIP=none`,
`CARGO_PROFILE_TEST_STRIP=none` and `--locked`. Each cargo command was started through a small
runner (`r2run.py` in this folder; `drive.sh` is the loop that ran the first pass of tests) that checked that at least 6 GiB were free on
`/Volumes/SSD Samsung`, ran the command with stdout and stderr merged into the `.log`, and wrote the
`.json` receipt (command, directory, environment, exit code, wall time, free space before and after).
Wall time is the whole cargo invocation, including cargo's own startup and the test process.
Free space before each command is in its `.json`; the lowest value seen was
51.8 GiB.

## environment

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: not timed (instant). [Output](environment.log).

```sh
uname -a
rustc -V
cargo -V
git rev-parse HEAD
git branch --show-current
git tag --points-at HEAD
git status --short | wc -l
git log --format='%h %s' 7a28fd822be..HEAD
```

## target-size-before

Directory: `/Volumes/SSD Samsung/frame-worktrees/r2-servo-evidence`. Exit: **0**. Wall time: not timed (instant). [Output](target-size-before.txt).

```sh
du -sh '/Volumes/SSD Samsung/cargo-targets/servo-r1'
```

## servo-build

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **205.452 s**. [Output](servo-build.log). Free space before: 54.8 GiB, after: 52.9 GiB.

```sh
cargo test --locked -p servo --no-run --test request_activity --test webview --lib
```

## target-size-after-build

Directory: `/Volumes/SSD Samsung/frame-worktrees/r2-servo-evidence`. Exit: **0**. Wall time: not timed (instant). [Output](target-size-after-build.txt).

```sh
du -sh '/Volumes/SSD Samsung/cargo-targets/servo-r1'
```

## request_activity-test_fetch_upload_after_load

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **10.712 s**. [Output](request_activity-test_fetch_upload_after_load.log).

```sh
cargo test --locked -p servo --test request_activity test_fetch_upload_after_load -- --exact
```

## request_activity-test_xhr_upload_after_load

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.426 s**. [Output](request_activity-test_xhr_upload_after_load.log).

```sh
cargo test --locked -p servo --test request_activity test_xhr_upload_after_load -- --exact
```

## request_activity-test_abort_fetch_upload

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.793 s**. [Output](request_activity-test_abort_fetch_upload.log).

```sh
cargo test --locked -p servo --test request_activity test_abort_fetch_upload -- --exact
```

## request_activity-test_fetch_network_error

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.565 s**. [Output](request_activity-test_fetch_network_error.log).

```sh
cargo test --locked -p servo --test request_activity test_fetch_network_error -- --exact
```

## request_activity-test_subresource_redirect

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.108 s**. [Output](request_activity-test_subresource_redirect.log).

```sh
cargo test --locked -p servo --test request_activity test_subresource_redirect -- --exact
```

## request_activity-test_document_redirect

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.043 s**. [Output](request_activity-test_document_redirect.log).

```sh
cargo test --locked -p servo --test request_activity test_document_redirect -- --exact
```

## request_activity-test_navigate_away_during_upload

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **2.275 s**. [Output](request_activity-test_navigate_away_during_upload.log).

```sh
cargo test --locked -p servo --test request_activity test_navigate_away_during_upload -- --exact
```

## request_activity-test_drop_webview_finishes_upload

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **2.137 s**. [Output](request_activity-test_drop_webview_finishes_upload.log).

```sh
cargo test --locked -p servo --test request_activity test_drop_webview_finishes_upload -- --exact
```

## request_activity-test_data_url_fetch

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.705 s**. [Output](request_activity-test_data_url_fetch.log).

```sh
cargo test --locked -p servo --test request_activity test_data_url_fetch -- --exact
```

## webview-test_negative_resize_to_request

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **10.015 s**. [Output](webview-test_negative_resize_to_request.log).

```sh
cargo test --locked -p servo --test webview test_negative_resize_to_request -- --exact
```

## webview-test_page_zoom

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.446 s**. [Output](webview-test_page_zoom.log).

```sh
cargo test --locked -p servo --test webview test_page_zoom -- --exact
```

## webview-test_pinch_zoom_update_dom_visual_viewport

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.360 s**. [Output](webview-test_pinch_zoom_update_dom_visual_viewport.log).

```sh
cargo test --locked -p servo --test webview test_pinch_zoom_update_dom_visual_viewport -- --exact
```

## webview-test_resize_webview_zero

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.282 s**. [Output](webview-test_resize_webview_zero.log).

```sh
cargo test --locked -p servo --test webview test_resize_webview_zero -- --exact
```

## webview-test_set_viewport_css_resize

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.406 s**. [Output](webview-test_set_viewport_css_resize.log).

```sh
cargo test --locked -p servo --test webview test_set_viewport_css_resize -- --exact
```

## webview-test_set_viewport_scale_only

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.318 s**. [Output](webview-test_set_viewport_scale_only.log).

```sh
cargo test --locked -p servo --test webview test_set_viewport_scale_only -- --exact
```

## webview-test_set_viewport_then_legacy_resize_and_scale

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.216 s**. [Output](webview-test_set_viewport_then_legacy_resize_and_scale.log).

```sh
cargo test --locked -p servo --test webview test_set_viewport_then_legacy_resize_and_scale -- --exact
```

## webview-test_viewport_meta_tag_initial_scale

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.178 s**. [Output](webview-test_viewport_meta_tag_initial_scale.log).

```sh
cargo test --locked -p servo --test webview test_viewport_meta_tag_initial_scale -- --exact
```

## webview-test_webview_resize_interactivity

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **2.029 s**. [Output](webview-test_webview_resize_interactivity.log).

```sh
cargo test --locked -p servo --test webview test_webview_resize_interactivity -- --exact
```

## servo-lib-webview_delegate

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.620 s**. [Output](servo-lib-webview_delegate.log).

```sh
cargo test --locked -p servo --lib webview_delegate
```

## repeat2-request_activity-test_fetch_upload_after_load

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **3.767 s**. [Output](repeat2-request_activity-test_fetch_upload_after_load.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_fetch_upload_after_load -- --exact
```

## repeat2-request_activity-test_xhr_upload_after_load

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.103 s**. [Output](repeat2-request_activity-test_xhr_upload_after_load.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_xhr_upload_after_load -- --exact
```

## repeat2-request_activity-test_abort_fetch_upload

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.559 s**. [Output](repeat2-request_activity-test_abort_fetch_upload.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_abort_fetch_upload -- --exact
```

## repeat2-request_activity-test_fetch_network_error

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.072 s**. [Output](repeat2-request_activity-test_fetch_network_error.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_fetch_network_error -- --exact
```

## repeat2-request_activity-test_subresource_redirect

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.409 s**. [Output](repeat2-request_activity-test_subresource_redirect.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_subresource_redirect -- --exact
```

## repeat2-request_activity-test_document_redirect

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.283 s**. [Output](repeat2-request_activity-test_document_redirect.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_document_redirect -- --exact
```

## repeat2-request_activity-test_navigate_away_during_upload

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.354 s**. [Output](repeat2-request_activity-test_navigate_away_during_upload.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_navigate_away_during_upload -- --exact
```

## repeat2-request_activity-test_drop_webview_finishes_upload

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.400 s**. [Output](repeat2-request_activity-test_drop_webview_finishes_upload.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_drop_webview_finishes_upload -- --exact
```

## repeat2-request_activity-test_data_url_fetch

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.062 s**. [Output](repeat2-request_activity-test_data_url_fetch.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_data_url_fetch -- --exact
```

## repeat3-request_activity-test_fetch_upload_after_load

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.390 s**. [Output](repeat3-request_activity-test_fetch_upload_after_load.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_fetch_upload_after_load -- --exact
```

## repeat3-request_activity-test_xhr_upload_after_load

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.075 s**. [Output](repeat3-request_activity-test_xhr_upload_after_load.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_xhr_upload_after_load -- --exact
```

## repeat3-request_activity-test_abort_fetch_upload

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.038 s**. [Output](repeat3-request_activity-test_abort_fetch_upload.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_abort_fetch_upload -- --exact
```

## repeat3-request_activity-test_fetch_network_error

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.868 s**. [Output](repeat3-request_activity-test_fetch_network_error.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_fetch_network_error -- --exact
```

## repeat3-request_activity-test_subresource_redirect

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **2.059 s**. [Output](repeat3-request_activity-test_subresource_redirect.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_subresource_redirect -- --exact
```

## repeat3-request_activity-test_document_redirect

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **2.185 s**. [Output](repeat3-request_activity-test_document_redirect.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_document_redirect -- --exact
```

## repeat3-request_activity-test_navigate_away_during_upload

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.359 s**. [Output](repeat3-request_activity-test_navigate_away_during_upload.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_navigate_away_during_upload -- --exact
```

## repeat3-request_activity-test_drop_webview_finishes_upload

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.230 s**. [Output](repeat3-request_activity-test_drop_webview_finishes_upload.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_drop_webview_finishes_upload -- --exact
```

## repeat3-request_activity-test_data_url_fetch

Directory: `/Volumes/SSD Samsung/frame-deps/servo-r2`. Exit: **0**. Wall time: **1.474 s**. [Output](repeat3-request_activity-test_data_url_fetch.log). Flake-check repeat.

```sh
cargo test --locked -p servo --test request_activity test_data_url_fetch -- --exact
```

## target-size-after-tests

Directory: `/Volumes/SSD Samsung/frame-worktrees/r2-servo-evidence`. Exit: **0**. Wall time: not timed (instant). [Output](target-size-after-tests.txt).

```sh
du -sh '/Volumes/SSD Samsung/cargo-targets/servo-r1'
```

## disk-final

Directory: `/Volumes/SSD Samsung/frame-worktrees/r2-servo-evidence`. Exit: **0**. Wall time: not timed (instant). [Output](disk-final.txt).

```sh
df -h '/Volumes/SSD Samsung'
```
