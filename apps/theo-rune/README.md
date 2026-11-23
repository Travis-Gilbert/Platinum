# theo-rune: Theo as one Rune application

The Rune rebuild of Theo (spec: `2026-11-22-theo-rune-main-application.md`).
One application holds one `AppSession` above every window. Theo starts as a
small presence on the desktop, can change into working forms, and can open
into a full workspace. Throughout, it stays the same Theo, with the same
session, drafts and running behaviors.

This is a separate Cargo workspace with its own lockfile, toolchain
(`rust-toolchain.toml`, Rune's 1.99.1) and CI
(`.github/workflows/theo-rune.yml`). It reuses the renderer-neutral crates
from the Platinum root: `theo`, `presence`, `companion`, `theo-activity`,
`theo-body` and `launcher`. It must never depend on GPUI, Servo or
GStreamer, and CI fails if any of them appears in its tree. The GPUI desk
app keeps working while the migration runs.

## Build, test, run

```bash
cd apps/theo-rune
CARGO_TARGET_DIR="/Volumes/SSD Samsung/cargo-targets/theo-rune" cargo test --locked
CARGO_TARGET_DIR="/Volumes/SSD Samsung/cargo-targets/theo-rune" cargo run --release
```

`script/install` builds the release binary and installs it as
`~/Applications/Theo.app` with Rune's own installer (`scripts/mac-app` at the
pinned Rune revision), which signs it, swaps a running copy for the new
build, and checks its ready receipt. `ctrl+alt+t` summons and dismisses it.

`script/journey` drives the release build through one person's journey on
Rune's virtual desktop and checks the window at each step: the presence at
its home, a tap opening the workspace, typing kept as a draft, Escape back to
the presence, the shortcut hiding and showing Theo, and the draft still there
when the workspace reopens. Each step runs under its own `RUNE_DIRS_ROOT`,
which isolates both the preferences and the behaviors folder, so the
installed app's saved state never reaches it. CI runs it too.

```bash
CARGO_TARGET_DIR="/Volumes/SSD Samsung/cargo-targets/theo-rune" script/install
CARGO_TARGET_DIR="/Volumes/SSD Samsung/cargo-targets/theo-rune" script/journey
```

The home corner and the first workspace keep clear of the menu bar and Dock:
on macOS they are placed in the primary screen's `visibleFrame`.

To take a headless snapshot of one named state, use `--snap out.png --scene <name> --at <seconds>`.
The scenes are:

- `presence`
- `workspace`
- `opening`: mid-change.
- `authored`: the presence with every behavior in the behaviors folder running.

## Where state lives

| What | Owner | Lifetime |
| --- | --- | --- |
| Session id, form, drafts, surfaces | `session::AppSession` | the process; form and drafts saved by preferences |
| Form changes (presence to workspace) | `presentation`, written only by the session | one change at a time; a press mid-change reaches nothing |
| Face channels | `companion::Companion` (crates/theo) | the session |
| Behaviors and their revisions | `behavior::Catalog`, saved by one writer thread | durable, across restarts |
| Capability signals | `signals::Registry` | the session; values go stale after their lifetime |

## Authoring behavior

A behavior is a Luau file. It runs in its own sandboxed VM (rune-luau), on
the application's poll clock, with a 50 ms time limit and a 32 MB memory
limit. It needs no rebuild, and there is no closed enum of expressions: a
script can define new performances, draw new geometry and answer new
signals.

### Defining or revising

Write `<name>.luau` into the behaviors folder. The folder is
`$THEO_BEHAVIORS_DIR`, or by default
`~/Library/Application Support/com.theorem.theo/behaviors`. The running app
picks the file up within half a second.

- **A revision that fails to start changes nothing.** The previous revision
  keeps running with its state, and the failure is recorded.
- **A revision that starts takes over.** It inherits its predecessor's
  `saveHotState()` through `loadHotState(state)`.

`AppSession::author(name, source, by)` is the same route, called from
inside the application.

```text
behaviors/
  wave.luau             the inbox: write here to define or revise
  wave/1.luau           every revision's exact source
  wave/2.luau
  wave/revisions.json   author, runtime, and outcome of each revision
```

### The host module

```luau
local theo = require("@theo/host")

theo.frame:Connect(function(now, dt) end)          -- every frame, poll clock only
theo.changed:Connect(function(name, value) end)    -- a signal moved; nil = went stale
theo.signal("composer.typing")                     -- current value or nil

theo.perform("nod")                                -- a book performance by name
local h = theo.perform({ { op = "to", set = { ["face.tilt"] = 6 }, over = 120 } },
                       { intensity = 0.5 })        -- any program in the engine's JSON
theo.cancel(h)
theo.define("beckon", { { op = "use", name = "blink" } }, "why")  -- new book entry
theo.notice("person.wave", 1)  theo.tint(200)

theo.draw({ { x = 0, y = -60, rotate = 0.1, scale = 1, paths = {
    { points = { {0, -10}, {10, 10}, {-10, 10} }, fill = "#ffd166",
      stroke = "#ffffff", width = 2, closed = true } } } })   -- nil clears

theo.open("note", "n1")                            -- an application intent
theo.random(-4, 4)  theo.now()                     -- seeded; math.random is seeded too

return {
    saveHotState = function() return {} end,
    loadHotState = function(state) end,
}
```

What is refused, and why, is kept for the author in `Behaviors::notes`.
Refusals include an invalid program, a drawing beyond its limits, and an
unknown book name. A behavior can only open surfaces. It cannot change
Theo's form; that is the person's.

### Signals

A capability registers a namespaced measurement, `capability.measure`, with
an owner and a lifetime. Only its owner can set it, and a value nobody
refreshes reads as absent. The session registers two:

- `composer.typing`: characters per second, measured only while the
  composer can take input.
- `theo.form`: 0 at presence, rising to 1 in the workspace.

Names that `theo::signals` already knows are fed into the face engine too.

### Replay

`Behaviors::start_capture` records a capture: the engine and runtime
revisions, the behavior and face seeds, the clock origin, and every exact
source and input, with each frame's signal changes.
`behavior::run::replay(&capture)` runs it again from that reset state. The
same capture gives the same trace, before and after a JSON round trip.

Motion that must replay should be driven by `theo.frame`. rune-luau's
`task.wait` adds wall time to its deadlines, so it wakes at a rate that
depends on how fast the machine is.

## Status against the spec

| Stage | State |
| --- | --- |
| 1. Shell and session | Done. The journey tests drive the real runtime: presence, change, workspace and back, with the same session and draft; reduced motion; presses mid-change. Snapshots of each form were observed. |
| 2. Authored behavior, geometry, signals, replay | Done, with one gap. The release binary drew a behavior loaded from the folder. The gap: the Rune app has no TTS or capture yet, so their default transformations and lifecycle signals are not wired. Starting a capture from the running app (CLI or menu) is also not wired; the API and replay are tested. |
| 3. Real conversation and note | Not started. It needs the chat controller and transport, authentication, and the `crates/notes` document authority, which pulls the pinned Theorem checkout. |
| 4. to 7. | Not started. |

Observed on this Mac's window server, with no person at the machine: the
installed app restores its saved form at the saved frame, in one visible
window. The window settles only while a display is awake; with every display
asleep, AppKit's ordering animation stops partway, at about 98% of the
window's size. A second Theo window of the same size stays ordered out, never
on screen; what it is has not been traced. Not observed yet: focus and the
global summon with a person at the machine, and a picture of the real screen
(the shell taking it has no screen-recording permission).
