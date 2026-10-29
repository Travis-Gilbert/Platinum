# Sources for the notes plan

This file lists where the facts in [notes-app.md](../../notes-app.md) that
were checked or added on 2026-10-26 come from. Code was read with read-only
commands (`git show <rev>:<path>`, `git grep`, `git rev-list`,
`git ls-remote`) and plain file reads. Web pages were read through Firecrawl,
package data from the crates.io and npm registry APIs, and GitHub data through
`gh`. Every line number below was read again on 2026-10-26.

## Frame and its Servo fork

Frame at `b7db174`, whose working tree changes only `README.md`:

- Browser tabs: `src/browser.rs:267` (`DockArea::new("frame-tabs", …)`) and
  `src/browser/dock.rs:70` (`impl Panel for TabPanel`).
- Page capture: `src/search.rs:204` (`capturable`, HTTP and HTTPS only) and
  `:234`, which captures with `BrowserCapturePolicy::default()`. That default
  is `BrowserCaptureDial::Full` (`apps/browser-substrate/src/lib.rs:124-126`
  in `deps/theorem-rustyweb-pw2` at `7ef4dfe`, and `:126-128` at `6e57dbb`).
- Navigation and engine start: `src/delegate.rs:202` (`request_navigation`)
  and `src/engine.rs:51` (`Engine::start`).
- Command palette matching:
  `vendor/gpui-component/src/command/item.rs:114-127`.
- Theorem pin: `script/bootstrap:16-17`
  (`7ef4dfe0c20def5dbc91705f051a6a846946ec54` on
  `claude/rustyweb-synthesis-on-request`).
  `git -C deps/theorem-rustyweb-pw2 rev-parse HEAD` gives `6e57dbb`, and
  `git log 7ef4dfe..6e57dbb` shows two commits that add reranking, crawling
  and embedding to the in-process browser search. Their 19 changed files are
  in `apps/browser-substrate` and the `rustyred-web`, `rustyred-rerank`,
  `rustyred-thg-fractal`, `rustyred-thg-mcp` and `rustyred-thg-server` crates.

The Servo fork in `deps/servo-mse` at `81d8e29`:

- Protocols: `components/net/protocols/mod.rs:38` (`ProtocolHandler`) and
  `:104` (`ProtocolRegistry::register`); `components/servo/servo.rs:1526`
  (`ServoBuilder::protocol_registry`).
- Webviews: `components/servo/webview.rs:455` (`resize`), `:475`
  (`set_hidpi_scale_factor`), `:490` (`show`), `:498` (`hide`), `:658`
  (`set_page_zoom`), `:721` (`paint`) and `:732` (`evaluate_javascript`);
  `components/constellation/constellation.rs:5409` (throttle messages for
  hidden webviews); `components/servo/webview_delegate.rs:1070`
  (`load_web_resource`); `components/servo/user_content_manager.rs:34`
  (`add_script`).
- Requests: `components/shared/net/request.rs:821` (`target_webview_id`).
- WebGL2: `components/script/dom/webgl/webgl2renderingcontext.rs:198`
  (`is_webgl2_enabled`) and `:217` (`WEBGL2_ORIGINS`);
  `components/script_bindings/webidls/WebGL2RenderingContext.webidl:335`,
  `:339` and `:343` (the three `texSubImage3D` overloads) and `:347`
  (`copyTexSubImage3D`), all commented out.

## Theorem runtime at Frame's pin

Read with `git -C deps/theorem-rustyweb-pw2 show 7ef4dfe:<path>`. Crate paths
are under `rustyredcore_THG/crates/`. None of these files changed between
`7ef4dfe` and `6e57dbb`.

- `theorem-ide-runtime`: `Cargo.toml:5` (description), `:12`
  (`loro = "=1.13.9"`), `:19` (`theorem-doc-session`) and `:23` (`tempfile`, a
  dev-dependency); `README.md:12` (products must bind the durable RedCore
  authority) and `:17` (the native durable-authority binding is pending);
  `src/authority.rs:177` (`DocumentAuthority`) and `:229`
  (`record_disk_hash`); `src/sessions/record.rs:1` (not a second undo owner);
  `src/sessions/source.rs:166` (`open_at`), `:483` (`import_remote`) and
  `:538` (`undo`); `src/services/authority_ops.rs:992`
  (`documents_reconcile`); `src/workspace.rs:315` (`write_file`) and `:321`
  (`std::fs::write`).
- `theorem-ide-authority`: `Cargo.toml:5` (description); `src/lib.rs:11` (each
  document's head, with its text inline), `:16-17` (each revision, with the
  exact text each edit removed), `:111` and `:117` (every accepted revision is
  fsynced before the commit returns), and `:123` (`RedCoreAuthority::open`).
- `theorem-doc-session`: `Cargo.toml:13` (`pulldown-cmark` 0.12) and `:18`
  (`theorem-block-registry`); `src/lib.rs:7` (the graph stays the sole source
  of truth).
- `rustyred-thg-vfs`: `Cargo.toml:5` (description) and `:14` (`notify` 6 with
  `macos_kqueue`); `src/lib.rs:688-689` (watcher events counted as hints,
  never as the completeness oracle), `:929` (`refresh`), `:1163` (`write`),
  `:1342` (`write_mounted_file`), `:1346` (its Unix branch calls
  `write_relative_file`), `:1356` (`std::fs::write` in its `cfg(not(unix))`
  branch), `:2025` (`write_relative_file`) and `:2043` (`openat` with
  `O_TRUNC` and `O_NOFOLLOW`).
- `commonplace`: `Cargo.toml:5` (description). The retiring `commonplace-api`
  is `apps/commonplace-api` (`README.md:3` and `src/lib.rs:12`).

## Theorem main and the Files API branch

Read in the local Theorem checkout, whose `origin/main` is `8e151d5`.

- The branch: `git rev-parse refs/heads/sdk-source/files-api` gives
  `a287925ace5cb3b39cf96d042418495586512502`, committed at
  2026-10-24T18:04:29-04:00, and
  `git rev-list --left-right --count 8e151d5...refs/heads/sdk-source/files-api`
  gives 2 and 11. `git ls-remote --heads origin` lists `main` at `8e151d5` and
  no `sdk-source/files-api`.
- Routes at `a287925`:
  `rustyredcore_THG/crates/rustyred-thg-server/src/theorem_files.rs:52-79` (19
  `.route` calls) and `:629-631` (`admit_write`: a tenant write, a current
  CSRF token and an `Idempotency-Key`, as `/api/documents/v2` mutations
  require).
- Merge: `rustyredcore_THG/crates/theorem-files/src/merge.rs:38`
  (`merge_text(base, ours, theirs)`).
- Smoke: `docs/plans/files-api/smoke.sh:5`
  (`RUSTY_RED_MODE=memory RUSTY_RED_REQUIRE_AUTH=false`) and `:22-24` (the
  CSRF token from `/api/chat/v1/bootstrap`);
  `docs/plans/files-api/smoke-output.txt:30` (`"merged":true`) and `:32-35`
  (412 answers, with `currentHash` at `:33`).
- Spec: `docs/plans/SPEC-THEOREM-FILES-API-1.0.md:15` (Loro canonical
  documents with a Markdown projection) and `:107` (an edit against an earlier
  projection is merged through Loro and answered with `merged: true`).
- Main at `8e151d5`:
  `rustyredcore_THG/crates/rustyred-thg-server/src/theorem_project_files.rs:29`
  (`THEOREMWEB_PROJECT_ROOT`) and `:30` (`MAX_READ_BYTES`, 4 MiB);
  `apps/theorem-ide-host/README.md:19-21` (the execution grant, and not a
  sandbox for mutually untrusted tenants);
  `apps/theorem-ide-host/src/http.rs:144` (`RedCoreAuthority::open` for each
  workspace), `:232` (one route for every operation) and `:240` (the operation
  list); `rustyredcore_THG/crates/theorem-ide-runtime/src/services.rs:5001`
  (`advertised_operations`, 188 names, `documents.exportUpdates` and
  `documents.importRemote` among them);
  `packages/theorem-sdk/fern/openapi.json` (Theorem Environment API 0.4.0,
  with 188 `x-theorem-operation` entries and the operationIds
  `documentSessions_exportUpdates` and `documentSessions_importRemote`).

## Theorem-UI PR #16

- `gh pr view 16 --repo Travis-Gilbert/Theorem-UI --json state,mergedAt,mergeCommit`:
  merged at 2026-10-26T03:29:52Z as `259f6d3`.
- `gh api repos/Travis-Gilbert/Theorem-UI/commits/259f6d3/check-runs`:
  "Workers Builds: theorem-ui" completed with `failure`.
- `gh api repos/Travis-Gilbert/Theorem-UI/pulls/16/files`: the proxy at
  `src/routes/api/files/v1/$.ts` and the vendored SDK 0.5.0 OpenAPI file. The
  statements about the live backend and the first-heading CSS rule come from
  the PR's description (`gh pr view 16 --json body`).

## Obsidian

Pages read through Firecrawl on 2026-10-25 and 2026-10-26. The changelogs are
those of the desktop releases.

- The CLI, including `eval` and `dev:dom`: the [CLI
  page](https://obsidian.md/help/cli) and the [1.12.0
  changelog](https://obsidian.md/changelog/2026-02-10-desktop-v1.12.0/).
- Highlight colours: the [syntax page](https://obsidian.md/help/syntax) and
  the [1.14.0](https://obsidian.md/changelog/2026-09-02-desktop-v1.14.0/) and
  [1.14.5](https://obsidian.md/changelog/2026-10-22-desktop-v1.14.5/)
  changelogs.
- Search operators and `query` blocks: the [search
  page](https://obsidian.md/help/plugins/search).
- MathJax 4.1.3: the [1.14.1
  changelog](https://obsidian.md/changelog/2026-09-08-desktop-v1.14.1/).
- Mermaid: the [1.13.6
  changelog](https://obsidian.md/changelog/2026-08-10-desktop-v1.13.6/) says
  1.13 made Mermaid diagrams off by default, and the [1.13.0
  changelog](https://obsidian.md/changelog/2026-05-28-desktop-v1.13.0/) adds a
  one-time banner that asks before Mermaid blocks render. That 1.14.5 keeps
  this default is an inference. The [1.15.0 early-access
  changelog](https://obsidian.md/changelog/2026-10-22-desktop-v1.15.0/) draws
  Mermaid diagrams with Pele instead of the Mermaid library, removes the
  Mermaid setting, and adds filter reordering to Bases and heading appends to
  the URI scheme and CLI.
- Prism in the reading view: the [syntax
  page](https://obsidian.md/help/syntax) and the API reference for
  [loadPrism](https://docs.obsidian.md/Reference/TypeScript+API/loadPrism).
- Callouts and snippet-defined types: the [callouts
  page](https://obsidian.md/help/callouts); `--callout-color` taking any CSS
  colour: the 1.13.0 changelog.
- Following an unresolved link creates the note: the [linking
  page](https://obsidian.md/help/link-notes).
- Word count: the [word count
  page](https://obsidian.md/help/plugins/word-count).
- Recent notes in the quick switcher: the [quick switcher
  page](https://obsidian.md/help/plugins/quick-switcher).
- Excluded files: the [settings page](https://obsidian.md/help/settings).
- The More options menu: the [tabs](https://obsidian.md/help/tabs) and [More
  options menu](https://obsidian.md/help/more-options-menu) pages.
- Community plugin downloads, read on 2026-10-25: Obsidian's [plugin stats
  file](https://raw.githubusercontent.com/obsidianmd/obsidian-releases/master/community-plugin-stats.json).

## Packages

Versions, licenses and publication dates come from
`https://crates.io/api/v1/crates/<name>` and
`https://registry.npmjs.org/<name>`, read on 2026-10-26. `loro` and
`gpui-base` were read from the local Cargo registry.

- `comrak` 0.56.0, 0.57.0 and 0.58.0 were published on 2026-10-06, 2026-10-21
  and 2026-10-26 (BSD-2-Clause). In the 0.58.0 source: `src/nodes.rs:334`
  (`NodeTaskItem.symbol`), `:373` (`NodeWikiLink.embed`) and `:602`
  (`CalloutFold`); `src/parser/options.rs:821`
  (`Parse.relaxed_tasklist_matching`), `:1015` (`Parse.sourcepos_chars`) and
  `:1217` (`Render.sourcepos`). The extension list is comrak's [`Extension`
  options](https://docs.rs/comrak/0.58.0/comrak/options/struct.Extension.html)
  on docs.rs.
- `htmd` 0.5.6 (Apache-2.0, 2026-10-15), `modalkit` 0.0.32 (Apache-2.0,
  2026-10-19), `nucleo-matcher` 0.3.1 (MPL-2.0, 2024-02-20) and `yaml-edit`
  0.3.5 (Apache-2.0, 2026-10-22).
- Moment tokens: `moment` 0.1.0 (MIT, 2019-05-20) from its [source on
  docs.rs](https://docs.rs/moment/latest/src/moment/locale/mod.rs.html),
  `kg_moment` 0.0.1 from its [source on
  docs.rs](https://docs.rs/kg_moment/latest/src/kg_moment/moment.rs.html), and
  `tempotime` 0.1.3 (MIT OR Apache-2.0) from its
  [README](https://github.com/hyoussef07/tempotime).
- `prismjs` 1.30.0 (MIT, 2025-03-10), `idiomorph` 0.8.0 (0BSD, 2026-09-05) and
  `force-graph` 1.52.0 (MIT, 2026-09-28).
- cosmos.gl: `README.md:137` in the `@cosmos.gl/graph` 3.5.1 package names
  `EXT_float_blend` as the extension behind its many-body force.
- Cosmograph 2.5.1: the licenses of `@cosmograph/cosmograph` and
  `@cosmograph/cosmos` 2.5.1 from the registry, and the business-license
  contact in the `@cosmograph/cosmograph` package README.
- `loro` 1.13.9: `src/lib.rs:159` (`LoroDoc::fork_at`).
- `gpui-base` 0.8.0: `src/calendar.rs:552` (`ItemRenderer`);
  `src/input/editor/decorations.rs:13` (`RangeDecorationStyle`, with `Fill`
  and `Frame`) and `:72` (`TextDecoration`, a byte range and a
  `HighlightStyle`); `src/input/base/inline_tokens.rs:12` (`InlineToken`);
  `src/input/editor/display_map/folding.rs:3` (`FoldRange`);
  `src/input/editor/display_map/display_map.rs:29` (`DisplayMap`, holding only
  `wrap_map` and `fold_map`).
- Dataview 0.5.70 (MIT, released 2025-04-07): `src/index.ts` on its default
  branch (parser exports at `:29-38`, `import "obsidian"` at `:46`) and
  `src/query/engine.ts` (imports at `:4-13`; `executeList` at `:287`,
  `executeTable` at `:329`, `executeTask` at `:388` and `executeCalendar` at
  `:471`). Datacore: `README.md:3` in
  [blacksmithgu/datacore](https://github.com/blacksmithgu/datacore).
- The Calendar plugin: the README of liamcain/obsidian-calendar-plugin, read
  with `gh api`.

## Probe runs

- The Canvas 2D run: [graph-probe-canvas.html](graph-probe-canvas.html),
  [servo-default-canvas.json](servo-default-canvas.json),
  [servo-default-canvas.png](servo-default-canvas.png) and
  [servo-default-canvas.err](servo-default-canvas.err), with the Chromium
  control in [chromium-canvas.json](chromium-canvas.json). The
  [manifest](manifest.json) records them under `runs` and `canvasControl`.
- The WebGL extension requests:
  [chromium-v3-extensions.json](chromium-v3-extensions.json).
- The probe build's time and the Files API branch facts: `probeBuild` and
  `filesApiBranch` in the manifest.
