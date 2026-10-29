# Building a portable Platinum.app

The package contains the browser, the pinned Servo GStreamer plugin set, all
non-system dynamic dependencies, the GIO TLS module, Dawn/WebGPU, the icon, and
dependency notices. Bundled startup confines GStreamer and GIO module discovery
to shipped code and ignores machine-local GStreamer registry caches.
The Cargo executable remains `frame`; profile and Keychain storage are unchanged.
The new application bundle identifier is `dev.platinum.browser`.

## Prepare and build

Use the normal pinned dependency bootstrap from the README first. Packaging
needs macOS, Xcode command line tools, Python 3.11+, and dylibbundler 1.0.5.
The Python tools are build-time dependencies; users do not need Python or
Homebrew to run the resulting app.

```sh
brew install dylibbundler glib-networking
python3 -m venv .packaging-venv
.packaging-venv/bin/pip install -r packaging/macos/requirements.txt
.packaging-venv/bin/python script/package-macos build --output dist/Platinum.app
```

The build command runs `script/media-env cargo build --locked --release --bin
frame`, then packages that artifact. Set `CARGO_TARGET_DIR` to reuse an existing
build cache. `--snapshot` includes the existing native acceptance harness.

For an already-built binary, use the explicit packaging command. Its manifest
records the input hash and marks build provenance as unspecified:

```sh
.packaging-venv/bin/python script/package-macos package \
  --binary /absolute/path/to/target/release/frame --output dist/Platinum.app
```

Output must not already exist. Packaging occurs in a new staging directory,
then independently checks dependency paths, architectures, minimum OS versions,
pinned plugins, and internal symlinks. It applies an ad-hoc signature for local
execution and verifies the whole resource envelope before publishing the app
at the requested path. The adjacent receipt contains hashes and validation
results; the relocation log records the source library locations.

`LSMinimumSystemVersion` is derived from the bundled Mach-O binaries. This is
their loader requirement, not proof of full behavior on every older macOS.
The app has the architecture of its input executable; this pipeline does not
claim an Intel or universal build when supplied only an arm64 build.

## Sign when the certificate is available

Install the Developer ID Application certificate and its private key in the
signing user's Keychain. Do not put the key or its password in this repository.
Use a unique certificate label or fingerprint from Keychain's valid identities:

```sh
.packaging-venv/bin/python script/package-macos sign dist/Platinum.app \
  --identity 'Developer ID Application: Your Organization (TEAMID)'
.packaging-venv/bin/python script/package-macos verify dist/Platinum.app --developer-id
```

The command checks the identity before changing files, signs nested code first,
then signs the app with a secure timestamp and hardened runtime. It uses
`packaging/macos/entitlements.plist`, matching the pinned Servo host's JIT and
unsigned executable-memory requirements. It does not disable library validation
or grant debugging access. Real Developer ID validation remains a required gate;
ad-hoc signing deliberately does not claim hardened-runtime acceptance.

## Notarize and archive

Create a notarytool Keychain profile using Apple's interactive credential
workflow, or your organization's supported App Store Connect API credentials.
Only the profile name is passed to this pipeline; credentials stay in Keychain.
Then explicitly submit the signed app:

```sh
.packaging-venv/bin/python script/package-macos notarize dist/Platinum.app \
  --keychain-profile platinum-notary
.packaging-venv/bin/python script/package-macos archive dist/Platinum.app \
  --output dist/Platinum.zip
```

Notarization refuses ad-hoc signatures, waits for an Accepted result, staples
and validates the ticket, and checks Gatekeeper. Archive **after** stapling so
the downloadable ZIP contains the ticket. Archive can also create an explicitly
unnotarized local-test ZIP; it never represents that ZIP as trusted distribution.
No command publishes a GitHub release or uploads to a distribution site.

## Validation

```sh
.packaging-venv/bin/python -m unittest discover -s tests/packaging -v
.packaging-venv/bin/python script/package-macos verify dist/Platinum.app
```

The tests compile small real Mach-O fixtures, verify dependency rejection,
resource tampering and identity failures, and execute a relocated signed fixture.
For a snapshot-enabled build, run the packaged browser's native harness from a
new location with a fresh output directory:

```sh
FRAME_WORKSPACE_CHECK=/absolute/path/to/evidence \
  /absolute/path/to/Platinum.app/Contents/MacOS/frame
```

This uses a temporary Servo profile, synthetic search fixtures, actual native
rendering, real Servo navigation, Back, and Command-Return. It is not live-provider,
media playback, or model inference acceptance. Before distributing a release,
exercise its Finder launch, navigation, search, media, downloads and profile
persistence on a clean Mac; run Gatekeeper against the downloaded artifact.
Review the bundled dependency notices and source/redistribution obligations.

References: [Apple notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution),
[dylibbundler](https://github.com/auriamg/macdylibbundler),
[macholib](https://macholib.readthedocs.io/).
