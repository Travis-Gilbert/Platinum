# Theorem workbench: the Agent place

The Agent place on Home opens Theorem inside Platinum: a navigator of
projects and conversations beside one elevated work card holding the
conversation. This is the first slice of the native Theorem workbench; the
checklist in `theorem-workbench.checklist.json` tracks it.

## What this slice builds, and what waits

Builds now, wired to the real authority:

- The workbench shell in the Agent place: navigator on the chrome, the work
  card beside it, their split resizable and remembered.
- The places bump, the empty state's starters and the composer as the card's
  bottom bump, lifted from the design reference (Layout).
- Conversation: projects, a project's conversations by recency, open one,
  read its whole transcript (text, reasoning, tool calls and results,
  approvals, citations, attachments, errors, usage, the rest as honest
  rows), send, follow the run as it streams, stop it, resume a live run
  after a restart, answer approvals, start a new conversation, copy, retry
  and branch between answers, attach files.
- Every state the authority can be in: not connected, connecting, signed
  out, refused, empty, ready, streaming, needs approval, failed, stopped,
  interrupted.

Waits, and why:

- **Code workspace** (GPUI Kit editor, DocumentSession, theorem-ide-host
  language servers, the ELY terminal). ELY is only on `feat/kit-081-ely`,
  which needs GPUI Kit 0.8.1 and is behind `main`; the editor surface is its
  own slice once that port lands. This reorders the pasted plan's steps 2
  and 3: the conversation's data path exists today, the code workspace's
  does not.
- **Graph editor.** Graph, program and artifact parts render as rows that
  say what they are; opening them needs the programmable-graph surface.
- **Tasks and memory panes.** The runtime has a task-list client but the
  chat host projects no tasks or memory yet; they become navigator sections
  when it does.
- **Voice, dictation, read aloud, capability customization, extension
  slots, Life Agent integrations, Teable tools.** No Platinum owner exists
  for speech or for contributed tools. The controller's effects for them are
  answered with a notice saying so, never silently dropped. The reference's
  microphone in the composer waits with them: GPUI Kit's `SpeechButton`
  needs its `speech` feature, new dependencies and the bundle's microphone
  and speech-recognition usage strings, without which macOS ends the
  development binary.
- **The rest of the reference's card.** Its zoom (100%) and canvas tools
  (hand, shapes, checklist) belong to a canvas the card does not hold yet,
  and its "Reads Calendar, Gmail, Slack" lines name connectors Platinum does
  not have, so the starters claim none. Zephium folds the canvas tools away
  while its work bar is in use; with no tools there is nothing to fold yet.
- **Pages in what a send will use.** The composer shows the project, an
  edit's references and the attachments. Platinum sends no page or object
  context with a message (`ComposerIntent::Submit`'s `context` stays empty),
  so the composer names none rather than claim one.
- **The top search opening.** Zephium's breakdown gives the search field at
  the top of a frame the same curve as the composer at its foot. That would
  move Home's search field into the frame's top edge, a change to Home rather
  than to this place, so it waits for its own decision.

## Data: bound, not rebuilt

Theorem already ships the renderer-neutral host this needs,
`theoremweb-agent-runtime` (`apps/theoremweb/crates/agent-runtime`), in the
Theorem checkout Platinum pins (`deps/theorem-search`, `script/bootstrap`).
Platinum binds it rather than writing a client:

- `ChatHostController` owns all host state: the one `ThreadRuntime`, the
  selection, the composer, owed stops and refusals. It takes
  `ThreadIntent` / `ComposerIntent`, returns `ChatHostEffect`s, and projects
  `ChatHostProjection` (`thread: ThreadDocument`, `composer:
  ComposerDocument`, projects, threads, phase, notice).
- `HttpChatTransport` fulfills effects against the personal authority over
  reqwest (bearer token, CSRF, idempotency keys, SSE run events with
  reconnect). `DeterministicDemoAuthority` fulfills the same trait in
  process, for the demo and the tests.
- Theorem's own GPUI hosts (`apps/theoremweb/crates/gpui/*`) build on
  Theorem's Zed-fork `gpui` 0.2.2 and gpui-component 0.7, so Platinum cannot
  link them; `gpui/host/src/chat_driver.rs` is the reference Platinum's
  driver follows.

The native transport reads `THEOREMWEB_AUTHORITY_URL` (HTTPS, or loopback
HTTP) and `THEOREMWEB_AUTH_TOKEN_FILE` (a file readable only by its owner).
Platinum already loads `.env` from its data folder, so both go there; the
token itself never enters a settings file.

### Ownership

| State | Owner | Lifetime |
| --- | --- | --- |
| Projects, conversations, messages, runs | The Theorem authority | Durable |
| Selection, transcript runtime, composer, owed stops | `ChatHostController` | Per window |
| Transport, in-flight calls, generation, saved selection | `TheoremDriver` (`src/theorem/driver.rs`) | Per window |
| Navigator width, expanded rows, composer field, scroll | `Workbench` (`src/theorem/workbench.rs`) | Per window |
| The place the window shows, Always Show Places, opening sources in tabs | `Browser` (`src/browser/place.rs`) | Per window; Always Show Places saved with the window |
| Starter role | `Workbench` (`src/theorem/starters.rs`) | Per window, not saved |

One driver and one workbench view per window. A place belongs to the
window, not to a tab or a pane: it shows over the tabs or the workspace and
leaves them as they were, so the workbench draws once, wherever the window
is. Which starter is picked is not stored: it is the one whose prompt the
composer begins with.

Every transport call settles back through the driver, with a generation
check so a superseded answer (the reader moved on) is dropped, and every
projection is published, including ones that carry only a refusal.

## Layout

The layout is lifted from the design reference (a glass column, one card
with a dot grid, a centered empty state, a composer that bumps up from the
card's foot), with one change: the reference's bump runs vertically on the
window's leading edge and holds the places.

A place is part of the application, not a page, so no page chrome stands
over it: no tab row, sidebar, Back, Reload or address bar, in the regular or
the compact layout. It shows over the tabs or the workspace and leaves them
as they were. Back and ⌘W leave it for what is beneath; Focus Address Bar
leaves it for the address bar; opening a page Theorem asks for (a sign-in
page, a source) leaves it for the tab that shows the page.

The places bump (`PlacesBump`, `src/places.rs`) is how a person moves
between places: the way out first (Home over the workspace, Show Tabs over
the tabs), a divider, then the six places, the one showing selected. In the
Agent place it stands on the window's leading edge, centered, on the
navigator's glass, which keeps its width clear (`bump::standing`); the card
beside the navigator has only the composer cut into it. The other places
have no navigator, so there it stands out of the card's leading edge, a
frame's width from the window's, and the card's outline runs around it
(`bump::card`). It shows with a place by default; **Always
Show Places** (View menu, palette) stands it over Home and the tabs too,
saved with the window. There, over a page it cannot be cut into, it is
solid; over the workspace it fades in from the frame. Home still lists the
places under its search field.

A bump and the notch it makes share one outline (`bump.rs`, after Zephium's
`composer-hole.ts`), so the card's fill, the bump's body and their edges
cannot drift apart: an eased fillet twice the corner radius where it leaves
its base, tip corners and the card's own corners at 1.5 times `radius_lg`.
A bump is one color, the theme's `sidebar_border`, clear at its base and
full by 45% of its reach, so the window's material carries on into it. The
edge is one stroke of one weight drawn with the card's: from the card's
sides through each flare, round the bump and back out, so no line ends
beside another and none runs across where the card's edge would have been.
The card's dot grid is
gpui-whiteboard's own (`paint_board`), aligned across the card, kept off the
bumps and their flares.

Two levels, told apart by surface, not by color:

- **Navigator, level 0.** On the chrome: the tab's chrome pane
  (`glass::pane`) in a tab, the frame in the workspace. No fill of its own:
  the window's Liquid Glass shows through. Default 15rem, 12.5 to 22rem,
  resizable from its trailing edge, width remembered.
- **Work card, level 1.** Inset `FRAME` from the top, trailing and bottom
  edges, `radius_lg`, the theme's raised surface `theme.popover`, opaque
  (dense text never sits on glass), edged with `theme.sidebar_border`, with
  a dot grid and the two bumps cut into its outline. No shadow: it would
  outline the notches. Opaque in Reduce Transparency too, where the window
  has no glass. Measured in OKLCH with CIEDE2000, compositing the glass in linear
  sRGB over black, dark grey, white and blue desktops. In the light theme
  the general `border` is the frame's own color (ΔE00 1.8 to 2.9 from the
  navigator, under what reads) and `background` is a flat grey among the
  frame's cool greys. The white raised surface and the sidebar line make
  that ΔE00 3.2 to 4.3 for the fill and 6.6 to 7.6 for the edge. In the
  dark theme both tokens fall back to `background` and `border`. The edge
  carries the separation (ΔE00 6.4 or more), because over a black desktop
  the fill matches the navigator (ΔE00 0.3).
- **Overlays, level 2.** Kit popovers and menus.

Density, denser than Linear and about Devin's:

- Navigator rows 1.625rem, `text_sm`, a leading status glyph and a trailing
  time lane in `text_xs` muted; section headers `text_xs` semibold muted,
  1.5rem; 0.125rem between rows.
- Card header 2.25rem: the conversation title as a menu (new conversation,
  sign in, reconnect, the demo), project, run state, Stop while a run
  streams.
- Transcript column at most 44rem, centered in the card; 1rem between
  turns, 0.375rem between a turn's parts.
- Activity rows (reasoning, tool calls, progress) 1.375rem, monospace tool
  names, collapsed by default, expanding in place; the grouped summary reads
  like Devin's "Worked for".
- Composer: the card's bottom bump, in the content's lane, growing upward
  from one line to eight with its base on the card's foot: attach, a
  divider, the field, send or stop. At rest it is 39rem wide and only that
  row; in use (focused, written in, or carrying an edit, a quote or
  attachments) it widens to the column's 44rem and names what the send will
  use: the project, an edit's references, the attachments. The widths follow
  Zephium's work bar (620 and 700 points). The width changes at once for
  now. Zephium eases it, and GPUI can too; that waits until the edge is
  settled. An edit, a quote, queued sends and the
  reason a send is refused sit above the row.
- Transcript scrolled under the card header fades out beneath it.
- Empty conversation: "What should we get done?", role tabs (Developer,
  Designer, Marketing, Manager, Student, Office, Founder) and two starters
  per role, each a sketch, a title and a sentence. A starter fills the
  composer with its prompt and focuses it; the hint says Return starts it.
  The starters are Platinum's own text.

Texture and type do the work color cannot: the user's turns sit on a
filled block, Theorem's read as plain text, activity reads in monospace,
selection is a fill of its own, and every status says its word as well as
showing its glyph (WCAG 1.4.1).

## States

| State | Navigator | Card |
| --- | --- | --- |
| Not connected | Empty | What to set, where, and "Try the demo" |
| Connecting | Skeleton rows | Skeleton transcript |
| Signed out | Empty | The authority's sentence, "Open sign-in page" |
| Refused | Empty | The authority's sentence, Retry |
| No conversations, or nothing said yet | Projects, empty list | The starters, with the composer |
| Ready | Projects, conversations | Transcript, composer |
| Streaming | Row shows Running | Activity streams, Stop |
| Needs approval | Row shows Needs approval | Approval with Approve and Decline |
| Failed / stopped / interrupted | Row says so | The turn says so; Retry where the runtime allows |
| Run never closed | Row shows Interrupted | The turn reads Interrupted, the notice says to start a new conversation, the draft stays |
| Demo | "Demo data" badge | The demo notice |

## Keyboard and focus

| Key | Where | Does |
| --- | --- | --- |
| ⌘N | Workbench | New conversation, focus the composer |
| Enter / ⇧Enter | Composer | Send / new line |
| Escape, ⌘. | Composer, workbench | Stop the run when one streams |
| ⌥↑ / ⌥↓ | Workbench | Previous / next conversation |
| Tab | Workbench | Navigator rows, then the card header, then the composer |

Opening the place focuses the composer. Opening a conversation keeps focus
in the navigator when it came from there. Escape in an overlay closes it and
returns focus to its trigger.

## Verification

- Unit: recency grouping, part-to-row mapping, a settled answer holding only
  unfinished activity reading as incomplete, the bumps kept clear of the
  card's corners, a shallow bump's flare kept within its tip, the dot grid
  off the bumps and their flares, every role's starters with their
  own ids and prompts.
- UI integration (`#[gpui_kit::test]`) against `DeterministicDemoAuthority`:
  the place mounts the workbench, the demo hydrates, selecting a
  conversation shows its transcript, sending streams a reply into the card,
  Stop settles it, the not-connected state renders without the env vars, a
  starter fills the composer, the navigator's rows clear of the bump, and
  the composer at rest then widened in use, naming its project and growing
  upward from a fixed base. In the browser: a place over the workspace and
  over the tabs with no tab row, the Agent place's bump on the window's
  edge clear of the card, the bump moving between places and out to the
  tabs or Home, Back, ⌘W and ⌘L leaving a place, and Always Show Places
  over Home and the tabs, saved.
- Driver, against a demo authority that refuses what the real one refuses
  (a stale revision, a kept message whose run is refused, a forgotten CSRF
  session, a failed run written without its failure, a run lost across a
  restart): the next message waits until the authority has written the last
  turn into the conversation; a refused turn rereads the session and the
  conversation, keeping the draft; the first send or new conversation after
  the authority restarts renews the session and goes through, once, and a
  send refused again after that renewal is shown as refused; a run whose
  stream ended failed stays failed once the authority writes its turn; a
  run the authority lost is not followed again, and the conversation says
  to start a new one.
- Mounted: the native visual check (`FRAME_WORKSPACE_CHECK=<output>
  script/media-env <frame>`, snapshot feature) draws the Agent place in the
  production window, light and dark: not connected, the demo with a
  conversation open, a message sent and answered, and the workspace with
  every tab closed ([captures](evidence/theorem-workbench-2026-10-29/); with
  no page chrome over the place and the raised card,
  [2026-10-30](evidence/theorem-workbench-2026-10-30/)). With the lifted
  layout it also draws the starters, the composer at rest, Mail over the
  workspace and Always Show Places over Home ([theorem-places-2026-10-30](evidence/theorem-places-2026-10-30/)).
  Its captures hold GPUI's layer, not the glass's vibrancy behind it, so
  where a bump fades to clear the capture shows white, not the frame.
- Live: the live check (`FRAME_THEOREM_LIVE_CHECK=<output>
  FRAME_THEOREM_LIVE_STORE=<script> THEOREMWEB_AUTHORITY_URL=<url>
  THEOREMWEB_AUTH_TOKEN_FILE=<file> script/media-env <frame>`, snapshot
  feature) drives the Agent place in the production window against a
  disposable authority the script starts, kills and restarts: down at
  launch, Retry, a new conversation, a live answer, Stop mid-run, the
  authority killed mid-run and back, a second window following a live run,
  a model provider refusing the authority's key, and a refused credential
  then sign-in. It sends messages and kills the authority, so it never
  points at a shared one. Runs, the breaks they found, and what is still
  Theorem's: [theorem-live-2026-10-30](evidence/theorem-live-2026-10-30/).
  The hosted authority is not covered: neither token on this machine is
  accepted at theoremweb.com.
