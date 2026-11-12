# Theo desktop: handoff to the Mac session

Written 2026-11-12 at the end of a cloud session that could not build the
application (`deps/` was absent, so the root workspace did not load). Read this,
then the spec at [theo-desktop.md](theo-desktop.md). The spec is the authority;
this file says where the work stands against it.

## Where things are

| Branch | PR | Holds |
| --- | --- | --- |
| `claude/theo-core-port` | #22, ready for review, base `main` | `crates/theo`, `crates/presence`, `script/test-theo`, `.github/workflows/theo.yml` |
| `claude/theo-desktop-host` | #23, draft, base `claude/theo-core-port` | `crates/companion`, this file, the spec |

Work continues on `claude/theo-desktop-host`. If #22 merges first, rebase this
branch onto `main`. The owner asked that the PRs be left alone while building:
do not babysit review bots unless asked.

## What exists

- **`crates/theo`** (no dependencies): a port of web Theo's pure core from
  `Theorem-UI` `src/lib/face` at `88043d3`, checked against fixtures recorded
  from the web (`crates/theo/fixtures`). `Face` in `src/engine.rs` is the entry
  point. `Profile::Desktop` makes sound a loudspeaker and never a mouth.
- **`crates/presence`** (no dependencies): the activity arbiter of spec §5.
  `Presence::apply` takes an `Activity`, refuses stale generations and
  revisions, and `primary()` names the one activity the face stands for.
  Bounded at 64 live and 256 ended.
- **`crates/companion`** (depends on the two above): one `Face` plus one
  `Presence`. Form switching, `hide` that ends nothing, `stop()` that returns
  a `StopRequest { activity, generation }` for the host to route, the invoking
  `Scope`, the optional double-right-click detector (`Calling`), and
  `frame(now_ms)` which feeds only fresh signals and returns the grains.

## Evidence so far

- Test-verified on Linux x86-64: `script/test-theo`, 31 tests, at `b5e835f`.
- Test-verified on macOS: CI ran `theo` and `presence` green at `5361cb2`.
  `companion` has not run on macOS.
- Not integrated: nothing in `frame` depends on any of the three crates.
- Not observed: nothing has been seen in the running application.

## First on the Mac

```sh
script/media-env cargo test --locked -p theo -p presence -p companion
script/media-env cargo build --locked
```

The `Cargo.lock` entries for `theo`, `presence` and `companion` were written by
hand. If `--locked` rejects them, run the build once without `--locked`, check
that only those three entries changed, and commit the lockfile.

## Stage B, what is left

All of it is in `frame` (do not rename the package).

1. **Add the dependency.** `companion = { path = "crates/companion" }` in the
   root `Cargo.toml`.
2. **Renderer.** A GPUI element that draws `Companion::frame(now_ms)`. The
   buffer is `GRAIN_STRIDE` (8) `f32` per grain: x, y, radius, phase, then
   premultiplied r, g, b, a (`crates/theo/src/particles.rs`). `Face::head()`
   gives tilt, scale, offset and squash for the visor; `shapes_now()` the
   shapes; `emitting()` what leaves the sides. GPUI source is vendored under
   `vendor/gpui-pre` and `vendor/gpui-component`; `src/canvas/` and
   `src/scene.rs` are the nearest existing drawing code. The `gpui` skill
   covers custom elements.
3. **Stage adapter.** Implement `theo::player::Stage` (`gaze_at`, `go`,
   `burst`, optionally `knows_things` and `thing`) over the real window and
   pass it to `face.attach`. `PlainStage` in `engine.rs` is the reference.
4. **Clock and stillness.** Drive `frame` from GPUI's frame callback with a
   monotonic millisecond clock. Map the system reduced-motion setting to
   `set_still`; it changes interpolation only.
5. **Activity source.** Turn Theorem thread and run state (`src/theorem/driver.rs`)
   into `presence::Activity` updates and `set_liveness`. Send a snapshot on
   connect (`Companion::restore`), then ordered updates.
6. **Expression.** `src/theorem/transcript.rs:1102` still shows the fallback
   note for `ThreadPart::Expression`. See the open decision below before
   wiring it.
7. **Invocation.** A keyboard action for show, hide and composer; the pointer
   path through `Companion::right_click` with a setting for `Calling`. Capture
   `Scope` at the moment of invocation.
8. **Stop.** Route `Companion::stop()` to the owner of the run. The companion
   never ends work itself, and `Face::ending` is not proof that work finished.

Then run the application and say what was observed, separately from what was
tested.

## Open decisions for the owner

- **The two `Expression` types differ.** The wire type is
  `agpui_contract::field::expression::Expression` (hue, chroma, density, size,
  calm, tightness, churn, frame, lattice, pose, energy and more). The port's
  `theo::engine::Expression` is the web face's (energy, breath, tilt, gaze,
  hue). Only hue and energy line up by name. Either map the subset and say what
  is dropped, or extend the port; check how web Theo consumes the same part
  before choosing.
- **`camera` and `record-player` forms do not exist.** The book has orb,
  boombox and the expressive performances. An activity that prefers a missing
  form is recorded as refused and the face shows nothing for it. They need
  authoring, or the spec should name forms that exist.
- **Deleted version names are reused.** In `crates/theo/src/book.rs`, `define`
  numbers the next version from the last one present. Fixing it needs a stored
  highest-issued number, which departs from the web's saved format. This is the
  one review finding on #22 left unanswered.
- **Equal standing.** Among activities of the same standing the latest begun
  has the face, so sound shows over a run. It is a one-line change in
  `Presence::primary` if the other order is wanted.

## Later stages

C (computer use: compare Cua Driver and agent-desktop on this Mac, then the
observe, resolve, act, verify broker), D (Herdr attach, MCP and typed API),
E (expandable local browser reusing `Page` and `Engine`), F (remote computers).
None is started. C needs accessibility and screen-recording permission granted
on the machine.

## Standing rules that bit or nearly did

- Never invent amplitude: an unmetered boombox shows no level.
- Hiding never cancels work; stop is explicit and routed.
- A passing pure-Rust test is not a claim about the native application.
- Prefer `cargo build` to `cargo check` here (see `AGENTS.md`).
