---
title: An exact version pin in a shared library crate downgrades every consumer's lock
kind: anti_pattern
trigger_case: >
  theorem-ide-runtime declared unicode-normalization = "=0.1.25" to hold
  the version steady. When Frame moved its Theorem pin, Cargo had to
  downgrade Frame's lock from 0.1.26 to satisfy the exact requirement.
  Review asked for a caret requirement ("0.1.25"), with CRATE.md relying
  on Cargo.lock to hold the version. A later review round almost restored
  the exact pin as a "fix", because the reason for the caret was not
  written next to the requirement.
rule_short: >
  Library crates that other workspaces consume take caret requirements;
  the consumer's Cargo.lock holds the exact version. Write the reason
  beside any requirement that looks loose on purpose.
---
