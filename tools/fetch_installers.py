"""Puts the latest Znimok installers (Windows .msi, macOS .dmg) into a folder for the owner.

    python tools/fetch_installers.py dev       # the newest successful build of main
    python tools/fetch_installers.py dev --build   # no build of the current main yet: start one and wait
    python tools/fetch_installers.py release   # the newest published GitHub release

Layout (default root C:/AIHome/builds/znimok-installers, or --dest):

    dev/windows/Znimok-<version>-windows-x64.msi
    dev/macos/Znimok-<version>-macos-arm64.dmg
    dev/VERSION.txt            commit, date, CI run, SHA-256
    release/windows/ ...       the same for the published release
    release/macos/ ...
    release/VERSION.txt

The installers come from the `release` workflow (the same packaging as a real release: the MSI is
installed, checked and uninstalled on the runner, the DMG verified). Older files in a folder are
replaced, so each folder always holds exactly one installer per OS. Needs `gh` logged in.
"""

import argparse
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time

REPO = "V-Plum/znimok"
WORKFLOW = "release.yml"
DEFAULT_DEST = pathlib.Path("C:/AIHome/builds/znimok-installers")


def run(*args, capture=True):
    r = subprocess.run(args, capture_output=capture, text=True, encoding="utf-8")
    if r.returncode != 0:
        sys.exit(f"{' '.join(args)} failed:\n{r.stderr if capture else ''}")
    return r.stdout if capture else ""


def gh_json(*args):
    return json.loads(run("gh", *args))


def sha256(p: pathlib.Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main_head() -> str:
    return gh_json("api", f"repos/{REPO}/commits/main")["sha"]


def runs():
    return gh_json(
        "run", "list", "-R", REPO, "--workflow", WORKFLOW, "-L", "40",
        "--json", "databaseId,headSha,status,conclusion,event,createdAt,url",
    )


def find_or_build(build: bool):
    head = main_head()
    ok = [r for r in runs() if r["conclusion"] == "success"]
    mine = [r for r in ok if r["headSha"] == head]
    if mine:
        return mine[0], head
    if not build:
        if not ok:
            sys.exit("no successful build of the release workflow yet; run with --build")
        print(f"note: main is at {head[:7]}, the newest successful build is {ok[0]['headSha'][:7]} "
              f"(use --build for the current main)")
        return ok[0], head
    before = {r["databaseId"] for r in runs()}
    run("gh", "workflow", "run", WORKFLOW, "-R", REPO, "--ref", "main")
    print("started the release workflow on main; waiting for it to appear…")
    new = None
    for _ in range(60):
        time.sleep(5)
        new = next((r for r in runs() if r["databaseId"] not in before
                    and r["event"] == "workflow_dispatch"), None)
        if new:
            break
    if not new:
        sys.exit("the dispatched run did not appear")
    print(f"building {new['url']} (about 15–25 minutes)")
    run("gh", "run", "watch", str(new["databaseId"]), "-R", REPO, "--exit-status",
        "--interval", "30", capture=False)
    done = next(r for r in runs() if r["databaseId"] == new["databaseId"])
    return done, head


def place(files, dest: pathlib.Path, info: list[str]):
    """One installer per OS folder: remove the old ones, copy the new, write VERSION.txt."""
    for sub, pattern in (("windows", "*.msi"), ("macos", "*.dmg")):
        d = dest / sub
        d.mkdir(parents=True, exist_ok=True)
        src = [f for f in files if f.match(pattern)]
        if not src:
            print(f"warning: no {pattern} in the downloaded files; {d} left as it was")
            continue
        for old in d.glob(pattern):
            old.unlink()
        for f in src:
            shutil.copy2(f, d / f.name)
            info.append(f"{sub}/{f.name}  sha256 {sha256(d / f.name)}")
            print(f"→ {d / f.name}")
    (dest / "VERSION.txt").write_text("\n".join(info) + "\n", encoding="utf-8")


def dev(args):
    r, head = find_or_build(args.build)
    with tempfile.TemporaryDirectory() as tmp:
        t = pathlib.Path(tmp)
        for name in ("windows", "macos"):
            run("gh", "run", "download", str(r["databaseId"]), "-R", REPO, "-n", name, "-D", str(t / name))
        files = list(t.rglob("*.msi")) + list(t.rglob("*.dmg"))
        info = [
            "Znimok — робоча (dev) збірка / working build",
            f"commit {r['headSha']}" + ("" if r["headSha"] == head else f"  (main is at {head[:7]})"),
            f"built {r['createdAt']}",
            f"CI {r['url']}",
            "",
        ]
        place(files, args.dest / "dev", info)


def release(args):
    rels = gh_json("release", "list", "-R", REPO, "--exclude-drafts", "-L", "1",
                   "--json", "tagName,publishedAt,isPrerelease")
    if not rels:
        for sub in ("windows", "macos"):
            (args.dest / "release" / sub).mkdir(parents=True, exist_ok=True)
        (args.dest / "release" / "VERSION.txt").write_text(
            "Релізів ще немає. / No release yet.\n", encoding="utf-8")
        print("no published release yet; release/ is empty")
        return
    tag = rels[0]["tagName"]
    with tempfile.TemporaryDirectory() as tmp:
        t = pathlib.Path(tmp)
        run("gh", "release", "download", tag, "-R", REPO, "-D", str(t),
            "-p", "*.msi", "-p", "*.dmg", "-p", "SHA256SUMS")
        sums = {}
        for line in (t / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
            h, _, name = line.partition("  ")
            sums[name.strip()] = h
        files = list(t.glob("*.msi")) + list(t.glob("*.dmg"))
        for f in files:
            if sums.get(f.name) != sha256(f):
                sys.exit(f"{f.name}: checksum does not match SHA256SUMS")
        info = [
            f"Znimok {tag}" + (" (pre-release)" if rels[0]["isPrerelease"] else ""),
            f"published {rels[0]['publishedAt']}",
            f"https://github.com/{REPO}/releases/tag/{tag}",
            "checksums match SHA256SUMS of the release",
            "",
        ]
        place(files, args.dest / "release", info)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("which", choices=["dev", "release"])
    ap.add_argument("--build", action="store_true", help="dev: build the current main if it has no build yet")
    ap.add_argument("--dest", type=pathlib.Path, default=DEFAULT_DEST)
    args = ap.parse_args()
    (dev if args.which == "dev" else release)(args)


if __name__ == "__main__":
    main()
