#!/usr/bin/env python3
"""Memory and time of Tesseract per model set and picture size (ZK-120, step 3): the helper's
RAM matters more than the installer's size (owner, 29.09).

    TESSDATA_FAST=… TESSDATA_BEST=… python tools/ocr-eval/memory.py > memory.md

Builds screen-sized pictures from the evaluation set (the text of real interface lines, packed as
densely as a busy screen), runs the tesseract CLI under /usr/bin/time -v and reports the peak
resident memory (RSS) and the wall time: models only (a 1×1 picture), a full screen at 1× and 2×.
Linux only (GNU time).
"""

import os
import re
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

SET = Path(__file__).resolve().parent / "set"
TMP = Path(tempfile.gettempdir())


def screen(w: int, h: int) -> Path:
    """A w×h picture tiled with the set's pictures, like a screen full of interface text."""
    canvas = Image.new("RGB", (w, h), (255, 255, 255))
    x = y = row = 0
    tiles = [Image.open(p).convert("RGB") for p in sorted(SET.glob("*light*.png"))]
    i = 0
    while y < h:
        t = tiles[i % len(tiles)]
        i += 1
        if x + t.width > w:
            x, y, row = 0, y + row, 0
            continue
        canvas.paste(t, (x, y))
        x += t.width
        row = max(row, t.height)
    p = TMP / f"zk-screen-{w}x{h}.png"
    canvas.save(p)
    return p


def upscale(p: Path, k: int) -> Path:
    if k == 1:
        return p
    img = Image.open(p)
    out = TMP / f"{p.stem}-x{k}.png"
    img.resize((img.width * k, img.height * k), Image.LANCZOS).save(out)
    return out


def measure(src: Path, tessdata: str, langs: str) -> tuple[float, float]:
    r = subprocess.run(["/usr/bin/time", "-v", "tesseract", str(src), "stdout", "--tessdata-dir",
                        tessdata, "-l", langs, "--psm", "3"], capture_output=True, text=True)
    rss = int(re.search(r"Maximum resident set size \(kbytes\): (\d+)", r.stderr).group(1))
    m = re.search(r"Elapsed \(wall clock\) time \(h:mm:ss or m:ss\): (?:(\d+):)?(\d+):([\d.]+)",
                  r.stderr)
    secs = int(m.group(1) or 0) * 3600 + int(m.group(2)) * 60 + float(m.group(3))
    return rss / 1024, secs


def main():
    dot = TMP / "zk-1x1.png"
    Image.new("RGB", (1, 1), (255, 255, 255)).save(dot)
    fhd, qhd = screen(1920, 1080), screen(2560, 1440)
    pictures = [
        ("models only (1×1)", dot),
        ("1920×1080 at 1×", fhd),
        ("1920×1080 at 2×", upscale(fhd, 2)),
        ("2560×1440 at 1×", qhd),
        ("2560×1440 at 2×", upscale(qhd, 2)),
    ]
    sets = [("fast", os.environ["TESSDATA_FAST"]), ("best", os.environ["TESSDATA_BEST"])]
    print("| Models | Languages | Picture | Peak RAM, MB | Time, s |")
    print("|---|---|---|---|---|")
    for name, td in sets:
        for langs in ("ukr+eng", "eng"):
            for label, p in pictures:
                if langs == "eng" and "models" not in label and "1920×1080 at 2×" not in label:
                    continue
                rss, secs = measure(p, td, langs)
                print(f"| {name} | {langs} | {label} | {rss:.0f} | {secs:.1f} |")


if __name__ == "__main__":
    main()
