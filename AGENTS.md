# Platinum: shared instructions for coding agents

Read this file when working in Platinum. `CLAUDE.md` imports it for Claude Code.
The canonical engineering rules, including data-oriented design, are in
[CODING_STANDARDS.md](CODING_STANDARDS.md). Do not maintain a second copy of
those rules here.

## Understand the repository first

- Check the current branch, working tree, and exact task. Inspect source and
  callers before editing. Preserve other agents' work: do not reset, force-push,
  clean, overwrite, stash, or discard it to simplify your own task.
- The Cargo package is currently named `frame`; **Platinum** is the project.
  Do not rename the package, stored data, environment variables, or application
  identifiers unless the approved task includes that change and its migration.
- Read `README.md`, `Cargo.toml`, `rust-toolchain.toml`, and the affected source.
  Open task-relevant material under `docs/`, `tests/`, and `script/` as needed.
  The root is one Cargo package/workspace; the pinned checkouts beneath `deps/`
  are path dependencies, not owned root-workspace members.
- Identify the current consumer, source of truth, state owner, error path, and
  relevant tests. Check whether an existing operation or GPUI Kit component
  already supplies the requested behavior before creating another one.

## Architecture and data-oriented design

Platinum is a native Rust browser: Servo renders web content, GPUI Kit draws
browser chrome, and the pinned Theorem/RustyWeb dependencies provide search and
browser-substrate capabilities. Respect these boundaries; a native view is not
an excuse to duplicate engine or storage ownership.

Apply `DOD-001` through `DOD-008` in `CODING_STANDARDS.md`. For substantive
state or performance work, begin with the data: identify its identity, owning
component, lifetime, size/growth, frequently accessed fields, mutation path,
consumers, and stale-result behavior. Choose representation for the measured
or expected workload, not because a particular technique is fashionable.
Ordinary Rust structs and GPUI entities are valid; do not impose an ECS,
structure-of-arrays layout, or novel runtime everywhere.

Keep durable records, engine state, derived indexes, and transient views
separate. Preserve stable tab/page/request identity through reordering,
cancellation, and async completion. Put blocking work off the UI thread and
bound queues, concurrency, and retention without silently discarding user data.

## Scope, safety, and product behavior

- Deliver the authorized outcome, including its real consumer integration.
  Partial slices must name remaining connections and cannot quietly shrink the
  requested feature. A compiled library or plausible mock UI is not proof that
  the actual browser operation works.
- Browser content, downloads, network responses, model output, and extensions
  are untrusted. Preserve origin/profile isolation, permission boundaries,
  credential handling, and user approval for consequential actions.
- A control reaches the real operation or exposes an honest unavailable state.
  Preserve navigation, focus, selection, history, media, downloads, and native
  surface lifecycle when the change affects them.
- A new abstraction, scheduler, state store, or external dependency needs a
  named responsibility. Prefer extending existing ownership and actual
  integration seams; no silent replacement of Servo, GPUI, or RustyWeb.
- External or vendor edits require a reproducible pin and patch or tracked fork
  change. Do not leave necessary fixes only in ignored `deps/` working copies.
  Preserve the `vendor/` patch provenance notes where relevant.

## Build, test, and evidence

Follow the checked-in README and scripts. The pinned `rust-toolchain.toml`
currently selects Rust 1.97.1; use the repository's actual selected toolchain.
The native media build relies on GStreamer and the patched media environment.
For an environment that has already been bootstrapped, the baseline commands
are:

- `cargo fmt -p frame -- --check` — scoped formatting check.
- `script/media-env cargo build --locked` — compile the application when its
  media environment is installed.
- `script/media-env cargo test --locked <test-filter>` — focused relevant tests,
  selecting an exact test target as needed.
- `script/media-env cargo run --locked` — exercise an actual native application
  scenario when required by the change.

For a fresh environment, follow `README.md`: `script/bootstrap`,
`script/bootstrap-media`, and `script/setup-media` as applicable. The README
notes that `cargo check` rebuilds Servo from scratch; **prefer `cargo build`**
for this project's normal development loop rather than assuming `cargo check`
is cheaper. Full native builds are expensive and need adequate disk space;
avoid needless clean builds. A development runtime is not a tested release
bundle. Where an environment cannot support native checks, say so explicitly.

Review source **and** execute appropriate checks. Test the changed invariant,
then its true consumer boundary; broaden to native rendering/media or live-site
compatibility only when the claim requires it. Keep tests capable of detecting
regressions. Do not remove failing tests, weaken assertions, or mark skipped,
blocked, not-run, or source-reviewed work as passing.

## Review and completion

Use rule IDs from `CODING_STANDARDS.md` for documented standards violations.
Differentiate a demonstrated defect (conditions and consequence), a policy
violation (applicable rule), and optional polish. Review correctness, ownership,
data flow, memory/lifetimes, concurrency, cleanup, authorization, and user
behavior proportionally to the change. A real defect can block without a
pre-existing policy rule; a preference cannot.

Before declaring completion, report the source revision and outstanding changes,
what changed, the exact commands/target/environment actually exercised, and
results. Identify which claims are source-implemented, test-verified,
integrated, or observed in the mounted application. Update affected operational
or implementation documentation when contracts or behavior change. If blocked,
name the exact boundary and preserve completed work rather than inventing
successful verification.
