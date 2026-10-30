# Native foundation: source evidence and limits

Inspected 2026-10-25 (America/Detroit). This supports the [architecture recommendation](theorem-native-foundation.md); it is not a live acceptance report.

## Baselines

| Source | Inspected identity | Interpretation |
| --- | --- | --- |
| Current Frame checkout | `b7db17457ce2e12c3fcaa5ab9c0ea1fde52a58aa`, branch `feat/liquid-glass-chrome`, tracking `origin/feat/liquid-glass-chrome` | Host for the proposed harness. Existing untracked configuration/learnings were preserved. |
| Theorem remote main, also present as local `origin/main` | `8e151d510722c2946f3f1cf63eb5a73e06c1695b` | Used for current source comparisons through GitHub and read-only `git show`. |
| Theorem-UI remote main | `38e723006c8419a305e7ed443180e6c5e64bed81` | Used for the assistant-ui integration and Theia architecture. |
| Sibling Theorem working checkout | `87dcddbe48cb5d730c11f4d76cb221fcde20bd17`; dirty and behind its tracked main | Not used as current backend truth; no pull/reset/stash. |
| Frame's Theorem dependency checkout | `6e57dbb39124f472d8619fb20a9984e89eb225e8` | Search/browser dependency, not an assumed current harness SDK or server. |
| Sibling Agentic-GPUI checkout | `5c77e0343e1036af8f033f8da3da7d74c1c0cee2`; unresolved merge entries | Inspected contracts only. No repair or dependency on its dirty worktree was introduced. |
| Named Platinum repository | Its fetched Cargo manifest also names the package `frame` and pins GPUI Kit 0.8.0 | The session's actual checkout remote is Frame. No repository or product rename was performed. |

The three pasted documents were read as prior proposals. Their claims about existing code were checked selectively against the sources below. Claims about previous successful runs remain user-reported unless independently reproduced.

## Findings tied to source

### E1. Preserve this host, not the previous visual shell

[Frame Cargo manifest](../../Cargo.toml) pins GPUI Kit 0.8.0, embeds the Servo fork, and patches `gpui-pre`, `gpui-pre-apple`, `gpui-component`, and `zed-font-kit`. [The dock implementation](../../src/browser/dock.rs) uses GPUI Kit's `DockArea` and its own renderer for Frame's tab/card chrome. [The application root](../../src/main.rs) initializes GPUI Kit and its themed native root.

The local component source has [message primitives](../../vendor/gpui-component/src/message.rs), [a virtualized tail-following message scroller](../../vendor/gpui-component/src/message_scroller.rs), and [attachments](../../vendor/gpui-component/src/attachment.rs). The scroller owns scrolling state while the caller supplies messages. These are concrete reuse candidates; no chat interface was built or rendered in this audit.

### E2. What the working web integration actually contains

Theorem-UI's [runtime.tsx](https://github.com/Travis-Gilbert/Theorem-UI/blob/38e723006c8419a305e7ed443180e6c5e64bed81/src/lib/theorem/runtime.tsx) combines `useLocalRuntime`, `useRemoteThreadListRuntime`, durable history mapping, edit/regenerate alignment and live-run resumption. It is not an ExternalStoreRuntime integration.

Its [client.ts](https://github.com/Travis-Gilbert/Theorem-UI/blob/38e723006c8419a305e7ed443180e6c5e64bed81/src/lib/theorem/client.ts) configures `auth: false` against the window origin and documents that the server proxy supplies bearer authentication. It also owns bootstrap refresh and mutation helpers. Extracting this into a native session module therefore requires replacing browser/proxy assumptions and separating React/store references; the file cannot simply be imported unchanged into a child process.

The SDK's [assistant-ui adapter](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/packages/theorem-sdk/src/assistant-ui.ts) is a client adapter, not an execution backend. It sends one new turn, uses durable IDs, invokes edit/retry, and folds cumulative parts. It intentionally leaves tool execution to Theorem. It rejects attachments in this adapter path and directs callers to upload/send APIs. Full native attachment support must therefore use those APIs, not claim parity from the adapter alone.

The [SDK README](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/packages/theorem-sdk/README.md) and [package manifest](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/packages/theorem-sdk/package.json) describe version 0.4.0, Node 20+/browser support, local package installation, and an optional assistant-ui adapter. Runtime code uses native fetch. Publication, deployment and Bun compatibility were not independently checked.

### E3. Existing native integration has specific missing behavior

In [chat_host.rs](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/apps/theoremweb/crates/agent-runtime/src/chat_host.rs), lines 1938–1958 refuse single-tool retry, message edit and feedback for named reasons. Lines 1986–2004 explain the regenerate path: a local fork can exist without an operation that starts its new run. These are source observations, not a claim that every native operation is broken.

The [native transport](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/apps/theoremweb/crates/agent-runtime/src/client/native.rs) expects `THEOREMWEB_AUTHORITY_URL` and `THEOREMWEB_AUTH_TOKEN_FILE`, reads a bearer, and admits identity. Its flow includes append-message, start-run, and link-run operations. This differs from the web SDK's higher-level send operation. No credential files were opened to test it.

The [native driver](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/apps/theoremweb/crates/gpui/host/src/chat_driver.rs) contains useful observer-generation and host-reach patterns but depends on the existing ProductShell composition. Reuse the behavior selectively; do not treat the whole driver as a ready-made Frame adapter.

### E4. Client/server contract drift is visible in one pinned source snapshot

All of these observations use Theorem commit `8e151d510722c2946f3f1cf63eb5a73e06c1695b`:

| Contract side | Observed behavior |
| --- | --- |
| [SDK OpenAPI](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/packages/theorem-sdk/fern/openapi.json) and assistant-ui adapter | `messages_send` uses `POST /api/chat/v1/threads/{threadId}/messages`, passes `text`, and consumes a response containing `run`. |
| [Server chat router and request types](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/rustyredcore_THG/crates/rustyred-thg-server/src/theorem_chat.rs) | That route calls `append_message`. `AppendMessageRequest` requires `branch_id`, `role`, and `parts`, uses camelCase JSON, and denies unknown fields. The response is an appended-message mutation plus context admissions. |
| SDK edit/retry | Declares `/api/chat/v1/messages/{messageId}/edit` and `/retry`, attributing both to `theorem_chat.rs`. Neither is registered in that inspected router. |
| [Agent stream router](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/rustyredcore_THG/crates/rustyred-thg-server/src/theorem_agent_stream.rs) | Has chat run cancel/events routes. Their presence does not supply the missing edit/retry registrations. |
| SDK app-extension routes | OpenAPI attributes `/api/extensions/v1/...` handlers to `rustyred-thg-server/src/theorem_extensions.rs`; that file is absent from the inspected Git tree. |

This establishes a source-alignment problem to resolve. It does **not** establish which source a running deployment uses, whether another unmerged branch contains the missing handlers, or which operations the user's successful web run exercised. Do not label all SDK operations operational or conclude that the deployed web app cannot work.

Read-only reproduction from a clone containing that commit:

```sh
git show 8e151d510722c2946f3f1cf63eb5a73e06c1695b:packages/theorem-sdk/src/assistant-ui.ts
git show 8e151d510722c2946f3f1cf63eb5a73e06c1695b:rustyredcore_THG/crates/rustyred-thg-server/src/theorem_chat.rs
git ls-tree -r --name-only 8e151d510722c2946f3f1cf63eb5a73e06c1695b rustyredcore_THG/crates/rustyred-thg-server/src
```

### E5. Reuse existing app and AGPUI owners

The [AppManifest implementation](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/rustyredcore_THG/crates/theorem-blocks/src/app.rs) validates shapes, seed blocks, grants, WASM modules and contribution references. Versions start at one. [Extension points](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/rustyredcore_THG/crates/theorem-blocks/src/extension_point.rs) define five core slots and content-derived contribution IDs. [Presentation packaging](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/rustyredcore_THG/crates/theorem-blocks/src/presentation.rs) already checks artifact integrity and configured trusted publisher signatures. Native handlers and lifecycle acceptance remain separate from these library implementations.

The [renderer-free semantic crate](https://github.com/Travis-Gilbert/Agentic-GPUI/blob/main/crates/agpui-contract/Cargo.toml) depends on serde, serde_json and sha2, not GPUI. It can be evaluated independently of old views. Source/path compatibility with a consumer must still be checked.

Theorem also contains an [AGPUI surface protocol](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/rustyredcore_THG/crates/agpui-shell-protocol/src/lib.rs), [QuickJS worker](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/apps/agpui-shell-host/README.md), [stdio session](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/apps/agpui-shell-host/src/session.rs), [native materializer](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/apps/theoremweb/crates/agpui/shell/src/lib.rs) and [plugin SDK](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/packages/agpui-plugin-sdk/README.md). Its JSX is explicitly not React compatibility. These sources were inspected, not built or tested here.

The [existing Shell specification](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/docs/plans/agpui-shell/SPEC-AGPUI-SHELL-1.0.md) calls for isolated worker execution and reuse of existing registration/permission owners. The new recommendation preserves that requirement for arbitrary contributions. It does not mistake upstream in-process Shell for the implementation of the worker specification.

### E6. External library verification

Context7 was used for assistant-ui, GPUI Kit and terminal backend documentation. GitHub source and Firecrawl retrieval checked the named projects and Twenty's app concepts. External moving sources must be pinned before implementation. Git blob hashes below identify the inspected files, not released package versions or repository commits.

| File/source | Inspected value or finding |
| --- | --- |
| [assistant-ui core package](https://github.com/assistant-ui/assistant-ui/blob/main/packages/core/package.json) | Version field `0.3.31`; blob `2893d96663c14d05fca0eb37a098cea70876ba24`; core/internal/store/React entrypoints. |
| [assistant-ui Vue README](https://github.com/assistant-ui/assistant-ui/blob/main/packages/vue/README.md) | Blob `9513bd0c2094a62db7c647d85c0ad60012621dff`; standalone shims and RuntimeAdapter path. |
| [GPUI Shell README](https://github.com/longbridge/gpui-kit/blob/main/crates/shell/README.md) | Blob `b686a2e42a1f918b083f13a7cd238832c1e22571`; M0 status, incomplete contribution registry, check limitations. |
| [GPUI Shell hosting](https://gpui-kit.com/shell/hosting/) | VM tied to App thread; host-originated changes need explicit refresh. |
| [Twenty app concepts](https://docs.twenty.com/developers/extend/apps/getting-started/concepts) | Typed definitions compiled to a manifest; useful authoring precedent, not a Theorem runtime dependency. |
| [Glim Cargo.toml](https://github.com/glimapp/glim/blob/main/Cargo.toml) | Blob `5dd5d65c6c52e0dc097d47a03bf2b5ccbe222024`; GPUI Kit 0.7.1 plus patches. |
| [Glim app manifest](https://github.com/glimapp/glim/blob/main/crates/glim-app/Cargo.toml) | Blob `3f1277fb9b78bf3be95716855607fea4dfca6959`; binary target and macOS wry/tao dependencies. |
| [Glim format catalog](https://github.com/glimapp/glim/blob/main/docs/file-format-support.md) | Blob `869e1517d178f3c715f8b19ada23a9d246a2cfac`; native and platform-specific readers, partial-preview limits. |
| [Ratatui architecture](https://github.com/ratatui/ratatui/blob/main/ARCHITECTURE.md) | Terminal application/backend separation; does not establish a PTY host for a GPUI app. |

Theorem's [terminal README](https://github.com/Travis-Gilbert/Theorem/blob/8e151d510722c2946f3f1cf63eb5a73e06c1695b/apps/theoremweb/crates/terminal/README.md) pins `alacritty_terminal` 0.26.0 and describes PTY ownership, attach/detach, bounded event hints, snapshots and session loss after host restart. Theorem-UI's [IDE README](https://github.com/Travis-Gilbert/Theorem-UI/blob/38e723006c8419a305e7ed443180e6c5e64bed81/ide/README.md) describes Theia 1.76.0, browser-only operation over Theorem services and Bun-specific historical patches. Neither was executed in this audit.

## Validation and continuation

The user's follow-up makes [SDK+ repair](theorem-sdk-foundation.md) the first workstream and supersedes the initial TypeScript-sidecar preference. The source findings above remain evidence; the new SDK record verifies the capability registry, typed-operation pattern, contract tests and the distinct 203/188 IDE operation counts. No earlier source-only observation has been promoted to live verification.

- Source files, relevant local dependency source and the three supplied proposals were inspected.
- Documentation links and whitespace are checked as part of this change. No application acceptance result follows from those checks.
- No service/model run, native build, screenshot acceptance, installation, publishing or deployment was performed.
- No runtime choice is described as proven until the corresponding gates in the architecture record pass.
- First continuation: SDK+ S0–S6, then native G0/G1. Resolve coverage, contract and working-deployment mismatches before exposing an apparently functional composer.
- Update this record and the main architecture record with the resolved source/build pair and actual evidence. Keep the pre-existing Theorem and Agentic-GPUI working-tree changes intact.
