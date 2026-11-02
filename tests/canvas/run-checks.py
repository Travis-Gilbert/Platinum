#!/usr/bin/env python3
"""Build and measure M1 through the shared lane, retaining timed receipts."""
import argparse
import contextlib
import functools
import hashlib
import http.server
import json
import os
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--evidence', type=Path, required=True)
parser.add_argument('--only', choices=['tests', 'build', 'native', 'probes', 'zoom', 'signin',
                                     'measure', 'diagnose', 'surface-diagnose'])
parser.add_argument('--dry-run', action='store_true', help='Record the plan; no build, server, caffeinate or native window')
args = parser.parse_args()
root = Path(__file__).resolve().parents[2]
evidence = args.evidence.resolve()
evidence.mkdir(parents=True, exist_ok=True)
os.chdir(root)
env = dict(os.environ, TMPDIR='/Volumes/SSD Samsung/tmp',
           CARGO_TARGET_DIR='/Volumes/SSD Samsung/cargo-targets/frame-canvas')
wrapper = '/Volumes/SSD Samsung/cargo-targets/frame-canvas.sh'
binary = '/Volumes/SSD Samsung/cargo-targets/frame-canvas/debug/examples/canvas_check'
receipts = evidence / 'commands.json'
commands = json.loads(receipts.read_text()) if receipts.exists() else []
native_names = {'zoom', 'signin', 'measure', 'diagnose', 'surface'}


def disk_gate():
    if shutil.disk_usage('/Volumes/SSD Samsung').free < 6 * 1024**3:
        sys.exit('Stopped: SSD has less than 6 GiB free')


def stop_process(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def run(name, command, timeout=None):
    disk_gate()
    output = f'{name}-{time.time_ns()}.log'
    started = time.monotonic()
    executable_hash = hashlib.file_digest(Path(binary).open('rb'), 'sha256').hexdigest() if name in native_names else None
    with contextlib.ExitStack() as cleanup:
        scratch = cleanup.enter_context(tempfile.TemporaryDirectory(prefix='frame-canvas-check-', dir=env['TMPDIR'])) if executable_hash else None
        process_env = dict(env, TMPDIR=scratch) if scratch else env
        if name in ('diagnose', 'signin', 'surface'):
            process_env['FRAME_CANVAS_TRACE'] = '1'
        with (evidence / output).open('w') as log:
            process = subprocess.Popen(command, env=process_env, text=True, stdout=log,
                                       stderr=subprocess.STDOUT)
            timed_out = False
            interrupted = None
            try:
                while process.poll() is None:
                    disk_gate()
                    if timeout and time.monotonic() - started > timeout:
                        timed_out = True
                        break
                    try:
                        process.wait(timeout=1)
                    except subprocess.TimeoutExpired:
                        pass
            except BaseException as error:
                interrupted = error
            finally:
                stop_process(process)
        row = dict(name=name, command=command, cwd=str(root),
                   environment={key: process_env[key] for key in ('TMPDIR', 'CARGO_TARGET_DIR')},
                   exit_code=process.returncode, elapsed_seconds=time.monotonic() - started,
                   output=output, timed_out=timed_out, interrupted=interrupted is not None,
                   binary_sha256=executable_hash)
        if name in native_names:
            result_file = evidence / name / 'result.json'
            verdict = json.loads(result_file.read_text()) if result_file.exists() else {}
            row['accepted'] = not timed_out and interrupted is None and verdict.get('pid') == process.pid and verdict.get('passed') is True
            row['probe_result'] = verdict
        commands.append(row)
        receipts.write_text(json.dumps(commands, indent=2) + '\n')
        print(json.dumps(row), flush=True)
        if interrupted:
            raise interrupted
    return process.returncode or (1 if timed_out or row.get('accepted') is False else 0)


def display_gate():
    started = time.monotonic()
    output = subprocess.run(['ioreg', '-n', 'Root', '-d1', '-a'], capture_output=True, check=True)

    def states(value):
        if isinstance(value, dict):
            if 'IOConsoleLocked' in value:
                yield value['IOConsoleLocked']
            for child in value.values():
                yield from states(child)
        elif isinstance(value, list):
            for child in value:
                yield from states(child)

    locked = list(states(plistlib.loads(output.stdout)))
    (evidence / 'display-state.json').write_text(json.dumps(dict(
        command=['ioreg', '-n', 'Root', '-d1', '-a'], locked=locked,
        elapsed_seconds=time.monotonic() - started), indent=2) + '\n')
    if not locked or any(locked):
        sys.exit('needs the foreground: display locked or state unavailable; no window opened')


def interrupted(_signal, _frame):
    raise KeyboardInterrupt


signal.signal(signal.SIGTERM, interrupted)
disk_gate()
cases = []
if args.only in (None, 'native', 'surface-diagnose'):
    cases.append(('surface', 'benchmark.html', ['--diagnose']))
if args.only in (None, 'native', 'probes', 'zoom'):
    cases.append(('zoom', 'zoom-probe.html', []))
if args.only in (None, 'native', 'probes', 'signin'):
    cases.append(('signin', 'signin/', []))
if args.only in (None, 'native', 'measure'):
    cases.append(('measure', 'benchmark.html', ['--measure']))
if args.only == 'diagnose':
    cases.append(('diagnose', 'signin/', ['--diagnose']))
if args.dry_run:
    plan = dict(dry_run=True, binary=binary, binary_exists=Path(binary).is_file(),
                wrapper=wrapper, cases=cases, bind='127.0.0.1',
                caffeinate=['caffeinate', '-d', '-w', str(os.getpid())],
                stop_on='first failed process or probe verdict; no retry',
                windows_opened=0, server_started=False, caffeinate_started=False)
    (evidence / 'dry-run.json').write_text(json.dumps(plan, indent=2) + '\n')
    print(json.dumps(plan), flush=True)
    sys.exit(0 if Path(binary).is_file() else 1)

run('disk', ['df', '-h', '/Volumes/SSD Samsung'])
if args.only in (None, 'tests'):
    if run('frame-tests', [wrapper, 'cargo', 'test', '--locked', '--bin', 'frame']):
        sys.exit(1)
if args.only in (None, 'build'):
    if run('canvas-build', [wrapper, 'cargo', 'build', '--locked', '--example', 'canvas_check']):
        sys.exit(1)
if not cases:
    sys.exit(0)
for name, _, _ in cases:
    if (evidence / name).exists():
        sys.exit(f'Refusing to overwrite {evidence / name}; choose a fresh evidence directory')
display_gate()
with contextlib.ExitStack() as cleanup:
    awake = subprocess.Popen(['caffeinate', '-d', '-w', str(os.getpid())])
    cleanup.callback(stop_process, awake)
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(root / 'tests/canvas'))
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
    cleanup.callback(server.server_close)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    cleanup.callback(server.shutdown)
    port = server.server_address[1]
    for name, path, extra in cases:
        destination = evidence / name
        destination.mkdir()
        status = run(name, ['script/media-env', binary,
                           f'http://127.0.0.1:{port}/{path}', '--zoom', '1,0.35,2,1',
                           '--evidence', str(destination), *extra], timeout=600)
        if status:
            sys.exit(1)
