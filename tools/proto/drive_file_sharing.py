"""ZK-282 prototype: does Google's `drive.file` scope let one person's Znimok see files that
another person's Znimok made and shared with them?

Same OAuth client as Znimok (desktop, PKCE, loopback). Two people, two runs:

  python drive_file_sharing.py owner  <colleague e-mail>
      signs in (account A), makes a folder «Znimok ZK-282 test» with one PNG in it — both made
      by the app, so drive.file sees them — and shares the folder with the colleague as writer.
      Prints the folder id.

  python drive_file_sharing.py member <folder id> [<A's file id> <B's e-mail>]
      signs in (account B, the colleague), then with drive.file only:
        1. files.get of the folder               — can B's Znimok see the folder at all?
        2. files.list of the folder's children   — does it see A's file inside?
        3. files.get of A's file (by id from 2 or from the owner run's output)
        4. upload a file into the folder         — can it add to the shared folder?
      Prints yes / no per step. Nothing else in either Drive is touched.

  python drive_file_sharing.py cleanup <folder id>
      (account A) moves the test folder to the trash.

The client comes from C:/AIHome/keys/google_oauth.json (never printed); tokens stay in memory.
"""

import base64
import hashlib
import http.server
import json
import os
import secrets
import sys
import threading
import urllib.parse
import urllib.request
import webbrowser

CLIENT = json.load(open("C:/AIHome/keys/google_oauth.json", encoding="utf-8"))["installed"]
SCOPE = "https://www.googleapis.com/auth/drive.file openid email"
API = "https://www.googleapis.com/drive/v3"
UPLOAD = "https://www.googleapis.com/upload/drive/v3/files"
# A 1×1 PNG.
PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
)


def sign_in(hint=None):
    verifier = secrets.token_urlsafe(64)
    challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).rstrip(b"=").decode()
    state = secrets.token_urlsafe(16)
    got = {}

    class Back(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            q = urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query)
            got.update({k: v[0] for k, v in q.items()})
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write("Готово — можна закрити вкладку.".encode())

        def log_message(self, *a):
            pass

    srv = http.server.HTTPServer(("127.0.0.1", 0), Back)
    redirect = f"http://127.0.0.1:{srv.server_port}/"
    url = "https://accounts.google.com/o/oauth2/v2/auth?" + urllib.parse.urlencode({
        "client_id": CLIENT["client_id"],
        "redirect_uri": redirect,
        "response_type": "code",
        "scope": SCOPE,
        "code_challenge": challenge,
        "code_challenge_method": "S256",
        "state": state,
        "prompt": "select_account consent",
        **({"login_hint": hint} if hint else {}),
    })
    print("Відкриваю браузер для входу в Google…")
    webbrowser.open(url)
    t = threading.Thread(target=srv.handle_request)
    t.start()
    t.join(300)
    srv.server_close()
    if got.get("state") != state or "code" not in got:
        sys.exit(f"вхід не вдався: {got.get('error', 'немає коду')}")
    body = urllib.parse.urlencode({
        "client_id": CLIENT["client_id"],
        "client_secret": CLIENT["client_secret"],
        "code": got["code"],
        "code_verifier": verifier,
        "grant_type": "authorization_code",
        "redirect_uri": redirect,
    }).encode()
    tok = json.load(urllib.request.urlopen("https://oauth2.googleapis.com/token", body, timeout=30))
    if "drive.file" not in tok.get("scope", ""):
        sys.exit("доступ до Drive не надано (галочку на екрані згоди знято)")
    who = call(tok["access_token"], "GET", "https://openidconnect.googleapis.com/v1/userinfo")[1]
    print("Увійшли як", who.get("email"))
    if hint and who.get("email", "").lower() != hint.lower():
        sys.exit(f"треба увійти як {hint}, а не {who.get('email')} — запустіть ще раз")
    return tok["access_token"]


def call(token, method, url, body=None, ctype="application/json"):
    data = None if body is None else (body if isinstance(body, bytes) else json.dumps(body).encode())
    req = urllib.request.Request(url, data=data, method=method,
                                 headers={"Authorization": "Bearer " + token, "Content-Type": ctype})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            raw = r.read()
            try:
                return r.status, (json.loads(raw) if raw else {})
            except ValueError:
                return r.status, {"bytes": len(raw)}
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            return e.code, json.loads(raw)
        except ValueError:
            return e.code, {"error": raw.decode(errors="replace")[:300]}


def upload(token, name, parent):
    boundary = "zk282" + secrets.token_hex(8)
    meta = json.dumps({"name": name, "parents": [parent]}).encode()
    body = (f"--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n".encode() + meta
            + f"\r\n--{boundary}\r\nContent-Type: image/png\r\n\r\n".encode() + PNG
            + f"\r\n--{boundary}--\r\n".encode())
    return call(token, "POST", UPLOAD + "?uploadType=multipart&supportsAllDrives=true", body,
                f"multipart/related; boundary={boundary}")


def owner(email):
    t = sign_in()
    s, folder = call(t, "POST", API + "/files", {
        "name": "Znimok ZK-282 test", "mimeType": "application/vnd.google-apps.folder"})
    assert s == 200, folder
    s, f = upload(t, "made-by-A.png", folder["id"])
    assert s == 200, f
    s, p = call(t, "POST", API + f"/files/{folder['id']}/permissions?sendNotificationEmail=false",
                {"type": "user", "role": "writer", "emailAddress": email})
    print("поділено з колегою:", "так" if s == 200 else f"ні ({s} {p})")
    print("\nТЕКА:", folder["id"])
    print("ФАЙЛ A:", f["id"])
    print("\nКолега запускає:  python drive_file_sharing.py member", folder["id"], f["id"])


def member(folder, file_a=None, who=None):
    t = sign_in(who)
    s, r = call(t, "GET", API + f"/files/{folder}?fields=id,name,capabilities(canAddChildren)&supportsAllDrives=true")
    print("1. бачить теку:", "так" if s == 200 else f"ні ({s})", r.get("name", ""), r.get("capabilities", ""))
    q = urllib.parse.quote(f"'{folder}' in parents and trashed = false")
    s, r = call(t, "GET", API + f"/files?q={q}&fields=files(id,name)&includeItemsFromAllDrives=true&supportsAllDrives=true")
    names = [x["name"] for x in r.get("files", [])] if s == 200 else []
    print("2. бачить вміст теки:", names if s == 200 else f"ні ({s} {r})")
    if file_a:
        s, r = call(t, "GET", API + f"/files/{file_a}?fields=id,name&supportsAllDrives=true")
        print("3. бачить файл A за id:", "так" if s == 200 else f"ні ({s})")
        s, _ = call(t, "GET", API + f"/files/{file_a}?alt=media&supportsAllDrives=true")
        print("   може завантажити його вміст:", "так" if s == 200 else f"ні ({s})")
    s, r = upload(t, "made-by-B.png", folder)
    print("4. може додати файл у теку:", "так" if s == 200 else f"ні ({s} {r.get('error', r)})")


def cleanup(folder, who=None):
    t = sign_in(who)
    # Before the trash: does A's Znimok see what B's Znimok put into A's folder?
    q = urllib.parse.quote(f"'{folder}' in parents and trashed = false")
    s, r = call(t, "GET", API + f"/files?q={q}&fields=files(id,name)&supportsAllDrives=true")
    print("власник бачить у теці:", [x["name"] for x in r.get("files", [])] if s == 200 else f"ні ({s})")
    s, r = call(t, "PATCH", API + f"/files/{folder}", {"trashed": True})
    print("теку в кошик:", "так" if s == 200 else f"ні ({s} {r})")


if __name__ == "__main__":
    if len(sys.argv) < 3 or sys.argv[1] not in ("owner", "member", "cleanup"):
        sys.exit(__doc__)
    {"owner": lambda: owner(sys.argv[2]),
     "member": lambda: member(sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else None,
                              sys.argv[4] if len(sys.argv) > 4 else None),
     "cleanup": lambda: cleanup(sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else None)}[sys.argv[1]]()
