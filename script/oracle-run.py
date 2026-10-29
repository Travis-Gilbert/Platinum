#!/usr/bin/env python3
"""oracle-run.py - run and validate Frame's nightly acceptance oracle.

Rows and pins live in docs/plans/oracle-spec.json. The runner executes each
selected row as one subprocess under bash, tees its output to a per-row log
and writes one receipt of what actually happened: preflight.json for
``run --check`` (pins and archival patch identity, no build), receipt.json
for a full run. A receipt is a record of this invocation; it never promotes
source pins, fixtures or expectations to execution evidence.

Rows with stage "live" need a window server, a GPU, served fixtures or a
human. A hosted runner never executes them; validate refuses a CI-mode
receipt that reports one demonstrated. On a development Mac ``run --live``
executes the runnable live rows into the same receipt shape, so one runner
produces both the nightly CI evidence and the local acceptance evidence.

Usage:

  script/oracle-run.py run [--check] [--live] [--output DIR]
                           [--spec PATH] [--timeout SECONDS] [--only ID,...]
  script/oracle-run.py validate --receipt PATH [--spec PATH] [--report PATH]

Validate refusals:

  oracle_receipt_unreadable  receipt absent, malformed or the wrong schema
  oracle_spec_drift          the receipt was produced against different spec text
  oracle_pin_drift           receipt pins disagree with the spec
  oracle_row_missing         a spec row the receipt does not carry
  oracle_row_not_demonstrated  a row present but not demonstrated (skipped, filtered)
  oracle_row_failed          a row that ran and did not exit zero
  oracle_live_row_claimed    a CI-mode receipt demonstrating a live row
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
import platform
import shlex
import signal
import socket
import subprocess
import sys
import threading
import time
from urllib.parse import urlsplit
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SPEC = ROOT / "docs/plans/oracle-spec.json"
RECEIPT_SCHEMA = "frame-oracle-receipt/1.0"
PREFLIGHT_SCHEMA = "frame-oracle-preflight/1.0"
VALID_OUTCOMES = {"demonstrated", "failed", "not-run"}


def now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path: Path, value) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def load_spec(path: Path) -> dict:
    spec = json.loads(path.read_text(encoding="utf-8"))
    spec["_sha256"] = sha256_file(path)
    spec["_path"] = str(path)
    return spec


def command_output(argv: list[str]) -> str:
    result = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, check=False)
    return result.stdout.strip() if result.returncode == 0 else ""


def run_info(mode: str) -> dict:
    info = {
        "id": uuid.uuid4().hex,
        "mode": mode,
        "started_at": now(),
        "platform": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "python": platform.python_version(),
        },
        "frame_head": command_output(["git", "rev-parse", "HEAD"]),
        "frame_dirty": bool(command_output(["git", "status", "--porcelain"])),
        "rustc": command_output(["rustc", "--version"]),
        "cargo": command_output(["cargo", "--version"]),
    }
    runner = {
        key: value
        for key in ("GITHUB_RUN_ID", "GITHUB_SHA", "RUNNER_NAME", "RUNNER_OS", "RUNNER_ARCH")
        if (value := os.environ.get(key))
    }
    if runner:
        info["runner"] = runner
    return info


def pins_record() -> dict | None:
    """The unified pin record, or None when check-pins refuses."""
    result = subprocess.run(
        [sys.executable, str(ROOT / "script/check-pins.py"), "--json", "--require-deps"],
        cwd=ROOT, capture_output=True, text=True, check=False,
    )
    if result.stderr.strip():
        print(result.stderr, end="", file=sys.stderr)
    if result.returncode != 0:
        return None
    try:
        return json.loads(result.stdout)
    except ValueError:
        return None


def run_row(row: dict, output: Path, timeout_override: int | None, placeholders: dict) -> dict:
    command = row["command"]
    for key, value in placeholders.items():
        # Placeholders can expand to paths with spaces; the row runs under bash.
        command = command.replace("{" + key + "}", shlex.quote(value))
    log_dir = output / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / f"{row['id']}.log"
    env = {**os.environ, **(row.get("env") or {})}
    timeout = timeout_override or int(row.get("timeout_s") or 3600)
    print(f"[{row['id']}] {command}", flush=True)
    started = time.monotonic()
    timed_out = threading.Event()
    with log_path.open("w", encoding="utf-8") as log:
        log.write(f"# {command}\n# started {now()}\n")
        process = subprocess.Popen(
            ["bash", "-c", command], cwd=ROOT, env=env,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, bufsize=1, start_new_session=True,
        )

        def kill() -> None:
            timed_out.set()
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except OSError:
                pass

        watchdog = threading.Timer(timeout, kill)
        watchdog.start()
        try:
            assert process.stdout is not None
            for line in process.stdout:
                sys.stdout.write(line)
                sys.stdout.flush()
                log.write(line)
            exit_code = process.wait()
        finally:
            watchdog.cancel()
    duration = round(time.monotonic() - started, 1)
    outcome = "demonstrated" if exit_code == 0 and not timed_out.is_set() else "failed"
    record = {
        "id": row["id"],
        "item": row["item"],
        "stage": row.get("stage", "run"),
        "oracle_class": row["oracle_class"],
        "evidence_class": row["evidence_class"],
        "command": command,
        "exit_code": exit_code,
        "outcome": outcome,
        "duration_s": duration,
        "log": str(log_path),
        "log_sha256": sha256_file(log_path),
    }
    if timed_out.is_set():
        record["reason"] = f"timeout after {timeout}s"
    print(f"[{row['id']}] {outcome} (exit {exit_code}, {duration}s)", flush=True)
    return record


def skipped_row(row: dict, reason: str) -> dict:
    return {
        "id": row["id"],
        "item": row["item"],
        "stage": row.get("stage", "run"),
        "oracle_class": row["oracle_class"],
        "evidence_class": "not-run",
        "command": row.get("command"),
        "exit_code": None,
        "outcome": "not-run",
        "duration_s": 0.0,
        "reason": reason,
    }


def cargo_target_dir() -> str:
    metadata = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    )
    return json.loads(metadata.stdout)["target_directory"]


def start_fixture_server(spec: dict) -> subprocess.Popen:
    server = spec.get("fixture_server")
    if not server:
        raise RuntimeError("spec has no fixture_server for a row that needs one")
    process = subprocess.Popen(
        ["bash", "-c", server["command"]], cwd=ROOT,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True,
    )
    port = urlsplit(server["base"]).port or 80
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        with socket.socket() as probe:
            probe.settimeout(1)
            if probe.connect_ex(("127.0.0.1", port)) == 0:
                print(f"[fixture-server] listening on {server['base']}", flush=True)
                return process
        time.sleep(0.3)
    os.killpg(process.pid, signal.SIGKILL)
    raise RuntimeError(f"fixture server did not reach {server['base']}")


def select_rows(spec: dict, args) -> list[dict]:
    """The rows this invocation executes, in order."""
    if args.check:
        executable = [row for row in spec["rows"] if row.get("stage") == "preflight"]
    else:
        executable = list(spec["rows"])
        if args.live:
            executable.extend(row for row in (spec.get("live_rows") or []) if row.get("runnable"))
    if args.only:
        wanted = {identifier.strip() for identifier in args.only.split(",") if identifier.strip()}
        known = {row["id"] for row in spec["rows"]} | {row["id"] for row in (spec.get("live_rows") or [])}
        unknown = sorted(wanted - known)
        if unknown:
            raise SystemExit(f"unknown row ids: {unknown}")
        executable = [row for row in executable if row["id"] in wanted]
    return executable


def cmd_run(args) -> int:
    spec = load_spec(Path(args.spec))
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    mode = "check" if args.check else ("live" if args.live else "ci")
    info = run_info(mode)
    payload: dict = {
        "schema": PREFLIGHT_SCHEMA if args.check else RECEIPT_SCHEMA,
        "spec": {"path": os.path.relpath(spec["_path"], ROOT), "sha256": spec["_sha256"]},
        "run": info,
    }

    pins = pins_record()
    payload["pins"] = pins
    if pins is None:
        payload["refusal"] = "check-pins refused; its refusals are on this step's stderr"
        payload["rows"] = []
        payload["status"] = "blocked"
        write_json(output / ("preflight.json" if args.check else "receipt.json"), payload)
        print(f"oracle {mode}: blocked at pin unity", flush=True)
        return 1

    try:
        executable = select_rows(spec, args)
    except SystemExit as error:
        print(str(error), file=sys.stderr)
        return 2

    wanted = {row["id"] for row in executable}
    placeholders = {"output": str(output)}
    if any("{" + "target_dir" + "}" in (row.get("command") or "") for row in executable):
        placeholders["target_dir"] = cargo_target_dir()

    results: list[dict] = []
    demonstrated: set[str] = set()
    fixture_server = None
    try:
        if any(row.get("needs_fixture_server") for row in executable):
            fixture_server = start_fixture_server(spec)
        for row in executable:
            missing = [dep for dep in row.get("depends_on") or [] if dep not in demonstrated]
            if missing:
                results.append(skipped_row(row, f"dependency not demonstrated: {missing}"))
                continue
            record = run_row(row, output, args.timeout, placeholders)
            results.append(record)
            if record["outcome"] == "demonstrated":
                demonstrated.add(record["id"])
    finally:
        if fixture_server:
            os.killpg(fixture_server.pid, signal.SIGTERM)

    roster = list(spec["rows"]) + list(spec.get("live_rows") or [])
    for row in roster:
        if row["id"] in wanted or any(item["id"] == row["id"] for item in results):
            continue
        if row.get("stage") == "live":
            reason = row.get("reason") or "live_oracle_required: a hosted runner has no window server, GPU or human"
        else:
            reason = "not selected for this invocation"
        results.append(skipped_row(row, reason))
    order = {row["id"]: index for index, row in enumerate(roster)}
    results.sort(key=lambda item: order.get(item["id"], len(order)))

    payload["rows"] = results
    blocked = [item["id"] for item in results if item["id"] in wanted and item["outcome"] != "demonstrated"]
    payload["status"] = "blocked" if blocked else "passed"
    if blocked:
        payload["blocked_rows"] = blocked
    payload["artifacts"] = [
        {"path": os.path.relpath(str(path), output), "sha256": sha256_file(path)}
        for path in sorted(output.rglob("*"))
        if path.is_file() and path.name not in ("receipt.json", "preflight.json")
    ]
    info["finished_at"] = now()
    destination = output / ("preflight.json" if args.check else "receipt.json")
    write_json(destination, payload)
    print(f"oracle {mode}: {payload['status']} -> {destination}", flush=True)
    return 0 if payload["status"] == "passed" else 1


def normalize_pin_values(pins: dict) -> dict:
    return {
        "theorem_rev": (pins.get("theorem") or {}).get("rev"),
        "theorem_branch": (pins.get("theorem") or {}).get("branch"),
        "servo_rev": (pins.get("servo") or {}).get("rev"),
        "servo_archival_base": (pins.get("servo") or {}).get("archival_base"),
        "gstreamer_rev": (pins.get("gstreamer") or {}).get("rev"),
        "rust_toolchain": pins.get("rust_toolchain"),
    }


def cmd_validate(args) -> int:
    spec = load_spec(Path(args.spec))
    refusals: list[str] = []
    try:
        receipt = json.loads(Path(args.receipt).read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        print(f"oracle_receipt_unreadable {args.receipt}: {error}", file=sys.stderr)
        return 1
    if receipt.get("schema") != RECEIPT_SCHEMA:
        print(
            f"oracle_receipt_unreadable {args.receipt}: schema is "
            f"{receipt.get('schema')!r}, expected {RECEIPT_SCHEMA!r}",
            file=sys.stderr,
        )
        return 1

    if (receipt.get("spec") or {}).get("sha256") != spec["_sha256"]:
        refusals.append(
            "oracle_spec_drift: the receipt was produced against different spec text than "
            f"{spec['_path']}"
        )
    observed_pins = normalize_pin_values(receipt.get("pins") or {})
    for key, value in observed_pins.items():
        expected = spec["source_pins"].get(key)
        if expected is not None and value != expected:
            refusals.append(f"oracle_pin_drift: {key}: receipt {value!r}, spec {expected!r}")

    rows = {row.get("id"): row for row in receipt.get("rows") or []}
    mode = (receipt.get("run") or {}).get("mode")
    for row in spec["rows"]:
        observed = rows.get(row["id"])
        if observed is None:
            refusals.append(f"oracle_row_missing {row['id']}: {row['item']}")
        elif observed.get("outcome") == "failed":
            refusals.append(
                f"oracle_row_failed {row['id']}: exit {observed.get('exit_code')} "
                f"({observed.get('reason') or 'see log'})"
            )
        elif observed.get("outcome") != "demonstrated":
            refusals.append(
                f"oracle_row_not_demonstrated {row['id']}: {observed.get('reason') or observed.get('outcome')}"
            )
    if mode in ("ci", "check"):
        for row in spec.get("live_rows") or []:
            observed = rows.get(row["id"])
            if observed and observed.get("outcome") == "demonstrated":
                refusals.append(
                    f"oracle_live_row_claimed {row['id']}: a {mode} receipt cannot demonstrate a live row"
                )

    verdict = "BLOCKED" if refusals else "READY"
    if args.report:
        write_report(Path(args.report), spec, receipt, refusals, verdict)
    for line in refusals:
        print(line, file=sys.stderr)
    outstanding = [
        row for row in (spec.get("live_rows") or [])
        if (rows.get(row["id"]) or {}).get("outcome") != "demonstrated"
    ]
    demonstrated = sum(
        1 for row in spec["rows"] if (rows.get(row["id"]) or {}).get("outcome") == "demonstrated"
    )
    print(
        f"oracle verdict: {verdict} ({demonstrated}/{len(spec['rows'])} CI rows demonstrated, "
        f"{len(outstanding)} live rows outstanding)"
    )
    return 0 if verdict == "READY" else 1


def write_report(path: Path, spec: dict, receipt: dict, refusals: list[str], verdict: str) -> None:
    rows = {row.get("id"): row for row in receipt.get("rows") or []}
    run = receipt.get("run") or {}
    lines = [
        f"# Frame oracle readiness - {verdict}",
        "",
        f"- Receipt: `{run.get('id')}` mode `{run.get('mode')}`, {run.get('started_at')} .. {run.get('finished_at')}",
        f"- Frame: `{run.get('frame_head')}`{' (dirty)' if run.get('frame_dirty') else ''}",
        f"- Platform: {run.get('platform', {}).get('system')} {run.get('platform', {}).get('release')} {run.get('platform', {}).get('machine')}, {run.get('rustc') or 'rustc unknown'}",
        "",
        "## Pins",
        "",
        "| Source | Revision |",
        "|---|---|",
    ]
    pins = receipt.get("pins") or {}
    for label, value in (
        ("Theorem substrate", (pins.get("theorem") or {}).get("rev")),
        ("Servo fork", (pins.get("servo") or {}).get("rev")),
        ("Servo archival base", (pins.get("servo") or {}).get("archival_base")),
        ("GStreamer", (pins.get("gstreamer") or {}).get("rev")),
        ("Rust toolchain", pins.get("rust_toolchain")),
    ):
        lines.append(f"| {label} | `{value}` |")
    lines += ["", "## CI rows", "", "| Row | Outcome | Exit | Seconds | Item |", "|---|---|---|---|---|"]
    for row in spec["rows"]:
        observed = rows.get(row["id"]) or {}
        lines.append(
            f"| {row['id']} | {observed.get('outcome', 'missing')} | {observed.get('exit_code')} "
            f"| {observed.get('duration_s')} | {row['item']} |"
        )
    lines += ["", "## Live rows (never claimed by a hosted runner)", ""]
    for row in spec.get("live_rows") or []:
        observed = rows.get(row["id"]) or {}
        state = observed.get("outcome", "not-run")
        lines.append(f"- **{row['id']}** {state} - {row['item']}")
        if row.get("command"):
            lines.append(f"  - command: `{row['command']}`")
        if row.get("reason"):
            lines.append(f"  - why not runnable here: {row['reason']}")
    if refusals:
        lines += ["", "## Refusals", ""]
        lines += [f"- `{line}`" for line in refusals]
    artifacts = receipt.get("artifacts") or []
    if artifacts:
        lines += ["", "## Artifacts", ""]
        lines += [f"- `{item['path']}` sha256 `{item['sha256'][:16]}...`" for item in artifacts]
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    commands = parser.add_subparsers(dest="command", required=True)

    run = commands.add_parser("run", help="execute spec rows and write the receipt")
    run.add_argument("--check", action="store_true", help="preflight rows only; write preflight.json")
    run.add_argument("--live", action="store_true", help="also execute runnable live rows (development Mac)")
    run.add_argument("--output", required=True, help="directory for the receipt, logs and evidence")
    run.add_argument("--spec", default=str(DEFAULT_SPEC))
    run.add_argument("--timeout", type=int, help="override every row timeout, seconds")
    run.add_argument("--only", help="comma-separated row ids to execute")

    validate = commands.add_parser("validate", help="rule a receipt against the spec")
    validate.add_argument("--receipt", required=True)
    validate.add_argument("--spec", default=str(DEFAULT_SPEC))
    validate.add_argument("--report", help="write the readiness markdown here")

    args = parser.parse_args(argv[1:])
    if args.command == "run":
        return cmd_run(args)
    return cmd_validate(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
