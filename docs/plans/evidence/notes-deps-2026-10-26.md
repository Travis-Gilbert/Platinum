# Notes app: Theorem dependencies and the AI-off audit

Date: 2026-10-26. Branch `feat/notes-core`. Theorem checkout: `deps/theorem-rustyweb-pw2` at 6e57dbb (read, never edited).

`crates/notes` (package `frame-notes`) links three Theorem crates by path:

| Crate | Used for (step 2 onward) |
|---|---|
| `theorem-ide-runtime` | `IdeRuntime`, `DocumentAuthority`, Loro document sessions |
| `theorem-ide-authority` | `RedCoreAuthority` (`GraphAuthority<RedCoreGraphStore>`) |
| `rustyred-thg-vfs` | `Vfs` with its `notify` watcher |

Nothing in `frame-notes` calls them yet. They are linked now so the build, the lock and this audit cover them.

## 1. Build and lock

`cargo check -p frame-notes --locked` builds. Measured: 8m29.9s wall on the first (cold) run, which failed, and 11.05s wall on the passing run with a warm target dir.

**Lock skew.** A path dependency's own `Cargo.lock` is ignored; Frame's lock re-resolves everything. 227 packages in the graph resolve to different versions than in Theorem's lock. One of them breaks the build: Frame's lock picked `loro-internal 1.17.1`, `loro-common 1.17.0` and `loro-kv-store 1.17.0` beside `loro 1.13.9` (pinned by `theorem-doc-session` and `theorem-ide-runtime` as `=1.13.9`). The Loro sub-crates are not versioned in lockstep with `loro`, and `loro 1.13.9` does not compile against 1.17 (E0063 and E0308 in `loro`). Fixed in `Cargo.lock` only:

```
cargo update -p loro-internal --precise 1.13.9
cargo update -p loro-kv-store --precise 1.13.9
cargo update -p loro-common   --precise 1.13.1   # only after loro-internal is down
```

These three pins live in the lock, not in any manifest. Any lane that regenerates or merges `Cargo.lock` (and `cargo update` without a package name) can undo them. Whoever merges the lock should re-run `cargo check -p frame-notes --locked`. The durable fix is on Theorem's side (pin the sub-crates in `theorem-doc-session`, or depend on `loro` with a tilde range that also bounds them); not patched here.

Lock size: 1501 packages at HEAD, 1852 now (+351, none removed). Of the 351, 233 are in the `frame-notes` build graph (host and all targets, normal and build edges) and 118 are lock-only: optional features nobody enables (`candle-core`, `tch`/`torch-sys`, `cubecl-*` with CUDA/ROCm/WGPU, `tokenizers`, `tracel-mlir-*`, `comrak 0.39.1` and `ureq 2.12.1` via `tracel-mlir-rs-macros` and `torch-sys`, and similar). They are never compiled.

## 2. `cargo tree --workspace --locked --duplicates`

Measured 3.2s wall, whole Frame workspace (includes `frame`, GPUI and Servo), host target.

Resolve exactly once across the whole graph:

| Family | Resolved |
|---|---|
| Loro | `loro 1.13.9`, `loro-internal 1.13.9`, `loro-common 1.13.1`, `loro-kv-store 1.13.9`, `loro-delta 1.13.0`, `loro-rle 1.6.0`, `loro-thunderdome 0.6.2`, `loro_fractional_index 1.13.0` |
| RedCore | `rustyred-thg-core 0.5.0` (path, one copy) |
| GPUI | `gpui-pre 0.3.9` (vendored) with its 14 `gpui-pre-*` sub-crates, `gpui-base 0.8.0`, `gpui-component 0.8.0`, `gpui-kit 0.8.0` |
| Markdown | `comrak 0.58.0` (the 0.39.1 in the lock is unreachable, see section 1), `pulldown-cmark 0.12.2` |

127 crate names appear in more than one version. 104 of them were already duplicated before this change. 23 are new:

```
addr2line 0.25.1/0.26.1       bincode 1.3.3/2.0.1          bit-set 0.11.1/0.8.0
bit-vec 0.10.1/0.8.0          enum-as-inner 0.5.1/0.6.1    fearless_simd 0.4.1/1.2.0
fixedbitset 0.4.2/0.5.7       fluent-uri 0.1.4/0.4.1       gimli 0.32.3/0.33.0
hash32 0.2.1/0.3.1            heapless 0.7.17/0.8.0/0.9.3  mach2 0.4.3/0.6.0/0.7.0
mio 0.8.11/1.2.4              notify 6.1.1/8.2.0           object 0.37.3/0.38.1
oneshot 0.1.13/0.2.1          petgraph 0.6.5/0.8.3         quick_cache 0.6.24/0.7.0
rand_distr 0.4.3/0.6.0        ropey 1.6.1/2.0.0-beta.1     syn 1.0.109/2.0.119/3.0.9
wasm-encoder 0.245.1/0.262.0  wast 35.0.2/262.0.0
```

The ones that matter:

- **`ropey`**: 1.6.1 is ours and `rustyred-thg-text-model`'s (Theorem's session offsets); 2.0.0-beta.1 belongs to `gpui-base` and `gpui-component`. Different major versions with different APIs; they never meet in one type. `frame-notes` uses 1.6.1.
- **`notify`**: 6.1.1 comes from `rustyred-thg-vfs` (feature `macos_kqueue`), 8.2.0 from `gpui-component`. Two watcher stacks in one process once the app links both.
- **`pulldown-cmark 0.12.2`**: only through `theorem-doc-session` (markdown import). `frame-notes` parses with comrak; Theorem's session crate keeps its own Markdown importer.
- **`wasm-encoder`, `wast`, `gimli`, `object`, `addr2line`, `mach2`**: the Wasm plugin runtime (section 3).
- **`syn 1.0.109`**: a third `syn` major, from the Theorem side.

## 3. What the three crates bring in

37 local path crates are in the `frame-notes` graph (36 plus `frame-notes`). 22 were already in Frame's lock (the root package depends on `theorem-browser-substrate` and `rustyred-web`):

`agpui-contract`, `agpui-shell-protocol`, `compute-offload`, `rustyred-thg-checkpoint`, `rustyred-thg-core`, `rustyred-thg-datawave`, `rustyred-thg-derivation`, `rustyred-thg-graph-lisp`, `rustyred-thg-offload`, `rustyred-thg-reconstruct-fact`, `rustyred-thg-resolve`, `theorem-bayes`, `theorem-block-registry`, `theorem-body-registry`, `theorem-capability-registry`, `theorem-capability-scope`, `theorem-datalog`, `theorem-egraph`, `theorem-harness-core`, `theorem-oplog`, `theorem-plan-saturate`, `theorem-surface-contracts`.

14 are new: `commonplace`, `rustyred-plugin`, `rustyred-thg-ml`, `rustyred-thg-schema`, `rustyred-thg-text-model`, `rustyred-thg-vfs`, `theorem-blocks`, `theorem-doc-session`, `theorem-extract`, `theorem_extract_derive`, `theorem-ide-authority`, `theorem-ide-runtime`, `theorem-oplog-graph-compat`, `theorem-provenance`.

The chains that carry weight:

```
frame-notes -> rustyred-thg-vfs -> commonplace -> theorem-oplog-graph-compat -> theorem-blocks
            -> rustyred-plugin -> extism 1.30.0 -> wasmtime 43.0.2 (+ cranelift 0.130.2, wasi-common, wiggle, pulley, winch)
frame-notes -> rustyred-thg-vfs -> commonplace -> theorem-harness-core (feature "kernel")
            -> rustyred-thg-ml -> burn 0.21.0 (ndarray backend; burn-* core crates), + compute-offload, sqlparser, ed25519-dalek, serde_yaml
theorem-blocks -> rustyred-thg-schema -> theorem-extract -> tree-sitter 0.26.13 (+ javascript, python, rust grammars)
theorem-blocks -> jsonschema 0.46.10
rustyred-thg-core -> object_store 0.12.5 (feature "aws") -> reqwest 0.12.28 -> hyper 1.12.0, hyper-rustls, h2, tokio 1.54.1, rustls
theorem-ide-runtime -> theorem-doc-session -> loro 1.13.9, pulldown-cmark 0.12.2, theorem-block-registry
theorem-ide-runtime -> rustyred-thg-text-model -> ropey 1.6.1
```

So the answer to the three questions in the brief:

- `theorem-doc-session` brings `pulldown-cmark 0.12.2` (default features off) and `loro`.
- `commonplace` brings `theorem-harness-core` with its default feature `kernel` (`theorem-harness-core/Cargo.toml:13-15`; `commonplace/Cargo.toml:21` does not turn defaults off), and through it `rustyred-thg-ml` and `burn`; and, through `theorem-oplog-graph-compat` -> `theorem-blocks` -> `rustyred-plugin`, the Wasm plugin runtime (`extism`, `wasmtime`, `cranelift`). A note-taking VFS links a Wasm runtime and an ML framework it does not call. Worth raising with Theorem: `commonplace` depending on `theorem-harness-core` with `default-features = false` would drop the ML edge (the harness-core comment says `kernel` is the inference stack), and gating the plugin edge in `theorem-blocks` would drop the Wasm runtime. Features are additive, so Frame cannot switch either off from its side.
- A second `ropey` exists (1.6.1 beside GPUI Kit's 2.0.0-beta.1).

The 218 registry crates added to the `frame-notes` build graph, by name and version (the other 118 are the lock-only set above):

```
addr2line 0.26.1, ambient-authority 0.0.2, append-only-bytes 0.1.12, arref 0.1.0, atomic-polyfill 1.0.3, atomic_float 1.1.0, beef 0.5.2, bincode 2.0.1, bit-set 0.8.0, bit-vec 0.8.0, borrow-or-share 0.2.4, burn 0.21.0, burn-autodiff 0.21.0, burn-backend 0.21.0, burn-core 0.21.0, burn-derive 0.21.0, burn-ir 0.21.0, burn-ndarray 0.21.0, burn-nn 0.21.0, burn-optim 0.21.0, burn-std 0.21.0, burn-tensor 0.21.0, bytecount 0.6.9, cap-fs-ext 3.4.6, cap-primitives 3.4.6, cap-rand 3.4.6, cap-std 3.4.6, cap-time-ext 3.4.6, caseless 0.2.2, colored 3.1.1, comrak 0.58.0, cpp_demangle 0.4.5, cranelift-assembler-x64 0.130.2, cranelift-assembler-x64-meta 0.130.2, cranelift-bforest 0.130.2, cranelift-bitset 0.130.2, cranelift-codegen 0.130.2, cranelift-codegen-meta 0.130.2, cranelift-codegen-shared 0.130.2, cranelift-control 0.130.2, cranelift-entity 0.130.2, cranelift-frontend 0.130.2, cranelift-isle 0.130.2, cranelift-native 0.130.2, cranelift-srcgen 0.130.2, critical-section 1.2.0, cubecl-common 0.10.0, cubecl-zspace 0.10.0, dateparser 0.2.1, debugid 0.8.0, derive-new 0.7.0, derive_arbitrary 1.5.0, diff 0.1.13, directories-next 2.0.0, dirs-sys-next 0.1.2, email_address 0.2.9, embassy-futures 0.1.2, embassy-time 0.5.1, embassy-time-driver 0.2.2, embedded-hal 0.2.7, embedded-hal 1.0.0, embedded-hal-async 1.0.0, ensure-cov 0.1.0, entities 1.0.1, enum-as-inner 0.5.1, enum-as-inner 0.6.1, enum_dispatch 0.3.13, extism 1.30.0, extism-convert 1.41.0, extism-convert-macros 1.41.0, extism-manifest 1.41.0, fancy-regex 0.18.0, fd-lock 4.0.4, fearless_simd 1.2.0, fearless_simd_macros 0.1.0, finl_unicode 1.5.0, fixedbitset 0.4.2, fluent-uri 0.4.1, fraction 0.15.4, fs-set-times 0.20.3, fxprof-processed-profile 0.8.1, generator 0.8.11, generic-btree 0.10.7, geo-types 0.7.20, gimli 0.33.0, hash32 0.2.1, heapless 0.7.17, id-arena 2.3.0, im 15.1.0, inotify 0.9.6, interim 0.2.1, io-extras 0.18.4, io-lifetimes 2.0.4, ittapi 0.4.0, ittapi-sys 0.4.0, jsonschema 0.46.10, jsonschema-regex 0.46.10, leb128 0.2.7, leb128fmt 0.1.0, logos 0.15.1, logos-codegen 0.15.1, logos-derive 0.15.1, loom 0.7.2, loro 1.13.9, loro-common 1.13.1, loro-delta 1.13.0, loro-internal 1.13.9, loro-kv-store 1.13.9, loro-rle 1.6.0, loro-thunderdome 0.6.2, loro_fractional_index 1.13.0, macerator 0.3.4, macerator-macros 0.2.0, mach2 0.4.3, manyhow 0.11.4, manyhow-macros 0.11.4, matchers 0.2.0, maybe-owned 0.3.4, md5 0.7.0, memchr-n 0.1.10, memfd 0.6.6, micromap 0.3.0, mio 0.8.11, moddef 0.3.0, nb 0.1.3, nb 1.1.0, ndarray 0.17.2, nonmax 0.5.5, notify 6.1.1, nu-ansi-term 0.50.3, num-cmp 0.1.0, object 0.38.1, oneshot 0.2.1, outref 0.5.2, pathdiff 0.2.3, pest 2.9.4, pest_derive 2.9.4, pest_generator 2.9.4, pest_meta 2.9.4, petgraph 0.6.5, pretty_assertions 1.4.1, proc-macro-utils 0.10.0, prost 0.14.4, prost-derive 0.14.4, psm 0.1.24, pulldown-cmark 0.12.2, pulley-interpreter 43.0.2, pulley-macros 43.0.2, quick_cache 0.6.24, rand_distr 0.6.0, recursive 0.1.1, recursive-proc-macro-impl 0.1.1, referencing 0.46.10, regalloc2 0.15.2, rmp-serde 1.3.1, ropey 1.6.1, rust_decimal 1.43.0, rustix-linux-procfs 0.1.1, rusty-money 0.4.2, serde_columnar 0.3.14, serde_columnar_derive 0.3.7, serde_yaml 0.9.34+deprecated, sharded-slab 0.1.7, sqlparser 0.62.0, sqlparser_derive 0.5.0, stacker 0.1.25, streaming-iterator 0.1.9, syn 1.0.109, system-interface 0.27.3, tracing-log 0.2.0, tracing-subscriber 0.3.23, tree-sitter 0.26.13, tree-sitter-javascript 0.25.0, tree-sitter-language 0.1.9, tree-sitter-python 0.25.0, tree-sitter-rust 0.24.2, tynm 0.2.0, typed-arena 2.0.2, typed-path 0.12.4, ucd-trie 0.1.7, unicode-general-category 1.1.0, unicode-xid 0.2.6, unsafe-libyaml 0.2.11, unty 0.0.4, uuid-simd 0.8.0, vsimd 0.8.0, wasi-common 43.0.2, wasm-compose 0.245.1, wasm-encoder 0.245.1, wasm-encoder 0.262.0, wasmparser 0.245.1, wasmprinter 0.245.1, wasmtime 43.0.2, wasmtime-environ 43.0.2, wasmtime-internal-cache 43.0.2, wasmtime-internal-component-macro 43.0.2, wasmtime-internal-component-util 43.0.2, wasmtime-internal-core 43.0.2, wasmtime-internal-cranelift 43.0.2, wasmtime-internal-fiber 43.0.2, wasmtime-internal-jit-debug 43.0.2, wasmtime-internal-jit-icache-coherence 43.0.2, wasmtime-internal-unwinder 43.0.2, wasmtime-internal-versioned-export-macros 43.0.2, wasmtime-internal-wit-bindgen 43.0.2, wast 35.0.2, wast 262.0.0, wat 1.262.0, wiggle 43.0.2, wiggle-generate 43.0.2, wiggle-macro 43.0.2, winx 0.36.4, wit-parser 0.245.1, witx 0.9.1, wkt 0.10.3, xxhash-rust 0.8.19, yansi 1.0.1, zip 0.6.6
```

The notes core's own direct dependencies (Part B) add six lock entries beyond the Theorem crates: `comrak 0.58.0` with `fearless_simd 1.2.0`, `fearless_simd_macros 0.1.0`, `finl_unicode 1.5.0` and `memchr-n 0.1.10`, plus `pathdiff 0.2.3`. `anyhow`, `indexmap`, `percent-encoding`, `regex`, `ropey`, `serde`, `serde-saphyr`, `serde_json`, `typed-path` and `ulid` were already in the lock or arrived with the Theorem crates. `serde-saphyr 1.3.0` was in Frame's lock before this change, through `rust-i18n-support`, so no new YAML parser was added.

## 4. No GPUI in `frame-notes`

`cargo tree -p frame-notes --locked --target all -e all` contains no GPUI crate. The literal check `cargo tree -p frame-notes | grep -i gpui` is not empty, though: it matches `agpui-contract` and `agpui-shell-protocol`, which are Theorem's renderer-free AGPUI semantic-tree crates (their dependencies are `serde`, `serde_json`, `sha2`, `schemars`, `thiserror`; `agpui-contract/src/lib.rs` says it names no renderer and a boundary test fails the build if one enters). The check that distinguishes them, and prints nothing:

```
cargo tree -p frame-notes --locked --target all -e all | grep -iE '(^|[^a-z])gpui'
```

## 5. Gate G6: does anything start a network client, a Tokio runtime or an AI provider?

Method: read every constructor the notes core will call and every place the linked crates spawn a process, open a socket or build a runtime, in non-test code of the 36 Theorem path crates in the graph.

**Verdict: no.** Constructing and using these crates for documents starts no network client, no Tokio runtime and no AI provider. Several code paths exist that would, each behind an explicit call or an environment variable. `frame-notes` calls none of them. The summary table, then the citations.

| Question | Answer |
|---|---|
| Tokio runtime started anywhere in the linked Theorem crates? | No. `tokio` is declared by `rustyred-thg-core` (`Cargo.toml:82`, feature `rt-multi-thread`) but nothing under its `src/` uses it (`hooks.rs:31` states the substrate is tokio-free). No `Runtime::new`, `Builder::new_*_thread` or `#[tokio::main]` in non-test code of any linked crate. |
| HTTP/S3 client started at construction? | No. The only client is `S3CompatibleChunkPublisher` (`rustyred-thg-core/src/history_object_store.rs:43`, `AmazonS3Builder` at `:72`, `from_env` at `:88`). Nothing constructs it: `RedCoreGraphStore::open` does not reference it; it is only re-exported (`lib.rs:354`). |
| AI provider or model call? | No. `commonplace`'s default embedder is `DeterministicEmbedder` (`commonplace/src/ingest.rs:305-315`, "deterministic local embedder"). `rustyred-thg-ml` is a CPU `burn`/`ndarray` library (`Cargo.toml:29`, default features off; the GPU features `burn`/`cubecl` in its `[features]` are not enabled). No API key or provider URL is read in any constructor. |
| Background threads at construction? | Local only. `Vfs::open_in` registers a `notify` kqueue watcher (`rustyred-thg-vfs/src/lib.rs:477-483`). `RedCoreAuthority::open` asks for `AofAlways` (`theorem-ide-authority/src/lib.rs:126-131`), which maps to the `Immediate` commit policy and no worker (`graph_store.rs:268-271`); the `rustyred-journal-group-commit` fsync thread (`journal.rs:644-647`) starts only for `AofEverysec`, the `RedCoreOptions` default (`graph_store.rs:288`). The post-commit hook dispatcher thread (`hooks.rs:1007-1012`) exists only if a caller builds a `HookDispatcher` and installs its emitter (`graph_store.rs:2113`). |

**`theorem-ide-runtime`** (the crate with the most side-effect surface):

- `IdeRuntime::new` (`services.rs:159-187`) is pure state: `MemoryAuthority::new()`, empty maps, `language_server: None`, `debug_adapter: None`, `extension_host: None`. `Default` calls `new` (`:4929`).
- `bind_checkout` only records a path (`services.rs:208`). `register_locked_providers` (`:230-239`) "is not a live session": it registers descriptors.
- Subprocesses exist and are all reached by an explicit operation, never by construction: the Node extension host (`extension_host.rs:108` `spawn`, `:117` `Command::new("node")`) only from `ensure_extension_host` (`services.rs:842-851`) which only `extensions.activate` calls (`:532`, `:567`); the language server and debug adapter only after `attach_language_server` / `attach_debug_adapter` (`:249-251`, `lsp.rs:63`, `dap.rs:54`); `git` for `scm.*` and workspace status (`scm.rs:979,1001`, `workspace.rs:819`); test and run commands (`testing.rs:75`, `run.rs:171`); `python3` for VSIX unpacking (`vsix.rs:126`).
- **One network path:** `lsp.rs:1363` `fetch_schemastore_catalog` runs `curl` against SchemaStore (`:1364`); `live_schemastore_catalog_path` (`:1415-1429`) calls it when no workspace or cached catalog exists (guarded by `cfg!(test)` at `:1424`). It is reached only while building the `evenBetterToml` configuration for an attached TOML language server (`lsp.rs:327,1190,1204,1222`). Not reachable from document sessions. Turning it off: do not attach a language server, or put a catalog file where the lookup finds it (`THEOREM_IDE_SCHEMASTORE_CATALOG`, `lsp.rs:1403`, or `~/.cache/theorem-ide/schemastore-catalog.json`, `lsp.rs:1407-1410`).
- `extension_install.rs:135` names `https://open-vsx.org` as the registry in a lock listing; it only builds JSON (`discover`), no request.

**`commonplace`** (pulled by `rustyred-thg-vfs`):

- The VFS uses only the `BlobStore` trait and `InMemoryBlobStore` from it (`rustyred-thg-vfs/src/lib.rs:31`, `history.rs:5`, `theoremweb.rs:10`). It never builds an `IngestPipeline`.
- **The one path that is on by default and reaches outside the process:** `IngestPipeline::ingest` -> `content_core_extract_with_config` (`ingest.rs:717`) runs `content-core extract <source> --format json` (`content_core.rs:233`, command list at `:338-341`), falling back to `uvx content-core`, which downloads a Python package and can fetch URLs. It is enabled unless `THEOREM_CONTENT_CORE_ENABLED` is falsey (`content_core.rs:16`, `:61-64` `unwrap_or(true)`). To turn it off: set `THEOREM_CONTENT_CORE_ENABLED=0`, or build the pipeline with `ContentCoreExtractionConfig::disabled()` (`:86`). The notes core never calls `IngestPipeline`; if a later step does, it must pass the disabled config.

**`rustyred-thg-core`**: `RedCoreGraphStore::memory()` and `open()` (`graph_store.rs:1516`, `:1602`) read no network settings; the only `env::var` calls in `graph_store.rs` are test crash hooks (`:215`, `:219`). `from_env` constructors in the linked crates exist only in `content_core.rs` (above) and `history_object_store.rs:88` (AWS variables, only when a caller asks). One AI-shaped config exists and is inert: `LearnedConnectionScorerConfig::default` reads `THEOREM_EPISTEMIC_SCORER_ENDPOINT` and `THEOREM_EPISTEMIC_SCORER_MODEL` (`epistemic.rs:402-418`), but `LearnedConnectionScorer::score` returns an error in both branches and has no transport (`epistemic.rs:432-451`). `theorem-harness-core/src/model_catalog.rs:355` reads `THEOREM_MODELS_JSON` as a file path for a model catalogue; it is data, not a client.

**Other subprocess sites in linked crates** (all behind explicit calls, none in construction): `compute-offload/src/executors/smt.rs:381,471` (an SMT solver binary), `rustyred-thg-offload/src/smt_check.rs:234` (`kill`). `theorem-harness-core/src/project_slug.rs:38` is `#[cfg(test)]`. No `#[ctor]`, `inventory::submit` or `linkme` section in any linked crate, so nothing runs at load time.

**Not covered.** This is a read of source in the 36 path crates, not a runtime trace. Registry crates (`extism`, `wasmtime`, `burn`, `reqwest`, `tokio`) were not read; `extism` is only reached through `rustyred-plugin`, which no constructor on the notes path instantiates. The follow-up that would make G6 mechanical rather than read-based is a test in step 2 that opens `RedCoreAuthority` + `Vfs` + `IdeRuntime` with the network namespace denied and asserts no socket is opened; not done here.

## 6. After the foundations branch

Sections 1 to 5 describe `frame-notes` against Theorem 6e57dbb. Frame now
builds against `claude/notes-foundations` (d9d1b9f44, local only), which sits directly on 6e57dbb
and adds atomic writes, the move to `notify` 8.2.0 with FSEvents, the
three-way merge in `documents_reconcile`, and one dependency cut:
`rustyred-thg-vfs` used only `BlobStore` and `InMemoryBlobStore` from
`commonplace`, so that 90-line seam moved into `rustyred-thg-core`
(`rustyred_thg_core::blob`), `commonplace` re-exports it unchanged, and the
VFS no longer depends on `commonplace`.

Measured on the merged `feat/notes-app` lock, by name and version in
`Cargo.lock` (a different count from section 2's `cargo tree --duplicates`):

| | Packages | Names in more than one version |
|---|---|---|
| Frame at b7db174 | 1501 | 161 |
| Merged lanes, Theorem 6e57dbb | 1856 | 195 |
| Merged lanes, foundations branch | 1572 | 166 |

The cut removes 284 packages and adds none. `burn`, `wasmtime`, `extism`,
`cranelift`, `cubecl`, `tch`, `tree-sitter`, `jsonschema`, `commonplace`,
`theorem-harness-core`, `theorem-blocks`, `rustyred-plugin` and
`rustyred-thg-ml` are gone from the `frame-notes` graph; 18 path crates and
479 registry crates remain in it (all targets, normal and build edges). The
names newly duplicated against b7db174 are `enum-as-inner`, `fearless_simd`,
`hash32`, `quick_cache` and `ropey` (1.6.1 beside GPUI Kit's 2.0.0-beta.1).
`notify` resolves once, at 8.2.0, shared with `gpui-component`, so the app
has one watcher stack. The Loro pins from section 1 are unchanged.

What stays: `rustyred-thg-core` depends on `object_store` with its `aws`
feature, which brings `reqwest` 0.12.28 and `tokio` 1.54.1. Section 5's
reading still holds: nothing on the notes path constructs that client. Making
`object_store` optional in `rustyred-thg-core` would remove both, but every
Theorem crate depends on that crate, so it is not done here.
