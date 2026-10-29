# Handoff: the vault and spatial canvas work, 2026-10-29

This is for the session that picks up the notes vault and the spatial canvas
after the planning session that wrote both plans. That session ran from
2026-10-26 to 2026-10-29: "feat(vault): Obsidian-style vault over local and
Theorem files, with an editor design pass", and its fork. Read this first,
then the issue for the plan you are working on.

## Where things are

| What | Where |
|---|---|
| Notes plan | [`docs/plans/notes-app.md`](../notes-app.md); tracking issue #13 |
| Canvas plan | [`docs/plans/spatial-canvas.md`](../spatial-canvas.md); tracking issue #12 |
| Vault code | `crates/notes`, `crates/notes-fixtures`, `vendor/zorite-*`, on `main` since #8 |
| Canvas code | `src/canvas/`, with the chrome kit in `src/canvas/ui/`, on `main` since #8 |
| Old branch tips | tags `archive/feat/notes-*`, `archive/feat/spatial-canvas`, `archive/feat/canvas-*`, `archive/feat/site-panel` |
| Servo | `efdf7370`, the fork's `main` ([Travis-Gilbert/servo](https://github.com/Travis-Gilbert/servo)), at `deps/servo-consolidated`. It holds the WebGL2 work, R1 and R2's Servo half |
| Theorem | `8c5d22bd` (`script/bootstrap`), which holds the notes foundations |
| Theorem plan | `plan:b31a6f04c7036aea` for the canvas. The Harness was degraded at handoff, and claims on M1 and W0 never landed |

This handoff, the canvas plan, its briefs and evidence, and W0's spike reach
`main` with #11. Until that merges they are on `docs/canvas-plan-handoff`.

## Done

- **Notes:** steps 1 to 3 of 14 (foundations, the headless vault core, Servo
  WebGL2). The index matches Obsidian 1.14.5 on 537 of 537 CLI calls. There is
  no notes window in the app yet.
- **Canvas:** P0, R1, W0, and M1 apart from its by-hand checks. The chrome kit
  and the site panel's data are built ahead of M2.

## Open, in the order I would take them

1. **Port the glass and chrome commits onto `main`.** Two commits exist only
   on this Mac's `feat/native-home`, in
   `/Volumes/SSD Samsung/frame-worktrees/native-home`:
   - `96585c5`: larger toolbar buttons (`CHROME_BUTTON` is 28/0.75), Ask
     uses the agent icon, and Home loses its place tooltips and shortcut
     hints;
   - `71dacfe`: regular glass, tinted to the title-bar color at 85% in dark
     and 60% in light, and the solid frame color while the window is inactive.
   `feat/native-home` went to `main` as #4 before these existed. Cherry-picking
   onto `origin/main` conflicts in `README.md`, `src/browser/underlay.rs`,
   `src/glass.rs`, `src/home.rs` and `src/icons.rs`. Build, run
   `cargo test --locked --bin frame browser::tests`, then open a PR. The
   tuning and measurements are in
   `docs/plans/evidence/spatial-canvas-glass-2026-10-28/README.md`, section
   "The switch to regular glass". Keep the worktree until the port lands.
2. **Notes step 4** (#13): one note end to end, then the reading view and
   `notes_check`. Its live web card half is done as M1. It needs nothing that
   is still open.
3. **Canvas M2** (#12). The plan puts it after P0 (done) and the Arc merge
   (not done). Open decisions 5 (one canvas per Arc Space) and 6 (rename Arc's
   `Placement` to `Section` before M2) touch the Arc line, so settle them with
   the owner, or with the Arc session, first.
4. **R2's checks on this Mac.** Fetch the tag
   `archive/Travis-Gilbert/request-activity` (the branch is deleted) into
   `/Volumes/SSD Samsung/frame-deps/servo-r2`. Rerun its tests in
   `cargo-targets/servo-r1` and write the evidence folder. Frame's half of R2
   waits for M3.
5. **M1's by-hand checks** in `tests/canvas/MANUAL.md`: the IME candidate
   window, where menus, pickers and dialogs open, and drops from Finder. These
   need the owner at the Mac.

## Not mine, don't touch

- **The Arc line** (`feat/arc-split-model`, in
  `/Volumes/SSD Samsung/frame-worktrees/arc-split-model`) belongs to the
  session "Arc Browser feature planning for platinum". At handoff it had 11
  uncommitted files. Merging native-home into it was asked for and then put on
  hold. When it happens, the Arc line should take `main` or the port PR from
  item 1, not the stale local `feat/native-home`.
- **The main checkout** (`/Users/travisgilbert/Tech Dev Local/Creative/Platinum`,
  on `feat/liquid-glass-chrome`) holds other sessions' uncommitted work. Its
  untracked copies of this session's docs become duplicates once #11 merges.
  `docs/plans/html-in-canvas.md`, `rustyred-profile-search.md` and the two
  `ort-*` learnings there come from other sessions.

## Worktrees

| Worktree | Keep until |
|---|---|
| `frame-worktrees/native-home` | item 1's port lands |
| `frame-worktrees/web-viewer-spike` | #11 merges; its untracked `spikes/` and W0 evidence are in #11 |

## House rules that bit during this work

- **The SSD had 7.8 GiB free at handoff, and the floor is 6 GiB.** Check
  `df -h "/Volumes/SSD Samsung"` before any build, and stop below 6 GiB.
- Serialize builds in a shared target (`cargo-targets/frame`,
  `cargo-targets/frame-canvas`). Never run `cargo check`. Never edit `deps/`;
  only the orchestrator relinks it or moves pins, after building Frame against
  the candidate.
- Merge with `git -c core.attributesFile=/dev/null`. The weave driver
  corrupted merges.
- Push and publish only with the owner's yes. Commits use
  `<type>(<scope>): <description>`, with no Co-Authored-By lines.
- Native checks share one screen with other agents. A glass capture must say
  whether the window was key: read the close button at (119,104) in a full
  1372x891 capture, red when key. The memory notes
  `frame-computer-use-quirks` and `frame-native-checks-share-the-screen` have
  the details.
- Temporary profiles only, with no sign-ins. Bind only 127.0.0.1.
