#!/usr/bin/env python3
"""Run the pinned upstream H.264 parser suite with Frame's selected plugins.

Run script/bootstrap-media first, then script/media-env python3 tests/media/check_h264.py.
Uses GstCheck directly; unrelated GstValidate tooling is not required.
"""
from pathlib import Path
import shlex
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
source = root / "deps/gstreamer-mse/subprojects/gst-plugins-bad"
flags = shlex.split(subprocess.check_output(
    ["pkg-config", "--cflags", "--libs", "gstreamer-check-1.0",
     "gstreamer-video-1.0", "gstreamer-codecparsers-1.0"], text=True))
with tempfile.TemporaryDirectory(prefix="frame-h264-check-") as directory:
    binary = str(Path(directory) / "h264parse-check")
    subprocess.run([
        "cc", "-DGST_USE_UNSTABLE_API", "-DHAVE_CONFIG_H",
        *["-I" + str(path) for path in [root / "deps/gstreamer-mse-build",
                                       source, source / "gst-libs", source / "tests/check"]],
        str(source / "tests/check/elements/h264parse.c"),
        str(source / "tests/check/elements/parser.c"), "-o", binary, *flags,
    ], check=True)
    subprocess.run([binary], check=True, timeout=60)
