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
     Right-clicking the face opens Theo's own panel below it, the
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
- **Enter closed the menu, so the answer arrived in a menu nobody could see**
  (owner, run20b). The menu was a gpui-kit `PopupMenu`. Kit's input calls
  `cx.propagate()` on Enter before it emits `PressEnter`, so Enter also
  reached the menu's own `enter` binding (`Confirm`), which dismisses on the
  selected item. Selection follows the pointer and includes disabled element
  items, so it was usually the field or the trail. A `PopupMenu` is a command
  list that closes when something in it is chosen, the wrong component for a
  conversation. The menu is now Theo's own panel: a column in the Kit
  popover style holding the trail, the notice, the field and a row of small
  ghost `Button`s. Nothing in it closes it. Only Escape does, or a click in
  the application beneath Theo (`close_if_left_for`). The right-click
  `keep_open` workaround went with the popup.
- **The field and the trail read faint** (owner). They were disabled menu
  items, which Kit dims. As plain children of the panel they take the theme's
  foreground; the person's turns take `secondary_foreground` on `secondary`.
- **The menu was a flat black** (owner: borrow Platinum's Liquid Glass, as
  the frame shows with every tab closed, and leave Theo's colour alone). The
  panel now lies on `glass::PanelGlass`. It is the frame's `NSGlassEffectView`
  with the same style and tint, sized to the panel alone, so the face and
  the clear margins stay clear. It is made on the menu's first paint, after
  `unframe` has restyled the window, and is placed from the measuring
  canvas's prepaint in AppKit's bottom-left coordinates. It is hidden when
  the menu closes. While Theo's window is inactive, or without the glass,
  the panel takes the theme's solid frame colour, as the frame does
  (`glass::panel`).
- **A refusal ran off the menu's edge.** The notice now wraps to the menu's
  width.
- **"chat revision conflict" on "what do you know about me" (not fixed
  here).** The node holds the message (thread
  `chatthread_sha256:85645c7ec0b527e8a719cf8b1`: created at revision 1, the
  `message.append` receipt at revision 2, 20:27:08Z) and no run for it:
  `MATCH (r:HarnessRun)` finds none for that thread, and the node log has no
  "started durable Theorem chat run" then. So the run start was refused, or
  the client never reached it. The likely cause: the node log shows a 57 s
  tantivy commit and GC starting at 20:27:11Z, close to the client's 60 s
  request timeout. If the append's answer did not return in time, the client
  retries an indeterminate request once (`retry_indeterminate`), and a retry
  racing a commit still in flight reads the thread one revision on. That has
  not been confirmed: a refused operation leaves no receipt, and the node on
  8390 is the mod node, built from Theorem on 2026-11-11, newer than this
  repository's pin. Its source has no revision check before a run is created.
  The full refusal text, which now wraps, will name the revisions the next
  time it happens.

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
`Page`) is Stage E, in the next section.

## Stage E: the screen (2026-11-12, Mac session)

The face opens into a small browser screen that holds a real Platinum `Page`,
folds back into the face without losing it, and can be pinned, paused and
played.

### How it works

- **Opening.** Typing an address into Ask Theo (`screen::address_typed`:
  one whole http or https URL; a bare host such as `example.com` stays a
  question for Theo) opens the screen instead of asking Theo. "Show Page" in the menu reopens a held page. The composer is put
  away without handing the keyboard back (`Desk::put_composer_away`): handed
  back first, macOS refuses Platinum taking it again for the screen.
- **The morph** (`morph.rs`). `Morph` runs Face, Opening, Screen, Closing and
  settles as Opened, Collapsed or Closed. The window grows frame by frame
  through AppKit (`mac::after_update`), centred on the face and kept on the
  visible frame. The body is a path between the face's plate and the screen's
  bezel; the eyes broaden into the content backing; the page's last picture
  shows once the backing is mostly there, and the live page once settled.
  Reduced motion changes form at once.
- **The page** (`screen.rs`, `src/page.rs`). `Session` holds the page through
  collapse, hiding and form changes, and lets it go only on Close Page. A
  collapsed page is rebound to the face's window with `Page::rebind`, which
  moves it without reloading. Each page has a generation, so a playback report
  about an earlier page is refused.
- **Controls.** The bar has Back, the address, Keep on Top, Pause or Play while
  the page has media, Back to Theo and Close Page. With the face collapsed,
  the menu has Show Page, Pause or Play, and Close Page.
- **Sound.** Servo's media session reports playback (`src/delegate.rs`,
  `Playback`); Pause and Play go back through
  `notify_media_session_action_event`. While the page plays, the face takes the
  boombox. The level is measured where Platinum's sound leaves for the speakers
  (`meter.rs`): a GStreamer sink ranked just above `osxaudiosink`, a bin of
  `audioconvert ! level ! osxaudiosink`, with each measurement held until its
  sound reaches the clock. No measurement means no level. Each start of
  playback is a new run of the sound activity, because `Presence` never begins
  a run that has ended.
- **Resizing.** The settled screen is resizable from its edges. Collapsing
  puts the face back at the same fraction of the screen it left from, so a
  smaller screen never leaves it hanging outside.

### Tested

`script/media-env cargo test --locked --bin frame theo_desk`: 47
passed, covering geometry, the morph's forms and reversal, the session's
page and generation rules, sound runs through `Presence`, the level mapping
and queue, the sink's `ts-offset`, and address parsing.

### Observed in the running application

These ran in an isolated copy (`dev.platinum.theo-stage-e`, its own HOME),
driven by computer use on this Mac:

- Typing `https://example.com` into Ask Theo opens the screen. The morph was
  seen midway: the dark bezel growing, with two white backings broadening from
  the eyes. It settles at 800x520, kept in the visible frame's bottom-right
  corner.
- The live page loads and scrolls, the bar's buttons are present with
  accessibility labels, and Platinum stays frontmost after opening.
- Keep on Top changes to Unpin and the screen floats above Platinum's own
  window.
- Back to Theo runs the morph backwards and the face comes back where it left.
  Show Page reopens the same page, still scrolled and still pinned.
- Dragging the left edge narrows the screen to its 360 pt minimum; the page
  reflows, and collapsing puts the face at the same fraction across.
- After a page has opened, the menu's field is empty and the action row wraps.
- A local page playing a looping 440 Hz tone puts Pause in the bar, and
  collapsing shows the boombox with sound waves, which appear only with a
  measured level. Pause in the menu turns the face back to its eyes and the
  button to Play. Pause then Play from the bar brings the boombox and its
  waves back. With the measuring sink's `ts-offset`, GStreamer's warning
  about it is gone from the log.
- Hiding and showing Theo keeps the page and its playback; Close Page takes
  the page actions out of the menu.

### Not yet observed

- Whether the sound stops on Close Page (nobody was listening). The page is
  dropped with the session.
- YouTube. Every Platinum window, not only the screen, fails youtube.com with
  "client error (SendRequest)" in this build, while example.com loads and
  curl reaches YouTube. It played in the build recorded in
  `docs/plans/evidence/youtube-2026-10-25/`, so it is a regression or an
  environment difference outside Stage E.

### Open question for the owner

What shape Theo takes while reading text aloud as opposed to playing music.
The boombox fits music. For reading, a "reading" form whose eyes pulse with
the voice's loudness is the suggestion; it needs authoring in the book.

## Later stages

C (computer use: compare Cua Driver and agent-desktop on this Mac, then the
observe, resolve, act, verify broker), D (Herdr attach, MCP and typed API),
E (expandable local browser reusing `Page` and `Engine`, now built: see
Stage E above), F (remote computers). C needs accessibility and
screen-recording permission granted on the machine.

## Standing rules that bit or nearly did

- Never invent amplitude: an unmetered boombox shows no level.
- Hiding never cancels work; stop is explicit and routed.
- A passing pure-Rust test is not a claim about the native application.
- Prefer `cargo build` to `cargo check` here (see `AGENTS.md`).
