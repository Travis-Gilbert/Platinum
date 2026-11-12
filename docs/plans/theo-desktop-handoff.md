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
| `claude/theo-stage-c-fe5831` | #26, draft, base `claude/theo-desktop-host` | `src/computer/` (Stage C broker), `script/fetch-cua-driver` |

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
   (`mac::pointer_in`, in window points from the top left, measured from the
   face's centre wherever the face sits in the window); the window is placed
   by a fraction of the screen's visible frame so a place survives resizing.
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
     Right-clicking the face opens a gpui-component `PopupMenu` below it, the
     way a browser's context menu opens under the pointer: the face stays
     where it is and the window grows around it to 344 by 572 (`OPEN_SIZE`).
     Near the bottom of the screen the menu opens above the face instead
     (`mac::menu_frame`, which also keeps the window on the screen beside an
     edge). The menu is 320 pt wide and never changes width. It has no title
     row (the owner asked for that space to go to the conversation). It holds
     the conversation as a trail, an "Ask Theo" field focused at once, "New
     Conversation", then Stop while a run goes on, or "Use the Demo Authority"
     when native Theorem is not configured.
   - **The trail** (`trail.rs`) is every turn of the selected conversation,
     oldest first, scrolled to the newest, at most 260 pt before it scrolls.
     What the person asked sits to the right in a tinted block; each Theorem
     answer is a read-only auto-grow `Textarea` (gpui-kit) whose content is
     `InputContent` with inline tokens: a citation reads as its label with a
     book icon, an attachment as its name with a file icon, an artifact as the
     first line of its fallback text with a frame icon (`reply_content`,
     `token_icon`). An answer's text box is kept by message id and its content
     replaced only when it changes. Insight and system messages are not shown.
     While a run is live the trail ends with "Theo is working on it.", which
     goes when the run ends, as Stop does. The trail follows the driver only
     when the driver changes, not every frame.
   - **"Ask Theo" is an auto-grow `Textarea`** (1 to 6 rows) with
     `submit_on_enter`: it wraps, Enter sends, Shift-Enter starts a new line.
   - Stop, New Conversation and the demo keep the menu open. **Dismissal**
     (owner's rule): working in another application keeps the menu open;
     clicking in the application Theo sits over, away from the menu, closes
     it (`Body::close_if_left_for`: once the menu has given up the keyboard,
     the frontmost application is read after 150 ms, and the menu closes when
     it owns the ordinary window directly beneath Theo's, or is the Finder
     when only the desktop is beneath; `mac::front_is_beneath` reads the
     window server's list, so Platinum's own windows count like any other
     application's). The application captured when Theo was called is not
     used for this: Theo can be moved, and it may be over a different
     application, its own included, by the time of the click. Escape and a click on the
     menu's own window outside the menu also close it. Closing shrinks the
     window back to the face and hands the keyboard back to the application
     Theo was called over. A left click on the face only drags it. The window
     title is "Theo: " and the conversation's first words.
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
- Right-clicking Theo showed a caret in "Ask Theo" but typing did nothing.
  `unframe` changes the style mask, and AppKit then made the window's content
  view first responder in place of GPUI's input view, so keystrokes never
  reached GPUI. `unframe` now restores the first responder it found (the log
  says "restored: true").
- Clicks on the menu fell through to the application beneath. The canvas that
  measures the menu for the hit test was absolutely positioned with no inset,
  so GPUI left it at its static place, after the menu: the measured bounds
  were a menu's height too low. It is now pinned to the menu's top left.

- **A finished run looked live for minutes** (the owner waited about four
  minutes with the spinner and Stop showing after the reply was complete).
  Replaying the node's stream showed `RUN.CLOSED` arriving; the client's
  `stream_run` only checked for the terminal event after the connection
  closed, and the node keeps it open after the terminal event. The client now
  returns as soon as the terminal event is delivered. The fix is in the
  Theorem dependency (`apps/theoremweb/crates/agent-runtime`, native and
  browser clients) and is carried as `patches/theorem-run-terminal.patch`,
  applied by `script/bootstrap` (see `patches/README.md`).
- A second question replaced the first answer: the menu showed only the latest
  Theorem message. It now shows the whole conversation (the trail above).
- A long question ran on in one line: "Ask Theo" was a single-line `Input`.

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
- `cargo test --locked --bin frame theo_desk`: 17 passed (activity
  generations, stale stops, snapshot, liveness, the Expression subset, the
  plate's rounded corners and its outline; the menu opening under the face
  without moving it, over the face at the bottom of the screen, and staying
  on the screen beside an edge; a reply's words and tokens in order, a token
  with no name still named, and an empty reply; the trail reading the
  person's words joined, Theorem's reply, leaving out empty turns, insights
  and system messages, and showing only when it has a turn or a state).

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

- After the menu redesign (`run16`, native Theorem through the local node):
  over TextEdit, `control+alt+KeyT` shows Theo and TextEdit stays frontmost.
  Right-clicking the face, which sat near the bottom of the screen, opens the
  menu above it with the face held in place. A long reply from an existing
  conversation filled the textarea to 12 rows and scrolled. "New
  Conversation" took the click, the menu read "New chat" and "Say what you
  need.", the reply shrank to one row and the menu shortened from 374.5 to
  154.5 pt with its edge by the face fixed, and the node listed a new "New
  chat" thread. A click inside Theo's window above the menu went through to
  TextEdit, which took the caret, and the menu closed.

- The owner, on the `run17` build: typing into "Ask Theo" works with real
  keys and a question gets Theorem's reply in the menu. Their report is what
  led to the trail, the wrapping field, the dismissal rule, the run-terminal
  fix and the title's removal.

- The owner, on the `run18` build: clicking the window behind the menu did
  not close it; only a click on the menu's window did. `run18` compared the
  frontmost application with the one captured when Theo was called, while
  Theo sat over Platinum's own browser window. `run19` judges by the window
  beneath Theo instead (three tests over the beneath choice).

- `run20`, with no key pressed and no menu item chosen: the window server
  lists Theo's face (layer 101, 264 by 132) beside the browser window (layer
  0) moments after launch, the face over the browser window. A first launch
  started before the previous instance had exited showed no windows and an
  empty log until relaunched; wait for the old process to go before starting
  a new one.

### Not yet observed

- **The beneath-window dismissal** (`run19`, built and launched): a click in
  the application whose window is beneath Theo closes the menu, and a click
  in another application's window elsewhere keeps it open.
- **This round's changes in the running application** (`run18`): the trail,
  the wrapping field, and a run
  ending (Stop and "working" going, the eyes returning) when the reply is
  done. The screen was locked when the check began, so nothing of it has been
  seen. Wheel scrolling over an answer inside the trail is also unchecked.

- Typing into "Ask Theo" with real keys, and a sent question's reply. The
  automated check cannot type here by design: Theo's panel takes the keyboard
  without making Platinum frontmost, and the checking tool refuses to send
  keys to an application that holds the keyboard behind another. It needs
  the owner's hands.
- Stop from the menu, and the draft kept across a hide and show (both need
  typing first).
- The menu opening below the face (the face was near the bottom edge in every
  check; the geometry is tested).
- The double right-click and its Input Monitoring prompt.
- **The gate**: invoke over Firefox and an editor, converse with the selected
  agent, dismiss and remount without losing state or stealing focus. The
  automated check could not do it: Firefox is read-only to computer use, and
  the owner was using the machine. It needs the owner's hands.

### Native Theorem on this Mac

Theo talks to the local Theorem node at `127.0.0.1:8390`, named by
`THEOREMWEB_AUTHORITY_URL` in the worktree's `.env` (gitignored), with
`THEOREMWEB_AUTH_TOKEN_FILE` pointing at
`~/Library/Application Support/Platinum/theorem.token`, a file only the owner
can read. Theo and the browser's Agent place share the one driver, so both
show the same conversation. The Fly instance is not wired: its chat routes
answer 403 `chat_scope_required`, and no token on this Mac carries the
`tenant:read` scope they need.

## Stage C: where it stands (2026-11-12, Mac session)

Branch `claude/theo-stage-c-fe5831`, based on `claude/theo-desktop-host`. The
broker is `src/computer/` in `frame` (macOS only). The driver is Cua Driver,
run in process.

### The driver comparison

Cua Driver 0.47.23 (`cua-driver-rs`, C ABI 1.1, release tarball sha256
`694b2dcf…d1d7`) against agent-desktop, both driven over MCP from the same
probe on this Mac, three passes. These are the third pass's figures. Each
driver used its own temporary Firefox profile, and neither touched the
owner's windows.

| Measure | Cua Driver | agent-desktop |
| --- | --- | --- |
| TextEdit snapshot | 192–1961 ms, mostly 200–450 ms; 15 elements | 400–3285 ms; 14 elements |
| TextEdit type, verified by read-back | 627–788 ms | 203–554 ms |
| Window move | 59–127 ms | 470–2064 ms |
| Old element ref after a window move | works | works |
| Old ref after a re-snapshot | refused as stale, explicitly | silently re-resolved and typed |
| Focus of the person's front app | preserved | preserved |
| Paced typing, 10 steps at 75 ms | `run_steps` 5810 ms, all landed | reported TIMEOUT, yet all 10 landed |
| MCP `notifications/cancelled` at 1 s into 40 paced keys | run 1: all 40 landed, response suppressed; run 2: none landed | 48 landed, response suppressed |
| Finder | 176 elements, 180–200 ms; AXOpen reported -25205 yet opened the folder; back worked | 111 elements, about 620 ms; double-click POLICY_DENIED; back TIMEOUT |
| Preview | 40 elements, 51–80 ms | 41 elements, 400–545 ms |
| A modal dialog from `osascript` | found by pid; Proceed pressed in 56 ms and verified | absent from the window inventory |
| Firefox page, observe only | 16 → 27 elements, 19–27 ms | 33–34 elements, 330–500 ms |

Preview was inconclusive for both. In Firefox neither driver reached the
page's button or input, though both found the link.

**Cua Driver was selected.** It refuses a stale reference instead of quietly
re-aiming it, which is the spec's "bind targets to the observation". It
reaches dialogs, observes several times faster, and moves windows an order
of magnitude faster. It also ships a C ABI, so it runs inside Platinum with
no daemon or second TCC identity. agent-desktop types faster, but it
re-resolves stale references and misses dialogs, and neither of those can be
worked around from outside the driver. Neither driver honours MCP
cancellation of a paced sequence once it is under way, so the broker does
not rely on the driver for interruption (below).

The Cua SDK crate pins `zune-core = "=0.5.1"`, which conflicts with the
locked `image`, so `src/computer/cua.rs` binds the ABI with `libloading`
instead. `script/fetch-cua-driver` downloads the pinned tarball, checks its
sha256, and extracts `libcua_driver_sdk.dylib`, its header and licence into
`deps/cua-driver-rs-0.47.23/`. The library is found through
`FRAME_CUA_DRIVER`, then the bundle's `Frameworks`, then that folder.

### The broker

- **Observe.** `Broker::observe(target, capture)` reads one window's
  Accessibility tree (at most 400 elements in 5 s) with a capture of that
  window alone, so Theo's own windows are excluded by construction. Each
  observation has an id and keeps Cua's snapshot. If Cua refuses the
  capture, the element tree alone makes the observation; `capture_missing`
  says why when there is no picture.
- **Resolve.** `contract::resolve(query, observation)` gives One, None or
  Ambiguous. An action names an element by its index in a given observation,
  never by pixels, so layout does not move its aim. There is one coordinate
  transform, `Rect::in_capture`, from screen points to capture pixels.
- **Act under a generation.** `take_control()` starts a generation and
  records the HID input counters (key down, mouse buttons, scroll). The check
  is made at the write: before sending, and polled while the Cua call is in
  flight, which is then cancelled through the ABI. Any input from the person
  revokes the generation, and Stop (`computer::stop()`, called first in
  `Desk::stop`) revokes it outright. A dispatch for an old generation, an old
  observation, or a missing element is refused before anything is sent.
- **Verify.** After a dispatch the element is read back from a fresh
  observation. Outcomes: `verified`, `delivered` (no read-back possible),
  `refused` (revoked, stale, no element, executor, unavailable), or
  `uncertain` (sent, but the application does not show it, or the call was
  cut off).
- **Receipts.** Every action appends one line to
  `<profile>/computer/receipts.jsonl` and writes `a<action>.png`: the
  capture it was aimed at, with the target outlined.
- **Interruption and resume** (`task.rs`). Typing goes in pieces of up to
  eight characters, each one atomic Accessibility insert. A revoked step
  waits for the person to be idle, takes control again, and resumes from a
  fresh observation. What is left to type is worked out from the field as it
  now reads (`contract::remaining`), never from what was sent, so nothing is
  typed twice. A press is never sent twice: a press whose outcome is
  uncertain stops the task for a person to look at. `--theo-task <file>` runs
  a task file, for verification only.

### Findings that shaped it

- **Cua trims values** (`get_window_state.rs`:
  `value.map(str::trim).filter(|v| !v.is_empty())`). An empty field has no
  value at all. Whitespace at the edges of what landed is therefore
  invisible, so a piece never ends just before whitespace (an eight-letter
  word takes its space along). Reconciliation takes the longest prefix of
  the text that fits what landed: it may lose a space, but it never doubles
  one. Measured: 3014 characters set, 3010 read back, no truncation at 3000.
- **`type_text` inserts at the caret,** and TextEdit opens a document with
  the caret at 0. So `remaining` finds what landed wherever the caret was,
  not only at the end.
- **Cua refuses every capture of the gate window** with "capture binding
  failed: capture binding is invalid. Not dispatching click." The window is
  on a display above the main one (negative y). The picture Cua wrote is
  still correct, the window alone at 2x, so the broker keeps it and reads the
  elements in a second call. The binding is what Cua's pixel-aimed clicks
  would need, which the broker never uses.
- **Cua's `is_on_screen` was false** for a window the window server listed
  as on screen. A task therefore finds its window by title and uses
  on-screen only to order the candidates.
- **The HID counters are system-wide.** Any key, click or scroll in any
  application revokes control, even input aimed elsewhere. A key event
  posted at the HID tap (`CGEventPost`) counts as the person, which is how
  the gate simulates one (`HID key-downs 456 -> 457`). Cua's Accessibility
  typing does not count. A sample of every counter across each passing gate
  run (90 s and 60 s, 22 typing actions each) changed once per run: the
  injected key-down.
- The boot disk filled up during the comparison (ENOSPC), then the Mac
  rebooted and `/tmp` was wiped, raw comparison JSON included. The figures
  above were recorded before that.

### How to run it

```bash
script/fetch-cua-driver
```

```bash
script/media-env cargo test --locked --bin frame computer
```

A task file names a window (`{"pid":…,"window":…}` or
`{"app":"TextEdit","title":"…"}`) and steps (`find` a role and label, `act`
with `press`, `type` or `set_value`, optional `until`). Run
`frame --theo-task <file>` from a process that holds Accessibility (and
Screen Recording, for captures and titles). The report goes to
`<file>.report.json` and the journal to `<file>.journal.json`.

### Tested (macOS, this session)

- `script/media-env cargo test --locked --bin frame computer`: 14 passed.
  Covered: resolution; outcomes; pieces; `remaining` with the caret not at
  the end, with trimmed values, and with partial landing; never sending a
  press twice; a padded value counted as set; resuming typing where it
  stopped; divergence; task-file parsing; and the Cua reply parsers against
  recorded shapes.
- `script/media-env cargo build --locked`: builds.

### Observed in the running application (the gate)

Platinum debug build, its own profile (`HOME` set to a gate folder), run
under an app holding Accessibility and Screen Recording, against a TextEdit
document `theo-gate.txt`. The task: type "Theo typed this sentence into an
existing TextEdit document in small pieces, was interrupted by the person,
and resumed without typing anything twice." The interruption was
**simulated**: an F19 key press posted at the HID tap, not a person's hand.
The gate passed twice.

- **Final run** (receipts with pictures). Actions 1–4 (generation 2) typed
  "Theo ", "typed ", "this ", "sentence ", each verified by read-back. The
  F19 was posted while action 4 was in flight (started …362925, posted
  …363347, ended …363397, in ms). The broker logged "the person took over;
  generation 2 revoked". The cancelled call's atomic insert had already
  landed, and the read-back says so: action 4 is `verified`, not assumed.
  About 3 s later (the task's idle wait) the task resumed from a fresh
  observation under generation 4. It worked out the rest from the field as
  it read, and typed actions 5–22 ("into an " … "twice."), all verified,
  including a word split mid-way ("interrup", "ted by "). Report: finished,
  1 takeover, 1 resume, the field holding the sentence exactly once. Each of
  the 22 actions left `a<n>.png`, the window with the text area outlined.
- **First passing run.** The same task and result, with the F19 landing
  during action 5. That run's receipts had no pictures (the capture refusal
  above, since fixed).
- **An unplanned takeover.** A re-run of the task, which typed the sentence
  a second time because a new run types its text again (the journal resumes
  only within a run), was revoked once with no injection from this session.
  Its source is unknown: the owner, or another session's input. It resumed
  after the idle wait and doubled nothing.
- **Before the fixes.** One run lost a space ("sentenceinto"), because a
  piece stopped just before the space. The task stopped itself with "the
  field changed under the step" instead of guessing. In that run the
  System Events key press never arrived (AppleEvent timeout), hence the
  HID-tap injection.

### Not yet observed or not done

- A person's own hand taking over. The gate's interruption was simulated.
- Clicks and `set_value` under interruption. The gate typed; a press is
  covered only by the rule that it is never sent twice.
- Cua's own clicks against the HID counters. Its typing does not move them;
  clicks were not measured in a quiet room.
- Theo's own surfaces. The Stage D capability seam
  (`theo_desk::capability`, branch `claude/theo-stage-d-6d67c9`) is not on
  this branch. The adapter (`install_broker(Arc::new(adapter))` after
  `computer::init`) is a follow-up for whichever branch merges second. Its
  mapping is in the shared notes (`stage-c-broker-reply.md`,
  `stage-d-reply-to-stage-c.md`).
- An `Act::Key`, to place the caret before typing, and the agent cursor as a
  live annotation, rather than receipts afterwards.
- Bundling the dylib into the app's `Frameworks` in the release script.

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
- **Takeover is system-wide (Stage C).** Any key, click or scroll anywhere
  revokes Theo's control of the desktop, even when the person is typing in
  another application. That is the safe reading of the spec. Scoping it to
  the target application would need an event tap, and with it Input
  Monitoring permission.
- **Screen Recording (Stage C).** Without it the broker still acts through
  Accessibility, but it has no captures and no window titles (it falls back
  to the Accessibility title). Grant it to Platinum, or accept receipts
  without pictures.
- **Equal standing.** Among activities of the same standing the latest begun
  has the face, so sound shows over a run. It is a one-line change in
  `Presence::primary` if the other order is wanted.

## Direction: Platinum is used through Theo

The owner, 2026-11-12: Theo is not to be extracted from Platinum. It is to
become the way Platinum is used. So Theo appears when Platinum opens
(`theo_desk::init` defers `Desk::appear`), and its key and the menu bar's
item still show and hide it. Theo's code treats Platinum's own windows as one
more application it can sit over (the dismissal rule already does). Theo
reaches the rest of Platinum through `crate::theorem` (`TheoremDriver`) and
`crate::install_menus`; that is expected, not a seam to cut.

The face's shape transformation (spec section 6: the eyes broaden into a
content backing and the body stretches into a bezel, hosting Platinum's
`Page`) is Stage E below, and is not started.

## Later stages

C (computer use) is built, and its gate has been observed (see Stage C
above). D (Herdr attach, MCP and typed API) and E (expandable local browser
reusing `Page` and `Engine`) are being built on their own branches; see them
for where they stand. F (remote computers) is not started.

## Standing rules that bit or nearly did

- Never invent amplitude: an unmetered boombox shows no level.
- Hiding never cancels work; stop is explicit and routed.
- A passing pure-Rust test is not a claim about the native application.
- Prefer `cargo build` to `cargo check` here (see `AGENTS.md`).
