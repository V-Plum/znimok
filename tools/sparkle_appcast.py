#!/usr/bin/env python3
"""Sparkle appcast for a Znimok macOS release (ZK-143): the DMG signed with the Ed25519 key.

    # sign (CI): the private key seed, base64, in the environment
    ZNIMOK_SPARKLE_KEY=... python tools/sparkle_appcast.py sign --file dist/Znimok-1.2.3-macos-arm64.dmg \
        --version 1.2.3 --url https://github.com/V-Plum/znimok/releases/download/v1.2.3/Znimok-1.2.3-macos-arm64.dmg \
        --notes https://github.com/V-Plum/znimok/releases/tag/v1.2.3 --out dist/appcast.xml

    # verify (CI, verify_release.py): the appcast's signature against the committed public key
    python tools/sparkle_appcast.py verify --appcast dist/appcast.xml --file dist/Znimok-1.2.3-macos-arm64.dmg

The signature is plain Ed25519 over the archive bytes, base64 — what Sparkle's `sign_update`
makes and what `SUPublicEDKey` (the base64 public key, keys/znimok-sparkle-ed25519.pub.pem)
verifies. Signing goes through OpenSSL 3 (`pkeyutl -rawin`): LibreSSL on macOS cannot, so on a
Mac the Homebrew openssl@3 is used when present. The private key never touches the disk
unencrypted for longer than the call: it is written to a temporary file that is removed at once.
"""

import argparse
import base64
import os
import re
import shutil
import subprocess
import sys
import tempfile
import xml.sax.saxutils as sx
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PUB = ROOT / "keys" / "znimok-sparkle-ed25519.pub.pem"
# PKCS#8 DER prefix of an Ed25519 private key (RFC 8410): the 32-byte seed follows.
PKCS8_PREFIX = bytes.fromhex("302e020100300506032b657004220420")
MIN_OS = "15.0"


def openssl() -> str:
    for c in ("/opt/homebrew/opt/openssl@3/bin/openssl", "/usr/local/opt/openssl@3/bin/openssl"):
        if Path(c).exists():
            return c
    exe = shutil.which("openssl")
    if not exe:
        sys.exit("openssl not found")
    v = subprocess.run([exe, "version"], capture_output=True, text=True).stdout
    if not v.startswith("OpenSSL 3"):
        sys.exit(f"OpenSSL 3 is needed for Ed25519 (found: {v.strip()})")
    return exe


def sign(seed_b64: str, path: Path) -> str:
    seed = base64.b64decode(seed_b64.strip())
    if len(seed) != 32:
        sys.exit("ZNIMOK_SPARKLE_KEY must be the 32-byte seed, base64")
    fd, key = tempfile.mkstemp(suffix=".pem")
    try:
        os.close(fd)
        pem = base64.encodebytes(PKCS8_PREFIX + seed).decode()
        Path(key).write_text(f"-----BEGIN PRIVATE KEY-----\n{pem}-----END PRIVATE KEY-----\n")
        sig = subprocess.run(
            [openssl(), "pkeyutl", "-sign", "-inkey", key, "-rawin", "-in", str(path)],
            capture_output=True, check=True,
        ).stdout
    finally:
        os.remove(key)
    return base64.b64encode(sig).decode()


def verify(sig_b64: str, path: Path) -> bool:
    fd, sig = tempfile.mkstemp(suffix=".sig")
    try:
        os.close(fd)
        Path(sig).write_bytes(base64.b64decode(sig_b64))
        r = subprocess.run(
            [openssl(), "pkeyutl", "-verify", "-pubin", "-inkey", str(PUB), "-rawin",
             "-in", str(path), "-sigfile", sig],
            capture_output=True, text=True,
        )
    finally:
        os.remove(sig)
    # pkeyutl says "Signature Verified Successfully" (dgst would say "Verified OK").
    return r.returncode == 0 and "Verified" in r.stdout


def appcast(version: str, url: str, notes: str, length: int, sig: str, min_os: str) -> str:
    e = sx.escape
    q = sx.quoteattr
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">\n'
        "  <channel>\n"
        "    <title>Znimok</title>\n"
        "    <item>\n"
        f"      <title>Znimok {e(version)}</title>\n"
        f"      <sparkle:version>{e(version)}</sparkle:version>\n"
        f"      <sparkle:shortVersionString>{e(version)}</sparkle:shortVersionString>\n"
        f"      <sparkle:minimumSystemVersion>{e(min_os)}</sparkle:minimumSystemVersion>\n"
        f"      <sparkle:releaseNotesLink>{e(notes)}</sparkle:releaseNotesLink>\n"
        f"      <enclosure url={q(url)} length=\"{length}\" type=\"application/octet-stream\"\n"
        f"                 sparkle:edSignature={q(sig)}/>\n"
        "    </item>\n"
        "  </channel>\n"
        "</rss>\n"
    )


def parse(appcast_xml: str) -> dict:
    m = re.search(r'sparkle:edSignature="([^"]+)"', appcast_xml)
    n = re.search(r'length="(\d+)"', appcast_xml)
    v = re.search(r"<sparkle:version>([^<]+)</sparkle:version>", appcast_xml)
    if not (m and n and v):
        sys.exit("appcast: no signature, length or version")
    return {"sig": m.group(1), "length": int(n.group(1)), "version": v.group(1)}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("sign")
    s.add_argument("--file", required=True, type=Path)
    s.add_argument("--version", required=True)
    s.add_argument("--url", required=True)
    s.add_argument("--notes", required=True)
    s.add_argument("--min-os", default=MIN_OS)
    s.add_argument("--out", type=Path)
    v = sub.add_parser("verify")
    v.add_argument("--appcast", required=True, type=Path)
    v.add_argument("--file", required=True, type=Path)
    a = ap.parse_args()
    if a.cmd == "sign":
        seed = os.environ.get("ZNIMOK_SPARKLE_KEY")
        if not seed:
            sys.exit("ZNIMOK_SPARKLE_KEY is not set")
        sig = sign(seed, a.file)
        if not verify(sig, a.file):
            sys.exit("the signature does not verify with the committed public key")
        xml = appcast(a.version, a.url, a.notes, a.file.stat().st_size, sig, a.min_os)
        if a.out:
            a.out.write_text(xml, encoding="utf-8")
            print(f"{a.out}: signed {a.file.name} ({a.file.stat().st_size} bytes), verified")
        else:
            sys.stdout.write(xml)
        return 0
    info = parse(a.appcast.read_text(encoding="utf-8"))
    size = a.file.stat().st_size
    if info["length"] != size:
        print(f"length {info['length']} in the appcast, file has {size}")
        return 1
    if not verify(info["sig"], a.file):
        print("edSignature does not verify with keys/znimok-sparkle-ed25519.pub.pem")
        return 1
    print(f"appcast {info['version']}: signature and length of {a.file.name} verified")
    return 0


if __name__ == "__main__":
    sys.exit(main())
