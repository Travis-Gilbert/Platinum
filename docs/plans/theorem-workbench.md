# Theorem workbench: the Agent place

The Agent place on Home opens Theorem inside Platinum: a navigator of
projects and conversations beside one elevated work card holding the
conversation. This is the first slice of the native Theorem workbench; the
checklist in `theorem-workbench.checklist.json` tracks it.

## What this slice builds, and what waits

Builds now, wired to the real authority:

- The workbench shell in the Agent place: navigator on the chrome, the work
  card raised above it, their split resizable and remembered.
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
  answered with a notice saying so, never silently dropped.

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
| Which tab shows the place, opening sources in tabs | `Browser` | Per window |

One driver and one workbench view per window. When two panes show the Agent
place, the active tab mounts the workbench if it shows the place, otherwise
the first displayed pane that does; any other says where it is, with a way
to bring it there. One view never renders twice in a frame.

Every transport call settles back through the driver, with a generation
check so a superseded answer (the reader moved on) is dropped, and every
projection is published, including ones that carry only a refusal.

## Layout

Two levels, told apart by surface, not by color:

- **Navigator, level 0.** On the tab's chrome pane (`glass::pane`), no fill
  of its own. Default 15rem, 12.5 to 22rem, resizable from its trailing
  edge, width remembered.
- **Work card, level 1.** Inset `FRAME` from the top, trailing and bottom
  edges, `radius_lg`, `theme.background` opaque (dense text never sits on
  glass), hairline `theme.border`, `shadow_sm`. Opaque in Reduce
  Transparency too, where the window has no glass.
- **Overlays, level 2.** Kit popovers and menus.

Density, denser than Linear and about Devin's:

- Navigator rows 1.625rem, `text_sm`, a leading status glyph and a trailing
  time lane in `text_xs` muted; section headers `text_xs` semibold muted,
  1.5rem; 0.125rem between rows.
- Card header 2.25rem: conversation title, project, run state, Stop while a
  run streams, an overflow menu.
- Transcript column at most 44rem, centered in the card; 1rem between
  turns, 0.375rem between a turn's parts.
- Activity rows (reasoning, tool calls, progress) 1.375rem, monospace tool
  names, collapsed by default, expanding in place; the grouped summary reads
  like Devin's "Worked for".
- Composer: an inset field at the card's foot, `theme.secondary` fill,
  growing from one line to eight, the send control at its trailing edge.

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
| No conversations | Projects, empty list | "Ask Theorem anything" with the composer |
| Ready | Projects, conversations | Transcript, composer |
| Streaming | Row shows Running | Activity streams, Stop |
| Needs approval | Row shows Needs approval | Approval with Approve and Decline |
| Failed / stopped / interrupted | Row says so | The turn says so; Retry where the runtime allows |
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

- Unit: recency grouping, part-to-row mapping, single-mount rule.
- UI integration (`#[gpui_kit::test]`) against `DeterministicDemoAuthority`:
  the place mounts the workbench, the demo hydrates, selecting a
  conversation shows its transcript, sending streams a reply into the card,
  Stop settles it, the not-connected state renders without the env vars.
- Mounted: the native visual check (`FRAME_WORKSPACE_CHECK=<output>
  script/media-env <frame>`, snapshot feature) draws the Agent place in the
  production window, light and dark: not connected, the demo with a
  conversation open, a message sent and answered, and the workspace with
  every tab closed ([captures](evidence/theorem-workbench-2026-10-29/)). Its
  captures hold GPUI's layer, not the glass's vibrancy behind it. The
  configured authority, read-only, is a check by hand in the running app.
