# R2 Servo verification, 2026-10-29

R2's Servo half passes every test it was asked to pass on this Mac. **21 of 21
test functions passed**, with no failures, and a repeat of the nine
`request_activity` tests passed on both extra passes (27 of 27 runs). No code
was changed and nothing was committed; this folder is the only thing written.

[Every command, exit code and wall time](commands.md) · [Environment](environment.log)

## What was tested

- **Revision:** `eca10114b4f3d43feab0ce5b385cbd22711e2428`, the tag
  `archive/Travis-Gilbert/request-activity`. Checkout
  `/Volumes/SSD Samsung/frame-deps/servo-r2`, local branch `feat/request-activity`,
  `git status` empty before and after. HEAD did not move.
- It sits two commits above R1's `7a28fd822be`: `35948a4a1ac` (feat: report when
  each request starts and ends) and `eca10114b4f` (fix: finish fetches an embedder
  navigation or webview drop used to leave running). Both are contained in the
  local remote-tracking refs `origin/main` and `origin/consolidated` of the shared
  repository. I did not fetch, so that is what the local refs say, not a check
  against GitHub.
- **Machine:** Darwin 27.0.0, arm64 (`RELEASE_ARM64_T6000`). `rustc 1.97.1 (8bab26f4f 2026-07-14)`
  and `cargo 1.97.1`, both run from the checkout.
- **Date:** 2026-10-29, between about 17:53 and 17:59 EDT.

## Build

`cargo test --locked -p servo --no-run --test request_activity --test webview --lib`
with the brief's environment (`TMPDIR`, `CARGO_TARGET_DIR`, `DEBUG=0`, `STRIP=none`
for dev and test profiles). **Exit 0, 205.452 s wall time** (cargo reported
"Finished `test` profile in 3m 25s"). [Log](servo-build.log).

I built only the three test targets I was going to run (the library tests,
`request_activity` and `webview`), not every test target in the `servo` package.
The runs afterwards recompiled nothing, so the feature set matched.

The build log has 36 `warning` lines, mostly unused imports and dead-code notes in
`servo-script`, `constellation` and `paint`, plus one linker warning
(`__eh_frame section too large`). I looked at the two warned files that the R2
diff also touches, `document.rs` and `script_thread.rs`; the warned lines are
not in the diff. I did not trace the other warnings.

| Target dir `cargo-targets/servo-r1` | `du -sh` |
| --- | ---: |
| Before the build | 3.3G |
| After the build | 4.0G |
| After all tests | 4.0G |

Free space on `/Volumes/SSD Samsung` was 54.8 GiB before the build and 52 GiB at
the end (`df -h` in `disk-final.txt`: 52Gi available when the tests finished; it read
51Gi a few minutes later). That is volume-wide: other agents were building
on the same disk during the run, so the 0.7 GiB growth of the target dir is the
only part I can attribute to this work. The lowest free value before any command
was 51.8 GiB, far above the 6 GiB stop line.

## Results

Each row is one cargo invocation with `--exact`, one test per process. "Wall"
is the whole invocation in seconds, including cargo startup. "Test" is the time
the test harness printed.

### `request_activity.rs`, 9 tests

| Test | Result | Wall (s) | Test (s) |
| --- | --- | ---: | ---: |
| `test_fetch_upload_after_load` | Pass | 10.712 | 0.39 |
| `test_xhr_upload_after_load` | Pass | 1.426 | 0.46 |
| `test_abort_fetch_upload` | Pass | 1.793 | 0.25 |
| `test_fetch_network_error` | Pass | 1.565 | 0.40 |
| `test_subresource_redirect` | Pass | 1.108 | 0.23 |
| `test_document_redirect` | Pass | 1.043 | 0.21 |
| `test_navigate_away_during_upload` | Pass | 2.275 | 0.64 |
| `test_drop_webview_finishes_upload` | Pass | 2.137 | 0.34 |
| `test_data_url_fetch` | Pass | 1.705 | 0.48 |

The brief lists eight tests for this file. The file has **nine**. The extra one is
`test_drop_webview_finishes_upload`, which drops the webview while an upload is
held open and asserts that the upload gets exactly one finished. `git log -S` puts
its addition in `eca10114b4f`, the second commit. I ran all nine.

### R1's viewport tests in `webview.rs`, 9 tests

The brief lists nine and the file has nine functions with exactly those names, so
there was nothing to add or drop.

| Test | Result | Wall (s) | Test (s) |
| --- | --- | ---: | ---: |
| `test_negative_resize_to_request` | Pass | 10.015 | 0.40 |
| `test_page_zoom` | Pass | 1.446 | 0.27 |
| `test_pinch_zoom_update_dom_visual_viewport` | Pass | 1.360 | 0.43 |
| `test_resize_webview_zero` | Pass | 1.282 | 0.31 |
| `test_set_viewport_css_resize` | Pass | 1.406 | 0.36 |
| `test_set_viewport_scale_only` | Pass | 1.318 | 0.42 |
| `test_set_viewport_then_legacy_resize_and_scale` | Pass | 1.216 | 0.29 |
| `test_viewport_meta_tag_initial_scale` | Pass | 1.178 | 0.30 |
| `test_webview_resize_interactivity` | Pass | 2.029 | 0.53 |

### Interception unit tests

`cargo test --locked -p servo --lib webview_delegate`, one invocation, 3 tests.

| Tests | Result | Wall (s) | Test (s) |
| --- | --- | ---: | ---: |
| `webview_delegate::test::*` (3 tests: `test_allow_deny_request`, `test_authentication_request`, `test_web_resource_load`) | Pass (3 passed, 0 failed) | 1.620 | 0.00 |

### Flake check on `request_activity.rs`

Not asked for, but cheap: I ran the nine tests two more times, each in its own
process, after the first pass. Results are first pass / second / third, wall seconds
likewise. [Logs](commands.md) are named `repeat2-*` and `repeat3-*`.

| Test | Result | Wall (s) |
| --- | --- | --- |
| `test_fetch_upload_after_load` | Pass / Pass / Pass | 10.71 / 3.77 / 1.39 |
| `test_xhr_upload_after_load` | Pass / Pass / Pass | 1.43 / 1.10 / 1.07 |
| `test_abort_fetch_upload` | Pass / Pass / Pass | 1.79 / 1.56 / 1.04 |
| `test_fetch_network_error` | Pass / Pass / Pass | 1.57 / 1.07 / 1.87 |
| `test_subresource_redirect` | Pass / Pass / Pass | 1.11 / 1.41 / 2.06 |
| `test_document_redirect` | Pass / Pass / Pass | 1.04 / 1.28 / 2.19 |
| `test_navigate_away_during_upload` | Pass / Pass / Pass | 2.27 / 1.35 / 1.36 |
| `test_drop_webview_finishes_upload` | Pass / Pass / Pass | 2.14 / 1.40 / 1.23 |
| `test_data_url_fetch` | Pass / Pass / Pass | 1.70 / 1.06 / 1.47 |

All 27 runs passed. Three passes is a small sample, so this is the absence of a
failure I could see, not proof the tests are stable.

## Open items

- **No failures.** Nothing to record as failed or flaky.
- **Slow first launches.** The first run of each test binary took about 10 s
  (`test_fetch_upload_after_load` 10.712 s, `test_negative_resize_to_request`
  10.015 s) against 1 to 2 s for the rest. The test harness itself reported
  under 0.5 s for both, and no recompilation happened. I did not investigate
  the cause. Later runs of `test_fetch_upload_after_load` took 3.77 s and 1.39 s.
- **Not run, by the brief's rules:** `network_manager.rs` and net's own
  integration tests (they bind `0.0.0.0`), and `cargo check`. In
  `request_activity.rs` every server binds `127.0.0.1` (`make_loopback_server`,
  and a `127.0.0.1:0` listener closed before the network-error test); I checked
  that by reading the file, not by watching sockets.
- **Not run, out of scope for this task:** the other 36 tests in `webview.rs`,
  the rest of the `servo` package's tests and the other crates' tests, WPT,
  Frame built against this Servo, and any native app check. A pass here says
  that these 21 tests pass at this revision on this Mac, nothing wider.
- **Diff reach.** The two commits change more than the files the original brief
  listed as likely: `components/script/dom/document/document.rs`,
  `components/script/event_loop/script_thread.rs` and `components/servo/webview.rs`
  as well (from `git diff --stat 7a28fd822be..HEAD`, read only). The brief allowed
  anything under `servo-r2`, so this is a note for review, not a violation.
  Judging by their names and the second commit's message,
  `test_navigate_away_during_upload` and `test_drop_webview_finishes_upload` are
  the tests that exercise the script-side cancel; I did not trace the code to
  confirm that.
- The runner and driver (`r2run.py`, `drive.sh`) are kept here as the R1 folder
  kept `probe-unit.cjs`. `target-size-*.txt` and `disk-final.txt` hold the
  `du` and `df` output. Nothing was committed or pushed, and no process I did not
  start was touched.
