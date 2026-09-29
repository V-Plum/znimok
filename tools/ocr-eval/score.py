#!/usr/bin/env python3
"""Scores OCR results against the set (ZK-120): character error rate overall and on Ukrainian
text, and how many of the letters і ї є ґ (and the apostrophe) survive.

    python tools/ocr-eval/score.py out/*.json          # a Markdown table on stdout
"""

import json
import sys
from collections import Counter
from pathlib import Path

SET = Path(__file__).resolve().parent / "set"
SPECIAL = set("іїєґІЇЄҐ'")


def norm(s: str) -> str:
    s = s.replace("’", "'").replace("ʼ", "'").replace("`", "'")
    s = s.replace("–", "-").replace("—", "-")
    return " ".join(s.split())


def lev(a: str, b: str) -> int:
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]


def score(results: dict) -> dict:
    errs = chars = 0
    uk_errs = uk_chars = 0
    sp_ref = sp_hit = 0
    worst = []
    lat_ref = lat_hit = 0
    for png in sorted(SET.glob("*.png")):
        ref = norm((SET / (png.stem + ".txt")).read_text(encoding="utf-8"))
        hyp = norm(results.get(png.name, ""))
        e = lev(ref, hyp)
        errs += e
        chars += len(ref)
        if any("Ѐ" <= c <= "ӿ" for c in ref):
            uk_errs += e
            uk_chars += len(ref)
        rc, hc = Counter(c for c in ref if c in SPECIAL), Counter(c for c in hyp if c in SPECIAL)
        sp_ref += sum(rc.values())
        sp_hit += sum(min(v, hc[k]) for k, v in rc.items())
        # Latin tokens (e-mail, paths, English words) exactly right: what the masking of
        # secrets sees.
        lt = Counter(t for t in ref.split() if any(c.isascii() and c.isalpha() for c in t))
        ht = Counter(hyp.split())
        lat_ref += sum(lt.values())
        lat_hit += sum(min(v, ht[k]) for k, v in lt.items())
        worst.append((e / max(1, len(ref)), png.name, ref, hyp))
    worst.sort(reverse=True)
    return {
        "cer": errs / max(1, chars),
        "cer_ukrainian": uk_errs / max(1, uk_chars),
        "special_recall": sp_hit / max(1, sp_ref),
        "latin_exact": lat_hit / max(1, lat_ref),
        "worst": worst[:3],
    }


def main():
    rows = []
    for f in sys.argv[1:]:
        s = score(json.loads(Path(f).read_text(encoding="utf-8")))
        rows.append((Path(f).stem, s))
    rows.sort(key=lambda r: r[1]["cer"])
    print("| Engine | CER, all | CER, Ukrainian text | і ї є ґ ' found | Latin words exact |")
    print("|---|---|---|---|---|")
    for name, s in rows:
        print(f"| {name} | {s['cer']:.1%} | {s['cer_ukrainian']:.1%} | {s['special_recall']:.1%} "
              f"| {s['latin_exact']:.1%} |")
    print()
    for name, s in rows:
        print(f"**{name}** — worst:")
        for rate, pic, ref, hyp in s["worst"]:
            print(f"- `{pic}` {rate:.0%}: «{ref}» → «{hyp}»")
        print()


if __name__ == "__main__":
    main()
