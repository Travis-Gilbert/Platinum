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
| `dom` | for each note `eval code=` to open it in the reading view, a pause, then `dev:dom selector=` |

Before anything else the harness runs `version`. It parses the first `x.y.z` of the output, because the CLI page only says "Show Obsidian version", and stops with `refusing to run` unless it is exactly 1.14.5. Nothing is written, not even the output directory, when it refuses.

A failing call is recorded and the run goes on. A call that gets no answer in 120 seconds is killed and recorded as failed. The app reports a bad command as `Error: ...` text on a successful exit (read from the 1.14.5 bundle), so that text counts as a failure too.

### Isolation

The CLI drives a running app, so a reference run has to start the app itself. Read from the installed 1.14.5 bundle:

- `obsidian-cli` is a thin client. It connects to `$HOME/.obsidian-cli.sock`, sends one JSON line (`argv`, `tty`, `cwd`) and prints the answer. It does not start the app: with no socket it prints "The CLI is unable to find Obsidian". This differs from the CLI page, which says the first command launches Obsidian.
- The app binds that socket under `os.homedir()` and takes a single-instance lock keyed by its user data directory.
- It picks the vault from `vault=`, else from the vault that contains the client's working directory, else the active vault.
- Failures arrive as text with exit code 0 (`Error: ...`, `Vault not found.`, `Command line interface is not enabled...`).
- If the last argument is a path that exists, the app opens it, so the app's own working directory is private.

`--app` therefore does this, and nothing else touches the user's Obsidian:

1. A temporary directory `nfo-*` under `$TMPDIR`, kept short because a unix socket path may be at most 104 bytes. In it: a `home/`, a `profile/` and `vaults/<name>`, a copy of the vault. The original is never opened.
2. `profile/obsidian.json` registers only that copy as open, with `cli` on and updates off. The app is started with `--user-data-dir=<profile>`, `HOME=<home>`, a private `TMPDIR`, a cleared environment and `PATH=/usr/bin:/bin:/usr/sbin:/sbin`.
3. `sandbox-exec` wraps it with a profile that denies the real `~/Library/Application Support/obsidian`, `Obsidian`, its saved window state and the real `~/.obsidian-cli.sock`. A misdirected app fails to read them instead of using them. Chromium cannot start its own sandbox inside that one, so the app gets `--no-sandbox`.
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

The window must be visible: the run needs the screen awake and unlocked (see the next section).

### Observed reference run and DOM failures

The supplied 1.14.5 recording answered 800 calls over 68 notes; four `dev:dom` calls returned
`Error: Command "dev:dom" not found. It may require a plugin to be enabled.` The other 64
returned DOM. All 68 opening evals returned `=> preview`. This supersedes the earlier startup
probe that stalled on a locked desktop. The screen should still be awake and unlocked.

The [CLI page](https://obsidian.md/help/cli) documents `dev:dom` as a developer command,
`eval` as executing JavaScript, and the CLI as connecting to the running app. It does not
document a transient "not found" guarantee or a readiness barrier between commands.
Read from the installed 1.14.5 bundle: the renderer's `handleCli` looks up its handler map
and emits this error before invoking any handler. `dev:dom` and `eval` are registered in
the desktop/Electron branch of CLI initialization; `dev:dom` does not check a plugin or
the active Markdown view. A selector miss instead returns `No elements found.` `eval`
awaits its result, including a promise, and formats string results without JSON quotes.
A different or reinitializing renderer/handler registry can therefore make a command
unavailable; opening a note or waiting 500 ms is not a handler-registration check. The
recording cannot establish which lifecycle event caused these four failures, and no
live reproduction was possible in the development sandbox.

The recorder checks that opening succeeded and returned exactly `preview` (bare, `=>`
prefixed, or JSON quoted). Missing files, another mode, thrown errors, and unrecognized
answers fail the opening record; DOM is not read from the preceding note. After the
configured settle delay, only the specific `dev:dom` command-not-found answer is retried:
three attempts maximum, with 200 ms between attempts. Empty DOM and `No elements found.`
are failures too. `dom.json` stores `attempts`, earlier failed records in `retries`, and
the final `dom` record (`null` when opening failed). The manifest's `dom_attempts` map
includes every note, with zero for failed opens; `commands` includes every actual attempt.
Exhausted retries stay in `failed`. Recovered failures remain visible in `retries`.

### Outputs

`crates/notes-fixtures/reference/<vault>/` holds `<group>.json` for each group (a map from the call, as typed, to its record: `command`, `args`, `exit_code`, normalized `stdout`, `stderr`, `error`, and `parsed` when `format=json` gave JSON), `dom.json` with `dom/<note>.html` for each note's reading view, and `manifest.json`: the Obsidian version and the normalized `version` output, the harness version, the vault, the groups run and skipped, the number of calls, the files `files` listed that are not on disk and the reverse, every failed call, and `known_gaps`, the things no run can ask Obsidian.

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
- `vault.size` and Bases `Size (KB)` derive from fixture byte lengths, not allocated disk
  blocks. No file ctime/mtime stat fields occur in the recorded JSON; the mtime queries
  above are the only epoch timestamps found. Metadata positions and counts are content-derived.
- Daily-note dates, frontmatter `created`/due dates, the pasted-image filename, task dates,
  template date/time tokens and custom HTML IDs are authored fixture data. Bases `Time left`
  subtracts the fixed `date("2026-10-25")`, not the current clock. These values are preserved.
- No additional user names or random IDs were found. Version strings, enabled plugins,
  content, errors and attempt counts are evidence: failures and retry counts can differ
  between runs and are intentionally retained rather than made to look successful.

The normalizer uses the existing `regex` and `serde_json` dependencies. Its DOM substitutions
are narrowly scoped to the generated attributes observed above; it does not parse or
reserialize the HTML tree. This preserves Obsidian's markup and avoids a new parser dependency.
Lazy rendering, asynchronous embeds, font availability, and different app settings can still
change *which* content is rendered. Normalizing geometry does not establish complete DOM capture;
that remains a live acceptance check when rerunning outside the sandbox.

Known gaps: the graph view (the CLI has no graph command; nodes and edges come from `resolvedLinks` and `unresolvedLinks`), unlinked mentions (`backlinks` is documented as linked only), canvas contents (no canvas command), the search results view, Live Preview and Source mode, and settings and workspace UI. `COVERAGE.md` says which matrix rows these leave to a comparison in the running app.

### Unverified

These still need live acceptance after the recorder changes:

- full reading-view capture for long notes and asynchronous embeds across window sizes (override the selector with `--dom-selector`);
- the JSON field names of every `format=json` answer, which the CLI page does not give: the harness normalizes stdout and parses it only as far as it is JSON;
- the shapes of the files in `vault/.obsidian/` (`app.json`, `bookmarks.json`, `graph.json`, `workspaces.json`, `hotkeys.json`, `types.json`, `core-plugins.json`);
- the outcome of each probe note: whether `[[Note#C# basics]]` or `[[Note#C basics]]` resolves, whether alias links resolve, whether links in comments count, which tag forms are tags.

The normalization tests also use representative strings from the supplied real recording. The fake CLI used by the tests is a shell script that answers in the shapes the CLI page shows; whatever the page leaves out, such as the JSON fields, it makes up.

## Tests

```
cargo test -p notes-fixtures --locked
```

- `tests/generate.rs`: determinism, the pinned digest, the mix, valid extras, no name collisions.
- `tests/reference.rs`: the harness against the fake CLI. The script is written to a temporary directory and put first on `PATH` for the binary tests; it logs every invocation and replies with the documented shapes. The tests check the call table, the version refusal, `vault=` ordering, the normalized outputs across two mock isolation roots, preview validation, bounded DOM retries and attempt accounting, and that failures are recorded: a non-zero exit, `Error:` text on a clean exit, and a call that never answers.
- `tests/launch.rs` (macOS): the isolated launch against a fake `Obsidian.app`: private home and profile, the sandbox denying stand-ins for the real profile and socket, the seeded `obsidian.json`, flags passed through, every CLI call on the private home and in the copy, the app and the directory gone afterwards, and the same on a wrong version, a vault mismatch, an app that exits or never binds, SIGINT, a missing app, and the `--app`/`--attach` rules.
- `tests/vault.rs`: the lints above.
- `tests/coverage.rs`: `COVERAGE.md` against the matrix in `docs/plans/notes-app.md`.
