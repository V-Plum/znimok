<p><img src="crates/znimok-app/icons/app-128.png" width="96" height="96" alt="Znimok"></p>

# Znimok

[Українська](README.md) · **English**

> **Preview version 0.0.1.** Znimok is in development: features arrive with every update, the code and the plan are changing.

Screenshots for Windows and macOS with an editor that keeps marks editable: an arrow, a caption or
a hidden area can be adjusted a week later. Shots live in a library on your computer, in their own
open format; the result goes to the clipboard, to a file, or straight to a person or an AI agent.
Screen recording is in preview; a DevTools log alongside it comes later.

Znimok succeeds the screenshot and video features of [Little Helpers](https://github.com/V-Plum/lilhelpers):
the same experience, but new code in Rust, two operating systems, an installer and a modern
interface. Little Helpers files are not compatible — the features move over, not the format.

**[Download →](https://v-plum.github.io/znimok/en/)** · [Guide](docs/guide.en.md) ·
[Keys](docs/SHORTCUTS.en.md) · [Privacy](docs/privacy.en.md)

## Features

- **Capture** a region, a window (without what covers it) or the whole screen, across several
  displays, with a loupe, a 3-2-1 countdown, scrolling capture of long pages and QR code reading.
  Correct exposure in HDR.
- **Three release actions** — into the editor, straight to the clipboard and the library, or edit
  right over the screen; what each gesture does can be reassigned.
- **Editor**: rectangles and ellipses, lines with arrowheads, pen, text, hide (pixelate, blur,
  plate), highlighter, counters, stamps and emoji, pictures, crop, rotation, tone; any colour with
  an eyedropper; groups, layers, turning marks; every action undoes.
- **Library** with thumbnails, search, date groups, pins, the keyboard, picking several cards and a trash (restore / destroy); the folder may live on a cloud drive. Every
  document opens in a window of its own — shots can be compared side by side and marks dragged
  between them.
- **Video (preview)**: record a region, a window or the screen on Windows; playback on the
  graphics card on Windows and macOS (forwards, backwards, 0.5–2×, thumbnails on the strip);
  trimming and cutting without re-encoding; marks with their time on a track; a frame as a
  screenshot of its own.
- **Share**: copy, drag into a chat, export to PNG/JPEG/WebP with metadata.
- **For agents**: an MCP server and the `znimok` command — capture, read the library, edit
  documents (off until you turn it on).
- **Privacy**: no telemetry, no ads; by itself Znimok goes online only to check for updates.
- Ukrainian and English, light and dark theme, signed updates.

Every feature in detail — in the [guide](docs/guide.en.md).

## Installing

Windows 11 — an `.msi` (no administrator rights), macOS 15+ on Apple silicon — a `.dmg`. Until the
installers are signed with Microsoft and Apple certificates, the system warns on the first start —
how to get past it is in the [guide](docs/guide.en.md#installing).

## Build it yourself

```sh
cargo run --release -p znimok-app              # the library
cargo run --release -p znimok-app -- shot.png  # open an image right away
```

`ZNIMOK_LIBRARY` sets another library folder. Self-test without a screen or a mouse:
`ZNIMOK_LIBRARY=<dir>/lib ZNIMOK_SELFTEST=<dir> znimok-app <image>` runs a scenario, writes window
snapshots and `report.txt` into the folder and exits with 0 when every check passed. Rendering is
checked against reference images (`cargo test -p znimok-render --test golden`; `ZNIMOK_BLESS=1`
updates them). Fonts are built in: Onest, JetBrains Mono, Unbounded — all under SIL OFL 1.1
(`crates/znimok-render/fonts`).

## Development

- Language — Rust (edition 2024, stable); the crate workspace is in `crates/`.
- Checks on every push and PR: `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo test` on Windows and macOS ([.github/workflows/ci.yml](.github/workflows/ci.yml)).
- Work is tracked in the Jira project `ZK`. **Every commit, branch and PR carries the
  issue key** (`ZK-21: …`, branch `zk-21-workspace`) so changes link to the ticket.
- The main branch is `main`; force-pushes and deletion are blocked on it.
- The download page is `site/` (static files); the `pages` workflow publishes it to
  [v-plum.github.io/znimok](https://v-plum.github.io/znimok/) after changes on `main`.
- Updates: Windows — Znimok's own updater (ECDSA-signed SHA256SUMS, MSI, a rollback when the
  new version does not start); macOS — Sparkle 2 in the bundle with Znimok's own Updates page
  (check, EdDSA-verified download, relaunch). The daily check runs only when switched on.
- Video (phase 8): recording — `crates/znimok-video-win` (WGC or Desktop Duplication → a wgpu
  shader → NV12 on the GPU → hardware H.264 through Media Foundation; the software encoder as the
  fallback); playback — `crates/znimok-play` (Media Foundation on D3D11 or VideoToolbox → a
  texture without a copy through the processor → wgpu). Try recording without the app:
  `cargo run --release -p znimok-video-win --example record -- --seconds 5 [--window Title]`.

## License

The code is open **for reading, not for reuse**: you may read, audit and build it for
yourself, but not redistribute it or put it into other projects without the author's
written permission. Full text — [LICENSE](LICENSE). Fonts and libraries remain under
their own licenses.

The interface is made with [Slint](https://slint.dev) (Royalty-free License 2.0; the About Slint
widget is on the About Znimok page).
