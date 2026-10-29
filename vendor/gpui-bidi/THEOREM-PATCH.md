# gpui-bidi 0.1.1, carried by Frame

Forked from `packetThrower/zorite@6ee46f28cbe0017f7f152033227d897434ddd0ef`.
This crate's manifest and LICENSE are MIT; its upstream copyright notices
and license are retained. The GPL application is not included.

Why: Frame's Markdown editor uses the zorite crates on Frame's existing
GPUI Kit 0.8.0 / gpui-pre 0.3.9 graph.

The changes:

- `Cargo.toml`: disable publishing and replace the inherited `gpui`
  dependency with `gpui-pre = "=0.3.9"`, retaining the `gpui` import name
  and using Frame's existing workspace patches.

No production Rust API changes were required by the gpui-pre 0.3.9 build.
No gpui-component/GPUI Kit API is consumed by this crate. Frame's existing
GPUI patches are unchanged; zorite's WGPU combining-mark patch is not used
on Frame's Apple text/Metal path.

Re-applying on an upstream upgrade: copy this MIT crate, carry the manifest
changes and any test additions above, and run Frame's focused offline
build, tests and clippy for all three forks. Compare the workspace GPUI
versions and features and the lockfile against the pre-upgrade graph.
