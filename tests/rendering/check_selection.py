#!/usr/bin/env python3
"""Check the corners of a rounded multi-line text selection (requires Pillow)."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from PIL import Image

# selection.js: three 30px lines starting 20px down, selected from column 4 of
# the first line to column 6 of the last, under a 4px corner radius.
LINE_TOPS = (20, 50, 80)
LINE_HEIGHT = 30


def is_selected(pixel):
    return sum(255 - channel for channel in pixel) > 150


def is_clear(pixel):
    return min(pixel) > 240


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("probe", type=Path)
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    script = Path(__file__).with_name("selection.js").resolve()
    report = {"passed": False, "corners": {}}
    with tempfile.TemporaryDirectory(prefix="frame-selection-") as temporary:
        result = subprocess.run(
            [str(args.probe.resolve())],
            env={**os.environ, "RUST_LOG": "error", "TMPDIR": temporary,
                 "FRAME_MEDIA_SCRIPT": str(script)},
            capture_output=True, text=True, timeout=70,
        )
        result.check_returncode()
        with Image.open(Path(temporary) / "frame-media-selection.png") as image:
            image = image.convert("RGB")
            spans = []
            for top in LINE_TOPS:
                y = top + LINE_HEIGHT // 2
                xs = [x for x in range(image.width) if is_selected(image.getpixel((x, y)))]
                spans.append((min(xs), max(xs) + 1))
            report["spans"] = spans
            (a1, b1), (a2, b2), (a3, b3) = spans
            t1, t2, t3 = LINE_TOPS
            bottom1, bottom2, bottom3 = (top + LINE_HEIGHT for top in LINE_TOPS)
            # (pixel, expected) for each corner, one pixel in from the corner point.
            corners = {
                "first top-left rounded": ((a1, t1), is_clear),
                "first top-right rounded": ((b1 - 1, t1), is_clear),
                "first bottom-left filleted": ((a1 - 1, bottom1 - 1), is_selected),
                "first bottom-right filleted": ((b1, bottom1 - 1), is_selected),
                "middle top-left rounded": ((a2, t2), is_clear),
                "middle top-right rounded": ((b2 - 1, t2), is_clear),
                "middle bottom-left square": ((a2, bottom2 - 1), is_selected),
                "middle bottom-right rounded": ((b2 - 1, bottom2 - 1), is_clear),
                "last top-left square": ((a3, t3), is_selected),
                "last top-right filleted": ((b3, t3), is_selected),
                "last bottom-left rounded": ((a3, bottom3 - 1), is_clear),
                "last bottom-right rounded": ((b3 - 1, bottom3 - 1), is_clear),
            }
            for name, (point, expected) in corners.items():
                pixel = image.getpixel(point)
                report["corners"][name] = {"point": point, "pixel": pixel,
                                           "passed": expected(pixel)}
        if args.evidence:
            args.evidence.mkdir(parents=True, exist_ok=True)
            for phase in ("selection", "selection-prose"):
                shutil.copyfile(Path(temporary) / f"frame-media-{phase}.png",
                                args.evidence / f"{phase}.png")
    report["passed"] = all(corner["passed"] for corner in report["corners"].values())
    if args.evidence:
        (args.evidence / "selection.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
