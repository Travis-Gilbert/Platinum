# Platinum

A standalone browser. [Servo](https://github.com/Travis-Gilbert/servo/tree/Travis-Gilbert/youtube-mse)
renders the web, [GPUI Kit](https://gpui-kit.com) draws everything around it,
and RustyWeb (from Theorem's `codex/rustyweb-programmable-web-2`, plus
`claude/rustyweb-synthesis-on-request` and `claude/rustyweb-full-frame` on
top of it) is the
default search engine, with its results page drawn natively.

## Using it

- **Right-click** anywhere: a menu opens at the pointer with its search field
  first and already focused, over the page's own entries and Platinum's tools.
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
- Platinum reopens the tabs it had, in their tree with their names and folds,
  side by side as they were, and the sidebar as it was, at its width. Each
  tab loads its page once it is the active tab, showing its site's icon kept
  from before.
- Drag a tab along its row to reorder it, onto another row to move it there,
  or onto the left, right, top or bottom of a page to see the two side by
  side. Each side keeps its own row of tabs; click a page to make it current.
- Back from a page that has nowhere to go returns to the search results.

The pins (`pins.json`), the tabs and their tree (`session.json`) and the
icons of the sites those name (`favicons/`) are kept in the profile's folder
in `~/Library/Application Support/Platinum`: `profile/` for the first profile,
`profiles/<id>/` for the others.

Search is local first. Every web page you open is kept in a local index in
`~/Library/Application Support/Platinum`, and search looks there before the web;
The app was called Frame before it was Platinum; a `Frame` folder there (and
in `~/Library/Caches`) is moved to the new name on first launch, with a `Frame`
link left in its place for builds from before the rename.
Ask answers from those kept pages. Queries and pages are embedded on this Mac
by Google's EmbeddingGemma 2 (about 314 MB, fetched from Hugging Face on first
run), on the GPU when there is one. A search waits at most four seconds for
the web, and afterwards Platinum fetches the results it has not kept yet, so the
next search on the topic finds them locally.
The chrome uses the Asciinema theme in dark mode and GitHub Sharp in light,
with the frame around each page drawn in the title-bar color (`#e1e4e8` in
light). On macOS 26 and later that frame, the row of tabs included, is
Liquid Glass: the title-bar color over the glass, at 55% in dark and 30% in
light, so the desktop shows through it. The glass follows Platinum's appearance,
not the system's. Pages, their toolbars and the selected tab stay opaque.
With Reduce Transparency on, or on an older macOS, the frame is solid.
As in Dia, the selected tab rises out of its page's toolbar, and both
take the color along the top of the page, following it as the page changes;
on a light page under the dark theme, or the other way round, their text and
icons turn dark or light to stay readable. A tab shows its site's icon once
the site has one. Tabs are spaced as Dia's are: each about 160 points wide,
narrowing when the row is full, with each branch of the sidebar's tree in a
rounded capsule of its own and New Tab just after the last tab.

## Browser profiles and migration

Choose **File > Import Browser Data…**, or the import icon at the bottom left of a new tab’s status bar.
The status bar appears only on the home page. Select a local Dia, Firefox, Chrome,
Chrome Beta, Brave, Edge or Chromium profile and the
categories to import. Profile names and account email labels are preserved in
separate destinations. Select **Open [profile] and restart** when the report is
ready. **File > Profiles…** also creates and switches profiles.

- Bookmarks include folders and account bookmarks. **Bookmarks > Show
  Bookmarks…** opens the library; **⌘D** saves the current page.
- **⌘Y** opens history. Typing a familiar title or address in the address bar
  suggests imported bookmarks and history; Down moves into the suggestions.
- Password import is opt-in and may prompt for macOS Keychain access. Passwords
  go into this application's own Keychain entries, separately for each profile.
  A Chrome-style password CSV and bookmarks HTML can also be imported into the
  **current** profile from the same sheet. No plaintext password copy is retained
  in application data. Remove an exported CSV yourself after import.
  Firefox passwords use Mozilla's installed NSS library. If the source has a
  Primary Password, unlock/export them in Firefox and use CSV import instead.
  Quit Firefox before importing if it locks its bookmark/history databases.
- On a secure login page, **Passwords > Fill Saved Password…** (**⇧⌘P**) lets
  you choose an account. Filling checks the exact origin and form destination;
  it never submits the form. Login forms are excluded from the local search index.
  Username-only steps are supported. Enter credentials
  and choose **Passwords > Save or Update Password…** to save a new or changed
  login after reviewing the account and site.
- Basic form information is optional and excludes passwords and payment fields.
  **Edit > Fill Form Information…** lets you choose one imported name, email,
  city or country value for a visible field on the current secure page.
- Open-tab import supports complete SNSS v3 session files and Firefox's
  compressed JSON session files. Only the selected navigation in each open
  Firefox tab is imported. Source windows are
  combined into one window. Incomplete, encrypted, unknown or navigation-pruned
  session files are reported as unsupported. Existing imported tabs are not
  added twice. Extension import inventories names and versions; it does not run
  the extensions.

For explicit local migration without opening the app, the profile crate also
provides `cargo run -p frame-profile --bin profile-import -- --browser dia`
(or `--browser firefox`). It imports all categories into separate profiles,
leaves the active profile unchanged, and prints aggregate counts only. Passwords
use the same Keychain authorization as the native importer.
`--verify-only` checks destination vault readability without decrypting source passwords.

Each profile has its own Servo storage, history, bookmarks, form values, search
store and restored session. Tabs, search queries, split layout and selection are
saved as they change. A restored search uses normal search; it does not start a
model request. The existing default profile remains in its original location.

Importing an account label does not transfer Google Sync, a Gmail authorization,
website cookies or passkeys. Websites may ask you to sign in again. Native
third-party plugins and WebExtension execution need additional integration.

The implementation and acceptance evidence are tracked in
[the migration plan](docs/plans/profile-migration.md). Engine-independent checks:

```bash
cargo test -p frame-profile
cargo clippy -p frame-profile --all-targets -- -D warnings
```

The explicitly invoked native Keychain tests use synthetic credentials and clean
up their test items. The Servo acceptance harness uses a temporary profile and a
local synthetic login server:

```bash
cargo test -p frame-profile -- --ignored
script/media-env cargo build --locked --example profile_check
script/media-env python3 tests/profiles/check_native.py /path/to/target/debug/examples/profile_check
```

## Search providers

Bluesky, which needs no key, is always on, ranked below the other providers.
Keyed general-web providers are used when their key is present in the
environment or in a `.env` file, either in the checkout (while developing) or
in `~/Library/Application Support/Platinum/.env`. `RUSTYWEB_SEARCH_PROVIDERS`
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
Platinum keeps a picture of its window there, redrawn every two seconds.

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
the same duotone as Platinum's others. The page, share, downloads and
appearance icons are redrawn from Streamline's Ultimate Duotone set as
single-color masks, so their duotone takes the theme's text color; the
downloads glyph is `sd-card-download`, from its CC BY 4.0 free subset.

The spatial canvas's glyphs (`assets/icons/canvas-*.svg`) are shapes from
[Lucide](https://lucide.dev), licensed ISC, with Feather's MIT licence for the
icons Lucide inherits from it. Both notices are in
`assets/icons/LICENSE-lucide.txt`. The glyphs are redrawn at the stroke width
of the icons above, with a lighter fill layer added so they take the theme's
text color as single-color masks.

The home status bar’s import icon is the supplied Streamline Ultimate
`App-Window-Download` PNG, embedded unchanged in an SVG so GPUI can tint its
alpha mask with the current theme.

## Brand

The app icon is the watercolor floppy painting,
`assets/brand/source/platinum-painting.png`. `script/brand-icons` rebuilds
`assets/brand/icons/` from it: `Platinum.icns`, `Platinum.ico` and a 1024 px
PNG. The logos are the Platinum brand kit (v1, 26 October 2026), a vector
drawing of the same disk, also kept in `assets/brand/`: `logos/` (horizontal
and stacked lockups, wordmark and one-color mark; names ending in `dark` are
dark ink for light backgrounds, `light` the reverse) and `source/` (the
editable master, compact and micro drawings). `src/brand.rs`
sets the `.icns` as the Dock icon at launch, since Platinum runs as a bare
binary; a bundle would name the same file in its `Info.plist`. The wordmark
is DejaVu Sans Bold, outlined to paths (`assets/brand/DejaVu-font-notice.txt`).
