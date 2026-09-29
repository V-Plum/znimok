#!/usr/bin/env python3
"""winget manifests and the Homebrew cask for one release (ZK-81).

    python packaging/stores/make_manifests.py --version 1.0.0 --files out --dest stores

Reads the MSI and the DMG in --files (their SHA-256 goes into the manifests), writes

    <dest>/winget/manifests/v/VPlum/Znimok/<version>/VPlum.Znimok{,.installer,.locale.en-US,.locale.uk-UA}.yaml
    <dest>/homebrew/Casks/znimok.rb

The URLs point at the GitHub release `v<version>`, so the files are valid only once that release
is published. Submitting them is a manual step (docs/RELEASE.md): a PR to microsoft/winget-pkgs
and a commit to the tap V-Plum/homebrew-znimok. Standard library only.
"""

import argparse
import hashlib
import pathlib
import sys

PACKAGE_ID = "VPlum.Znimok"
REPO = "https://github.com/V-Plum/znimok"
# Fixed in packaging/msi/znimok.wxs — never changes between versions.
UPGRADE_CODE = "{7B593159-BA4C-4248-9B58-82E33A5A9DBB}"
MANIFEST_VERSION = "1.10.0"
SCHEMA = "https://aka.ms/winget-manifest.{kind}.1.10.0.schema.json"

DESCRIPTION_EN = (
    "Znimok takes screenshots of the screen, a window or a region, lets you annotate them "
    "(arrows, frames, text, counters, blur), recognises text on the device and hides secrets "
    "before sharing. AI agents can use it through a built-in MCP server, with the person's "
    "permission for every scope."
)
DESCRIPTION_UK = (
    "Znimok знімає екран, вікно чи ділянку, дає позначити знімок (стрілки, рамки, текст, "
    "лічильники, розмиття), розпізнає текст на пристрої й приховує секрети перед надсиланням. "
    "Агенти ШІ можуть користуватися ним через вбудований MCP-сервер — з дозволу людини на "
    "кожну дію."
)


def sha256(p: pathlib.Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def one(files: pathlib.Path, pattern: str) -> pathlib.Path:
    found = sorted(files.glob(pattern))
    if len(found) != 1:
        sys.exit(f"expected one {pattern} in {files}, found {len(found)}")
    return found[0]


def header(kind: str) -> str:
    return f"# yaml-language-server: $schema={SCHEMA.format(kind=kind)}\n"


def q(s: str) -> str:
    """A YAML double-quoted scalar."""
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def winget(version: str, msi: pathlib.Path, dest: pathlib.Path) -> None:
    d = dest / "winget" / "manifests" / "v" / "VPlum" / "Znimok" / version
    d.mkdir(parents=True, exist_ok=True)
    url = f"{REPO}/releases/download/v{version}/{msi.name}"
    common = f"PackageIdentifier: {PACKAGE_ID}\nPackageVersion: {q(version)}\n"
    tail = f"ManifestVersion: {MANIFEST_VERSION}\n"

    (d / f"{PACKAGE_ID}.yaml").write_text(
        header("version")
        + common
        + "DefaultLocale: en-US\n"
        + "ManifestType: version\n"
        + tail,
        encoding="utf-8",
    )
    (d / f"{PACKAGE_ID}.installer.yaml").write_text(
        header("installer")
        + common
        + "MinimumOSVersion: 10.0.19041.0\n"
        + "InstallerType: wix\n"
        + "Scope: user\n"
        + "InstallModes:\n  - interactive\n  - silent\n  - silentWithProgress\n"
        + "UpgradeBehavior: install\n"
        + "Commands:\n  - znimok\n"
        + "FileExtensions:\n  - znimok\n"
        + "AppsAndFeaturesEntries:\n"
        + f"  - UpgradeCode: {q(UPGRADE_CODE)}\n"
        + "Installers:\n"
        + "  - Architecture: x64\n"
        + f"    InstallerUrl: {url}\n"
        + f"    InstallerSha256: {sha256(msi).upper()}\n"
        + "ManifestType: installer\n"
        + tail,
        encoding="utf-8",
    )
    for locale, kind, desc, short, extra in [
        (
            "en-US",
            "defaultLocale",
            DESCRIPTION_EN,
            "Screenshots with annotations, on-device OCR and an MCP server for AI agents",
            "",
        ),
        (
            "uk-UA",
            "locale",
            DESCRIPTION_UK,
            "Знімки екрана з позначками, розпізнаванням тексту й MCP-сервером для агентів ШІ",
            "",
        ),
    ]:
        (d / f"{PACKAGE_ID}.locale.{locale}.yaml").write_text(
            header(kind)
            + common
            + f"PackageLocale: {locale}\n"
            + "Publisher: Vadym Slyva\n"
            + "PublisherUrl: https://github.com/V-Plum\n"
            + f"PublisherSupportUrl: {REPO}/issues\n"
            + "Author: Vadym Slyva\n"
            + "PackageName: Znimok\n"
            + f"PackageUrl: {REPO}\n"
            + "License: Proprietary\n"
            + f"LicenseUrl: {REPO}/blob/v{version}/LICENSE\n"
            + "Copyright: Copyright (c) 2026 Vadym Slyva\n"
            + f"ShortDescription: {q(short)}\n"
            + f"Description: {q(desc)}\n"
            + "Moniker: znimok\n" * (kind == "defaultLocale")
            + "Tags:\n  - screenshot\n  - annotation\n  - ocr\n  - mcp\n"
            + f"ReleaseNotesUrl: {REPO}/releases/tag/v{version}\n"
            + extra
            + f"ManifestType: {kind}\n"
            + tail,
            encoding="utf-8",
        )
    print(f"winget: {d}")


def cask(version: str, dmg: pathlib.Path, dest: pathlib.Path) -> None:
    d = dest / "homebrew" / "Casks"
    d.mkdir(parents=True, exist_ok=True)
    # The DMG name carries the version: the URL is rebuilt from #{version}.
    name = dmg.name.replace(version, "#{version}")
    (d / "znimok.rb").write_text(
        f'''cask "znimok" do
  version "{version}"
  sha256 "{sha256(dmg)}"

  url "{REPO}/releases/download/v#{{version}}/{name}"
  name "Znimok"
  desc "Screenshots with annotations, on-device OCR and an MCP server for AI agents"
  homepage "{REPO}"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on macos: ">= :sequoia"

  app "Znimok.app"
  binary "#{{appdir}}/Znimok.app/Contents/MacOS/znimok"

  zap trash: [
    "~/Library/Application Support/Znimok",
    "~/Library/Caches/Znimok",
    "~/Library/Logs/Znimok",
    "~/Library/Preferences/ua.plum.znimok.app.plist",
  ]
end
''',
        encoding="utf-8",
    )
    print(f"homebrew: {d / 'znimok.rb'}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--version", required=True)
    ap.add_argument("--files", required=True, type=pathlib.Path, help="folder with the .msi and the .dmg")
    ap.add_argument("--dest", required=True, type=pathlib.Path)
    ap.add_argument("--only", choices=["winget", "homebrew"], help="one of the two (a CI job has one file)")
    a = ap.parse_args()
    if a.only != "homebrew":
        winget(a.version, one(a.files, "*.msi"), a.dest)
    if a.only != "winget":
        cask(a.version, one(a.files, "*.dmg"), a.dest)


if __name__ == "__main__":
    main()
