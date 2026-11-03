#!/usr/bin/env python3
"""Real Servo restarts: cookies, localStorage, IndexedDB, HTTP cache, isolation."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import json
import subprocess
import sys
import tempfile
import threading

class Fixture(BaseHTTPRequestHandler):
    cache_requests = 0
    def log_message(self, *_):
        pass
    def do_GET(self):
        if self.path == '/cache.js':
            Fixture.cache_requests += 1
            body = b'window.cacheSeen = "durable";'
            kind, cache = 'text/javascript', 'public, max-age=3600'
        else:
            body = b'''<!doctype html><title>Storage fixture</title>
            <script src="/cache.js"></script><h1>Storage fixture</h1><script>
            (async () => {
              const phase = location.search.slice(1), write = phase === 'write';
              if (write) {
                localStorage.setItem('fixture', 'persisted');
                document.cookie = 'fixture=persisted; Max-Age=3600; Path=/; SameSite=Lax';
              }
              const db = await new Promise((resolve,reject) => {
                const r = indexedDB.open('fixture', 1);
                r.onupgradeneeded = () => r.result.createObjectStore('values');
                r.onsuccess = () => resolve(r.result); r.onerror = () => reject(r.error);
              });
              if (write) await new Promise((resolve,reject) => {
                const tx=db.transaction('values','readwrite');
                tx.objectStore('values').put('persisted','key');
                tx.oncomplete=resolve;tx.onerror=()=>reject(tx.error);
              });
              const value=await new Promise((resolve,reject) => {
                const r=db.transaction('values').objectStore('values').get('key');
                r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);
              });
              db.close();
              const checks=[localStorage.getItem('fixture')==='persisted',document.cookie.includes('fixture=persisted'),value==='persisted'];
              window.storageResult = window.cacheSeen==='durable' && checks.every(v=>v===(phase!=='isolated'));
            })().catch(e=>{document.body.textContent=String(e);window.storageResult=false});
            </script>'''
            kind, cache = 'text/html', 'no-store'
        self.send_response(200)
        self.send_header('Content-Type',kind)
        self.send_header('Cache-Control',cache)
        self.send_header('Content-Length',str(len(body)))
        self.end_headers()
        self.wfile.write(body)

server=ThreadingHTTPServer(('127.0.0.1',0),Fixture)
threading.Thread(target=server.serve_forever,daemon=True).start()
base=f'http://127.0.0.1:{server.server_port}/'
results=[]
with tempfile.TemporaryDirectory(prefix='platinum-storage-') as root:
    for phase, expected_requests in [('write',1),('read',1),('isolated',2)]:
        run=subprocess.run([str(Path(sys.argv[1]).resolve()),'--root',root,'--base',base,'--phase',phase],capture_output=True,text=True,timeout=55)
        report={'phase':phase,'exit_code':run.returncode,'cache_requests':Fixture.cache_requests}
        for line in run.stdout.splitlines():
            try: report.update(json.loads(line))
            except json.JSONDecodeError: pass
        report['cache_reused_or_isolated']=Fixture.cache_requests==expected_requests
        results.append(report)
        if run.returncode or not report.get('passed') or not report['cache_reused_or_isolated']:
            print((run.stderr+'\n'+run.stdout)[-6000:],file=sys.stderr)
            print(json.dumps({'passed':False,'phases':results},indent=2));sys.exit(1)
server.shutdown()
print(json.dumps({'passed':True,'phases':results},indent=2))
