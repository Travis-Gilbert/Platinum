I've read the whole PR at its head commit (fb90fe5). I didn't run its tests, because that would mean installing its dependencies.

Every path below is relative to Travis-Gilbert/Theorem-UI at the PR's head commit, fb90fe5.

**One thing changes how to read the PR:** most of the Obsidian behaviour was already on `main`, merged in PR #10 on 2026-10-24. That covers wikilinks, backlinks, search, the quick switcher, properties, daily notes and canvas. PR #16 adds three things on top: the Vault view (layout, graph, outline, status bar), Theorem's own files as a second source, and an editor styling pass. Below, **new** means added by #16, **changed** means modified by it, and **main** means it was already there.

## 1. Metadata
- **Title:** "feat(vault): Obsidian-style vault over local and Theorem files, with an editor design pass". Still **open, not merged** when read on 2026-10-25. Update, 2026-10-26: merged at 03:29Z by merge commit `259f6d3`, with head fb90fe5. `main` ← `claude/vault-workspace-view`.
- **Dates and size:** opened 2026-10-25 01:01Z; commits run from 2026-10-24 06:05Z to 2026-10-25 01:16Z. +3,921/−159 across 66 files, 11 commits, one of which merges `main`.
- **Description:** one `WorkspaceClient` over a local folder or Theorem's Files API (SDK 0.5.0), a tree with Theorem marks, the Crepe editor, a Links pane with Theorem's link suggestions, and a cosmos.gl graph behind a same-origin proxy.
  - It claims `bun test` 479 pass, 0 fail across 69 files, and clean eslint and `tsc`.
  - Risks it names itself: the note styles also reach Theorem documents, the SDK moves from 0.4.0 to 0.5.0, nothing was checked under Servo, and nothing ran against a live Theorem backend.
- **Docs it references:**
  - `docs/plans/vault-view.md` (new): the Vault spec.
  - `docs/plans/vault-dynamic-documents.md`: D16 folder workspace, D12 sync, D13 handlers, D15 canvas.
  - `docs/desktop/workspace-routes.md`.
  - The vendored SDK's `fern/openapi.json`, which is the Files API contract.

## 2. Features
- **Vault layout (new):**
  - `src/components/vault/` holds `vault-workspace.tsx` (dockview: Files on the left, editor in the centre, Outline and Links as tabs over Graph on the right), `vault-panels.tsx`, `vault-state.ts` and `vault-phone.tsx`.
  - On a phone the Vault is only the file list.
  - The route is `src/routes/_chat/vault.tsx`; rail and mode wiring is in `src/components/shell/{modes,rail,chat-shell}.tsx`.
- **File tree (main, changed):** `src/components/workspace/workspace-tree.tsx` and `folders.tsx`. It uses headless-tree: keyboard, multi-select, F2 rename, drag and drop including Finder files, context menu. Theorem marks are in `entry-mark.tsx` (new).
- **Editor and live preview (main, changed):** `src/components/handlers/markdown/editor.tsx`.
  - Crepe is WYSIWYG all the time, and goes read-only with `setReadonly`.
  - The only source view is "Open with → Code editor" (`handlers/open-with.tsx`, `code/code-handler.tsx`).
  - The styling pass is in `src/styles.css`.
- **Wikilinks (main):** `markdown/syntax.ts`, `views.tsx` (a click resolves the link or creates the note), `wiki-suggest.tsx` (`[[`, `![[`, `#heading`, `#^block`, `|alias`), `link-target.ts`.
- **Backlinks, outgoing links, a note's tags (main, changed):** `src/components/workspace/links-pane.tsx`. New here: Theorem "Suggested links" (accept, reject or mute) and `related` links.
- **Tags pane:** there isn't one. All tags with counts appear in `search-dialog.tsx` while the query is empty.
- **Properties (main, changed):** `markdown/properties.tsx` and `frontmatter.ts`. Types are guessed from the value: text, list, number, checkbox, date.
- **Search (main):** `search-dialog.tsx`, on Mod+Shift+F, with `tag:#x` and nested tags.
- **Quick switcher (main, changed):** `quick-switcher.tsx`, on Mod+O, with fuzzysort; it creates the note if nothing matches.
- **Command palette:** none. Those two shortcuts are the only ones (`workspace-commands.tsx`).
- **Graph (new):** `src/components/vault/vault-graph.tsx` and `src/lib/vault/graph-model.ts`.
  - Present: Global and Local views, Local depth 1–5, and on/off chips for Tags, Attachments, Orphans and Unresolved.
  - Missing: colour groups, a display panel, a forces panel and a search filter. Colours are fixed per node kind.
- **Canvas (main):** `src/components/handlers/canvas/`, JSON Canvas 1.0 `.canvas` files.
- **Daily notes (main):** `src/lib/workspace/notes.ts` and `folder-settings.ts`, with a folder and a template filled with `{{date}}`, `{{time}}` and `{{title}}`.
  - There's no calendar. FullCalendar appears only in live-block query views.
  - The daily-note template is the only template feature.
- **Not present:** bookmarks, hover preview, unlinked mentions.
- **Outline (new):** in `vault-panels.tsx`. A click jumps to the heading; it doesn't track the heading you've scrolled to.
- **Tabs and splits (new):** dockview, with float and maximize; the layout is saved per browser.
- **Embeds (main):** `views.tsx` and `note-sections.ts`. `![[file]]` shows that file's card; `![[Note#H]]` and `#^id` show that section.
- **Other:**
  - Status bar (`vault-status-bar.tsx`), with word and character counts from `src/lib/vault/note-stats.ts`.
  - For Theorem plans and agents' documents: a read-only bar and "Make it mine" (`handlers/file-status.tsx`, `src/lib/workspace/theorem-actions.ts`), plus OKF bundle import and export.
  - Callouts, pasted and dropped attachments, saves that detect conflicts (`src/lib/handlers/file-sync.ts`).

## 3. Libraries
- **Editor:** Milkdown Crepe 7.22.3, built on ProseMirror, with `@prosemirror-adapter/react` and `prosemirror-autocomplete`. Code and source files use CodeMirror 6.
- **Markdown parsing:** Milkdown's remark pipeline plus `@flowershow/remark-wiki-link` 3.4.0 and the Obsidian callout extensions.
  - The index's parser is `src/server/desktop/workspace/markdown.ts` (mdast, gfm, frontmatter, yaml); inline `#tags` and `^ids` are hand-written.
  - Read-only excerpts use react-markdown.
- **Graph:** `@cosmos.gl/graph` 3.5.1, MIT licensed. Cosmograph was rejected because it is CC-BY-NC-4.0. It renders through luma.gl 9.3.6 (WebGL2); labels come from `@interacta/css-labels` 0.6.0.
  - **Data shape:** the server sends `{notes:[{path,title,tags,theorem?}], links:[{from,to|null,target}]}`.
    - The client builds nodes `{id,kind,label,path?,target?,degree}` and links as index pairs `[i,j]`.
    - Node kinds: note, plan, agent, tag, attachment, unresolved.
    - Everything goes to cosmos.gl as Float32Arrays: positions, links, sizes (4 for notes, 3 otherwise, plus 1.8·√degree) and colours.
  - **Simulation:** spaceSize 4096, gravity 0.2, repulsion 3, linkSpring 1, linkDistance 40, friction 0.85, decay 4000, `scalePointsOnZoom:false`, `enableDrag`.
    - Starting layout: linked notes in a disc, unlinked ones on a ring around it.
  - **Colours:** `--graph-*` CSS tokens for light and dark. The open note gets a ring.
  - **Labels:** they fade in past 1.4× the fitted zoom. Hovered nodes and the open note are always labelled.
  - **Interactions:** a click opens the file, runs a tag search, or creates a missing note. Hover lights up neighbours. The camera follows the layout until you zoom yourself.
  - **Settings:** zustand, saved in `localStorage` under `theorem-ui:vault`.
- **Rest of the stack:** canvas `@xyflow/react` 12.12.0; layout dockview-react 8.6.1; state zustand 5 plus TanStack Query; routing TanStack Router and Start.

## 4. Data and backend
There are two `WorkspaceClient`s (`src/lib/workspace/types.ts`), chosen in `workspaces.ts`.

- **Local folders (desktop app only):** `desktop-client.ts` calls the local Bun server under `/desktop/workspace/:id/`.
  - Files: `tree`; `file?path=` with GET, PUT (If-Match) and DELETE; `mkdir`; `rename`, which rewrites links.
  - Watching: `events` is a Server-Sent Events stream fed by `@parcel/watcher`.
  - Links: `links`, `backlinks`, `resolve`, and `graph` (new).
  - Search and metadata: `search?q=`, `tags`, `properties`, `settings`, `trust`.
  - Index: an embedded Rusty Red database (`@rustyreddb/node`, BM25 full text), in `index-db.ts`.
- **Theorem vault (also on the web once signed in):** `theorem-client.ts` calls SDK 0.5.0 `theorem().files`.
  - Requests go through the proxy at `src/routes/api/files/v1/$.ts`, and `src/server/theorem-proxy.ts` adds the bearer token.
  - Every request carries `schema=theorem-files/1`. Every write sends X-CSRF-Token, Idempotency-Key and X-Theorem-Client-Id.

| SDK method | Endpoint (all under `/api/files/v1`) |
|---|---|
| `vault`, `entries` | GET `/vault`, GET `/entries` |
| `read`, `write` | GET `/content`, PUT `/content` (write may come back `merged`) |
| `remove` | DELETE `/entries` (documents are archived, not deleted) |
| `mkdir`, `move` | POST `/directories`, POST `/moves` |
| `resolve`, `links`, `backlinks`, `graph`, `search`, `tags`, `properties` | GET, same names |
| `events` | GET `/events?after=&waitMs=25000` (long poll) |
| `suggestions`, `decide` | GET `/suggestions`, POST `/suggestions/{id}/decisions` |
| `promote`, `exportOkf`, `importOkf` | POST `/promotions`, POST `/okf/exports`, POST `/okf/imports` |

- **Loro:** not used for notes. Per-file Loro sync (D12) is still marked pending.
- **RustyRed:** the API spec says link suggestions come from "rustyred-thg-link". I couldn't confirm what Theorem's own index runs on.
- **Where the link graph is built:** on the server in both cases. The page only turns the server's graph into drawable nodes and edges, and re-parses the open note for its outline, tags, counts and `[[` suggestions.

## 5. What transfers
- **Could run as-is in a Servo web panel, given a data bridge:**
  - `graph-model.ts` is plain TypeScript, and about 150 lines if you'd rather port it to Rust.
  - The graph view in `vault-graph.tsx`, with the `--graph-*` and `.vault-graph` CSS.
  - The graph view calls app hooks and actions, so it needs a small shim from the host.
  - **WebGL2 in Servo is unverified.** Frame's `Cargo.toml` turns on Servo's `webgl` feature, but nothing shows cosmos.gl has ever run in Servo.
- **Reading view:** there is no separate renderer. The full-fidelity option is Crepe with `setReadonly(true)`, `syntax.ts`/`views.tsx` and the `.theorem-note` CSS. The lighter `NoteExcerpt` doesn't render wikilinks or callouts.
- **Reusable as specs:** the `WorkspaceClient` types, the Files API `openapi.json`, and the link resolution and rewrite rules (`workspace-routes.md`, `resolve.ts`, `rewrite.ts`).
- **Precedent:** Theorem-UI's own plan, in the Architecture section of `vault-dynamic-documents.md`, already runs this app inside Servo in a GPUI host, with the Bun server as a child process.
- **Must be rewritten in Rust and GPUI Kit:**
  - The dock layout, tree, switcher and search dialogs, Links and Outline panes, status bar, properties editor and shortcuts.
  - A Files API client.
  - The folder service too, unless Frame runs the Bun server, which loads native addons.
  - A native WYSIWYG editor has no drop-in replacement; the Milkdown stack only carries over as web content.
- **Build:** Vite 8 with TanStack Start and Nitro 3 (a Bun target, plus a Cloudflare Worker target that leaves out the desktop routes), Tailwind 4, and Bun to run and test.
  - The Vault loads lazily, and there is no standalone graph bundle, so a Frame panel would need its own Vite entry.

## 6. Gaps, bugs, tests
- **The spec's own "Not yet" list:** the outline doesn't track the current heading, and phones get only the file list.
- **Untested, by the PR's own account:** Servo (both WebGL2 and the `:is()`/`:not()` heading CSS) and a live Theorem backend.
- **Cloudflare build:** it failed on every push (1acba5e, ed64f61, f0f645c, fb90fe5), and again on the merge commit `259f6d3` on `main` (2026-10-26). The reason is only on Cloudflare's dashboard, which I couldn't see.
- **Review:** the Greptile code-review bot raised six issues. All six were fixed in f0f645c and fb90fe5, its final score was 5/5, and nothing has been pushed since.
  - P1: a merged save could lose the unsaved edits.
  - P2s: clicking an unresolved node made `Missing.md.md`; an outage hid the vault; read-only files could be edited through the code editor; `[[a/Note]]` and `[[b/Note]]` collapsed into one node; a `C# basics` heading couldn't be reached.
- **Stale docs:** `workspace-routes.md` doesn't list the new `graph` route. `vault-view.md` still says the Vault only appears where folders can open, but it now also appears on the web for signed-in users.
- **Tests added:**
  - `graph-model.test.ts` (5) and `note-stats.test.ts` (2).
  - One graph-route test in `service.test.ts`.
  - Changes to `links-pane`, `workspace-file-panel`, `file-sync` and `markdown-handler` tests.
- **Tests missing:** `vault-graph.tsx`, `vault-workspace.tsx`, `vault-status-bar.tsx` and `theorem-client.ts` have none. The 479-pass figure is the PR's claim, not something I checked.
