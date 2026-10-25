#!/usr/bin/env python3
"""Check native SVG pixels before and after CSS changes (requires Pillow)."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from PIL import Image


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("probe", type=Path)
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    script = Path(__file__).with_name("svg-paint.js").resolve()
    expectations = {
        "svg-initial": [(255, 255, 255), (0, 255, 0), (0, 0, 255), (255, 0, 0)],
        "svg-restyled": [(255, 0, 255), (255, 255, 0), (0, 0, 255), (255, 0, 0)],
    }
    report = {"passed": False, "phases": {}}
    with tempfile.TemporaryDirectory(prefix="frame-svg-paint-") as temporary:
        result = subprocess.run(
            [str(args.probe.resolve())],
            env={**os.environ, "RUST_LOG": "error", "TMPDIR": temporary,
                 "FRAME_MEDIA_SCRIPT": str(script)},
            capture_output=True, text=True, timeout=70,
        )
        result.check_returncode()
        for phase, expected in expectations.items():
            path = Path(temporary) / f"frame-media-{phase}.png"
            with Image.open(path) as image:
                pixels = [image.convert("RGB").getpixel((x, 50)) for x in (50, 150, 250, 350)]
            report["phases"][phase] = {"pixels": pixels, "expected": expected,
                                       "passed": pixels == expected}
            if args.evidence:
                args.evidence.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, args.evidence / f"{phase}.png")
        report["passed"] = all(phase["passed"] for phase in report["phases"].values())
    if args.evidence:
        (args.evidence / "svg-paint.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
