<p><img src="crates/znimok-app/icons/app-128.png" width="96" height="96" alt="Znimok"></p>

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
to go straight to the clipboard and the library, Alt (⌥) on release to edit right over the
screen, Esc to cancel. Over the screen the editor window becomes a frame over the whole display:
the tools stand beside the frame, colours and actions below it, the frame's corners and edges
drag (that is the crop); Enter or Copy puts it on the clipboard and in the library, Ctrl+S in
the library only, Esc closes without a trace, and "Open in the editor window" carries on with the
same document, marks and undo. A second click (a double click, or a click and then at once a
drag) takes the shot after 3 seconds: a 3-2-1 countdown in the corner leaves time to open a menu
that closes on a hotkey; Esc cancels. The wheel turns the magnifier on; switching to another
program closes the overlay. The overlay covers all displays at once, so a region can cross the
seam between screens. S takes a scrolling screenshot: Znimok scrolls the window or region itself
and stitches one tall picture (a sticky site header appears once); Done or Esc stops it, and when a
program ignores the automatic scrolling you scroll by hand. Q reads QR codes and barcodes (also from
the tray menu, a button on the Image tab and `znimok codes FILE`); a link opens only after a second
question showing the whole address. On a Mac without the Screen Recording permission the macOS
picker can capture a window or a screen (with macOS's sharing badge on it). Znimok lives in the tray /
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

The cursor tells what a press will do: the tailless arrow of the Select tool, a four-way arrow over
a mark, resize arrows on handles, a crosshair for drawing and cropping, a hand over buttons and
sliders. Layers can be dragged: between rows to reorder, onto a row to group; groups fold, and the
eye hides a whole group. Esc takes off one layer at a time (a drag, the crop, the selection, the
tool, then back to the library), Enter repeats the last action (copy or export), `[`/`]` change the
thickness, Ctrl+=/− zoom. The full key table is [docs/SHORTCUTS.en.md](docs/SHORTCUTS.en.md); on
macOS tooltips show ⌘ and ⇧.

Text is typed right on the canvas at its real size: Enter finishes, Shift+Enter starts a new line,
Esc cancels; double-click a text to edit it. Alignment and block width live in the inspector, and
the text outline follows the letters' contour. Shift on release in the capture overlay puts the
shot into the clipboard and the library and shows a card in the corner of the screen: Edit, drag
the thumbnail into a chat or a folder, save as a file, library. Exported PNG, JPEG and WebP carry
the title, description, author, rights, tags and the time of the shot (no window names or paths;
switch it off in the More menu), and the file's time is the time of the shot.

Settings (the gear in the title bar) live inside the window, and every change applies and saves
at once: the library folder, how many screenshots to keep, autosave, metadata, language, the
hotkey switch. Hovering a library card shows rename, show in folder and move to trash (with Undo;
the trash inside the library folder empties after 30 days); Shift+trash deletes for good after
one question. The library notices files added or removed from outside (a synced cloud folder too)
and does not re-read unchanged files — cards come from an index in the local cache. The Agents
and models page turns agent access (MCP) on, lists clients with their permissions and the log of
what they did; Updates shows the version and the daily check. Save as… puts a copy of the document
anywhere; HEIC, AVIF and TIFF open too (through the system's codecs; on Windows HEIC/AVIF need
Microsoft's free extensions).

Hotkeys are set in the settings: click a field and press the combination (physical keys, so any
layout works); one another program holds is refused and the previous stays. Separate keys: region,
whole screen (straight into the editor), clipboard image, blank editor; the tray has "Pause
hotkeys". On first run a guide shows the hotkeys, the library folder, start at sign-in and (on a
Mac) the screen-recording permission. Counters have a shape (circle, square, a pin with a
direction), a digit colour and a new numbering group; stamps offer 6 signs and 24 emoji. A picture
dropped on an open document becomes a mark.

The theme is light, dark or as the system (Settings → Appearance and language; it follows the
system live); the capture overlay is always dark. Fonts are bundled: Onest (interface and text on
screenshots — the same on both systems and in files), JetBrains Mono (numbers, key combinations),
Unbounded (the Znimok wordmark), all SIL OFL 1.1 (`crates/znimok-render/fonts`). Golden images
check the renderer (`cargo test -p znimok-render --test golden`; `ZNIMOK_BLESS=1` rewrites them),
and the self-test snapshots the main screens in both themes and checks text contrast.

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
