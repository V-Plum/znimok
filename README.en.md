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
