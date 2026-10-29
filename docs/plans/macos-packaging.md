# Portable macOS packaging

Base: `649e01d79d06ec5f1c1c3d1c8d4c9c69041f82f3` (merged native Home).
The requested outcome is a self-contained Platinum.app and a reproducible path
from a local build to Developer ID signing and notarization. No publishing or
certificate purchase is part of this task.

## Plan

1. Package the executable, its pinned GStreamer plugin set, WebGPU runtime,
   transitive native libraries, icon, metadata, and third-party notices.
2. Verify all code resolves within the bundle or Apple's system libraries;
   reject missing libraries, outside symlinks, incompatible architectures, and
   external rpaths. Confine GStreamer and GIO module discovery on launch.
3. Provide explicit ad-hoc and Developer ID signing, nested-code verification,
   archive creation, and Keychain-profile-based notarization/stapling.
4. Exercise a relocated bundle with development paths denied, plus native
   Home/Servo navigation. Report certificate-dependent gates separately.

## Ownership and tooling

Python orchestrates Apple's tools; dylibbundler 1.0.5 owns dependency discovery,
copying, and relocation. macholib 1.16.4 reads binary metadata for independent
validation. Existing script/setup-media owns the pinned plugin list. Bundles
are assembled in a fresh staging directory and never overwrite an existing
output. A failed build is not published as the final app. Receipts contain
source revision, input hash, tool versions, bundled code hashes, and checks.

Signing modifies only the selected app. No keys are exported or stored by the
pipeline. Notarization accepts a Keychain profile name and requires explicit
invocation. Developer ID signing must fail when its certificate is missing;
ad-hoc signing must remain clearly identified as local validation.

## Validation

Implemented the pipeline and workflow. An optimized arm64 build and full
bundle acceptance are in progress. No Developer ID Application identity was
available in the current keychain search list at the start of this task.

- Eight packaging tests passed using real compiled Mach-O fixtures, including
  signature/resource validation, missing libraries and dynamic modules,
  external paths, and rejection of missing signing identities.
- `actionlint .github/workflows/macos-package.yml` passed.
- `python3 script/check-pins.py --require-deps` passed.
- `rustfmt --edition 2024 --check --config skip_children=true src/main.rs
  src/bundle.rs` passed. `cargo fmt -p frame -- --check` reports pre-existing
  formatting differences in unrelated files, including controls.rs and page.rs.
- The full-size initial bundle exposed a codesign layout constraint:
  arbitrary plugin subdirectories under Frameworks are interpreted as bundles.
  Plugins now live directly in Frameworks, with GIO's module in PlugIns; the
  signing fixture covers these directories and the internal plugin symlink.
- Full dependency packaging from the prior debug executable passed: 109 native
  code files, 39 pinned plugins, arm64, minimum macOS 27.0.0, valid ad-hoc
  signatures, no non-system external linkage. This checks the packaging graph;
  it does not claim that old executable contains the new bundle startup code.
- A separate copy with a `gst-launch-1.0` probe decoded MP4 and WebM fixtures
  through the packaged libraries to EOS. A GIO probe loaded GTlsBackendGnutls.
  Both ran with reads denied under Homebrew, the development dependency tree,
  Cargo, this checkout, and the ONNX download cache. The denial was independently
  confirmed with a failing file read. This is native codec/module acceptance,
  not in-browser playback. Results: [media](evidence/macos-packaging-2026-10-29/media.json),
  [GIO](evidence/macos-packaging-2026-10-29/gio.txt).

Certificate-dependent gates remain: timestamped Developer ID signatures,
hardened-runtime execution, Apple notarization/stapling, and downloaded-artifact
Gatekeeper acceptance. A separate clean Mac is still needed for release
compatibility acceptance. Public redistribution also needs a completed license
review: Cargo declares MIT but the project has no root LICENSE, and the
prebuilt Dawn input has no adjacent license notice. The inventory records the
available upstream notices and native package provenance without inventing
missing terms.
