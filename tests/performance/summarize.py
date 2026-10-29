#!/usr/bin/env python3
"""Summarize paired native measurements using medians across processes."""

import json
from pathlib import Path
from statistics import median
import sys

runs = json.loads(Path(sys.argv[1]).read_text())
rows = []
for mode in dict.fromkeys(r["mode"] for r in runs):
    variants = {}
    for variant in ("baseline", "candidate"):
        samples = [
            r
            for r in runs
            if r["mode"] == mode
            and r["variant"] == variant
            and r.get("valid_foreground", True)
        ]
        if not samples:
            continue
        if mode == "search":
            metrics = {
                "cold_search_ms": [r["sequential"][0]["ms"] for r in samples],
                "warm_search_ms": [r["sequential"][1]["ms"] for r in samples],
                "three_search_batch_ms": [r["batch_ms"] for r in samples],
                "capture_ms": [r["capture_ms"] for r in samples],
            }
        else:
            metrics = {
                "surface_interval_p95_ms": [
                    r["native"].get("surface_present_interval", {}).get("p95_ms")
                    for r in samples
                ],
                "surface_presents": [
                    r["native"].get("surface_present", {}).get("count") for r in samples
                ],
                "page_render_interval_p95_ms": [
                    r["native"].get("page_render_interval", {}).get("p95_ms")
                    for r in samples
                ],
                "inactive_page_renders": [
                    r["native"].get("inactive_page_render", {}).get("count", 0)
                    for r in samples
                ],
                "servo_interval_p95_ms": [
                    r["native"]["servo_frame_interval"]["p95_ms"] for r in samples
                ],
                "raf_interval_p95_ms": [r["fixture"]["p95_ms"] for r in samples],
                "raf_over_33_fraction": [
                    (
                        r["fixture"]["over_33_ms"] / r["fixture"]["interval_count"]
                        if r["fixture"]["interval_count"]
                        else None
                    )
                    for r in samples
                ],
                "tint_reads": [r["native"]["tint_sample"]["count"] for r in samples],
                "tint_total_ms": [
                    r["native"]["tint_sample"]["total_ms"] for r in samples
                ],
                "session_snapshots": [
                    r["native"].get("session_snapshot", {}).get("count", 0)
                    for r in samples
                ],
            }
        metrics = {
            name: values
            for name, values in metrics.items()
            if all(v is not None for v in values)
        }
        metrics["process_cpu_s"] = [r["cpu_s"] for r in samples]
        variants[variant] = {
            name: {"median": median(values), "runs": values}
            for name, values in metrics.items()
        }
    rows.append({"mode": mode, **variants})
print(json.dumps(rows, indent=2))
