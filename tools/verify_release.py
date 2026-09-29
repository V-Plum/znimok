#!/usr/bin/env python3
"""Independent check of a Znimok release (ZK-79).

Downloads the assets of a GitHub release (or takes a folder), checks every file against
SHA256SUMS, and — when the release is signed — the ECDSA P-256 signature of SHA256SUMS with the
public key committed in this repository. Our own code, not the app's: otherwise the check would
only prove the app agrees with itself.

    python tools/verify_release.py v0.1.0            # a published or draft release (gh needed)
    python tools/verify_release.py --dir release-preview

Exit code 0 = everything matches.
"""

import argparse
import hashlib
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = "V-Plum/znimok"
PUB = Path(__file__).resolve().parent.parent / "keys" / "znimok-release-p256.pub.pem"


def sha256(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def check_sums(d: Path) -> bool:
    sums = d / "SHA256SUMS"
    if not sums.exists():
        print("SHA256SUMS is missing")
        return False
    ok = True
    listed = set()
    for line in sums.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        digest, name = line.split(maxsplit=1)
        name = name.lstrip("*")
        listed.add(name)
        f = d / name
        if not f.exists():
            print(f"MISSING  {name}")
            ok = False
        elif sha256(f) != digest:
            print(f"CHANGED  {name}")
            ok = False
        else:
            print(f"ok       {name}")
    extra = {p.name for p in d.iterdir() if p.is_file()} - listed - {"SHA256SUMS", "SHA256SUMS.sig", "notes.md"}
    for e in sorted(extra):
        print(f"UNLISTED {e}")
        ok = False
    return ok


def check_signature(d: Path) -> bool | None:
    """True / False, or None when the release is not signed."""
    sig = d / "SHA256SUMS.sig"
    if not sig.exists():
        return None
    if not PUB.exists():
        print(f"signed, but {PUB} is not in this checkout")
        return False
    data = (d / "SHA256SUMS").read_bytes()
    try:
        from cryptography.exceptions import InvalidSignature
        from cryptography.hazmat.primitives import hashes
        from cryptography.hazmat.primitives.asymmetric import ec
        from cryptography.hazmat.primitives.serialization import load_pem_public_key

        key = load_pem_public_key(PUB.read_bytes())
        try:
            key.verify(sig.read_bytes(), data, ec.ECDSA(hashes.SHA256()))
            return True
        except InvalidSignature:
            return False
    except ImportError:
        openssl = shutil.which("openssl")
        if not openssl:
            print("neither the `cryptography` package nor openssl is available")
            return False
        r = subprocess.run([openssl, "dgst", "-sha256", "-verify", str(PUB), "-signature",
                            str(sig), str(d / "SHA256SUMS")], capture_output=True, text=True)
        return r.returncode == 0


def check_appcast(d: Path) -> bool | None:
    """The Sparkle appcast (macOS updates): its edSignature over the DMG. None = no appcast."""
    appcast = d / "appcast.xml"
    if not appcast.exists():
        return None
    dmgs = sorted(d.glob("*-macos-arm64.dmg"))
    if not dmgs:
        print("appcast.xml without a DMG")
        return False
    tool = Path(__file__).resolve().parent / "sparkle_appcast.py"
    r = subprocess.run([sys.executable, str(tool), "verify", "--appcast", str(appcast), "--file", str(dmgs[0])],
                       capture_output=True, text=True)
    print(r.stdout.strip() or r.stderr.strip())
    return r.returncode == 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("tag", nargs="?", help="release tag, e.g. v0.1.0")
    ap.add_argument("--dir", type=Path, help="a folder with the assets instead of downloading")
    ap.add_argument("--repo", default=REPO)
    a = ap.parse_args()
    if a.dir:
        d = a.dir
    elif a.tag:
        d = Path(tempfile.mkdtemp(prefix=f"znimok-{a.tag}-"))
        subprocess.run(["gh", "release", "download", a.tag, "-R", a.repo, "-D", str(d)], check=True)
    else:
        ap.error("give a tag or --dir")
    print(f"assets: {d}")
    sums_ok = check_sums(d)
    sig = check_signature(d)
    appcast = check_appcast(d)
    print()
    print("checksums:", "all match" if sums_ok else "MISMATCH")
    print("signature:", {True: "valid (ECDSA P-256, key of this repository)",
                         False: "INVALID", None: "not signed"}[sig])
    print("appcast:", {True: "DMG signature valid (Ed25519, key of this repository)",
                       False: "INVALID", None: "none"}[appcast])
    return 0 if sums_ok and sig is not False and appcast is not False else 1


if __name__ == "__main__":
    sys.exit(main())
