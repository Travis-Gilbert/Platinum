#!/usr/bin/env python3
"""check-pins.py - Frame's pinned dependencies as one identity.

Frame builds against three external revisions that are not its own tree: the
private Theorem substrate (script/bootstrap), the Servo fork and a patched
GStreamer (script/bootstrap-media plus patches/). Those revisions are written
down in the bootstrap scripts, patches/README.md, the nightly workflow,
docs/plans/oracle-spec.json and, after bootstrapping, in the deps/ checkouts
themselves. A silent drift in any of them means the oracle measures a browser
nobody pinned. This checker makes the invariant binding: every named source
names one Theorem, one Servo, one GStreamer, and deps/ sits at those
revisions with the GStreamer patch applied.

Refusals:

  frame_pin_missing           a pin script no longer carries a revision literal
  frame_pin_split             named sources disagree about a revision
  frame_pin_oracle_missing    the nightly workflow or the oracle spec is absent
                              or will not parse
  frame_deps_missing          deps/ or one of its checkouts is absent although
                              the run requires it
  frame_deps_drift            a deps/ checkout is not at its pinned revision
  frame_patch_missing         the GStreamer worktree does not carry its patch
  frame_runtime_missing       the patched MSE runtime library is not installed
  frame_toolchain_drift       Frame's Rust channel differs from the fork's
  frame_servo_patch_drift     the archival Servo patch does not cover exactly
                              the base..pin diff of the fork
  frame_servo_patch_identity  the archival patch applied to the base does not
                              reproduce the pinned tree byte-for-byte
  frame_pin_unreadable        a named source will not read or parse

Usage:

  script/check-pins.py                       # sources, plus deps/ when present
  script/check-pins.py --source-only         # ignore deps/ entirely
  script/check-pins.py --require-deps        # refuse when deps/ is absent
  script/check-pins.py --json                # print the pin record as JSON
  script/check-pins.py --verify-servo-patch  # archival patch byte identity

Exit 0 means one identity across every named source.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SHA = r"[0-9a-f]{40}"


def fail(refusals: list[str], code: str, detail: str) -> None:
    refusals.append(f"{code} {detail}")


def read_text(path: Path, refusals: list[str], what: str) -> str | None:
    try:
        return path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as error:
        fail(refusals, "frame_pin_unreadable", f"{what} {path}: {error}")
        return None


def git(repo: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess:
    result = subprocess.run(
        ["git", "-C", str(repo), *args], capture_output=True, text=True, check=False
    )
    if check and result.returncode != 0:
        raise RuntimeError(
            f"git {' '.join(args)} in {repo}: {result.stderr.strip() or result.stdout.strip()}"
        )
    return result


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def parse_bootstrap_scripts(refusals: list[str]) -> dict:
    """The revision literals the bootstrap scripts actually execute."""
    pins: dict = {}
    bootstrap = read_text(ROOT / "script/bootstrap", refusals, "bootstrap script")
    if bootstrap is not None:
        rev = re.search(rf'^theorem_rev="({SHA})"$', bootstrap, re.M)
        branch = re.search(r'^theorem_branch="([^"]+)"$', bootstrap, re.M)
        url = re.search(r'^theorem_url="([^"]+)"$', bootstrap, re.M)
        if rev and branch and url:
            pins["theorem"] = {
                "rev": rev.group(1),
                "branch": branch.group(1),
                "url": url.group(1),
            }
        else:
            fail(refusals, "frame_pin_missing",
                 "script/bootstrap: theorem_rev/theorem_branch/theorem_url literals")

    media = read_text(ROOT / "script/bootstrap-media", refusals, "media bootstrap script")
    if media is not None:
        servo = re.search(rf'checkout\(\s*"servo-mse"\s*,\s*"([^"]+)"\s*,\s*"({SHA})"', media)
        gst = re.search(
            rf'checkout\(\s*"gstreamer-mse"\s*,\s*"([^"]+)"\s*,\s*"({SHA})"\s*,\s*"([^"]+)"',
            media,
        )
        if servo:
            pins["servo"] = {"url": servo.group(1), "rev": servo.group(2)}
        else:
            fail(refusals, "frame_pin_missing", "script/bootstrap-media: servo-mse checkout literal")
        if gst:
            pins["gstreamer"] = {"url": gst.group(1), "rev": gst.group(2), "patch": gst.group(3)}
        else:
            fail(refusals, "frame_pin_missing", "script/bootstrap-media: gstreamer-mse checkout literal")
    return pins


def load_spec(refusals: list[str]) -> dict | None:
    path = ROOT / "docs/plans/oracle-spec.json"
    try:
        spec = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        fail(refusals, "frame_pin_oracle_missing", f"oracle spec {path}: {error}")
        return None
    if not isinstance(spec.get("source_pins"), dict):
        fail(refusals, "frame_pin_oracle_missing", f"oracle spec {path}: no source_pins object")
        return None
    return spec


def check_spec(spec: dict | None, pins: dict, refusals: list[str]) -> None:
    if spec is None or "servo" not in pins or "theorem" not in pins or "gstreamer" not in pins:
        return
    source_pins = spec["source_pins"]
    expected = {
        "theorem_rev": pins["theorem"]["rev"],
        "theorem_branch": pins["theorem"]["branch"],
        "servo_rev": pins["servo"]["rev"],
        "servo_repo": pins["servo"]["url"],
        "gstreamer_rev": pins["gstreamer"]["rev"],
        "gstreamer_repo": pins["gstreamer"]["url"],
        "gstreamer_patch": f"patches/{pins['gstreamer']['patch']}",
    }
    for key, value in expected.items():
        if source_pins.get(key) != value:
            fail(refusals, "frame_pin_split",
                 f"oracle spec {key} is {source_pins.get(key)!r}, bootstrap scripts say {value!r}")


def check_readme(spec: dict | None, pins: dict, refusals: list[str]) -> None:
    text = read_text(ROOT / "patches/README.md", refusals, "patches README")
    if text is None or "servo" not in pins or "gstreamer" not in pins:
        return
    required = {
        "servo pin": pins["servo"]["rev"],
        "gstreamer pin": pins["gstreamer"]["rev"],
    }
    if spec:
        required["servo archival base"] = spec["source_pins"].get("servo_archival_base", "")
        required["servo branch"] = spec["source_pins"].get("servo_branch", "")
    for label, value in required.items():
        if value and value not in text:
            fail(refusals, "frame_pin_split",
                 f"patches/README.md does not name the {label} {value}")


def check_manifests(spec: dict | None, refusals: list[str]) -> str | None:
    toolchain_text = read_text(ROOT / "rust-toolchain.toml", refusals, "toolchain file")
    cargo_text = read_text(ROOT / "Cargo.toml", refusals, "root manifest")
    channel = None
    if toolchain_text is not None:
        try:
            channel = tomllib.loads(toolchain_text)["toolchain"]["channel"]
        except (ValueError, KeyError) as error:
            fail(refusals, "frame_pin_unreadable", f"rust-toolchain.toml: {error}")
    if spec and channel and spec["source_pins"].get("rust_toolchain") != channel:
        fail(refusals, "frame_pin_split",
             f"oracle spec rust_toolchain is {spec['source_pins'].get('rust_toolchain')!r}, "
             f"rust-toolchain.toml says {channel!r}")
    if cargo_text is not None:
        try:
            servo_dep = tomllib.loads(cargo_text)["dependencies"]["servo"]
        except (ValueError, KeyError) as error:
            fail(refusals, "frame_pin_unreadable", f"Cargo.toml servo dependency: {error}")
            servo_dep = None
        if servo_dep is not None:
            if servo_dep.get("path") != "deps/servo-mse/components/servo":
                fail(refusals, "frame_pin_split",
                     f"Cargo.toml servo path is {servo_dep.get('path')!r}, expected "
                     "'deps/servo-mse/components/servo'")
            features = sorted(servo_dep.get("features") or [])
            if spec and sorted(spec["source_pins"].get("servo_features") or []) != features:
                fail(refusals, "frame_pin_split",
                     f"oracle spec servo_features {spec['source_pins'].get('servo_features')} "
                     f"disagree with Cargo.toml {features}")
    return channel


def check_workflow(pins: dict, spec: dict | None, refusals: list[str]) -> None:
    path = ROOT / ".github/workflows/browser-oracle.yml"
    text = read_text(path, refusals, "nightly workflow")
    if text is None:
        fail(refusals, "frame_pin_oracle_missing", f"{path}: the nightly workflow is absent")
        return
    if "theorem" in pins:
        refs = set(re.findall(rf"ref:\s*({SHA})", text))
        if refs != {pins["theorem"]["rev"]}:
            fail(refusals, "frame_pin_split",
                 f"workflow revision refs {sorted(refs)} must be exactly the Theorem pin "
                 f"{pins['theorem']['rev']}")
        if "repository: Travis-Gilbert/Theorem" not in text:
            fail(refusals, "frame_pin_split",
                 "workflow does not check out repository Travis-Gilbert/Theorem")
    checkout_path = "deps/theorem-rustyweb-pw2"
    if spec:
        checkout_path = spec["source_pins"].get("theorem_checkout_path", checkout_path)
    if f"path: {checkout_path}" not in text:
        fail(refusals, "frame_pin_split",
             f"workflow does not check the substrate out to {checkout_path}")


def check_deps(pins: dict, spec: dict | None, require: bool, refusals: list[str]) -> dict:
    record: dict = {"checked": False}
    deps = ROOT / "deps"
    if not deps.is_dir():
        if require:
            fail(refusals, "frame_deps_missing",
                 f"{deps}: run script/bootstrap and script/bootstrap-media first")
        return record
    record["checked"] = True
    heads = {}
    if "theorem" in pins:
        heads["theorem-rustyweb-pw2"] = pins["theorem"]["rev"]
    if "servo" in pins:
        heads["servo-mse"] = pins["servo"]["rev"]
    if "gstreamer" in pins:
        heads["gstreamer-mse"] = pins["gstreamer"]["rev"]
    for name, rev in heads.items():
        path = deps / name
        if not path.is_dir():
            fail(refusals, "frame_deps_missing", str(path))
            continue
        head = git(path, "rev-parse", "HEAD").stdout.strip()
        record.setdefault("heads", {})[name] = head
        if head != rev:
            fail(refusals, "frame_deps_drift", f"{path} is at {head}; Frame pins {rev}")

    if "gstreamer" in pins:
        gst = deps / "gstreamer-mse"
        patch = ROOT / "patches" / pins["gstreamer"]["patch"]
        if gst.is_dir() and patch.is_file():
            applied = git(gst, "apply", "--reverse", "--check", str(patch), check=False)
            if applied.returncode != 0:
                fail(refusals, "frame_patch_missing",
                     f"{gst} does not carry {patch.name}; run script/bootstrap-media")
            record["gstreamer_patch_applied"] = applied.returncode == 0

    if spec:
        runtime = ROOT / spec["source_pins"].get("mse_runtime_library", "")
        record["mse_runtime_present"] = runtime.is_file()
        if not runtime.is_file():
            fail(refusals, "frame_runtime_missing", f"{runtime}: run script/bootstrap-media")

    fork_toolchain = deps / "servo-mse" / "rust-toolchain.toml"
    frame_toolchain = ROOT / "rust-toolchain.toml"
    if fork_toolchain.is_file() and frame_toolchain.is_file():
        try:
            fork_channel = tomllib.loads(fork_toolchain.read_text())["toolchain"]["channel"]
            frame_channel = tomllib.loads(frame_toolchain.read_text())["toolchain"]["channel"]
        except (ValueError, KeyError, OSError) as error:
            fail(refusals, "frame_pin_unreadable", f"toolchain comparison: {error}")
        else:
            record["fork_toolchain"] = fork_channel
            if fork_channel != frame_channel:
                fail(refusals, "frame_toolchain_drift",
                     f"pinned fork builds on {fork_channel}, Frame on {frame_channel}")
    return record


def verify_servo_patch(pins: dict, spec: dict | None, refusals: list[str]) -> None:
    """Re-run the archival check: base + patches/servo-mse.patch == pin, byte for byte."""
    if spec is None or "servo" not in pins:
        return
    repo = ROOT / "deps/servo-mse"
    if not repo.is_dir():
        fail(refusals, "frame_deps_missing", f"{repo}: run script/bootstrap-media first")
        return
    base = spec["source_pins"]["servo_archival_base"]
    pin = pins["servo"]["rev"]
    patch_path = ROOT / spec["source_pins"].get("servo_patch", "patches/servo-mse.patch")
    patch_text = read_text(patch_path, refusals, "archival servo patch")
    if patch_text is None:
        return

    try:
        for rev in (base, pin):
            probe = git(repo, "cat-file", "-e", f"{rev}^{{commit}}", check=False)
            if probe.returncode != 0:
                git(repo, "fetch", "--filter=blob:none", "origin", rev)
        listed = git(repo, "diff", "--name-only", base, pin).stdout.split()
    except RuntimeError as error:
        fail(refusals, "frame_servo_patch_drift", str(error))
        return

    pairs = re.findall(r"^diff --git a/(\S+) b/(\S+)$", patch_text, re.M)
    patched: list[str] = []
    for left, right in pairs:
        if left != right:
            fail(refusals, "frame_servo_patch_drift", f"renamed path in patch: a/{left} b/{right}")
        patched.append(right)
    if sorted(set(patched)) != sorted(set(listed)):
        missing = sorted(set(listed) - set(patched))
        extra = sorted(set(patched) - set(listed))
        fail(refusals, "frame_servo_patch_drift",
             f"patch covers {sorted(set(patched))} but base..pin differs in {sorted(set(listed))}; "
             f"missing={missing} extra={extra}")
        return

    workdir = Path(tempfile.mkdtemp(prefix="frame-servo-patch-"))
    tree = workdir / "tree"
    try:
        git(repo, "worktree", "add", "--detach", "--no-checkout", str(tree), base)
        git(tree, "sparse-checkout", "set", "--no-cone", *sorted(set(patched)))
        git(tree, "read-tree", "-mu", "HEAD")
        git(tree, "apply", str(patch_path))
        mismatches = []
        for path in sorted(set(patched)):
            shown = subprocess.run(["git", "-C", str(repo), "show", f"{pin}:{path}"],
                                   capture_output=True, check=False)
            expected = shown.stdout if shown.returncode == 0 else None
            local = tree / path
            actual = local.read_bytes() if local.is_file() else None
            if actual != expected:
                mismatches.append(path)
        if mismatches:
            fail(refusals, "frame_servo_patch_identity",
                 f"{len(mismatches)} file(s) differ from {pin[:12]}: {mismatches}")
        else:
            print(f"servo patch identity: {len(set(patched))} files reproduce {pin[:12]} "
                  f"byte-for-byte from {base[:12]}", file=sys.stderr)
    except RuntimeError as error:
        fail(refusals, "frame_servo_patch_identity", str(error))
    finally:
        git(repo, "worktree", "remove", "--force", str(tree), check=False)
        git(repo, "worktree", "prune", check=False)
        shutil.rmtree(workdir, ignore_errors=True)


def pin_record(pins: dict, spec: dict | None, deps_record: dict) -> dict:
    record: dict = {
        "schema": "frame-pins/1.0",
        "theorem": dict(pins.get("theorem", {})),
        "servo": dict(pins.get("servo", {})),
        "gstreamer": dict(pins.get("gstreamer", {})),
        "deps": deps_record,
    }
    if spec:
        source_pins = spec["source_pins"]
        record["servo"]["archival_base"] = source_pins.get("servo_archival_base")
        record["servo"]["branch"] = source_pins.get("servo_branch")
        record["rust_toolchain"] = source_pins.get("rust_toolchain")
    for name in ("servo-mse.patch", "gstreamer-mse.patch"):
        path = ROOT / "patches" / name
        if path.is_file():
            record.setdefault("patches", {})[name] = sha256_file(path)
    return record


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--json", action="store_true", help="print the pin record as JSON on stdout")
    parser.add_argument("--source-only", action="store_true", help="skip the deps/ checkout checks")
    parser.add_argument("--require-deps", action="store_true", help="refuse when deps/ is absent")
    parser.add_argument("--verify-servo-patch", action="store_true",
                        help="also prove patches/servo-mse.patch reproduces the pin byte-for-byte")
    args = parser.parse_args(argv[1:])

    refusals: list[str] = []
    pins = parse_bootstrap_scripts(refusals)
    spec = load_spec(refusals)
    check_spec(spec, pins, refusals)
    check_readme(spec, pins, refusals)
    check_manifests(spec, refusals)
    check_workflow(pins, spec, refusals)
    deps_record = {} if args.source_only else check_deps(pins, spec, args.require_deps, refusals)
    if args.verify_servo_patch and not refusals:
        verify_servo_patch(pins, spec, refusals)

    if args.json:
        print(json.dumps(pin_record(pins, spec, deps_record), indent=2, sort_keys=True))
    for line in refusals:
        print(line, file=sys.stderr)
    if refusals:
        return 1
    if not args.json:
        checked = " and deps/ at those revisions" if deps_record.get("checked") else ""
        print(
            f"frame pin unity: Theorem {pins['theorem']['rev'][:12]}, "
            f"Servo {pins['servo']['rev'][:12]}, GStreamer {pins['gstreamer']['rev'][:12]}{checked}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
