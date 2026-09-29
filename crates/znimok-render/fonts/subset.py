"""Bundled fonts of Znimok (ZK-34): download the originals from github.com/google/fonts and cut
them down to the scripts Znimok shows (Latin with extensions, Cyrillic with extensions, the
punctuation, arrows and signs of the UI). Variable axes and OpenType features stay.

    python crates/znimok-render/fonts/subset.py            # downloads into fonts/src, writes the *.ttf next to this script

Needs `pip install fonttools`. The originals in src/ are not committed; the subsets and the OFL
texts are. All three fonts are SIL Open Font License 1.1.
"""

import pathlib
import subprocess
import sys
import urllib.request

HERE = pathlib.Path(__file__).resolve().parent
SRC = HERE / "src"
BASE = "https://raw.githubusercontent.com/google/fonts/main/ofl/"

FONTS = {
    # file name: (path in google/fonts, role)
    "Onest.ttf": ("onest/Onest%5Bwght%5D.ttf", "UI and text marks"),
    "JetBrainsMono.ttf": ("jetbrainsmono/JetBrainsMono%5Bwght%5D.ttf", "numbers and key combinations"),
    "Unbounded.ttf": ("unbounded/Unbounded%5Bwght%5D.ttf", "the Znimok wordmark"),
}
LICENSES = {
    "Onest-OFL.txt": "onest/OFL.txt",
    "JetBrainsMono-OFL.txt": "jetbrainsmono/OFL.txt",
    "Unbounded-OFL.txt": "unbounded/OFL.txt",
}

UNICODES = ",".join([
    "U+0000-024F",  # Basic Latin, Latin-1, Latin Extended-A and -B
    "U+0300-036F",  # combining marks
    "U+0400-052F",  # Cyrillic and Cyrillic Supplement (Ukrainian ґ є і ї are here)
    "U+1E00-1EFF",  # Latin Extended Additional
    "U+2000-206F",  # general punctuation: dashes, quotes, ellipsis, spaces
    "U+20A0-20CF",  # currency (₴ € …)
    "U+2100-214F",  # letterlike (№ ™)
    "U+2190-21FF",  # arrows
    "U+2200-22FF",  # math (− × ≈ ≠)
    "U+2300-23FF",  # technical (⌘ ⌥ ⌃ ⏎)
    "U+25A0-25FF",  # geometric shapes
    "U+2713-2717",  # check marks
])


def fetch(path: str, to: pathlib.Path) -> None:
    if to.exists():
        return
    print("download", path)
    with urllib.request.urlopen(BASE + path) as r:
        to.write_bytes(r.read())


def main() -> int:
    SRC.mkdir(exist_ok=True)
    for name, (path, _) in FONTS.items():
        fetch(path, SRC / name)
    for name, path in LICENSES.items():
        fetch(path, HERE / name)
    for name in FONTS:
        out = HERE / name
        # The wordmark face only ever writes the name: keep just its letters.
        chars = (
            "--text=Znimok ЗНІМОКзнімок" if name == "Unbounded.ttf" else f"--unicodes={UNICODES}"
        )
        subprocess.run(
            [
                sys.executable, "-m", "fontTools.subset", str(SRC / name),
                chars,
                "--layout-features=*",
                "--name-IDs=*",
                "--notdef-outline",
                f"--output-file={out}",
            ],
            check=True,
        )
        print(f"{name}: {(SRC / name).stat().st_size} -> {out.stat().st_size} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
