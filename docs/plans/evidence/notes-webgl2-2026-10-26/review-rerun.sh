#!/bin/bash
# Run outside the restricted sandbox. Existing base and mine2 binaries are preserved.
set -u
lane='/Volumes/SSD Samsung/frame-webgl2-lane'
servo_link='/private/tmp/claude-501/-Users-travisgilbert-Tech-Dev-Local-Creative-Frame/dc9b58c8-3296-4e2f-aafe-6e51633e23d2/scratchpad/webgl2-servo'
export CARGO_NET_OFFLINE=true UV_OFFLINE=true
cd "$lane/servo" || exit 1
# The lane's base and mine2 builds link no GStreamer: mach needs the official framework for it.
"$servo_link/mach" build --dev --media-stack=dummy || exit $?
cp target/debug/servoshell target/debug/servoshell.mine3 || exit 1
export PYTHONPATH="$servo_link/tests/wpt/tests"
paths=(
  "$servo_link/tests/wpt/webgl/tests/conformance2"
  "$servo_link/tests/wpt/webgl/tests/conformance/attribs"
  "$servo_link/tests/wpt/webgl/tests/conformance/buffers"
  "$servo_link/tests/wpt/webgl/tests/conformance/canvas"
  "$servo_link/tests/wpt/webgl/tests/conformance/context"
  "$servo_link/tests/wpt/webgl/tests/conformance/extensions"
  "$servo_link/tests/wpt/webgl/tests/conformance/glsl"
  "$servo_link/tests/wpt/webgl/tests/conformance/limits"
  "$servo_link/tests/wpt/webgl/tests/conformance/misc"
  "$servo_link/tests/wpt/webgl/tests/conformance/more"
  "$servo_link/tests/wpt/webgl/tests/conformance/offscreencanvas"
  "$servo_link/tests/wpt/webgl/tests/conformance/programs"
  "$servo_link/tests/wpt/webgl/tests/conformance/reading"
  "$servo_link/tests/wpt/webgl/tests/conformance/renderbuffers"
  "$servo_link/tests/wpt/webgl/tests/conformance/rendering"
  "$servo_link/tests/wpt/webgl/tests/conformance/state"
  "$servo_link/tests/wpt/webgl/tests/conformance/textures"
  "$servo_link/tests/wpt/webgl/tests/conformance/typedarrays"
  "$servo_link/tests/wpt/webgl/tests/conformance/uniforms"
  "$servo_link/tests/wpt/tests/webgl/texture-validation-overflow.html"
  "$servo_link/tests/wpt/tests/webgl/float-extensions-and-read-pixels.html"
)
# Expectations were recorded with WebGL2 disabled. Retain raw logs even when
# test-wpt exits nonzero, and compare PASS subtest counts per file and area.
for version in base mine3; do
  "$servo_link/mach" test-wpt \
    --binary "$servo_link/target/debug/servoshell.$version" \
    --pref dom_webgl2_enabled=true --processes 6 \
    --log-raw "$lane/servo/target/wpt-$version-review.raw.log" \
    "${paths[@]}"
done
"$servo_link/mach" test-tidy
# Run from Servo's own workspace so its integration-test dev-dependencies resolve.
cargo test --offline -p servo-net-traits --test request_client
# Currently blocked by unrelated IndexedDB test trait errors; retain the output.
cargo test --offline -p servo-script --lib dom::webgl
cd "$lane/frame" || exit 1
script/media-env cargo test --offline -p frame
script/media-env cargo clippy --offline -p frame --all-targets -- -D warnings
script/media-env cargo build --offline --example graph_probe
