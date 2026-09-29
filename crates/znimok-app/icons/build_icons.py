"""Builds the Znimok icon packs from the designer's PNGs in this folder.

    python build_icons.py          # znimok.ico (Windows), and on macOS also Znimok.icns
    python build_icons.py --check  # the packs match the PNGs (CI / before a commit)

Sources (one hand-made PNG per size, never resampled here):
    app-<size>.png      the colour app icon: 16 20 24 30 32 36 40 48 50 60 64 72 80 96 128 256
                        300 512 1024
    menubar-16/32.png   black-on-transparent glyph for the macOS menu bar (a template image:
                        macOS tints it for light and dark bars)

Packs:
    znimok.ico    16 20 24 32 40 48 64 256 — the sizes Windows asks for at 100–200 % (256 as
                  PNG inside, the rest as bitmaps); the exe's icon (build.rs), the MSI.
    Znimok.icns   the iconset macOS wants (16…512 @1x and @2x), built by `iconutil`, so only on
                  a Mac; the result is committed and copied into Znimok.app by the bundle scripts.

Needs Pillow.
"""

import pathlib
import shutil
import subprocess
import sys
import tempfile

from PIL import Image

HERE = pathlib.Path(__file__).resolve().parent
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
# iconset name → source size
ICONSET = {
    "icon_16x16.png": 16,
    "icon_16x16@2x.png": 32,
    "icon_32x32.png": 32,
    "icon_32x32@2x.png": 64,
    "icon_128x128.png": 128,
    "icon_128x128@2x.png": 256,
    "icon_256x256.png": 256,
    "icon_256x256@2x.png": 512,
    "icon_512x512.png": 512,
    "icon_512x512@2x.png": 1024,
}


def src(size: int) -> Image.Image:
    im = Image.open(HERE / f"app-{size}.png").convert("RGBA")
    if im.size != (size, size):
        sys.exit(f"app-{size}.png is {im.size}, expected {size}×{size}")
    return im


def build_ico(path: pathlib.Path) -> None:
    images = [src(s) for s in ICO_SIZES]
    # The biggest first; append_images keeps each size's own pixels (no resampling).
    images[-1].save(
        path,
        format="ICO",
        sizes=[(s, s) for s in ICO_SIZES],
        append_images=images[:-1],
    )


def check_ico(path: pathlib.Path) -> list[str]:
    problems = []
    ico = Image.open(path)
    have = sorted(ico.info.get("sizes", set()))
    if have != sorted((s, s) for s in ICO_SIZES):
        problems.append(f"{path.name}: sizes {have}")
    for s in ICO_SIZES:
        ico.size = (s, s)
        frame = ico.convert("RGBA")
        if frame.tobytes() != src(s).tobytes():
            problems.append(f"{path.name}: {s}×{s} differs from app-{s}.png")
    return problems


def build_icns(path: pathlib.Path) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        iconset = pathlib.Path(tmp) / "Znimok.iconset"
        iconset.mkdir()
        for name, size in ICONSET.items():
            shutil.copyfile(HERE / f"app-{size}.png", iconset / name)
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(path)], check=True)


def main() -> None:
    check = "--check" in sys.argv
    for s in [16, 32]:
        m = Image.open(HERE / f"menubar-{s}.png")
        if m.size != (s, s) or m.mode != "RGBA":
            sys.exit(f"menubar-{s}.png: {m.size} {m.mode}")
    ico = HERE / "znimok.ico"
    if check:
        problems = check_ico(ico) if ico.exists() else ["znimok.ico is missing"]
        if not (HERE / "Znimok.icns").exists():
            problems.append("Znimok.icns is missing (build it on a Mac)")
        if problems:
            sys.exit("\n".join(problems))
        print("icons ok")
        return
    build_ico(ico)
    problems = check_ico(ico)
    if problems:
        sys.exit("\n".join(problems))
    print(f"{ico.name}: {', '.join(str(s) for s in ICO_SIZES)}")
    if sys.platform == "darwin":
        build_icns(HERE / "Znimok.icns")
        print("Znimok.icns")
    else:
        print("Znimok.icns: run this on a Mac (iconutil)")


if __name__ == "__main__":
    main()
