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
window. New screenshot, Ctrl+Shift+4 (Windows) or ⌃⇧4 (macOS) freeze the screen under the
pointer: drag for a region, click for a window, Space for the whole screen, Shift on release
to go straight to the clipboard and the library, Esc to cancel. Znimok lives in the tray /
menu bar; closing the window hides it there, and Quit is in the icon's menu. Selection: a
rubber band over empty space, Shift/Ctrl-click, holding Ctrl makes any tool Select for the
moment; Ctrl+D duplicates, Ctrl+]/[ changes the order, Ctrl+G groups; align and distribute
live in the inspector.

Line (L) takes heads on either end — and so does the pen; a line shows X1/Y1/X2/Y2 in the
inspector, and shapes, text, counters and stamps get shadow and glow. The Image tab has crop
(C shows the whole picture: drag a frame, pull its handles or its middle; Enter applies, Esc
cancels), rotate and mirror, tone (exposure, gamma, contrast; hold Compare to see the
original) and image size. Crop, turns and tone are a recipe over the original, each change is
one undo step. The Copy button can be dragged into a chat or a folder — it carries a PNG
file. Questions such as "Save changes?" with autosave off come in Znimok's own dialog, not a
system box.

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
