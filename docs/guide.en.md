# Znimok guide

[Українська](guide.md) · **English**

Znimok takes screenshots on Windows and macOS (screen recording comes in the next version). Marks
on a screenshot stay editable: an arrow, a caption or a hidden area can be fixed a week later.
Everything stays on your computer, in the library, in the open `.znimok` format.

- [Installing](#installing)
- [Capturing](#capturing)
- [The editor](#the-editor)
- [The library](#the-library)
- [Sharing the result](#sharing-the-result)
- [Settings](#settings)
- [Agents](#agents)
- [Updates](#updates)
- [Privacy](#privacy)
- [If something goes wrong](#if-something-goes-wrong)

Every key is in the [shortcut table](SHORTCUTS.en.md). On macOS, ⌘ stands in for Ctrl and ⌥ is Alt.

## Installing

Download the installer from the [Znimok page](https://v-plum.github.io/znimok/en/) or from the
[GitHub releases](https://github.com/V-Plum/znimok/releases/latest).

**Windows 11.** Run `Znimok-…-windows-x64.msi`. No administrator rights are needed: Znimok is
installed for your user only. Until the installer is signed with a certificate, SmartScreen says
"Windows protected your PC": click "More info" → "Run anyway". If Smart App Control is on, it does
not let an unsigned program in at all — until Znimok is signed it cannot be installed there.

**macOS 15+ (Apple silicon).** Open `Znimok-…-macos-arm64.dmg` and drag Znimok to Applications.
Until Znimok is notarised by Apple, the first start is blocked: try to open it, then go to
System Settings → Privacy & Security → "Open Anyway" and enter your password. Znimok then asks for
the Screen Recording permission — without it a screenshot can only be taken through the system
window picker (the shot then shows the sharing indicator).

After the first release Znimok can also be installed with `winget install VPlum.Znimok` on
Windows and `brew install --cask v-plum/znimok/znimok` on macOS.

On the first start a short guide shows the hotkeys, the library folder, start at login and (on a
Mac) the screen recording permission. Znimok lives in the tray (on a Mac, in the menu bar):
closing the window hides it there, "Quit" is in the icon's menu.

## Capturing

"New screenshot" in the tray, Alt+Shift+4 on Windows or ⌃⇧4 on a Mac freezes the screen under the
pointer. Then:

- **drag** — a region; **click** — a window (without what covers it); **Space** — the whole screen;
- **release the mouse button** — the shot opens in the editor;
- **Shift** on release — straight to the clipboard and the library, and a card appears in the
  corner: "Edit", drag the thumbnail into a chat or a folder, save as a file, the library;
- **Alt (⌥)** on release — edit right over the screen (see below);
- **double click**, or click and drag at once — a shot in 3 seconds: a 3-2-1 countdown in the
  corner, time enough to open a menu that closes on a key press;
- **S** — a scrolling capture: Znimok scrolls the window or region itself and stitches one long
  image (a sticky site header appears once); "Done" or Esc stops; if the app does not scroll by
  itself, scroll by hand;
- **Q** — read QR codes and barcodes. A link opens only after a separate question showing the
  whole address;
- **the wheel** turns the loupe on and changes its zoom; **Esc** cancels.

Only the keys held at the moment the button is released count — whether they went down before the
click or after it. What each gesture does (release / Shift / Alt) can be reassigned in Settings →
Screenshots, as in Little Helpers. The hint strip at the bottom of the overlay shows the current
assignment; it can be turned off there too. The overlay covers every display at once — a region
can be dragged across the seam between screens; switching to another app closes the overlay.

Separate hotkeys (changeable in the settings): the whole screen straight into the editor, the image
on the clipboard, an empty editor, QR codes (Alt+Shift+Q, ⌃⇧Q on a Mac). The tray has "Pause
hotkeys".

### Over the screen

After Alt (⌥) the editor window becomes a frame over the whole display: tools beside the frame,
colours and actions below it. The corners and edges of the frame drag (it is the crop). When the
frame comes close to an edge of the screen, the panels glide over to the other side. Enter or
"Copy" — to the clipboard and the library, Ctrl+S — to the library only, Esc — close without a
trace, "Open in the editor window" — the same document with its marks and history.

## The editor

Tools are on the left; the letters work on the Ukrainian layout too.

| Tool | Key | What it does |
|---|---|---|
| Select | V | select, move, resize and turn |
| Rectangle, ellipse | R, E | outline and fill, dashes, corners |
| Line | L | arrowheads on either end |
| Pen | P | a free stroke, with arrowheads too |
| Text | T | typed right on the canvas |
| Hide | B | pixelate, blur or a plate |
| Highlighter | H | a translucent band |
| Counter | N | a numbered circle, square or pin |
| Stamp | S | 6 signs and 24 emoji |
| Image | I | a picture from a file as a mark |
| Crop | C | cut the screenshot |

Holding Ctrl (⌘) turns any tool into Select for the moment. The pointer tells what will happen: an
arrow over empty space, a four-way arrow over a mark, resize arrows on the handles, a turning arrow
on the rotation handle, a crosshair for drawing.

**Selection and order.** A band over empty space, Shift- or Ctrl-click; Ctrl+D duplicates;
Ctrl+] / Ctrl+[ — forward / backward; Ctrl+G groups, Ctrl+Shift+G ungroups. On the canvas a group
is one: a click on a member selects the whole group and a drag moves all of it; Alt+click (⌥-click)
selects that member alone. Align and distribute are in the inspector.

**Counters.** Counters of one numbering are one group (in Layers too); the one just placed stays
selected on its own, to turn or move it at once. The inspector has "Edit the whole group" (colour,
shape, size, effects for all at once), "Delete group" and "New numbering group"; its title names the
group: "Counter — Group 1".

**Turning.** A selected mark has a round handle on a stem above it: drag it to turn the mark about
its centre; Shift turns in 45° steps. Several selected marks turn about the middle of their shared
box. The angle shows beside the pointer and can be typed into the "∠" field of the inspector. A
turned mark resizes in its own frame — the opposite corner stays where it is.

**Size.** Drag the handles; with Shift the size changes in proportion, with Alt (⌥) symmetrically
about the centre. Counters and stamps have corner handles only and always scale as a whole — the
number grows with the circle.

**Colours.** Eight palette colours, "none" and the rainbow button: it opens a picker with a
saturation/brightness square, a hue strip, a HEX field, an eyedropper (the next click on the
screenshot takes its colour) and a row of recent colours. A shape without an outline is a solid
plate of one colour: the "Fill" row changes it, "no fill" brings the outline back in that colour.

**Text.** Typed on the canvas at its real size: Enter — done, Shift+Enter — a new line, Esc —
cancel; double click a caption to edit it. Alignment, box width and the letter outline are in the
inspector.

**Pictures as marks.** The Image button (I) takes a picture from a file, Ctrl+V from the clipboard;
a picture can also just be dropped on the open screenshot.

**The Image tab.** Crop (C: drag the frame or its handles, Enter — apply, Esc — cancel), rotate and
mirror, tone (exposure, gamma, contrast; hold "Compare" to see the original), image size, QR codes.
Crop, rotation and tone are a recipe over the original: each change undoes in one step and the
original is never lost.

**Layers.** The list of marks on the right: drag a row between others (a new order) or onto another
row (a group); groups fold, the eye hides a mark or a whole group.

**Undo.** Ctrl+Z / Ctrl+Shift+Z undo and redo any action. Esc takes off one layer at a time: a drag,
the crop, the selection, the tool — and only then closes the window and goes back to the library.

**Windows.** Every document has a window of its own: a new shot, a file or a library card opens one
more, so two shots can sit side by side or on different displays, and marks and pictures can be
dragged between them. The card of a document that is already open raises its window instead of
opening a copy. «Library» or Esc save the document, close its window and show the library; the
window's close button just closes it. The window's title is the document's name.

**View.** Ctrl+0 — fit, Ctrl+1 — 100 %, Ctrl+wheel or pinch — zoom, the held wheel — pan.

## The library

Every shot saves itself to the library (autosave can be turned off — then Ctrl+S and a question on
closing). Cards show thumbnails; search looks at the name, description and tags. Hovering a card
offers rename, show in folder, to the trash (with "Undo"; the trash empties itself after 30 days).
Shift+trash deletes for good after one question.

The library folder is the source of truth: `%LOCALAPPDATA%\Znimok\Library` on Windows,
`~/Library/Application Support/Znimok/Library` on macOS, or any other in the settings. It can live
on a cloud drive — Znimok notices files added or removed from outside. In Explorer and Finder
`.znimok` files have thumbnails, and videos a ▶ badge with their length.

Znimok opens PNG, JPEG, WebP, GIF, BMP, and HEIC, AVIF and TIFF through the system codecs (on
Windows HEIC/AVIF need Microsoft's free extensions).

## Sharing the result

- **Copy** (Ctrl+C) — the image to the clipboard. The Copy button can be dragged into a chat or a
  folder — a PNG file goes there. Enter repeats the last action.
- **Other ways** (the arrow next to Copy): export to PNG, JPEG or WebP (Ctrl+Shift+S), the system
  Share, "Save as…" — a copy of the `.znimok` document anywhere.
- Exported files carry the title, description, author, rights, tags and the time of the shot —
  no window titles or paths; this can be turned off in Other ways. The file time equals the shot
  time.

## Settings

The gear in the title bar. Every change applies and is saved at once.

- **Screenshots** — what each release gesture does, the hint strip, quick saving to the library.
- **Hotkeys** — click a field and press the combination. Keys are physical: they work on any
  layout. One already taken by another app is not grabbed; the previous one stays.
- **Library** — the folder and how many shots to keep.
- **Agents and models** — access for agents (MCP), clients and their permissions, the log of what
  they did.
- **Appearance and language** — Ukrainian or English, light, dark or system theme, start at login.
- **Privacy** — the update check and everything else that touches the network.
- **Updates** — the version, check now and daily.

## Agents

Znimok can be handed to an AI agent (Claude Code, Claude Desktop and other MCP clients): the MCP
server (off until you turn it on in the settings) lets them take screenshots, read the library,
edit documents and read QR codes — each client separately, with permissions and a log. The
`znimok` command in a terminal does the same without a window. Details: [AGENTS.md](AGENTS.md) and
[CLI.md](CLI.md).

## Updates

Once a day Znimok checks for a new version, if that is on (Settings → Updates). On Windows the
update is downloaded, its signature checked, and if the new version does not start, Znimok goes
back to the previous one. On macOS updates come through Sparkle.

## Privacy

Znimok collects no telemetry and shows no ads. By itself it goes online only to check for updates
(which can be turned off). A cloud model works only with your own key and only for features you
turned on. The full text: [privacy policy](privacy.en.md).

## If something goes wrong

- **A hotkey does nothing** — another app probably has it: Settings → Hotkeys show which
  combination is active.
- **On a Mac the shot is empty or only the wallpaper** — the Screen Recording permission is
  missing: System Settings → Privacy & Security → Screen Recording → Znimok.
- **After a crash** Znimok offers the crash report at the next start; whether to send it is up to
  you.
- Questions and bugs go to [GitHub Issues](https://github.com/V-Plum/znimok/issues).
