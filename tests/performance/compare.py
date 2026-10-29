#!/usr/bin/env python3
"""Run the actual native browser with synthetic profiles and local HTTP fixtures.

Use script/media-env python3 tests/performance/compare.py BINARY OUTPUT_DIR
Retained binaries must remain beside their lib/ runtime links.
"""

import argparse
import contextlib
import functools
import http.server
import json
import os
from pathlib import Path
import resource
import sqlite3
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[2]


class Fixture(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        if self.path.startswith("/search?"):
            time.sleep(0.6)
            body = json.dumps(
                {
                    "results": [
                        {
                            "url": "https://example.org/ocean-tides",
                            "title": "Ocean tides",
                            "content": "The moon drives ocean tides through gravity.",
                            "engine": "fixture",
                        }
                    ]
                }
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            with contextlib.suppress(BrokenPipeError, ConnectionResetError):
                self.wfile.write(body)
        elif self.path.startswith(("/green.mp4", "/resize.mp4")):
            path = (
                self.server.video_fixture.with_name("green.mp4")
                if self.path.startswith("/green.mp4")
                else self.server.video_fixture.with_name("green-small.mp4")
            )
            body = path.read_bytes()
            self.send_response(200)
            self.send_header(
                "Content-Type",
                "video/mp4",
            )
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            with contextlib.suppress(BrokenPipeError, ConnectionResetError):
                self.wfile.write(body)
        elif self.path.startswith("/motion.mp4"):
            body = self.server.video_fixture.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "video/mp4")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            with contextlib.suppress(BrokenPipeError, ConnectionResetError):
                self.wfile.write(body)
        elif self.path.startswith("/intrinsic.mp4"):
            path = (
                ROOT
                / "deps/servo-mse/tests/wpt/tests/media/400x300-red-resize-200x150-green.mp4"
            )
            body = path.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "video/mp4")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            with contextlib.suppress(BrokenPipeError, ConnectionResetError):
                self.wfile.write(body)
        elif self.path.startswith("/movie.webm"):
            path = ROOT / "deps/servo-mse/tests/wpt/tests/media/movie_5.webm"
            body = path.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "video/webm")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            with contextlib.suppress(BrokenPipeError, ConnectionResetError):
                self.wfile.write(body)
        else:
            super().do_GET()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--live-search", action="store_true")
    parser.add_argument("--video-fixture", type=Path)
    parser.add_argument(
        "--compare-with",
        type=Path,
        help="Alternate the baseline and this candidate for each workload",
    )
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--modes", nargs="+", default=["search", "scroll", "video"])
    args = parser.parse_args()
    if any("-hd" in mode or mode == "video-transition" for mode in args.modes):
        if args.video_fixture is None or not args.video_fixture.is_file():
            parser.error(
                "HD/transition workloads require --video-fixture pointing to motion-1080p60.mp4"
            )
    if "video-transition" in args.modes:
        for name in ("green.mp4", "green-small.mp4"):
            if not args.video_fixture.with_name(name).is_file():
                parser.error(
                    f"Transition workload also requires {name} beside the video fixture"
                )
    args.output.mkdir(parents=True, exist_ok=True)
    server = http.server.ThreadingHTTPServer(
        ("127.0.0.1", 0),
        functools.partial(Fixture, directory=ROOT / "tests/performance"),
    )
    server.video_fixture = args.video_fixture
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{server.server_port}"
    results = []
    try:
        for repeat in range(args.repeats):
            for mode in args.modes:
                variants = (
                    [("baseline", args.binary), ("candidate", args.compare_with)]
                    if args.compare_with
                    else [("single", args.binary)]
                )
                if repeat % 2:
                    variants.reverse()
                for variant, binary in variants:
                    prefix = f"{variant}-" if args.compare_with else ""
                    for attempt in range(3):
                        suffix = f"-retry{attempt}" if attempt else ""
                        output = (
                            args.output / f"{prefix}{mode}-{repeat}{suffix}.json"
                        ).resolve()
                        with tempfile.TemporaryDirectory(
                            prefix="profile-", dir=args.output
                        ) as profile:
                            env = os.environ.copy()
                            # Never inherit live provider/model credentials or endpoints into fixtures.
                            env = {
                                k: v
                                for k, v in env.items()
                                if not any(
                                    x in k
                                    for x in [
                                        "RUSTYWEB",
                                        "RUSTY_RED",
                                        "QWEN",
                                        "SYNTHESIS",
                                    ]
                                )
                            }
                            env.update(
                                FRAME_PERF_ROOT=str(Path(profile).resolve()),
                                FRAME_PERF_URL=f"{base}/page.html?{mode}",
                                FRAME_PERF_OUTPUT=str(output),
                                RUSTYWEB_SEARCH_PROVIDERS="searxng",
                                RUSTYWEB_SEARXNG_URL=base,
                            )
                            if args.live_search:
                                env.pop("RUSTYWEB_SEARCH_PROVIDERS", None)
                                env.pop("RUSTYWEB_SEARXNG_URL", None)
                            usage = resource.getrusage(resource.RUSAGE_CHILDREN)
                            started = time.monotonic()
                            run = subprocess.run(
                                [str(binary.resolve())],
                                env=env,
                                capture_output=True,
                                text=True,
                                timeout=90,
                            )
                            after = resource.getrusage(resource.RUSAGE_CHILDREN)
                            (
                                args.output / f"{prefix}{mode}-{repeat}{suffix}.log"
                            ).write_text(run.stderr)
                            if run.returncode or not output.exists():
                                raise RuntimeError(
                                    f"{mode} failed ({run.returncode}): {run.stderr[-3000:]}"
                                )
                            result = json.loads(output.read_text())
                            if result.get("error"):
                                raise RuntimeError(f"{mode}: {result['error']}")
                            result.update(
                                mode=mode,
                                variant=variant,
                                repeat=repeat,
                                attempt=attempt,
                                wall_s=time.monotonic() - started,
                                cpu_s=(after.ru_utime + after.ru_stime)
                                - (usage.ru_utime + usage.ru_stime),
                                host_load=os.getloadavg(),
                            )
                            result["valid_foreground"] = mode == "search" or (
                                result.get("window_active", False)
                                and result.get("native", {})
                                .get("page_render", {})
                                .get("count", 0)
                                > 10
                                and result.get("native", {})
                                .get("inactive_page_render", {})
                                .get("count", 0)
                                == 0
                            )
                            if not result["valid_foreground"] and mode not in (
                                "resume",
                                "settle",
                                "redirect",
                                "video-transition",
                                "video-resize",
                            ):
                                result["excluded_reason"] = (
                                    "window inactive during timing"
                                )
                                output.write_text(json.dumps(result, indent=2))
                                results.append(result)
                                print(json.dumps(result), flush=True)
                                continue
                            if mode != "search":
                                if not mode.startswith(
                                    ("video-only", "video-transition", "video-resize")
                                ):
                                    assert result["fixture"]["interval_count"] > 10, (
                                        result
                                    )
                                if mode.startswith("video"):
                                    assert result["fixture"]["video"]["advancing"], (
                                        result
                                    )
                                else:
                                    assert result["fixture"]["scroll_y"] > 0, result
                            elif not args.live_search:
                                assert all(
                                    r["hits"]
                                    for r in result["sequential"] + result["concurrent"]
                                ), result
                            if mode == "video-transition":
                                video = result["fixture"]["video"]
                                assert len(video["sources"]) == 3, result
                                assert all(
                                    source["maxTime"] > 0.4 and source["playResolved"]
                                    for source in video["sources"]
                                ), result
                                assert video["ended"], result
                                assert (
                                    abs(video["current"] - video["duration"]) < 0.1
                                ), result
                                if "transition_tints" in result:
                                    first, second = result["transition_tints"]
                                    assert first[1] > 0.4 and first[0] < 0.2, result
                                    assert second[0] > 0.8 and second[1] < 0.2, result
                                last = result["tint_rgb"]
                                assert last[1] > 0.4 and last[0] < 0.2, result
                                assert (
                                    result["fixture"]["video"]["width"],
                                    result["fixture"]["video"]["height"],
                                ) == (200, 150), result
                            if mode == "video-resize":
                                video = result["fixture"]["video"]
                                assert [400, 300] in video["dimensions"], result
                                assert [200, 150] in video["dimensions"], result
                                assert video["ended"] and video["current"] > 1.9, result
                                assert (
                                    abs(video["current"] - video["duration"]) < 0.1
                                ), result
                                tint = result["tint_rgb"]
                                assert tint[1] > 0.4 and tint[0] < 0.2, result
                            if mode == "resume":
                                assert result["hidden_surface_presents"] == 0, result
                            if mode == "settle" and "tint_rgb" in result:
                                expected = [30 / 255, 160 / 255, 70 / 255]
                                assert all(
                                    abs(a - b) < 0.02
                                    for a, b in zip(result["tint_rgb"], expected)
                                ), result
                            if mode != "search":
                                with sqlite3.connect(
                                    Path(profile) / "profile" / "browser.sqlite3"
                                ) as db:
                                    session = json.loads(
                                        db.execute(
                                            "SELECT value FROM session WHERE id=1"
                                        ).fetchone()[0]
                                    )
                                expected_mode = "scroll" if mode == "redirect" else mode
                                assert (
                                    session["tabs"][0]["url"]
                                    == f"{base}/page.html?{expected_mode}"
                                ), session
                                result["session_restored_after_exit"] = True
                            output.write_text(json.dumps(result, indent=2))
                            results.append(result)
                            print(json.dumps(result), flush=True)
                        if result["valid_foreground"] or mode in (
                            "resume",
                            "settle",
                            "redirect",
                            "video-transition",
                            "video-resize",
                        ):
                            break
                    else:
                        raise RuntimeError(
                            f"{mode}: window repeatedly lost foreground focus; raw results retained"
                        )
    finally:
        server.shutdown()
        (args.output / "results.json").write_text(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
