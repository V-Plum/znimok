# Znimok

[Українська](README.md) · **English**

> **In development.** There are no builds yet; the code and the plan are changing.

Screenshots and screen recording for Windows and macOS with an editor that keeps marks
editable: an arrow, a caption or a hidden area can be adjusted a week later. Shots and
recordings live in a library in their own open format; the result goes to the
clipboard, to a file, or straight to a person or an agent.

Znimok succeeds the screenshot and video features of
[Little Helpers](https://github.com/V-Plum/lilhelpers): the same experience, but new code
in Rust, two operating systems, an installer and a modern interface. There is no
compatibility with Little Helpers files — the features move over, not the format.

## What the first version will have

- Capture of the screen, a window or a region, correctly exposed in HDR.
- An editor: frames, arrows, text, counters, stamps, marker, hiding (plate, pixels,
  blur), cropping, effects; every action can be undone.
- A library with thumbnails, metadata and search.
- Export to PNG/JPEG and sharing; a command layer, CLI and MCP for agents.

Video recording with a synchronised DevTools log comes in the second version. The full
plan is in [docs/PLAN.md](docs/PLAN.md) (in Ukrainian).

## Try the prototype

The first working prototype of the app is the `znimok-app` crate. It has the library with
thumbnails and search, an editor with ten tools, undo, autosave to the library, Copy, export
to PNG/JPEG/WebP, opening files, pasting from the clipboard and dropping an image onto the
window. On Windows there is New screenshot and Ctrl+Shift+4 (the whole screen under the
pointer for now). Screen capture on macOS is not enabled yet.

```sh
cargo run --release -p znimok-app              # the library
cargo run --release -p znimok-app -- shot.png  # open an image right away
```

The library is `%LOCALAPPDATA%\Znimok\Library` or
`~/Library/Application Support/Znimok/Library`; `ZNIMOK_LIBRARY` points elsewhere.
A self-test that needs no screen or mouse: `ZNIMOK_SELFTEST=<dir> znimok-app <image>` runs a
scenario, writes window snapshots and `report.txt` into the folder and exits with 0 when every
check passed.

## Development

- Language — Rust (edition 2024, stable); the crate workspace is in `crates/`.
- Checks on every push and PR: `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo test` on Windows and macOS ([.github/workflows/ci.yml](.github/workflows/ci.yml)).
- Work is tracked in the Jira project `ZK`. **Every commit, branch and PR carries the
  issue key** (`ZK-21: …`, branch `zk-21-workspace`) so changes link to the ticket.
- The main branch is `main`; force-pushes and deletion are blocked on it.

## License

The code is open **for reading, not for reuse**: you may read, audit and build it for
yourself, but not redistribute it or put it into other projects without the author's
written permission. Full text — [LICENSE](LICENSE). Fonts and libraries remain under
their own licenses.
