#!/usr/bin/env python3
"""Builds the MCP Bundle (.mcpb) for Claude Desktop (ZK-74).

A .mcpb is a ZIP with manifest.json at its root and the server under server/. The tool list in
the manifest is read from the built binary itself (`znimok mcp`, tools/list), so the bundle and
the code never disagree.

    python packaging/mcpb/pack.py --win target/release/znimok.exe --mac dist/znimok \
        --out dist/znimok.mcpb

At least one of --win / --mac; the host one is asked for the tool list.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent


def workspace_version() -> str:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    m = re.search(r'\[workspace\.package\][^\[]*?version\s*=\s*"([^"]+)"', text, re.S)
    if not m:
        sys.exit("no workspace version in Cargo.toml")
    return m.group(1)


def tools_from(binary: Path) -> list:
    """Asks the server itself, over the legacy handshake (works with any MCP era)."""
    msgs = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize",
         "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                    "clientInfo": {"name": "mcpb pack", "version": "1"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
    ]
    stdin = "".join(json.dumps(m) + "\n" for m in msgs)
    out = subprocess.run([str(binary), "mcp"], input=stdin, capture_output=True,
                         text=True, encoding="utf-8", timeout=60, check=True).stdout
    for line in out.splitlines():
        msg = json.loads(line)
        if msg.get("id") == 2:
            return [{"name": t["name"], "description": t["description"]}
                    for t in msg["result"]["tools"]]
    sys.exit("the server did not answer tools/list")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--win", type=Path, help="znimok.exe (Windows CLI)")
    ap.add_argument("--mac", type=Path, help="znimok (macOS CLI, universal or arm64)")
    ap.add_argument("--out", type=Path, required=True)
    a = ap.parse_args()
    if not a.win and not a.mac:
        sys.exit("give --win and/or --mac")
    host = a.win if os.name == "nt" else a.mac
    host = host or a.win or a.mac

    manifest = json.loads((HERE / "manifest.template.json").read_text(encoding="utf-8"))
    manifest["version"] = workspace_version()
    manifest["tools"] = tools_from(host)
    platforms = [p for p, b in (("darwin", a.mac), ("win32", a.win)) if b]
    manifest["compatibility"]["platforms"] = platforms

    a.out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(a.out, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("manifest.json", json.dumps(manifest, ensure_ascii=False, indent=2))
        # The app icon for Claude Desktop's extension list.
        z.write(HERE.parent.parent / "crates/znimok-app/icons/app-512.png", "icon.png")
        if a.win:
            z.write(a.win, "server/znimok.exe")
        if a.mac:
            info = zipfile.ZipInfo("server/znimok")
            info.external_attr = 0o755 << 16  # executable after unpacking on macOS
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, a.mac.read_bytes())
    print(f"{a.out}: {len(manifest['tools'])} tools, platforms {', '.join(platforms)}")


if __name__ == "__main__":
    main()
