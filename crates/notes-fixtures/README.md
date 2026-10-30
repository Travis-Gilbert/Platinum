# notes-fixtures

The test corpus and the Obsidian reference harness for Frame's notes app, which has to reach parity with Obsidian 1.14.5 (`docs/plans/notes-app.md`). The crate is `publish = false`.

- `vault/`: a small hand-written Obsidian vault that exercises the parity matrix.
- `jsoncanvas/`: JSON Canvas samples under their own MIT licence.
- `COVERAGE.md`: every row of the matrix, the fixture files for it, and the rows no fixture can exercise.
- `notes-fixtures generate`: a deterministic vault of thousands of notes for benchmarks.
- `notes-fixtures reference`: runs Obsidian's CLI over any vault and stores its answers as JSON.

```
crates/notes-fixtures
  vault/               fixture vault: 68 notes, 4 canvases, 3 bases, 17 attachments, .obsidian/ config
  jsoncanvas/          sample.canvas, LICENSE, SOURCE.md (upstream commit)
  script/make-media    regenerates vault/attachments from local tools; nothing is downloaded
  queries.json         search queries the reference run asks (operators, case, path, limit, Unicode)
  src/generate.rs      the generator
  src/obsidian.rs      the CLI client: argv, one call (with a timeout), one record
  src/reference.rs     the groups of calls and the manifest
  src/launch.rs        starts Obsidian on a private HOME and profile, and quits it
  tests/               generator, harness and launch (against a fake CLI and a fake app), vault lints, coverage
  reference/<vault>/   outputs of a real run (see below); only fixture-vault/ is committed
```

## The fixture vault

Every construct a matrix row names is in a note that says what it is for, and `tests/vault.rs` checks about 145 of them by pattern, so a fixture cannot silently lose the line it exists for. The same file lints the rest:

- every link resolves, except the ones meant not to (a pinned list of unresolved files and unresolved headings and blocks);
- `a/Note.md` and `b/Note.md` are two notes with one name, and a link resolves to the one in its own folder first;
- the notes nothing links to are exactly the ones `COVERAGE.md` names;
- frontmatter parses, except in the note that is broken on purpose, and property types agree with `.obsidian/types.json` except in the note that disagrees on purpose;
- every canvas is valid JSON Canvas 1.0 and every base follows the Bases syntax: only documented keys, functions from the Bases function list;
- attachments are small, are what their extension says (magic bytes), and are all embedded except `attachments/unused.png`;
- the CRLF note has only CRLF and the byte-order-mark note starts with one.

The attachments are synthesised by `script/make-media` from ffmpeg's test sources and a typeset PDF; none is downloaded. The saved layout in `.obsidian/workspaces.json`, the other files in `.obsidian/` and the colour emoji of the highlight note are written from memory of Obsidian's formats and are unverified until the first real run (see "Unverified" below).

### The help vault

Obsidian's help vault (`obsidian-help`) has no licence and is never committed. The harness takes any vault path, so it can be pointed at a clone kept outside the repository, or inside `reference/obsidian-help/`, which `.gitignore` excludes. `tests/vault.rs` fails if anything but `reference/fixture-vault/` could be committed under `reference/`.

## JSON Canvas samples

`jsoncanvas/sample.canvas` is verbatim from <https://github.com/obsidianmd/jsoncanvas> (MIT). `jsoncanvas/LICENSE` is the upstream licence and `jsoncanvas/SOURCE.md` names the commit. The sample's file cards point at upstream files that are not here, so the canvas lint allows those three paths.

## Generator

```
cargo run -p notes-fixtures --release -- generate --notes 10000 --seed 42 --out /path/to/new-or-empty-dir
```

The same seed gives the same bytes: the generator draws from a seeded ChaCha8 stream (`rand_chacha`), keeps every collection ordered, and writes the tree in a fixed order. It prints the counts and a SHA-256 over the tree (`digest`). It refuses a directory that is not empty and a count of zero.

The mix: eight top folders (projects, areas, journal, reference, people, meetings, archive, library) with nested subfolders; wikilinks to notes, headings and blocks with aliases; Markdown links; unresolved links; embeds of notes, headings and images with sizes; nested and frontmatter tags; properties of several types; tasks with several status characters; callouts, folded ones among them; code blocks, tables, math, footnotes, highlights and comments; and a large note (300 to 900 sections) every 250 notes. Beside the notes it writes a canvas per thousand notes, two bases and a few copies of a one-pixel PNG.

`tests/generate.rs` generates twice and compares every file byte for byte, pins the digest of `--notes 300 --seed 42`, and checks the mix. If the pin fails after a dependency update, the generator's output changed; update the digest on purpose, never to quiet the test.

## Reference harness

```
notes-fixtures reference --vault <dir> --out <dir> (--app /Applications/Obsidian.app | --attach [--obsidian <path>] [--vault-name <name>])
                         [--queries <file>] [--only vault,links,...] [--dom-selector <css>] [--settle-ms 500]
                         [--startup-secs 90] [--app-arg <electron flag>]...
notes-fixtures probe --app /Applications/Obsidian.app --vault <dir> [--startup-secs 90] [--app-arg <flag>]...
```

`--app` starts Obsidian itself, isolated (see "Isolation" below). `--attach` talks to whichever Obsidian the CLI at `--obsidian` reaches, with that app's own profile; use it only with an instance you isolated yourself, or with the fake used by the tests. One of the two is required.

The commands, parameters and output formats are the ones documented on these pages, and only those:

| Page | Used for |
| --- | --- |
| <https://obsidian.md/help/cli> | every command below, their parameters and `format=` values; `vault=<name>` first; `file=` against `path=`; the working directory selecting the vault; installer 1.12.7 or later and a running app |
| <https://obsidian.md/help/plugins/search> | the operators in `queries.json` |
| <https://obsidian.md/help/bases/syntax> | the keys of a `.base` file, so `views[].name` can be passed to `base:query` as `view=` |
| <https://obsidian.md/help/bases/functions> | the function list the base lint allows |
| <https://docs.obsidian.md/Reference/TypeScript+API/> | the app calls inside `eval`: `metadataCache.getCache`, `.resolvedLinks`, `.unresolvedLinks`, `vault.getFileByPath`, `WorkspaceLeaf.openFile`, `MarkdownView.getMode` |

`tests/reference.rs` holds a table of each command's documented parameters, formats and flags (copied from the CLI page on 2026-10-26) and fails if the harness makes a call the table does not allow.

| Group | Calls |
| --- | --- |
| `vault` | `vault`, `vault info=path`, `files` (and `files total`), `folders`, `bases`, `plugins:enabled filter=core format=json`, `plugins:enabled filter=community format=json`, `bookmarks format=json`, `templates`, `snippets:enabled` |
| `links` | `unresolved verbose counts format=json`, `unresolved total`, `orphans`, `deadends` (each also with `total`); for each note `links path=` and `backlinks path= counts format=json` |
| `outline` | for each note `outline path= format=json` |
| `properties` | `aliases verbose`, `aliases total`, `properties counts format=json`; for each note `properties path= format=json` and `aliases path=` |
| `tags` | `tags counts format=json`, the same with `sort=count`, `tags total`; for each note `tags path= counts format=json` |
| `tasks` | `tasks format=json`, `verbose`, `todo`, `done`, `total`; `tasks status=<c> format=json` for each status character found in the vault; `tasks path= format=json` for each note with tasks |
| `search` | per query in `queries.json`: `search query= format=json`, `search:context query= format=json`, and `search query= total`; `path=`, `limit=` and `case` as the query says |
| `bases` | `base:query path= format=json` for each `.base`, and again with `view=` for each view name read from its YAML (`base:views` lists the current base only) |
| `wordcount` | for each note `wordcount path=` |
| `eval` | `eval code=` with `app.metadataCache.resolvedLinks`, `.unresolvedLinks`, `getCache(path)` for every note, and the file count |
| `dom` | for each note `eval code=` to open it in the reading view, then `dev:dom selector=` every `--settle-ms` until three answers in a row agree |

Before anything else the harness runs `version`. It parses the first `x.y.z` of the output, because the CLI page only says "Show Obsidian version", and stops with `refusing to run` unless it is exactly 1.14.5. Nothing is written, not even the output directory, when it refuses.

A failing call is recorded and the run goes on. A call that gets no answer in 20 seconds is killed and run again, up to three attempts; each killed attempt is listed in the manifest's `stalled` and counted in `commands`, and a call killed three times is recorded as failed (see [Stalled calls](#stalled-calls)). The app reports a bad command as `Error: ...` text on a successful exit (read from the 1.14.5 bundle), so that text counts as a failure too.

### Isolation

The CLI drives a running app, so a reference run has to start the app itself. Read from the installed 1.14.5 bundle:

- `obsidian-cli` is a thin client. It connects to `$HOME/.obsidian-cli.sock`, sends one JSON line (`argv`, `tty`, `cwd`) and prints the answer. It does not start the app: with no socket it prints "The CLI is unable to find Obsidian". This differs from the CLI page, which says the first command launches Obsidian.
- The app binds that socket under `os.homedir()` and takes a single-instance lock keyed by its user data directory.
- It picks the vault from `vault=`, else from the vault that contains the client's working directory, else the active vault.
- Failures arrive as text with exit code 0 (`Error: ...`, `Vault not found.`, `Command line interface is not enabled...`).
- If the last argument is a path that exists, the app opens it, so the app's own working directory is private.

`--app` therefore does this, and nothing else touches the user's Obsidian:

1. A temporary directory `nfo-*` under `$TMPDIR`, kept short because a unix socket path may be at most 104 bytes. In it: a `home/`, a `profile/` and `vaults/<name>`, a copy of the vault. The original is never opened.
2. `profile/obsidian.json` registers only that copy as open, with `cli` on and updates off. The app is started with `--user-data-dir=<profile>`, `--use-mock-keychain`, `HOME=<home>` (and `CFFIXED_USER_HOME`), a private `TMPDIR`, a cleared environment and `PATH=/usr/bin:/bin:/usr/sbin:/sbin`. Without the mock keychain, Electron's safeStorage looked for a login keychain under the private home, found none, and the main thread waited on a password prompt, so the first CLI request was never answered; the mock also keeps the app off the user's keychain.
3. `sandbox-exec` wraps it with a profile that denies the real `~/Library/Application Support/obsidian`, `Obsidian`, its saved window state and the real `~/.obsidian-cli.sock`. A misdirected app fails to read them instead of using them. One thing it cannot cover: the app's Cocoa defaults (`md.obsidian`) are served by `cfprefsd` from the real user's domain whatever the home, so the isolated app shares that domain with the user's Obsidian. Obsidian keeps its own settings in its profile, not there. Chromium cannot start its own sandbox inside that one, so the app gets `--no-sandbox`.
4. Every CLI call runs with the private `HOME`, in the vault copy. After the first answer the harness checks that `vault info=path` names the copy; anything else stops the run.
5. The app runs in its own process group. It is quit on success, on any error, on SIGINT, SIGTERM and SIGHUP, and on a panic: SIGTERM, ten seconds of grace, then SIGKILL for the group. The temporary directory is removed. The real socket's inode and modification time are compared before and after. `manifest.json` records placeholders for the isolation paths, the app version and whether the sandbox was on (`isolation`).

`tests/launch.rs` checks all of this against a fake `Obsidian.app` that records its environment and tries to read stand-ins for the real profile and socket.

### Running it for real

Nothing here needs the user's Obsidian, vault list or settings. Install Obsidian 1.14.5 (the app bundle is enough; the CLI is `Contents/MacOS/obsidian-cli`).

1. Check the launch first. It starts the isolated app, asks six read-only questions (`version`, `vault`, `vault info=name`, `vault info=path`, `files total`, `help`), prints the answers and the app's log, and quits it:

```
cargo run -p notes-fixtures --release -- probe --app /Applications/Obsidian.app --vault crates/notes-fixtures/vault
```

2. Run the harness:

```
cargo run -p notes-fixtures --release -- reference \
  --app /Applications/Obsidian.app \
  --vault crates/notes-fixtures/vault \
  --out crates/notes-fixtures/reference/fixture-vault
```

3. Settle the settings the matrix depends on and write them down: for example Mermaid rendering is off by default since 1.13, so the diagrams are in the DOM only if it is on. The copy starts from the fixture's `.obsidian/` files, so put the setting there, not in the app.

For the help vault, clone it outside the repository and use `--vault <clone> --out crates/notes-fixtures/reference/obsidian-help` (ignored by git). To drive an instance you started and isolated yourself, use `--attach` (with `--vault-name <name>` to pass `vault=<name>` first on every call).

Every run so far had the screen awake and unlocked. Whether a locked screen or a hidden window changes the answers, the reading view in particular, has not been tried.

### The committed recording

`reference/fixture-vault/` was recorded on 2026-10-26 with Obsidian 1.14.5 by the command in step 2, in 154 seconds: 940 calls over 68 notes, none failed. Two calls stalled and were answered on their second attempt (`tasks status=< format=json` and one `dev:dom`; see [Stalled calls](#stalled-calls)). All 68 notes opened in the reading view. 67 reading views gave the same answer on the first three reads; `embeds/Embed Media.md` changed once, when its PDF finished loading.

Six earlier full recordings and one of the DOM group alone were compared with it, answer by answer. Apart from calls that stalled in a run that had no retry yet, two things varied:

- The ambiguous `[[Note]]` in `links/Link Forms.md` resolved to `a/Note.md` in four runs and to `b/Note.md` in three, which changes `resolvedLinks` and the backlink counts of both notes. Obsidian does not fix this order; see the audit under [Outputs](#outputs).
- Reading views read once, 500 ms after opening, had not always finished rendering: the PDF in `embeds/Embed Media.md` (no pages in six runs, two of three in one) and the embedded notes in `embeds/Embed Notes.md` (an empty view in one run). Reading until the view stops changing, as below, gave the complete view both times it was run.

### Reading the DOM

The recorder checks that opening succeeded and returned exactly `preview` (bare, `=>` prefixed, or JSON quoted). Missing files, another mode, thrown errors, and unrecognized answers fail the opening record; DOM is not read from the preceding note.

The reading view renders long notes lazily and embeds asynchronously, and the CLI says nothing about when it is done, so `dev:dom` is called every `--settle-ms` (500 ms by default) until three answers in a row are the same, at most 20 times. Answers are compared after normalization, so generated geometry is not a change. A view still changing after 20 reads is recorded as it stands and listed in `failed`. An empty answer or `No elements found.` is read again the same way and fails if it stays. Other errors end the reads.

`Error: Command "dev:dom" not found. It may require a plugin to be enabled.` is retried up to three times. An earlier recording got it for four notes, and none of the eight runs since has. Read from the 1.14.5 bundle: the renderer's `handleCli` looks up its handler map and emits this error before invoking any handler; `dev:dom` and `eval` are registered in the desktop branch of CLI initialization, and `dev:dom` checks no plugin and no view. A renderer whose handler map is not in place answers this way; what caused it then is not known.

`dom.json` stores, per note, `attempts` (every `dev:dom` call), `changes` (how often an answer differed from the one before it), the command-not-found answers in `retries`, and the final `dom` record (`null` when opening failed). The manifest's `dom_attempts` map includes every note, with zero for failed opens; `commands` includes every call.

### Stalled calls

1.14.5 now and then never finishes a call it has answered: the CLI has the answer (in one case only part of an outline) and waits for the app to close the connection, which it does not do. From samples of both processes during stalls, and from the bundle:

- The client connects to `$HOME/.obsidian-cli.sock`, writes one JSON line and copies the socket to its stdout until end of file. During a stall its main thread waits in `pthread_join` and its reader thread in `read()` on the socket.
- The app's socket handler reads that line, runs the command in the vault's renderer through `executeJavaScript`, writes the answer and calls `end()` on the socket in the same step. During a stall the app's main and renderer threads are idle and it still holds the connection open. The five-second header timer the handler never clears only rejects a promise that has already settled.
- A second call during a stall is answered normally while the stalled one stays stuck, so the app as a whole is not blocked.
- No call that finished took more than a second. In three runs with a 120-second limit and no retry, each had one stall, on the call in flight four to five seconds after the app started; runs with the retry also stalled later in the run, as the committed recording did. Over all the runs it came to about one call in 800, on no particular command.

The cause inside the app is not known. Every call the recorder makes is a query, or opens a note that opening again leaves as it is, so a call gets 20 seconds and is then killed and run again, up to three attempts. Each killed attempt is listed in the manifest's `stalled` (group, call, error) and counted in `commands`. The final attempt is the one recorded, and a call killed three times is in `failed`.

### Outputs

`crates/notes-fixtures/reference/<vault>/` holds `<group>.json` for each group (a map from the call, as typed, to its record: `command`, `args`, `exit_code`, normalized `stdout`, `stderr`, `error`, and `parsed` when `format=json` gave JSON), `dom.json` with `dom/<note>.html` for each note's reading view, and `manifest.json`: the Obsidian version and the normalized `version` output, the harness version, the vault, the groups run and skipped, the number of calls, the files `files` listed that are not on disk and the reverse, every stalled attempt (`stalled`), every failed call, and `known_gaps`, the things no run can ask Obsidian.

Every answer goes through the same recording normalizer before it reaches a group JSON,
DOM sidecar, or manifest. The live CLI and the launch safety checks still use original
answers. JSON keys and string values, stdout, stderr and errors are normalized together;
JSON stdout is serialized with sorted object keys. No post-processing of recordings is needed.

| Placeholder | Where it applies |
| --- | --- |
| `__vault__` | Original vault and isolated copy roots in answers, including `/private/var/…` and `/var/…` aliases and resource URLs; `isolation.vault_copy`. The manifest's `vault` instead uses the repository-relative source path (`crates/notes-fixtures/vault`); an external vault uses this placeholder. |
| `__app__` | Authority of every `app://` resource URL. |
| `__mtime__` | Numeric cache-busting query on an `app://` resource; the `?` and any fragment or following parameters remain. |
| `__home__` | Private HOME in answers/diagnostics and `isolation.home`. |
| `__profile__` | Private user-data directory in answers/diagnostics and `isolation.user_data_dir`. |
| `__run__` | Other paths beneath the isolation root, such as its private temporary directory, in diagnostics. |
| `__repo__` | Repository root in answers/diagnostics outside the source vault. |
| `__footnote_N__` | Random 16-hex footnote suffixes in DOM `id`, `data-footnote-id`, and `href`; N starts at 1 for each DOM answer in first-occurrence order, preserving distinct embedded-note namespaces and reference links. |
| `__layout__` | Runtime geometry in generated preview sizers, Bases containers/rows/cells/cards, PDF containers/text layers/thumbnails, and canvas backing dimensions. CSS declarations, units and DOM structure remain. Authored image dimensions, raw HTML styles, SVG geometry and MathJax metrics remain. |
| `__limited_result__` | Each returned entry of a search with a positive `limit`. The app truncates its unsorted file enumeration, so membership and context are run-dependent. Cardinality is retained; selected file identity/context are deliberately not a parity assertion for these limited calls. Unlimited calls retain full results. |

For example, an image becomes
`app://__app__/__vault__/attachments/pixel.png?__mtime__`.
`isolation.app` and `sandboxed` are retained as requested provenance.

Audit of the supplied recording:

- The vault/source paths, resource authority and mtime queries, random footnote suffixes,
  and generated layout measurements above vary with the checkout, run or window.
- Search result order follows `getMarkdownFiles()` enumeration in the installed bundle;
  complete result arrays are sorted. Tags sorted by count have no tie-breaker in the app;
  ties are recorded in tag order. JSON object keys and recorder maps are sorted. Semantic
  arrays (outline/source positions, frontmatter lists, bookmarks and Base view rows) keep
  their order. `base:query` sorts input files by path before applying the view, making
  ties deterministic. Each alias and unresolved target in this recording has only one
  source file, so their unsorted source lists cannot vary here. The other CLI listings
  and task output are sorted by the app, and fixture scanning, status sets and file
  differences already use ordered traversal/collections.
- An ambiguous link resolves in an order the app does not fix. Read from the bundle,
  `getLinkpathDest` takes the files with the link's name, puts the ones under the source note's
  folder first, and sorts each group by path length; files whose paths are equally long keep the
  order the vault loaded them in, which varies between runs. `[[Note]]` in `links/Link Forms.md`
  has two such candidates, `a/Note.md` and `b/Note.md` (nine characters each, neither under
  `links/`), and resolved to each in different runs; the recording has `a/Note.md`. For that one
  link, `resolvedLinks` of `links/Link Forms.md` and the backlink counts of the two notes are not a
  parity assertion: either candidate matches Obsidian. Links from inside `a/` and `b/` resolve to
  their own folder's note in every run.
- `vault.size` and Bases `Size (KB)` derive from fixture byte lengths, not allocated disk
  blocks. No file ctime/mtime stat fields occur in the recorded JSON; the mtime queries
  above are the only epoch timestamps found. Metadata positions and counts are content-derived.
- Daily-note dates, frontmatter `created`/due dates, the pasted-image filename, task dates,
  template date/time tokens and custom HTML IDs are authored fixture data. Bases `Time left`
  subtracts the fixed `date("2026-10-25")`, not the current clock. These values are preserved.
- No additional user names or random IDs were found. Version strings, enabled plugins,
  content, errors and attempt counts are evidence: failures, stalls, retry counts and DOM
  read counts can differ between runs and are intentionally retained rather than made to
  look successful.

The normalizer uses the existing `regex` and `serde_json` dependencies. Its DOM substitutions
are narrowly scoped to the generated attributes observed above; it does not parse or
reserialize the HTML tree. This preserves Obsidian's markup and avoids a new parser dependency.
Font availability and different app settings can still change *which* content is rendered,
and content that appears only after a longer pause than three reads, or only on scrolling, is
not captured. Normalizing geometry does not establish complete DOM capture.

Known gaps: the graph view (the CLI has no graph command; nodes and edges come from `resolvedLinks` and `unresolvedLinks`), unlinked mentions (`backlinks` is documented as linked only), canvas contents (no canvas command), the search results view, Live Preview and Source mode, and settings and workspace UI. `COVERAGE.md` says which matrix rows these leave to a comparison in the running app.

### Unverified

These still need live acceptance after the recorder changes:

- reading-view capture of content that renders only on scrolling, and across window sizes (override the selector with `--dom-selector`);
- the JSON field names of every `format=json` answer, which the CLI page does not give: the harness normalizes stdout and parses it only as far as it is JSON;
- the shapes of the files in `vault/.obsidian/` (`app.json`, `bookmarks.json`, `graph.json`, `workspaces.json`, `hotkeys.json`, `types.json`, `core-plugins.json`);
- the outcome of each probe note: whether `[[Note#C# basics]]` or `[[Note#C basics]]` resolves, whether alias links resolve, whether links in comments count, which tag forms are tags. The recording holds Obsidian's answers; they have not been read out into `COVERAGE.md` yet.

The normalization tests also use representative strings from the supplied real recording. The fake CLI used by the tests is a shell script that answers in the shapes the CLI page shows; whatever the page leaves out, such as the JSON fields, it makes up.

## Comparing DOM structure

`notes_fixtures::compare` (and `notes-fixtures compare-dom --expected <dir> --actual <dir>`) checks what Frame renders for a note against the DOM Obsidian recorded for it (`reference/<vault>/dom/<note>.html`). It compares structure, not text files: both sides are parsed with html5ever, reduced to a tree, and the trees are compared; the first differences are printed with their path from the content root (`div.el-ul[3] > ul.has-list-bullet > li[0]@class`).

```
cargo run -p notes-fixtures --locked -- compare-dom \
  --expected crates/notes-fixtures/reference/fixture-vault/dom --actual <dir with Frame's recordings> \
  --allow-truncated-expected --opaque .math --hollow pdf-embed --ignore button.copy-code-button
```

The exit status is 1 when a note mismatches or a recording is missing. The Rust API is `compare_dirs(expected, actual, &Options, note_filter)`; `crates/notes/tests/reading_dom.rs` uses it to render every note of the fixture vault with `frame_notes::render_note`.

What is compared is the content root's children (`.markdown-preview-sizer`): tag, class set, attributes and children, with these normalizations. Each is a difference in how the DOM was recorded or serialized, not in what a reader sees.

- **Serialization.** Attribute order and quoting, `<br>` against `<br/>`, entity forms, and the order of class names do not count. Whitespace is collapsed as a browser does and dropped beside and inside block elements, so the exact newlines of Obsidian's markdown renderer are not needed.
- **`style`** is compared as its sorted declarations (`display:none;` is `display: none;`); an empty `style=""` is no style.
- **Footnote ids** (`fn-1-<16 hex>`, `fnref-…`) are renumbered by first use; the hex part is a per-note salt.
- **Resource URLs** compare as vault-relative paths: Obsidian's `app://<hash>/<vault>/<path>?<mtime>` and Frame's `/files/<path>` are the same file.
- **The recorder's layout numbers** (`__layout__` in an expected value) match any number.
- **Attributes not compared:** `data-sourcepos` (Frame's annotation for the host), `tabindex`, `contenteditable`, `spellcheck`, `autocapitalize`, `enterkeyhint`, `inputmode`, `placeholder` and `draggable` (the editing affordances of Obsidian's property editor and workspace; the reading view edits through the host), `aria-disabled`, and every `data-frame-*` (what a Frame page script needs to fill an embed). `value` and `checked` on the property editor's inputs are DOM properties in Obsidian and absent from its serialization, so a renderer that shows the value writes them as attributes: they are not compared there.
- **The class `is-loaded`** is added by Obsidian once an embed's image or a script has finished with it.
- **Excluded children of the content root:** `.markdown-preview-pusher` (a spacer the virtual scroller inserts) and `.mod-footer` (the embedded-backlinks footer, built from the index).
- **A hidden `.metadata-error-container`** (the properties' "Invalid properties" box with `display: none`) is compared without its children. Obsidian fills the box the first time a note with invalid properties is shown and keeps the text for every note after it, so the same note records with or without them depending on what was opened first. A visible box is compared with its text.

Options for what scripts in the page produce, which a headless render does not run (all empty by default, so nothing is excluded unless asked):

- `ignore` drops matching elements from both sides (the copy button Prism adds to a code block).
- `opaque` compares matching elements by tag, classes and `data-*` attributes only (MathJax's output in `.math`, a Mermaid or query fence replaced in the page).
- `opaque_text` adds the plain text of the subtree (Prism wraps the code's text in token spans).
- `hollow` names the classes of `span`s whose script-built content is block-level (the PDF viewer, a Bases view). The content is cut from the markup before parsing: a parser ends the paragraph around such a span at the first `div` inside it and moves the rest out, so the recording and a renderer that writes an empty span would never match. The span and its attributes are still compared.

Long notes are recorded as far as Obsidian rendered them (its reading view draws sections as they scroll into view). With `allow_truncated_expected` a note whose recording ends early is compared on the blocks the recording holds and reported as `Prefix { verified, total }`, never as a full match; an actual side shorter than the recording is a mismatch. A note present on one side only is `missing` or `extra`.

## Tests

```
cargo test -p notes-fixtures --locked
```

- `tests/generate.rs`: determinism, the pinned digest, the mix, valid extras, no name collisions.
- `tests/reference.rs`: the harness against the fake CLI. The script is written to a temporary directory and put first on `PATH` for the binary tests; it logs every invocation and replies with the documented shapes. The tests check the call table, the version refusal, `vault=` ordering, the normalized outputs across two mock isolation roots, preview validation, DOM reads until the view stops changing, bounded command-not-found retries and attempt accounting, and that failures are recorded: a non-zero exit, `Error:` text on a clean exit, a call that never answers, and a call (or a DOM read) that stalls once and is run again.
- `tests/launch.rs` (macOS): the isolated launch against a fake `Obsidian.app`: private home and profile, the sandbox denying stand-ins for the real profile and socket, the seeded `obsidian.json`, flags passed through, every CLI call on the private home and in the copy, the app and the directory gone afterwards, and the same on a wrong version, a vault mismatch, an app that exits or never binds, SIGINT, a missing app, and the `--app`/`--attach` rules.
- `src/compare.rs` (unit tests): every normalization above, in both directions (what is ignored and what still fails).
- `tests/vault.rs`: the lints above.
- `tests/coverage.rs`: `COVERAGE.md` against the matrix in `docs/plans/notes-app.md`.
