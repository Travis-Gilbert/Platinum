# Platinum coding standards

Proposed baseline: 2026-10-27. These rules take effect when adopted in the
repository. They apply to first-party Platinum code and changes to its
integration boundaries. Existing code may predate them; adoption does not
certify the current implementation or authorize a wholesale rewrite.

Platinum uses **data-oriented design**: design around the data the application
owns, the transformations it performs, and the workloads the machine executes.
Correctness, understandable ownership, responsiveness, and measured resource
use guide the implementation.

The standards adapt Theorem's engineering manual, Google's review practices,
NASA's requirement-writing guidance, and established Rust and data-oriented
design guidance. The rule IDs below are Platinum policy, not claims of
certification or wholesale adoption of another organization's rules.

## Reading guide

| Change | Relevant sections |
| --- | --- |
| Every substantive change | Authority, requirements, data-oriented design, verification and review |
| State, services, models, or interfaces | Data-oriented design, architecture and contracts |
| Rust implementation or native bindings | Rust and code quality, concurrency and lifecycle |
| Browser or GPUI behavior | Browser and UI, concurrency and lifecycle |
| Capture, search, storage, or external data | Data and trust boundaries |
| Performance or dependency change | Performance and dependencies |
| Commands and repository navigation | [AGENTS.md](AGENTS.md) and the current repository README |

## 1. Authority and adoption

**GOV-001 — One authored policy.** This document is the canonical coding
standard. `AGENTS.md` describes the working procedure and `CLAUDE.md` is an
entry point. Memory, generated summaries, and agent suggestions MUST reference
the authored policy and revision rather than create competing rules.

**GOV-002 — Requirement levels.** MUST and MUST NOT are requirements. SHOULD
is the default, with a documented reason for a different choice. MAY is
optional. Follow higher-priority session instructions, the authorized task,
and approved requirements. An explicit maintainer instruction may already
authorize a scoped exception; record that authorization rather than requesting
it again. Seek a maintainer decision only for an unresolved binding conflict,
and continue unaffected work. An exception to a MUST names the rule, scope,
reason, compensating verification, and review condition. Routine choices within
these rules do not need an additional approval process.

**GOV-003 — Proportionate enforcement.** New code and materially changed
behavior MUST follow applicable rules. Record unrelated existing gaps without
expanding the task into a rewrite. These rules are enforced by review unless a
named executable check actually implements them. This document creates no CI
jobs, hooks, branch protections, or evidence of compliance by itself.

## 2. Requirements and scope

**REQ-001 — State observable obligations.** A substantive feature or behavior
change MUST identify the responsible component, triggering condition, required
outcome, and verification method. Separate requirements from rationale and
implementation ideas. Include applicable failure, resource, privacy, and
lifecycle behavior. A small fix may express these in its issue or change
description; a separate specification is not mandatory for every change.

**REQ-002 — Preserve the promised outcome.** Delivery slices MUST retain the
agreed feature scope and name any outstanding integration. Do not silently
replace an engine, protocol, owner, dependency, or required live behavior with
a convenient substitute. A stub, mocked service, successful compilation, or
registered command does not establish that the user can exercise the feature.

Verification: review requirements for necessity, consistency, and a concrete
pass/fail observation; trace them to the changed entry point and consumer.

## 3. Data-oriented design

**DOD-001 — Start with data and workload.** Before introducing or materially
changing a state store or performance-sensitive pipeline, identify the data,
owner, expected size and growth, common reads and writes, update frequency,
ordering needs, and lifetime. Explain which operations consume which fields.
This can be a short design note or module comment. Unknown sizes are explicit
assumptions to validate, not reasons to build speculative machinery.

**DOD-002 — Make ownership and transformations explicit.** Each authoritative
piece of application state MUST have a clear owner and mutation path. Prefer
typed records and named operations over networks of objects with hidden
cross-object effects. Keep transformation logic separable from network,
filesystem, engine, and UI effects when practical. Functions, methods, traits,
and GPUI entities are all valid tools; choose the form that exposes the data
flow and invariants most clearly. Do not require universal immutability,
copy-on-write snapshots, or one global application store.

**DOD-003 — Distinguish authority, derivation, and presentation.** Durable
records, live browser state, indexes/caches, and view state MUST have explicit
relationships. Each derived store MUST identify its source and update or
invalidation rule; include a source revision or generation where staleness
affects behavior. Views may retain focus, selection, scroll, and edit buffers.
They MUST NOT become independent owners of persistent browser or search truth.
Avoid duplicate mutable state unless synchronization and failure behavior are
defined.

**DOD-004 — Choose storage by access pattern.** Prefer straightforward
collections, including contiguous `Vec<T>` or slices for sequential processing.
Choose maps and indexes for lookup requirements. Consider separating frequently
accessed metadata from large or rarely touched payloads when the workload
benefits. Array-of-structs, structure-of-arrays, arenas, and hybrid layouts are
choices, not universal requirements. Specialized layouts MUST have a workload
reason; performance claims require measurements under PERF-001. An ordinary
Rust struct is often the clearest correct representation.

**DOD-005 — Separate identity from position and lifetime.** References that
survive reorder, asynchronous completion, deletion, or storage reuse MUST use
identity whose validity can be checked. Use domain-specific ID types when
confusing two identifiers would be a correctness risk. A collection offset
MUST NOT silently become a durable identity. Reused slots require a generation
or equivalent stale-handle protection. Validate that the target still exists
and matches the relevant request/navigation generation before applying a
delayed result. A local handle is not automatically a persistent or global ID.

**DOD-006 — Process related work together.** Bulk operations SHOULD work over
explicit collections and batches when this improves access locality, reduces
repeated work, or limits coordination overhead. Keep batches bounded so input
and cancellation remain responsive. Coalesce replaceable state updates only
when ordering and meaning are preserved; do not drop commands, final results,
or durable records. Review repeated whole-collection scans, deep clones, and
per-item allocations in frequently executed paths before adding them. Small,
infrequent operations may favor simpler code.

**DOD-007 — Bound retained data and scheduled work.** Every new application-owned
queue, cache, worker pool, and continuously growing collection MUST define a
capacity, retention/eviction strategy, or justified growth policy. Specify
behavior at the limit: backpressure, rejection, coalescing, paging, or explicit
degradation. Durable user data requires a preservation and storage-pressure
policy; it MUST NOT be silently evicted using a cache policy. External input
sizes and traversal depth require limits appropriate to the operation.

**DOD-008 — Respect framework and ownership boundaries.** Use GPUI entities,
subscriptions, callbacks, and its ownership/threading rules for integration.
Keep application computation and state transitions independently testable where
practical. A new ECS, custom allocator, registry, scheduler, or actor framework
requires a concrete requirement and evidence that existing facilities are
insufficient. Data-oriented design alone does not justify replacing GPUI,
Servo internals, or Theorem's existing state owners.

### Applying this to Platinum

These examples guide design; they do not claim the repository already has the
named types or require an immediate migration.

| Workload | Appropriate design question |
| --- | --- |
| Tab switching and reordering | Can tab order change without changing identity or confusing pending work? Does the hot path need page bodies and history, or just compact metadata and handles? |
| Search and native results | Does search produce typed result data for the view? Can an older query overwrite a newer query? Which fields need sorting or scoring together? |
| Page capture and embeddings | Can captures be processed in bounded batches without holding the store lock across network or model work? What happens when the queue fills? |
| Graphs, maps, and canvases | Which fields are traversed together? Can visibility and selection operate on compact IDs while large documents remain elsewhere? |
| Downloads and progress | Which intermediate progress updates are replaceable? How are completion, failure, cancellation, and partial files preserved accurately? |
| Tab suspension or discard, when implemented | Which metadata must survive, which engine resources can be released, and which user activity prevents that transition? |

Verification: inspect owners, mutation paths, access patterns, identity checks,
and bounds; exercise meaningful state and overload cases. Profile or benchmark
when making a performance claim. Do not demand a benchmark for every data type.

## 4. Architecture and contracts

**ARCH-001 — Extend the existing owner.** Before adding an operation, store,
runtime, or adapter, trace the existing implementation and consumers. Reuse or
extend it when it can satisfy the requirement. New abstractions MUST serve a
current behavior, necessary boundary, or demonstrated reuse. Keep modules
cohesive and dependencies directed toward the capability they consume.

**ARCH-002 — Preserve the product/engine boundary.** Platinum owns its browser
interface and application orchestration. Servo owns web-engine behavior;
Theorem dependencies retain their existing search, substrate, and storage
responsibilities. Use explicit integration seams. Do not copy backend policy
or create a parallel database because a UI needs another view of the same
data. Changes to ownership MUST describe the new boundary and migration path.

**ARCH-003 — Make contracts typed and truthful.** Cross-module, engine,
background-worker, persistence, and network boundaries MUST define inputs,
outputs, errors, identity, and lifetime semantics. Use typed Rust values for
known shapes. Raw JSON may exist at dynamic boundaries, but MUST be validated
and converted before application decisions rely on its structure. Ground
adapters in the pinned dependency or observed protocol. Contract changes name
affected consumers, compatibility behavior, and verification.

Verification: trace a real caller through the owner and back to the consumer;
test the affected boundary, including incompatible or malformed input where
relevant.

## 5. Rust and code quality

**RUST-001 — Use idiomatic, readable Rust.** Follow the repository toolchain,
formatter, and configured lints. Use precise names and cohesive functions.
Comments explain reasons, assumptions, and invariants; public API documentation
describes use, errors, and relevant panic or safety conditions. Prefer standard
types and established libraries when they express the requirement clearly.
There are no universal function/file line quotas or assertion quotas.

**RUST-002 — Encode meaningful states.** Use enums for mutually exclusive
states and types that make invalid combinations difficult to construct.
Keep orthogonal facts separate: a tab's visibility, focus, loading state, and
audio activity are not necessarily one exclusive state machine. Use explicit
units for sizes, time, and coordinate spaces where confusion is plausible.

**RUST-003 — Handle expected failures.** Input, network, storage, permission,
and native-runtime failures MUST have intentional outcomes. Propagate or handle
errors with useful context. Do not use unchecked indexing, `unwrap`, `expect`,
or panics for expected failures on production paths. Invariant assertions and
test assertions are appropriate when their assumptions and failure consequences
are understood. Ignored results require a reason; empty catches and fabricated
success values are not recovery strategies.

**RUST-004 — Keep unsafe boundaries narrow.** New or changed `unsafe`, FFI,
raw-handle, and native-surface code MUST document its safety obligations and
why each unsafe operation satisfies them. Address ownership, aliasing, lifetime,
thread affinity, synchronization, and cleanup as applicable. Prefer safe
wrappers with constrained interfaces. Use suitable tests and supported tooling;
Miri or a successful run cannot certify external native APIs or all executions.

**RUST-005 — Make sharing deliberate.** Prefer ownership and borrowing when
they fit. `Rc`, `Arc`, interior mutability, locks, and dynamic dispatch are
allowed when their role is clear. A shared pointer alone does not establish
synchronization or semantic ownership. Do not add clones, locks, or boxed trait
objects merely to avoid understanding the lifetime or access pattern. Keep
lint suppressions local and justified; do not suppress diagnostics wholesale.

**RUST-006 — Apply BeautifulRust guidance.** For Rust implementation and
refactoring, use BeautifulRust's `rust-best-practices` skill; use `rust-review`
for requested Rust reviews and `rust-port` for ports from another language.
Read the relevant bundled references when the reasoning needs clarification.
The adopted reference is
[BeautifulRust at `a9cdcfa`](https://github.com/morganlinton/BeautifulRust/tree/a9cdcfa94d1330c0d41c7f67764c5f7786fbb71d/skills),
reviewed 2026-10-31. Install the three sibling skill directories together so
review and porting can consult the shared best-practices guidance.

Apply its ownership, typed-state, error, API, documentation, and safety guidance
to the changed code. Keep application code simple and distinguish it from APIs
with external consumers. Ports MUST preserve required observable behavior and
map the original tests to the Rust tests; document any authorized differences.
Review findings MUST follow `REV-002`, distinguishing defects from optional
style improvements.

These authored standards and the authorized task take precedence where the
skill's generic advice differs. In particular, preserve GPUI's thread-affine
entities and callbacks, intentional dependency pins, and existing engine/FFI
contracts. Use Platinum's scoped formatting, media build wrapper, and relevant
test targets under `AGENTS.md` and `PERF-003`; do not automatically reformat the
workspace, expand to every feature/target, or repair unrelated warnings.
BeautifulRust's optional lint bundle is a reference, not an adopted CI
configuration. Installing the skills does not add lint enforcement or certify
existing code.

Verification: formatting and applicable lints, source review, and tests of the
affected behavior. A clean formatter does not establish correctness.

## 6. Concurrency and lifecycle

**CON-001 — Keep the UI responsive.** Network requests, blocking file work,
substantial parsing, indexing, and model inference MUST run off the UI thread.
Respect GPUI and engine thread affinity. Exchange owned data or valid handles
through their supported dispatch mechanisms. Render paths MUST NOT initiate
uncontrolled persistent side effects, blocking work, or background jobs on
every redraw.

**CON-002 — Own work until it ends.** Every background task, subscription,
timer, and worker MUST have a lifetime owner and defined completion,
cancellation, failure, and shutdown behavior. App-owned workers may outlive a
view when that lifetime is intentional. Closing a tab or changing a query
must not allow old work to mutate a replacement target. Define how cancellation
affects already committed external actions; do not promise rollback where the
underlying API cannot provide it.

**CON-003 — Bound waiting and retries.** External operations and retries MUST
have deadlines or an explicit long-lived protocol with cancellation and
bounded buffering. Retries require an attempt/time budget and an assessment of
whether repeating the operation can duplicate effects. Use backpressure and
bounded concurrency. Potentially unbounded loops need an intentional lifetime,
a cancellation path, and a per-iteration work bound. Input-driven recursion
needs a depth bound or an iterative alternative.

**CON-004 — Make synchronization reviewable.** State the protected data and
lock ordering when multiple locks interact. Do not hold a lock or mutable
borrow across unrelated I/O, expensive inference, reentrant callbacks, or an
`await` without a documented design that requires and safely supports it.
Publish results through controlled owner updates; validate freshness at the
point of application. Shutdown MUST release native resources and subscriptions
in an order compatible with their owners.

Verification: inspect cancellation and ownership paths; test stale completion,
tab/view removal, error, overload, and shutdown cases that the change can affect.

## 7. Browser and GPUI behavior

**UI-001 — Keep views connected to real behavior.** Reuse the established GPUI
Kit components, theme sources, actions, and input conventions. Keep browser
state changes in explicit operations, with views presenting that state. Every
user-visible control MUST reach its intended operation or expose an honest
unavailable state. Production paths MUST NOT present fixture content, raw debug
JSON, or simulated success as real results. Explicit development tools may
display diagnostics.

**UI-002 — Define the interaction states.** UI changes MUST handle applicable
empty, loading, partial/degraded, success, error, and cancellation states.
Preserve keyboard access, focus restoration, readable contrast, accessibility
semantics, and relevant reduced-motion/transparency settings. Retain scroll or
selection across updates when the interaction requires it. Use incremental
updates or virtualization when collection size makes full rebuilding costly.

**UI-003 — Treat lifecycle and rendering as contracts.** Browser lifecycle
changes MUST specify what happens to navigation, history, input focus, media,
downloads, pending operations, and native surfaces where affected. Native
rendering changes address logical/physical coordinates, scale factor, clipping,
pixel format, ownership, and frame lifetime as applicable. Do not conceal an
engine failure behind a successful-looking page or unannounced engine switch.

Verification: exercise the affected user interaction on a supported host.
Headless GPUI tests cover their modeled behavior; native engine/media/surface
claims require the corresponding native path. Visual changes need visual
inspection at the relevant appearance and scaling settings.

## 8. Data and trust boundaries

**DATA-001 — Treat external content as data.** Web pages, model output, extension
or plugin input, downloads, and network responses MUST be validated at their
boundary. Preserve origin, profile, permission, and user-authority checks.
Content from a page or model MUST NOT acquire host filesystem, shell, credential,
or automation authority merely because the application can read it. Agent
proposals pass through the deterministic action/permission boundary; applicable
user interruption and approval semantics remain enforceable there.

**DATA-002 — Preserve user information.** Persistent formats and storage changes
MUST define compatibility, migration, interruption recovery, and failure
behavior appropriate to the risk. A switch from durable storage to memory is
an explicit degradation, not equivalent success. Never delete user data to fix
a build or treat a database as a rebuildable cache. Downloads require safe path
handling and explicit completion/cancellation semantics.

**DATA-003 — Keep collection and disclosure intentional.** Respect the product's
capture, retention, and external-service policies. Changes that expand page
capture, background fetching, or disclosure to remote providers MUST make that
change explicit. Keep credentials out of source, normal logs, fixtures, and
settings files; preserve the existing approved secret sources. Log enough
identity and state to diagnose failures without retaining unnecessary page
contents, prompts, tokens, or personal data.

Verification: inspect trust crossings and write paths; exercise relevant
validation, storage failure, migration, and authorization cases. A typed API
does not by itself establish authorization.

## 9. Performance and dependencies

**PERF-001 — Measure the claim.** A claimed performance improvement, or a change
whose acceptance depends on a performance target, MUST define the workload,
input size, hardware, operating system, dependency revisions, build profile,
measurement method, and relevant cold/warm conditions. Compare baseline and
candidate; include variability or distributions when timing noise matters.
Distinguish CPU, GPU, network, model, memory, and build costs. A microbenchmark
only establishes its measured operation, not an entire application speedup.
Unmeasured performance improvements remain hypotheses and MUST be reported as
such.

**PERF-002 — Set meaningful budgets.** New performance-sensitive paths MUST name
the user-facing latency/resource requirement or measurement needed to establish
it. Budgets should reflect representative tab counts, data sizes, and supported
hardware. Agree thresholds before judging a regression. Do not fabricate a
memory target or universal frame-time guarantee. Performance work MUST preserve
behavior, correctness, and data integrity.

**PERF-003 — Protect the development loop.** Follow the build procedure in
`AGENTS.md` and the current README. Preserve the pinned dependency graph and
configured build caches. Run focused checks during development and broaden
where the changed integration requires it. Avoid full cleans, repeated Servo
rebuilds, and unsupported target matrices without a concrete reason.

**DEP-001 — Justify and reproduce dependency changes.** A new dependency MUST
solve a named need and be assessed for compatibility, maintenance, license,
native/build cost, and duplication of existing facilities. Preserve the
repository lockfile and explicit pins. Dependency changes record the new
revision and affected interfaces; a floating local checkout is not a
reproducible fix.

**DEP-002 — Preserve patch provenance.** Changes to Servo, GStreamer, vendored
GPUI components, or other pinned dependencies MUST follow the existing patch
workflow. Keep the pinned revision, patch or fork change, rationale, and
verification together. Do not leave required edits only inside ignored `deps/`
checkouts. Do not rewrite or reformat unrelated upstream code, or claim a
development runtime arrangement is a distributable bundle.

Verification: reproducible measurements for performance claims; dependency and
patch review plus the build/integration path that actually consumes the change.

## 10. Verification and review

**VER-001 — Match evidence to the claim.** Choose verification for the property
and boundary involved: state tests for invariants, boundary tests for contracts,
native runs for engine behavior, and measurements for performance. Source
review and execution complement each other. Mocks and fixtures MUST be labeled;
they cannot replace required live integration without an explicit accepted
substitution. A native smoke test does not replace an invariant test either.

**VER-002 — Test meaningful behavior.** Substantive defect fixes SHOULD include
a focused regression test that can detect the defect. Add property tests,
fuzzing, concurrency tools, or soak tests when the risk warrants them. Avoid
tests that only repeat implementation details. Simple documentation or cosmetic
edits need proportionate review, not an invented test suite. Do not weaken
assertions, remove tests, or refresh baselines merely to obtain a passing result.

**VER-003 — Report execution honestly.** Verification reports MUST name the
revision and relevant local changes, command, target/features, environment,
and result. Distinguish passed, failed, blocked, skipped, and not run. Zero
selected tests, ignored checks, source inspection, and historical artifacts
are not new successful executions. State exactly what remains unverified.

**VER-004 — Verify the delivered path.** Feature completion MUST demonstrate
the required entry point, state transition, participating components, and
user-visible or API-visible result. For a browser/backend change, exercise the
actual connection. For a library-only deliverable, verify its supported consumer
contract. Update affected documentation and retain relevant limitations.

**REV-001 — Review for code health and progress.** Keep changes coherent and
reviewable; separate unrelated cleanup. Review correctness, failure handling,
ownership, data layout, stale state, concurrency, cleanup, trust boundaries,
tests, and documentation where relevant. Fix material defects while allowing
incremental improvements. Scale independent review to risk; no fixed reviewer
count or PR line quota is required.

**REV-002 — Ground findings.** A blocking defect MUST identify the conditions,
affected path, consequence, and reproduction or causal reasoning. A standards
violation cites its applicable rule ID. A real defect needs no pre-existing
rule ID; personal preference does not become a rule through confidence.
Clearly mark optional polish and unrelated pre-existing findings.

### Completion questions

- Does the requested behavior work through its actual consumer?
- Are data ownership, freshness, identity, and growth behavior clear?
- Are errors, cancellation, native lifetimes, and cleanup covered where changed?
- Do tests and measurements establish the specific claims being made?
- Are source facts, proposals, and execution results reported separately?

## Sources and adoption notes

References reviewed 2026-10-27. External references inform these authored rules;
they do not independently impose every rule above.

| Reference | Contribution |
| --- | --- |
| [Theorem engineering manual](https://github.com/Travis-Gilbert/Theorem/tree/8e151d510722c2946f3f1cf63eb5a73e06c1695b/docs/engineering) and [shared instructions](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/AGENTS.md) | Ownership, verifiable requirements, truthful evidence, proportional review, one authored policy |
| [Google: review standard](https://google.github.io/eng-practices/review/reviewer/standard.html), [review contents](https://google.github.io/eng-practices/review/reviewer/looking-for.html), [small changes](https://google.github.io/eng-practices/review/developer/small-cls.html) | Code health, concrete review findings, coherent changes, behavior and complexity review |
| [NASA: writing a good requirement](https://www.nasa.gov/reference/appendix-c-how-to-write-a-good-requirement/) | Necessary, clear, traceable, verifiable requirements and defensible performance criteria |
| [Richard Fabian: Data-Oriented Design](https://www.dataorienteddesign.com/dodbook/node2.html) and [optimization guidance](https://www.dataorienteddesign.com/dodbook/node9.html) | Design around data and transformations; choose layouts using access patterns and measurement |
| [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) | Selective idiomatic API, type, naming, and documentation guidance |
| [BeautifulRust skills](https://github.com/morganlinton/BeautifulRust/tree/a9cdcfa94d1330c0d41c7f67764c5f7786fbb71d/skills) (reviewed 2026-10-31) | Rust writing, review, and behavior-preserving porting workflows; adopted with the project-specific precedence in `RUST-006` |
| [Claude Code: project instructions](https://code.claude.com/docs/en/memory) | A small `CLAUDE.md` importing shared instructions; instruction text is distinct from executable enforcement |

Repository observations were checked against Platinum commit
`4f3373f69f2e063f09930d648945759bd04eb2b8`, including its README, Cargo manifest,
toolchain, bootstrap/media wrapper, source tree, and portions of browser/search
state. This was a documentation and source review, not a complete code audit or
a successful native build. The inspected package is `frame`; this standards
proposal does not rename it or alter storage paths. Older project handoffs are
context only and do not override the current repository's ownership or layout.
