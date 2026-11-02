#!/usr/bin/env python3
"""Run with script/media-env to test the selected GStreamer MSE runtime."""
from pathlib import Path
import shlex
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
fixtures = root / "deps/servo-consolidated/tests/wpt/tests/media-source/webm"
flags = shlex.split(subprocess.check_output(
    ["pkg-config", "--cflags", "--libs", "gstreamer-mse-1.0"], text=True))
with tempfile.TemporaryDirectory(prefix="frame-mse-native-") as directory:
    binary = str(Path(directory) / "mse_remove")
    subprocess.run(["cc", "-DGST_USE_UNSTABLE_API", "-Wall", "-Wextra",
                    str(root / "tests/media/mse_remove.c"), "-o", binary, *flags], check=True)
    subprocess.run([binary,
                    str(fixtures / "test-v-128k-320x240-30fps-10kfr.webm"),
                    str(fixtures / "test-a-192k-44100Hz-1ch.webm")], check=True, timeout=40)
