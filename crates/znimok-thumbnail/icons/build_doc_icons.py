"""Document icons of .znimok files (ZK-150): a screenshot, a video, a video with a DevTools log.

    python build_doc_icons.py --from DIR   # cut every size from the designer's 1024 px originals
    python build_doc_icons.py              # build the packs from the per-size PNGs here
    python build_doc_icons.py --check      # the packs match the PNGs (before a commit)

Originals (the owner's, `C:\\AIHome\\tools\\znimok_icons\\Files`): zK_shot.png, zK_video.png,
zK_code.png, 1024 px. `--from` squares them (two are 1024×1025: the whole picture is resampled,
nothing cut off) and writes doc-<kind>-<size>.png with a Lanczos filter; any of them may later be
replaced by a hand-drawn size and is then used as is — the packs never resample.

Packs:
    doc-image.ico, doc-video.ico, doc-report.ico
        16 20 24 32 40 48 64 96 256 — what Explorer asks for at 100–200 % in every view; the
        thumbnail DLL's icon handler hands one of them out per file (by the kind in the file).
    ZnimokDocument.icns
        macOS has one icon per type, so the screenshot one; a video shows its own thumbnail.
        Built by `iconutil` (the full iconset, 16…512 @1x and @2x), so only on a Mac; elsewhere
        the committed file is kept and not checked.

Separate from crates/znimok-app/icons (the app icon, another folder and script). Needs Pillow.
"""

import io
import pathlib
import shutil
import subprocess
import sys
import tempfile

from PIL import Image

HERE = pathlib.Path(__file__).resolve().parent
KINDS = {"image": "zK_shot.png", "video": "zK_video.png", "report": "zK_code.png"}
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 256]
ICNS_SIZES = [16, 32, 64, 128, 256, 512, 1024]
ALL_SIZES = sorted(set(ICO_SIZES) | set(ICNS_SIZES))


def cut(originals: pathlib.Path) -> None:
    for kind, name in KINDS.items():
        im = Image.open(originals / name).convert("RGBA")
        if im.size != (1024, 1024):
            im = im.resize((1024, 1024), Image.LANCZOS)
        for s in ALL_SIZES:
            (im if s == 1024 else im.resize((s, s), Image.LANCZOS)).save(
                HERE / f"doc-{kind}-{s}.png", optimize=True
            )
    print(f"cut {len(KINDS)} x {len(ALL_SIZES)} sizes from {originals}")


def src(kind: str, size: int) -> Image.Image:
    im = Image.open(HERE / f"doc-{kind}-{size}.png").convert("RGBA")
    if im.size != (size, size):
        sys.exit(f"doc-{kind}-{size}.png is {im.size}, expected {size}x{size}")
    return im


def ico_bytes(kind: str) -> bytes:
    images = [src(kind, s) for s in ICO_SIZES]
    out = io.BytesIO()
    # The biggest first; append_images keeps each size's own pixels (no resampling).
    images[-1].save(
        out,
        format="ICO",
        sizes=[(s, s) for s in ICO_SIZES],
        append_images=images[:-1],
        bitmap_format="bmp",
    )
    return out.getvalue()


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


def icns_bytes() -> bytes | None:
    if sys.platform != "darwin" or not shutil.which("iconutil"):
        return None
    with tempfile.TemporaryDirectory() as t:
        iconset = pathlib.Path(t) / "ZnimokDocument.iconset"
        iconset.mkdir()
        for name, size in ICONSET.items():
            shutil.copyfile(HERE / f"doc-image-{size}.png", iconset / name)
        out = pathlib.Path(t) / "ZnimokDocument.icns"
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(out)], check=True)
        return out.read_bytes()


def packs() -> dict[str, bytes]:
    p = {f"doc-{k}.ico": ico_bytes(k) for k in KINDS}
    icns = icns_bytes()
    if icns is not None:
        p["ZnimokDocument.icns"] = icns
    return p


def main() -> None:
    if "--from" in sys.argv:
        cut(pathlib.Path(sys.argv[sys.argv.index("--from") + 1]))
    built = packs()
    if "--check" in sys.argv:
        stale = [n for n, b in built.items() if not (HERE / n).is_file() or (HERE / n).read_bytes() != b]
        if stale:
            sys.exit(f"out of date: {', '.join(stale)} — run build_doc_icons.py")
        print("packs match the PNGs")
        return
    for name, b in built.items():
        (HERE / name).write_bytes(b)
        print(f"{name}: {len(b)} bytes")


if __name__ == "__main__":
    main()
