# Theo for Theorem desktop
## Native presence, connected agents, computer use, and an expanding screen

**Status:** Revised implementation handoff, November 12, 2026 UTC. This revision incorporates Platinum PR #22 and Travis's expanded requirements. It supersedes the earlier implementation sequence. Repository and documentation inspection was read-only; no code was changed, native application was run, or Rust test was reproduced during this review.

## 1. Product direction and fixed scope

Theo is the life agent's native presence across the user's desktop. Theorem remains the owner of conversations, tasks, specialists, and durable memory. Theo brings that work into the application the user is already using, and can also open a useful working surface of its own.

**Invoking Theo over Firefox means invoking a native application while Firefox is the current application. It does not mean a Firefox extension.**

The first complete product must work across ordinary desktop applications through native observation and control. All browser-extension work is deferred until that experience is complete. Browser-specific extensions can later enrich semantic access; they are not a dependency for native co-use, the embedded browser, API integrations, or MCP.

The required native capabilities are:

- Invoke, converse, observe, guide, and act from anywhere in the supported desktop environment.
- Connect to existing Claude Code or Codex sessions through Theorem's Herdr integration.
- Create additional agent sessions through Claude Agent SDK and OpenAI agent APIs when the user requests them.
- Call search, browser, application, and other capability APIs.
- Expose the same native capabilities to outside agents through MCP and a typed application interface.
- Open a pasted URL in a resizable surface formed from Theo's body, including useful YouTube playback.
- Support optional local or remote computers, with their displays presented inside that surface.
- Allow programmable agent adapters, tool integrations, environment drivers, and device forms.
- Support configurable global summoning, including an optional double-right-click gesture.

Mac is the first implementation target implied by the existing GPUI/AppKit work. “Anywhere” means the applications and desktop surfaces the operating system allows the app to observe and operate. Platform restrictions and application-specific semantic gaps remain explicit.

The central design rule is that **Theo changes its visible affordances to match what it is doing**. Speakers communicate output audio; a camera communicates capture or a photo task; a record player communicates music; a screen becomes an interactive place to work.

## 2. Current implementation baseline

| Source | Observed state | Use in this project |
| --- | --- | --- |
| Platinum PR #22, head 7950be398bf99d53c67171d997a0afd89ec44eb5 | New pure Rust crates/theo, sourced from web Theo at 88043d3156d1a3bd993b2181d83c221b689eb9af; no GPUI consumer | Build the native host around this port |
| Platinum main, 46bbc63e7c4b391e4d2107a77097f1e5054a9831 | Existing GPUI application, Theorem driver, browser Engine/Page, speech wrapper | Reuse the current desktop and page rendering boundaries |
| Theorem PR #1021, head 88cf1eb0c1eb1b662ba2b951adf7e9a29e4de4e0 | Open, unmerged Herdr Core backend behind terminal.* | Integration dependency, not assumed deployed functionality |
| Theorem PR #1042, head 3440ade7d4e46d3c1733153f89ffbf3f2a0909f7 | Open, unmerged terminal ownership/control work | Adapt ownership conventions; desktop input needs a distinct resource |
| Theorem-UI PR #73, head 604432efe83b13226d7c88b173f850df4c6a4111 | Open, unmerged Herdr terminal profiles | Follow existing terminal identity and routing |
| Legacy companion PRs #864 and #871 | Capture/geometry, overlay and vision work in the older apps/theoremweb host | Selective reuse; not a ready-made current host |
| Current pet computer-use and Swift helper | Verification/types plus limited semantic AX actions | Preserve useful receipts and adapters while replacing the thin execution boundary |

PR #22 is substantial progress. Its tests contain compilation fixtures for 100 accepted and 18 rejected programs, 2,560 feature-seating samples, and six recorded scenarios with 145 snapshots. The traces inspect channel/shape state and sampled grain data; they do not exhaustively compare every field of every particle. The author's reported Linux test pass is useful compatibility evidence. It is not native rendering, focus, audio, action, or performance evidence. [Rust tests][P3]

The root package does not yet depend on theo. A renderer, native window, body travel, stage target resolution, audio measurement, activity coordination, and agent-operation integration remain. Platinum already receives an Expression part but renders a fallback message for it; this is a natural first real consumer. [Manifest][P2] [Expression fallback][P4]

Keep the existing package name frame and the repository's vendored GPUI/Servo boundary. Do not rename packages or replace the browser engine as part of Theo integration.

## 3. Resolve the four decisions from PR #22

### Core location: retain crates/theo in Platinum

Keep the pure visual engine where it is for now. It should accept time, measurements, events, and a Stage interface, and produce presentation state. Agent credentials, provider clients, windows, capture, actuation, and durable task ownership stay outside it. A later WASM/shared-core move remains possible without becoming the first milestone.

### Activity ownership: the desktop host coordinates activities

Introduce a host-owned presence coordinator. It maintains the current activity set, chooses the primary visible form, and restores still-running activities after a transient one ends. The core executes the selected visual program; it does not infer a multi-agent task graph from start/stop event names.

The current Face has one form layer and one form-ending matcher. A run can enter the thinking form, audio can replace it, and audio stopping can remove that form while the run still exists. Fix that at the host activity boundary, preserving the core's small job. [Form lifecycle][P5]

### Sound form: boombox is the desktop default

Apply a desktop profile that removes the default mouth rule and selects the boombox for output audio. Keep reference-port fixtures against the source web defaults unchanged, then test the native profile separately. Fixture compatibility does not require the product to retain the old 3:1 boombox/mouth choice.

Agent-authored forms remain possible. Stable audio semantics and real output measurements apply to them too. [Built-in rules][P6]

### Speech: build a measured playback path

The existing spoken.rs uses NSSpeechSynthesizer; AppKit owns synthesis and playback, and the inspected wrapper exposes no audio buffers. [Current speech][P7]

The target design is a common playback pipeline accepting audio buffers from a native or API provider. The speaker animation is measured against buffers actually played and the playback clock. Measuring ahead-of-time synthesis alone can animate speech before it is heard.

AVSpeechSynthesizer's buffer callback is one native source of speech samples. It is an option to evaluate, not an already implemented replacement. [Apple speech buffers][A1]

An intermediate build can use an honest, unmetered boombox whose active/inactive state follows synthesis lifecycle. Do not invent amplitude from token arrival or a timer. This fallback does not satisfy final audio-reactivity acceptance.

### Two bounded changes before host integration

1. **Complete the memory bound.** Memory::note caps the transient kinds map at 512, but independently inserts each name into days.today. Rollover and persistence retain other counters, and restored maps also accept unbounded input. Define a coordinated retention or overflow-aggregation policy across kinds, today, usual, and reload. Test the saved representation after high-cardinality input. [Memory][P8]
2. **Expose intentional performance cancellation.** Face::stop is private; unform only handles the form layer. Add a supported stop operation identified by invocation or owner/generation, so a stale stop cannot end a newer performance. [Face API][P5]

The old web follow/reduced-motion and release-binding issues are corrected in this port. They should not be reported as current PR #22 defects. The manually added lock entry is structurally plausible; no specific defect was found. The remaining workspace gate is script/media-env cargo test --locked -p theo on a bootstrapped checkout. This review environment has no rustc/cargo and did not reproduce that command.

## 4. Native architecture and ownership

These are logical responsibilities; they need not each become a separate process.

| Component | Owns | Does not own |
| --- | --- | --- |
| Theorem runtime | Conversations, durable tasks, specialists, memory, outcomes | Native window animation |
| Agent-session adapters | Existing-session attachment or explicitly new sessions; structured messages, images, tool events | Independent foreground input outside the broker |
| Capability service | Typed API and MCP dispatch, task scope, tool availability, receipts | Cosmetic form selection |
| Environment and surface registry | Device/computer identity, application/window/page targets, display connections | Agent conversation history |
| Observation/action broker | Semantic snapshots, captures, target resolution, serialized native input, verification | Planning a separate agent loop |
| Presence coordinator | Current activity projection, primary form, placement, captions, available controls | Durable completion inferred from animation |
| Pure Theo core | Programs, particles, channels, reactions, character habits | Device access or task authority |
| GPUI presentation | Body, composer, live page/viewer, annotations, keyboard/accessibility behavior | Implicit cancellation when a view closes |

The same operation should behave consistently whether initiated from Theo's controls, a native Theorem API, a Claude SDK tool, or external MCP. These are different entry points to the same capability owner.

Preserve the existing distinction between observing a run and cancelling it. Hiding Theo, collapsing a screen, or detaching a terminal view must not silently terminate the underlying work. Stop is an explicit command with an outcome. [Desktop driver][P9]

## 5. Activity and action contracts

Maintain separate state for presence, activity lifecycle, control ownership, presentation, and platform readiness.

A reconstructible activity projection should include:

| Field | Purpose |
| --- | --- |
| conversation_id, session_id, agent_id | Preserve the intended agent relationship |
| task_id, activity_id, parent_activity_id | Attribute work and restore parent activities |
| revision, generation | Reject stale callbacks and update ordering |
| environment_id, surface_id | Identify where work occurs |
| phase | Starting, active, awaiting user, stopping, completed, failed, or unknown |
| preferred_form | Presentation preference, subject to host arbitration |
| target_ref, observation_id | Identify what the activity refers to |
| signals | Measurements with units, timestamps, source and freshness |
| available_actions | Current user controls |
| outcome_ref | Verified result or explicit uncertainty |

Connections begin with a snapshot and continue with ordered updates. Recreating a view recovers its current appearance from that snapshot. Completion events drive expressive timing, but an animation is never a durable work receipt.

For example: music is playing; capture temporarily selects the camera; the capture receipt arrives; the record player returns because playback is still active. A delayed stop for an older track cannot stop a later one.

Face::ending deliberately treats unknown or aged-out invocation history as completed and keeps only a small animation history. Do not use it as proof that an agent task completed. [Player history][P10]

Foreground pointer/keyboard actions require an owner for the selected environment. Background API work can proceed independently when it does not compete for that input resource. The action carries a cancellation generation, target identity, observation revision, and action ID. Dispatch and ownership checks must cover the actual write, not only enqueue time.

On takeover or stop, immediately stop further dispatch and show that local state. An action already sent can complete, fail, or have an uncertain outcome. Re-observe before repeating it. Distinguish physical user input from the application's own injected events so the broker does not interrupt itself.

## 6. Theo's expanding screen

### Interaction and morph

The user invokes Theo, pastes a URL, and opens it. The white eye shapes broaden into the content backing; the dark outer body stretches into a thin bezel. The resulting screen is a real, resizable working surface with restrained controls: origin/address, back, loading state, close/collapse, and an optional pin.

Use the same material and motion language as the other device forms. Avoid making the character disappear and replacing it with an unrelated floating browser card.

Keep the live content viewport stable during the most extreme geometry changes. Animate the shell and a preview texture during the transition, then reveal the interactive surface at the settled layout. Resizing during use should update the underlying viewport normally. This is a proposed implementation technique, to verify against GPUI's clipping and surface behavior.

A persistent surface session retains URL, navigation history, scroll, media state, and its selected browser profile. Expansion does not recreate the browser. Collapse does not reload the page. Close, hide, and stop-media are distinct operations.

When music/video continues after collapse, indicate playback and retain a stop/pause affordance; a record-player/boombox arrangement can express that state. Do not silently discard browser work when another agent briefly requests a camera form.

### First renderer: reuse Platinum's Page and Engine

Platinum's Page already uses a hand-built Servo/GPUI embedder. The source explicitly avoids a native child view so GPUI chrome can draw over the page; Render presents SceneRenderingContext frames through GPUI surface(). That is a directly relevant implementation boundary for Theo's screen. [Page][P11] [Engine][P12]

Create a surface host around that existing page/session ownership. Validate page lifetime, input routing, resizing, clipping, focus and movement between presentations. Do not assume reparenting a live Page is free or already supported.

The YouTube plan contains later native watch-page probes and user confirmation of video playback, as well as earlier failures. Read it chronologically. It establishes a credible starting point; it does not establish general Chrome parity, every login/account flow, DRM, live streams, or all long-session behavior. [YouTube evidence][P13]

The first browser acceptance target should include a representative YouTube video with audible synchronized audio, pause/seek, keyboard focus, resize, collapse/reopen, and retained playback position. Report actual resolution, device, duration, dropped-frame or stall observations, and audio behavior. Keep signed-in playback, live content, and protected-content compatibility as explicit additional checks.

## 7. Local browser, private computer, and cloud browser

An **environment** is the computer or execution context. A **surface** is the application window, browser page, or display being observed or shown. A local Platinum page can share the Mac environment while exposing its own precise page API.

| User-facing choice | Execution | What Theo presents |
| --- | --- | --- |
| This Mac | Existing applications on the user's desktop | Context, annotations and controls over the real application |
| Theorem browser | Existing local Platinum browser engine | A Page rendered inside Theo |
| Private desktop | A chosen local VM or isolated desktop provider | A remote-display/viewer surface |
| Cloud browser or desktop | Existing server browser or remote computer | A streamed interactive surface |

Adopt the existing server browser if it offers the capabilities this experience needs. An automation endpoint that can navigate and capture screenshots is not automatically a display-and-audio endpoint.

Keep two separate delivery paths:

- **Control and context:** commands, typed events, tool results, task status, bounded semantic observations and selected model-visible images.
- **Display and sound:** local browser frames, or a remote video/audio stream with input, viewport sizing and reconnect behavior.

Do not feed every video frame to the model. The user can watch a fluid stream while the agent receives observations appropriate to its task.

Cloud display quality depends on capture, codecs, transport, decoding, audio/video synchronization and input latency. A screenshot loop is not the acceptance path for YouTube. Selecting a remote browser can improve browser compatibility, but does not by itself solve media quality or session continuity.

A cloud action targets the cloud environment and surface, not the host window that happens to display it. Keep environment IDs and coordinate transforms explicit. Switching the visible viewer never silently changes where a pending action will execute.

## 8. Herdr and agent API integration

### Preserve existing live sessions

Theo should attach through the existing Theorem terminal.* host boundary. The host already owns the Herdr connection and terminal binding; bypassing it with another unmanaged viewer would bypass its control conventions. The relevant work is still in open PRs and has deployment/reconnect gaps. Coordinate against those exact branches. [Herdr PR][H1] [Terminal controls][H2] [UI profiles][H3]

Herdr preserves terminals, input/output, and explicit program status. It is useful transport and lifecycle infrastructure. Its output stream is not the complete provider conversation with tool records and image attachments.

Maintain two correlated streams:

| Stream | Carries |
| --- | --- |
| Terminal | Exact running terminal identity, bytes, user input, status, attachment/control, reconnect |
| Semantic agent | Conversation messages, tool calls/results, images, turn identity, steering and cancellation acknowledgements |

A provider adapter or a structured channel supplies the semantic stream. Reconstructing full agent context from ANSI output is a limited fallback.

Herdr exposes terminal.put to stage bytes on the terminal host; Theorem's inspected client does not yet wrap it. Even after adding transfer, a staged screenshot is not proof that the selected model turn accepted an image. Capture, transfer, provider submission, and acceptance are separate states. [Herdr client][H4] [Herdr API][H5]

### Existing adapters are a starting point, not live attachment

Theorem's current Claude adapter launches a new print-mode, nonpersistent text request. Its Codex adapter launches an app-server, starts an ephemeral thread and sends a text turn. The latter already supports turn interruption. Neither inspected request path provides attachment to a running Herdr session or image input. [Claude adapter][H6] [Codex adapter][H7]

The Claude Agent SDK offers interactive streaming input, image blocks, tools and session resumption. Resumption restores accessible session history; it is not automatic attachment to an independently running Claude Code process. [Claude streaming][D1] [Claude sessions][D2]

Codex app-server offers structured thread/turn operations, images, steering with expectedTurnId, and interruption. Integrate with the runtime that owns the selected session, or explicitly resume under coordinated ownership. Starting a new thread is a different operation. [Codex protocol][D3]

### Keep agent creation distinct from attachment

The interface should clearly support:

- **Attach:** continue the user's existing Claude Code, Codex, or Theorem work.
- **Start:** create an owned run through Claude Agent SDK, OpenAI Agents/Responses APIs, or another provider.
- **Connect tools:** add search, browser, application, or MCP capabilities to a run.

Credentials and provider-specific transports belong to adapters. GPUI and the pure visual core should not depend on whether an adapter is implemented in Rust, TypeScript, Python, or a managed sidecar.

A new API run does not become the user's current ChatGPT conversation. Connecting that conversation to device capabilities is the MCP path.

### Reuse ownership conventions carefully

Theorem #1042 supplies controller/observer roles, attachment identity, generation, binding epoch, duplicate input suppression and detach-without-kill behavior. Input does not acquire ownership on its own. The private Lease implementation is currently terminal-specific, not an already reusable generic crate. [Terminal host][H2]

Adapt or extract these conventions for a separate foreground-input resource per environment. Add desktop revocation, interruption and current observation identity. Do not inherit legacy unleased writes, optional epochs, terminal replies, or terminal-grid resize policy as desktop behavior.

## 9. Native API and MCP exposure

Expose an MCP server backed by the same capability service used by Theo itself. First-party native calls can use typed APIs; MCP is the interoperability projection.

A proposed small initial tool surface is:

| Proposed operation | Purpose |
| --- | --- |
| theo_environments | Discover connected devices/computers and available capabilities |
| theo_observe | Observe a selected application/window/surface; return semantic context and an image when requested |
| theo_act | Act on a current target within the selected environment and return delivery/verification state |
| theo_surface_open | Open or show a browser/remote surface with explicit environment identity |
| theo_session_attach | Attach the presence to an existing supported agent session |
| theo_present | Request a supported expression, caption, or form without granting device authority |
| theo_stop | Stop a specified task/activity or revoke its control generation |

These names are proposals, not existing endpoints.

Image results must reach the caller as actual usable image content or a retrievable artifact supported by that client. A path on the Mac is insufficient for a cloud caller. Every observation should retain environment, surface, capture time and observation identity through the result.

Pair a client to a device and an allowed scope. Subsequent authorized work should run through that established scope; do not introduce a separate confirmation dialog for every ordinary low-level step. A visual-form request never grants new capture or input access.

Local agents can connect through a suitable local MCP transport. For ChatGPT, current official guidance supports a remote HTTPS endpoint or Secure MCP Tunnel to reach a private/local server. ChatGPT does not directly connect to arbitrary localhost. The current plan distinction matters: the help page lists custom-server read/fetch access for Pro and full write/modify MCP for Business and Enterprise/Edu. Discover actual client capabilities instead of promising that every ChatGPT account can drive the desktop. [ChatGPT MCP][D4] [OpenAI MCP][D5]

No Theo device connection was created in this review.

## 10. A substantial computer-use implementation

### Candidate roles

| Project and inspected revision | What is relevant | Decision |
| --- | --- | --- |
| Cua, 3f3ccd48bd7db73d41132d7f45a92a96eca05100 | Native AX/UIA/AT-SPI and input/capture; a safe Rust SDK can create a runtime in process; separate VM/media components | Leading candidate for the native executor |
| agent-desktop, 54855b6225aec00da4ac65e2d0362fd41cd9fcb3 | Model-free Rust accessibility automation, snapshot-qualified targets, stale/ambiguous-target errors, in-process C ABI and MCP | Strong alternative; use its target/recovery discipline in the common contract |
| OpenMausBot, 1a4e4da9f380804ad98c7fd1b141bb0dbfafced4 | Full application integrating Cua Driver, remote computers and viewers | Study lifecycle, ownership, and target switching; retain Theorem's own harness and GPUI UI |

Cua Driver is directly relevant to a native Rust host: CuaDriver::create documents and implements a same-process runtime without launching a driver daemon. This is a different boundary from adopting Cua Spaces as the application. [Cua SDK][C1]

agent-desktop assigns actionable references to a specific snapshot, rejects unqualified references, and exposes stale/ambiguous outcomes. Its FFI runs in process. It is not a model or a requirement to launch a CLI for every step. [agent-desktop][C2] [FFI][C3]

OpenMausBot uses an app-owned Cua Driver for local execution. Its desktop viewer uses noVNC; its cloud/isolated-computer composition is useful design evidence. Its documented “Local VM” route includes containerized Linux desktops, so do not assume that label always means a hardware VM. [Integration][C4] [Viewer][C5] [Local desktop][C6]

### Selection gate

Compare Cua Driver and agent-desktop on the same real Mac tasks: Firefox, Finder, a text editor, a graphics application, a native dialog, and user interruption during a pending operation. Measure semantic coverage, target validity after changes, recovery, focus interference, latency and integration effort. Select one default executor based on that evidence; do not run competing native executors against the desktop by default.

### Required broker behavior

Theorem must own the full observe–resolve–act–verify loop:

1. Identify environment, application and window/surface.
2. Collect semantic information and selected visual evidence.
3. Bind actionable targets to the observation and process/window identity.
4. Prefer meaningful application/native operations when supported; use pixel input where necessary.
5. Dispatch under the current control owner and cancellation generation.
6. Observe the result and distinguish delivered input, verified result, refusal, and uncertain outcome.
7. On a stale target, acquire fresh evidence; on uncertain delivery, reconcile before retry.

A target reference needs its resolver, observation identity, scope, supported actions and bounds with a coordinate space. Centralize transforms among pixels, screen points, window coordinates and embedded content coordinates. A decorative highlight and the actuator must refer to the same target, although decorative spring movement must never move the actual click coordinate.

Capture the intended application/window before activating Theo's composer. Keep that invoking scope until explicitly changed. Exclude decorative overlays from ordinary desktop capture. When the target is a browser or remote surface inside Theo, capture that underlying surface directly so “exclude Theo” does not accidentally hide the requested content.

The old pet verification and receipts remain useful. The helper's limited actions, process-global cancellation and focus behavior are not the final architecture. [Legacy pet/helper][L1]

### Media and component licenses

Cua's current media path describes H.264 video and Opus audio over rcdp v2 using WebSocket or QUIC. That is a separate subsystem from its desktop driver. [Cua media][C7]

The source marks Cua Driver, Lume and the general SDK as MIT, while Spaces and substantial streaming/client/codec/viewer components are FSL-1.1-MIT. Some summary documentation is inconsistent about the wire layer; inspect component manifests/licenses before selecting a streaming dependency. agent-desktop is Apache-2.0. OpenMausBot's root application is Apache-2.0 with separate enterprise and bundled-dependency terms. These are source license labels, not a legal compatibility conclusion. [Cua licensing][C8] [agent-desktop license][C9] [OpenMausBot licensing][C10]

## 11. Native windows, focus, and summoning

Use a small interactive body window and passive annotation windows. Expansion creates or reveals a focusable browser/composer surface. The body must not require a full-desktop input-catching window. Transparent pixels are not equivalent to mouse pass-through.

Record the invoking application and window before activating any composer. Passive appearance and progress changes should not steal focus. Deliberately opening a composer or browser accepts keyboard focus; dismissal returns to the intended prior context.

The old overlay is a GPUI popup whose backing NSWindow is mutated and set to ignore mouse events; it is not evidence of a completed nonactivating-panel design. Verify the exact vendored backend on Spaces, fullscreen, Stage Manager and multiple displays. [Legacy overlay][L2] [AppKit panels][A2] [Mouse events][A3]

Build a configurable global binding layer with keyboard, menu-bar, pointer and gesture entry points. Support the requested double-right-click binding, but make its context-menu policy explicit:

- If the first right click reaches the application immediately, its normal menu can open before the second click summons Theo.
- Delaying/filtering the first click can recognize the gesture cleanly, but changes ordinary right-click timing.
- A modifier-plus-right-click binding avoids that particular ambiguity and can be offered alongside it.

A passive event monitor cannot retroactively swallow the first context-menu click. Active event taps have different filtering capabilities. Test timing against the operating system's configured double-click interval and real applications. Ignore injected agent input for summon recognition. [Apple event taps][A4]

The summon command opens the same selected agent relationship wherever invoked. It must not create a new conversation accidentally.

## 12. Programmability and device forms

Publish four extension boundaries, allowing an integration to implement one or several:

| Extension type | Example |
| --- | --- |
| Agent adapter | Claude Agent SDK, Codex app-server, another agent backend |
| Capability adapter | Search API, music-generation service, photo editor operations |
| Environment/surface adapter | Existing server browser, a VM provider, another application's document surface |
| Presentation package | Record player, camera, sequencer, screen arrangement |

A presentation package declares a namespaced ID and version, compatible feature/program format, required signals and units, entry/active/exit behavior, still representation, accessible label, supported controls and fallback. The host validates it.

The host owns native windows, rendering and activity arbitration. The specialist supplies capability state and preferred form; it does not need to implement another overlay system.

Use measurements with meaningful scope. The initial audio meter measures sound Theorem is playing. Another application's audio requires a separate supported adapter or capture path. The camera can represent a photo specialist without implying active screen capture; an explicit capture indication and receipt distinguish the operation.

Keep character habits separate from life-agent knowledge. Theo's local memory informs expression and reactions. Current attention, session evidence, and durable user memory belong to distinct runtime stores. A Keep interaction is acknowledged after persistence, not after an animation.

Accessible names, keyboard equivalents, readable status and reduced-motion presentation belong to the contract. Reduced motion changes interpolation; it never suspends expiration, cleanup, cancellation, or ownership release.

## 13. Implementation sequence and completion gates

### A. Finish the port boundary

Resolve the retained-memory policy and public scoped cancellation. Preserve fixture parity, add narrow tests for those actual risks and the native boombox profile, and validate the crate in the locked workspace. Add a reproducible crate test job if dependency/bootstrap constraints permit; a mergeable draft and secret scan do not compile Rust.

**Gate:** a verifiable visual-core baseline and supported host API.

### B. Native presence and one real agent

Wire crates/theo into frame, provide a GPUI renderer/Stage adapter and the presence coordinator, consume real Expression parts, and implement show/hide/composer/stop. Add keyboard and configurable pointer invocation. Preserve invoking scope and conversation identity.

**Gate:** invoke over Firefox and an editor, converse with the selected agent, dismiss and remount without losing its state or stealing unrelated focus.

### C. Observation and computer use

Run the bounded executor comparison, select the native driver, then connect the broker, current-target resolver, captures, passive annotations, receipts and interruption.

**Gate:** complete a small real task in an existing application; interrupt while input is pending; resume from fresh observation without duplicate effects. No browser extension is installed.

### D. Agent continuity, API and MCP

Attach through the Herdr branches, add semantic/image delivery to the selected live sessions, and expose the native capability service through MCP. Add explicitly new SDK/API sessions separately.

**Gate:** the same existing Claude Code or Codex session demonstrably receives a chosen screen image, responds to it, and can request a supported tool action; an external MCP client gets usable image context. Report client-specific write limitations.

### E. Expandable local browser

Host the existing Page in Theo's screen form. Preserve the surface session through expansion/collapse and form changes. Complete navigation, resizing, focus, media control and measured audio plumbing.

**Gate:** useful YouTube playback with sound and continuity through resize/collapse/reopen. The test record identifies observed limits.

### F. Remote computers and programmable integration

Attach the existing server browser or chosen VM/cloud provider through the same environment/surface contract. Add a genuine display/audio adapter. Prove the extension contract with a record-player/music or photo-editing integration.

**Gate:** switching surfaces never misroutes input; a new capability and form can be added without editing domain-specific GPUI branches.

Browser-surface and agent-adapter work can proceed in parallel once the ownership/surface contracts are fixed. A VM is optional and must not delay local desktop usefulness.

**After native completion:** consider a browser extension for richer document/tab semantics. It remains outside all gates above.

## 14. Verification record required from the implementation

| Area | Meaningful evidence |
| --- | --- |
| Native focus | Invoke while typing in two applications; verify intentional composer focus and return |
| Gesture behavior | Ordinary right click, selected double-right-click policy, injected-event exclusion |
| Context identity | Capture the pre-invocation application, including after Theo takes keyboard focus |
| Semantic targets | Move/resize a window, open a dialog, change zoom, switch displays; stale refs refuse or refresh |
| Action ownership | Competing agent requests serialize; stale generations cannot dispatch |
| Interruption | Human takeover stops future input; uncertain in-flight work is reconciled |
| Agent continuity | Existing live session receives actual image input, not only a filename or terminal echo |
| Form lifecycle | Audio plus capture, remount, stale stop, still mode and missing-signal fallbacks |
| Browser session | Navigation, history, profile, scroll and media survive the defined hide/collapse operations |
| Media | Audible synchronized playback, resizing, pause/seek, representative duration and device |
| Remote target | Viewer coordinates and execution environment remain distinct under resize/reconnect |
| Accessibility | Names, keyboard operation, focus order/return, reduced motion |
| Resource use | Idle wakeups, renderer cost, capture latency, stream cost and bounded retained state |

Correlate session, environment, surface, activity, observation, action and outcome IDs. Measure rendering, observation, model response and cancellation separately. Do not claim native usability from a successful pure Rust test.

## Source register

### Platinum and Theo

- [P1: PR #22][P1]
- [P2: Root manifest][P2]
- [P3: Trace tests][P3]
- [P4: Existing Expression consumer][P4]
- [P5: Face engine and form/stop API][P5]
- [P6: Generated built-ins][P6]
- [P7: Speech wrapper][P7]
- [P8: Memory implementation][P8]
- [P9: Desktop run driver][P9]
- [P10: Player history and binding lifecycle][P10]
- [P11: Page rendering][P11]
- [P12: Browser engine][P12]
- [P13: YouTube evidence chronology][P13]

### Herdr and provider integration

- [H1: Herdr backend PR][H1]
- [H2: Terminal control host][H2]
- [H3: UI Herdr profiles][H3]
- [H4: Theorem Herdr client][H4]
- [H5: Herdr API types][H5]
- [H6: Current Claude provider][H6]
- [H7: Current Codex provider][H7]
- [D1: Claude streaming input][D1]
- [D2: Claude sessions][D2]
- [D3: Codex app-server][D3]
- [D4: ChatGPT MCP connection and availability][D4]
- [D5: OpenAI MCP and tunnel support][D5]

### Computer-use candidates

- [C1: Cua same-process Rust SDK][C1]
- [C2: agent-desktop overview][C2]
- [C3: agent-desktop FFI][C3]
- [C4: OpenMausBot integration][C4]
- [C5: OpenMausBot viewer][C5]
- [C6: OpenMausBot local desktop][C6]
- [C7: Cua media protocol][C7]
- [C8: Cua component licensing][C8]
- [C9: agent-desktop license][C9]
- [C10: OpenMausBot licensing][C10]

### Native and legacy boundaries

- [A1: Speech buffer API][A1]
- [A2: Nonactivating panel][A2]
- [A3: Mouse-event behavior][A3]
- [A4: Event-tap filtering][A4]
- [L1: Swift AX helper][L1]
- [L2: Legacy companion overlay][L2]
- [L3: Legacy companion PR][L3]
- [L4: Legacy vision follow-up][L4]

The earlier supplied co-browse, harness-policy and procedural-engine handoffs remain design context. Their older host placement or claimed completion does not establish current native functionality. No browser-extension implementation is required by this revision.

[P1]: https://github.com/Travis-Gilbert/Platinum/pull/22
[P2]: https://github.com/Travis-Gilbert/Platinum/blob/7950be398bf99d53c67171d997a0afd89ec44eb5/Cargo.toml
[P3]: https://github.com/Travis-Gilbert/Platinum/blob/7950be398bf99d53c67171d997a0afd89ec44eb5/crates/theo/tests/trace.rs
[P4]: https://github.com/Travis-Gilbert/Platinum/blob/46bbc63e7c4b391e4d2107a77097f1e5054a9831/src/theorem/transcript.rs
[P5]: https://github.com/Travis-Gilbert/Platinum/blob/7950be398bf99d53c67171d997a0afd89ec44eb5/crates/theo/src/engine.rs
[P6]: https://github.com/Travis-Gilbert/Platinum/blob/7950be398bf99d53c67171d997a0afd89ec44eb5/crates/theo/data/builtin.json
[P7]: https://github.com/Travis-Gilbert/Platinum/blob/46bbc63e7c4b391e4d2107a77097f1e5054a9831/src/spoken.rs
[P8]: https://github.com/Travis-Gilbert/Platinum/blob/7950be398bf99d53c67171d997a0afd89ec44eb5/crates/theo/src/memory.rs
[P9]: https://github.com/Travis-Gilbert/Platinum/blob/46bbc63e7c4b391e4d2107a77097f1e5054a9831/src/theorem/driver.rs
[P10]: https://github.com/Travis-Gilbert/Platinum/blob/7950be398bf99d53c67171d997a0afd89ec44eb5/crates/theo/src/player.rs
[P11]: https://github.com/Travis-Gilbert/Platinum/blob/46bbc63e7c4b391e4d2107a77097f1e5054a9831/src/page.rs
[P12]: https://github.com/Travis-Gilbert/Platinum/blob/46bbc63e7c4b391e4d2107a77097f1e5054a9831/src/engine.rs
[P13]: https://github.com/Travis-Gilbert/Platinum/blob/46bbc63e7c4b391e4d2107a77097f1e5054a9831/docs/plans/youtube.md
[H1]: https://github.com/Travis-Gilbert/Theorem/pull/1021
[H2]: https://github.com/Travis-Gilbert/Theorem/blob/3440ade7d4e46d3c1733153f89ffbf3f2a0909f7/apps/theorem-ide-host/src/terminal.rs
[H3]: https://github.com/Travis-Gilbert/Theorem-UI/pull/73
[H4]: https://github.com/Travis-Gilbert/Theorem/blob/88cf1eb0c1eb1b662ba2b951adf7e9a29e4de4e0/apps/theorem-ide-host/src/herdr/client.rs
[H5]: https://github.com/herdrdev/herdr-core/blob/f0828f1e40fac656a5bac077e20c605d38760164/crates/herdr-core/src/api/types.rs
[H6]: https://github.com/Travis-Gilbert/Theorem/blob/3ed7ff18fbfd92b6a7ea13e57b2ee05541cdb871/rustyredcore_THG/crates/theorem-receiver/src/providers/claude_cli.rs
[H7]: https://github.com/Travis-Gilbert/Theorem/blob/3ed7ff18fbfd92b6a7ea13e57b2ee05541cdb871/rustyredcore_THG/crates/theorem-receiver/src/providers/codex_app_server.rs
[D1]: https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode
[D2]: https://code.claude.com/docs/en/agent-sdk/sessions
[D3]: https://developers.openai.com/codex/app-server
[D4]: https://help.openai.com/en/articles/12584461-connecting-custom-mcp-servers-in-chatgpt
[D5]: https://developers.openai.com/api/docs/guides/tools-connectors-mcp
[C1]: https://github.com/trycua/cua/blob/3f3ccd48bd7db73d41132d7f45a92a96eca05100/libs/cua-driver/rust/crates/cua-driver-sdk/src/lib.rs
[C2]: https://github.com/vercel-labs/agent-desktop/blob/54855b6225aec00da4ac65e2d0362fd41cd9fcb3/README.md
[C3]: https://github.com/vercel-labs/agent-desktop/blob/54855b6225aec00da4ac65e2d0362fd41cd9fcb3/crates/ffi/README.md
[C4]: https://github.com/milind-soni/OpenMausBot/blob/1a4e4da9f380804ad98c7fd1b141bb0dbfafced4/docs/computer-use-integration.md
[C5]: https://github.com/milind-soni/OpenMausBot/blob/1a4e4da9f380804ad98c7fd1b141bb0dbfafced4/src/components/DesktopViewer.tsx
[C6]: https://github.com/milind-soni/OpenMausBot/blob/1a4e4da9f380804ad98c7fd1b141bb0dbfafced4/apps/docs/content/docs/computers/local-vm.mdx
[C7]: https://github.com/trycua/cua/blob/3f3ccd48bd7db73d41132d7f45a92a96eca05100/libs/cua/proto/MEDIA.md
[C8]: https://github.com/trycua/cua/blob/3f3ccd48bd7db73d41132d7f45a92a96eca05100/legal/LICENSING.md
[C9]: https://github.com/vercel-labs/agent-desktop/blob/54855b6225aec00da4ac65e2d0362fd41cd9fcb3/LICENSE
[C10]: https://github.com/milind-soni/OpenMausBot/blob/1a4e4da9f380804ad98c7fd1b141bb0dbfafced4/LICENSING.md
[A1]: https://developer.apple.com/documentation/avfaudio/avspeechsynthesizer/write(_:tobuffercallback:)
[A2]: https://developer.apple.com/documentation/appkit/nswindow/stylemask-swift.struct/nonactivatingpanel
[A3]: https://developer.apple.com/documentation/appkit/nswindow/ignoresmouseevents
[A4]: https://developer.apple.com/documentation/coregraphics/cgeventtapoptions
[L1]: https://github.com/Travis-Gilbert/Theorem/blob/3ed7ff18fbfd92b6a7ea13e57b2ee05541cdb871/apps/theorem-voice-helper/Sources/TheoremVoiceHelper/AxActService.swift
[L2]: https://github.com/Travis-Gilbert/Theorem/blob/a9eab6bfbfacd438207c83f50c15aadae3a3e48c/apps/theoremweb/crates/companion-macos/src/macos/overlay.rs
[L3]: https://github.com/Travis-Gilbert/Theorem/pull/864
[L4]: https://github.com/Travis-Gilbert/Theorem/pull/871

