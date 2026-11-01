# BeautifulRust review verification

Source: `ef0e603` plus the review fixes in this commit, on
`claude/platinum-theorem-interface-990ec3` for Platinum PR #17. The review report
is [here](../../beautifulrust-review-2026-10-31.md).

Validation used macOS arm64, Rust 1.97.1, the repository media wrapper,
`CARGO_TARGET_DIR=/Volumes/SSD Samsung/cargo-targets/frame`, four build jobs,
`CARGO_INCREMENTAL=0` and SSD-backed `TMPDIR`. Cargo invocations also set
`--config 'cache.auto-clean-frequency="never"'` to avoid unrelated global cache
cleanup. No production profile was opened.

| Check | Result |
| --- | --- |
| `script/media-env cargo test --locked -p frame --bin frame -- --test-threads=1` | **303 passed, 2 ignored**, 14.80 seconds test execution |
| `script/media-env cargo build --locked --features snapshot --bin frame --example canvas_check --example site_panel_check` | **Passed**; both previously broken example roots compiled |
| `script/media-env cargo clippy --locked -p frame --bin frame --no-deps -- -W clippy::pedantic` | **Completed**, 599 warnings; not warning-free |
| `cargo fmt -p frame -- --check` | Existing broad formatting differences; no unrelated reformat applied |
| `git diff --check` | Passed |
| Native workspace flow | [Passed](report.json); public browsing, supplied-transcript insertion, same-note promotion and compact board composer |
| Focused website modifier release | [Passed](web-modifier-release.json); quick tap stays idle, hold starts once, release stops once and commits |
| Fresh-process restore | [Passed](restore-report.json); board, shared note and edited shortcut restored |
| Native window focus | [Active](native-focus.json) during insertion checks |

The application suite includes CRC corruption, compressed expansion budgets,
absolute/escaping/deep archive paths, extraction failure, stale drag destinations,
concurrent sidebar edits, unique extraction files, stale read cancellation,
rendered summary/proposal clicks and typing, attachment snapshot consistency,
table order/width/sort retention and note promotion from expanded Notes.

The first full test pass exposed the archive library's absolute-path normalization
and a clipped proposal acceptance control. Those were corrected. A later focused
failure was a fixture writing into a different directory from the CRM records;
the final full suite above passed after correction. Earlier compiler invocations
started while the manifest was still changing and were superseded by the final
locked commands. They are not counted as successful tests.

The first native attempt reported an inactive window and stopped. The successful
second run and fresh-process restore produced the reports here. The loopback
fixture server was stopped afterward. Clippy's warnings include existing unused
code, pedantic formatting/API suggestions and numeric casts; these were reviewed
without wholesale edits outside the defects. Existing dependency warnings and
an oversized debug unwind-table linker warning also remain.

## Rendered evidence

- [Index proposal](index-proposal-fixture.png): native rendering with the existing
  sample CRM records in an isolated fixture profile. The visible action flow is
  exercised by the headless GPUI click/type regression; this screenshot is not a
  claim that a model generated a proposal or that native mouse clicks were used.
- [Website release](web-modifier-release.png): real Servo textarea containing the
  supplied transcript after release through GPUI dispatch.
- [Board note](board-note.png): the same note/editor promoted from expanded Notes,
  with the compact composer restored.

## Reproduction and limits

Use the [native reproduction instructions](../zephium-spatial-2026-10-30/checks.md#reproducing-the-native-flow)
with a fresh output directory. The updated harness also renders the Index
proposal and tests website modifier release. Check JSON outcomes, not only process
exit status. The successful output directory for this run was
`/Volumes/SSD Samsung/tmp/platinum-beautiful-native-20261031-2`.

Speech tests supply the left-key side and synthetic capture/recognition, then use
real modifier-release dispatch, the focused production Page and production text
insertion. They do **not** establish physical-key or live microphone behavior.
No live paid provider, audible playback, release signing or deployment was tested.
The ignored model/audio oracle was not rerun in this review. ZIP budgets apply
per archive, not as a global filesystem quota; an already executing Quick Look
process is not forcibly killed when its view is left.
