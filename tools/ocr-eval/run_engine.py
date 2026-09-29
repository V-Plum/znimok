#!/usr/bin/env python3
"""Runs one OCR engine over the evaluation set and writes {picture: recognised text} as JSON
(ZK-120). Each engine is imported only when asked, so a runner needs only its own.

    python tools/ocr-eval/run_engine.py tesseract out/tesseract.json
    python tools/ocr-eval/run_engine.py tesseract-2x out/tesseract-2x.json
    python tools/ocr-eval/run_engine.py easyocr out/easyocr.json
    python tools/ocr-eval/run_engine.py paddle out/paddle.json

Model variants (ZK-120, step 2): `tesseract-{fast,best}-2x` read the models from
$TESSDATA_FAST / $TESSDATA_BEST; `...-dual` adds a second English-only pass and keeps, word by
word, the English reading where the Ukrainian+English one is visibly wrong (mixed scripts,
e-mail, paths, code) — the way the masking of secrets would use it.
"""

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

SET = Path(__file__).resolve().parent / "set"


def pictures():
    return sorted(SET.glob("*.png"))


def scaled(p: Path, scale: int) -> Path:
    if scale == 1:
        return p
    from PIL import Image

    img = Image.open(p)
    img = img.resize((img.width * scale, img.height * scale), Image.LANCZOS)
    src = Path(tempfile.gettempdir()) / f"zk-ocr-{scale}-{p.name}"
    img.save(src)
    return src


def tess_args(tessdata: str | None) -> list[str]:
    return ["--tessdata-dir", tessdata] if tessdata else []


def tesseract(scale: int, tessdata: str | None = None):
    out = {}
    for p in pictures():
        r = subprocess.run(["tesseract", str(scaled(p, scale)), "stdout", *tess_args(tessdata),
                            "-l", "ukr+eng", "--psm", "6"],
                           capture_output=True, text=True, encoding="utf-8")
        out[p.name] = r.stdout
    return out


def tsv_words(src: Path, langs: str, tessdata: str | None):
    """Words with boxes and confidence: [(line key, (x0, y0, x1, y1), conf, text)]."""
    r = subprocess.run(["tesseract", str(src), "stdout", *tess_args(tessdata), "-l", langs,
                        "--psm", "6", "-c", "tessedit_create_tsv=1"],
                       capture_output=True, text=True, encoding="utf-8")
    words = []
    for row in r.stdout.splitlines()[1:]:
        f = row.split("\t")
        if len(f) < 12 or f[0] != "5" or not f[11].strip():
            continue
        x, y, w, h = map(int, f[6:10])
        words.append(((f[2], f[3], f[4]), (x, y, x + w, y + h), float(f[10]), f[11]))
    return words


def is_cyr(c: str) -> bool:
    return "\u0400" <= c <= "\u04ff"


def is_lat(c: str) -> bool:
    return c.isascii() and c.isalpha()


def wants_english(ua: str, ua_conf: float, en: str, en_conf: float) -> bool:
    """The Ukrainian+English reading is visibly wrong and the English one is not worse."""
    if not en:
        return False
    mixed = any(map(is_cyr, ua)) and any(map(is_lat, ua))
    technical = any(ch in en for ch in "@/\\_=") or "." in en.strip(".,;:!?")
    if mixed or technical:
        return en_conf >= ua_conf - 15
    # A Latin word read as Cyrillic look-alikes: English is clearly surer.
    return en_conf >= ua_conf + 12


def iou(a, b) -> float:
    ix = max(0, min(a[2], b[2]) - max(a[0], b[0]))
    iy = max(0, min(a[3], b[3]) - max(a[1], b[1]))
    inter = ix * iy
    union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter
    return inter / union if union else 0.0


def dual(scale: int, tessdata: str | None = None, merge: bool = True, langs: str = "ukr+eng"):
    out = {}
    for p in pictures():
        src = scaled(p, scale)
        ua = tsv_words(src, langs, tessdata)
        en = tsv_words(src, "eng", tessdata) if merge else []
        lines: dict = {}
        for key, box, conf, text in ua:
            best = max(en, key=lambda w: iou(box, w[1]), default=None)
            if best and iou(box, best[1]) > 0.5 and wants_english(text, conf, best[3], best[2]):
                text = best[3]
            lines.setdefault(key, []).append(text)
        out[p.name] = "\n".join(" ".join(ws) for ws in lines.values())
    return out


def easyocr_run():
    import easyocr

    reader = easyocr.Reader(["uk", "en"], gpu=False, verbose=False)
    out = {}
    for p in pictures():
        res = reader.readtext(str(p), detail=1, paragraph=False)
        # Lines top to bottom, words left to right.
        res.sort(key=lambda r: (round(min(pt[1] for pt in r[0]) / 10), min(pt[0] for pt in r[0])))
        lines, last_y = [], None
        for box, text, _ in res:
            y = round(min(pt[1] for pt in box) / 10)
            if last_y is not None and y == last_y:
                lines[-1] += " " + text
            else:
                lines.append(text)
            last_y = y
        out[p.name] = "\n".join(lines)
    return out


def paddle():
    from paddleocr import PaddleOCR

    # oneDNN in PaddlePaddle 3.x fails on CPU (ConvertPirAttribute2RuntimeAttribute): off.
    ocr = PaddleOCR(lang="uk", use_doc_orientation_classify=False, use_doc_unwarping=False,
                    use_textline_orientation=False, enable_mkldnn=False, device="cpu")
    out = {}
    for p in pictures():
        res = ocr.predict(str(p))
        texts = []
        for page in res:
            texts.extend(page.get("rec_texts", []))
        out[p.name] = "\n".join(texts)
    return out


def main():
    engine, dest = sys.argv[1], Path(sys.argv[2])
    fast, best = os.environ.get("TESSDATA_FAST"), os.environ.get("TESSDATA_BEST")
    runs = {
        "tesseract": lambda: tesseract(1),
        "tesseract-2x": lambda: tesseract(2),
        "tesseract-2x-dual": lambda: dual(2),
        "tesseract-fast-2x": lambda: tesseract(2, fast),
        "tesseract-fast-2x-dual": lambda: dual(2, fast),
        "tesseract-best-2x": lambda: tesseract(2, best),
        "tesseract-best-2x-dual": lambda: dual(2, best),
        # Controls: the word list without merging (does the TSV layout alone change the score?)
        # and English as the first language.
        "tesseract-best-2x-words": lambda: dual(2, best, merge=False),
        "tesseract-best-2x-engfirst": lambda: dual(2, best, merge=False, langs="eng+ukr"),
        "easyocr": easyocr_run,
        "paddle": paddle,
    }
    result = runs[engine]()
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(json.dumps(result, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"{engine}: {len(result)} pictures → {dest}")


if __name__ == "__main__":
    main()
