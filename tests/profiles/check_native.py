#!/usr/bin/env python3
"""Synthetic local login, Keychain fill and fresh-process session/cookie restore."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs
from pathlib import Path
import json
import os
import subprocess
import tempfile
import threading
import sys

class Fixture(BaseHTTPRequestHandler):
    events = []
    def log_message(self, *_):
        pass

    def do_GET(self):
        if self.path == '/signed-in':
            ok = 'frame_fixture=authenticated' in self.headers.get('Cookie', '')
            Fixture.events.append({'signed_in_cookie': ok})
            body = b'<h1 id="authenticated">Signed in</h1>' if ok else b'<h1>Signed out</h1>'
        else:
            Fixture.events.append({'login_page_requested': self.path == '/login'})
            body = b'''<!doctype html><title>Fixture login</title>
            <form action="/signin" method="post"><label>Account <input name="username" autocomplete="username"></label>
            <label>Password <input name="password" type="password"></label><button>Sign in</button></form>'''
        self.send_response(200)
        self.send_header('Content-Type', 'text/html')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        fields = parse_qs(self.rfile.read(int(self.headers.get('Content-Length', 0))).decode())
        ok = fields == {'username': ['fixture-user'], 'password': ['fixture-password']}
        Fixture.events.append({'submitted_credentials_valid': ok})
        self.send_response(303 if ok else 403)
        if ok:
            self.send_header('Set-Cookie', 'frame_fixture=authenticated; Max-Age=3600; Path=/; HttpOnly; SameSite=Lax')
            self.send_header('Location', '/signed-in')
        self.send_header('Content-Length', '0')
        self.end_headers()

server = ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
threading.Thread(target=server.serve_forever, daemon=True).start()
base = f'http://127.0.0.1:{server.server_port}/'
binary = Path(sys.argv[1]).resolve()
results = []
with tempfile.TemporaryDirectory(prefix='frame-profile-acceptance-') as root:
    try:
        for reopen in (False, True):
            command = [str(binary), '--root', root, '--base', base]
            if reopen:
                command.append('--reopen')
            run = subprocess.run(command, capture_output=True, text=True, timeout=55)
            phase = {'phase': 'reopen' if reopen else 'login', 'exit_code': run.returncode}
            for line in run.stdout.splitlines():
                try:
                    phase.update(json.loads(line))
                except json.JSONDecodeError:
                    pass
            results.append(phase)
            if run.returncode or not phase.get('passed'):
                diagnostic = (run.stderr + '\n' + run.stdout).replace('fixture-password', '[redacted]').replace('fixture-user', '[redacted]')
                print(diagnostic[-5000:], file=sys.stderr)
                print(json.dumps({'passed': False, 'phases': results, 'events': Fixture.events}))
                sys.exit(1)
    finally:
        subprocess.run([str(binary), '--root', root, '--base', base, '--reopen', '--cleanup'],
                       capture_output=True, text=True, timeout=10)
server.shutdown()
print(json.dumps({'passed': all(r.get('passed') for r in results), 'phases': results, 'events': Fixture.events}, indent=2))
