# Notes index parity evidence, 2026-10-26

The reference gate passes: **537 recorded calls matched, zero differences**, seven unchanged
scope omissions. The fixture exercises one permitted load-order tie with two derived resolution
variants. The full suite and clippy pass when run outside the Codex sandbox; see the Rust
results below.

The fixture vault, recordings, `omitted()` scope and machine-local `.cargo/config.toml` were not
changed. Obsidian was not run and its profile was not accessed. Shared dependencies were read
through `deps/`; no edits or Git commands were made in their checkouts. Cargo ran from this
worktree with the authorized local target override.

## Difference counts

The original **432** differences are the user-supplied baseline. Shape and word-count changes
preceded restoration of the dependencies, so that baseline could not be reproduced here. The
first executable Rust reference run in this continuation found **29** differences. Its complete
payload is retained in `target/codex/tmp/notes-reference-before.json`; the final Rust report is
`target/codex/tmp/notes-reference-differences.json`. Counts below are failing recorded calls,
not individual fields. A metadata call can contain several rule differences.

`parity::` names refer to `crates/notes/tests/parity.rs`; `reference::` names refer to
`crates/notes/tests/reference.rs`, including its `adapter_regressions` module.

| Difference class | First executable Rust run | Final Rust run | Fix and regression coverage |
| --- | ---: | ---: | --- |
| Links, backlinks, dead ends, unresolved names | 11 | 0 | Comment/footnote/property links, canvas backgrounds, folder-first resolution, unresolved name stem; `parity::comments_index_links_and_tags_but_not_code_or_math`, `footnotes_keep_unused_definitions_and_index_inline_content`, `property_links_use_decoded_whole_scalar_values_not_yaml_keys_or_prose`, `resolution_prefers_source_folder_then_utf16_path_length`, `unresolved_names_drop_md_but_source_links_preserve_it`, `canvas_backgrounds_are_file_links`, `deadends_include_files_with_only_unresolved_links` |
| Tags | 8 | 0 | Unicode grammar, whole frontmatter strings, parents, written case, exclusions; `parity::tags_accept_obsidian_unicode_and_slash_forms_with_exact_spans`, `frontmatter_tags_are_whole_written_strings`, `vault_tags_expand_parents_merge_case_and_skip_exclusions` |
| Tasks | 6 | 0 | Reject two-unit status; non-space completion; `parity::tasks_reject_surrogate_pairs_and_exclude_nested_lists_from_spans`, `done_includes_every_status_except_a_space` |
| Property summary | 1 | 0 | Assigned types, latest non-null inference, unknown nested objects, exclusions; `parity::properties_use_assigned_types_unknown_objects_and_latest_nonnull_values` |
| Eval metadata cache and two link maps | 3 | 0 | Field corrections detailed below; entire modeled cache and both maps compared by `reference::fixture_vault_matches_obsidian_recorded_groups` |
| Word counts | 0 (already fixed; original report: 67/68 notes) | 0 | Source counting after leading frontmatter removal; `parity::wordcount_matches_every_recorded_note`, `wordcount_excludes_only_closed_leading_frontmatter_and_counts_source` |
| CLI shapes, outline, vault listings | 0 (already fixed; included in supplied 432) | 0 | Typed-value formatting, with the adapter regressions below |
| **Total** | **29** | **0** | **537 calls matched** |

Final matched calls by recording group: vault 4, links 142, outline 68, properties 139, tags 71,
tasks 41, wordcount 68, eval 4. No new waiver or metadata projection omission was added.

### Adapter shapes and tests

These changes render Frame's typed values; they do not normalize away comparison failures.

| Shape rule | Recording evidence | Regression in `reference::adapter_regressions` |
| --- | --- | --- |
| String counts; backlinks use `file` | `links.json`, `tags.json` | `counts_use_strings_and_backlinks_use_file` |
| Empty listing sentences | Empty calls across the groups | `empty_listings_use_the_recorded_sentences` |
| Outline carries numeric `line` | `outline.json` | `outline_lines_are_numbers` |
| Tasks use `file`, string `line`, raw trimmed source line, numbered and quote markers | `tasks.json`; CLI handler at bundle byte 2415838 | `task_rows_keep_raw_lines_and_string_line_numbers` |
| Verbose tasks grouped by file, tab-separated line/text | `tasks.json` | `task_verbose_groups_lines_by_file` |
| Word count uses two labeled lines | `wordcount.json` | `wordcount_uses_labeled_lines` |
| Root folder is `/` | `vault.json` | `folders_include_the_root` |
| Verbose unresolved includes sources; outgoing missing targets carry suffix | `links.json`; handlers near 2433022 and 4264091 | `unresolved_verbose_includes_sources_and_links_have_a_suffix` |
| Verbose aliases are alias/tab/path lines | `properties.json`; handler near 2434519 | `aliases_verbose_uses_tab_separated_lines` |
| Lowercase Obsidian property names; property summary counts stay numeric | `properties.json` | `property_names_use_obsidian_spelling`, `property_table_counts_remain_numbers` |

The committed recording resolves two details differently from the brief's examples: outline
line numbers and property summary counts are numeric, and absent/empty/broken frontmatter
prints `No frontmatter found.` (no parsed JSON field), rather than `[]`. The adapter follows the
recorded stdout. This is not a claim that the recording is wrong; it was left unchanged.

## Rule evidence and implementation

Primary evidence is the committed JSON under `crates/notes-fixtures/reference/fixture-vault/`.
Its provenance and normalization are documented in `crates/notes-fixtures/README.md` and
`crates/notes-fixtures/src/normalize.rs`. Bundle offsets below are byte offsets in the read-only
`/Applications/Obsidian.app/Contents/Resources/obsidian.asar`, not extracted JavaScript offsets.

1. **Word count.** `gre` near 4557997 removes closed leading frontmatter through `pm` near
   1539690, regardless of YAML validity. `mre` counts source with the pinned `WI` (2215696) and
   `jI` (2219808) character classes, numeric punctuation groups, and hyphen/apostrophe runs.
   JavaScript character counts are UTF-16 units. The BOM fixture establishes stripping the
   initial BOM before frontmatter recognition. No Markdown rendering or generic whitespace
   split is involved. The two word-count regressions above execute Rust, including every one
   of the 68 recorded pairs and independent closed/broken/unclosed/BOM/Unicode cases.
2. **Body tags.** `rm` near 1524107 excludes punctuation ranges instead of limiting tags to
   alphanumerics; its locator requires start or preceding whitespace and rejects pure ASCII
   digits. This admits numeric slash tags, middle dots, accents and astral characters.
   `tagging/Unicode Tags.md` and `tagging/Tags.md` establish the exact names and positions,
   including the trailing slash preserved in per-note metadata. Covered by the Unicode/span
   regression above.
3. **Frontmatter and aggregate tags.** `mm`/`ym`/`bm` near 1539690 recognize the case-insensitive
   `tags` key, only string entries, with a scalar treated as one tag and strings with spaces
   rejected. `getTags` near 2519347 skips excluded files, recursively includes parents, counts
   written spellings, then folds case using the most frequent spelling. The three tagging
   fixtures plus `ignored/` and the global calls distinguish these steps. Frame keeps written
   case in note metadata and uses deterministic ordering for equal spelling counts. The
   frontmatter and aggregate regressions above exercise these rules separately.
4. **Comments and footnotes contribute links/tags.** The recorded backlinks of `Dead End.md`
   include `syntax/Comments and Highlights.md` and `syntax/Footnotes.md`. Their metadata records
   include comment tags and links in unused footnote definitions and inline footnote contents.
   Comrak already models comment nodes and offers `leave_footnote_definitions`; enabling that
   option retains unused definitions in document order. Opaque comments and synthetic inline
   footnotes reuse Comrak on their inner text, shifting positions to original source bytes.
   The metadata builder `Im` near 1543208 supplies inline definition IDs and inner-content
   spans. Both focused comment/footnote regressions check values and positions.
5. **Frontmatter links.** `Pm` near 1542283 recursively visits decoded scalar values and
   recognizes whole wiki/Markdown link strings, not YAML keys, prose containing links, or
   embeds. The recorded `Link Properties` backlink to `Blocks Target` and the second
   `All Property Types` backlink to `Headings Target` establish the count change. The existing
   YAML event parser supplies decoded scalar values and original spans; existing Comrak
   supplies link parsing. `property_links_use_decoded_whole_scalar_values_not_yaml_keys_or_prose`
   checks nested arrays, mapping keys, prose and embeds.
6. **Resolution.** `getLinkpathDest` near 2520109 compares paths case-insensitively, checks exact
   paths, then partitions suffix candidates under the source folder before other candidates.
   `RR` near 2514863 compares JavaScript `path.length`, hence UTF-16 units, rather than folder
   depth. The recorded `b/Links From B.md` backlinks split two each between the two Note files.
   `resolution_prefers_source_folder_then_utf16_path_length` and
   `resolution_uses_utf16_length_and_exact_vault_paths_before_local_suffixes` cover folder
   priority, nested folders, case, UTF-16 length, exact root paths and explicit relative paths.
7. **Unresolved names.** `links.json` and eval's `unresolvedLinks` use names without trailing
   `.md`; the per-note outgoing CLI still prints the authored target plus its unresolved
   suffix. The crate strips the summary name while retaining `Link.target` and its span.
   Covered by `unresolved_names_drop_md_but_source_links_preserve_it` and the adapter regression.
8. **Canvas and dead ends.** Eval's `resolvedLinks` records `canvases/Groups.canvas` linking to
   its background image. The existing canvas parser now handles group backgrounds as file
   links. The CLI near 2434519 defines a dead end by no resolved outgoing keys, so notes with
   only missing targets remain dead ends. The canvas and dead-end regressions above cover both.
9. **Properties.** `XR` near 2537441 infers numbers, booleans, text, strict ASCII date and
   minute-precision `T` datetime forms, string-or-null lists, and unknown objects/other lists.
   Assigned types from `.obsidian/types.json` take precedence. Cache update/rebuild logic near
   2540292 updates an unassigned shared name from non-null values. Frame replays files in its
   deterministic order; the template's authored `created` string makes the fixture-wide type
   text, while individual date values still infer date. Excluded files do not add occurrences.
   `properties_use_assigned_types_unknown_objects_and_latest_nonnull_values` covers declarations,
   the shared-name conflict, null preservation, exclusions, lists/objects and strict date forms.
10. **Tasks.** The CLI near 2415838 treats every status except space as done, with explicit status
    taking precedence. Metadata accepts a one-UTF-16-unit marker, rejecting an astral status.
    `Im` near 1543208 trims trailing nested lists from list-item positions; the recorded
    `tagging/Tags.md` task includes an adjacent lazy quote continuation. Crate spans implement
    both rules. The task regressions above plus
    `tasks_include_adjacent_lazy_quote_continuations` cover them.
11. **Multiple embed dividers and URI escapes.** `embeds/Embed Images.md` records a size/caption
    multi-divider embed that Comrak returns as plain text; its target now goes through the
    existing wiki-link splitter. `review/Heading With Hash.md` preserves `%23` in its fragment,
    consistent with the bundle's `decodeURI` behavior (reserved URI escapes stay escaped).
    Existing percent decoding is retained with reserved escapes protected. Both are checked
    by `multiple_embed_dividers_and_reserved_uri_escapes_are_preserved`.
12. **BOM and invalid frontmatter cache shape.** The recorded `syntax/Byte Order Mark.md` offsets
    omit the BOM, while Frame's byte spans retain the original source for safe edits. The
    metadata adapter subtracts that leading UTF-16 unit. Broken frontmatter remains available
    as a typed parse error in Frame but has no frontmatter value or position in the CLI cache.
    `reference::metadata_uses_bomless_utf16_coordinates_and_omits_invalid_frontmatter_values`
    checks both representations.

Comrak's existing option was verified against its documentation:
[Parse::leave_footnote_definitions](https://docs.rs/comrak/latest/comrak/options/struct.Parse.html#structfield.leave_footnote_definitions).
No dependency was added. Custom logic is limited to rules not represented by the upstream AST:
the pinned source-count character classes, tag recognition, Obsidian resolution/property
aggregation, exact span adjustments, reserved URI behavior and multi-divider wiki fallback.
Comments, inline footnotes and decoded YAML link values reuse the existing parsers rather than
introducing a new Markdown or YAML parser.

## Field-by-field metadata check

The first Rust cache diff contained 17 differing note/field pairs. Every pair is now equal in
the strict modeled-cache comparison; arrays retain source order and all modeled entries remain.

| Field | Before pairs | After | Correction and focused coverage |
| --- | ---: | ---: | --- |
| `embeds` | 1 | 0 | Multi-divider embed; `multiple_embed_dividers_and_reserved_uri_escapes_are_preserved` |
| `frontmatterLinks` | 2 | 0 | Decoded whole scalar links; `property_links_use_decoded_whole_scalar_values_not_yaml_keys_or_prose` |
| `frontmatterPosition` | 2 | 0 | Invalid YAML omission and BOM offset; metadata adapter regression |
| `links` | 4 | 0 | Comments, footnotes, BOM and reserved URI fragment; corresponding focused regressions |
| `headings` | 1 | 0 | BOM offset; metadata adapter regression |
| `tags` | 3 | 0 | Comments, raw trailing slash and Unicode grammar; comment and Unicode regressions |
| `footnoteRefs` | 1 | 0 | Inline footnote is a definition; footnote regression |
| `footnotes` | 1 | 0 | Unused definition, inline ID and inner span; footnote regression |
| `listItems` | 2 | 0 | Status width, trailing nested lists and adjacent quote; task span regressions |

The two separate eval maps now also match. `resolvedLinks` includes frontmatter, comment,
footnote, multi-divider embed and canvas background edges with corrected folder resolution;
`unresolvedLinks` uses the stripped names. Existing omissions remain: sections, separate
reference definitions, non-task list entries/id/parent, property-link key/display/original, and
rendered link display labels. They were not expanded by this change.

## The only accepted ambiguity

`FileIndex::resolution_candidates` is shared by resolution and the acceptance derivation.
The test enumerates only equal-shortest-length suffix candidates when none is under the linking
note's folder. Exact paths, explicit relative matches, Path links, local candidates and unequal
lengths cannot become accepted ties. Each eligible occurrence can move one contribution between
candidate buckets; every fixed edge and count remains fixed. Acceptance applies only to the
resolved-links eval and the corresponding backlink output. Before accepting a variant, the test
also checks that Frame's actual backlink answer agrees with its own resolved map.

The fixture must contain at least one eligible tie. It currently contains one, giving two
variants. `load_order_acceptance_moves_only_derived_tied_occurrences` tests the alternate answer
and rejects loss of an unrelated fixed edge; `local_or_unequal_candidates_are_not_accepted_ties`
tests the boundaries. No fixture path list is hard-coded in the acceptance. The committed
recording and Frame both select `a/Note.md` for the fixture's tie, so this run needs no residual
value mismatch. This preserves the sole acceptance in `docs/plans/notes-app.md`.

## Real Rust validation

All commands ran from `/Volumes/SSD Samsung/frame-worktrees/notes-parity`. The local target
override preserves `.cargo/config.toml`. Logs and full reports are gitignored build evidence.

```sh
CARGO_TARGET_DIR="/Volumes/SSD Samsung/frame-worktrees/notes-parity/target/codex" cargo test --offline -p frame-notes --test reference > target/codex/tmp/notes-parity-reference.log 2>&1
CARGO_TARGET_DIR="/Volumes/SSD Samsung/frame-worktrees/notes-parity/target/codex" cargo test --offline -p frame-notes > target/codex/tmp/notes-parity-full.log 2>&1
CARGO_TARGET_DIR="/Volumes/SSD Samsung/frame-worktrees/notes-parity/target/codex" cargo clippy --offline -p frame-notes --all-targets -- -D warnings > target/codex/tmp/notes-parity-clippy.log 2>&1
```

- Dedicated reference: exit 0, **15 tests passed**, 537 recorded calls matched, zero differences;
  test runtime 7.70 seconds (build 19.71 seconds).
- Full suite, inside the Codex sandbox: exit 101, **150 passed, 1 failed** before Cargo stopped.
  `g6_network_denied_mount_index_edit_save_reconcile` (`crates/notes/tests/safety.rs:158`)
  could not apply its nested macOS sandbox: `sandbox-exec: sandbox_apply: Operation not
  permitted`. The test was not skipped, weakened or replaced.
- Full suite, the same command run outside the Codex sandbox on the final edits: exit 0,
  **154 passed, 0 failed**. That is 95 unit, 19 core, 18 parity, 15 reference, 4 safety
  (including G6, 154.66 seconds) and 3 vault tests; 3 minutes 24 seconds in all.
- Clippy, the same command run outside the Codex sandbox on the final edits: exit 0, no
  `frame-notes` diagnostics; 26 seconds. Three existing dependency warnings, untouched:
  `vector_index.rs:467` (`full_builds` and `index_bytes`), `symbolic.rs:314`
  (`object_field`), and `versioned_graph.rs:5522`
  (`hydrate_in_memory_graph_from_payload_index`) in shared `rustyred-thg-core`.
- A throwaway probe checked the new comment and inline-footnote span arithmetic. Lone and
  unclosed markers (`%%`, `%%%`, `%%%%`, `a %%`, `^[`, `^[]`, `[^]`, `- %%`, `> %%%` and
  others) parse without a panic. The probe file was removed.

During iteration the first executable reference failed with 29 payload differences and one
adapter-test expectation error. One compile error in the YAML event integration was corrected.
Full-suite attempts then exposed old expectations for superseded semantics (11 unit assertions,
then one integration tag-case assertion) and a new test's accidental case-insensitive self-link;
these were corrected to the evidenced rules. A temporary Comrak AST probe passed and was removed.
The final reference and focused suites above are real Rust results, not the earlier Python
source-rule cross-check. No test was ignored or substituted to bypass the safety gate.

The temporary diagnostic command was
`CARGO_TARGET_DIR="/Volumes/SSD Samsung/frame-worktrees/notes-parity/target/codex" cargo test --offline -p frame-notes --test ast_probe -- --nocapture`
(exit 0, one test). It only inspected Comrak nodes; it is not a substitute for any gate.
Scoped Rust formatting and `git diff --check` passed during implementation. No dependency,
fixture, recording, config or safety-test changes were made to accommodate the environment.

There are no outstanding recorded index differences, and the full-suite and clippy gates pass.
