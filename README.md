# Frame

A standalone browser. [Servo](https://github.com/Travis-Gilbert/servo/tree/Travis-Gilbert/youtube-mse)
renders the web, [GPUI Kit](https://gpui-kit.com) draws everything around it,
and RustyWeb (from Theorem's `codex/rustyweb-programmable-web-2`, plus
`claude/rustyweb-synthesis-on-request` and `claude/rustyweb-full-frame` on
top of it) is the
default search engine, with its results page drawn natively.

## Using it

- **Right-click** anywhere: a menu opens at the pointer with its search field
  first and already focused, over the page's own entries and Frame's tools.
  Type to filter those and find your tabs and every command, or to open an
  address, search the web, or Ask. Right-click a tab for its own entries:
  Rename, Pin Tab, Close Tab and, for a branch, Close Branch.
- **⌘K** opens the same search as a palette, also when the toolbar is hidden.
- **⌘L** focuses the address bar. An address opens; anything else searches.
  A leading `?` always searches. ⌘Return opens in a new tab. Away from the
  field, the bar shows the page as "host / title".
- **Ask**, the button at the end of the address bar, turns the AI on (filled)
  and off (outlined). While it is on, the field reads "Ask..." and what you
  type is answered rather than searched. The palette and View > Ask AI do the
  same.
- **Share**, after Ask, opens macOS's share sheet for the page: Mail,
  Messages, AirDrop, Notes and the rest. File > Share... and the palette do
  the same.
- **Downloads**: a link to a file (a `.dmg`, `.zip`, `.pdf` and the like), a
  file Servo cannot show, or the right-click entries Download Linked File and
  Download Image save to your Downloads folder, with the page's cookies. The
  list opens from the toolbar's downloads button, which shows progress while
  anything is downloading, from **⌥⌘L** or from View > Show Downloads. Click a
  saved file to open it, or its folder button to show it in Finder.
- **Appearance**, the toolbar's last button, switches between Light and Dark
  (the default). Match System, which follows the system as it changes, is in
  View > Appearance and the palette.
- **Ctrl⌘C** toggles compact mode: the chrome hides behind a 6 px frame and
  comes back when the pointer reaches the top edge. Compact mode is also in
  the palette and the View menu.
- Tabs: ⌘T, ⌘W, ⌘1 to ⌘9, Ctrl-Tab, ⌘⇧[ and ⌘⇧]. The numbers and
  Ctrl-Tab count the pinned sites' open tabs first, then the tabs as they
  are drawn.
- **Ctrl⌘S** opens the sidebar (also the button at the start of the rows of
  tabs, View > Sidebar and the palette): the pinned sites as a grid of their
  icons, and under them the tabs as a tree, in place of the rows along the
  top. A page's links that open new tabs, and results opened in a new tab,
  hang under the tab they came from; ⌘T starts a new branch at the top. A
  row's chevron folds its branch. Right-click a row to fold it, Pin it,
  Rename it, Close it, or Close Branch: it and every tab under it. Drag a row
  onto another's top or bottom edge to put it beside that one, onto its
  middle to put it inside, onto New Tab to bring it to the top level, onto
  the pins to pin it, or onto a page's edge to see the two side by side, as
  with a tab from the rows. Drag the sidebar's edge to widen it;
  double-click the edge for its first width.
- **Pinned sites** keep their site's icon and own one tab, outside the tree.
  Without the sidebar they sit in a tray at the start of the first row of
  tabs, as in Dia. A pin's button opens the site, or goes to its tab once
  open; closing that tab leaves the pin. The tray's pin button, File > Pin
  Tab, the palette, Pin Tab in a tab's right-click menu, or dropping a tab
  on the tray or the sidebar's pins pins a site; right-click a pin to Unpin
  it.
- Frame reopens the tabs it had, in their tree with their names and folds,
  and the sidebar as it was, at its width. Each tab loads its page once it
  is the active tab, showing its site's icon kept from before.
- Drag a tab along its row to reorder it, onto another row to move it there,
  or onto the left, right, top or bottom of a page to see the two side by
  side. Each side keeps its own row of tabs; click a page to make it current.
- Back from a page that has nowhere to go returns to the search results.

The pins (`pins.json`), the tabs and their tree (`session.json`) and the
icons of the sites those name (`favicons/`) are kept in
`~/Library/Application Support/Frame`.

Search is local first. Every web page you open is kept in a local index in
`~/Library/Application Support/Frame`, and search looks there before the web;
Ask answers from those kept pages. Queries and pages are embedded on this Mac
by Google's EmbeddingGemma 2 (about 314 MB, fetched from Hugging Face on first
run), on the GPU when there is one. A search waits at most four seconds for
the web, and afterwards Frame fetches the results it has not kept yet, so the
next search on the topic finds them locally.
The chrome uses the Asciinema theme in dark mode and GitHub Sharp in light,
with the frame around each page drawn in the title-bar color (`#e1e4e8` in
light). On macOS 26 and later that frame, the row of tabs included, is
Liquid Glass: the title-bar color over the glass, at 55% in dark and 30% in
light, so the desktop shows through it. The glass follows Frame's appearance,
not the system's. Pages, their toolbars and the selected tab stay opaque.
With Reduce Transparency on, or on an older macOS, the frame is solid.
As in Dia, the selected tab rises out of its page's toolbar, and both
take the color along the top of the page, following it as the page changes;
on a light page under the dark theme, or the other way round, their text and
icons turn dark or light to stay readable. A tab shows its site's icon once
the site has one. Tabs are spaced as Dia's are: each about 160 points wide,
narrowing when the row is full, with each branch of the sidebar's tree in a
rounded capsule of its own and New Tab just after the last tab.

## Search providers

Bluesky, which needs no key, is always on, ranked below the other providers.
Keyed general-web providers are used when their key is present in the
environment or in a `.env` file, either in the checkout (while developing) or
in `~/Library/Application Support/Frame/.env`. `RUSTYWEB_SEARCH_PROVIDERS`
names an explicit list instead. Ask needs `THEOREM_WEB_SYNTHESIS_CONFIG`.
Results are reranked on OpenRouter (Voyage Rerank 3 Lite, then Cohere Rerank
4 Pro) when an OpenRouter key is in the login Keychain under
`theorem-openrouter`.
Never put keys in settings files or shell rc files.

## Building

macOS only for now.

Media uses Servo's GStreamer backend. Install its development runtime first:

```bash
brew install gstreamer libnice-gstreamer meson ninja
```

```bash
FRAME_TARGET_DIR="/Volumes/SSD/cargo-targets/frame" script/bootstrap
```

```bash
script/bootstrap-media
script/media-env cargo build --locked
script/setup-media
script/media-env cargo run --locked
```

`script/setup-media` links the plugins required by the pinned Servo version
beside the development executable and examples. Run it again after changing
the Cargo target directory; use `--release` for a release build. These links
depend on the local GStreamer installation and are not a portable app bundle.

YouTube compatibility is tracked in [the media plan](docs/plans/youtube.md).
`script/bootstrap-media` checks out the pinned Servo commit with the media
and rendering fixes, then applies `patches/gstreamer-mse.patch` to pinned
GStreamer sources. `script/media-env` selects the patched
MSE runtime. This work is under validation; see the plan for exact acceptance
evidence and remaining YouTube limitations.

`script/bootstrap` checks out the pinned Theorem commit under `deps/` (from a
sibling `../Theorem` clone when there is one) and can put Cargo's target
directory on another disk; a Servo build needs about 25 GB. Use
`cargo build`, not `cargo check`: check rebuilds Servo from scratch.

To see the window without a screenshot (which needs the Screen Recording
permission), build with the development `snapshot` feature and name a file:
Frame keeps a picture of its window there, redrawn every two seconds.

```bash
script/media-env cargo build --features snapshot
FRAME_SNAPSHOT=/tmp/frame.png script/media-env target/debug/frame
```

## Credits

`src/engine.rs`, `src/scene.rs`, `src/keys.rs`, `src/delegate.rs`,
`src/page.rs` and `src/controls.rs` come from Theorem UI's desktop host
(`Theorem-UI/desktop/src`), adapted for tabs and for drawing pages inside
GPUI's scene. `vendor/` holds two small patches, each described in its
`THEOREM-PATCH.md`.

The Ask icons (`assets/icons/ask.svg`, `ask-on.svg`) are Streamline's
"design file AI", licensed CC BY 4.0. The pin, unpin and rename icons
(`pin.svg`, `pin-off.svg`, `rename.svg`) are Lucide's, ISC licensed, given
the same duotone as Frame's others. The page, share, downloads and
appearance icons are redrawn from Streamline's Ultimate Duotone set as
single-color masks, so their duotone takes the theme's text color; the
downloads glyph is `sd-card-download`, from its CC BY 4.0 free subset.
