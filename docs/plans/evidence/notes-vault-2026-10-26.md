# Headless vault core, step 2

Branch: `feat/notes-vault`. Only `crates/notes`, `Cargo.lock`, and this evidence
file were changed. The fixture harness format and the dependency checkouts were
not changed. Theorem pin: `d9d1b9f44`.

**Step 2 acceptance is incomplete.** The implementation and local tests are
present; Obsidian recordings, the upstream rename transaction and a completed
10,000-note measurement remain outstanding. The large measurement exposed
substantial VFS/RedCore write amplification.

## Acceptance checklist

- [x] Durable VFS mount, blobs and one RedCore authority, bound before opening documents
- [x] Runtime sessions, atomic save, close/reopen, undo/redo, typed external reconciliation
- [x] JSON/YAML validation before edits and authority commits; invalid merged JSON refused
- [x] Durable RedCore metadata nodes, link edges, incremental updates and queries
- [x] Exclusion flags and profile-owned settings, independent of live `.obsidian` configuration
- [x] VFS-owned stable ids; observed rename, deletion, recreation and replay tests
- [x] Rename/move rewrite computation, including current unsaved heads and revision/hash guards
- [ ] Runtime transaction integration: upstream operation not yet supplied
- [x] Comparison test covering eight recorded groups, failing explicitly when recordings are missing
- [ ] Actual Obsidian parity: no recordings were present
- [x] SIGKILL save/index recovery and external-edit tests
- [x] G6 dynamic acceptance: passed under a network-denying `sandbox-exec` profile, run outside Codex's sandbox
- [x] Benchmark example, deterministic 10,000-note generation, measured attempt and fixture run
- [ ] Completed 10,000-note cold/warm/RSS/authority/index measurements
- [x] Offline compilation, formatting and clippy; test failures below remain visible

## Public API and ownership

`Vault::open(root, data_dir)` takes an existing vault folder and Frame's per-vault
profile data directory. The data directory must be outside the vault. It opens:

- `authority/`: one `RedCoreAuthority`, installed with `set_authority` before
  `bind_checkout` or any document operation. The runtime's temporary default
  in-memory authority never owns a vault document.
- `vfs/`: `RedCoreGraphStore` with `AofAlways`; `Vfs::open_in("frame-notes", ...)`
  mounts the root. Restored mounts are refreshed. New mounts already scan once.
- `blobs/`: the existing durable `DiskObjectStore` implementation of `BlobStore`.
- `index/`: a separate rebuildable RedCore metadata graph, also `AofAlways`.
- `settings.json`: serde data, atomically replaced with tempfile, file sync and
  directory sync. Production does not read `.obsidian` settings. The comparison
  test explicitly configures its vault from the fixture's settings to match its oracle.

`open_document`, `edit(path, revision, operation_id, text)`, `save`, `close`,
`undo`, `redo` and `reconcile` are the document boundary. `Document` returns the
runtime id, revision and text; opening a divergent document also exposes its
optional typed reconciliation conflict. `save` and `reconcile` return the
runtime's `ReconcileResult`. Overlaps preserve both texts and save neither.
Canvases and bases are source sessions, validated with the existing JSON/YAML
parsers. Undo/redo are previewed before publication, and the authority adapter
validates commits as a second boundary.

The runtime is private. Its atomic save calls `record_disk_hash`; attachments
are written only by `write_attachment` through VFS. `IdStore::save_vfs` writes
`.theorem/ids.json` through VFS. Read-only index, settings, ids, authority and VFS
accessors do not expose a mutable alternate writer.

`refresh` is the completeness/rescan path. `apply_events` consumes explicit VFS
events without reading unrelated file contents. A durable VFS generation in
the index permits replay after VFS publication but before index processing.
Closed external edits become Reconcile revisions. Deleted documents are retired,
retaining their authority history; recreating a path receives fresh identities.
`rebuild_index` first refreshes the VFS, then projects current disk bytes.

While a session is open, queries reflect accepted unsaved edits and merges.
Reopen/rebuild uses disk for the metadata projection; accepted unsaved text
remains recoverable in the durable authority. This distinction is checked by
the kill tests.

`plan_rename(from, to)` is pure computation. `RenamePlan` contains per-document
UTF-8 `TextEdit`s, content hash, authority id and expected revision.
`apply_text_edits` checks bounds/overlap and applies edits backwards. The plan
uses current authority text, includes incoming links and relative outgoing
links affected by moving the source, preserves explicit labels/fragments/embed
markers, and applies `VaultSettings::link_to` with the configured format.
Canvas file nodes and Markdown text nodes are included. `RawValue` locates JSON
string tokens so unrelated JSON formatting/unknown fields survive. YAML scalar
spans come from `granit-parser`; affected strings are safely quoted, preserving
surrounding comments. Single quotes are retained when possible; an apostrophe
can change that scalar to double quotes. Folded/plain scalar formatting may
change to a quoted string while preserving its decoded value.

No public rename-commit method exists yet. The upstream transaction must validate
all guarded heads/hashes, move the file and apply the edits atomically, then
publish the resulting index/id changes. External OS moves currently rebind the
existing durable authority identity and reopen an active session under its new
key; retaining the active session's undo stack across that rekey still needs the
upstream session/rename operation. Rename planning is not transaction acceptance.

## RedCore schema and query behavior

| Record | Identity / contents |
|---|---|
| `NotesFile` | `file:<vault path>`; stable `vf_<ULID>` property for source documents, kind, exclusion flag, hash, parsed note, resolved links, property types, word/character counts, parse error |
| `NotesFolder` | `folder:<vault path>`; includes empty folders, found using locked `walkdir` on mount/rescan |
| `Heading`, `Block`, `Tag`, `Task`, `Footnote`, `InlineField`, `Alias`, `Property` | `item:<path>:<ordinal>`; parser records and UTF-8 source spans; typed property values |
| `Link`, `Embed`, `UnresolvedLink` | Link syntax, target, fragment, label, source spans, resolution, optional canvas node id |
| `CONTAINS` | File to metadata item |
| `LINKS_TO`, `EMBEDS` | One file-to-file edge per resolved occurrence, retaining link metadata |
| `NotesMeta` | `notes:meta`; completed VFS generation |

The complete file projection is one atomic graph mutation batch. Ordinary content
changes parse and rewrite only changed records. A file-set change re-resolves
cached links because duplicate basenames can alter another note's resolution;
only changed projections are published. An edit to a target does not delete its
incoming graph edges. Folder updates and the replay checkpoint follow the file
projection; no fresh per-file scanner or watcher was introduced.

`VaultIndex` provides files, folders, outgoing links, backlinks with counts,
unresolved links with counts, orphans, dead ends, tags with counts, typed
properties with counts, tasks, outline and word count. `resolved_links` and
`unresolved_links` return the source-path → target → count maps. The stored `Note`
provides blocks, aliases, footnotes and inline fields to later panes. Search and
`rustyred-thg-find` were not added.

Excluded files remain resolvable and present in these metadata queries, with
`IndexedFile.excluded` for search/graph/unlinked-mention filtering and lower quick
switcher ranking. [Obsidian Settings](https://obsidian.md/help/settings#Excluded%20files)
lists those filtered views; the [CLI documentation](https://obsidian.md/help/cli)
does not specify exclusion filtering for the metadata commands in this step.
This is the documented behavior used here, not a claim of measured CLI parity.

## Obsidian comparisons

`fixture_vault_matches_obsidian_recorded_groups` requires:

`crates/notes-fixtures/reference/fixture-vault/{vault,links,outline,properties,tags,tasks,wordcount,eval}.json`

All eight were absent. The test fails naming the directory and every missing
file. **No group has been verified to match; no actual reference differences can
be reported before the recordings exist.** Search, bases and dom are excluded as
requested. The `vault` group's bases file listing is still compared.

Once supplied, the test evaluates the recorded command/argument pairs and writes
all mismatches, expected/actual payloads and scope explanations to
`target/notes-reference-differences.json`. Unsupported shapes fail rather than
being skipped. The CLI page does not document its JSON field layouts; adapters
for those layouts are necessarily unverified until that run. There are no
approved difference waivers.

The eval comparison covers `resolvedLinks`, `unresolvedLinks`, file count, and
metadata headings, blocks, links/embeds and original text/positions, tags,
frontmatter and its position, frontmatter link targets, task list items,
footnote definitions and references. Position conversion uses `Offsets` and
Obsidian's zero-based UTF-16 locations. The metadata fields were checked against
[Obsidian's API definitions](https://github.com/obsidianmd/obsidian-api/blob/master/obsidian.d.ts)
through Context7. Not compared:

- `sections` and `referenceLinks` definitions: no corresponding complete parser records.
- Non-task `listItems`, list parent/id: the index models tasks, not every list item.
- `frontmatterLinks.key/displayText/original`: the existing model has no property-key source attribution.
- Generated `links/embeds.displayText`: the parser stores source aliases, not generated display labels.
- Application/plugin state commands in `vault` (`vault`, enabled plugins,
  bookmarks, templates, enabled snippets): outside this headless index's modeled
  state, with a per-command omission reason.

Known unverified semantics: word counting currently splits raw source on Unicode
whitespace; character count is UTF-16 code units. This is not an Obsidian word-count
parity claim. CLI count/shape semantics, exact cache spans, escaped YAML link
spellings, and rename format normalization require the oracle to resolve their
differences. Rename tests prove this implementation's rewrites and documented
link settings, not undocumented Obsidian rewrite details.

## Validation and G6

All commands ran from the worktree root with `--offline`, `CARGO_TARGET_DIR=target`,
`CARGO_INCREMENTAL=0`, and `RUSTC_WRAPPER` unset. `TMPDIR="$PWD/target/tmp"` was
used. Only `frame-notes` and `notes-fixtures` were built; never `frame`/Servo.

```sh
cargo fmt -p frame-notes
env -u RUSTC_WRAPPER CARGO_TARGET_DIR=target CARGO_INCREMENTAL=0 TMPDIR="$PWD/target/tmp" cargo clippy --offline -p frame-notes --all-targets -- -D warnings
env -u RUSTC_WRAPPER CARGO_TARGET_DIR=target CARGO_INCREMENTAL=0 TMPDIR="$PWD/target/tmp" cargo test --offline -p frame-notes --no-fail-fast
```

`--no-fail-fast` ensures the intentionally failing reference test does not prevent
safety tests from running. Final receipts and counts are appended below.

The SIGKILL tests re-execute the integration-test binary. Each runs at three kill
delays (0, 3 and 15 ms), checks SIGKILL, reopens the stores, verifies complete disk
text, and compares the recovered index with a disk rebuild. The save child
acknowledges an accepted durable edit before entering repeated saves; that text
must still be in the authority after kill even if disk still has the old version.
The index child writes externally before refresh/rebuild and is killed during
that work. These exercise process termination, not power-loss simulation.

`g6_network_denied_mount_index_edit_save_reconcile` invokes:

```text
/usr/bin/sandbox-exec -p '(version 1) (allow default) (deny network*)' <test-binary> --exact child_worker --nocapture
```

The child exercises mount/index/edit/merge/save and checks a loopback socket bind
is denied. Inside Codex's sandbox macOS refuses the nested sandbox before the
child runs (`sandbox_apply: Operation not permitted`). Run outside it, with the
default `TMPDIR`, the test passed.

Source audit of the current used paths found no network/AI startup:
`IdeRuntime::new` initializes empty provider/session state and no language server
or extension host; `sync_language_document` returns immediately with no server.
`RedCoreAuthority::open` opens the local fsynced graph. `RedCoreGraphStore` acquires
a directory lock and local journal; no hook emitter/dispatcher is installed.
`DiskObjectStore::open` opens the local chunk journal. `Vfs::open_in` starts only
the local notify/FSEvents watcher. Vault calls no provider registration, language
server attachment, extension activation, ingest pipeline, S3 publisher or model
operation. This source audit is distinct from the blocked dynamic G6 test.

The sandbox does not deliver FSEvents. All integration tests use `refresh` or
explicit events. Real watcher acceptance remains the upstream
`fsevents_delivers_create_modify_rename_and_delete_hints` test, plus Frame's later
UI event-pump integration; neither is claimed here.

## Measurements

Machine: **Apple M1 Max, 32 GB** (`sysctl -n machdep.cpu.brand_string` prints
`Apple M1 Max`; `sysctl -n hw.memsize` prints `34359738368`).

Generated exactly the requested workload:

```sh
env -u RUSTC_WRAPPER CARGO_TARGET_DIR=target CARGO_INCREMENTAL=0 TMPDIR="$PWD/target/tmp" cargo run --offline -p notes-fixtures --release -- generate --notes 10000 --seed 1 --out target/notes-bench-vault
env -u RUSTC_WRAPPER CARGO_TARGET_DIR=target CARGO_INCREMENTAL=0 TMPDIR="$PWD/target/tmp" cargo run --offline -p frame-notes --example vault_bench -- target/notes-bench-vault target/notes-bench-data
```

Generator: 10,000 notes (40 large), 10 canvases, 2 bases, 6 attachments,
25,435,456 bytes. Digest:
`b15bc4d01472f8f6364725a5e8dfb6ed8d7494257ddfb6cee65b97dda42461dd`.
Generation/build command completed; generator release build took 11.57 seconds.
The vault benchmark uses the workspace dev profile (dependencies optimized at
level 2; the notes crate is unoptimized).

**10,000-note attempt did not finish.** Last pre-interruption elapsed sample:
1,808.81 seconds. It was still in VFS mounting before authority/index population.
At interruption the stores contained:

| Store | Bytes |
|---|---:|
| VFS | 21,640,007,467 |
| Blobs | 14,031,804 |
| Authority | 0 |
| Index | not created |

Cold total, warm reopen, peak RSS, final authority size and final index size are
**unavailable for 10,000 notes**. The run was interrupted (exit 130) because of
this excessive write amplification, not recorded as a successful benchmark.
The incomplete disposable data directory was removed after recording its sizes;
the deterministic generated source vault remains in `target/notes-bench-vault`.

The pinned VFS's `scan_root` persists a blob-reference node for each scanned file;
`mount` persists each baseline record separately. Every upsert is a RedCore
commit. `refresh` also persists blob references for unchanged files. Those are
upstream paths (`rustyred-thg-vfs/src/lib.rs`, `scan_root`, `persist_blob_ref`,
`mount`), not index-parser work. The final wrapper avoids an unnecessary immediate
second scan on a new mount; the interrupted draft run had not reached that second
scan, so this refinement does not explain away the observed initial-mount cost.
A bulk mount/unchanged-write optimization in the pinned dependency and another
complete measurement are required. No durability relaxation or replacement store
was introduced to make the measurement look better.

Supplemental **92-file fixture** run of the final benchmark example (68 notes,
4 canvases, 3 bases, 17 attachments), on a copied vault, not the tracked fixtures:

```sh
env -u RUSTC_WRAPPER CARGO_TARGET_DIR=target CARGO_INCREMENTAL=0 TMPDIR="$PWD/target/tmp" cargo run --offline -p frame-notes --example vault_bench -- target/notes-bench-small-vault target/notes-bench-small-data
```

| Metric | Result |
|---|---:|
| Cold mount + authority + index | 25.836236 s |
| Warm reopen | 3.082374 s |
| Peak RSS (`getrusage`, macOS bytes) | 136,658,944 |
| Authority on disk | 5,589,287 bytes |
| Index on disk | 2,377,510 bytes |
| VFS on disk | 15,833,852 bytes |
| Blobs on disk | 133,208 bytes |

These small-vault numbers do not substitute for the requested large-vault result.
The example reports peak RSS for both opens in the same process and logical file
sizes summed recursively, not allocated filesystem blocks. Cold here means a
fresh application data directory, not an OS disk-cache flush.

## Dependency and hand-written code choices

All added direct dependencies were already locked and built offline:
`rustyred-thg-core`, `libc`, `walkdir`, and `granit-parser 1.3.0` (already used by
`serde-saphyr`). `serde_json` enables its existing `raw_value` feature. No package
version changed and no new lockfile package was introduced.

Reused: comrak and the existing AST pass; `FileIndex`; `VaultSettings::link_to`;
Loro sessions and merge through `IdeRuntime`; durable `RedCoreAuthority` and
`RedCoreGraphStore`; VFS events/history/atomic writes; `DiskObjectStore`; serde
JSON/YAML; granit's scalar source spans; tempfile's atomic replacement; walkdir.

The new code is vault orchestration, metadata projection/query mapping and rename
edit planning. The small `SharedAuthority` adapter forwards to one mutex-protected
`RedCoreAuthority` because the runtime has no mutable authority accessor for
external deletion/rename observation. It is not a second authority or store.
No custom Markdown/YAML/JSON parser, watcher, storage engine or merge algorithm was
written. No search lane, GPUI dependency, network client or AI provider was added.

## Final test receipts

- `cargo clippy --offline -p frame-notes --all-targets -- -D warnings`: passed.
  Three existing dead-code warnings come from `rustyred-thg-core`; none from
  frame-notes.
- `cargo fmt -p frame-notes --check` and `git diff --check`: passed.
- Full `cargo test --offline -p frame-notes --no-fail-fast`, run outside Codex's
  sandbox with the default `TMPDIR`: 92 unit, 19 core, 4 safety (including G6)
  and 3 vault integration tests passed; only the reference test failed, because
  the recordings are absent. Safety run: 74.13 s.
- That run first failed 16 core tests and both kill tests: the default `TMPDIR`
  is under `/var`, which the VFS names by that alias while `Vault::open` had
  canonicalized the root to `/private/var`, so every snapshot path fell outside
  the vault and nothing was indexed. The vault now takes its root from
  `Vfs::root_path` after mounting. Codex's runs had passed only because its
  `TMPDIR` was inside the worktree.

New core integration test names (`tests/core.rs`, 19):

- `durable_sessions_save_undo_redo_and_profile_settings`
- `external_disjoint_merge_preserves_undo_and_overlapping_edits_conflict`
- `closed_external_change_is_a_reconcile_revision`
- `canvas_and_base_invalid_edits_and_external_merges_are_refused`
- `index_queries_hidden_exclusions_and_incremental_resolution`
- `explicit_move_preserves_id`
- `rename_plan_rewrites_aliases_fragments_embeds_markdown_properties_and_canvas`
- `rename_moved_source_rewrites_relative_outgoing_links_and_can_be_disabled`
- `fixture_vault_indexes_and_reopens`
- `deletion_recreation_gets_fresh_note_and_authority_identity`
- `rename_plan_includes_unsaved_authority_links_and_revision_guards`
- `empty_folders_are_durable_and_refresh_removes_deleted_folders`
- `open_reports_conflict_and_invalid_external_canvas_does_not_prevent_mount`
- `repeated_move_event_keeps_id_and_authority_identity`
- `replay_after_vfs_publication_before_index_keeps_move_identity`
- `invalid_json_after_disjoint_merge_is_refused_without_a_revision`
- `redcore_incoming_edges_survive_target_edits_and_reopen`
- `rename_quotes_yaml_scalar_values_without_changing_comments`
- `rebuilding_observes_new_and_removed_files_without_a_prior_refresh`

Safety test names (`tests/safety.rs`, 4):

- `child_worker` — helper; re-executed by the other tests.
- `save_survives_sigkill_without_truncation_or_losing_accepted_edit` — passed.
- `index_update_survives_sigkill_and_repairs_from_disk` — passed.
- `g6_network_denied_mount_index_edit_save_reconcile` — passed outside Codex's sandbox; inside it the nested sandbox is refused.

Reference test (`tests/reference.rs`, 1):

- `fixture_vault_matches_obsidian_recorded_groups` — failed, all eight recording files absent.

Next concrete acceptance steps: supply the reference recordings and fix/classify
the real diffs; provide the runtime rename/session-rekey operation; run the
FSEvents path with a real watcher; repair upstream bulk VFS mounting/unchanged
graph writes and rerun the generated 10,000-note benchmark. None was replaced with a
mock, compilation result, or smaller-vault acceptance claim.
