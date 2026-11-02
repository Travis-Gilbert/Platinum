Theorem's whiteboard fork is the best starting point for the canvas, and zorite's editor and markdown crates are worth forking. None of the three crates drops into Frame as is. Nothing was built. zorite was read at packetThrower/zorite@6ee46f28cbe0017f7f152033227d897434ddd0ef.

In the citations, `ed`, `md` and `wb` are zorite's crates zorite-editor, zorite-markdown and gpui-whiteboard. `fork`, `ts` and `tsg` are Theorem's whiteboard fork, theorem-space and theorem-space-gpui on origin/main.

## 1. Licenses
- **zorite repo:** GPL-3.0-or-later (LICENSE:1-2, Cargo.toml:6). The app also pulls in the AGPL heic_decoder (Cargo.toml:199-204); the crates don't.
- **The three crates:** all MIT (each crate's Cargo.toml:6), each with its own LICENSE file. So all three differ from the repo, on purpose: their READMEs say the crates are MIT and only the app is GPL.
- **gpui-bidi**, which ed and md need, is also MIT. wb bundles JetBrains Mono under OFL.
- All of this is compatible with Frame's MIT license.

## 2. GPUI pin
- **No zed git rev.** Cargo.toml:40 has `gpui = { package = "gpui-pre", version = "0.3" }`, locked at 0.3.2 (Cargo.lock:2830). docs.rs metadata confirms 0.3.2 is zed@801c087, from 2026-09-03.
- **App-only:** gpui-component =0.6.0 (Cargo.toml:143) and ropey. None of the three crates uses them.
- **Crate dependencies:**
  - wb: gpui, serde, serde_json, log, ttf-parser 0.25.
  - md: markdown 1, with gpui and gpui-bidi behind the default `view` feature.
  - ed: gpui, unicode-segmentation, md, gpui-bidi.
- All are edition 2024. The versions they share with Frame's lockfile match.

## 3. zorite-markdown (0.10.0, about 6.3k lines)
It is a read-only GPUI view, `MarkdownView`, built on StyledText. It parses with `gfm()` plus math (view.rs:789-791), caches parse results, and exposes mdast types in its API (view.rs:185).

Obsidian syntax:
- **Supported:** `[[t]]`, `[[t|alias]]`, `[[t#Heading]]`, `[[t#^id]]` (syntax.rs:487-494, 701-726), `==highlight==`, hidden `^id` markers and tasks.
- **Partial:**
  - Embeds work only as a `![[x]]` on its own line (769-774). The host resolves them through EmbedProvider (view.rs:379-383).
  - Callouts cover only GitHub's five kinds. Folding works; custom titles don't (syntax.rs:11-65).
  - Tags are ASCII only (497-499).
- **Missing:**
  - Frontmatter: `gfm()` leaves it off (markdown-1.0.0 configuration.rs:395-404), and neither crate has frontmatter code.
  - `%%comments%%`.
  - Clickable footnotes.
- **Logseq-style extras:** `key:: value` properties and `((id))` references.

API: about 21 builder methods (view.rs:523-700), 15 callback types and 50 syntax functions. It has 57 unit tests, none of which use gpui.

## 4. zorite-editor (0.11.2, about 16.6k lines)
- **What it is:** an editor written from scratch for GPUI. It edits plain text until `set_markdown_style` is called, then shows a live markdown preview (lib.rs:1-30).
- **Storage:** one `String` addressed by byte offsets. The selection and the IME range are byte ranges (771-778). It doesn't use ropey.
- **IME:** a full `EntityInputHandler` (5153-5300).
- **Undo:** saves a snapshot of the whole document (278-290, 2986-3054).
- **Highlighting:** supplied by the host (1395).
- **Vim:** none.
- **Decorations:** a fixed set of block and line types (`Block`, `LineMark`, 6334-6532). The host can also supply images, math, mermaid and embedded views (1302-1379). There is no general way to decorate a text range.
- **Coupling:** none to zorite's app state, but Logseq conventions are built in. On load, `normalize_loaded` rewrites `$$` math fences (373-382).
- **API:** about 80 public methods and 10 `EditorEvent` variants (496-562).
- **Tests:** 65 unit tests plus 2 gpui tests (IME and keys). element.rs and tables.rs have none.

**Can an external CRDT drive the buffer?** Not as shipped. The `Changed` event carries no data (498-500), and there's no way to apply a remote edit.

**The seam is the private `record_edit(range, new_text)` at lib.rs:3025.** It runs just before each of the 17 places where text is spliced:
- lib.rs: 1208, 1452, 2540, 2803, 2840, 3249, 3281, 3791, 4019, 5202.
- tables.rs: 496, 543, 575, 618, 659, 720, 962.

Three paths skip it: IME composition (5233-5234), undo restore (3013-3015) and `set_text`.

To drive it from Loro:
1. Emit an edit event from `record_edit` and from the three paths that skip it.
2. Add an `apply_remote` that shifts the selection, IME range and diagnostics without touching undo. `remap_diagnostics` (1972) shows the pattern.
3. Hand undo to Loro.
4. Stop `normalize_loaded` from making the editor text differ from the CRDT text.

Separately, every keystroke rebuilds the whole `String` (5203-5204).

## 5. gpui-whiteboard (0.5.1, about 12.4k lines)
- **Model:** `Scene{camera, elements}` (scene.rs:13-21). Each `Element` has a `u64` id, a kind, stroke, fill, label, styles and mind-map data (49-82).
- **Element types:** freehand drawing, seven shapes, Line, Arrow, Text, Embed and Image (357-374). Connectors attach to one of an element's four sides (446-481).
- **Serialization:** its own JSON format. `from_json` returns an empty board if the input is bad (26-40).
- **Undo and saving:** both work on the whole scene; every flush writes the full JSON (lib.rs:2617-2716). There's no API to replace or patch a scene from outside.

Fit against JSON Canvas (I re-checked the spec):
- Connector sides map directly.
- There are no groups, link nodes, subpaths, string ids or preset colors.
- Labels are painted only on closed shapes (paint.rs:377-379), so edges never show labels.
- Arrowheads are drawn only at an edge's end (paint.rs:1163-1188).

Missing for Obsidian Canvas parity:
- Live markdown note cards.
- Groups with a label and background.
- Labelled, colored edges with arrows at either end.
- Link cards, and file cards with a subpath.
- Ids and colors that survive a round trip.

## 6. Theorem's fork
- **Base:** upstream 0.5.1 at zorite 4aad847, 2026-10-01 (FORK.md:1-7).
- **What Theorem added** (FORK.md:19-39 lists 19 changes):
  - Parsing that keeps elements it doesn't recognize.
  - Host-managed history (`apply_external`, Commit events).
  - A gesture engine that runs without a view.
  - Hooks for fonts, camera, constraints and menus.
  - Rotation.
  - The Space adapter.
  - `unsafe` forbidden.
- paint.rs and input.rs are unchanged from the base.

**What the fork lacks:** five upstream commits since its base, still at 0.5.1:
- Pan bounds (3586a48).
- A fix for dashed lines stalling and exhausting memory (7c11533).
- A minimum stroke width (077cd9a).
- Glyph and mesh caches (c963c3c, b5e9121).

7c11533 matters most: the fork still has the old dash loop (fork paint.rs:1101-1121), and it draws every semantic edge dashed (fork space.rs:775-779).

**JSON Canvas fields supported today:**
- **theorem-space** models every node type and every edge field (jsoncanvas.rs:32-116). A round trip still loses data:
  - Export never writes `background` or `subpath` (146-147, 189-190, 212-216).
  - Import drops `subpath` (326), group `background`/`backgroundStyle` (303-314) and unknown fields.
  - Import also replaces every id with a new one (357, 402).
- **theorem-space-gpui:**
  - Draws edge sides, arrows at both ends and labels at the midpoint (edge.rs:17-31, 71-160, 218-221).
  - Doesn't draw edge colors: neither the edge frame nor the projected connection has a color field (ts relation.rs:369-381).
  - Handles preset and hex colors for cards and groups (theme.rs:343-354).
  - Shows note cards as plain text, not markdown (card.rs:890-909).
- **The fork's own board view:** from reading the code, it would show at most one arrowhead per edge and no edge labels (fork space.rs:783-789, paint.rs:362-364). I didn't run it.

## 7. Compatibility
All three projects use zed's gpui 0.2.2, but at different commits:
- **zorite:** zed 801c087, 2026-09-03.
- **Theorem:** Travis-Gilbert/zed 6d4d907. That is upstream zed as of 2026-09-03 (801c087 is an ancestor) plus 39 Theorem commits. Theorem pulls it from git as crate `gpui`, with gpui-component 0.7 (Theorem apps/theoremweb/Cargo.toml:117-146).
- **Frame:** gpui-pre 0.3.9, which is zed@93d7456 (PR #65448, around 2026-10-10).

Neither zorite's dependency graph nor Theorem's unifies with Frame's as is. All the source imports `gpui::…`, though, so moving to Frame means editing Cargo.toml and fixing whatever API changed between the zed commits.

Every gpui name the four crates import exists in 0.3.9: 79 in ed, 43 in md, 60 in wb and 27 in gpui-bidi. I checked names only, not signatures, with a scan of each crate's `gpui::` imports.

None of the crates uses gpui-component, so nothing conflicts with Frame's 0.8.0. theorem-space needs only rstar 0.12.2, which is already in Frame's Cargo.lock:10422.

Port size by API surface:
- **zorite-markdown:** one view, 21 builder methods, 15 callback types and 50 syntax functions, plus gpui-bidi.
- **zorite-editor:** 80 methods, 10 events, a custom Element, the IME implementation, and the 20 seam sites (17 calls plus 3 bypasses).
- **gpui-whiteboard:** 49 public functions, 3 views and 15 hook types.
- **Theorem's fork:** adds about 3.9k lines of modules and 3.9k lines of tests. It uses only two font functions from theorem-design-gpui (fork font.rs:18, 149).

## 8. Verdicts
- **zorite-markdown: fork.** The `syntax` module works as is. The view needs frontmatter, Obsidian callouts, inline embeds, Unicode tags and `%%` comments.
- **zorite-editor: fork.** It's the strongest base here: IME, live preview and tables. It still needs the CRDT seam, undo handed to Loro, and a move to gpui-pre 0.3.9.
- **gpui-whiteboard: fork, starting from Theorem's fork.** Rebase it onto upstream 6ee46f2, taking 7c11533 first, and swap in a Frame font provider. Upstream on its own can't let the host manage history or patch scenes.
- **theorem-space: adopt** with default features, once the round-trip losses are fixed.
- **theorem-space-gpui: reference only.** It depends on Theorem's agpui shell (tsg Cargo.toml:13-38).

## Not verified
- Whether the crates compile against gpui-pre 0.3.9.
- How frontmatter renders today.
- How the fork actually draws edges at runtime.
- Whether `![[image]]` works inside embeds.
- The exact date of zed 93d7456.
- Whether Frame needs zorite's gpui-pre-wgpu patch for combining marks (Cargo.toml:306-307).
