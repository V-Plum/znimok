#!/usr/bin/env python3
"""Runs one OCR engine over the evaluation set and writes {picture: recognised text} as JSON
(ZK-120). Each engine is imported only when asked, so a runner needs only its own.

    python tools/ocr-eval/run_engine.py tesseract out/tesseract.json
    python tools/ocr-eval/run_engine.py tesseract-2x out/tesseract-2x.json
    python tools/ocr-eval/run_engine.py easyocr out/easyocr.json
    python tools/ocr-eval/run_engine.py paddle out/paddle.json
"""

import json
import subprocess
import sys
import tempfile
from pathlib import Path

SET = Path(__file__).resolve().parent / "set"


def pictures():
    return sorted(SET.glob("*.png"))


def tesseract(scale: int):
    from PIL import Image

    out = {}
    for p in pictures():
        src = p
        if scale != 1:
            img = Image.open(p)
            img = img.resize((img.width * scale, img.height * scale), Image.LANCZOS)
            src = Path(tempfile.gettempdir()) / f"zk-ocr-{p.name}"
            img.save(src)
        r = subprocess.run(["tesseract", str(src), "stdout", "-l", "ukr+eng", "--psm", "6"],
                           capture_output=True, text=True, encoding="utf-8")
        out[p.name] = r.stdout
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

    ocr = PaddleOCR(lang="uk", use_doc_orientation_classify=False, use_doc_unwarping=False,
                    use_textline_orientation=False)
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
    runs = {
        "tesseract": lambda: tesseract(1),
        "tesseract-2x": lambda: tesseract(2),
        "easyocr": easyocr_run,
        "paddle": paddle,
    }
    result = runs[engine]()
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(json.dumps(result, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"{engine}: {len(result)} pictures → {dest}")


if __name__ == "__main__":
    main()
