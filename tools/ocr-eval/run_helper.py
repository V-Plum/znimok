#!/usr/bin/env python3
"""Runs znimok-ocr.exe (tools/ocr-helper) over the evaluation set through its stdin protocol and
reports its peak memory (ZK-120). Windows only.

    python tools/ocr-eval/run_helper.py target/ocr-helper/znimok-ocr.exe out/znimok-ocr.json [--mode 0|1]
    python tools/ocr-eval/run_helper.py target/ocr-helper/znimok-ocr.exe --screens

The JSON is scored by score.py like the other engines. --screens feeds screen-sized pictures
(memory.py's) and prints the peak working set and the time per picture.
"""

import ctypes
import json
import struct
import subprocess
import sys
import time
from ctypes import wintypes
from pathlib import Path

from PIL import Image

SET = Path(__file__).resolve().parent / "set"


class Counters(ctypes.Structure):
    _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD),
                ("PeakWorkingSetSize", ctypes.c_size_t), ("WorkingSetSize", ctypes.c_size_t),
                ("QuotaPeakPagedPoolUsage", ctypes.c_size_t), ("QuotaPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t), ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                ("PagefileUsage", ctypes.c_size_t), ("PeakPagefileUsage", ctypes.c_size_t)]


def peak_mb(proc: subprocess.Popen) -> float:
    c = Counters()
    c.cb = ctypes.sizeof(c)
    ctypes.windll.psapi.GetProcessMemoryInfo(wintypes.HANDLE(proc._handle), ctypes.byref(c), c.cb)
    return c.PeakWorkingSetSize / 2**20


class Helper:
    def __init__(self, exe: str):
        self.p = subprocess.Popen([exe, "--idle", "30"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL)

    def recognize(self, img: Image.Image, mode: int = 0) -> dict:
        g = img.convert("L")
        self.p.stdin.write(b"ZOCR" + struct.pack("<IIII", 1, mode, g.width, g.height) + g.tobytes())
        self.p.stdin.flush()
        n = struct.unpack("<I", self.p.stdout.read(4))[0]
        return json.loads(self.p.stdout.read(n).decode("utf-8"))

    def close(self) -> float:
        peak = peak_mb(self.p)
        self.p.stdin.write(b"ZOCR" + struct.pack("<IIII", 1, 2, 0, 0))
        self.p.stdin.close()
        self.p.wait(10)
        return peak


def main():
    exe = sys.argv[1]
    if "--screens" in sys.argv:
        sys.path.insert(0, str(Path(__file__).resolve().parent))
        from memory import screen
        print("| Picture | Mode | Peak RAM, MB | Time, s | Lines |")
        print("|---|---|---|---|---|")
        for w, h in ((1920, 1080), (2560, 1440)):
            for mode in (0, 1):
                hp = Helper(exe)
                t = time.perf_counter()
                r = hp.recognize(Image.open(screen(w, h)), mode)
                secs = time.perf_counter() - t
                print(f"| {w}×{h} | {mode} | {hp.close():.0f} | {secs:.1f} | {len(r.get('lines', []))} |")
        return
    dest = Path(sys.argv[2])
    mode = int(sys.argv[sys.argv.index("--mode") + 1]) if "--mode" in sys.argv else 0
    hp = Helper(exe)
    out = {}
    t = time.perf_counter()
    for p in sorted(SET.glob("*.png")):
        r = hp.recognize(Image.open(p), mode)
        if "error" in r:
            sys.exit(f"{p.name}: {r['error']}")
        out[p.name] = "\n".join(line["text"] for line in r["lines"])
    secs = time.perf_counter() - t
    peak = hp.close()
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(json.dumps(out, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"znimok-ocr mode {mode}: {len(out)} pictures in {secs:.1f} s, peak {peak:.0f} MB → {dest}")


if __name__ == "__main__":
    main()
