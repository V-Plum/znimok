# -*- coding: utf-8 -*-
"""ZK-97: the browser log end to end, with the REAL Chrome, the extension and the Native Messaging host.

    python tools/devtools_e2e.py

1. builds the CLI (the host) and the hub's stand-in (`hub_probe`, an IPC server under a suffix of
   its own, so a running Znimok is not touched);
2. registers the host for Chrome under HKCU for the test (the previous value is put back after);
3. starts Chrome on a profile of its own with remote debugging, loads the extension unpacked over
   CDP (`--load-extension` is off in branded Chrome since 137) and opens a test page that logs to
   the console every 300 ms, throws now and then, and POSTs JSON with a header of its own;
4. the probe «records» for a few seconds once the host has said hello, and prints the log;
5. checks: console events with their text, an uncaught error, the POST with its request headers,
   payload, response headers and body — the full data of the owner's decision of 30.09.2026.

Windows only (the host's registration is in the registry here). Exit code 0 when it all holds.
"""
import http.server
import json
import os
import shutil
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
import winreg

sys.stdout.reconfigure(encoding="utf-8")
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHROME = r"C:\Program Files\Google\Chrome\Application\chrome.exe"
HOST = "com.znimok.devtools"
KEY = r"Software\Google\Chrome\NativeMessagingHosts" + "\\" + HOST
CDP = 9341
SUFFIX = "e2e-" + str(os.getpid())

PAGE = """<!doctype html><html><head><meta charset="utf-8"><title>Znimok devtools test</title></head>
<body><div id="n">0</div><script>
let n = 0;
setInterval(() => {
  n++;
  document.getElementById("n").textContent = n;
  console.log("tick " + n, {n: n, ok: true});
  if (n % 4 === 0) setTimeout(() => { throw new Error("boom " + n); }, 0);
  if (n % 3 === 0) fetch("/api/echo?n=" + n, {method: "POST", headers: {"Content-Type": "application/json", "X-Znimok-Test": "yes"},
                                             body: JSON.stringify({n: n, secret: "s3cr3t"})}).then(r => r.text()).catch(() => {});
}, 300);
</script></body></html>"""


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        body = PAGE.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0)
        got = self.rfile.read(n)
        body = json.dumps({"echo": json.loads(got or b"{}"), "ok": True}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("X-Znimok-Reply", "pong")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def run(cmd, **kw):
    print("$", " ".join(cmd))
    return subprocess.run(cmd, cwd=ROOT, check=True, **kw)


def main():
    if not os.path.isfile(CHROME):
        print("skipped: no Chrome")
        return 0
    # Two builds: with `--example` cargo builds the example only, not the CLI.
    run(["cargo", "build", "-q", "-p", "znimok-cli"])
    run(["cargo", "build", "-q", "-p", "znimok-devtools", "--example", "hub_probe"])
    target = os.path.join(ROOT, "target", "debug")
    cli = os.path.join(target, "znimok.exe")
    probe = os.path.join(target, "examples", "hub_probe.exe")
    tmp = tempfile.mkdtemp(prefix="znimok-devtools-e2e-")
    # The host's manifest for the test, and the registry pointing at it (the old value kept).
    manifest = os.path.join(tmp, HOST + ".json")
    ext_id = "mmkhmcoabdpolbfkghgpaihcpjlliakn"
    json.dump({"name": HOST, "description": "e2e", "path": cli, "type": "stdio",
               "allowed_origins": [f"chrome-extension://{ext_id}/"]}, open(manifest, "w"))
    old = None
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, KEY) as k:
            old = winreg.QueryValue(k, None)
    except OSError:
        pass
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, KEY) as k:
        winreg.SetValue(k, None, winreg.REG_SZ, manifest)
    httpd = socketserver.TCPServer(("127.0.0.1", 0), Handler)
    port = httpd.server_address[1]
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    env = dict(os.environ, ZNIMOK_IPC_SUFFIX=SUFFIX, ZNIMOK_HOST_LOG=os.path.join(tmp, "host.log"))
    probe_p = subprocess.Popen([probe, "6"], cwd=ROOT, env=env, stdout=subprocess.PIPE, text=True, encoding="utf-8")
    flags = subprocess.CREATE_NEW_PROCESS_GROUP
    ch = subprocess.Popen([CHROME, f"--user-data-dir={os.path.join(tmp, 'profile')}", f"--remote-debugging-port={CDP}",
                           "--enable-unsafe-extension-debugging", "--no-first-run", "--no-default-browser-check",
                           "--disable-features=CalculateNativeWinOcclusion", "--disable-backgrounding-occluded-windows",
                           "--disable-renderer-backgrounding", "--window-position=40,40", "--window-size=900,700",
                           "about:blank"], env=env, creationflags=flags)
    ok = False
    try:
        v = None
        for _ in range(80):
            try:
                v = json.load(urllib.request.urlopen(f"http://127.0.0.1:{CDP}/json/version"))
                break
            except Exception:
                time.sleep(0.25)
        import asyncio
        import websockets

        async def go():
            async with websockets.connect(v["webSocketDebuggerUrl"], max_size=None) as ws:
                await ws.send(json.dumps({"id": 1, "method": "Extensions.loadUnpacked", "params": {"path": os.path.join(ROOT, "extension")}}))
                r = json.loads(await ws.recv())
                print("extension:", r.get("result", r))
                await ws.send(json.dumps({"id": 2, "method": "Target.getTargets"}))
                t = json.loads(await ws.recv())
                page = next(i for i in t["result"]["targetInfos"] if i["type"] == "page")
                await ws.send(json.dumps({"id": 3, "method": "Target.attachToTarget", "params": {"targetId": page["targetId"], "flatten": True}}))
                while True:
                    m = json.loads(await ws.recv())
                    if m.get("id") == 3:
                        sid = m["result"]["sessionId"]
                        break
                await ws.send(json.dumps({"id": 4, "sessionId": sid, "method": "Page.navigate", "params": {"url": f"http://127.0.0.1:{port}/"}}))
                while True:
                    m = json.loads(await ws.recv())
                    if m.get("id") == 4:
                        break
                await ws.send(json.dumps({"id": 5, "sessionId": sid, "method": "Target.activateTarget", "params": {"targetId": page["targetId"]}}))
                await ws.recv()
                # What the extension's service worker sees (for a failing run).
                await asyncio.sleep(2)
                await ws.send(json.dumps({"id": 20, "method": "Target.getTargets"}))
                while True:
                    m = json.loads(await ws.recv())
                    if m.get("id") == 20:
                        break
                sw = [i for i in m["result"]["targetInfos"] if i["type"] == "service_worker" and ext_id in i["url"]]
                print("service worker:", [i["url"] for i in sw])
                if sw:
                    await ws.send(json.dumps({"id": 21, "method": "Target.attachToTarget", "params": {"targetId": sw[0]["targetId"], "flatten": True}}))
                    while True:
                        m = json.loads(await ws.recv())
                        if m.get("id") == 21:
                            swid = m["result"]["sessionId"]
                            break
                    expr = "'port ' + (port ? 'open' : 'closed') + ' · ' + JSON.stringify(app)"
                    await ws.send(json.dumps({"id": 22, "sessionId": swid, "method": "Runtime.evaluate", "params": {"expression": expr}}))
                    while True:
                        m = json.loads(await ws.recv())
                        if m.get("id") == 22:
                            print("extension state:", m.get("result"))
                            break
                # Our own CDP session is let go: the extension's debugger attaches to the tab.
                await ws.send(json.dumps({"id": 6, "method": "Target.detachFromTarget", "params": {"sessionId": sid}}))
        asyncio.run(go())
        out, _ = probe_p.communicate(timeout=120)
        hl = os.path.join(tmp, "host.log")
        print("host log:", open(hl, encoding="utf-8").read() if os.path.exists(hl) else "(none — the host was not started)")
        log = json.loads(out.strip().splitlines()[-1])
        if "error" in log:
            print("FAIL:", log["error"])
            return 1
        ev = [e["json"] for e in log["events"]]
        kinds = {}
        for e in ev:
            kinds[e.get("k")] = kinds.get(e.get("k"), 0) + 1
        print("events:", len(ev), kinds)
        ticks = [e for e in ev if e.get("k") == "console" and str(e.get("text", "")).startswith("tick ")]
        errors = [e for e in ev if e.get("k") == "error" and "boom" in str(e.get("text"))]
        posts = [e for e in ev if e.get("k") == "net" and e.get("method") == "POST" and "/api/echo" in e.get("url", "")]
        checks = {
            "console ticks with their objects": len(ticks) >= 5 and any("ok" in str(e.get("args")) for e in ticks),
            "uncaught errors with a stack": len(errors) >= 1,
            "times in order on the video": all(a["ms"] <= b["ms"] for a, b in zip(log["events"], log["events"][1:])),
            "a POST": len(posts) >= 1,
        }
        if posts:
            p = posts[0]
            req_h = {k.lower(): v for k, v in (p.get("reqHeaders") or {}).items()}
            res_h = {k.lower(): v for k, v in (p.get("resHeaders") or {}).items()}
            checks["its request headers"] = req_h.get("x-znimok-test") == "yes"
            checks["its payload"] = "s3cr3t" in str(p.get("postData"))
            checks["its response headers"] = res_h.get("x-znimok-reply") == "pong"
            checks["its response body"] = '"echo"' in str(p.get("body")) and p.get("status") == 200
            checks["its timing"] = isinstance(p.get("timing"), dict)
        for name, good in checks.items():
            print(("ok  " if good else "FAIL"), name)
        ok = all(checks.values())
    finally:
        subprocess.call(["taskkill", "/F", "/T", "/PID", str(ch.pid)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if probe_p.poll() is None:
            probe_p.kill()
        httpd.shutdown()
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER, KEY) as k:
            if old is not None:
                winreg.SetValue(k, None, winreg.REG_SZ, old)
        if old is None:
            try:
                winreg.DeleteKey(winreg.HKEY_CURRENT_USER, KEY)
            except OSError:
                pass
        time.sleep(1)
        shutil.rmtree(tmp, ignore_errors=True)
    print("ALL OK" if ok else "FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
