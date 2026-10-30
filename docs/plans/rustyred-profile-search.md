# RustyRed profile storage and address-bar search

Requested 2026-10-29. Implement browser-owned persistence using the supported
RustyRedDB API at 708d22ef5da2801925ec0b1506b40b4aafb0e364, typed address-bar
results, and Servo HTTP disk caching. Preserve unrelated checkout changes.

## Plan and acceptance

1. Replace the profile catalog and profile SQLite writes with a shared,
   long-lived RustyRed owner. Atomically migrate existing catalogs and records;
   retain SQLite only as an old-format/import reader. Preserve profile paths,
   Keychain namespaces, tab layouts, import deduplication and timestamps.
2. Use explicit title/URL full-text indexes, without revision history or control
   events. Verify clearing visits removes query/index results across reopen.
3. Address-bar results distinguish Visit, Search the web, Bookmark, History and
   Switch to tab. Reuse the native Command component and existing submitted web
   search pipeline; perform persisted suggestion lookups off the UI executor.
   Discard stale responses and freeze rows once keyboard selection starts.
4. Enable Servo's per-profile HTTP disk cache; retain its existing website
   storage and shutdown lifecycle. Run synthetic fresh-process acceptance for
   cookies, localStorage, IndexedDB and cache reuse, plus native UI rendering.

## Decisions

- Existing pinned engine sources own the Servo API; Context7's Servo reference
  was also consulted. Context7 has no RustyRedDB entry, so inspect the pinned
  supported Rust API and upstream tests directly.
- Use RustyRed's typed mutations and Tantivy-backed full-text API, GPUI's
  background executor and Command widget. No custom database, tokenizer, search
  engine or popup primitive.
- SQLite migration sources remain rollback copies, never authoritative after
  the committed migration marker. Clearing history must address these copies;
  no claim of physical secure erasure on SSDs or external backups.

## Progress

Implementation and local acceptance complete on the original checkout base; publication status is recorded below.

### Implementation decisions and first checks

- `browser.RR` at the installation root holds catalog and profile-scoped records.
  Clones and independently opened stores in this process share one writer;
  RustyRed's file lock excludes a second writing process. Browser profile and
  Keychain paths retain their previous identity.
- Existing SQLite files are consistent backup-API inputs. All profiles, their
  records, active selection and schema marker commit together. An invalid
  session aborts migration without exposing partial imported records.
- Passwords remain in Keychain. The graph contains only origin/account labels.
- Bookmark and History indexes cover titles/URLs only, with title boosts and
  trailing-word prefixes. Literal URL matching supplements tokenization.
  Exact addresses rank first; relevance, visit recency and frequency rank the
  remaining records. Existing web search executes only on submission.
- Suggestions debounce for 100 ms off the UI thread; navigation, cancellation,
  Ask and query changes invalidate the pending task. Keyboard/pointer selection
  freezes rows. History writes run off the UI thread and drain on shutdown.
- Clear History scrubs records into empty tombstones: RustyRed's ordinary
  logical deletion retains the original payload. It also clears the legacy
  SQLite history copy, updates the search index, and compacts the graph. Revision
  history/control events are disabled from initial database creation. A timestamp
  fence prevents a queued older navigation from undoing a clear.
- Servo keeps its own cache and website storage implementations, including their
  internal SQLite formats. `network_http_disk_cache` now names the active profile's
  `http-cache.sqlite3`, using Servo's default 100 MiB limit. Cookies, localStorage
  and IndexedDB retain the existing `config_dir` and orderly shutdown path.
- The application `cargo check --locked --bin frame` passed, with existing
  dependency warnings. Final test/build/native results are recorded below.

### Shutdown cache correction

Source inspection found Servo persists HTTP entries on memory eviction only.
Enabling the path therefore does not preserve the hot cache on an ordinary quit.
The tracked `servo-disk-cache-shutdown.patch` drains completed public entries via
Servo's existing disk-cache API before the resource thread acknowledges exit.
Bootstrap applies both the existing media patch and this patch idempotently.
No in-flight/private resources are added to persistent storage.

The first native invocation could not load GStreamer because two generated plugin
links still named the checkout's old `Frame` path. Those broken links were
repointed to the existing canonical runtime; no installed application was changed.

## Acceptance (2026-10-29)

Evidence: [rustyred-profile-search-2026-10-29](evidence/rustyred-profile-search-2026-10-29/).

- `cargo test --locked -p frame-profile`: 23 passed. The three existing opt-in
  native Keychain/NSS tests were not rerun; ordinary tests retain credential
  isolation, denied-vault and import-retry coverage.
- `cargo clippy --locked -p frame-profile --all-targets --no-deps -- -D warnings`:
  passed on the final storage implementation.
- `script/media-env cargo test --locked --bin frame`: 66 passed, including stale
  lookup cancellation, frozen keyboard selection, switch-to-existing-tab,
  ordinary Enter search, Ask separation and shutdown session draining.
- `script/media-env cargo build --locked --bin frame`: passed for the normal
  executable, without the optional native capture feature. Build output:
  `/Volumes/SSD Samsung/cargo-targets/frame/debug/frame`.
- Native `storage_check` with the production Engine, in three separate processes:
  write, reopen, and isolated profile all passed. Cookies, localStorage,
  IndexedDB, bookmarks/indexes and saved session survived reopen. The server saw
  one cache-resource request across write/reopen and a second request for the
  separate profile, proving both disk reuse and profile separation.
- Native production Browser preview driven through real keystrokes passed and
  was visually inspected: Search, Switch to tab, Bookmark and History rows,
  readable titles and URL subtitles, anchored under the URL bar. The fixture's
  search service is intentionally absent; this is UI acceptance, not a live
  general-web provider test. Submitted searches use the unchanged configured
  RustyWeb providers.
- Both tracked Servo patches were checked idempotently against the pinned
  dependency, preserving the pre-existing media edits. `git diff --check` passed
  in Platinum and the Servo checkout.

The automatic migration has been tested with synthetic legacy databases. No
personal profile was opened by the acceptance fixtures. The first normal launch
of this build migrates existing browser-owned SQLite records; the old files stay
as rollback inputs and are never automatically reimported after migration.
SQLite remains for external-browser import and Servo's internal storage engines.
Clear History removes the legacy history copy and normal graph/index/revision
access; it does not promise physical SSD erasure or removal from external backups.
Only one writing process may open an installation's graph; the offline import CLI
therefore requires the browser to be closed.

No commit, push, installation, or publication was performed. Existing unrelated
checkout changes were preserved. No new worktree was created.


## ub-mode follow-up (2026-10-30)

Installed and applied the requested skill. Split the new storage/migration
responsibilities, centralized profile-directory identity, and shared form import
and allowed-field policy across Chromium, Firefox and explicit filling. Eleven
additional behavioral tests pin previously uncovered migration, ranking,
deduplication and clearing-boundary cases. All 34 profile tests and strict profile
Clippy pass. Every function in the new storage/legacy modules measures below
CRAP 10 with actual LLVM coverage. The native browser was not rebuilt again in
this follow-up because its dependency rebuild started with only 10 GiB of free disk space;
the earlier application/native results above remain separately identified.

See the [quality report and action log](evidence/ub-mode-2026-10-30/README.md) for
measurements, mutation findings, remaining broader quality gates and tool fixes.


## Publication preparation (2026-10-30)

Implementation committed as `f93143a` on
`Travis-Gilbert/rustyred-address-persistence`, preserving unrelated local changes.
The original base is `187c89a`. Refreshed `origin/main` is `b2b3b69`, 113 commits
newer, and a non-mutating `git merge-tree` check reports conflicts in Cargo.lock,
Cargo.toml, crates/profile/src/lib.rs, script/bootstrap-media, src/browser.rs,
src/browser/performance.rs, src/engine.rs and src/session.rs. The PR will be a
draft against main so the implementation and evidence are reviewable. Integrating
the current browser/session architecture and retesting that result remain merge
requirements; the old-checkout checks are not integration acceptance.

The Servo cache patch was reverse-apply checked again. Removed whitespace-only
context markers in the patch file so the staged diff also passes `git diff
--check`; application source and patch behavior did not change.
