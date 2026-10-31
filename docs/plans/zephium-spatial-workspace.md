# Zephium spatial workspace

Requested 2026-10-30 on `claude/platinum-theorem-interface-990ec3`.
Implementation began at `2e5b735`; Claude's concurrent bump repair at
`a755b4e3adf32adcf328e2da7d8c532b034cdb57` is preserved. The attached
Park/Zephium specification is the scope. The user approved either parakeet-rs
or mlx-audio-swift, excluding whisper.cpp, and explicitly approved per-role
model overrides in Theorem.

## Delivered behavior

- Persistent browser-owned multipage board with stable tab identities, saved
  placement/camera, sleeping tabs, focused/framed/overview presentations,
  interruptible spring camera motion and reduced-motion handling. Visual
  scaling retains the page layout viewport. Opener lines remain distinct from
  organizational parents. Card menus, connections, groups and group dragging
  reuse the whiteboard engine's existing operations.
- Individual SQLite visits retain URL, title, navigation time, per-tab
  predecessor and owned PNG previews. The native Trail view pages through
  history and reopens visits. Repeated URLs remain separate visits. Capture
  copies pixels while locked and compresses off the UI thread; it never retains
  recyclable Servo surfaces. Preview slots, source pixels and encoded images are bounded.
- One Vault-authoritative note and one editor entity are shared by the sidebar,
  canvas card and expanded Notes place. Selection survives promotion. Debounced
  saves reject stale generations, preserve disk conflicts and observe Undo.
  Shutdown flushes off the UI thread. Overview displays a compact preview.
- Claude's existing bump outline and growing composer remain the geometry owner.
  The board composer stays anchored while the canvas moves. Selected page
  references and note text use the existing attachment/admission path and show
  the selected context. Page references are identified as links, not HTML captures.
  Existing starter workflows and direct canvas/page menus are reused.
- Model settings persist Main, Browser, Coding, Quick tasks, Review and Typed
  decisions assignments. The selected workflow and assignments reach the actual
  Theorem HTTP runtime, catalog resolution and native provider configuration.
  Automatic keeps the old wire request when no override is present. Explicit
  assignments refuse unknown models unless binding fallback was enabled. Run
  receipts retain requested/bound identities, source and fallback reason.
- Theorem's organization validator now permits deliberate specialist bindings
  to compatible catalog models; default specialists still share the pool.
  This is the user's explicit amendment to the previous front-role-only policy.
- The authenticated `typed_decision` graph tool uses the real Jev contract,
  fixed TypeSafe endpoint and exact `jev-1.13.0` model, or an explicitly selected
  compatible chat provider. Graph Lisp can discover/call it through the existing
  tool router. Credentials use the authority's environment/Keychain owner;
  settings select the credential source, not a plaintext key or endpoint.
  The tool requires model invocation and graph read/write authority plus an
  existing observation. Atomic pending receipts prevent automatic repeats of
  paid calls with unknown outcomes. Requests, observation version, attempts,
  successful model, fallback reason, validated answers and usage are retained.
  Replayed answers are revalidated. Answers grant no execution permission.
- Local dictation uses parakeet-rs 0.4.3 and its pinned EOU model. GPUI Kit owns
  microphone capture, resampling and speech lifecycle; ONNX Runtime is shared
  with embeddings. Audio/results are bounded, cancellation is explicit, recording
  has a five-minute ceiling, and a final-result timeout never commits partials.
  Native fields use their mounted IME handler. Servo input, textarea and simple
  rich contenteditable destinations use captured document/element/focus/text/
  selection tokens and the existing native composition owner. Focus or selection
  changes refuse stale completion. Dictation never automatically submits.
- Spoken output has separate installed macOS voice/rate preferences,
  read-selection, pause/resume/stop. Kokoro was a TTS reference in the supplied
  discussion; this implementation reuses native macOS speech instead.

## Ownership and dependency choices

Browser owns tabs; TabTree owns organization and opener provenance; SpatialCanvas
owns presentation and transient gestures; the session store owns atomic board
persistence. Vault owns notes. ProfileStore owns visits. Theorem owns model
catalogs, credentials, execution authority and decision receipts. There is no
second tab, note, provider-catalog or execution-policy store in the UI.

Parakeet fits the existing Rust/ONNX stack: exact ONNX Runtime rc.15 and
Tokenizers 0.23 are already present. MLX Audio Swift would require another
language/runtime bridge. No custom recognizer, resampler or downloader was added.
The EOU model is pinned at `altunenes/parakeet-rs` revision
`b53b9101fa87271041773b5036aeb8db9d810d40`.

Typed questions and observation binding reuse `zephium-decision` (MPL-2.0),
from Zephium `96257cd3f67dd639eb5decb85b135f2674444603`. A small protocol adapter
is necessary because Jev differs from chat; importing Zephium's entire agentic
transport would introduce another execution policy/runtime and reqwest version.
Choice questions use the library constructor to add its reserved abstention.

Reproducible companion patch files are included in `patches/` and applied by the
existing bootstrap scripts. Theorem remains pinned at
`08495b15879a7bc2449d43c49530f549268d05fc`; Servo remains pinned at
`efdf7370da024f467f7526798fe5c6e50e784b52`. See `patches/README.md` for provenance,
credential configuration and bootstrap behavior. Vendor GPUI patches document
the focused text-handler adapter and strict final-result behavior.

## Verification record

Native checks used Rust 1.97.1, `script/media-env`, jobs=4, incremental=0,
`TMPDIR=/Volumes/SSD Samsung/tmp`, and
`CARGO_TARGET_DIR=/Volumes/SSD Samsung/cargo-targets/frame`.

- `script/media-env cargo test --locked -p frame --bin frame -- --test-threads=1`:
  **243 passed, 2 ignored**. Native composition/final/Undo, focus refusal, strict
  speech timeout, board/group restore, note conflicts and saved-role transport
  are covered. The ignored audio oracle was separately exercised below.
- `script/media-env cargo test --locked -p frame-profile --test visits`:
  **2 passed**; repeated URLs, predecessors/profile isolation, timestamps,
  pagination and invalid/oversized input rejection.
- The ignored Parakeet oracle explicitly run with a generated 16 kHz mono WAV:
  **passed**, recognizing “the river flows beside the mountain this is a local
  voice recognition test”. No user microphone audio was recorded.
- `script/media-env cargo build --locked --features snapshot --bin frame
  --example canvas_check --example site_panel_check`: **passed**. Existing
  warnings remain; this is a development build, not release/signing acceptance.
- Native `FRAME_SPATIAL_CHECK=/tmp/platinum-spatial-20261030-9`, with the
  supplied local fixture and `FRAME_SPATIAL_PUBLIC_URL=https://example.com`:
  **passed**. Real public page, child opener, board survival, native sidebar
  note composition/commit, same note on the board, light/dark rendering and
  model settings. Real Servo textarea and rich editor partial/final insertion,
  selected bold text, one-shot final, stale context and changed-focus refusal.
  Input is a supplied transcript through production destinations, with ASR
  verified independently by the WAV oracle. AppKit focus was active on this run.
- A fresh process with that profile and `FRAME_SPATIAL_RESTORE=1`: **passed**,
  comparing exact saved BoardState and restored note text. Reports/screenshots:
  `evidence/zephium-spatial-2026-10-30/`.
- Theorem checks used Rust 1.96.1 and target
  `/Volumes/SSD Samsung/cargo-targets/platinum-model-runtime`.
  `cargo check --locked --manifest-path rustyredcore_THG/Cargo.toml
  -p rustyred-thg-server --tests`: **passed** including client/authority changes
  and role-routing test compilation. Server tests were not executed.
- Theorem `cargo test --locked --manifest-path rustyredcore_THG/Cargo.toml
  -p theorem-harness-core --lib organization::tests`: **13 passed**.
  Surface-contract model routing: **1 passed**. Jev adapter checks cover valid
  usage, mismatched observations, malformed answers, forbidden credential fields
  and reserved choice abstention: **3 passed**.
- Standalone theoremweb workspace checking encountered its existing lockfile /
  tree-sitter resolution mismatch. Its changed agent-runtime consumer compiled
  and was tested through Platinum's locked dependency graph instead.
- Source `git diff --check`: **passed**. When newly staged, the unified patch
  files contain expected single-space blank context lines; companion source
  whitespace and clean-base application were checked separately. The full frame formatting check encounters
  pre-existing unrelated formatting differences; no broad reformat was applied.

## Live acceptance limits

Live microphone capture and audible playback have not been verified. Rich
editables containing non-editable islands or nested editing hosts fail closed;
this does not claim compatibility with every custom web editor. Website Undo
remains unverified; insertion uses Servo's existing IME path and does not add a
separate browser undo engine. Native editor Undo is tested.

Live Jev calls have not run: `JEV_API_KEY` is absent in the test environment.
The companion authority has not been deployed. Hosted model routing and context
attachment admission therefore remain unverified against production. Receipts
identify the bound client/provider model, not an undisclosed provider-internal
model revision. Capture ownership is tested, but compositor-frame attribution
across rapid overlapping navigations has no engine epoch proof.

The next live acceptance uses a configured authority to run a role override and
an authenticated `typed_decision` graph call, inspects its durable receipt and
fallback, and admits selected workspace context. On the Mac, record microphone
input into the same native/web destinations and verify audible read-aloud.
These are verification gaps, not silently replaced implementations.

## Workspace preservation

Existing worktree reused:
`/Volumes/SSD Samsung/var-18/Platinum/platinum-theorem-interface-990ec3`.
Main checkout, shared dependency edits, Claude's bump and workbench records,
and the pre-existing `.theorem/` directory are preserved. The user subsequently
authorized committing and pushing these changes on Claude's branch and opening
a PR with implementation and validation details. No deployment is included.

Two isolated shared clones remain as this worktree's build dependencies:
`/Volumes/SSD Samsung/platinum-model-runtime-20261030` and
`/Volumes/SSD Samsung/platinum-voice-servo-20261030`. The managed worktree tool
refused the dependency source outside the chat workspace, so clones were used.
They retain only this task's companion changes, also preserved in repository
patch files. They are required while this worktree's dependency links point to them.


## PR #17 review repairs

All six supplied findings were valid. The repairs retain the same branch and
existing ownership boundaries:

1. Note saves carry the editor's base document/revision. An in-flight save's
   completion is never canceled by later typing. `diffy` three-way merges that
   typing onto the returned document before advancing the editor base. The
   store also rebases a shutdown flush that reaches it before UI completion.
   Overlapping edits fail closed and leave the editor and authority versions
   intact. The existing Vault still owns revision admission and disk conflicts.
2. Tab activation leaves a place through `leave_place`, restoring the compact
   board composer. A headless rendered regression checks the board composer
   returns and the full Theorem card is absent.
3. Sessions persist `next_tab_id`, including the closed-tab allocation watermark.
   Imported tabs honor it too. Legacy sessions without this field start a new
   visit namespace once: their historical visits remain available, but new tabs
   cannot accidentally attach to a closed legacy tab's trail.
4. Small visit records retain FIFO order independently of the four-image budget.
   Preview saturation or encoding failure omits only the optional image; URL,
   title, visit timestamp, predecessor and aggregate history still get written.
   The writer drains queued records on normal shutdown. A deliberately stalled
   consumer regression verifies all twelve arrivals with only four previews.
5. Typed decision failures distinguish proven pre-dispatch failure, unknown
   outcome and a received invalid/refused response. Only proven pre-dispatch
   failures permit the configured fallback. Send/read failures remain pending
   and are replayed without another provider call. Socket fixtures drop and
   truncate real HTTP replies to test that classification.
6. The primary attempt is in the initial pending receipt. A fallback attempt and
   its reason are persisted before calling the second provider; a failed write
   prevents dispatch. An interruption test inspects the saved receipt inside the
   provider callback and verifies replay does not call either provider again.

`diffy` 0.4.2 supplies the three-way text merge rather than a custom diff/merge
algorithm. Its installed Rust source/docs were inspected; Context7 returned a
Ruby project with the same name, so that unrelated documentation was not used.

### How the five spatial criteria are implemented

| Criterion | Implementation and user behavior |
| --- | --- |
| Keep page cards alive across tab changes | `Browser.canvas` retains one `SpatialCanvas`; `close_stale_canvas` synchronizes browser-owned tab/page references instead of replacing it. `BoardState` saves card IDs, geometry, camera, groups and connections. Inactive loaded pages use cached presentation; sleeping pages wake when opened. |
| Animate between browsing and board | `toggle_canvas`, `focus_current`, `overview` and `animate_to` move the camera using GPUI springs. A card opens framed at 0.75 zoom; Focus page animates to 1.0 before returning to ordinary browsing. Camera scale is distinct from the page's layout viewport, and reduced motion bypasses movement. |
| Show opener provenance | Each `BoardPage` receives `TabTree.opener`. The canvas places a newly opened child beside that source and draws the connector. Reparenting the tab changes organization while preserving the original opener. |
| Stable previews and expandable history | The visit writer copies owned pixels from the live surface, encodes bounded PNG thumbnails off the UI thread and writes individual SQLite visits. Visit trail opens a separate paged history view (50 entries per page), with timestamps, previews and reopening; repeated URLs stay separate visits. Optional previews may be absent, and rapid-navigation frame attribution remains the previously documented limitation. |
| Sidebar note becomes the same board document | `NotesEvent::Promote` passes the existing `Entity<Note>` to `SpatialCanvas::add_note`. Sidebar, card and expanded Notes place share its Vault document identity and editor entity. Promotion saves a path/placement reference, not another document; overview shows a compact preview. |

Review verification results are recorded in the accompanying evidence check summary.
