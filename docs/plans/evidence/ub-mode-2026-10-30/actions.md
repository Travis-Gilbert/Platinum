# ub-mode application log — 2026-10-30

Scope: the RustyRed profile migration, address suggestions and Servo persistence
changes in `docs/plans/rustyred-profile-search.md`. Preserve unrelated changes.

1. Read the skill-installer instructions and upstream ub-mode skill/README to
   establish the installation path and requested quality gates.
2. Checked the branch, upstream and dirty tree. The checkout still contains the
   previous implementation and unrelated work; no reset, pull or broad staging.
3. Installed `unclebob/ub-mode` from `master` with the official installer at
   `~/.codex/skills/ub-mode`. Read the installed instructions before applying them.
4. Read companion tool READMEs, then cloned crapper, dryer and mutator beneath
   `/Volumes/SSD Samsung/agent-tools/ub-mode`. Installed them into a dedicated
   Python 3.13 virtual environment, without modifying application dependencies.
5. Inspected storage, migration, address classification, tests and mutation worker
   isolation. Chose explicit task files rather than `--changed`, which would
   include unrelated work. Mutation workers must leave original files intact.
6. Checked disk headroom (10 GiB external, 12 GiB internal). Started installation
   of cargo-llvm-cov for focused profile coverage. A full instrumented Servo
   rebuild does not fit the available external space; no unrelated artifacts
   will be deleted to force it.
7. Ran crapper complexity-only and dryer at the skill's 0.8 threshold on the
   explicit task files to locate meaningful simplifications before editing.

The standing user instruction against an unauthorized push to main takes
precedence over the skill's automatic push instruction. No push to main is
part of this run. Existing uncommitted work and active worktrees are retained.

8. Collected real LLVM coverage with all 23 profile tests passing. Report
   generation initially lacked its output directory, then needed the explicit
   `-p frame-profile` report scope; corrected both and verified nonempty LCOV
   before using crapper. Saved the measured baseline rather than the invalid
   empty-report scores.
9. Stopped only this run's fresh application build when it began rebuilding
   Servo dependencies. Kept the previously tested UI unchanged and retained
   its existing native acceptance evidence; no full new UI coverage claim.
10. Mutator initially crashed while scanning unrelated JavaScript fixtures for
    a Rust mutation. Applied a local tool fix to skip JS importer expansion for
    non-JS source files. The exact patch is recorded here. Its 76 self-tests pass
    using a no-space Python invocation; the upstream tests interpolate an
    unquoted interpreter path and fail when that path contains spaces.
11. Restarted all 57 storage/migration mutation sites with the real profile test
    command, one private worker at a time. Drafted the refactor separately while
    the baseline workers run, keeping their input sources unchanged.
12. Prepared behavior tests for the two legacy survivors (missing active setting
    and default-profile migration), plus ranking, title updates, invalid URLs,
    credential-label deletion and pending/committed import deduplication.

Tool revisions:
- ub-mode: `57a8bdd28e45ed41900ace0bafa2ff76dd9ed70f`
- crapper: `6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d`
- dryer: `66ff6d21a42c04afcad89c78a80066176d1294b0`
- mutator: `a5a0cb803b66e0cdb8bf471b3b08af701d424a41` plus the recorded Rust scan fix.

13. Verified SHA-256 hashes of the original inputs after baseline mutation;
    workers did not alter application source. Saved the original mutation
    snapshots: 40 killed, 17 survived, 57 executed (70.2%).
14. Added nine behavior tests and ran all 32 profile tests successfully before
    refactoring. Included exact clearing timestamp and persisted search-path
    identity, based on the survivors. Kept fallback/equivalent cases distinct
    rather than adding implementation-only tests.
15. Applied the prepared changes: separated catalog, row conversion and record
    identity in migration; shared the profile-directory rule; separated writer
    ownership, opening and indexing; extracted full-text score retrieval and
    exact-address matching; shared form selection and import across adapters.
16. Ran all 32 profile tests again successfully, then regenerated LLVM coverage.
    crapper passes `< 10` for every function in the two new storage/migration
    modules. Highest score is about 9.0 (table reading and suggestion ranking).
17. Reran dryer at 0.8. The form importer duplicate is gone. The remaining 0.88
    candidate compares password-reading and form-filling script builders; their
    embedded scripts and security behavior differ. Keep those separate rather
    than introducing a generic security-sensitive script builder.
18. Started a fresh complete mutation run for the final storage/migration source
    and strict profile Clippy. Rechecked Servo's unchanged cache flush (CC 3);
    its prior fresh-process native disk-cache acceptance remains applicable.

19. Strict profile Clippy and focused rustfmt checks passed. Updated the existing
    plan and wrote the linked quality report, including the scope limits and
    domain glossary. No application source outside the profile crate changed
    during this follow-up.
20. After coverage and tool installation completed, removed only this run's
    dedicated instrumented build and cargo-install build directories. External
    free space rose from 7.4 to 8.5 GiB; preserved the LCOV reports, installed
    tools, shared build target and all existing worktrees.

21. The full final mutation pass caught 41/52 sites. Inspected all survivors,
    then added a stronger exact-URL ranking fixture (equal search terms, newer
    fragment visit) and an absence-of-ControlEvent-records persistence check.
    Each passes normally and kills its targeted mutant in a focused rerun.
22. Combined final snapshots show 43 killed, 9 survived, 0 uncovered, 52 sites.
    Recorded the remaining numeric fallback, empty-query-result and weak-owner
    cleanup cases explicitly. Do not claim a zero-survivor gate or corruption
    coverage. All legacy-migration mutation sites are killed.
23. Saved final mutation snapshots and reports. Verified that the browser,
    address, engine and session source hashes still match this turn's baseline.
    Removed only temporary tool-generated metrics/workers after preserving
    their evidence; no existing worktree or shared build cache was cleaned.

24. Final verification passes: 34 profile tests, strict all-target profile Clippy,
    focused formatting and `git diff --check`. Linked the three installed tool
    entrypoints into the existing `~/.local/bin` PATH directory without replacing
    any command. Help invocations confirm they are available for later skill runs.
