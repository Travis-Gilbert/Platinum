#!/usr/bin/env python3
"""Run one cargo command for the R2 evidence: check free disk, run, write <name>.log and <name>.json."""
import json, os, shutil, subprocess, sys, time

EVID = "/Volumes/SSD Samsung/frame-worktrees/r2-servo-evidence/docs/plans/evidence/spatial-canvas-r2-2026-10-29"
CWD = "/Volumes/SSD Samsung/frame-deps/servo-r2"
VOL = "/Volumes/SSD Samsung"
ENVV = {
    "TMPDIR": "/Volumes/SSD Samsung/tmp",
    "CARGO_TARGET_DIR": "/Volumes/SSD Samsung/cargo-targets/servo-r1",
    "CARGO_PROFILE_DEV_DEBUG": "0",
    "CARGO_PROFILE_TEST_DEBUG": "0",
    "CARGO_PROFILE_DEV_STRIP": "none",
    "CARGO_PROFILE_TEST_STRIP": "none",
}

name = sys.argv[1]
cmd = sys.argv[2:]
free_gib = shutil.disk_usage(VOL).free / 2**30
df = subprocess.run(["df", "-h", VOL], capture_output=True, text=True).stdout
if free_gib < 6:
    print(f"STOP: only {free_gib:.1f} GiB free on {VOL}", file=sys.stderr)
    sys.exit(90)

env = dict(os.environ)
env.update(ENVV)
t0 = time.monotonic()
started = time.strftime("%Y-%m-%dT%H:%M:%S%z")
with open(f"{EVID}/{name}.log", "wb") as log:
    p = subprocess.run(cmd, cwd=CWD, env=env, stdout=log, stderr=subprocess.STDOUT)
elapsed = time.monotonic() - t0
free_after = shutil.disk_usage(VOL).free / 2**30
rec = {
    "name": name,
    "cwd": CWD,
    "command": cmd,
    "exit_code": p.returncode,
    "elapsed_seconds": elapsed,
    "started": started,
    "free_gib_before": round(free_gib, 2),
    "free_gib_after": round(free_after, 2),
    "df_before": df.strip().splitlines(),
    "environment": ENVV,
}
with open(f"{EVID}/{name}.json", "w") as f:
    json.dump(rec, f, indent=2)
    f.write("\n")
print(f"{name}: exit {p.returncode} in {elapsed:.3f}s, free {free_gib:.1f} -> {free_after:.1f} GiB")
sys.exit(0)
