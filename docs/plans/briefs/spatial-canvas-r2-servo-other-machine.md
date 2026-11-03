# R2 brief: changes for working on another machine

The R2 brief was written for the Mac that holds Frame's checkouts. Its `/Volumes/SSD Samsung/...` paths don't exist on your machine. Everything in the brief still applies, except the parts below, which replace it.

## Get the code (replaces "Where to work")

Clone only the branch you need, from R1's commit:

```bash
git clone --depth 1 --single-branch --branch Travis-Gilbert/viewport-css-size https://github.com/Travis-Gilbert/servo.git servo-r2
```

```bash
cd servo-r2 && git switch -c feat/request-activity && git rev-parse HEAD
```

`HEAD` must be `7a28fd822bee1b2147d4598985c719e9f444b88b`. If it isn't, stop and say so.

Before you open the clone in Cursor, create `.vscode/settings.json` in it with exactly this content. It's gitignored, and it stops rust-analyzer from running `cargo check` on all of Servo:

```json
{
  "rust-analyzer.checkOnSave": false,
  "rust-analyzer.cargo.buildScripts.enable": false,
  "rust-analyzer.procMacro.enable": false
}
```

Work only inside this clone.

## Build and test (replaces the paths in "How to build and run")

- **Build target:** use the clone's default `target/`. Leave out `CARGO_TARGET_DIR` and the SSD `TMPDIR`.
- **Everything else stays:** the `CARGO_PROFILE_*` settings, `--locked`, and one test per process. `STRIP=none` matters only on macOS, but it is harmless elsewhere.
- **Cold build:** this machine has never built this Servo, so the first build compiles everything, mozjs included.
- **Missing system packages:** if the build fails for one, stop and tell the user which packages are missing. Don't install them yourself, and don't run `./mach`.
- **Disk:** check free space on this machine before each build, and stop below 6 GiB.

For example:

```bash
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_STRIP=none CARGO_PROFILE_TEST_STRIP=none cargo test --locked -p servo --test request_activity <name> -- --exact
```

## Git (replaces the git rule in "Rules")

Git stays read-only while you work. When every test in the brief passes:

1. Commit the Servo changes on `feat/request-activity` as one commit, with the message `feat(servo): report when each request starts and ends`.
   - Leave out `.vscode/`.
   - Add no `Co-Authored-By` lines.
2. Push it to the fork:

   ```bash
   git push origin feat/request-activity:Travis-Gilbert/request-activity
   ```

3. Do nothing else with git: no other branches, no force pushes, no pull requests and nothing upstream.

## Report (replaces "Report")

- Write no report folder. The Mac reruns the tests and records the evidence itself.
- End with one message that holds what the brief's `README.md` would have held:
  - what changed, file by file, and the final API;
  - how the invariant is enforced, and what you found about navigation redirects and about when the fetch task ends;
  - each test's result and wall time;
  - anything left open;
  - the pushed commit's hash.
