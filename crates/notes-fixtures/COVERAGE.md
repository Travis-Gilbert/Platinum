# Fixture coverage

Every row of the parity matrix in `docs/plans/notes-app.md` (the tables under "Parity matrix", including the ten community plugins), the fixture files that exercise it, and the reference answers that record what Obsidian 1.14.5 does with them. `tests/coverage.rs` fails if a row of the plan is missing here, if a row is here that the plan lacks, if a cited file does not exist, or if a note, canvas or base in the vault is cited nowhere.

Paths are relative to `vault/`, except `queries.json`, which is in the crate. A path ending in `/` is a folder. The vault is checked for the constructs the rows name by `tests/vault.rs`.

## How to read the tables

The third column says where Obsidian's answer for the row comes from once the reference run has been made against this vault. The names are the groups of `notes-fixtures reference` (`vault`, `links`, `outline`, `properties`, `tags`, `tasks`, `search`, `bases`, `wordcount`, `eval`, `dom`), plus two more:

- `ui`: nothing the CLI can return. The fixture is the input to a side-by-side comparison in the running app, by screenshot or by hand.
- `lint`: only `tests/vault.rs` looks at it.

The fourth column is what a row does not cover even with its fixtures. A row with no fixture says `none` and appears again under "Rows no fixture can exercise", with the reason.

## Parity matrix

### Vault and files

| Feature | Fixture files | Reference answers | Not covered |
| --- | --- | --- | --- |
| Vaults | `Home.md`, `.obsidian/app.json` | vault | vault switcher and one window per vault need a second vault and the app's windows |
| File explorer | `a/`, `b/`, `daily/`, `projects/`, `.obsidian/workspaces.json` | vault, ui | create, rename, move, delete, drag and drop, multi-select and sort orders are actions on the tree, not content |
| Coloured folders | `.obsidian/snippets/folder-colours.css`, `projects/`, `daily/`, `review/` | vault, ui | - |
| Locations | `inbox/Inbox.md`, `attachments/`, `.obsidian/app.json`, `review/Missing Note Creation.md` | vault, links | where a new note or pasted attachment lands is seen only after creating one |
| Links follow renames | `links/Link Forms.md`, `links/Markdown Links.md`, `a/Links From A.md`, `properties/Link Properties.md`, `.obsidian/app.json` | links, properties | the rename is an action and the harness only reads; rename in a copy and rerun to compare the rewritten links |
| Link format | `links/Link Forms.md`, `links/Markdown Links.md`, `a/Links From A.md`, `b/Links From B.md`, `.obsidian/app.json` | links, eval | the setting decides what an inserted link looks like, which only the editor shows |
| Excluded files | `excluded/Hidden Note.md`, `search/Search Corpus.md`, `.obsidian/app.json` | search, links, vault | quick switcher and link suggestion ranking are UI |
| Unused attachments (1.12) | `attachments/unused.png`, `embeds/Embed Images.md`, `embeds/Embed Media.md` | vault, links | the offer to delete with the note is UI |
| File types | `canvases/`, `bases/`, `attachments/`, `embeds/Embed Images.md`, `embeds/Embed Media.md`, `syntax/Line Endings.md`, `syntax/Byte Order Mark.md` | vault, dom | - |
| Single files (1.14) | none | - | no fixture: see below |
| File recovery | none | - | no fixture: see below |
| Stable ids | none | - | no fixture: see below |

### Markdown and editing

| Feature | Fixture files | Reference answers | Not covered |
| --- | --- | --- | --- |
| Modes | `.obsidian/workspaces.json`, `.obsidian/app.json`, `Home.md`, `syntax/Inline Formatting.md` | dom, ui | Live Preview and Source mode are editor states; only the reading view's DOM is recorded |
| Internal links | `links/Link Forms.md`, `links/Headings Target.md`, `links/Blocks Target.md`, `links/Aliases.md`, `links/Markdown Links.md`, `links/Unresolved Links.md`, `review/Heading With Hash.md`, `review/Missing Note Creation.md`, `a/Note.md`, `b/Note.md` | links, outline, eval, dom | the `[[##` and `[[^^` suggestion lists are editor UI; Link Forms.md holds both trigger lines |
| Embeds | `embeds/Embed Notes.md`, `embeds/Embed Images.md`, `embeds/Embed Media.md`, `embeds/Embed Canvas and Base.md`, `embeds/Loop A.md`, `embeds/Loop B.md`, `attachments/` | dom, links | - |
| Callouts | `callouts/All Callouts.md`, `callouts/Callout Titles and Folding.md`, `callouts/Nested Callouts.md`, `callouts/Custom Callout.md`, `.obsidian/snippets/callouts.css` | dom | - |
| Properties | `properties/All Property Types.md`, `properties/Aliases and Cssclasses.md`, `properties/Link Properties.md`, `properties/Type Mismatch.md`, `properties/Broken Frontmatter.md`, `properties/Empty Frontmatter.md`, `properties/Frontmatter Only.md`, `syntax/Byte Order Mark.md`, `syntax/Line Endings.md`, `.obsidian/types.json` | properties, eval, dom | links in properties updating on rename is an action |
| Tags | `tagging/Tags.md`, `tagging/Unicode Tags.md`, `tagging/Frontmatter Tags.md`, `tagging/Tags As Text.md` | tags, eval | - |
| Highlights | `syntax/Comments and Highlights.md` | dom | the swatch in Live Preview is editor UI; the six colour emoji are written from memory of the 1.14 notes and unverified |
| Inline syntax | `syntax/Footnotes.md`, `syntax/Comments and Highlights.md`, `syntax/Math.md`, `syntax/Raw HTML.md`, `syntax/Inline Formatting.md` | dom, eval | - |
| Tasks | `tasks/Task Statuses.md`, `syntax/Line Endings.md`, `daily/2026-10-25.md`, `bases/Tasks.base` | tasks, search, dom | toggling a task writes the file, an action |
| Code blocks | `syntax/Code Blocks.md` | dom | - |
| Tables | `syntax/Tables.md` | dom | the Live Preview table editor (add, delete, move, sort, align) is UI |
| Mermaid | `syntax/Mermaid.md` | dom | rendering is off by default since 1.13, so the reference DOM holds the unrendered fence unless the setting is turned on in the app first |
| Folding | `syntax/Folding and Lists.md`, `links/Headings Target.md`, `.obsidian/app.json` | outline, ui | fold state is UI |
| Multiple cursors | none | - | no fixture: see below |
| Vim | none | - | no fixture: see below |
| Spellcheck | none | - | no fixture: see below |
| Paste and drop | none | - | no fixture: see below |
| Editor settings | `.obsidian/app.json`, `syntax/Inline Formatting.md`, `syntax/Folding and Lists.md` | dom, ui | smart lists, auto-pair, indent guides and font size commands are editing behaviour; strict line breaks changes the reading DOM and is the only setting here with an answer |
| Images | `embeds/Embed Images.md`, `attachments/wide.png`, `attachments/Pasted image 20261024101500.png` | dom | drag-to-resize and the full-screen viewer are UI |
| Find and replace | `search/Case Sensitivity.md`, `search/Search Corpus.md` | ui | the find bar is editor UI; these notes only give it text to match |
| Slash commands | none | - | no fixture: see below |
| Right to left | `syntax/Bidirectional Text.md` | dom | the workspace flip is a 1.14 setting of the app, not content |

### Reading view

| Feature | Fixture files | Reference answers | Not covered |
| --- | --- | --- | --- |
| Rendering | `Home.md`, `syntax/`, `callouts/`, `embeds/`, `tagging/Tags.md`, `Dead End.md` | dom | - |
| Website look | `.obsidian/appearance.json`, `Home.md`, `.obsidian/snippets/callouts.css` | ui | the look depends on the theme and is compared by screenshot, not by DOM |
| Links | `links/Link Forms.md`, `links/Unresolved Links.md`, `review/Missing Note Creation.md`, `a/Links From A.md` | dom, links | clicks and modifier-clicks are interaction |
| Interaction | `callouts/Callout Titles and Folding.md`, `tasks/Task Statuses.md`, `syntax/Footnotes.md`, `syntax/Code Blocks.md` | dom, ui | folding, toggling, jumping and copying are interaction; the DOM shows the markup they act on |
| Page preview | `links/Link Forms.md`, `links/Headings Target.md`, `links/Blocks Target.md` | ui | the hover popover is UI |
| CSS snippets, `cssclasses` | `.obsidian/snippets/callouts.css`, `.obsidian/snippets/folder-colours.css`, `properties/Aliases and Cssclasses.md`, `Home.md` | dom, vault | - |
| Updates | none | - | no fixture: see below |
| Export to PDF | none | - | no fixture: see below |

### Navigation and panes

| Feature | Fixture files | Reference answers | Not covered |
| --- | --- | --- | --- |
| Backlinks | `links/Mentions.md`, `a/Note.md`, `b/Note.md`, `Home.md`, `canvases/Board.canvas` | links | unlinked mentions are not in the CLI (`backlinks` is documented as linked only); collapse, sort and more context are UI |
| Outgoing links | `Home.md`, `links/Link Forms.md`, `links/Mentions.md`, `Dead End.md` | links | unlinked mentions are not in the CLI |
| Outline | `links/Headings Target.md`, `review/Heading With Hash.md`, `syntax/Inline Formatting.md`, `links/Link Forms.md` | outline | click to jump and following the cursor are UI |
| Tags | `tagging/Tags.md`, `tagging/Unicode Tags.md`, `tagging/Frontmatter Tags.md` | tags | the tree's sorting and counts display are UI; the counts themselves are in the tags answers |
| Properties view | `properties/All Property Types.md`, `properties/Aliases and Cssclasses.md`, `properties/Type Mismatch.md`, `.obsidian/types.json` | properties | rename everywhere is an action |
| Bookmarks | `.obsidian/bookmarks.json`, `links/Headings Target.md`, `links/Blocks Target.md`, `canvases/Board.canvas`, `bases/Notes.base` | vault | bookmark search (1.13) is UI |
| Footnotes view | `syntax/Footnotes.md` | ui | the sidebar list has no CLI command |
| Hover preview | `links/Link Forms.md`, `links/Headings Target.md` | ui | the Mod-key popover is UI |
| Quick switcher | `a/Note.md`, `b/Note.md`, `links/Aliases.md`, `Home.md`, `Unicode é 日本.md`, `excluded/Hidden Note.md` | vault, properties | fuzzy ranking over full paths and recent notes is UI; the twins, aliases and excluded note give it names to rank |
| Command palette | none | - | no fixture: see below |
| Breadcrumbs and history | none | - | no fixture: see below |
| Note composer | `links/Headings Target.md`, `links/Link Forms.md`, `projects/Project Alpha.md` | links | merge and extract are actions; Headings Target.md is linked by heading from many notes, so extracting a heading shows the link rewrite |
| Random note, unique note creator | none | - | no fixture: see below |
| Web viewer | `canvases/Cards.canvas`, `links/Markdown Links.md`, `embeds/Embed Images.md` | ui | opening a link as a tab is UI and needs the network; the web viewer plugin is off in `core-plugins.json` |
| Web viewer tools | none | - | no fixture: see below |

### Search

| Feature | Fixture files | Reference answers | Not covered |
| --- | --- | --- | --- |
| Query language | `search/Search Corpus.md`, `search/Case Sensitivity.md`, `excluded/Hidden Note.md`, `tagging/Tags.md`, `tasks/Task Statuses.md`, `properties/All Property Types.md`, `review/Heading With Hash.md`, `syntax/Bidirectional Text.md`, `Unicode é 日本.md`, `queries.json` | search | - |
| Results | `search/Search Corpus.md`, `queries.json` | search | explain search term, collapse, the six sort orders and copy are UI; the order of the CLI's matches is undocumented |
| Embedded results | `embeds/Embed Query.md`, `search/Search Corpus.md` | dom | - |

### Graph

| Feature | Fixture files | Reference answers | Not covered |
| --- | --- | --- | --- |
| Global graph | `a/Note.md`, `b/Note.md`, `Orphan.md`, `Dead End.md`, `links/Unresolved Links.md`, `Home.md`, `canvases/Board.canvas` | eval, links | nodes and edges are derived from the metadata cache's resolved and unresolved links; the drawn layout cannot be queried |
| Filters | `.obsidian/graph.json`, `attachments/unused.png`, `Orphan.md`, `excluded/Hidden Note.md`, `tagging/Tags.md` | eval, links | the filtered result is not queryable |
| Groups | `.obsidian/graph.json`, `projects/`, `tagging/Tags.md` | ui | colour groups are drawn, not queryable |
| Display | `.obsidian/graph.json` | ui | arrows, fade, node size and link thickness are drawn, not queryable |
| Forces | `.obsidian/graph.json` | ui | calibration is by eye against the app |
| Local graph | `.obsidian/workspaces.json`, `Home.md`, `a/Note.md`, `b/Note.md` | eval, links | depth and direction settings are UI; the neighbours are derivable from the links |
| Interaction | none | - | no fixture: see below |
| Canvas links (1.12) | `canvases/Board.canvas`, `canvases/Cards.canvas`, `canvases/Groups.canvas` | links, eval | whether the CLI counts canvas file cards in `backlinks` is what the reference run settles |

### Canvas

| Feature | Fixture files | Reference answers | Not covered |
| --- | --- | --- | --- |
| File format | `canvases/Board.canvas`, `canvases/Cards.canvas`, `canvases/Edges.canvas`, `canvases/Groups.canvas`, `jsoncanvas/sample.canvas` | vault | Obsidian's own reading of a canvas is not exposed by the CLI; the files are checked against JSON Canvas 1.0 by `tests/vault.rs` |
| Cards | `canvases/Cards.canvas`, `canvases/Board.canvas` | vault | convert to file and swap file are actions; web pages and YouTube need the network |
| Edges | `canvases/Edges.canvas`, `canvases/Board.canvas` | vault | follow connection is an action |
| Groups | `canvases/Groups.canvas`, `attachments/pixel.png`, `attachments/wide.png`, `attachments/photo.jpg` | vault | grouping around a selection, renaming and jumping are actions |
| Board | `canvases/Board.canvas` | vault | pan, zoom, snapping, align, PNG export and read-only mode are board interaction; the nodes are spread out to give them something to act on |
| Embeds | `embeds/Embed Canvas and Base.md`, `canvases/Board.canvas` | dom | - |

### Bases and queries

| Feature | Fixture files | Reference answers | Not covered |
| --- | --- | --- | --- |
| Files | `bases/Notes.base`, `bases/Projects.base`, `bases/Tasks.base`, `Home.md`, `embeds/Embed Canvas and Base.md` | bases, dom | - |
| Views | `bases/Notes.base`, `bases/Projects.base`, `bases/Tasks.base` | bases | the map view needs the Maps plugin and is not planned |
| Logic | `bases/Notes.base`, `bases/Projects.base`, `bases/Tasks.base`, `projects/Project Alpha.md`, `projects/Project Beta.md`, `projects/Project Gamma.md`, `projects/Project Delta.md` | bases | - |
| UI | none | - | no fixture: see below |
| Dataview (reference screenshot) | `dataview/Dataview Fields.md` | dom, search | Dataview is a community plugin and the harness installs none, so its blocks and inline queries are recorded unrendered |

### Workspace

| Feature | Fixture files | Reference answers | Not covered |
| --- | --- | --- | --- |
| Tabs, groups, splits | `.obsidian/workspaces.json` | ui | dragging a tab to split is interaction; the saved layout holds two tab groups |
| Pinned tabs | `.obsidian/workspaces.json`, `Home.md` | ui | links opening elsewhere from a pinned tab is behaviour |
| Stacked tabs | `.obsidian/workspaces.json` | ui | - |
| Linked views | `.obsidian/workspaces.json`, `Home.md` | ui | following a chosen tab is behaviour |
| Pop-out windows | none | - | no fixture: see below |
| Pane menu | none | - | no fixture: see below |
| Sidebars and ribbon | `.obsidian/workspaces.json` | ui | customising the ribbon is interaction; the layout hides none of its items |
| Status bar | none | - | no fixture: see below |
| Workspaces | `.obsidian/workspaces.json`, `.obsidian/core-plugins.json` | ui | saving and loading are actions; the CLI's workspace commands are not among the ones the harness calls |
| Startup note | `Home.md`, `bases/Notes.base` | dom | the Default file to open setting is not written because its key in `app.json` is unverified |
| Settings | `.obsidian/app.json`, `.obsidian/appearance.json`, `.obsidian/core-plugins.json` | vault | the searchable settings window is UI |
| Hotkeys | `.obsidian/hotkeys.json` | ui | - |
| Appearance | `.obsidian/appearance.json`, `.obsidian/snippets/callouts.css`, `.obsidian/snippets/folder-colours.css` | vault, dom | light and dark, fonts and themes are drawn, not queryable |

### Other core plugins

| Feature | Fixture files | Reference answers | Not covered |
| --- | --- | --- | --- |
| Daily notes | `daily/2026-10-23.md`, `daily/2026-10-24.md`, `daily/2026-10-25.md`, `.obsidian/daily-notes.json`, `templates/Daily.md` | vault, wordcount | creating today's note is an action |
| Templates | `templates/Daily.md`, `templates/Meeting.md`, `.obsidian/templates.json` | vault | inserting a template is an action |
| Calendar | `daily/2026-10-23.md`, `daily/2026-10-24.md`, `daily/2026-10-25.md`, `weekly/2026-W43.md` | wordcount, tasks | the calendar is a community plugin the harness does not install; the three daily notes hold 0, 1 and 2 dots of words and only the last has an open task |
| Audio recorder | none | - | no fixture: see below |
| Slides | `slides/Deck.md` | dom | the presentation view is a mode of the app; the reading DOM shows the note unsplit |
| Word count | `daily/2026-10-23.md`, `daily/2026-10-24.md`, `daily/2026-10-25.md`, `syntax/Bidirectional Text.md`, `Unicode é 日本.md`, `syntax/Math.md`, `syntax/Code Blocks.md` | wordcount | the selection's count is UI |
| Publish | none | - | no fixture: see below |
| Sync | none | - | no fixture: see below |

### Platform

| Feature | Fixture files | Reference answers | Not covered |
| --- | --- | --- | --- |
| URI scheme | none | - | no fixture: see below |
| CLI (1.12) | `queries.json` | vault, links, outline, properties, tags, tasks, search, bases, wordcount, eval, dom | the commands that act (daily notes, workspaces, creating, moving or deleting files) are not called; the harness only reads |
| Other platforms | none | - | no fixture: see below |

### The ten most-downloaded community plugins

| Feature | Fixture files | Reference answers | Not covered |
| --- | --- | --- | --- |
| Excalidraw | none | - | no fixture: see below |
| Templater | none | - | no fixture: see below |
| Dataview | `dataview/Dataview Fields.md` | dom, search | `dataviewjs` is not in the plan, and the plugin is not installed for the reference run |
| Tasks | `tasks/Task Statuses.md`, `bases/Tasks.base`, `daily/2026-10-25.md` | tasks, search | the plugin's own query language is not in the plan |
| Git | none | - | no fixture: see below |
| Advanced Tables | `syntax/Tables.md` | dom | spreadsheet formulas are not in the plan |
| Calendar | `daily/2026-10-23.md`, `daily/2026-10-24.md`, `daily/2026-10-25.md`, `weekly/2026-W43.md` | wordcount, tasks | see the core plugin row above |
| Copilot | none | - | no fixture: see below |
| Style Settings | none | - | no fixture: see below |
| Kanban | `bases/Projects.base`, `projects/` | bases | the plugin's own board files are not in the vault and would render as lists |

## Rows no fixture can exercise

| Row | Why |
| --- | --- |
| Vault and files / Single files (1.14) | A file opened from outside the vault is not part of any vault; it needs Open With, Launch Services and Quick Look on a real file. |
| Vault and files / File recovery | Snapshots exist only after edits in the running app; a committed vault has no history to browse. |
| Vault and files / Stable ids | Obsidian has none, so there is no answer to compare; the ids are Frame's `.theorem/ids.json`. |
| Markdown and editing / Multiple cursors | Alt-click and Shift-Alt-drag are pointer input to the editor; they have no content and no CLI answer. |
| Markdown and editing / Vim | A key mode of the editor; no content depends on it. |
| Markdown and editing / Spellcheck | A service of the operating system acting on text as it is typed. |
| Markdown and editing / Paste and drop | Clipboard and drag-and-drop are actions; imports are checked by Frame's own tests. |
| Markdown and editing / Slash commands | A popup listing commands at the cursor; no content depends on it. |
| Reading view / Updates | Re-rendering on change needs edits made in the running app. |
| Reading view / Export to PDF | A File-menu action producing a PDF; any note serves as input. |
| Navigation and panes / Command palette | A fuzzy list of commands; no content depends on it. |
| Navigation and panes / Breadcrumbs and history | Per-tab back and forward is interaction state. |
| Navigation and panes / Random note, unique note creator | Actions that create or open a note; nothing in the vault matters to them. |
| Navigation and panes / Web viewer tools | Reader view and Save to vault act on live web pages, and the fixtures stay offline. |
| Graph / Interaction | Click, hover and drag are pointer input to the canvas. |
| Bases and queries / UI | The search box, drag-and-drop import, row menu and Group menu are interface; the CLI returns rows only. |
| Workspace / Pop-out windows | Windows are not in a vault; moving tabs between them is interaction. |
| Workspace / Pane menu | A menu of commands on each tab. |
| Workspace / Status bar | Interface chrome; its numbers are the backlink and word count answers recorded under other rows. |
| Other core plugins / Audio recorder | Needs a microphone and writes a recording; the plugin is off in `core-plugins.json`. |
| Other core plugins / Publish | A paid hosted service the vault cannot reach, so there is no answer to compare. |
| Other core plugins / Sync | A paid hosted service the vault cannot reach, so there is no answer to compare. |
| Platform / URI scheme | Handled by the operating system and the running app; no vault content is involved. |
| Platform / Other platforms | Not planned, so there is nothing to compare. |
| The ten most-downloaded community plugins / Excalidraw | Not in the plan: its drawings are scenes stored in Markdown and open as Markdown. |
| The ten most-downloaded community plugins / Templater | Not in the plan: its templates run JavaScript against Obsidian's API. |
| The ten most-downloaded community plugins / Git | Not in the plan: file recovery covers local history. |
| The ten most-downloaded community plugins / Copilot | Not in the plan: Theorem's AI surfaces replace it. |
| The ten most-downloaded community plugins / Style Settings | Not in the plan: it reads settings declared inside theme CSS. |

## Notes without an incoming link

These are on purpose: Obsidian's `orphans` should list them, and a vault where everything is linked would not test it. Canvases count as links since 1.12.

- `Orphan.md`: the orphan the `orphans` command is asked about; it has no outgoing links either.
- `inbox/Inbox.md`: the folder for new notes, which nothing refers to.
- `templates/Daily.md` and `templates/Meeting.md`: templates are inserted, not linked.
