#!/usr/bin/env python3
"""Builds znimok-ocr.exe (ZK-120): Leptonica and Tesseract from pinned sources, static, with the
static CRT and nothing Znimok does not need (no image-file codecs, no network, no archives, no
training tools, no legacy engine), then the helper, and puts the models next to it.

    python tools/ocr-helper/build.py                 # → target/ocr-helper/{znimok-ocr.exe, tessdata/, NOTICE.txt}
    python tools/ocr-helper/build.py --out DIR --work DIR

Windows: finds the MSVC environment itself (vswhere → vcvars64.bat); CMake and Ninja from PATH or
from Visual Studio. Every download is checked against its SHA-256. Standard library only.
"""

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent

SOURCES = {
    "leptonica": (
        "https://github.com/DanBloomberg/leptonica/releases/download/1.87.0/leptonica-1.87.0.tar.gz",
        "c73363397f96eb1295602bf44d708a994ad42046c791bf03ea0505d829bdb6a7",
        "leptonica-1.87.0",
    ),
    "tesseract": (
        "https://github.com/tesseract-ocr/tesseract/archive/refs/tags/5.5.3.tar.gz",
        "9218e62793116d42a9f6d14cd9348518b27f382096eea3d0f2d1a24616bb5884",
        "tesseract-5.5.3",
    ),
}
# tessdata_best 4.1.0 (Apache-2.0): chosen by the evaluation (tools/ocr-eval, ZK-120).
MODELS = {
    "ukr": "1277f6e3b6f707063a92d40e7678e7f57154e8414e328e340be9ee9275eea9c8",
    "eng": "8280aed0782fe27257a68ea10fe7ef324ca0f8d85bd2fd145d1c2b560bcb66ba",
}
MODEL_URL = "https://github.com/tesseract-ocr/tessdata_best/raw/4.1.0/{}.traineddata"
# What the conversion must produce: byte-identical to `lstmtraining --convert_to_int` (checked
# against Tesseract 5.5.1 on macOS, 29.09.2026); a different result = not reproducible.
INT_MODELS = {
    "ukr": "ebc2449655d7084fa57a9edd486f4e31fcb1c90b51accb29827f66571631a0fa",
    "eng": "651409a6c81603f8565bceb773e3d34f1a77d3fa4e42c6a8458db04810f3bfb2",
}

COMMON = [
    "-DCMAKE_BUILD_TYPE=Release",
    "-DBUILD_SHARED_LIBS=OFF",
    "-DCMAKE_POLICY_DEFAULT_CMP0091=NEW",
    "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded",
    "-DSW_BUILD=OFF",
]
LEPTONICA = [
    "-DBUILD_PROG=OFF",
    *(f"-DENABLE_{x}=OFF" for x in ("ZLIB", "PNG", "GIF", "JPEG", "TIFF", "WEBP", "OPENJPEG")),
]
TESSERACT = [
    "-DBUILD_TRAINING_TOOLS=OFF",
    "-DBUILD_TESTS=OFF",
    "-DDISABLE_ARCHIVE=ON",
    "-DDISABLE_CURL=ON",
    "-DDISABLE_TIFF=ON",
    "-DGRAPHICS_DISABLED=ON",
    "-DDISABLED_LEGACY_ENGINE=ON",
    "-DOPENMP_BUILD=OFF",
    "-DENABLE_CCACHE=OFF",
    "-DINSTALL_CONFIGS=OFF",
]


def fetch(url: str, sha: str, dest: Path) -> Path:
    if not dest.exists() or hashlib.sha256(dest.read_bytes()).hexdigest() != sha:
        print(f"download {url}")
        dest.parent.mkdir(parents=True, exist_ok=True)
        with urllib.request.urlopen(url) as r:
            dest.write_bytes(r.read())
    got = hashlib.sha256(dest.read_bytes()).hexdigest()
    if got != sha:
        sys.exit(f"{dest.name}: sha256 {got}, expected {sha}")
    return dest


def msvc_env() -> dict:
    """The environment of vcvars64.bat (Windows), so cl/link/rc work without a developer prompt."""
    if os.name != "nt" or shutil.which("cl"):
        return dict(os.environ)
    vswhere = Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / \
        "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    vs = subprocess.run([str(vswhere), "-latest", "-products", "*", "-requires",
                         "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property",
                         "installationPath"], capture_output=True, text=True, check=True).stdout.strip()
    bat = Path(vs) / "VC" / "Auxiliary" / "Build" / "vcvars64.bat"
    out = subprocess.run(f'"{bat}" >nul && set', shell=True, capture_output=True, text=True,
                         check=True).stdout
    # Windows variable names are case-insensitive; a plain dict is not ("Path" vs "PATH").
    env = {k.upper(): v for k, v in (line.split("=", 1) for line in out.splitlines() if "=" in line)}
    # CMake and Ninja that come with Visual Studio, when they are not on PATH.
    extra = Path(vs) / "Common7" / "IDE" / "CommonExtensions" / "Microsoft" / "CMake"
    env["PATH"] = os.pathsep.join([env.get("PATH", ""),
                                   str(extra / "CMake" / "bin"), str(extra / "Ninja")])
    return env


def run(cmd: list, env: dict, cwd: Path | None = None) -> None:
    print("+", " ".join(str(c) for c in cmd), flush=True)
    # CreateProcess looks programs up on the parent's PATH, not on `env`'s.
    exe = shutil.which(str(cmd[0]), path=env.get("PATH")) or str(cmd[0])
    subprocess.run([exe, *(str(c) for c in cmd[1:])], env=env, cwd=cwd, check=True)


def cmake_project(src: Path, build: Path, prefix: Path, args: list, env: dict) -> None:
    gen = ["-G", "Ninja"] if shutil.which("ninja", path=env.get("PATH")) else []
    run(["cmake", "-S", src, "-B", build, *gen, f"-DCMAKE_INSTALL_PREFIX={prefix}",
         f"-DCMAKE_PREFIX_PATH={prefix}", *COMMON, *args], env)
    run(["cmake", "--build", build, "--config", "Release", "--parallel"], env)
    run(["cmake", "--install", build, "--config", "Release"], env)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", type=Path, default=ROOT / "target" / "ocr-helper")
    ap.add_argument("--work", type=Path, default=ROOT / "target" / "ocr-helper-work")
    a = ap.parse_args()
    work, out = a.work.resolve(), a.out.resolve()
    prefix = work / "prefix"
    env = msvc_env()

    for name, (url, sha, top) in SOURCES.items():
        tgz = fetch(url, sha, work / "dl" / Path(url).name.replace(".tar.gz", f"-{name}.tar.gz"))
        if not (work / top).exists():
            with tarfile.open(tgz) as t:
                t.extractall(work, filter="data")
    lept_top, tess_top = SOURCES["leptonica"][2], SOURCES["tesseract"][2]
    if not (prefix / "lib" / "cmake" / "leptonica").exists():
        cmake_project(work / lept_top, work / "build-leptonica", prefix, LEPTONICA, env)
    if not (prefix / "lib" / "cmake" / "tesseract").exists():
        cmake_project(work / tess_top, work / "build-tesseract", prefix,
                      [*TESSERACT, f"-DLeptonica_DIR={prefix / 'lib' / 'cmake' / 'leptonica'}"], env)
    cmake_project(HERE, work / "build-helper", out,
                  [f"-DCMAKE_PREFIX_PATH={prefix}",
                   f"-DZNIMOK_TESS_SRC={work / tess_top}",
                   f"-DZNIMOK_TESS_BUILD={work / 'build-tesseract'}",
                   f"-DTesseract_DIR={prefix / 'lib' / 'cmake' / 'tesseract'}",
                   f"-DLeptonica_DIR={prefix / 'lib' / 'cmake' / 'leptonica'}"], env)

    # Integer models from tessdata_best: half the memory and time at nearly the same accuracy.
    convert = work / "build-helper" / ("znimok-ocr-convert.exe" if os.name == "nt" else "znimok-ocr-convert")
    (out / "tessdata").mkdir(parents=True, exist_ok=True)
    for lang, sha in MODELS.items():
        src = fetch(MODEL_URL.format(lang), sha, work / "models" / f"{lang}.traineddata")
        dst = out / "tessdata" / f"{lang}.traineddata"
        run([convert, src, dst], env)
        got = hashlib.sha256(dst.read_bytes()).hexdigest()
        want = INT_MODELS[lang]
        if want and got != want:
            sys.exit(f"{lang}: integer model sha256 {got}, expected {want} (conversion not reproducible)")
        print(f"{lang}: {dst.stat().st_size} bytes, sha256 {got}")
    notice = [
        "znimok-ocr.exe contains Tesseract 5.5.3 and Leptonica 1.87.0; tessdata/ holds",
        "tessdata_best 4.1.0 models converted to integer weights. Their licenses follow.\n",
        "=" * 70, "Tesseract (Apache License 2.0)", "=" * 70,
        (work / tess_top / "LICENSE").read_text(encoding="utf-8", errors="replace"),
        "=" * 70, "Leptonica (BSD 2-clause)", "=" * 70,
        (work / lept_top / "leptonica-license.txt").read_text(encoding="utf-8", errors="replace"),
        "=" * 70, "tessdata_best models: Apache License 2.0, as Tesseract above.",
    ]
    (out / "NOTICE.txt").write_text("\n".join(notice), encoding="utf-8")
    print(f"done: {out}")


if __name__ == "__main__":
    main()
