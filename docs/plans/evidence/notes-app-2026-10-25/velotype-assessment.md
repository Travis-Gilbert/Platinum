I recommend using Velotype as a reference, not forking it as the base for the editor or the file browser. It runs on the exact same GPUI as Frame, so there's nothing to port, but its document model is the wrong way round for a Loro text authority. I read it at manyougz/velotype@07a24a9b5b953539dede8e61a1f7d9d0adbba590. Velotype citations are relative to that repository's root; `Frame/` means this repository. I ran no build.

**1. License and activity**
- **License:** `Cargo.toml:6` says `license = "Apache-2.0"`. The only license file is `LICENSE-APACHE`: line 1 is "Copyright 2025 - 2026 manyougz", then the full Apache 2.0 text (`:22`–`:222`). There is no appendix and no NOTICE file. GitHub's API labels it "Other", probably because of the copyright line added on top; the text itself is standard Apache 2.0, so a fork is allowed.
- **Activity:** 121 commits (97 non-merge), first on 2026-05-09, the pinned commit on 2026-10-16. `main` has one later commit, a CI fix. Commits per month from May to October: 59, 34, 9, 6, 2, 11.
- **Authorship:** 12 contributors. manyougz wrote 74 commits (61%), kzadorozhny 20, ctfvoge 15, everyone else 2 or fewer.
- **GitHub (via `gh`):** 611 stars, 60 forks, 83 issues (17 open), 47 PRs.
- **Tests:** about 830, but CI only runs `cargo test updater` (`.github/workflows/build-release.yml:96`).

**2. Dependencies**
- **GPUI:** crates.io `gpui-pre =0.3.9` and `gpui-pre-platform =0.3.9` (`Cargo.toml:60-61`). No zed git rev (the lockfile has no git sources), no gpui-component, no gpui-kit.
- **Markdown parser:** hand-written. Blocks are parsed line by line (`src/editor/document.rs:988-1011`), inline syntax in `src/components/markdown/inline.rs`. `pulldown-cmark 0.13` is only used for HTML export and HTML blocks (`src/export/html.rs:8`).
- **Text storage:** no rope and no sum_tree. Each block holds a `Vec<InlineFragment>` of `String`s (`inline.rs:516-519`, `:123`).
- **Other:**
  - tree-sitter 0.27.1 with 17 grammars plus YAML and TOML (`Cargo.toml:86-105`)
  - RaTeX for math, drawn as SVG (`:80-82`)
  - mermaid-rs-renderer 0.3 (`:83`)
  - reqwest 0.13 (blocking) for remote images (`:84`)
  - PDF export by driving a local Chromium through chromiumoxide (`:74`, `src/export/pdf.rs:1-5`)
  - a self-updater

**3. Architecture** (about 72.6k lines: roughly 47k editor engine, 22k app shell)
- **App shell:** `main.rs` sets up the app-wide globals (`:237-244`) and opens one OS window per file (`:305`). `app_menu.rs:62-85` makes `Editor` the window's root view.
- **`editor/`:** `Editor` is one large object (`mod.rs:72-214`) that owns the block tree, save/close flow, dialogs, menu hover state, the file panel, the status bar and undo. Supporting files: `tree.rs` (`DocumentTree` `:61`, serializer `markdown_text` `:159-163`), `events.rs` (structural edits), plus history, selection, source_mapping, workspace, persistence and render.
- **`components/`:** one GPUI entity per block (`block/runtime/mod.rs:104`) holding its kind, text, table and HTML data (`state.rs:517-534`), with its own focus, selection and IME range (`runtime/mod.rs:130,136`). Custom text elements live in `element.rs:686,938`, Typora-style reveal of `**`/`_` markers in `runtime/projection.rs`, and parsers in `markdown/`, `latex/` and `mermaid/`.
- **Document model:** the block tree is the source of truth. Markdown syntax is absorbed into styles on import and rewritten in a canonical form on save: a test turns `_a_ __b__` into `*a* **b**` (`inline.rs:3444-3449`). Keystrokes edit the visible text of one block (`input.rs:88-122`). Structural changes go to `Editor` as events (`state.rs:787`), and the plain "changed" event carries no range (`state.rs:795`).
- **Undo/redo:** snapshots of the whole document as text, 200 entries, 1-second coalescing (`mod.rs:315-324,408-410`). Every edit re-serializes the whole document (`events.rs:1873-1900` → `history.rs:108-145`). Undo re-parses everything and replaces every block (`history.rs:291-315`).
- **Rendering:** WYSIWYG blocks, or a source mode where the whole document is one raw block (`window_state.rs:344-383`). There is no side-by-side live preview. Rendered rows are virtualized (`window_state.rs:77`).
- **Selection, IME, clipboard:**
  - per-block UTF-8 selection with a UTF-16 bridge for IME (`input.rs`, `text_offsets.rs`)
  - selection across blocks (`mod.rs:333-356`)
  - pasted images are copied to an assets folder or uploaded to PicGo (`events.rs:386-401`)
- **File I/O:** direct `std::fs` calls (`persistence.rs:101,174`, `file_drop.rs:81`).
- **File tree:**
  - the root is the current file's parent folder (`workspace.rs:138-141`)
  - a synchronous recursive scan of `.md` files only (`:602-645`)
  - no file watching, no create/rename/delete, not virtualized
  - opening a file replaces the current document (`:257-260`)
- **Tabs:** none.
- **Settings and theme:** preferences live in `config/preferences.rs:156,217-224` and open in their own window (`:2789`). The theme is a `ThemeManager` global (`theme.rs:1724`) with about 220 tokens.

**4. Features**
- **Present:**
  - tables, with native editing
  - task lists
  - strikethrough
  - footnotes, with jump to definition
  - highlighted code blocks
  - inline and block math
  - Mermaid
  - outline
  - HTML and PDF export
- **Partial:**
  - **CommonMark:** hand-written parser that falls back to raw blocks for anything it can't handle; I found no spec test suite.
  - **Autolinks:** only `<…>` (`link.rs:139`); no bare-URL GFM autolinks.
  - **Images:** only on their own line or in table cells (`image.rs:156,55`).
  - **Callouts:** only GitHub's five types (`state.rs:20-31`).
  - **IME:** the README still lists it as open work (`README.md:110`).
  - **Large files:** rendering is virtualized, but every edit costs time proportional to the whole document. Source mode lays out the entire file as one element (`element.rs:1030-1047`). I didn't measure this.
- **Absent** (none of these appear in the action list `actions.rs:14-75` or the parser):
  - heading folding
  - embeds
  - wikilinks, aliases, heading/block refs
  - #tags
  - YAML frontmatter
  - find/replace
  - multiple cursors
  - vim mode
  - spellcheck

**5. Extraction**
It is binary-only: `cargo metadata` shows just the `[[bin]]` target (`Cargo.toml:200-203`), and every module is private in `main.rs:21-37`.

To mount it in a GPUI Kit DockArea panel:
1. Add a `lib.rs` and split `Editor` into the editing core and the window shell.
2. Remove the titlebar, menu bar, status bar, file panel, dialogs and app-level actions from `render.rs:2145-2251`.
3. Replace the 9 `window.viewport_size()` layout reads with the panel's own bounds.
4. Set up its globals (ThemeManager, I18nManager, FontSettings, EditorSettings, ImageRetries) without its menus or updater. Skip `cx.set_http_client` (`net/mod.rs:29`), which would replace Frame's HTTP client.
5. Wrap the core in a view that implements `Panel` (`Frame/vendor/gpui-component/src/dock/panel.rs:71`).

To read and write through the Loro document authority:
1. **Input:** `from_markdown(cx, String, Option<PathBuf>)` (`mod.rs:416`). Replace `file_path` (`mod.rs:109`) with a handle that gives the base folder for images and pasted assets.
2. **Outbound edits:** the full new text already exists after each edit, at `history.rs:108`. Pass it to `LoroText::update` (`loro-1.13.9/src/lib.rs:2583`), which computes the diff itself. Real per-edit insert/delete ops would need new plumbing through the offset mapping (`mod.rs:369-380`).
3. **Inbound remote changes:** there is no incremental path. The only option today is a full re-parse and `replace_roots` (`tree.rs:153`), which throws away every block's focus and IME state. It needs a re-parse limited to the blocks the change touches.
4. **Undo:** must be replaced by Loro's `UndoManager` (`lib.rs:3856`). Restoring whole-document snapshots would also undo collaborators' edits.
5. **Saving:** remove the `std::fs` saves, dirty tracking and close guard (`persistence.rs:94-234`, `close.rs`).
6. **Hardest:** untouched blocks are re-serialized in canonical form. The first local edit pushes a normalization diff for the whole document into the CRDT, and later edits keep rewriting other authors' syntax. Fixing it means keeping each block's original source, which only opaque blocks do today (`raw_fallback`). That work runs through `events.rs` (about 5k lines), `tree.rs` and `inline.rs`.

**6. Compatibility with Frame**
- **GPUI:** same as Frame (`Frame/Cargo.toml:64`; Frame's vendored copy is a snapshot of zed@93d7456). Zero GPUI call sites need porting. Frame's patches touch surfaces, which Velotype never uses.
- **Look and feel:** it uses its own theme (about 220 tokens, about 870 reads) and its own widgets. Matching GPUI Kit is a restyle, not a port.
- **tree-sitter conflict:** 0.27.1 declares `links = "tree-sitter"`, and so does gpui-component's optional 0.26.13 (`Frame/vendor/gpui-component/Cargo.toml:420-422`). Cargo won't allow both in one build. It's fine today because Frame's lockfile has no tree-sitter at all, but turning on GPUI Kit's highlighter later would force both onto one version.
- **To drop:** chromiumoxide, the updater stack and `gpui-pre-platform` (only used in `main.rs:216`).

**7. Overlap with Frame's gpui-component**
- **What Frame already has:**
  - GPUI Kit's Input (from gpui-base) gives a ropey buffer, search/replace, folding, LSP-style hooks and tree-sitter highlighting. That last feature is currently switched off in Frame's build.
  - TextView renders markdown-rs read-only, with a frontmatter plugin (`text/frontmatter.rs`) and hooks for custom nodes (`text/compat.rs:179`).
- **What Velotype adds:**
  - editing directly in the rendered view
  - a table editing UI
  - callouts, footnote navigation, task toggles
  - math and Mermaid rendering
  - image paste, drop and upload
  - safe HTML blocks
  - export
- **What it lacks** that those already have: a rope, find/replace, folding.

**8. Verdict: use as reference; don't fork as base.**
Reasons:
1. **Wrong source of truth.** Obsidian parity and a Loro text CRDT both want the Markdown text to be the truth, with the rendered view as decoration (Obsidian's Live Preview model). Velotype does the opposite, like Typora: the block tree is the truth and Markdown is a canonical export. Flipping that is the deepest change in the codebase.
2. **The parity features would mean extending its hand-rolled parser.** Wikilinks, embeds, tags, frontmatter, folding and find/replace are all missing. Adding them means working in about 8k lines of hand-written parser that would drift from markdown-rs, which Frame already has. That cuts against your don't-hand-roll rule.
3. **Tight coupling.** The editor is the window root, tied to its own globals, dialogs and file I/O.
4. **Maintenance risk.** One main author, and commit activity has dropped off since June.
5. **File browser: no.** It only scans the current file's folder, synchronously, `.md` files only. GPUI Kit's `Tree` (`Frame/vendor/gpui-component/src/tree.rs:32`) is a better base.

**Worth borrowing:**
- the marker-reveal logic (`runtime/projection.rs`)
- table editing (`markdown/table.rs`, `editor/table_edit.rs`)
- the RaTeX and Mermaid SVG-cache approach (`latex/mod.rs:90-146`)
- the IME UTF-16 bridge
- the height cache used for virtualization

**What I'd build on instead:** GPUI Kit's rope-backed Input as the source buffer, synced to `LoroText`, with rendering decorations driven by markdown-rs.
