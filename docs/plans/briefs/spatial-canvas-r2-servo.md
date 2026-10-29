# R2, Servo half: tell the embedder when each request starts and ends

A brief for one agent. Read all of it before you change anything.

## Why

Frame is a browser built on a fork of Servo (`Travis-Gilbert/servo`). Its spatial canvas shows pages as cards, and it disposes cards it isn't showing to save memory. It must never dispose a card that still has a request in flight. The case that matters most is an upload a page sends after it has loaded: if Frame drops the page while the upload runs, the person's upload is lost.

Today Servo tells the embedder when a request starts, but not when it ends. Your job is to add the end, all in Servo, and test it. Later, Frame will count each page's requests in flight. That half isn't part of this task.

## Where to work

- **Checkout:** `/Volumes/SSD Samsung/frame-deps/servo-r2`. It's on local branch `feat/request-activity`, at `7a28fd822be`, which is R1's commit. It's clean and made for this task.
- **Build target:** `/Volumes/SSD Samsung/cargo-targets/servo-r1`. R1 used it, and it already holds the built mozjs and crates.io dependencies. Expect your first build to recompile Servo's own crates, because the checkout path is new. Only this task builds there now.
- **Files you may change:** anything under `servo-r2`. That will most likely be:
  - `components/net/resource_thread.rs`
  - `components/net/embedder.rs`
  - `components/net/request_interceptor.rs`
  - `components/net/test_util.rs`
  - `components/shared/embedder/lib.rs`
  - `components/servo/servo.rs`
  - `components/servo/webview_delegate.rs`
  - `components/servo/lib.rs`
  - `components/servo/tests/common/mod.rs`
  - a new `components/servo/tests/request_activity.rs`
- **Report folder:** `/Volumes/SSD Samsung/frame-worktrees/spatial-canvas/docs/plans/evidence/spatial-canvas-r2-2026-10-28/`. Write nothing else in that worktree.

## What exists today

- **Where Servo sends the start:**
  - `components/net/request_interceptor.rs:36` `intercept_request` builds a `WebResourceRequest` (lines 47-56).
  - It sends that as `NetToEmbedderMsg::WebResourceRequested` (line 59).
- **When it fires:**
  - It's called only from `components/net/http_loader.rs:2224`, in the HTTP transport branch.
  - So it fires once per HTTP hop, and again on every redirect.
  - It never fires for cache hits, or for `data:`, `blob:`, `file:` or `about:` URLs.
  - Nothing reports the end.
- **How it reaches the embedder:**
  - `components/servo/servo.rs:633-656` turns the message into `WebViewDelegate::load_web_resource` (`components/servo/webview_delegate.rs:1083`).
  - When the request has no webview, it goes to `ServoDelegate::load_web_resource` instead.
- **One task per fetch:**
  - `components/net/resource_thread.rs:807` `fn fetch` spawns one task per fetch (`spawn_task`, near line 860).
  - The `Some(res_init)` branch of that function continues a navigation after a redirect.
  - The only other spawn site is `websocket_connect` (line 926). WebSockets are out of scope.
- **Request ids:**
  - `components/shared/net/request.rs:443` gives every `Request` an `id: RequestId`, which wraps a UUID (line 34).
  - Line 509 holds its `target_webview_id: Option<WebViewId>`.
- **The response body:** `components/net/fetch/methods.rs:852` and `:877` are where `main_fetch` calls `process_response_eof`.
- **Test helpers:**
  - `components/servo/tests/common/mod.rs` has `ServoTest` and the shared test delegate, `WebViewDelegateImpl`.
  - `components/servo/tests/webview.rs:656` (`test_set_viewport_scale_only`) is a recent test to copy from.
  - `components/servo/tests/network_manager.rs:19` loads a page from a test server.
- **The test server binds every interface:**
  - `components/net/test_util.rs:84` `make_server` binds `0.0.0.0:0` and returns `http://localhost:{port}`.
  - Its handler runs synchronously.
  - Binding `0.0.0.0` breaks a rule of this project (see Rules).

## What to build

1. **Two notifications on `WebViewDelegate`,** next to `load_web_resource`. Each one does nothing by default.

   ```rust
   /// A request for this webview has started. Each call is followed by exactly
   /// one `notify_request_finished` with the same id.
   fn notify_request_started(&self, _webview: WebView, _id: RequestId, _request: WebResourceRequest) {}

   /// A request that `notify_request_started` reported has ended: its response
   /// body completed, it failed, or it was cancelled.
   fn notify_request_finished(&self, _webview: WebView, _id: RequestId) {}
   ```

   - Re-export the id type from `components/servo/lib.rs`, next to `WebResourceLoad` (line 122).
   - If Servo's conventions point to a better name or shape, use it, and say why in the report.

2. **Send them from the fetch task in `resource_thread.rs`.**
   - Send started before the fetch runs.
   - Send finished from a guard whose `Drop` sends it. Then every way out, including a cancel or an early return, sends it exactly once.
   - Use new `NetToEmbedderMsg` variants on the existing channel, so started always arrives before finished.
   - In `servo.rs`, drop the message if the webview no longer exists.

3. **Report only requests that have a `target_webview_id`,** but report every scheme: `http`, `https`, `data`, `blob`, `file` and `about`. Frame filters on its side.

4. **Build the `WebResourceRequest` the way `intercept_request` does.** Move that construction into one function that both callers use. Don't write a second copy.

5. **Keep this invariant:**
   - for each id, one started, then one finished;
   - never a finished without a started;
   - never two starts for the same id while it's in flight.

   Two places need checking:
   - **Navigation redirects.** The `Some(res_init)` continuation may reuse the navigation's request id. One pair per hop or one pair per navigation are both fine, but the pairs for one id must not overlap.
   - **When the fetch task ends.** Confirm it ends only after the response body completes (`process_response_eof`), not when the headers arrive. If some path ends it earlier, find where that body really ends and send finished there.

6. **Leave `load_web_resource` and interception as they are.**

Frame needs the request's details because some requests stay open on purpose: `EventSource` and long polling. Frame will decide which requests keep a card alive from the method, `has_body`, the destination and the URL. `WebResourceRequest` already carries all four.

## Tests

Put them in a new file, `components/servo/tests/request_activity.rs`.

**How the tests work:**
- A test delegate records every started and finished, in order.
- Every test ends by checking the invariant: each started id has exactly one finished, no finished lacks a started, and nothing is still in flight.
- The servers bind `127.0.0.1:0`, and the pages use `http://127.0.0.1:{port}` URLs.
  - Don't use `make_server` or `make_ssl_server` as they are.
  - To reuse their code, add a loopback variant to `net::test_util` that shares `make_server`'s body. Leave `make_server`'s own behaviour unchanged.
- To hold a response open, don't block the synchronous handler.
  - Send the headers, and give the response a body fed from a channel.
  - The body ends when the test sends the last chunk or drops the sender.
  - `components/net/http_loader.rs:572` already builds such a body: an `http_body_util::StreamBody` over a `tokio_stream::wrappers::ReceiverStream`.
  - Build the held body inside `net::test_util`, because net already depends on those crates. The servo crate's tests don't, and adding them there would change `Cargo.lock`.
  - If you need a new dependency anyway, stop and say so.

**The tests:**
1. **`fetch` upload after load** (the done criterion).
   - Load a page from the loopback server and wait for `LoadStatus::Complete`.
   - From script, send `fetch(url, { method: "POST", body: new Uint8Array(1 << 20) })`.
   - While the server holds the response, there is a started (method `POST`, `has_body` true) and no matching finished.
   - When the test releases the response, finished arrives with the same id.
2. **The same upload through `XMLHttpRequest`.**
3. **Abort.** `AbortController.abort()` on a held fetch produces exactly one finished.
4. **Network error.** A fetch to a `127.0.0.1` port where nothing is listening produces a started and then a finished.
5. **Subresource redirect.** A `fetch` through a 302 produces one started and one finished.
6. **Document redirect.** Load the page itself through a 302. The invariant holds, and nothing is in flight once the page has loaded and gone quiet.
7. **Navigating away while an upload is in flight.** The held request gets its finished.
8. **A `data:` URL fetch** produces one pair.

**Regression checks:**
- R1's nine viewport tests in `components/servo/tests/webview.rs`, each run separately:
  - `test_negative_resize_to_request`
  - `test_page_zoom`
  - `test_pinch_zoom_update_dom_visual_viewport`
  - `test_resize_webview_zero`
  - `test_set_viewport_css_resize`
  - `test_set_viewport_scale_only`
  - `test_set_viewport_then_legacy_resize_and_scale`
  - `test_viewport_meta_tag_initial_scale`
  - `test_webview_resize_interactivity`
- The interception unit tests in `webview_delegate.rs`.
- Don't run `network_manager.rs` or net's own integration tests. They bind `0.0.0.0`.

## How to build and run

Run everything from the checkout root. Run each Servo test on its own: two in one process fail with `Already initialized`.

1. Check free space first. Stop if less than 6 GiB is free (33 GiB is free now):

   ```bash
   df -g "/Volumes/SSD Samsung"
   ```

2. Run one integration test. Put the test's name in `<name>`:

   ```bash
   cd "/Volumes/SSD Samsung/frame-deps/servo-r2" && TMPDIR="/Volumes/SSD Samsung/tmp" CARGO_TARGET_DIR="/Volumes/SSD Samsung/cargo-targets/servo-r1" CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_STRIP=none CARGO_PROFILE_TEST_STRIP=none cargo test --locked -p servo --test request_activity <name> -- --exact
   ```

   For R1's tests, use `--test webview` instead.

3. Run the interception unit tests:

   ```bash
   cd "/Volumes/SSD Samsung/frame-deps/servo-r2" && TMPDIR="/Volumes/SSD Samsung/tmp" CARGO_TARGET_DIR="/Volumes/SSD Samsung/cargo-targets/servo-r1" CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_STRIP=none CARGO_PROFILE_TEST_STRIP=none cargo test --locked -p servo --lib webview_delegate
   ```

Why these settings:
- `STRIP=none` avoids a macOS loader failure ("mis-aligned LINKEDIT string pool") on stripped proc-macro libraries.
- `DEBUG=0` keeps the target small.
- `TMPDIR` on the SSD keeps test stores off the internal disk.

These tests are headless and open no window.

## Rules

- **Only these two places.** Work only in `servo-r2` and the report folder.
  - Never edit `frame-deps/servo-r1` or any other checkout under `frame-deps`: they are pinned, and other branches build from them.
  - Never edit any Frame worktree's `deps/`.
- **Git is read-only:** `status`, `diff`, `log` and `show` only.
  - Never commit, check out, switch, reset, stash, rebase, create or delete a branch, add or remove a worktree, `gc` or push.
  - Every checkout under `frame-deps` shares one repository, so any of these affects the others.
- **No extra builds.**
  - Never run `cargo check`, `cargo clippy` or `./mach`. Each one builds all of Servo again.
  - Leave `.vscode/settings.json` as it is. It stops rust-analyzer from running `cargo check` when you open or save a file.
- **No new dependencies.** Don't add any, and don't change `Cargo.lock`; `--locked` enforces that.
- **Disk.** Check free space before each build, and stop below 6 GiB.
- **Network and screen.** Bind only to `127.0.0.1`. Open no windows, take no screen captures, and change no system settings.
- **Other processes.** Don't stop any process you didn't start. Other agents build Frame on this Mac.
- **Nothing outward-facing.** Don't open an issue or pull request upstream. That's the user's call.
- **Style.**
  - Match the surrounding code's naming, comments and idiom.
  - If you run `rustfmt`, run it only on the files you changed, and revert any hunk outside your change.
- **When to stop and say so:**
  - The spec looks wrong, or there's a better seam: say so in the report, and don't quietly shrink the task.
  - A rule blocks you: stop and report it. Don't work around the rule.

## Report

In the report folder, write:
- **`README.md`:**
  - what changed, file by file, and the final API;
  - how the invariant is enforced, including what you found about navigation redirects and about when the fetch task ends;
  - each test's result and wall time;
  - anything left open.
- **`commands.md`:** for every command you ran, its directory, exit code, wall time, log file and the command itself. Copy the format of `../spatial-canvas-r1-2026-10-27/commands.md`.
- **One `.log` per command.**
- **`servo-r2.diff`:** the output of `git -C "/Volumes/SSD Samsung/frame-deps/servo-r2" diff`, plus any new files.

Leave every change uncommitted. End with a short message: the files you changed, which tests passed and failed, and any open questions.

## Not part of this task

- **Frame's half:** counting each page's requests in flight, and keeping such a card from being disposed. That waits for card disposal, in milestone M3.
- **Landing the work:** reviewing and committing it on `feat/request-activity`, building Frame against it, and moving Frame's pin. The orchestrator does these.
- **Pushing to the fork.** That needs the user's yes.
- **Upstreaming** the work to Servo.
- **WebSocket activity.**
