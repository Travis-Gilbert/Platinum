# Platinum profile migration

Requested 2026-10-25 from the supplied development-preview document. The current
application and storage names remain Frame; this change preserves existing data.

## Delivered

- Durable profile catalog with imported names/account email labels, separate
  Servo storage, search stores, history, bookmarks, form values and Keychain
  namespaces. The default profile retains its existing paths. Switching restarts
  the application through GPUI's normal lifecycle.
- Native import/profile/library sheet, menu and command-palette entry points,
  first-run/new-tab import button, category choices and per-category reports.
  Dia, Firefox, Chrome, Chrome Beta, Brave, Edge and Chromium source discovery on macOS.
- Bookmarks and folders from Bookmarks and AccountBookmarks, plus HTML exports.
  Bookmark command and searchable library. History uses SQLite's consistent
  online backup API, includes WAL data, normalizes timestamps and feeds native
  address-bar suggestions and history lookup.
- Opt-in direct macOS Chrome-family password import, including account login
  databases; OS authorization failure remains isolated from other categories.
  CSV fallback streams to a per-profile Keychain service. Destination SQLite
  contains account labels, not passwords. Existing logins are skipped on retry;
  explicit save/update changes the vault entry.
- Account chooser, exact-origin HTTPS/localhost fill with renderer-time origin
  and same-origin form-action checks, visible fields, username-only steps,
  explicit save/update confirmation, and no automatic submission or clipboard
  exposure. Login/one-time-code forms are excluded from local search capture.
- Selective basic form import/fill, excluding payment/password fields, and
  extension name/version inventory with an explicit unsupported-port status.
- Supported open-tab import through the pinned snss-core decoder. Only the newest
  Session file participates; closed objects and unsafe URLs are excluded, retries
  do not multiply tabs, and incomplete/pruned/unknown formats report failure.
  Source windows combine into one native window.
- Session persistence for tab order, URLs, search queries, selected tab and GPUI
  split layout. Restoring queries uses normal search, never an automatic model
  request. Orderly Servo shutdown flushes its own persistent website state.

## Validation

Evidence: [profile-migration-2026-10-25](evidence/profile-migration-2026-10-25/).

- `cargo test -p frame-profile`: 13 passed. Two native tests are opt-in rather
  than part of the default suite (one unit test and one integration test).
- `cargo test -p frame-profile -- --ignored`: both native Keychain tests passed.
  One exercises a real synthetic Chrome Login Data database and source Safe
  Storage key, decryption, native destination vault storage and retry behavior.
  Synthetic Keychain entries were removed.
- `cargo clippy -p frame-profile --all-targets -- -D warnings`: passed.
- `script/media-env cargo test --locked --bin frame`: 55 passed. UI interaction
  coverage includes source selection/import into a separate profile, address-bar
  suggestion focus/keyboard dismissal, profile-library opening and session tab
  selection. Existing browser tests also pass.
- Native Servo acceptance: CSV import -> Keychain retrieval -> origin-bound
  form fill -> actual local server login -> quit -> fresh-process saved-tab and
  authenticated-cookie restoration passed. The probe also verifies that the
  production document-capture script excludes the login form. All credentials
  and source data are synthetic; temporary profiles and vault entries are cleaned
  up. This does not establish compatibility with a person's real websites.
- Native GPUI profile panel rendered and inspected; see `profiles.png`. The
  native backend's existing screenshot feature is enabled only for development
  targets. No screen-recording or external-desktop capture is used.
- Final `script/media-env cargo build --locked` passed. `git diff --check` passed.
  Build evidence is recorded alongside the tests.
  The graph retains existing Servo/Theorem dependency warnings.

Native probe setup initially lacked the development GStreamer plugin links;
`script/setup-media` installed the project's normal links. The probe waits for a
painted, completed document before filling. Initial failed attempts are not
counted as acceptance. Runtime media behavior was not changed in this task.

## Dependencies and implementation choices

Use rusqlite's online backup API, security-framework for Keychain, maintained
AES/CBC/PBKDF2 implementations for Chrome's macOS format, csv for exported
credentials, scraper/html5ever for HTML bookmarks and snss-core for session
framing/Pickle/replay. The small session adapter filters closed objects and
rejects navigation-pruned logs because the decoder does not replay those
commands. No Chromium rendering runtime was introduced.

References inspected:

- [Mozilla macOS Chrome login format](https://github.com/mozilla-firefox/firefox/blob/main/browser/components/migration/ChromeMacOSLoginCrypto.sys.mjs)
- [SQLite online backup](https://sqlite.org/backup.html)
- [security-framework password API](https://docs.rs/security-framework/3.7.0/security_framework/passwords/index.html)
- [snss-core 0.1.1](https://docs.rs/snss-core/0.1.1/snss/)
- [Chromium session command definitions](https://github.com/chromium/chromium/blob/main/components/sessions/core/session_service_commands.cc)

## Remaining acceptance and capabilities

- Personal Dia and Firefox migration was exercised as recorded below. Actual
  everyday website logins through the normal native UI remain an acceptance
  step; the migration does not establish those providers' sign-in compatibility.
- Direct import deliberately supports macOS v10 password blobs. Other formats
  and OS authorization refusal use CSV fallback. Passkeys need native provider
  integration and are not passwords or CSV records.
- Imported account email is metadata. Existing Google/Gmail sessions, Chrome
  Sync, a native mail account authorization and Platinum account synchronization
  require their own provider contracts and are not created by this importer.
  Source website cookies/session tokens are not imported.
- SNSS v3 is the supported session format. Navigation pruning, incomplete logs
  and other versions need a more complete upstream replay adapter or a supported
  export. Window geometry and multiple native windows are not restored; Frame
  currently owns one native window, with its existing dock splits preserved.
- Extension execution, selected WebExtension ports, third-party GPUI Shell plugin
  loading and a shared plugin/agent browser capability API still need host and
  engine integration. The imported inventory now supplies the concrete port list.
- No publication, deployment, browser announcement, full product rename or
  production packaging occurred. Changes remain local for review.

## Home status bar follow-up (2026-10-25)

The home pane now uses GPUI Kit's StatusBar with the import command in its left
region. The command is icon-only, with an accessible name and tooltip, and opens
the existing importer. The centered import button was removed. Status bars are
restricted to home panes, including split layouts; search results and web pages
retain their existing chrome. The supplied Streamline App-Window-Download PNG is
embedded unchanged in an SVG for the existing theme-aware icon mask renderer.

Validation: the final development build passed; all 56 application tests passed,
including bottom-left geometry,
opening/dismissing the importer and visibility across home/search tab switching.
Native dark/light renders were inspected using the production Browser and an
isolated empty profile. Images and the updated test run are in the existing
evidence folder. The temporary render harness was removed after capture.

Liquid Glass audit: previews used the application's real `glass::install` and
transparent Root, with native glass active in both themes. Native view ordering
confirmed NSGlassEffectView below GPUI. GPU-layer captures confirmed translucent
outer left/bottom frame pixels and rounded card corners, while the footer itself
remained opaque (alpha 255). Frame tint stays 0.55 dark / 0.30 light. The light
theme's dark status-bar background uses the existing contrasting chrome-button
treatment so the import icon and hover remain legible. These captures inspect
the GPUI layer and native view order, not a composited desktop screenshot.


## Dia and Firefox import follow-up (2026-10-25)

The explicit personal-import request added Dia discovery and a Firefox adapter.
Firefox profiles.ini is parsed with configparser; Places and form-history data
use SQLite online backups, including committed WAL contents. History preserves
Unix microseconds. Folder paths are retained. lz4_flex decodes Mozilla's session
container; only each open tab's selected navigation is retained, with a bounded
uncompressed size. Closed tabs and browser-internal URLs are excluded.

Firefox passwords use Mozilla NSS from an installed Homebrew or Firefox library,
with a narrow libloading bridge for the official NSS C ABI. Existing Rust NSS
wrappers inspected did not expose the standalone profile SDR workflow. This
bridge does not implement Firefox cryptography. NSS receives encrypted SQLite
snapshots in an owner-only temporary directory, opens them read-only and clears
native plaintext allocations. The original key databases are never opened by
NSS. A Primary Password requires Firefox's own unlock/export flow and CSV import;
no Primary Password is requested through chat or command arguments.

The explicit local `profile-import` utility uses clap and the same production
adapter/vault APIs as the native sheet. It imports all categories, keeps sources
in separate destinations, leaves active-profile selection unchanged and emits
only aggregate counts. Passwords run last so authorization cannot block other
categories. `--without-passwords` supports completing the independent categories.

Validation: 18 ordinary profile tests passed and the explicit synthetic native
NSS encryption -> snapshot -> login import -> password retrieval -> retry test
passed. Strict profile Clippy passed. All 56 native application tests passed.
The final development build passed, retaining existing upstream warnings.
Logs are alongside the existing evidence. The fixtures cover Firefox discovery,
relative-path rejection, placeholder profiles, WAL snapshots, folders, timestamp
units, category filters, compressed sessions and retry deduplication.

Authorized live imports from populated Dia and Firefox profiles completed.
Destination Keychain read-back confirmed Firefox credentials and Dia HTTPS
credentials. One Dia HTTP login is stored but remains ineligible for filling.
Storage verification now uses a separate aggregate-only vault check, covered by
an HTTP regression test. `--verify-only` avoids source-key authorization during
this check; its additional destination Keychain authorization did not complete,
so the optional verification process was canceled.

Firefox initially locked Places and form-history databases. After the user
authorized closing it, the remaining categories imported successfully. Repeated
imports added no duplicate rows or tabs, and hashes confirmed Firefox's source
database/login files remained unchanged. Dia's source Safe Storage lookup needed
interactive macOS authorization; import completed after the user approved it.
Detailed personal results remain in local application storage.

The native sheet and utility discover the same sources. Imported tabs were stored
without opening the sites or switching the active profile. Names/account labels
are metadata; browser sync, website sessions and extension execution are not
created by the import. No real website login or network account operation was
performed for this migration. Personal rows and plaintext credentials are absent
from repository evidence and tool reports.
