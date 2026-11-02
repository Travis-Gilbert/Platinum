# ub-mode review of profile storage and address suggestions

Installed `unclebob/ub-mode` in `~/.codex/skills/ub-mode` and applied it to the
uncommitted implementation described in [the plan](../../rustyred-profile-search.md).
The full [action log](actions.md) records decisions, tool revisions and recovery
from tool issues. Existing unrelated changes were preserved.

## Changes and checks

- Separated legacy catalog loading, row conversion and record identity from the
  atomic migration commit. Default and imported profiles now use one directory
  rule for both migration and ordinary operation.
- Separated writer ownership, database opening/indexing and suggestion matching.
  The single writer, migration marker and transaction boundaries are unchanged.
- Removed duplicated form-import processing and unified the existing allowed
  fields across Chromium, Firefox and explicit form filling.
- Added nine behavior tests, first passing them against the original code and
  then against the refactor. Two follow-up mutation cases bring the total to 34
  passing profile tests, with the three existing
  opt-in native credential tests still ignored. Strict profile Clippy passes.

| Function | CRAP before | CRAP after |
| --- | ---: | ---: |
| Legacy migration orchestration | 38.4 | 7.0 |
| ProfileStore opening | 18.0 | 4.0 |
| Suggestion lookup/ranking | 26.7 | 9.0 |
| Chromium form import | 12.0 | 3.0 |
| Firefox form import | 12.0 | 3.0 |

Every function in the new storage and legacy migration modules is below CRAP 10,
using actual LLVM line coverage from the 32-test refactor run. The shared form-import helper scores 6.0.
See [before](crap-before.txt), [after](crap-after.txt) and
[adapter measurements](adapters-after.txt).

Dryer no longer reports the duplicated form import. Its remaining 0.88 candidate
compares password-reading and form-filling script builders: normalization hides
their different embedded scripts and security behavior. Those remain separate.

## Mutation results

The original run caught 40 of 57 mutations (70.2%). After refactoring, additional
behavior tests and two focused survivor reruns, the combined snapshots show
43 killed and 9 survived out of 52 sites (82.7%). All 15 legacy-migration sites
are killed. The number of sites changed because the refactor removed repeated
and avoidable conditions; these are measurements of different source versions.

The focused tests confirmed that the suite rejects both broken exact-address
priority and accidental persistence of `ControlEvent` audit records. The tests
also catch default-profile migration, absent active selection, zero result
limits, clearing at the exact timestamp, search-path identity and history merging.

The nine [survivors](survivors.txt) are retained and classified:

- Seven change numeric fallbacks for missing catalog/history fields. Valid
  records created through the public API contain those fields; malformed-file
  behavior remains outside these tests. These are coverage gaps, not a claim
  that corrupted files are safe.
- One changes the minimum full-text hit limit from one to zero when the profile
  contains no candidate records; the user sees an empty result either way.
- One retains an expired weak owner until replacement rather than pruning it.
  It changes registry cleanup, not a user's stored records or writer ownership.

See [the full run](mutation-after.txt), [exact-address rerun](mutation-exact-address.txt),
[control-event rerun](mutation-control-events.txt), and
[combined totals](mutation-final.json). Mutator correctly returns a nonzero status
while survivors remain. This pass does not claim the skill's zero-survivor gate.

## Boundaries

This is not an all-green quality report for the entire browser. The broader
complexity scan includes older UI and import functions above the threshold.
The full application test run began rebuilding Servo dependencies and was
stopped because the external drive had only 10 GiB free at entry. This pass
changes only the profile crate. The previous 66 application tests and native
restart/UI acceptance remain recorded in the original plan; they were not
rerun on this refactor. No installed browser or personal profile was opened.

Mutator's definition of a killed mutant includes compilation failure and timeout,
so its score is a diagnostic aid, not proof that every kill is a behavioral
assertion. Equivalent mutants and unreachable fallback paths need manual review.

## Domain terms

- Installation: the storage root containing one shared `browser.RR` writer.
- Profile: a catalog identity whose records and Servo directories stay isolated.
- Library entry: a bookmark or history title/URL offered for navigation.
- Migration marker: the schema record committed atomically with legacy records.
- History fence: the clear timestamp that rejects earlier queued visits.
- Suggestion action: a typed Visit, Search or Switch to tab operation; Ask is
  explicitly separate.

At the end of the quality pass, no commit or push had been made. Subsequent publication is recorded in the linked plan. The standing instruction against an unauthorized
push to main takes precedence over the skill's automatic push instruction.

## Reproduction

The three tools are installed in the dedicated Python environment at
`/Volumes/SSD Samsung/agent-tools/ub-mode/venv`. Mutator has the small, recorded
[Rust scan fix](mutator-rust-scan.patch); its 76 self-tests pass.

From the Platinum checkout:

```sh
cargo test --locked -p frame-profile
cargo clippy --locked -p frame-profile --all-targets --no-deps -- -D warnings
mkdir -p target/coverage/rust
CARGO_TARGET_DIR='/Volumes/SSD Samsung/agent-tools/ub-mode/profile-coverage' \
  CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo llvm-cov --locked -p frame-profile --lcov \
  --output-path target/coverage/rust/lcov.info
'/Volumes/SSD Samsung/agent-tools/ub-mode/venv/bin/crapper' \
  --use-existing-coverage --threshold 9.999 \
  crates/profile/src/storage.rs crates/profile/src/legacy.rs
'/Volumes/SSD Samsung/agent-tools/ub-mode/venv/bin/mutator' \
  --no-coverage --mutate-all --max-workers 1 --timeout-factor 100 \
  --test-command 'cargo test --locked -p frame-profile' \
  crates/profile/src/legacy.rs crates/profile/src/storage.rs
```
