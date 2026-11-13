#!/usr/bin/env python3
"""W0 build/serve utility. Uses the installed tools and Python's HTTP server."""
import argparse
import base64
import functools
import gzip
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import time
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = ROOT / "docs/plans/evidence/spatial-canvas-w0-2026-10-27"
TARGET = Path("/Volumes/SSD Samsung/cargo-targets/frame-canvas")
# Not in git (9.6 MB); fetched from the pinned URL in assets/provenance.json.
FONT = ROOT / "spikes/web-viewer/assets/NotoSansJP.ttf"
STACKS = {"gpui-pre": ("web-viewer", "frame_web_viewer_spike"),
          "zed": ("web-viewer-zed", "frame_web_viewer_zed_spike")}


def command(args, env=None):
    started = time.perf_counter()
    process = subprocess.Popen(args, cwd=ROOT, env=env, text=True,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    output = []
    for line in process.stdout:
        output.append(line)
        print(line, end="", flush=True)
    process.wait()
    elapsed = time.perf_counter() - started
    record = {"command": shlex.join(map(str, args)), "exit_code": process.returncode,
              "runtime_seconds": elapsed, "output": "".join(output)}
    if env:
        record["environment"] = {k: env[k] for k in ("TMPDIR", "CARGO_TARGET_DIR")}
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    with (EVIDENCE / "commands.jsonl").open("a") as log:
        log.write(json.dumps(record) + "\n")
    print(f"[{elapsed:.3f}s, exit {process.returncode}] {record['command']}", flush=True)
    if process.returncode:
        raise SystemExit(process.returncode)


def fetch_font():
    if FONT.exists():
        return
    pinned = json.loads((FONT.parent / "provenance.json").read_text())["files"][FONT.name]
    print(f"Fetching {pinned['url']}", flush=True)
    with urlopen(pinned["url"], timeout=60) as response:
        font = response.read()
    if len(font) != pinned["bytes"] or hashlib.sha256(font).hexdigest() != pinned["sha256"]:
        raise SystemExit(f"{pinned['url']} does not match assets/provenance.json")
    partial = FONT.with_name(FONT.name + ".part")
    partial.write_bytes(font)
    partial.replace(FONT)


def build(stack):
    command(["df", "-h", "/Volumes/SSD Samsung"])
    if shutil.disk_usage("/Volumes/SSD Samsung").free < 6 * 1024**3:
        raise SystemExit("STOP: less than 6 GiB free")
    directory, name = STACKS[stack]
    crate = ROOT / "spikes" / directory
    env = dict(os.environ, TMPDIR="/Volumes/SSD Samsung/tmp", CARGO_TARGET_DIR=str(TARGET))
    command(["cargo", "build", "--offline", "--locked", "--release", "--target",
             "wasm32-unknown-unknown", "--manifest-path", str(crate / "Cargo.toml")], env)
    # Never install a mismatched CLI automatically.
    cli = Path.home() / ".cargo/bin/wasm-bindgen"
    command([str(cli), "--version"])
    if subprocess.check_output([str(cli), "--version"], text=True).strip() != "wasm-bindgen 0.2.127":
        raise SystemExit("Use wasm-bindgen CLI 0.2.127, matching both lockfiles")
    pkg = crate / "pkg"
    pkg.mkdir(exist_ok=True)
    compiled = TARGET / "wasm32-unknown-unknown/release" / f"{name}.wasm"
    command([str(cli), "--target", "web", "--out-dir", str(pkg), "--out-name", "viewer", str(compiled)])
    raw = pkg / "viewer_bg.raw.wasm"
    shutil.copyfile(pkg / "viewer_bg.wasm", raw)
    command(["/opt/homebrew/bin/wasm-opt", "-Oz", "--all-features", str(raw), "-o", str(pkg / "viewer_bg.wasm")])
    measure_sizes(stack)


def measure_sizes(stack):
    directory, name = STACKS[stack]
    pkg = ROOT / "spikes" / directory / "pkg"
    compiled = TARGET / "wasm32-unknown-unknown/release" / f"{name}.wasm"
    raw = pkg / "viewer_bg.raw.wasm"
    optimized = (pkg / "viewer_bg.wasm").read_bytes()
    compressed = gzip.compress(optimized, mtime=0)
    (pkg / "viewer_bg.wasm.gz").write_bytes(compressed)
    js = (pkg / "viewer.js").read_bytes()
    font = FONT.read_bytes()
    font_gzip = len(gzip.compress(font, mtime=0))
    sizes = {"stack": stack, "compiler_wasm_bytes": compiled.stat().st_size,
             "raw_bindgen_wasm_bytes": raw.stat().st_size, "wasm_opt_bytes": len(optimized),
             "gzip_wasm_bytes": len(compressed), "js_bytes": len(js),
             "gzip_js_bytes": len(gzip.compress(js, mtime=0)),
             "font_bytes": len(font), "gzip_font_bytes": font_gzip,
             "raw_bundle_bytes": len(optimized) + len(js) + len(font),
             "gzip_wasm_js_bytes": len(compressed) + len(gzip.compress(js, mtime=0)),
             "gzip_bundle_bytes": len(compressed) + len(gzip.compress(js, mtime=0)) + font_gzip}
    (EVIDENCE / f"{stack}-sizes.json").write_text(json.dumps(sizes, indent=2) + "\n")


class Handler(SimpleHTTPRequestHandler):
    def translate_path(self, path):
        parts = urlsplit(path).path.strip("/").split("/")
        if len(parts) == 3 and parts[0] == "bundles" and parts[1] in STACKS:
            if parts[2] in ("viewer.js", "viewer_bg.wasm"):
                return str(ROOT / "spikes" / STACKS[parts[1]][0] / "pkg" / parts[2])
        return super().translate_path(path)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def do_POST(self):
        if self.path not in ("/result", "/lifecycle"):
            self.send_error(404)
            return
        length = int(self.headers.get("Content-Length", "0"))
        if not 0 < length <= 20_000_000:
            self.send_error(413)
            return
        try:
            result = json.loads(self.rfile.read(length))
            stack, browser = result["stack"], result["browser"]
            if stack not in STACKS or browser not in ("firefox", "safari", "arc", "unknown"):
                raise ValueError("unknown stack or browser")
            prefix = "lifecycle-" if self.path == "/lifecycle" else ""
            stem = f"{prefix}{stack}-{browser}-{time.time_ns()}"
            EVIDENCE.mkdir(parents=True, exist_ok=True)
            capture = result.get("capture", {})
            data_url = capture.pop("dataUrl", None)
            if data_url:
                prefix, encoded = data_url.split(",", 1)
                if prefix != "data:image/png;base64":
                    raise ValueError("expected PNG")
                image = base64.b64decode(encoded, validate=True)
                if not image.startswith(b"\x89PNG\r\n\x1a\n"):
                    raise ValueError("invalid PNG signature")
                image_path = EVIDENCE / f"{stem}.png"
                image_path.write_bytes(image)
                capture["file"] = image_path.name
            (EVIDENCE / f"{stem}.json").write_text(json.dumps(result, indent=2) + "\n")
        except (ValueError, KeyError, TypeError) as error:
            self.send_error(400, str(error))
            return
        self.send_response(201)
        self.end_headers()
        self.wfile.write(b'{"saved":true}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["build", "sizes", "serve"])
    parser.add_argument("--stack", choices=STACKS, default="gpui-pre")
    parser.add_argument("--port", type=int, default=8765)
    args = parser.parse_args()
    fetch_font()
    if args.action == "build":
        build(args.stack)
    elif args.action == "sizes":
        measure_sizes(args.stack)
    else:
        handler = functools.partial(Handler, directory=str(ROOT / "spikes/web-viewer"))
        server = ThreadingHTTPServer(("127.0.0.1", args.port), handler)
        print(f"W0 ready: http://127.0.0.1:{args.port}/?stack=gpui-pre&browser=firefox", flush=True)
        server.serve_forever()


if __name__ == "__main__":
    main()
