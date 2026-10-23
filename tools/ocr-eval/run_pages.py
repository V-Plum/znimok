#!/usr/bin/env python3
"""Page screenshots through znimok-ocr.exe (ZK-308): how many of a page's key phrases the helper
reads. The evaluation set (set/) is single lines; a page — a banner, cards, buttons, light text
on dark — is what broke the layout, so this keeps a few whole pages with the phrases an agent
would look for. Windows only (the helper).

    python tools/ocr-eval/run_pages.py target/ocr-helper/znimok-ocr.exe [pages/…png] [--dump]

Each `pages/<name>.png` has `pages/<name>.keys` — one phrase per line, matched case-insensitively
as a substring of any line read. Prints a table: phrases read, lines, lines at ≥ 90 confidence,
time; with --dump every line.
"""

import json
import os
import struct
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image

PAGES = Path(__file__).resolve().parent / "pages"


def read(exe: str, png: Path):
    img = Image.open(png).convert("L")
    p = subprocess.Popen([exe, "--idle", "5"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL)
    t0 = time.time()
    p.stdin.write(b"ZOCR" + struct.pack("<IIIII", 2, 0, img.width, img.height, 0) + img.tobytes())
    p.stdin.flush()
    n = struct.unpack("<I", p.stdout.read(4))[0]
    r = json.loads(p.stdout.read(n))
    dt = time.time() - t0
    p.stdin.write(b"ZOCR" + struct.pack("<IIIII", 2, 2, 0, 0, 0))
    p.stdin.close()
    p.wait()
    return r.get("lines", []), dt


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if not args:
        print(__doc__)
        return 2
    exe = args[0]
    pngs = [Path(a) for a in args[1:]] or sorted(PAGES.glob("*.png"))
    dump = "--dump" in sys.argv
    print("| page | phrases read | lines | ≥ 90 | time |")
    print("|---|---|---|---|---|")
    missing_all = {}
    for png in pngs:
        keys_file = png.with_suffix(".keys")
        keys = [k.strip() for k in keys_file.read_text(encoding="utf-8").splitlines() if k.strip()] \
            if keys_file.exists() else []
        lines, dt = read(exe, png)
        text = "\n".join(l["text"].lower() for l in lines)
        hit = [k for k in keys if k.lower() in text]
        missing = [k for k in keys if k.lower() not in text]
        good = sum(1 for l in lines if l["conf"] >= 90)
        print(f"| {png.stem} | {len(hit)} / {len(keys)} | {len(lines)} | {good} | {dt:.1f} s |")
        if missing:
            missing_all[png.stem] = missing
        if dump:
            for l in lines:
                print(f"  ({l['x']:4},{l['y']:4} {l['w']:4}x{l['h']:3}) {l['conf']:5.1f} {l['text'][:90]}")
    for page, miss in missing_all.items():
        print(f"\n**{page}** — not read: {', '.join(miss)}")
    return 0


if __name__ == "__main__":
    os.environ.setdefault("PYTHONIOENCODING", "utf-8")
    sys.exit(main())
