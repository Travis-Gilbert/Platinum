Gathered 2026-10-25 with Firecrawl. Help now lives at obsidian.md/help; the plugin directory at community.obsidian.md. UNCONFIRMED = not on a primary page. I wrote no files (the harness auto-saved two oversized tool results; I only read them).

## 1. Version and 2026 features
Source: https://obsidian.md/changelog (entries such as /changelog/2026-10-05-desktop-v1.14.4/)
- Current public desktop: 1.14.5, 2026-10-22 (Electron security update; reinstall). Latest early access (Catalyst): 1.15.0, 2026-10-22, not public.
- Public releases: 1.11.4 (01-12), 1.11.5, 1.11.7, 1.12.4 (02-27), 1.12.7, 1.13.4 (07-30), 1.13.6, 1.13.7, 1.14.4 (10-05), 1.14.5.
- 1.11: pasting a URL over selected text makes a Markdown link; Markdown links in text/list properties, auto-updated on rename; Keychain/secret storage for plugins (OS-encrypted in 1.11.5); "Default file to open" setting; plugin update checks every 3 days.
- 1.12: Obsidian CLI (native binary in 1.12.7); drag-resize images in Live Preview; automatic attachment cleanup on delete; Canvas links count in Backlinks and Graph; Bases search, drag-drop import, row menu. Sync 2026.02.27 added headless Sync.
- 1.13: rebuilt searchable Settings window; fullscreen image viewer with keyboard editing; Note composer rewrites links on extract; Bookmarks search; Mermaid off by default; URI approval dialog added, removed again in 1.13.6; `--callout-color` accepts any CSS color (breaking).
- 1.14: open single Markdown files from outside a vault (Open With, macOS Quick Look); Bases Kanban and Group menu; six highlight colors; fuzzy-search rework with full-path matching; right-to-left workspace flip; font-size commands; MathJax 4.1.3 replaces MathJax 3; Format converter's old formats removed.
- 1.15.0 early access: Mermaid rendered by Obsidian's new "Pele" library (Mermaid 12.1 syntax, theme-aware, zoom/pan, on by default); Bases filter drag-reorder; URI/CLI append under a heading; Vim additions.

## 2. Core plugins
Source: https://obsidian.md/help/plugins

Audio recorder: record to a note. Backlinks: linked/unlinked mentions. Bases: database views over properties. Bookmarks: notes, headings, searches. Canvas: infinite board. Command palette. Daily notes. File explorer (help title; the 1.15 notes still say "Files"; no rename documented). File recovery: snapshots. Footnotes view: sidebar list. Graph view. Note composer: merge/extract headings. Outgoing links: links plus unlinked mentions. Outline. Page preview: hover preview. Properties view. Publish (paid). Quick switcher. Random note. Search. Slash commands. Slides. Sync (paid). Tags view. Templates. Unique note creator: timestamped notes. Web viewer: in-app browser tabs, desktop only. Word count. Workspaces: saved layouts.
- Added: Web viewer (1.8.0 early access 2024-12-18, off by default), Bases (1.9), Footnotes view (1.9.0 early access 2025-05-21; public 1.9.10).
- Renamed: Zettelkasten prefixer to Unique note creator (forum post only; version UNCONFIRMED).
- De-listed: Format converter still has a help page (migrates alias/tag/cssclass) but is absent from the core list; 1.14 removed its old formats.
- Map layout needs the official Maps plugin; Importer is also an official community plugin. No new core plugin appears in the 1.11 to 1.14 public notes.

## 3. Editor and markdown
Sources: https://obsidian.md/help/obsidian-flavored-markdown, /help/links, /help/embeds, /help/callouts, /help/properties, /help/tags, /help/syntax
- Flavor: CommonMark + GFM + LaTeX + Obsidian extensions; no Markdown inside HTML. Modes: Reading, and editing in Live Preview (default) or Source; Ctrl/Cmd+E toggles.
- Links: `[[Note]]`, `[[Note#Heading]]` (nested `#H1#H2`), `[[Note#^block-id]]`, `[[Note|alias]]`, or Markdown links; `[[##` and `[[^^` search headings/blocks vault-wide. Settings: auto-update links on rename/move, wikilinks vs Markdown, shortest/relative/absolute paths. A link to a missing note creates it on click at that path; Graph hides these under "Existing files only"; their styling is UNCONFIRMED.
- Embeds: `![[note]]`, `#heading`, `#^block`; images `![[img.png|100x145]]`, external `![250](url)`; PDF `#page=3&height=400`; audio, video, `.base`, `.canvas`; search results via a `query` code block. Image formats: avif bmp gif jpeg jpg png svg webp.
- Callouts: `> [!type] Title`; `+`/`-` fold; nestable. Types: note, abstract, info, todo, tip, success, question, warning, failure, danger, bug, example, quote (plus aliases such as tldr, hint, faq); unknown types render as note; custom via CSS.
- Properties: YAML frontmatter. Types: Text, List, Number, Checkbox, Date, Date & time, Tags. Defaults `tags`, `aliases`, `cssclasses` (singular forms unsupported since 1.9). Type is global per property name; no nested-property UI or bulk edit.
- Tags: `#a/b` nests (`tag:a` matches children); letters, digits, `_ - /`, Unicode; needs one non-digit; case-insensitive.
- Syntax: `==highlight==` (six colors), `~~strike~~`, task lists with any character in `[ ]`, footnotes `[^1]` and inline `^[...]` (docs: inline is Reading view only), `%% comments %%`, Prism code highlighting in Reading view, Live Preview table editor (add/delete/sort/move/align), MathJax `$$`/`$`, `mermaid` fences.
- Editing: folding of headings and lists; multiple cursors (Alt/Option+click; Shift+Alt+drag rectangular); Vim key bindings; spellcheck (macOS native, Windows/Linux language picker); drag and drop (HTML becomes Markdown, files import); URL pasted over selection becomes a link, Ctrl/Cmd+Shift+V pastes raw; smart lists, auto-pair, strict line breaks.

## 4. Graph view
Source: https://obsidian.md/help/plugins/graph. Local-graph toggles come from forum user posts (https://forum.obsidian.md/t/local-graph-what-are-neighbor-links/6954), not docs.
- Cog opens settings; "Restore default settings" resets.
- Filters: Search files, Tags, Attachments, Existing files only, Orphans.
- Groups: "New group" = search query plus color.
- Display: Arrows, Text fade threshold, Node size, Link thickness, Animate (replays creation order).
- Forces: Center force, Repel force, Link force, Link distance.
- Slider ranges and defaults: UNCONFIRMED (docs give meanings only).
- Local graph ("Open local graph" command) follows the active note, takes the same settings, and adds a Depth slider (each level adds notes connected to the previous level). Forum posts show Incoming links, Outgoing links and Neighbor links toggles (Neighbor off hides links among neighbors).

## 5. JSON Canvas 1.0 and Canvas UI
Sources: https://jsoncanvas.org/spec/1.0/ (2024-03-11), https://obsidian.md/help/plugins/canvas, https://obsidian.md/canvas
- Top level: optional `nodes[]`, `edges[]`; array order is z-order (first lowest).
- All nodes: `id`, `type` (text|file|link|group), `x`, `y`, `width`, `height` (integers), optional `color`. text: `text`. file: `file`, optional `subpath` (starts `#`). link: `url`. group: optional `label`, `background`, `backgroundStyle` (cover|ratio|repeat).
- Edge: `id`, `fromNode`, `toNode`, optional `fromSide`/`toSide` (top|right|bottom|left), `fromEnd` (none|arrow, default none), `toEnd` (default arrow), `color`, `label`.
- Color: hex string or preset "1" red, "2" orange, "3" yellow, "4" green, "5" cyan, "6" purple; exact hex left to the app.
- UI: cards from text, notes, media, web pages (pasted URL, YouTube), other canvases, bases, whole folders; "Convert to file…", narrow to heading/block, swap file. Edges: labels, color, direction, connect to groups, "Follow connection". Groups: around selection, nest, rename, background image (Cover/Keep aspect ratio/Repeat), "Jump to group". Pan with Space-drag/middle-drag; Ctrl/Cmd+scroll zoom; Zoom to fit Shift+1; Zoom to selection Shift+2. Snap to grid/objects, Alt-drag duplicate, align/distribute/arrange, PNG export, read-only mode, canvas embeds in notes.

## 6. Bases
Sources: https://obsidian.md/help/bases, /help/bases/syntax, /help/bases/views, /help/bases/functions
- Publicly documented: yes; "Bases syntax" is the file-format reference. `.base` is YAML, also embeddable in a code block or via `![[File.base#View]]`.
- Top-level keys: `filters`, `formulas`, `properties`, `summaries`, `views`.
- Views (app version): table 1.9, cards 1.9, list 1.10, map 1.10 (needs the official Maps plugin), kanban 1.14. View YAML keys: `type`, `name`, `filters`, `groupBy`, `groupOrder`, `order` (columns), `summaries`, `limit`; the UI also has multi-property sort. Settings: table row height; cards size and image property/fit/aspect; list markers, indent, separator; map coordinates, icon, color, tiles; kanban column width.
- Filters: and/or/not trees, global plus per-view joined by AND; default scope is the whole vault; functions like `file.hasTag()`, `file.inFolder()`, `file.hasLink()`.
- Formulas: strings under `formulas`; read `note.*`, `file.*`, `formula.*`, `this`; JS-like operators, date and duration arithmetic. Functions: global (`if`, `now`, `link`, `list`, `image`…) plus Any, Date, String, Number, List, Link, URL, File, Object, RegExp groups.
- Summaries: built-ins by type (Empty/Filled/Unique; Average/Min/Max/Median/Range/Stddev/Sum; Earliest/Latest; Checked/Unchecked) and custom formulas over `values`.

## 7. Workspace
Sources: https://obsidian.md/help/tabs, /help/sidebar, /help/ribbon, /help/status-bar, /help/pop-out-windows, /help/plugins/workspaces
- Tabs: Ctrl/Cmd+T; every tab sits in a tab group. Link modifiers: Ctrl/Cmd new tab, +Alt split, +Alt+Shift new window. Drag to split or move; pinned tabs open links elsewhere; "Stack notes" stacks a group.
- Linked panes: docs say "Link with tab…" or "Open linked view" (Backlinks, Outline, local graph follow a chosen note); "Link with pane" is not in current docs.
- Pop-out windows: desktop only, move tabs between them.
- Sidebars: left and right, with tab groups and pinnable tabs. Ribbon (left): vault switcher, help, settings, plugin actions; reorder, hide, or hide entirely. Status bar (bottom right): plugin items such as backlink count, editor mode, word count, Sync status.
- Workspaces plugin saves/loads/deletes named layouts (tabs, sidebar widths, visibility).
- Page preview (core) is read-only hover. Hover Editor is a community plugin, not core.

## 8. Community plugins
Sources: https://community.obsidian.md/ (9,477 plugins; obsidian.md/plugins redirects there) and https://raw.githubusercontent.com/obsidianmd/obsidian-releases/master/community-plugin-stats.json (fetched 2026-10-25). Plugin pages show "Downloads" (Dataview 5.2M, matching the file), but the directory list is client-rendered, so this ranking sorts the stats file by cumulative downloads (not unique installs).

1 obsidian-excalidraw-plugin 8,710,895; 2 templater-obsidian 6,017,669; 3 dataview 5,204,016; 4 obsidian-tasks-plugin 4,502,158; 5 obsidian-git 3,353,680; 6 table-editor-obsidian (Advanced Tables) 3,311,919; 7 calendar 3,220,515; 8 copilot 2,921,541; 9 obsidian-style-settings 2,799,461; 10 obsidian-kanban 2,782,343.

Excalidraw, Templater, Dataview, Tasks and Calendar are confirmed top 10. Homepage is not: #21 with 1,400,565. Also: Importer #19, Hover Editor (`obsidian-hover-editor`) #43 with 624,308, Maps #273. The directory home "Popular" row shows no counts and uses an undocumented metric.

Unconfirmed overall: graph slider ranges and defaults, unresolved-link styling, the version of the Unique note creator rename, a Files-to-File-explorer rename, and Format converter's current in-app status.
