---
title: A shared cargo target serves test binaries that point at another worktree's files
kind: gotcha
trigger_case: >
  On 2026-10-27 notes-app's first test run on the converged Servo pin failed
  after 922 s: four notes-fixtures tests panicked with ENOENT at
  normalize.rs:270. The binaries came from cargo-targets/frame-canvas, which
  notes-app shares with the canvas worktrees. They had been built from the
  p0-notes worktree, and notes-fixtures compiles env!("CARGO_MANIFEST_DIR")
  in, so they read p0-notes's fixture files. Cargo does not rebuild when only
  CARGO_MANIFEST_DIR changes, and the sources were the same, so it reused
  them. They failed only once p0-notes had been removed. frame-canvas.sh had
  touched only crates/profile. After it touched all of crates/, the rerun
  passed 28 of 28.
rule_short: >
  A wrapper that shares one target between worktrees must touch every
  workspace crate's sources, not only the ones that differ. Any crate that
  compiles CARGO_MANIFEST_DIR in otherwise keeps the first worktree's path.
---
