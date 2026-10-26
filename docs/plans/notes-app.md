# Notes in Frame: Obsidian parity on GPUI and Servo

## Target and acceptance

The requested outcome is a native notes application on the existing chrome of
Frame, which the request calls Platinum, at feature parity with Obsidian. It
reads Markdown files from the user's device or from a remote RustyRed
database, edits them in a GPUI surface, browses them in a GPUI file explorer,
renders them in a Servo panel that reads like a website (a dock panel, not a
browser window), and shows how notes connect in a graph in a second Servo
panel. Notes are Loro-backed documents under the existing document authority.
The remote side goes through Theorem's Files API, whose server exists only on
an unpushed Theorem branch, so remote vaults come last (step 12). The request
names Cosmograph for the graph and calls it open source, but Cosmograph is
CC-BY-NC-4.0, which rules out commercial use without a business license, so
the graph uses cosmos.gl, the MIT graph engine in the same GitHub organisation
(cosmograph-org/cosmos), which the requester chose on 2026-10-26.

The parity target is Obsidian 1.14.5, the current public desktop release
(2026-10-22): Obsidian Flavored Markdown, every core plugin, the workspace,
the graph, JSON Canvas 1.0 and Bases, plus the community-plugin features in
the reference screenshots (Dataview tables, a calendar, a homepage, coloured
folders). Obsidian's community-plugin API is not part of the target; [Not in
this plan](#not-in-this-plan) gives the reasons, and the matrix says which of
the ten most-downloaded plugins are covered anyway.

Do not claim parity from compiling, from a demo vault, or from one of Frame's
renderers agreeing with another. Compare with Obsidian 1.14.5 on the same
machine and the same vaults:

- **Vaults.** A committed fixture vault that exercises every row of the parity
  matrix, the MIT-licensed JSON Canvas samples, and a generated vault of
  10,000 notes. Obsidian's help vault (obsidianmd/obsidian-help) is a good
  real-world vault, but GitHub reports no license for it, so use it only from
  an uncommitted local clone.
- **Reference data.** Obsidian's CLI (1.12 and later; it drives the running
  app) reports `links`, `backlinks`, `unresolved`, `orphans`, `deadends`,
  `outline`, `aliases`, `properties`, `tags`, `tasks`, `search`,
  `search:context`, `base:query` and `wordcount`, and two developer commands
  reach the rest: `eval` runs JavaScript in the app and returns the result,
  which reads the metadata cache, and `dev:dom` queries the app's live DOM,
  which includes the rendered reading view. Frame must match the link,
  backlink, unresolved-link, tag, alias, property, heading, task and
  word-count results exactly, including notes with the same name in different
  folders. One answer Obsidian does not fix: an ambiguous link whose candidates
  are equally long paths, none under the linking note's folder, resolves in the
  order the vault loaded (1.14.5's `getLinkpathDest`), so either candidate
  matches; Frame breaks such ties the same way every time.
- **Reading view.** The normalised DOM structure (elements, the class names
  that themes and snippets rely on, and text) matches Obsidian's reading view
  for every fixture note. Compare screenshots side by side as well.
- **Graph.** The same nodes and edges as Obsidian's graph under the same
  filters.
- **Files.** Opening and saving a note, `.canvas` or `.base` file without
  changes leaves its bytes identical, in local and remote vaults, and edits
  keep fields Frame does not understand. Renames rewrite links exactly as
  Obsidian does.
- **No lost edits.** Kill the app during a save, change an open file from
  another program (an editor, `git checkout`, Obsidian itself) while Frame
  holds unsaved edits to it, and drop the remote connection mid-edit. No
  accepted keystroke may be lost or silently overwritten: edits on both sides
  survive, and edits to the same lines end in a diff for the user.
- **Performance.** Record vault open, index build, quick switcher and search
  latency, typing latency, graph frame rate and memory for both applications
  on the 10,000-note vault. Set thresholds from the measured Obsidian numbers
  rather than inventing them.
- **AI off.** Everything works with AI disabled (foundation gate G6).

## Verified baseline — 2026-10-25

Checked on 2026-10-25. Rechecked or added on 2026-10-26: PR #16's merge, the
Obsidian details, the package versions, Theorem's file routes and Files API
branch, the runtime's sessions, and the Canvas 2D graph run.
[sources.md](evidence/notes-app-2026-10-25/sources.md) lists where each of
these facts came from.

- Frame base `b7db174` on `feat/liquid-glass-chrome`; Frame is macOS only.
  GPUI is `gpui-pre` 0.3.9 (zed@93d7456), and GPUI Kit 0.8.0 is vendored at
  `vendor/gpui-component`. Browser tabs live in a `DockArea` named
  `frame-tabs` (`src/browser.rs:267`), and each Servo tab is a `TabPanel` that
  implements `Panel` (`src/browser/dock.rs:70`). Frame has no reader view or
  ad blocking. It keeps every HTTP or HTTPS page it loads for its search,
  through theorem-browser-substrate's default `Full` capture dial (a
  `ContentSnapshot` node and the page's links, `src/search.rs:234`), and has
  no setting for the dial yet; other schemes, `frame-notes:` among them, are
  never captured (`src/search.rs:204`).
- Servo is the fork in `deps/servo-mse` at `81d8e29`, built with `webgl`,
  `webcrypto`, `brotli-compression-stream` and `media-gstreamer`, without
  WebGPU. `Engine::start` (`src/engine.rs:51`) builds Servo without a protocol
  registry, but every hook a notes panel needs exists: `ProtocolHandler` with
  `load`, `is_fetchable` and `is_secure`
  (`components/net/protocols/mod.rs:38`), `ProtocolRegistry::register`
  (`:104`; `http`, `https`, `chrome` and `about` are reserved),
  `ServoBuilder::protocol_registry` (`components/servo/servo.rs:1526`) and
  `WebView::evaluate_javascript` (`components/servo/webview.rs:732`). Frame
  already intercepts navigations in `request_navigation`
  (`src/delegate.rs:202`), and it already draws Servo inside GPUI's own scene:
  `src/scene.rs` hands each frame to `gpui::surface()` as a `CVPixelBuffer`
  over an IOSurface, and `src/page.rs` routes GPUI's input into the page. A
  `WebView` can be resized (`webview.rs:455`), given a scale factor (`:475`),
  shown and hidden (`:490`, `:498`), zoomed (`set_page_zoom`, `:658`) and
  painted (`:721`), and hiding one throttles its pipelines
  (`components/constellation/constellation.rs:5409`).
  `WebViewDelegate::load_web_resource`
  (`components/servo/webview_delegate.rs:1070`) sees every HTTP and HTTPS load
  of a webview and can answer it, and `UserContentManager::add_script`
  (`components/servo/user_content_manager.rs:34`) adds scripts to pages. The
  fork has no print or PDF output.
- RustyRed comes from `deps/theorem-rustyweb-pw2`, the checkout of
  Travis-Gilbert/Theorem that Frame already uses for
  `theorem-browser-substrate` and `rustyred-web`. `script/bootstrap` pins it
  at `7ef4dfe` on `claude/rustyweb-synthesis-on-request` and stops at any
  other commit. The checkout here is at `6e57dbb`, two commits later. They add
  reranking, crawling and embedding to the browser search, keep the capture
  dial's `Full` default, and change none of the runtime, authority,
  document-session or VFS crates. Frame's lock has `rustyred-thg-core` 0.5.0,
  `rustyred-thg-find` 0.1.0 and `tantivy` 0.22.1 through rustyred-web, and
  `theorem-space` 0.1.0 through theorem-browser-substrate.
  `theorem-ide-runtime`, `theorem-ide-authority`, `rustyred-thg-vfs`, `loro`,
  `comrak` and `rquickjs` are not in it. It does lock `notify` 8.2.0,
  `markdown` 1.0.0 (markdown-rs, through GPUI Kit), `ropey` 2.0.0-beta.1,
  `jiff` 0.2.38, `winnow` 1.0.4 and `objc2-app-kit` 0.2.2 and 0.3.2.
- `theorem-ide-runtime` ("renderer-neutral IDE services, document sessions,
  and provider routing") defines `DocumentAuthority` (`src/authority.rs:177`)
  on `loro` =1.13.9. An open source document is a session of three parts: a
  `TextModel` (`rustyred-thg-text-model`, on `ropey` 1.6.1, counting Unicode
  scalar values, with UTF-16 conversions), a Loro record
  (`sessions/record.rs`, "Not a second undo owner") and its publication. Undo
  belongs to the document (`undo`, `sessions/source.rs:538`), and
  `import_remote` (`:483`) applies remote changes without recording local
  undo. `documents_reconcile` (`src/services/authority_ops.rs:992`) settles a
  disk change by keeping one side; keeping the disk commits a `Reconcile`
  revision and reopens the session through `open_at`
  (`sessions/source.rs:166`), which starts a new Loro record and an empty undo
  history. The runtime's default authority is in memory, and its README says
  "products must bind the durable RedCore authority" and that the native
  durable-authority binding remains pending. `theorem-ide-authority` is that
  durable implementation: "revision-guarded source documents, multi-document
  transactions, stable identity across rename". `RedCoreAuthority::open`
  (`src/lib.rs:123`) fsyncs each accepted revision before the commit returns,
  and the store keeps each document's head text inline beside the exact text
  each edit removed. The runtime's `write_file` (`src/workspace.rs:315`) is
  not atomic; it calls `std::fs::write` at `:321`. The runtime also links
  `theorem-doc-session` (`Cargo.toml:19`), which brings `pulldown-cmark` 0.12
  and `theorem-block-registry`; its written-document sessions
  (`sessions/written.rs`) use that crate's model, a `LoroTree` of blocks in
  which "the graph stays the sole source of truth".
- `rustyred-thg-vfs` is a "graph-persisted, snapshotting virtual filesystem
  and local history": typed events, `refresh` (`src/lib.rs:929`), snapshots,
  history, `write` (`:1163`) and `restore`. Its reconciliation counts watcher
  events "as hints, but never used as the completeness oracle". Its writes are
  not atomic either: on macOS, `write_mounted_file` (`:1342`) calls
  `write_relative_file` (`:1346`), which opens the target through `openat` on
  the pinned parent directory with `O_TRUNC` and `O_NOFOLLOW` (`:2025`) and
  writes in place; the `std::fs::write` at `:1356` is the branch for other
  platforms. It watches with `notify` 6 and `macos_kqueue` (`Cargo.toml:14`),
  which holds one descriptor per watched file. It depends on the `commonplace`
  crate ("CommonPlace consumer object model", not the retiring
  `commonplace-api`), which brings `theorem-harness-core`,
  `theorem-provenance`, `theorem-surface-contracts`,
  `theorem-oplog-graph-compat` and `rustyred-thg-reconstruct-fact`.
- Theorem-UI PR #16 vendors a Files API SDK 0.5.0: 19 paths under
  `/api/files/v1`, `schema=theorem-files/1`, and X-CSRF-Token, Idempotency-Key
  and X-Theorem-Client-Id on writes. Theorem main `8e151d5` ships SDK 0.4.0
  without `/api/files/v1`. The handler exists only on `sdk-source/files-api`,
  a local branch of the Theorem checkout that GitHub does not have (tip
  `a287925`, 2026-10-24, 11 commits ahead of main and 2 behind): the
  `theorem-files` crate (MIT), 19 routes in `rustyred-thg-server`
  (`src/theorem_files.rs`), a theoremweb relay, and a curl smoke with a
  recorded run (`docs/plans/files-api/smoke.sh` and `smoke-output.txt`). The
  recorded run used an in-memory store with authentication off; writes need a
  CSRF token, which the smoke takes from `/api/chat/v1/bootstrap`, and an
  `Idempotency-Key`. The branch's spec
  (`docs/plans/SPEC-THEOREM-FILES-API-1.0.md`) serves a note as the
  doc-store's Markdown projection of a Loro block document, and merges a
  `PUT /content` made against an earlier projection through Loro, answering
  `merged: true`; the recorded run shows one such merge, and a 412 with the
  current hash for an unknown base. Main's other file routes are no
  substitute: `/api/project-files/v1`
  (`rustyred-thg-server/src/theorem_project_files.rs`) is a read-only view of
  one checkout the server binds through `THEOREMWEB_PROJECT_ROOT`, with reads
  capped at 4 MiB, and `theorem-ide-host --http` serves all 188 operations the
  IDE runtime advertises, `documents.exportUpdates` and
  `documents.importRemote` among them, over per-workspace fsynced
  `RedCoreAuthority` stores, but its README says its grant also covers
  subprocesses, terminals and extensions and that it is not a sandbox for
  mutually untrusted tenants. The PR, merged on 2026-10-26, adds only a
  same-path proxy to `THEOREM_API_URL` (`src/routes/api/files/v1/$.ts`), so
  production has no handler behind those paths until that branch is pushed,
  merged and deployed, and Cloudflare's build of the merge commit failed.
  `commonplace-api` is retiring ("do not add capabilities here"), so it is not
  a route for remote vaults.
- SDK+ ([theorem-sdk-foundation.md](theorem-sdk-foundation.md)) has since
  folded the Files handler into `Travis-Gilbert/sdk-foundation-20261025`, a
  locked Theorem worktree whose changes are all uncommitted on top of main
  `8e151d5`, with its status file at `complete: false`. Its record, read on
  2026-10-26, lists 22 Files operations (vault, entries, content read and
  write, directories, moves, deletion, links, backlinks, graph, resolution,
  search, tags, properties, settings read and write, events, suggestions and
  decisions, promotion, OKF export and import), payloads owned by
  `theorem-surface-contracts::files_api`, and candidate clients at
  `0.6.0-alpha.1`: TypeScript from Fern 3.107.3, and a Rust crate
  `theorem-sdk` (`packages/theorem-sdk-rust`, Progenitor 0.15.1,
  `publish = false`), both generated from one OpenAPI file (schema digest
  `c2e6c591`). It reports all 22 operations run against the shipping process
  with MCP disabled, each client reading what the other wrote, and
  acknowledged content, event positions and idempotent write receipts
  surviving a SIGKILL. Nothing is pushed, published or deployed:
  `localhost:3000/api/files/v1/vault` answers an HTML 404, and S1's inventory
  gate still refuses 129 unresolved projections. The Rust crate pins
  `futures = "=0.3.32"` and `reqwest = "=0.13.4"`, while Frame's lock has
  0.3.34 and 0.13.5, so adding it as it stands holds Frame's whole graph at
  the older versions, or fails to resolve if anything needs the newer ones.
- cosmos.gl 3.5.1 (`@cosmos.gl/graph`) is MIT, renders through luma.gl 9.3.6
  on WebGL2, and is active (cosmograph-org/cosmos was pushed on 2026-10-23).
  Cosmograph 2.5.1 is CC-BY-NC-4.0, with a business license only on request
  ("reach out to us"). Its engine is its own `@cosmograph/cosmos` 2.5.1, also
  CC-BY-NC-4.0, on the same luma.gl (~9.3.6), and it adds DuckDB-WASM 1.32.0,
  Apache Arrow 17.0.0, the Mosaic 0.21 packages (`@uwdata/mosaic-core`,
  `mosaic-sql`, `mosaic-plot`, `vgplot`) and `@supabase/supabase-js`. It sends
  telemetry to Supabase and checks a license key with `rpc("ll")`, adding
  attribution when there is none. In Servo it would need everything cosmos.gl
  needs, plus WebAssembly and workers for DuckDB.
- cosmos.gl did not run in Frame's Servo on 2026-10-25, the baseline for the
  [work list](#servo-webgl2-work-list), which has the results after it. The
  probe page draws 5,000 points and 10,000 links
  ([manifest](evidence/notes-app-2026-10-25/manifest.json),
  [page](evidence/notes-app-2026-10-25/graph-probe.html)):
  - Shipping Frame leaves `dom_webgl2_enabled` off, so cosmos.gl 3.5.1 cannot
    create its luma.gl device
    ([result](evidence/notes-app-2026-10-25/servo-default-v3.json)). cosmos.gl
    2.6.4, the WebGL1 version on regl, starts at 17 fps, then regl refuses
    float readback ("Reading 'float' values is not permitted") and nothing is
    drawn ([result](evidence/notes-app-2026-10-25/servo-default-v2.json),
    [screenshot](evidence/notes-app-2026-10-25/servo-default-v2.png)).
  - With WebGL2 enabled in a temporary probe build, cosmos.gl 3.5.1 starts at
    28 to 29 fps and still draws no nodes
    ([result](evidence/notes-app-2026-10-25/servo-webgl2-v3.json),
    [screenshot](evidence/notes-app-2026-10-25/servo-webgl2-v3.png)). Servo
    exposes no WebGL2 extensions; Chromium 152 exposes 36. `copyTexSubImage3D`
    is missing: it and the three `texSubImage3D` overloads are commented out
    in `WebGL2RenderingContext.webidl` (`:335`, `:339`, `:343`, `:347`).
    `ALIASED_POINT_SIZE_RANGE` reads [0, 0] under WebGL2 after
    `GetParameterFloat2` fails (WebGL1 reads [1, 64], Chromium [1, 511]). With
    the simulation off, `drawBuffers([BACK])` on the default framebuffer fails
    11 times
    ([result](evidence/notes-app-2026-10-25/servo-webgl2-v3-sim0.json),
    [log](evidence/notes-app-2026-10-25/servo-webgl2-v3-sim0.err)). Float
    `readPixels` fails with error 1282. Overriding the point range in the page
    still draws nothing
    ([result](evidence/notes-app-2026-10-25/servo-webgl2-v3-sim0-pointfix.json)).
  - Every WebGL2 run reads positions back as zeros (`positionSample` [0, 0, 0,
    0], progress 0). Those results still report `positionsMovedOf400` as 400,
    but only because zeros differ from the starting layout: the readback
    failed, and nothing shows that the simulation ran.
  - The same page in Chromium 152 runs at 120 fps with real positions,
    simulation progress 0.048 and float readback [0.5, 0.5, 0, 1]
    ([result](evidence/notes-app-2026-10-25/chromium-v3.json)). That control
    ran in the Claude desktop app's browser, not in Frame.
- A Canvas 2D graph does run in shipping Frame. force-graph 1.52.0 (MIT,
  Canvas 2D over d3-force-3d, published 2026-09-28) drew the same 5,000 nodes
  and 10,000 links at 13 fps and 13 simulation ticks a second; all 200 sampled
  nodes moved, and the canvas held node and link pixels
  ([page](evidence/notes-app-2026-10-25/graph-probe-canvas.html),
  [result](evidence/notes-app-2026-10-25/servo-default-canvas.json),
  [screenshot](evidence/notes-app-2026-10-25/servo-default-canvas.png)).
  Chromium 152 ran it at 32 fps on a larger canvas, 2048 by 1536 against
  Servo's 1280 by 820
  ([result](evidence/notes-app-2026-10-25/chromium-canvas.json)). When
  cosmos.gl starts in Chromium, luma.gl asks for 25 WebGL extensions
  ([log](evidence/notes-app-2026-10-25/chromium-v3-extensions.json)).
- zorite at packetThrower/zorite@6ee46f2 is GPL-3.0-or-later, but its
  zorite-editor 0.11.2, zorite-markdown 0.10.0, gpui-whiteboard 0.5.1 and
  gpui-bidi crates are MIT. They build on gpui-pre 0.3.2 (zed@801c087); every
  GPUI name they import exists in 0.3.9, but signatures are unchecked.
  zorite-editor has IME, live preview and tables, and no CRDT seam: edits go
  through the private `record_edit` (`lib.rs:3025`) at 17 sites, three paths
  bypass it, undo stores whole-document snapshots, and `normalize_loaded`
  rewrites `$$` fences on load
  ([assessment](evidence/notes-app-2026-10-25/zorite-assessment.md)).
- Velotype at manyougz/velotype@07a24a9 (Apache-2.0) runs on the same gpui-pre
  0.3.9, but its block tree is the truth and it rewrites Markdown in a
  canonical form on save, the opposite of what a Loro text authority and
  Obsidian need
  ([assessment](evidence/notes-app-2026-10-25/velotype-assessment.md)).
- Theorem's whiteboard fork (`apps/theoremweb/crates/gpui/whiteboard`) adds
  host-managed history and a Space adapter to gpui-whiteboard 0.5.1 but lacks
  five upstream commits, including the dashed-line memory fix 7c11533.
  theorem-space models every JSON Canvas field, but a round trip loses
  `background`, `backgroundStyle`, `subpath` and unknown fields, and replaces
  every id (zorite assessment).
- Theorem-UI PR #16 (head fb90fe5, merged on 2026-10-26 as `259f6d3`) is a web
  vault: dockview layout, cosmos.gl graph, Files API client and Crepe editor.
  It has no command palette, tags pane, bookmarks, hover preview or unlinked
  mentions, its outline does not follow the cursor, and its Cloudflare builds
  failed, on the merge commit as well. Its description says the vault was not
  exercised against a live Theorem backend, and that its first-heading CSS
  rule, which uses `:is()` and `:not()` with selector lists, was not checked
  in the desktop host under Servo. Its graph model is about 150 lines of
  TypeScript
  ([assessment](evidence/notes-app-2026-10-25/theorem-ui-pr16-assessment.md)).
- GPUI Kit's command palette matches with a case-insensitive substring test
  (`vendor/gpui-component/src/command/item.rs:114-127`). Its `Tree` wraps
  gpui-base's `TreeState`; behaviour at 50,000 entries is unverified. Its dock
  has no tab pinning. Components that cover parts of this plan: command, tree,
  breadcrumb, hover_card, calendar, date_picker, color_picker, kbd,
  status_bar, searchable_list, list, virtual_list, table, tab, dock, sidebar,
  setting, diff, input, text (read-only Markdown with frontmatter),
  highlighter, menu, notification, popover, resizable, tag, tooltip, chart,
  plot and title_bar.
- comrak 0.58.0 (BSD-2-Clause; 0.56.0, 0.57.0 and 0.58.0 came out on
  2026-10-06, 10-21 and 10-26) has extensions for wikilinks (title before or
  after the pipe), Obsidian callouts (`ofm_callouts`: custom titles, metadata
  after a pipe, and `+` and `-` folding, written to HTML as `data-callout` and
  `data-callout-fold`) and `%%` comments (`ofm_comments`), `==highlights==`,
  GitHub alerts, footnotes and inline footnotes, front matter, three math
  syntaxes, task lists, subscript, superscript, underline, spoilers and block
  directives, and can rewrite link and image URLs. With wikilinks on, it
  parses `![[embeds]]` as wikilinks with an `embed` flag (`src/nodes.rs:373`),
  and `parse.relaxed_tasklist_matching` keeps any task character
  (`NodeTaskItem.symbol`, `:334`; by default only `x` and `X`). Every node
  carries a line and column `Sourcepos`, and `render.sourcepos` writes them
  into the HTML as `data-sourcepos`; columns count UTF-8 bytes unless
  `parse.sourcepos_chars` is set. It does not parse `#tags` or `^block` ids.
- Dataview 0.5.70 (MIT, released 2025-04-07; the newest commit on its default
  branch is from the next day) exports its parsers from `src/index.ts`
  (`EXPRESSION`, `QUERY_LANGUAGE`, `parseField`, built on parsimmon and
  luxon), though that file also has a bare `import "obsidian"`. Its executor,
  `src/query/engine.ts` (`executeTable`, `executeList`, `executeTask`,
  `executeCalendar`), imports nothing from `obsidian` directly but takes a
  `FullIndex`. No Rust implementation of its query language turned up. For
  Moment.js format tokens, `moment` 0.1.0 (MIT, one release in 2019, no
  repository) formats with them but leaves the zone token `z` unimplemented,
  `kg_moment` 0.0.1 is a chain of string replacements, `tempotime` 0.1.3 uses
  Luxon's tokens, and no crate converts Moment tokens into another library's
  format.
- Obsidian 1.14.5 is inventoried in
  [obsidian-inventory.md](evidence/notes-app-2026-10-25/obsidian-inventory.md).
  Two details checked on 2026-10-26: 1.14's six highlight colours are stored
  as a coloured-circle emoji (U+1F534, U+1F7E0, U+1F7E1, U+1F7E2, U+1F535 or
  U+1F7E3) at the start of a `==highlight==`; and search supports `file:`,
  `path:`, `content:`, `match-case:`, `ignore-case:`, `tag:`, `line:`,
  `block:`, `section:`, `task:`, `task-todo:`, `task-done:`, `[property]`,
  `[property:value]`, `/regex/`, `OR`, `-`, quotes and parentheses, with
  results embeddable in a note through a `query` code block.

## Decisions

**Notes are source documents in the IDE document authority.** Every note,
canvas and base is a source document in `theorem-ide-authority`, edited
through `theorem-ide-runtime`'s `DocumentAuthority` with Loro text. The
editor, the reading view, the canvas, Bases and every pane are clients; none
of them writes a file. A note keeps one identity in every view: an editor tab,
a reading panel, a canvas card, a hover preview and a graph node all show the
same source document, and closing or removing a view never changes the note.
This is the foundation's rule of one authoritative durable writer per domain
(theorem-native-foundation.md:84) and Theorem-UI's D7 (file bytes are the
truth), and it builds D12's pending per-file Loro sync on the runtime's
sessions rather than on new code. Frame binds one `RedCoreAuthority` per vault
under its profile directory, as `theorem-ide-host` does for each workspace on
Theorem main; the runtime's in-memory default never holds a vault. Each note
then exists in three places: its file, its head in RedCore, and, while it is
open, its session. A rename is one multi-document transaction that moves the
file and rewrites every link to it. A change made outside Frame arrives as a
`Reconcile` revision. Today `documents_reconcile` keeps one side, and keeping
the disk discards the unsaved edits, the Loro record and undo. Add a three-way
merge upstream: fork the session's Loro record at the frontier of the revision
last written to disk (`LoroDoc::fork_at`, `loro` `src/lib.rs:159`), apply the
disk text to the fork with `LoroText::update`, which computes the edit itself,
and import the fork's changes into the session as `import_remote` does, so the
unsaved edits and the disk change both survive and undo keeps its history.
Calling `update` on the live record instead would delete every unsaved edit,
because it diffs the whole text against the disk. The Files API branch merges
remote writes the same way, applying each side to a copy of the common base
(`merge_text(base, ours, theirs)`, `theorem-files/src/merge.rs:38`). When both
sides changed the same lines, offer both versions in a diff instead of saving
the merge. Canvases and bases are Loro text too, because their bytes are the
truth as well: the board and the Bases views write the smallest text edit in
the file's existing formatting, and a merge that leaves JSON or YAML that does
not parse goes to the same diff instead of being saved. Undo is per note, as
the runtime keeps it: one history per open document, shared by every pane that
edits the note. Disk changes, through the merge above, and remote changes
arrive through the import path, which records no local undo, so undo never
reverts them. Notes never use the runtime's written-document sessions, because
`theorem-doc-session`'s block tree makes the graph the truth and the Markdown
file a projection.

**Local vaults go through rustyred-thg-vfs.** The VFS mounts the vault folder,
and each file has one writer. The runtime writes notes, canvases and bases
when the authority saves or renames them, recording the hash it wrote
(`record_disk_hash`) so the VFS event that follows reads as Frame's own write;
the VFS writes everything else (attachments, imports, moves, and deletes to
the Trash). The VFS provides typed change events, a full rescan for
completeness (watcher events are hints) and snapshot history for attachments
as well as notes. Two of step 1's upstream changes protect these bytes,
because every user of these crates needs them. The first is atomic writes in
both `write_mounted_file` and the runtime's `write_file`: the VFS opens a
temporary name with `openat` in the pinned directory, flushes it with
`F_FULLFSYNC`, renames it over the target with `renameat` and syncs the
directory, and the runtime does the same with `tempfile` (already a
dev-dependency there), whose `sync_all` issues `F_FULLFSYNC` on macOS before
`NamedTempFile::persist` renames the file. The second is a move from `notify`
6 with kqueue to `notify` 8 with FSEvents, which matches Frame's lock and does
not hold a descriptor per file. A Markdown file opened from outside any vault
(Obsidian 1.14) gets a single-file session whose links and images resolve
relative to its folder.

**Remote vaults go through Theorem's Files API.** The remote RustyRed database
is a Theorem vault behind Files API v1, reached with the SDK+ Rust client
`theorem-sdk`. The contract is the SDK+ candidate's OpenAPI file
(`0.6.0-alpha.1`, 22 operations), which supersedes PR #16's 0.5.0, and its
server is the native process built from the SDK+ worktree, so step 12 builds
and tests against that process run locally, and shipping remote vaults waits
for that work to be committed, merged and deployed. Neither file route on Theorem
main replaces it: `/api/project-files/v1` is read-only and serves one checkout
the server chooses, and `theorem-ide-host` carries an execution grant
(subprocesses, terminals, extensions) that a notes client should never hold.
Writing to a remote RedCore directly would add a second writer beside its
authority, which the foundation's rule forbids. Frame keeps a local mirror of
each remote vault under the VFS, so search, the graph and editing keep working
offline. It follows changes with the `/events` long poll and sends the API's
CSRF, idempotency and client-id headers. A save sends `If-Match` with the hash
it edited. The server merges an edit made against an earlier projection
through Loro and answers `merged: true` (the branch's spec, `PUT /content`),
and Frame applies the merged text to the open session with the same three-way
merge it uses for disk changes; an unknown base gets a 412 with the current
hash. Remote notes are the doc-store's Markdown projection of block documents,
so a remote vault must pass the same byte-for-byte checks as a local one, and
any normalisation in the projection is a fix to propose upstream. How a native
client signs in and gets its CSRF token is open: the recorded smoke ran with
authentication off. The SDK+ record shows bootstrap answering an API token
that has no product user (it made `principalId` nullable for that case), so an
API token is one route; how a Frame user gets one is unchecked. Loro update
exchange would save sending whole files; propose it once the whole-file path
passes.

**The editor is a fork of zorite-editor.** Fork zorite-editor, zorite-markdown
and gpui-bidi into `vendor/` and move them to gpui-pre 0.3.9. Of the three
bases, GPUI Kit's `Input` (gpui-base 0.8.0) has a rope buffer, text
decorations (`src/input/editor/decorations.rs:72`, a `HighlightStyle`, so
colour and weight but no font size), filled and framed ranges (`:13`), inline
tokens (`src/input/base/inline_tokens.rs:12`) and folding
(`src/input/editor/display_map/folding.rs:3`), but its display map has only
wrap and fold layers (`display_map.rs:29`), so it cannot draw larger heading
lines or block widgets for tables, images and embeds; the Velotype assessment
preferred it only over Velotype. zorite has a fixed set of block and line
marks and host views for images, math, Mermaid and embeds, but no general way
to decorate a text range (zorite assessment), so the fork adds one for
highlights, search matches and spelling. Velotype's block model is the wrong
way round. Connect the fork to the authority at `record_edit` and its three
bypasses: emit every edit, add an `apply_remote` that shifts the selection,
IME range and diagnostics without touching undo (`remap_diagnostics` at
`lib.rs:1972` shows the pattern), replace its snapshot undo with the session's
undo, and turn off `normalize_loaded`. `set_text` (`lib.rs:1221`) resets the
caret and clears undo, so it only loads a note, and remote changes never go
through it. Then add what Obsidian has and zorite lacks: all 13 callout types
with custom titles and the snippet-defined types, a properties widget for
frontmatter, inline embeds, Unicode tags, `%%` comments, footnotes, heading
and list folding, find and replace, multiple cursors and rectangular
selection, Vim bindings (evaluate modalkit 0.0.32 first, a pre-0.1
modal-editing library aimed at terminal applications; parity means
CodeMirror's Vim command set), macOS spellcheck through NSSpellChecker
(`objc2-app-kit`, already locked), smart lists, auto-pairing,
URL-over-selection paste, and HTML paste through htmd 0.5.6 (Apache-2.0). Port
from Velotype, with Apache-2.0 attribution: marker reveal, table editing, the
RaTeX and mermaid-rs-renderer SVG caches and the height cache. Measure typing
latency on a large note before changing the storage: the fork rebuilds its
whole `String` on every keystroke (`lib.rs:5203-5204`), and the session's
`TextModel` already keeps a rope if the measurement calls for one. The session
counts Unicode scalar values, zorite and comrak's source positions UTF-8
bytes, and macOS text input UTF-16 units, so the seam converts offsets in one
place, with fixtures for emoji, combining marks, CJK and right-to-left text.

**The reading view is comrak HTML in a Servo panel.** comrak 0.58 renders each
note with source positions and relaxed task matching, and a pass over its AST
adds what comrak lacks (tags, block ids and coloured highlights) and resolves
the embeds that comrak marks on wikilinks. pulldown-cmark 0.13.5 also reports
source ranges and has wikilinks, math and footnotes, but it has no Obsidian
callouts (its blockquote kinds are GitHub's five), `%%` comments or
`==highlights==`, so that pass would be larger. ammonia sanitises the note's
own HTML. Code blocks are highlighted in the page with Prism (`prismjs`
1.30.0, MIT), as in Obsidian's reading view, so the DOM matches and themes'
token classes apply. MathJax 4.1.3, which Obsidian has shipped since 1.14.1,
renders math in the page, and Mermaid renders diagrams behind Obsidian's
setting, off by default since 1.13. A `frame-notes:` protocol handler,
registered through `ServoBuilder::protocol_registry` in `Engine::start`,
serves note pages, preview fragments, graph data, attachments and the bundled
scripts. It answers only requests whose `target_webview_id`
(`components/shared/net/request.rs:821`) is one of the notes panels' webviews,
because a custom scheme's origin is opaque under the URL standard, so websites
in browser tabs and live web cards cannot read notes. The panel is a dock
panel like `TabPanel`, without browser chrome. Its default theme reads like a
well-set article, and it uses Obsidian's CSS variable and class names so that
CSS snippets and `cssclasses` apply. Link clicks reach `request_navigation`:
internal links open note tabs and web links open Frame browser tabs. When a
note changes, the host sends new HTML through `evaluate_javascript`, and
idiomorph (0.8.0, 0BSD) patches the DOM in place, keeping the scroll position
and folded callouts. Each patch carries the authority revision it was rendered
from, and the page drops a patch older than the one it shows. Math, diagrams
and embeds get ids derived from their source, so a patch keeps their rendered
output unless their source changed. Every block carries its source position,
which keeps the editor and the reading view on the same passage when Mod-E
switches between them, and lets a reading panel beside the editor follow its
position. Task toggles in the page go back through the host to the authority;
callout folds stay view state, as in Obsidian, kept by the host across
patches.

**GPUI previews use zorite-markdown.** Hover previews over GPUI surfaces
(explorer, search results, backlinks), note cards on canvases and embeds
inside Live Preview use the forked `MarkdownView`; a Servo web view per card
or preview would be too heavy. Inside the reading view, previews and embeds
are HTML from the same renderer as the page.

**The graph is cosmos.gl in a Servo panel.** Bundle cosmos.gl 3.5.1 with
luma.gl 9.3.6 and `@interacta/css-labels` 0.6.0 (all MIT) into the app;
nothing loads from a CDN. Port PR #16's graph model to Rust, so the index
produces the node and link arrays, kinds (note, tag, attachment, unresolved),
sizes, colours and the starting layout, and the page only draws. Obsidian's
graph settings (filters, groups, display, forces, local depth and link
directions) are GPUI controls. Changes reach the page through
`evaluate_javascript`, and node clicks come back as `frame-notes:`
navigations. The simulation pauses once it settles and while the panel is
hidden, and hiding the panel also throttles its page. Cosmograph is not used,
because of its non-commercial license, its telemetry and the DuckDB-WASM stack
it would also need from Servo. cosmos.gl needs the Servo WebGL2 work list.
Until that is done, the panel draws the same graph model with force-graph
1.52.0 (MIT), bundled the same way, which runs in shipping Frame at 13 fps on
the probe's 5,000 nodes and 10,000 links (see the baseline). WebGL2 is on for
`frame-notes:` pages only, through the `dom_webgl2_enabled_schemes` preference
that `is_webgl2_enabled` checks beside its host allow-list (set in
`src/engine.rs`), until Servo's WebGL tests pass for every page.

**Canvas is the whiteboard fork over theorem-space.** theorem-space, already
in the lock, is the JSON Canvas model, once its round trip keeps `background`,
`backgroundStyle`, `subpath`, unknown fields and ids. The board is Theorem's
whiteboard fork, rebased onto zorite 6ee46f2 with 7c11533 first, plus what
Obsidian Canvas has and the fork lacks: live Markdown note cards, groups with
labels and background images, coloured edges with labels and arrows at either
end, link cards, and file cards narrowed to a heading or block. A file card is
a placement of a note, not a copy: editing the card edits the note, and
deleting the card leaves the note alone. The fork's host-managed history
becomes the session's undo, as in the editor. Board positions never feed the
graph; only the references Obsidian counts (1.12) reach the index, checked
against the CLI's `backlinks`. Link cards show a page snapshot, become live
when focused, and open as web tabs, as in Obsidian.

**Live web cards are top-level webviews.** A live card is a Servo `WebView` of
its own, not an iframe, so a page that refuses to be framed still loads, and
it keeps its own origin, outside `frame-notes:`. It renders and takes input
the way `src/page.rs` already handles each browser tab inside GPUI's scene, so
the board composites it like any other element. While the board zooms, the
page's CSS viewport keeps the card's size: the host resizes the webview to the
card's on-screen pixels and sets its scale factor to the board's zoom times
the display's, so the page stays sharp and never reflows. Pointer events map
from screen to page coordinates through the board's transform. A drag on the
card's frame moves the card; once the card is focused, input inside it goes to
the page, and only one card or the board holds keyboard focus at a time. Cards
clip to their own bounds and to the board's viewport. A live card that leaves
the screen is hidden, which throttles it, and past a live-card limit set by
measurement the least recently focused card goes back to its snapshot, keeping
its URL and scroll position for when it is live again.

**The index is RedCore with rustyred-thg-find.** comrak is the one parser that
decides what a note contains. Its results go into the RedCore graph: files,
headings, blocks, links resolved by Obsidian's rules, embeds, tags, aliases,
typed properties, tasks, footnotes and inline fields. Every surface resolves
links through the index, so the editor, the reading view, backlinks, canvases
and the graph cannot disagree about a link's target. rustyred-thg-find's
exact, trigram and BM25 lanes answer search; its semantic lane stays optional,
since the app must work with AI off. Obsidian is closed source and documents
its search grammar only in prose, so parse it with winnow (already locked).
The editor's decorations come from zorite's parser and previews from
`MarkdownView`; conformance fixtures check that both agree with the index on
every link, tag and embed span.

**Bases, Dataview and formulas run in a QuickJS worker.** `.base` files are
YAML, and yaml-edit (0.3.5, Apache-2.0, pre-1.0, built on rowan) edits them
losslessly, so changes made in the UI keep comments, order and unknown keys.
Bases formulas are JavaScript-like expressions, so they run in `rquickjs`
against Frame's implementation of the Bases function groups (global, Any,
Date, String, Number, List, Link, URL, File, Object, RegExp) instead of a new
expression parser. Dataview queries run on Dataview's own MIT parser and
executor, bundled into the same worker, with Frame's index behind the
`FullIndex` the executor takes; check the bundle's import graph for `obsidian`
first. The worker has no file or network access: the host passes data in and
takes results out.

**Daily notes and templates use jiff with a Moment token table.** Obsidian's
date formats are Moment.js tokens. The one Rust crate that formats with them,
`moment` 0.1.0, had a single release in 2019 and leaves the zone token
unimplemented, and none converts them for `jiff`, so a token table over `jiff`
(already locked) is a justified hand-roll.

**The workspace extends DockArea.** Note tabs, tab groups and splits use GPUI
Kit's DockArea. Frame adds pinned tabs, stacked tabs, linked views, pop-out
windows and saved workspaces. The command palette and quick switcher use GPUI
Kit's command component with `nucleo-matcher` 0.3.1 (MPL-2.0, used unmodified)
in place of its substring test. Settings, hotkeys, the status bar,
breadcrumbs, the calendar and the ribbon use the matching GPUI Kit components.
The homepage is Obsidian's "Default file to open" setting plus dashboard notes
that embed bases and queries.

**Attachments use Glim's readers and Servo media.** PDFs, audio and video open
in tabs through Glim's readers (PDFKit, AVKit) once they are extracted as a
library and aligned to GPUI Kit 0.8.0, as the foundation already plans. Inside
the reading view, audio and video play through Servo's GStreamer media. PDF
embeds (`#page=` and `height=`) use pdf.js (`pdfjs-dist` 6.4.299, Apache-2.0),
which Obsidian's own PDF viewer is built on, if it runs in Servo; if it does
not, the protocol serves pages rendered by PDFKit. The audio recorder is a
GStreamer capture into the attachment folder. Export to PDF prints the reading
view's HTML through WebKit (`WKWebView`), because the Servo fork has no print
output; the foundation already keeps WebKit as an explicit platform adapter
(theorem-native-foundation.md:193).

**Settings live with Frame.** Following Theorem-UI's D16, Frame does not read
`.obsidian/` as live configuration. An explicit one-time import maps what it
can (app, appearance, hotkeys, core plugin toggles, bookmarks, workspaces,
graph, property types, daily notes, templates and snippets) and lists what it
could not. Note ids follow Theorem-UI's D8, `vf_<ULID>` in
`.theorem/ids.json`; links still use paths and names, as in Obsidian.

**Themes split at the GPUI boundary.** Obsidian themes are CSS for its whole
Electron interface. In Frame, CSS themes and snippets apply to the Servo
surfaces (reading view, graph, slides and PDF export). GPUI surfaces use GPUI
Kit theme tokens, and the import maps a theme's core variables
(`--background-primary`, `--text-normal`, `--interactive-accent` and similar)
onto them, so colours follow the theme and layout rules do not. Custom callout
types cross the boundary: a snippet's `.callout[data-callout="type"]` rules
set `--callout-color`, which takes any CSS colour since 1.13, and
`--callout-icon`, and Frame reads those two properties with `cssparser`
(already in Frame's lock) so the editor draws the same types as the reading
view.

**Built-in panes need no manifest.** The notes panes are part of Frame and
register with its DockArea directly; theorem-blocks, which defines
AppManifest, is not in Frame's lock. AppManifest is the route for later
third-party extensions, through `pane.kind` contributions and its other
extension points, and a manifest never grants document authority
(theorem-native-foundation.md:84). Obsidian's plugin API is not reproduced.

**Code layout.** A headless `crates/notes` (vault, index, parser, renderer,
protocol and queries), a GPUI `crates/notes-ui`, the forks under `vendor/`,
and the web assets under `crates/notes/web` with a pinned npm lockfile,
bundled by esbuild and embedded in the binary.

## Architecture

```mermaid
flowchart LR
  disk[("Vault folder")] --> vfs["rustyred-thg-vfs"]
  remote[("Theorem Files API v1")] --> sdk["SDK+ Rust client"] --> mirror["Local mirror"] --> vfs
  vfs <--> authority["Document authority<br/>theorem-ide-runtime, theorem-ide-authority, Loro"]
  authority -- "saves notes" --> disk
  authority --> index["Vault index<br/>comrak, RedCore, rustyred-thg-find"]
  authority <--> editor["GPUI editor<br/>zorite-editor fork"]
  authority <--> canvas["GPUI canvas<br/>whiteboard fork, theorem-space"]
  canvas --> cards["Live web cards<br/>Servo webviews via src/scene.rs"]
  index --> panes["GPUI explorer, panes,<br/>switcher, palette, search"]
  index --> queries["QuickJS worker<br/>Bases, Dataview"]
  authority --> renderer["comrak renderer"] --> protocol["frame-notes: handler"]
  index --> protocol
  protocol --> reading["Servo reading panel"]
  protocol --> graph["Servo graph panel<br/>force-graph, then cosmos.gl"]
  reading -- "task toggles" --> authority
```

## Parity matrix

Step numbers refer to the implementation sequence below.

### Vault and files

| Feature | Obsidian 1.14.5 | Frame | Step |
| --- | --- | --- | --- |
| Vaults | Open a folder as a vault; vault switcher; one window per vault | One VFS mount and one window per vault; vault list in Frame's profile | 2, 5 |
| File explorer | Tree; create, rename, move, delete to trash; drag and drop, including from Finder; multi-select; sort orders; reveal the active file | GPUI Kit `Tree` over the VFS; deletes go to the macOS Trash | 5 |
| Coloured folders | Community plugins or CSS snippets (reference screenshot) | Folder colours in vault settings, drawn in the tree | 5 |
| Locations | New notes in the root, the current folder or a chosen folder; attachment folder | Same settings | 2 |
| Links follow renames | "Automatically update internal links" | One authority transaction per rename | 2 |
| Link format | Wikilink or Markdown; shortest, relative or absolute path | Same settings; resolution checked against the CLI | 2 |
| Excluded files | Hidden from search, the graph and unlinked mentions; ranked lower in the quick switcher and link suggestions | Index filter and a lower rank in nucleo results | 2, 5 |
| Unused attachments (1.12) | Offers to delete them with their note | Same prompt, from the index | 5 |
| File types | Markdown, canvas, base, images, audio, video, PDF | Notes, canvases and bases natively; media through GPUI images, Glim readers and Servo | 4, 5 |
| Single files (1.14) | Open a file from outside the vault; Open With; Quick Look; document icons | Single-file session; Launch Services registration; Quick Look extension; icons | 13 |
| File recovery | Periodic snapshots; browse, diff, restore | Authority and VFS history in GPUI Kit's diff view | 11 |
| Stable ids | None; paths only | `vf_<ULID>` in `.theorem/ids.json` (Theorem-UI D8) | 2 |

### Markdown and editing

| Feature | Obsidian 1.14.5 | Frame | Step |
| --- | --- | --- | --- |
| Modes | Live Preview (default), Source mode and Reading view; Mod-E toggles | Live Preview and Source in the zorite fork; Reading view in Servo, also beside the editor, following it | 4, 6 |
| Internal links | `[[Note]]`, `#Heading`, nested `#H1#H2`, `#^block`, `\|alias`, Markdown links; `[[##` and `[[^^` search the whole vault; following an unresolved link creates the note (Mod-click while editing) | zorite syntax; suggestions from the index with nucleo; unresolved links create notes in the configured location | 2, 6 |
| Embeds | Notes, headings, blocks; images with a size; external images; PDF `#page=` and `height=`; audio; video; `.base`; `.canvas` | Renderer HTML in the reading view, with pdf.js for PDFs; zorite embed views in Live Preview | 4, 6 |
| Callouts | 13 types plus aliases; custom titles; `+` and `-` folding; nesting; unknown types render as note; custom types from CSS snippets | comrak callouts; the fork extended past GitHub's five kinds; snippet types in both views | 4, 6 |
| Properties | YAML frontmatter typed Text, List, Number, Checkbox, Date, Date & time or Tags; `tags`, `aliases`, `cssclasses`; one type per name; links in properties update on rename (1.11) | yaml-edit; GPUI properties widget; types in vault settings | 2, 6 |
| Tags | `#a/b` nesting; Unicode; at least one non-digit; case-insensitive | AST pass in the index; Unicode tags in the fork | 2, 6 |
| Highlights | `==text==`; six colours from a leading colour emoji (1.14); a swatch in Live Preview | comrak highlights plus the colour pass; swatch in the fork | 4, 6 |
| Inline syntax | Strikethrough; footnotes `[^1]` and inline `^[…]`; `%%comments%%`; sanitised HTML; math with `$` and `$$` | comrak extensions; ammonia; MathJax in Servo, RaTeX in GPUI | 4, 6 |
| Tasks | Any character between the brackets | comrak's relaxed task matching; toggles write through the authority | 2, 4, 6 |
| Code blocks | Highlighted, with Prism in the reading view | Prism in the reading view; host-supplied highlighting in the fork | 4, 6 |
| Tables | Live Preview table editor: add, delete, move, sort, align | Velotype's table editing on zorite's `tables.rs` | 6 |
| Mermaid | Fenced diagrams; rendering off by default since 1.13 | Mermaid in Servo; mermaid-rs-renderer in GPUI; same setting and default | 4, 6 |
| Folding | Headings and lists | Fold ranges from the parse | 6 |
| Multiple cursors | Alt-click; Shift-Alt-drag for a rectangle | New in the fork | 6 |
| Vim | CodeMirror's Vim emulation | modalkit, if its engine fits | 6 |
| Spellcheck | macOS spellchecker | NSSpellChecker | 6 |
| Paste and drop | A URL pasted over a selection makes a link (1.11); Mod-Shift-V pastes plain text; HTML becomes Markdown; dropped files import | htmd; imports through the VFS | 6 |
| Editor settings | Smart lists, auto-pair, strict line breaks, readable line length, line numbers, indent guides, font size commands (1.14) | Same settings | 6 |
| Images | Drag to resize in Live Preview (1.12); full-screen viewer (1.13) | Resizing writes the size suffix; viewer as a GPUI overlay | 6 |
| Find and replace | In the open note | New in the fork | 6 |
| Slash commands | `/` lists commands | Command palette entries at the cursor | 6 |
| Right to left | Bidirectional text; the whole workspace flips (1.14) | gpui-bidi; mirrored DockArea | 5, 6 |

### Reading view

| Feature | Obsidian 1.14.5 | Frame | Step |
| --- | --- | --- | --- |
| Rendering | Reading view of Obsidian Flavored Markdown | comrak HTML over `frame-notes:` in a Servo dock panel | 4 |
| Website look | Depends on the theme (reference screenshot) | Default article theme; Obsidian's variables and classes | 4 |
| Links | Click; Mod-click for a new tab; Mod-Alt-click for a split; Mod-Alt-Shift-click for a new window; an unresolved link creates its note | `request_navigation` routes to note tabs, splits and windows, creating notes for unresolved links; web links go to browser tabs | 4, 5 |
| Interaction | Fold callouts, toggle tasks, jump to footnotes, copy code | Page script; task toggles return through the authority, and callout folds stay view state | 4 |
| Page preview | Hover popover over links | In-page popover from the preview route | 4 |
| CSS snippets, `cssclasses` | Snippets folder; per-note classes | Applied to Servo surfaces | 4 |
| Updates | Re-renders on change | idiomorph patch tagged with the authority revision; math, diagrams and embeds kept by source id | 4 |
| Export to PDF | File menu | WebKit print of the same HTML | 11 |

### Navigation and panes

| Feature | Obsidian 1.14.5 | Frame | Step |
| --- | --- | --- | --- |
| Backlinks | Linked and unlinked mentions; collapse; sort; more context; count in the status bar | Index queries; unlinked mentions by phrase search of the title and aliases | 7 |
| Outgoing links | Links and unlinked mentions | Index | 7 |
| Outline | Headings; click to jump; follows the cursor | Index plus the editor position | 7 |
| Tags | Nested tags with counts and sorting | Index in a GPUI Kit tree | 7 |
| Properties view | Every property with counts; rename everywhere | Index; one transaction | 7 |
| Bookmarks | Notes, headings, blocks, searches, folders, groups; bookmark search (1.13) | GPUI Kit tree; vault settings | 7 |
| Footnotes view | Sidebar list | Index | 7 |
| Hover preview | Optional Mod key; from links, the explorer, search and backlinks | GPUI hover card with `MarkdownView` | 7 |
| Quick switcher | Fuzzy over full paths (1.14), aliases, create on a miss; recent notes before typing | nucleo over the index; recent notes from the workspace | 5 |
| Command palette | Fuzzy; pinned commands | GPUI Kit command with nucleo | 5 |
| Breadcrumbs and history | Path in the view header; back and forward per tab | GPUI Kit breadcrumb; per-tab history | 5 |
| Note composer | Merge notes; extract a heading or selection; rewrite links on extract (1.13) | Authority transactions | 11 |
| Random note, unique note creator | Core plugins | Same | 11 |
| Web viewer | External links open as workspace tabs; canvas web cards open as tabs | Frame browser tabs in the notes DockArea | 5, 9 |
| Web viewer tools | Reader view (Mozilla Readability); Save to vault converts the page to Markdown (1.8.1); ads blocked by default, with EasyList-style lists | Readability 0.6.0 (Apache-2.0) and Defuddle 0.19.6 (MIT) in the page; adblock-rust 0.13.3 (MPL-2.0) at `load_web_resource` | 11 |

### Search

| Feature | Obsidian 1.14.5 | Frame | Step |
| --- | --- | --- | --- |
| Query language | The operators, property, regex and boolean syntax listed in the baseline | winnow grammar over rustyred-thg-find's lanes | 7 |
| Results | Explain search term; collapse; more context; six sort orders; copy results | GPUI search pane | 7 |
| Embedded results | `query` code block | Rendered in both views | 7 |

### Graph

| Feature | Obsidian 1.14.5 | Frame | Step |
| --- | --- | --- | --- |
| Global graph | Every note and its links | force-graph, then cosmos.gl after step 3, in a Servo dock panel | 8 |
| Filters | Search query, tags, attachments, existing files only, orphans | Index-side filter with the search grammar | 8 |
| Groups | A query and a colour | Same | 8 |
| Display | Arrows, text fade threshold, node size, link thickness, animate | Settings of the renderer and its labels | 8 |
| Forces | Center, repel, link force, link distance | The renderer's simulation settings, calibrated against Obsidian | 8 |
| Local graph | Follows the active note; depth; incoming, outgoing and neighbour links | Same, from the index | 8 |
| Interaction | Click opens, hover highlights neighbours, drag moves | Renderer events through the protocol | 8 |
| Canvas links (1.12) | Counted in backlinks and the graph | `.canvas` nodes in the index | 2, 8 |

### Canvas

| Feature | Obsidian 1.14.5 | Frame | Step |
| --- | --- | --- | --- |
| File format | JSON Canvas 1.0: text, file, link and group nodes; edges with sides, ends, colours and labels; preset colours 1 to 6 | theorem-space with a lossless round trip | 9 |
| Cards | Text; live notes; media; web pages and YouTube; other canvases; bases; folders; convert to file; narrow to a heading or block; swap file | Whiteboard fork with `MarkdownView` cards and Servo link cards | 9 |
| Edges | Labels, colours, direction, connections to groups, follow connection | Fork extended | 9 |
| Groups | Around a selection; nested; renamed; background image (cover, keep ratio, repeat); jump to group | Fork extended | 9 |
| Board | Pan; zoom; zoom to fit (Shift-1) and to selection (Shift-2); snapping; Alt-drag duplicates; align, distribute, arrange; PNG export; read-only mode | Fork plus the missing commands | 9 |
| Embeds | Canvases embedded in notes | Snapshot in the reading view; the board in Live Preview | 9 |

### Bases and queries

| Feature | Obsidian 1.14.5 | Frame | Step |
| --- | --- | --- | --- |
| Files | `.base` YAML; `base` code blocks; `![[File.base#View]]` | yaml-edit; GPUI views | 10 |
| Views | Table, cards, list, kanban (1.14); map needs the Maps plugin | GPUI Kit table and virtual list, cards, list, kanban; map not planned | 10 |
| Logic | Filters (and, or, not), formulas, summaries, group by, sort, limit | QuickJS worker with the Bases functions | 10 |
| UI | Search, drag-and-drop import, row menu (1.12); Group menu and collapsible groups (1.14) | GPUI | 10 |
| Dataview (reference screenshot) | DQL `TABLE`, `LIST`, `TASK` and `CALENDAR`; inline fields `key:: value`; inline queries `= expr` | Dataview's parser and executor in the worker; fields in the index | 2, 10 |

### Workspace

| Feature | Obsidian 1.14.5 | Frame | Step |
| --- | --- | --- | --- |
| Tabs, groups, splits | Every tab sits in a tab group; drag to split | DockArea | 5 |
| Pinned tabs | Links from a pinned tab open elsewhere | New in DockArea | 5 |
| Stacked tabs | "Stack notes" | New DockArea layout | 5 |
| Linked views | "Open linked view", "Link with tab" | Backlinks, outline and the local graph follow a chosen tab | 5, 7, 8 |
| Pop-out windows | Move tabs between windows | GPUI windows sharing the vault session | 5 |
| Pane menu | Each tab's More options menu (the three dots, or a right-click on the tab) | GPUI Kit menu with the same commands | 5 |
| Sidebars and ribbon | Left and right sidebars with tab groups; customisable ribbon | DockArea sides; GPUI ribbon | 5 |
| Status bar | Backlinks, editor mode, word count, sync | GPUI Kit status bar | 5 |
| Workspaces | Save and load layouts | DockArea serialisation in vault settings | 5 |
| Startup note | "Default file to open" (1.11); homepage dashboards (reference screenshot) | Same setting; dashboards embed bases and queries | 5, 10 |
| Settings | Searchable settings window (1.13) | GPUI Kit setting | 5 |
| Hotkeys | Customisable | GPUI keymap per vault | 5 |
| Appearance | Light and dark, accent colour, fonts, themes, snippets | GPUI Kit tokens; CSS on Servo surfaces | 4, 5 |

### Other core plugins

| Feature | Obsidian 1.14.5 | Frame | Step |
| --- | --- | --- | --- |
| Daily notes | Moment date format, folder, template | jiff and the token table | 11 |
| Templates | Template folder; `{{date}}`, `{{time}}` and `{{title}}`, with formats | Same | 11 |
| Calendar | Community plugin (reference screenshot): a dot per 250 words, a hollow dot for open tasks, optional week numbers that open weekly notes | gpui-base's calendar with the dots drawn in its item slot (`src/calendar.rs:552`), a week-number column and weekly notes | 11 |
| Audio recorder | Records into the note | GStreamer capture | 11 |
| Slides | A note split on `---` | Servo panel in full screen | 11 |
| Word count | Words and characters in the status bar | Index counts, and the selection's while there is one | 5 |
| Publish | Paid hosted site | Static site export from the renderer | 11 |
| Sync | Paid hosted sync | Remote vaults through the Files API | 12 |

### Platform

| Feature | Obsidian 1.14.5 | Frame | Step |
| --- | --- | --- | --- |
| URI scheme | `open`, `new`, `daily`, `unique`, `search`, `choose-vault` and `hook-get-address`; shorthands for vault paths and absolute paths | A Frame URL scheme with the same actions and parameters | 13 |
| CLI (1.12) | Commands for files, links, search, properties, tags, tasks, bases, daily notes, templates and workspaces | A CLI that drives the running app, with the same verbs where they apply | 13 |
| Other platforms | Windows, Linux, iOS, Android | Not planned | none |

### The ten most-downloaded community plugins

Downloads are cumulative, from Obsidian's stats file on 2026-10-25.

| # | Plugin | Downloads | In this plan |
| --- | --- | --- | --- |
| 1 | Excalidraw | 8,710,895 | No. Its drawings are Excalidraw scenes stored inside Markdown; the files open as Markdown. |
| 2 | Templater | 6,017,669 | No. Its templates run JavaScript against Obsidian's API. Core templates are step 11. |
| 3 | Dataview | 5,204,016 | DQL, inline fields and inline queries in step 10; not `dataviewjs`. |
| 4 | Tasks | 4,502,158 | No. Tasks are indexed, searchable with the `task:` operators and listed by DQL `TASK`; the plugin's own query language is not. |
| 5 | Git | 3,353,680 | No. File recovery covers local history. |
| 6 | Advanced Tables | 3,311,919 | Table editing in step 6; not its spreadsheet formulas. |
| 7 | Calendar | 3,220,515 | Yes, in step 11. |
| 8 | Copilot | 2,921,541 | No. Theorem's AI surfaces arrive through `composer.verb`, and the app must work without them. |
| 9 | Style Settings | 2,799,461 | No. It reads settings declared inside theme CSS. |
| 10 | Kanban | 2,782,343 | Bases' kanban view in step 10; the plugin's own board files render as lists. |

Homepage (21st) is covered by the startup note. Hover Editor (43rd) is not:
core page previews are read-only.

## Implementation sequence

1. **Foundations.** On the Theorem branch Frame pins, land atomic writes in
   the VFS and the IDE runtime, the VFS move to `notify` 8 with FSEvents, and
   the three-way merge in `documents_reconcile`, then move
   `script/bootstrap`'s pin to the commit that carries them. Add
   `theorem-ide-runtime`, `theorem-ide-authority` and `rustyred-thg-vfs` as
   path dependencies from the checkout Frame already uses, and confirm with
   `cargo tree --locked --duplicates` that Loro, RedCore and GPUI each resolve
   once. Record everything else they bring in, among them
   `theorem-doc-session` with `pulldown-cmark` 0.12, the `commonplace` crate
   with `theorem-harness-core`, and a second `ropey` (1.6.1 beside GPUI Kit's
   2.0.0-beta.1), and check that none of it starts a network client or an AI
   provider, for gate G6. Vendor the zorite crates and gpui-bidi and port them
   to gpui-pre 0.3.9 without changing Frame's GPUI graph. Rebase the
   whiteboard fork onto zorite 6ee46f2, taking 7c11533 first, and move it from
   Theorem's zed `6d4d907` and gpui-component 0.7 to gpui-pre 0.3.9 and GPUI
   Kit 0.8.0, replacing its two theorem-design-gpui font functions with a
   Frame font provider. Build the fixture vault, the 10,000-note generator and
   the reference harness, which runs Obsidian 1.14.5's CLI over each vault and
   stores its output, with Obsidian's version, as the expected result.
2. **Vault core, headless.** In `crates/notes`, with no GPUI: vault mounts,
   the `RedCoreAuthority` binding and authority sessions, the comrak index
   parser and its AST pass, link resolution, the `.canvas` references Obsidian
   counts (1.12), the RedCore index, renames as transactions, excluded files,
   ids and settings. Done when the fixture vault matches every CLI result,
   rename tests produce Obsidian's rewrites, the kill, external-edit and
   three-way merge tests lose nothing, and index time, memory and the
   authority's size on disk for the generated vault are recorded beside
   Obsidian's.
3. **Servo WebGL2.** The [work list](#servo-webgl2-work-list) below, in the
   Servo fork. It needs nothing from steps 2 and 4 to 7 and runs beside them.
   Done when the graph probe draws nodes in Servo (checked by pixels), reads
   back real positions with progress above zero, has its frame rate recorded
   beside Chromium's, passes Servo's WPT WebGL tests for the changed areas,
   and WebGL2 is on for `frame-notes:` pages. Turning it on for every page
   waits until Servo's WebGL2 tests pass as a whole. Status: done on the
   branches named in the work list, not merged.
4. **First slice, then the reading view.** Start with one note end to end:
   open it from the fixture vault in a minimal editor tab on the fork's
   authority seam, show it in a Servo reading panel, save, close and reopen
   it, change it from another program, and kill the app during a save. Beside
   it, put one live web card on a board and check resizing, focus, scrolling,
   clipping and zoom, and measure how many live cards a board holds and what
   switching a card to live costs. The graph panel joins this proof on
   force-graph, and moves to cosmos.gl after step 3. Then the full reading
   view: the protocol handler and its webview check, the renderer, the panel,
   the default theme, link routing, notes created from unresolved links, page
   previews, idiomorph updates, interactive callouts, tasks and footnotes,
   every embed type, MathJax, Mermaid and snippets. Add a `notes_check`
   example, built like `media_check`, that loads fixture notes and prints
   their DOM structure. Done when the slice loses no edit in those checks, the
   live card passes its checks, and every fixture note matches the structure
   of Obsidian's reading view.
5. **Workspace and explorer.** A notes window on Frame's chrome (title bar,
   Liquid Glass frame, DockArea) with the explorer, note tabs that switch
   between editor and reading view, pinned and stacked tabs, linked views,
   pop-outs, saved workspaces, the ribbon, status bar, breadcrumbs, history,
   quick switcher, command palette, settings, hotkeys, startup note, folder
   colours, vault switcher, appearance, the pane menu, the unused-attachment
   prompt, attachment tabs through Glim's readers once they are a library, the
   one-time `.obsidian` import with its theme mapping, and web tabs. Check
   GPUI Kit's tree with 50,000 entries.
6. **Editor.** Every editing row of the matrix, on the seam built in step 4,
   with the fork's new range decorations. Done when the fixture edits produce
   the same bytes as the same edits in Obsidian, undo never reverts a change
   from disk or another client, and typing latency on the large-note fixture
   meets the threshold set from Obsidian.
7. **Panes and search.** Backlinks with unlinked mentions, outgoing links, an
   outline that follows the cursor, tags, the properties view, bookmarks,
   footnotes, hover previews, the search pane with the full grammar, and
   `query` blocks. Done when results match the CLI's `search`,
   `search:context`, `backlinks`, `tags` and `properties`.
8. **Graph.** Starts on force-graph and moves to cosmos.gl once step 3 is
   done. The Rust graph model, the panel, GPUI controls for every Obsidian
   setting, the local graph, groups, interaction and labels. Read Obsidian's
   defaults from a fresh vault's `.obsidian/graph.json`, because its
   documentation gives no slider ranges. Done when node and edge counts match
   Obsidian under each filter, clicking, hovering, dragging, selection and
   resizing work in the panel, the simulation pauses when settled or hidden,
   and the frame rate on the generated vault is recorded beside Obsidian's for
   each renderer.
9. **Canvas.** The theorem-space round-trip fixes, then the board, with live
   web cards on the host from step 4. Done when every JSON Canvas sample and
   fixture board survives an unchanged save byte for byte and an edit without
   losing fields, concurrent edits to one board from two windows never save
   JSON that does not parse, and the reference screenshot's dense boards, with
   coloured groups and labelled edges, render.
10. **Bases and queries.** The worker, the Bases function library, the four
    planned views, summaries and the Group menu, then Dataview. Done when
    results match the CLI's `base:query` and fixture DQL queries return the
    same rows as Dataview 0.5.70 in Obsidian.
11. **Remaining core plugins.** Daily notes, templates, the calendar with
    weekly notes, unique note creator, random note, note composer, audio
    recorder, slides, file recovery, PDF export, static publishing, and the
    web viewer's reader view, Save to vault and ad blocking.
12. **Remote vault.** Waits for SDK+ S1 to S3 to close and for the SDK+
    worktree to be committed, and builds against its native process run
    locally until it is deployed. Before Frame takes `theorem-sdk`, its exact
    `futures` and `reqwest` pins become ranges upstream, so it unifies with
    Frame's lock. Sign-in and the CSRF token, the client, the local mirror,
    event following, idempotent writes, `merged` answers applied through the three-way merge,
    and Loro update exchange if the contract adopts it. Done when two Frame
    clients and Theorem-UI edit the same remote vault, online and offline,
    without losing an edit, and remote notes pass the byte-for-byte file
    checks.
13. **Platform.** Single-file sessions, URL scheme actions, the CLI, Open
    With, Quick Look and document icons.
14. **Acceptance.** The full comparison from the target section, on the
    fixture, generated and help vaults, with AI disabled, recorded as evidence
    under `docs/plans/evidence/`.

## Servo WebGL2 work list

Each item came from the probe results and logs of 2026-10-25. All eight are
done in the Servo fork, on branch `feat/webgl2-cosmos` (18 commits on
`81d8e29`; not pushed or merged), and Frame's `feat/notes-webgl2` wires the
scheme. The probe was re-run after each item
([manifest](evidence/notes-webgl2-2026-10-26/manifest.json), one `itemN-*` run
set per build). Every number below is from a debug build.

1. **Extensions.** Done (`5ebb886`). `getSupportedExtensions()` lists 6 on
   WebGL2: `EXT_color_buffer_float`, `EXT_color_buffer_half_float`,
   `EXT_float_blend`, `OES_texture_float_linear`,
   `EXT_texture_filter_anisotropic` and `WEBGL_compressed_texture_s3tc`.
   luma.gl asks for 25
   ([log](evidence/notes-app-2026-10-25/chromium-v3-extensions.json)) and
   cosmos.gl draws and simulates with those 6. The other 19 are not exposed;
   most have no implementation in Servo (`EXT_render_snorm`,
   `EXT_texture_norm16`, `WEBGL_debug_renderer_info`,
   `EXT_disjoint_timer_query_webgl2`, `EXT_depth_clamp` and others).
2. **3D texture uploads.** Done (`9410629`): `texSubImage3D` (the
   `ArrayBufferView` form) and `copyTexSubImage3D`. The PBO and
   `TexImageSource` overloads are still not there, as in `texImage3D`.
3. **Point sizes.** Done (`b792c4e`). The desktop WebGL2 context is a core
   profile, which has `POINT_SIZE_RANGE` and no `ALIASED_POINT_SIZE_RANGE`;
   the range reads [1, 64].
4. **Default framebuffer.** Done (`9c01a24`). `drawBuffers([BACK])` and
   `readBuffer(BACK)` map to the surfman framebuffer's colour attachment. The
   468 `DrawBuffers` failures of a `?lib=v3` run
   ([item3](evidence/notes-webgl2-2026-10-26/item3-v3.err)) became none
   ([item4](evidence/notes-webgl2-2026-10-26/item4-v3.err)).
5. **Float readback.** Done (`c48525e`, `9f9f2a1`). `readPixels` takes the
   pairs of ES 3.0 4.3.2 for each colour-buffer kind (`RGBA`/`FLOAT`,
   `RGBA_INTEGER`/`INT` and `UNSIGNED_INT`, `RGBA`/`UNSIGNED_BYTE`), and
   `texImage2D` keeps the sized internal format that decides the kind
   ([matrix](evidence/notes-webgl2-2026-10-26/final-read-pixels-matrix.json)).
6. **Warnings.** Done (`f3a7289`). The `DetachShader` failures came from
   detaching shaders from a program that `glDeleteProgram` had already
   detached: 12 in a delete and replace cycle, none after
   ([before](evidence/notes-webgl2-2026-10-26/item6-lifecycle-before.err),
   [after](evidence/notes-webgl2-2026-10-26/item6-lifecycle-after.err)). The
   "zero texture because texture unloadable" warning is still logged, as
   before; it also comes from the Canvas 2D run.
7. **Drawing.** Done. Four separate causes, each a Servo bug, found with the
   probe's `sim=0` and `pointfix=1` modes and narrowed pages
   (`bf40d13`, `8d875a6`, `7129a82`, `bbc0d87`):
   - `texStorage2D` read its level count as a mipmap level and refused sizes
     that are not powers of two, so cosmos.gl's position and force textures
     were never allocated;
   - uniform block names kept ANGLE's `_u` prefix, so luma.gl found no
     bindings for a draw call and dropped it without a GL error;
   - `texSubImage2D` refused `RG`, `RED` and the `*_INTEGER` formats;
   - Apple's GL compiles shaders with fast math, which folds `isnan()` to
     false, so every point kept its NaN size and colour. The bodies of
     ANGLE's `isnan` overloads are rewritten to test the float's bits.
   The page's `pointfix=1` override makes no difference once the range is read
   correctly (109,394 against 109,735 node pixels).
8. **Scope.** Done (`dfd3fea`, and `src/engine.rs` here). The new preference
   `dom_webgl2_enabled_schemes` lists URL schemes, checked in
   `is_webgl2_enabled` after `dom_webgl2_enabled` and before the host list.
   Frame sets it to `frame-notes`; `dom_webgl2_enabled` stays off.
   `WebGLSync`, `WebGLQuery`, `WebGLTransformFeedback`, `WebGLSampler` and
   `WebGLVertexArrayObject` were gated by the preference alone and follow the
   same function now. The scheme-gate page
   ([frame-notes](evidence/notes-webgl2-2026-10-26/final-scheme-gate-frame-notes.json),
   [http](evidence/notes-webgl2-2026-10-26/final-scheme-gate-http.json)) finds
   a WebGL2 context and all six interfaces on `frame-notes:` and none on
   `http:`. The handler on that scheme in the probe is a minimal file server
   (`examples/graph_probe.rs`); the notes app brings its own.

Two more Servo changes came out of running the conformance tests: 3D texture
layers could not be attached to framebuffers (`e7a1abb`), and the WebGL2
unpack parameters (`UNPACK_ROW_LENGTH` and the others) were refused (`b6fa7f6`,
which luma.gl and any atlas upload use). Four more fixed what the first
conformance run found in the commits above (`5b2c23e`, `4d12230`, `e3a9352`,
`20c844a`).

**Results**, with the last build (Servo `20c844a`; the `final-*` runs; Chromium
152 for comparison):

| | Chromium | Frame, before | Frame, after |
|---|---|---|---|
| node-colour pixels, `sim=0` | not counted | 0 | 109,735 |
| link-colour pixels, `sim=0` | not counted | 0 | 182,911 |
| float readback of the first position, `sim=0` | not run | [0, 0, 0, 0] | [1851.84, 2248.23, 1979.82, 2013.56] |
| simulation on: first position after 5 s | [2223.2, 2116.91, 2241.68, 1977.31] | [0, 0, 0, 0] | [2070.63, 1879.61, 2458.17, 2061.23] |
| simulation on: `progress` | 0.048 | 0 | 0.0417 |
| cosmos.gl v3, simulation on, fps | 120 | 28 | 80 to 84 |
| force-graph (Canvas 2D), fps; no WebGL involved | 32 | 13 | 17 |
| WebGL2 extensions; point size range | 36; [1, 511] | 0; [0, 0] | 6; [1, 64] |
| GL errors in a `?lib=v3` run | not recorded | 2 `GetParameterFloat2` | none |

The frame rate depends on the machine's load, which other builds were raising:
the same binaries gave 7 to 11 fps at a load average of 50 to 70 and 83 to 84
at 18 to 25 (the `control-*` runs). Frame's numbers are for a debug build on a
machine under load (24 to 28 for the `final-*` runs); Chromium's are the
2026-10-25 control, not run again. With the simulation on, node pixels are
24,900 to 26,600 and link pixels about 1,000, because the layout is moving
when the screenshot is taken 1.5 s after load. The simulation-on position is
not at a fixed time, so only its being real, and different from the starting
layout, is the check.

**WPT** (`./mach test-wpt` over `conformance2` and the 17 `conformance`
directories the changes touch, 1,974 test files, with `dom_webgl2_enabled`
on): passing subtests went from 73,452 to 83,068. The largest changes:
`conformance2/misc` 689 to 5,343, `conformance2/textures` 2,580 to 6,094,
`conformance2/rendering` 3,138 to 4,203, `conformance2/reading` 378 to 588,
`conformance2/extensions` 608 to 774 and `conformance2/glsl3` 933 to 1,008. No
file passes fewer subtests, except four that differ between runs of one build:
`texture-upload-size` reports 54 to 94 subtests on the base build, and three
files crashed in the 6-process run of the branch and pass on both builds when
run alone (the manifest's `wpt.fileLevel`). The
`.ini` expectation files record WebGL2 off and were not rewritten, so the
counts come from the raw logs. What does not pass yet is in `wpt.gaps` of the
manifest; the largest are `compressedTexImage3D`, multisampled
depth and stencil renderbuffers, sRGB readback (`GL_FRAMEBUFFER_SRGB`),
feedback loop detection and the remaining WebGL2 extensions. The 6-process
suite crashes a few files, a different few on each run, on the base build as
well.

Turning `dom_webgl2_enabled` on for every page still waits until Servo's
WebGL2 tests pass as a whole.

## Upstream review

- **Theorem-UI PR #16** merged on 2026-10-26 (`259f6d3`), and its Cloudflare
  builds failed on every push and on the merge commit. Use its graph model,
  Files API client types, link resolution and rewrite rules, and OpenAPI
  contract as specifications, not as code. Its review found `[[a/Note]]` and
  `[[b/Note]]` collapsing into one graph node, a click on an unresolved node
  creating `Missing.md.md`, and a `C# basics` heading that links could not
  reach; each becomes a fixture.
- **zorite** 6ee46f2: fork the three MIT crates and gpui-bidi.
- **Theorem's whiteboard fork** at Theorem `6e57dbb`: rebase onto zorite
  6ee46f2 with 7c11533 first, and port it to gpui-pre 0.3.9 and GPUI Kit
  0.8.0.
- **theorem-space**: fix the round-trip losses upstream, where
  theorem-browser-substrate also uses it.
- **Velotype** 07a24a9: reference only; port the named pieces with
  attribution.
- **cosmos.gl** 3.5.1 (MIT): adopt once Servo's WebGL2 work is done.
  **force-graph** 1.52.0 (MIT): the renderer until then. **Cosmograph** 2.5.1:
  not used (CC-BY-NC-4.0 with a business license on request, telemetry, and
  DuckDB-WASM on top of the same WebGL2 needs).
- **Dataview** 0.5.70 (MIT): bundle its parser and executor, pinned. It has
  had no release since 2025-04-07, and Datacore, from the same author,
  describes itself as a "work-in-progress re-imagining" of Dataview.
- **comrak** 0.58.0 (BSD-2-Clause): adopt, pinned, since it releases often,
  and offer tag and block-id parsing upstream.
- **Readability** 0.6.0 (Apache-2.0), **Defuddle** 0.19.6 (MIT),
  **adblock-rust** 0.13.3 (MPL-2.0, used unmodified), **pdf.js** 6.4.299
  (Apache-2.0), **Prism** 1.30.0 (MIT) and **idiomorph** 0.8.0 (0BSD): adopt,
  pinned.
- **Theorem Files API**: the server and the `theorem-sdk` Rust client are
  uncommitted work in the SDK+ worktree (`Travis-Gilbert/sdk-foundation-20261025`,
  folded in from `sdk-source/files-api` at `a287925`); main `8e151d5` has no
  handler. Commit, merge and deploy it, turn the client's exact pins into
  ranges, and check its Markdown projection against the byte-for-byte
  fixtures.
- **obsidianmd/jsoncanvas** (MIT): vendor its sample canvases as fixtures.
  **obsidianmd/obsidian-help**: no license; local use only.

## Risks

- **cosmos.gl depends on Servo WebGL2.** The work list is done on the Servo
  branch and cosmos.gl draws and simulates in it, but the branch is not
  merged, `deps/servo-mse` does not point at it, and the conformance suite
  still fails on much of WebGL2 (see the work list). Cosmograph would not
  avoid it. force-graph covers the graph until the branch lands, but at 13 to
  17 fps on the probe's 5,000 nodes it may be slow on the generated vault.
- **Three parsers.** comrak decides what the index sees, zorite's parser
  decorates the editor, and `MarkdownView` renders previews. Conformance
  fixtures keep them in step, but every new syntax lands three times.
- **Porting zorite.** Names match gpui-pre 0.3.9; signatures and behaviour may
  not.
- **Data safety before the upstream fixes.** Until writes are atomic, a crash
  during a save can truncate a note, so do not open real vaults before step 1
  lands.
- **The durable binding is new.** The runtime's README calls its native
  durable-authority binding pending, so Frame's binding of `RedCoreAuthority`
  is new code; step 2's kill tests cover it.
- **A second copy of every note.** RedCore holds each note's head text and the
  text each edit removed, so the vault exists twice and its history grows with
  every edit; step 2 records the size on the generated vault.
- **Linked Theorem crates.** The runtime and the VFS bring
  `theorem-doc-session`, `commonplace` and `theorem-harness-core` into Frame's
  build; they must stay buildable at Frame's pin and inert with AI off (gate
  G6).
- **Combining marks.** zorite patches `gpui-pre-wgpu` for combining marks
  (zorite `Cargo.toml:306-307`); whether Frame needs the same patch is
  unverified, and it would touch the GPUI graph the foundation keeps fixed.
- **Large vaults.** kqueue descriptors until FSEvents, GPUI Kit's tree at
  50,000 entries, and QuickJS speed on large bases.
- **Live web cards.** How many live webviews one board can composite, and what
  switching a card from snapshot to live costs, are unmeasured until step 4.
- **pdf.js in Servo.** Whether pdf.js runs in Servo is unverified; PDFKit page
  images are the fallback.
- **Remote.** The Files API server is uncommitted SDK+ work, so remote vaults
  cannot ship until it is committed, merged and deployed; whether its Markdown
  projection keeps a note's bytes, and how a Frame user gets an API token, are
  unchecked.
- **Two renderers per feature.** MathJax and Mermaid in Servo against RaTeX
  and mermaid-rs-renderer in GPUI; WebKit against Servo for PDF export.
- **Media from a custom scheme.** Whether Servo's media elements play
  `frame-notes:` responses, including range requests, is unverified.
- **tree-sitter.** GPUI Kit's optional highlighter and any other tree-sitter
  user must agree on one version, because the `tree-sitter` crate declares
  `links`.
- **Obsidian keeps moving.** 1.15, in early access, replaces Mermaid with Pele
  and drops the Mermaid setting, and changes Bases filters, and URI and CLI
  appends. The target stays 1.14.5 until 1.15 is public; then re-run the
  inventory.
- **Accessibility.** VoiceOver support in GPUI and Servo is unverified here,
  while Obsidian's Electron interface is accessible.

## Not in this plan

- **Obsidian's community plugin API.** It is JavaScript against Obsidian's
  DOM, its CodeMirror 6 editor and its internal classes. Frame's editor and
  chrome are GPUI, so reproducing that API would mean emulating an Electron
  application. Third-party extensions would use theorem-blocks' AppManifest
  instead; the matrix shows what the most-used plugins get natively.
- **AI features.** Theorem's AI surfaces would arrive through `composer.verb`;
  the app must work without them (gate G6).
- **`dataviewjs` and Templater scripts.** Both run arbitrary JavaScript
  against Obsidian's API objects.
- **Excalidraw drawings.** A separate drawing format with its own editor; the
  files still open as Markdown.
- **Obsidian Sync and the hosted Publish service.** Both are paid services.
  Remote vaults use the Files API, and publishing is a static export. Sync's
  end-to-end encryption goes with it.
- **Importer and Maps.** Official plugins outside the core list; Bases' map
  view depends on Maps.
- **Format converter.** No longer in the core list, and 1.14 removed its old
  formats.
- **Saved pages, model viewers and executable documents.** Turning the page
  captures Frame already keeps for search into saved pages that a note can
  cite and annotate, CAD and glTF viewers, and MDX or React documents each
  need a plan of their own. Save to vault, which writes a page's Markdown into
  a note, is in step 11.
- **Windows, Linux and mobile.** Frame is macOS only.

## Relation to the foundation plans

[theorem-native-foundation.md](theorem-native-foundation.md) makes SDK+ S0 its
first implementation target, and its gates G0 and G1 then bind the repaired
SDK to a native host "before a large UI build" (:220). The local-vault steps
here never call the SDK, so this plan reads that ordering as covering
SDK-backed work: step 12 is the first step that uses the SDK, and it waits for
SDK+ S1 to S3, G0, G1 and a deployed Files API server. If the ordering is
meant to hold every large GPUI build in Frame, steps 4 onward wait for G1
instead. The rest of the foundation applies as written: one durable writer per
domain (:84), manifests that never grant authority, no upgrade of Frame's GPUI
graph, and Glim's readers once they are a library on GPUI Kit 0.8.0 (:193).
Its gates G6 (documents and tools work with AI disabled), G7 (a Glim-derived
viewer through existing registration and grants) and G8 (shared native and web
documents) belong to this plan's acceptance for the matching claims.

## Reproducing the checks

The probe pages and the no-op script are in the evidence folder. The pages
load cosmos.gl, luma.gl and force-graph from esm.sh, so the runs need network
access. Build the probe and serve the page:

```bash
script/media-env cargo build --locked --example media_check
python3 -I -m http.server 8765 --bind 127.0.0.1 --directory docs/plans/evidence/notes-app-2026-10-25
```

In another terminal, run shipping Frame against the WebGL2 and WebGL1 versions
of cosmos.gl:

```bash
FRAME_MEDIA_SCRIPT=docs/plans/evidence/notes-app-2026-10-25/graph-probe-noop.js script/media-env cargo run --locked --example media_check -- 'http://127.0.0.1:8765/graph-probe.html?lib=v3'
FRAME_MEDIA_SCRIPT=docs/plans/evidence/notes-app-2026-10-25/graph-probe-noop.js script/media-env cargo run --locked --example media_check -- 'http://127.0.0.1:8765/graph-probe.html?lib=v2'
```

The page logs one `FRAME_MEDIA_RESULT` line, and `media_check` prints its JSON
on stdout. The screenshot is `frame-media-svg-initial.png` in the system
temporary directory. `passed` means only that cosmos.gl started without
throwing; it does not check that nodes were drawn.

The Canvas 2D run uses the same build:

```bash
FRAME_MEDIA_SCRIPT=docs/plans/evidence/notes-app-2026-10-25/graph-probe-noop.js script/media-env cargo run --locked --example media_check -- 'http://127.0.0.1:8765/graph-probe-canvas.html'
```

The WebGL2 runs of 2026-10-25 used a temporary copy of `media_check` that
turned `dom_webgl2_enabled` on; that copy was deleted. Since 2026-10-26
`examples/graph_probe.rs` is part of the repository and runs the pages from
`frame-notes://probe/`, a scheme that has WebGL2 (it serves `--notes-root`
with a minimal file handler), and counts the node, link and background pixels
of the screenshot, which it prints with the page's JSON. It needs the Servo
branch `feat/webgl2-cosmos` in `deps/servo-mse`:

```bash
script/media-env cargo build --locked --example graph_probe
export FRAME_MEDIA_SCRIPT=docs/plans/evidence/notes-webgl2-2026-10-26/graph-probe-noop.js
P='script/media-env cargo run --locked --example graph_probe -- --notes-root docs/plans/evidence/notes-webgl2-2026-10-26'
$P 'frame-notes://probe/graph-probe.html?lib=v3'
$P 'frame-notes://probe/graph-probe.html?lib=v3&sim=0'
$P 'frame-notes://probe/graph-probe.html?lib=v3&sim=0&pointfix=1'
$P 'frame-notes://probe/scheme-gate.html?expect=1'
```

The other pages in `evidence/notes-webgl2-2026-10-26/` are checks of single
areas, each printing `passed`: `read-pixels-matrix.html`,
`program-lifecycle.html`, `shader-nan.html`, `unpack-params.html`,
`tex-storage.html`, `dbg-tex3d.html` and `dbg-readpixels-int.html`.
`scheme-gate.html?expect=0` run from a local HTTP server
(`python3 -I -m http.server 8765 --bind 127.0.0.1 --directory
docs/plans/evidence/notes-webgl2-2026-10-26`) checks that `http:` pages get no
WebGL2. Run Frame's stdout through `tail -n 1` for the JSON, and compare
the `final-*` files. `fps` counts animation frames in 5 s, so it depends on
the machine's load; the manifest's `loadAverage1m` records it.

Servo's WPT tests ran from the `feat/webgl2-cosmos` checkout through
`./mach test-wpt --binary <servoshell> --pref dom_webgl2_enabled=true
--processes 6` on `tests/wpt/webgl/tests/conformance2` and the `conformance`
directories listed in the manifest. mach refuses a checkout whose path
contains a space; the manifest's `wpt.runner` says how the runs worked around
that. The Servo changes have unit tests: `cargo test -p servo-config --lib`
(the scheme gate) and the `#[cfg(test)]` modules of `pixelunpack.rs` and
`shaderisnan.rs`.

Cargo reported 17 minutes 45 seconds for the first probe build of 2026-10-25;
later builds took 25 seconds to 13 minutes 16 seconds (manifest
`cargoBuilds`). For the Chromium controls, open the same URLs in Chromium and
read the `FRAME_MEDIA_RESULT` line from its console. The extension log came
from a console script that wrapped `getExtension` and `getSupportedExtensions`
on both WebGL prototypes before a second cosmos.gl graph ran for 4 seconds.

## Status

Work for steps 1 to 3 is merged into `feat/notes-app`, which is not pushed.
Step 1's atomic writes, FSEvents watching, three-way merge
and note moves are on `claude/notes-foundations` in Theorem, which Frame pins;
the moves commit is pushed, and a second round of review fixes to it is not yet
committed. The RedCore journal change is still in review on
`claude/notes-redcore-journal`. The zorite editor and whiteboard lane is
merged. Step 2's headless vault core is merged, and its index matches Obsidian
1.14.5 on all 537 recorded calls over the fixture vault
([evidence](evidence/notes-parity-2026-10-26.md)). Step 3's Servo WebGL2 work
is 27 commits on `81d8e29` on `feat/webgl2-cosmos` in the Servo fork, which
`script/bootstrap-media` now pins; that branch is not pushed yet, so a fresh
bootstrap cannot fetch it. The first end-to-end proof is step 4's slice. Remote
vaults (step 12) wait for SDK+ S1 to S3, G0, G1 and a deployed Files API
server. The requester chose cosmos.gl for the graph on 2026-10-26. One reading
is open for the requester: whether the foundation's G0 and G1 hold the local
steps (this plan reads them as holding only step 12).


### WebGL2 review follow-up (2026-10-26)

The review fixes are committed. That is nine more Servo commits on
`feat/webgl2-cosmos`, 27 on `81d8e29` in all and still not pushed, and the notes
protocol gate in Frame's `src/engine.rs`. `script/bootstrap-media` now pins that
branch's tip. The record is in
[`review-fixes.md`](evidence/notes-webgl2-2026-10-26/review-fixes.md).

What the fixes do:

- They harden mip and pixel arithmetic.
- They correct extension and readback validation, including WebGL 1 float
  readback.
- They enforce the notes protocol boundary in one place,
  `Engine::start_with_protocols`.

Servo now carries trusted initiator metadata, because custom-scheme origins are
opaque and referrers can be suppressed.

On WebGL conformance, the patched build passes 83,199 subtests against base's
73,592. The serial float run gains on every float extension file.

One gap remains, and it predates this work. Servo replaces a float texture that
has linear filtering but no linear-float support with RGBA8 black at upload
time. It should substitute black at draw time instead. That replacement accounts
for the remaining float blending failures and is left for its own change.
