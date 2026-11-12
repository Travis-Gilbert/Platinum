# Theo desktop: handoff to the Mac session

Written 2026-11-12 at the end of a cloud session that could not build the
application (`deps/` was absent, so the root workspace did not load), and
brought up to date the same day by the Mac session that built Stage B. Read this,
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

## Stage B: where it stands (2026-11-12, Mac session)

Stage B is implemented in `frame` under `src/theo_desk/` (commits `f5b5b37`,
`202f503` and the window fixes after them). What each spec item became:

1. **Dependency.** `companion` is a path dependency and a workspace member.
   `global-hotkey = "0.8"` supplies keys that work while another application
   is in front. The hand-written `Cargo.lock` entries were accepted by
   `--locked`.
2. **Renderer.** `body.rs` paints the grains as discs and the side arcs with
   GPUI's `canvas`, `paint_quad` and `PathBuilder`, at web Theo's measures
   (visor 120 pt, radius 21.6 pt, grains at 55% alpha, three arcs a side).
   The dark plate, its inset ring and its drop shadow are part of the
   design and stay (they were removed once by misunderstanding and restored
   at the owner's word). What the owner asked to remove was the macOS window
   around Theo: GPUI makes a titled panel even with no titlebar, so AppKit
   drew a rounded rim and shadow around the whole transparent rectangle.
   `mac::unframe` makes the window borderless with no window shadow, keeping
   GPUI's non-activating panel bit, so the window is only what Theo paints
   and the menu can still take the keyboard.
3. **Stage adapter.** `gaze_at` resolves against the real window and pointer
   (`mac::pointer_from`); the window is placed by a fraction of the screen's
   visible frame so a place survives resizing.
4. **Clock and stillness.** GPUI's frame callback with an `Instant`-based
   millisecond clock; reduce-motion is read once a second and mapped to
   `set_still`.
5. **Activity.** `activity.rs` turns the driver's `ThreadDocument` run state
   into `presence::Activity` updates (one generation per run, a stop for an
   older generation refused) and into liveness following web Theo's
   `livenessOf` (ask, work, a 1.5 s done hold, attend, idle). A new view
   restores from `Runs::snapshot`. Theo and the browser's Agent place share
   one `TheoremDriver`, so both show the same conversation and run.
6. **Expression.** Decided as a subset map, the same subset web Theo takes:
   energy, breath, tilt, gaze and hue from the wire expression, only while the
   turn runs. Chroma, density, size, calm, tightness, churn, lattice, pose and
   frame belong to the sprite or the field and are dropped. The transcript now
   says "Theorem set Theo's manner for this turn." on macOS.
7. **Invocation.**
   - Keys (in `theo.json` in the profile folder): `control+alt+KeyT` shows and
     hides Theo, `control+alt+KeyC` opens the talk menu, from any application.
   - **Talking is a right-click menu, not a text box** (owner's redesign).
     Right-clicking the face opens a gpui-component `PopupMenu` over it: the
     conversation title, Theorem's latest reply (scrolls past 200 pt), an "Ask
     Theo" `Input` focused at once, then Stop while a run goes on, or "Use the
     Demo Authority" when native Theorem is not configured. Enter sends; the
     reply arrives in the same menu. Stop and the demo keep the menu open.
     Escape, a click outside, or Platinum losing activation close it and hand
     the keyboard back to the application Theo was called over. A left click
     on the face only drags it.
   - Double right-click anywhere is an opt-in setting in the Theo menu of the
     menu bar. It uses a listen-only Core Graphics event tap, which needs
     **Input Monitoring** for Platinum and a relaunch; without it the menu says
     so and nothing else changes.
   - `Scope` (frontmost app's pid, bundle, name and frontmost window number)
     is captured at invocation, before Theo shows.
   - Outside the face and the open menu the window ignores the mouse, so clicks
     reach whatever is beneath.
8. **Stop.** `Companion::stop()`'s `StopRequest` goes to the driver that owns
   the run, and only when `Runs::is_current` says that generation is the one
   going. Hiding never ends a run.

### Bugs found running it, and their fixes

- `servo_media` exited silently: each new target directory needs
  `script/setup-media` once.
- Opening, closing or resizing windows and rebuilding the menu bar from inside
  an entity update panicked on GPUI re-entrancy. Those now go through
  `cx.defer` (`open`, `remove_window`, `install_menus`).
- Theo did not appear: resizing first could carry the window onto a
  neighbouring screen before it was placed. `mac::resize_at` sizes and places
  in one `setFrame` on the screen it is on.
- Eight to ten Theos on screen at once: `mount` took a busy window (one
  already being updated, as during a right click inside it) for a gone one
  and opened another. A window is now forgotten only when `cx.windows()` no
  longer holds it; a busy one is fitted once the update ends. This was also
  why right-click seemed to do nothing: the menu opened in a newly leaked
  window elsewhere.
- After the menu opened and closed, Theo's window painted nothing and the log
  said "RefCell already borrowed": a native `setFrame` or `setStyleMask`
  inside a GPUI update makes AppKit report the new size at once, GPUI cannot
  take it while it is updating, and the size is lost. `unframe`, `place` and
  `resize_at` now apply their change on the foreground executor after the
  update, as GPUI's own `resize` does.
- Edition 2024 closures returning `impl IntoElement` capture lifetimes; the
  reply element returns a concrete `Stateful<Div>`.

### How to run it

```sh
export CARGO_TARGET_DIR=/path/to/target   # one per worktree
script/setup-media                         # once per target dir
script/media-env cargo build --locked
```

computer-use cannot find a bare binary, so the check run used a throwaway
bundle wrapper (`PlatinumDev.app`, bundle id `dev.platinum.theo-check`) whose
`Contents/MacOS/frame` is the built binary, launched through
`script/media-env`.

### Tested (macOS, this session)

- `cargo test --locked -p theo -p presence -p companion`: 31 passed.
- `cargo test --locked --bin frame theo_desk`: 7 passed (activity
  generations, stale stops, snapshot, liveness, the Expression subset, the
  plate's rounded corners and its outline).

### Observed in the running application

- Before the redesign: Theo appeared over TextEdit and TextEdit stayed
  frontmost; hide and show repeated without a crash; after a remount, typing
  went to TextEdit; the old composer drew.
- After the window fixes (`run9`, this build): `control+alt+KeyT` over
  TextEdit shows exactly one Theo window, with the plate and no macOS frame.
  Hovering the face and right-clicking opens the menu in place above it (New
  chat, "Theorem isn't set up on this Mac.", the "Ask Theo" field focused,
  "Use the Demo Authority"). Escape closes it, the window returns to 264 by
  132 with the face drawn, and TextEdit is frontmost again. The log has no
  borrow errors.
- A right click that arrives with the pointer jumping straight onto the face
  in the same instant goes through to the application beneath: click-through
  is decided each frame from where the pointer is, and no frame had run yet.
  A real pointer moves there first, so this only bites synthetic clicks.

### Not yet observed

- Typing into the menu and the reply in it.
- A demo conversation and Stop from the menu.
- The draft kept across a remount; click-through around the face.
- The double right-click and its Input Monitoring prompt.
- **The gate**: invoke over Firefox and an editor, converse with the selected
  agent, dismiss and remount without losing state or stealing focus. The
  automated check could not do it: Firefox is read-only to computer use, and
  the owner was using the machine. It needs the owner's hands.
- Native Theorem: the authority is not configured on this Mac, so only the
  demo authority can be conversed with here.

## Open decisions for the owner

- **Expression (decided in Stage B, revisit if wanted).** The face takes
  the web's subset of the wire expression and drops the rest (see item 6).
  Extending the port to the field's terms would be a separate piece of work.
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
