---
title: Build frame-notes against a Theorem commit before moving the pin
kind: method
trigger_case: >
  Theorem commit 7eb1ba024 added two required methods to the
  DocumentAuthority trait (projection_pending, former_bindings) and two
  fields to AuthorityCommit (reapplies, reverts). All 419 Theorem tests
  passed. Building Frame's frame-notes against it then failed with E0046,
  because SharedAuthority did not forward the new methods, and E0063,
  because a test's AuthorityCommit literal lacked the fields. Building
  through a detached Theorem worktree at the candidate commit, symlinked
  at deps/theorem-rustyweb-pw2, caught both before the pin moved. The
  Theorem commit message was then amended to name both API changes.
rule_short: >
  Before changing theorem_rev in script/bootstrap, build and test
  frame-notes against the candidate commit through a detached worktree,
  and name any trait or struct API change in the Theorem commit message.
---
