# Theo's launcher

Written 2026-11-16 by a cloud session that could not build the application
(`deps/` was absent and `src/theo_desk` is macOS-only). The two crates were
built, tested and drawn on Linux; everything under `src/` is source only.
"What was checked" below says exactly which is which.

## What it is

One window, called from any application, with one field: **Search, ask, or run
a command…** The selected row says what Return will do (Open, Run, Resume,
Search, Ask Theo). Choosing "Ask Theo" turns the same window into the
conversation; "Open in Workspace" carries on that conversation in Platinum's
Agent place.

First version, as asked:

1. Call Theo from anywhere, and keep it available after the browser's window
   is closed.
2. Find applications, files, folders, commands and Theorem's conversations.
3. Preview a result and act on it: open, reveal in Finder, copy its path,
   attach it to a message.
4. Ask Theo about a selected file, and carry on the same conversation in the
   workspace.
5. Resume earlier conversations, and see and stop a run that is going.

Not in this version: calculator and conversions, dictionary, translation,
quick notes, clipboard search, screenshot text, voice.

## Where things are

| Path | Holds |
| --- | --- |
| `crates/launcher` | The model, with no dependencies: `Session` (query, scope, lanes, selection, what Return does), `Item`, name and path matching, `apps::scan`, `spotlight::Spotlight`, wording (`words`). |
| `crates/launcher-view` | The window's contents as a GPUI view: field, scope chips, rows, preview, conversation, key hints. Emits `Event`; touches neither the Mac nor Theorem. |
| `crates/launcher-view/examples/launcher_gallery.rs` | The view with sample data, for looking at it without the application. |
| `src/theo_desk/summon.rs` | The host on the Mac: the window, what each chosen row does, the commands, Theorem's conversations and the conversation shown. |
| `src/theo_desk/mod.rs` | The shortcut, the settings, the menu items, and feeding the launcher when the driver changes. |
| `src/main.rs` | `Workspace` (the browser's window, for what is asked of it from outside), `ask_workspace`, and staying running once the window is closed. |
| `src/theorem/driver.rs` | `attach_paths`: a chosen file goes through the same admission as the attachment picker. |
| `src/actions.rs`, `src/browser.rs` | `OpenTyped`: an address or a web search from the launcher opens in a new tab. |
| `script/test-launcher`, `.github/workflows/launcher.yml` | The two crates tested outside the root workspace. |

## How it behaves

- **Keys.** `alt+Space` shows and hides the launcher from any application
  (`summon` in `theo.json`, in the profile folder; the format is
  `global-hotkey`'s, as for `toggle` and `talk`). It is never Command-Space.
  In the window: up and down select, Return does what the row says, Tab and
  Shift-Tab move the scope, Escape steps back one thing at a time (the
  conversation, then the query, then the window), Command-Return asks Theo
  about the selected file, Command-R reveals, Shift-Command-C copies the path,
  Command-N starts a new conversation, Command-O opens the workspace,
  Command-Period stops a run.
- **Searching creates nothing.** Typing only matches. A question is sent only
  when the "Ask Theo" row is chosen, and that row says where it goes: "Starts
  a new conversation", or "Continues" and the title of the one it would add to. A name that finds
  nothing is not turned into a question.
- **Where a row comes from** is on the row: This Mac, Theorem, Platinum, Web.
- **Lanes are independent.** Applications, commands and conversations are
  handed in whole and matched as you type. Files are asked of Spotlight
  (`mdfind`) off the main thread, 90 ms after typing rests; a query that has
  been replaced is cancelled, and its late answer is dropped. A lane that
  cannot look says so in the list and the others still answer. The selected
  row stays the selected row when slower results arrive.
- **Hiding keeps everything.** The query, the rows, an unsent message and the
  conversation are kept between showings. Hiding stops nothing: a run goes on,
  and Stop is its own action, routed as Theo's own Stop is.
- **A message that cannot be sent is not lost.** While Theorem refuses (not
  set up, signed out, a run going, an attachment still being read) the field
  says why and keeps the text. A message the composer turns down at the last
  moment is handed back to the field.
- **Finding a file adds nothing to Theorem.** Spotlight is asked; nothing is
  indexed or stored. "Ask Theo About This" attaches the file to the message
  being written, through the driver's ordinary attachment admission, and shows
  it as a small removable item.
- **Staying running.** With "Keep Platinum Running for Theo" checked (the Theo
  menu; `resident` in `theo.json`, on by default), closing the browser's
  window no longer quits: the shortcuts still answer and a run goes on. The
  Dock icon, New Tab, or anything the launcher sends to the browser opens the
  window again, with its saved session. Quit quits. The notes app is
  unchanged: it has no browser window to open again.

## What was checked, and how

Tested here (Linux, Rust 1.97.1, the repository's vendored GPUI crates):

- `script/test-launcher`: `launcher` 46 passed, `launcher-view` 12 passed.
  The model's tests cover matching, ranking, scopes, the selection held across
  late results, cancellation, Spotlight's arguments and its failures (through
  a stand-in program), and the application scan. The view's tests drive the
  real view headlessly: typing, Return, Tab, Escape, Command-Return, a lane
  that fails, a search asked once per rested query, a message that cannot be
  sent, and what is kept across windows.
- The gallery was built and drawn under Xvfb in each state (empty, results,
  asking, the conversation, a run going, light appearance) and looked at.

Type-checked here, not run:

- `src/theo_desk/summon.rs` and the `Workspace` functions of `src/main.rs`
  were compiled in a throwaway crate against the real GPUI Kit, the launcher
  crates and `theorem-surface-contracts` at the pinned commit, with the rest
  of the application stood in for by signatures copied from it. That found
  and fixed two compile errors. It is not a build of `frame`.

Not built, not run, not observed (needs a Mac with `deps/`):

- `frame` itself with these changes: `src/theo_desk/mod.rs`, `mac.rs`,
  `src/browser.rs`, `src/theorem/driver.rs`, and `Cargo.lock`, whose two new
  entries were written by hand. If `--locked` refuses, build once without it
  and commit the lock Cargo writes.
- Everything about the window on macOS: that `alt+Space` registers, that the
  window takes the keyboard over another application and gives it back, that
  it closes when another window is chosen, its shadow, where it appears.
- Staying running: that the window reopens from the Dock and from New Tab with
  its session, and that nothing in the browser assumes one window per launch.
- A real Spotlight search, and a real conversation through the launcher.

Run on the Mac:

```sh
script/test-launcher
script/media-env cargo build --locked
script/media-env cargo test --locked --bin frame theo_desk
```

then: `alt+Space` over another application; type an application's name and
Return; type a file's name, Command-Return, and send a message; Command-O;
close the browser's window, press `alt+Space` again, and click the Dock icon.

## Known limits

- **Theorem's own content is conversations by title only.** Passages from
  Theorem's file index and a search inside conversations are not offered: the
  Theorem transport Platinum pins has no route for either. The lanes are left
  out rather than drawn empty. They need a native route at the owner first.
- **Files are Spotlight's.** One answer per query, the home folder only, no
  live updates, and nothing where Spotlight is turned off (the list says so).
- **Applications have a generic icon**, not their own.
- **The window opens on the main display**, a little above the middle, not on
  the display the pointer is on.
- **A message half-typed in the launcher is not carried into the workspace's
  composer** by "Open in Workspace". The conversation is; the unsent text
  stays in the launcher.
- **The Dock reopens the window only when no Platinum window is showing.**
  With Theo's face up, use New Tab or the launcher's "New Tab".
- The pure parts of `summon.rs` (conversations as rows, the conversation as
  the launcher draws it, why a message cannot be sent) have no tests of their
  own yet; they need the application's test build.

## Open decisions for the owner

- **The default key.** `alt+Space` is free on a stock Mac but types a
  non-breaking space in text fields while Platinum is not running, and other
  launchers claim it. Change `summon` in `theo.json`; there is no settings
  page for it yet.
- **Staying running by default.** On, because the launcher is of little use
  otherwise. If Platinum quitting with its window should stay the default,
  flip `Settings::default_resident`.
- **Two ways to talk to Theo.** The face's right-click menu and the launcher
  share one conversation and one driver. Whether the face's menu should now
  open the launcher instead is a product decision; nothing was removed.
