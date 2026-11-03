---
title: Large test stores go to TMPDIR on the internal disk, not the SSD target dir
kind: postmortem
trigger_case: >
  Codex's third round on the RedCore journal change died at 16:37 on
  2026-10-26. Codex panicked with "failed printing to stdout: No space
  left on device", and its prior-commit memory-guard baseline failed with
  chunk_store_journal_poisoned. The worktree, CARGO_TARGET_DIR and bench
  output were all on the external SSD (78 GiB free), but the unit tests
  create their stores with tempfile, which uses $TMPDIR under
  /private/var/folders on the internal data volume. That volume was
  already at 100% with a few GiB free, and 30 RedCore tempdirs (5.5 GiB)
  were still there from earlier killed runs. The job then sat dead for
  over three hours before anyone noticed.
rule_short: >
  Before running large-store tests or benches, export TMPDIR to a
  directory on the SSD and check df -h /System/Volumes/Data; a target dir
  on the SSD does not move tempfile stores.
---
