# Znimok guide

[Українська](guide.md) · **English**

Znimok takes screenshots on Windows and macOS (screen recording is in preview — see "Video"). Marks
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
- **X** — the text of the highlighted part (or of the whole screen) straight to the clipboard, read
  on this device, no AI; Znimok shows what was copied;
- **the wheel** turns the loupe on and changes its zoom; **Esc** cancels.

Only the keys held at the moment the button is released count — whether they went down before the
click or after it. What each gesture does (release / Shift / Alt) can be reassigned in Settings →
Screenshots, as in Little Helpers. The hint strip at the bottom of the overlay shows the current
assignment; it can be turned off there too. The overlay covers every display at once — a region
can be dragged across the seam between screens; switching to another app closes the overlay.

Separate hotkeys (changeable in the settings): the whole screen straight into the editor, the image
on the clipboard, an empty editor, QR codes (Alt+Shift+Q, ⌃⇧Q on a Mac), "Copy text from the
screen" (Alt+Shift+T, ⌃⇧T on a Mac — the overlay opens for text: choose a part and its text is on
the clipboard; the tray does the same). The tray has "Pause hotkeys".

### Over the screen

After Alt (⌥) the editor window becomes a frame over the whole display: tools beside the frame,
colours and actions above it (below when there is no room above). The corners and edges of the frame drag (it is the crop). When the
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
| Highlighter | H | a bar of fixed thickness (four to choose) along the drag, turns; its own bright inks |
| Counter | N | a numbered circle, square or pin |
| Stamp | S | 6 signs and 24 emoji; sizes S–XL |
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

**Text from the screenshot.** The button on the Image tab or X: Znimok reads the text of the
screenshot itself (not your marks; what is hidden stays hidden) — on this device, no AI, no
internet. The lines light up on the canvas, the text goes to a panel (correct it, "Copy all");
drag a frame to read only that part; click a line to copy it; Esc closes.

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
closing). Cards show thumbnails; search looks at the name, description and tags,
and with "Search the text on screenshots too" on (Settings → Library) also at the text on the screenshots
and in their captions: Znimok reads it slowly in the background on this computer and keeps it only in the
local index (turned off, what was read is forgotten); the "All / Shots /
Videos" switch in the header shows one kind (the choice is kept). Videos have a size limit of their
own (Settings → Library, 5 GB by default): the oldest go to the trash, screenshots are not affected. Hovering a card
offers rename, show in folder, to the trash (with "Undo"). Shift+trash deletes for good after one
question.

**Opening.** A click on a card opens the document in this same window (the library and the editor
are one window; "Library" goes back), Alt+click (⌥-click) in a new window, to compare shots side by
side. A document already open in another window does not open twice: Znimok offers to go to that
window or to open a copy — it is saved as a new document, so two windows never overwrite each other.

**Groups and pins.** Cards are grouped by date: "Today", "Yesterday", "This week", "This month",
then by month. The pin on a card (or "Pin" for the picked ones) puts the document into the
"Pinned" group on top; the library's limit (count or size) never removes pinned documents nor counts
them. The pin is kept in the file itself.

**Keyboard.** Arrows move through the grid (and across groups), Home / End to the first / last card,
Shift+arrow grows the pick, Space picks the card, Enter opens, F2 renames, Delete to the trash.

**Several cards.** Ctrl+click (⌘ on a Mac) adds or removes a card, Shift+click takes everything from
the last picked one, Ctrl+A all the cards shown, Esc clears the pick. A bar "Selected: N" shows at the
bottom with "Move to trash" (or Delete). A plain click still opens the document.

**Trash.** The "Library / Trash" switch in the header. In the trash a card has "Restore" and
"Destroy", the header "Destroy all" (after one question); picking several cards works the same.
A document cannot be opened straight from the trash — restore it first. Deleted documents stay in
the trash for 7 days (Settings → Library), then go for good.

The library folder is the source of truth: `%LOCALAPPDATA%\Znimok\Library` on Windows,
`~/Library/Application Support/Znimok/Library` on macOS, or any other in the settings. It can live
on a cloud drive — Znimok notices files added or removed from outside. In Explorer and Finder
`.znimok` files have thumbnails, and videos a ▶ badge with their length.

Znimok opens PNG, JPEG, WebP, GIF, BMP, and HEIC, AVIF and TIFF through the system codecs (on
Windows HEIC/AVIF need Microsoft's free extensions).

## Video (preview)

**Recording (Windows).** Alt+Shift+5 or "Record video" in the tray opens the same overlay as for
screenshots: drag — a region, click — a window (followed as it moves, or recorded "as a region" —
in the settings), Space — the whole screen, A — the sound (none / system / microphone / both; the
choice is kept). While it records, a thin red edge runs round the part
(outside it, so it is not in the video) and a bar next to it shows the time, "Pause" and "Stop"; the
time is in the tray too. The same key or "Stop" ends it: the video is in the library at once, with ▶
and its length on the card, and the card in the corner offers to open it. Settings → "Recording":
30/60 frames, quality, how a clicked window is recorded, sound: none, the system sound (what the
computer plays), the microphone or both — each source a track of its own; the microphone needs Windows'
permission (Privacy → Microphone), without it the recording goes on without it and Znimok says so. The pointer is drawn into
the video, a click as a spreading ring (the colour is in the settings), a held button as a steady
one; clicks on the recording bar are not recorded, the rest are kept as a log in the document. A window that stands still is recorded too: its first frame is taken
at once and repeated until the window changes. On macOS recording comes with the next update.

**Editor.** A video document opens in the "Video" mode: the same tools, and under the canvas a transport (Space —
play / pause, ◁ — play backwards, ← → — a frame, Shift+← → — a second, Home / End, speed 0.5–2×) and a
timeline with thumbnails of the frames on its strip. Trimming: the white
handles at the ends of the strip, or I / O at the current frame; a drag on the strip picks a piece —
Del cuts it, Shift+Del keeps only it; S splits; a cut-out piece is hatched, with "Restore". The
timeline's keys work while the last click was on it (a thin ring): there J plays backwards, K stops,
L plays forwards; on the canvas the letters stay with the tools. The recording is never changed: the edits live in the document and undo together
with the marks. The "Video" tab has the trimming summary, the sound (no sound / system /
microphone / both), the playback speed (0.5–2×, in the editor only), the crop (proportions Free / 16:9 / 4:3 / 1:1 /
9:16; a video's frame has even sides), the tone (applied to the video as it plays and kept with
the document) and the size on export (100 % / 1280 / 1920 / 50 % or your own W × H with the
proportions locked); the trimming summary has a bar of what is kept;
"Frame as screenshot" opens the current frame as a document of its own. The graphics card decodes
and shows the video — on Windows and macOS, without copying frames through the processor; the marks
are drawn over it.

**Marks in time.** Any mark on a video (frame, arrow, text, hide, marker…) appears from the frame it
was drawn on and lasts 3 seconds. Its bar is on the timeline's "marks" track: drag the bar to move
its time, or an end of it to make it longer or shorter; a click on a bar selects the mark and takes
the video into its time. Outside its time a mark is neither seen nor selectable. While the video
plays, a hide shows as a hatched plate and a marker as a translucent one; paused, they work on the
frame's pixels again. "Frame as screenshot" takes that frame's marks along, editable. Changes of
time undo like everything else.

**The browser's DevTools log.** The Znimok extension for Chrome and Edge writes, while you record,
what the page's DevTools panels show: the console (with objects and stacks), errors, the network —
request and response headers, the request's payload, the response, timings — navigations and
dataLayer (every GTM or gtag event as full JSON, with what the array held before the recording), in sync
with the video (both take their time from the system clock; the recording's pauses are cut out of the
log). The log lies in the recording's document. It is debugging data, so it is written in full;
sensitive parts can be hidden on export. While the log is written the browser shows its
"extension is debugging this browser" bar. The extension's "Record this window" starts recording the
browser's window (Windows only for now); while it records, a click on its icon stops it, pause is in the
right-click menu. Settings → "Recording" → "Browser": write the log, let the extension start a
recording, and whether a browser is connected. Installing until the extension is in the Chrome Web
Store: the release's `znimok-extension-….zip` → chrome://extensions → "Developer mode" → "Load
unpacked". Znimok registers itself for the browsers; no administrator rights needed.

**The DevTools log in the video editor.** When a recording has the browser's log, a "log" lane with
ticks appears under the marks lane of the timeline: errors red (with a faint line across the whole
timeline), warnings amber, dataLayer violet, navigations blue, network grey. The chevron by the lane's name opens a
panel under the timeline: a search (request and response bodies included), the filters "All",
"Errors", "Warnings", "Network", "Console", "Navigation", "dataLayer" with counts, and the list of events (time,
kind, message, file:line). The row the video has reached is highlighted; while it plays, the list
scrolls along. A click on a row or on a tick of the lane moves the video to that moment and opens the
details: for a request the tabs "Headers", "Payload", "Preview" (JSON laid out), "Response", "Timing"
(the request's phases as bars), as in DevTools; for the console and errors the full text and stack.
Long text is shown up to 256 KB; "Save as…" keeps the whole response, binary ones too. The triangle
button in the transport is "To the next error".

**Exporting a video.** The top button in the "Video" mode is "Copy MP4" (Ctrl+C): the file goes to
the clipboard, to paste into a chat or a folder. The menu next to it: "Copy GIF", "Copy frame"
(Ctrl+Shift+C), "Export file…" (Ctrl+E), "Save to the library" (a new video document), "Frame as
screenshot" (Ctrl+Shift+N). The export sheet says what comes out ("0:38 after trimming · 1280 × 720 ·
marks and frame applied") and offers **MP4** (H.264, the sound tracks that are on mixed into one,
short fades at the joints of what is cut), **GIF** (480–1280 wide, 5–20 fps, dithering, "Not more
than N MB" — when the estimate is over, Znimok lowers the frame rate, then the width, and says what
it changed), **HTML** (one page with the video, the marks a live layer in their time; a hide and a
marker go into the video itself) and **frame as screenshot**; each with a size estimate. A video
without any edit goes out as it is, without re-encoding. The progress shows while it runs; closing
the sheet cancels it. On macOS the GIF works for now; MP4 and HTML come with the next update.

**The developer report.** The last card of the video export sheet is "Report with the DevTools
log". It is the video with its marks and, beside it, the browser's log as in the editor: filters,
search, the list following the video, a click on a row taking the video to that moment, a request
opening in the tabs "Headers", "Payload", "Preview", "Response", "Timing". The header says when and
where it was recorded (browser, page). The events' times are after trimming; events in cut-out parts
are left out. Two forms: **one HTML page** up to 100 MB, for a chat or a ticket; **a .zreport
archive** — the page, the video beside it, the log and the dataLayer as JSON (`log.json`,
`datalayer.json`) and a poster; a .zreport opens in Znimok as a recording with its log. The log is
written in full and the sensitive parts are hidden on export: the sheet's "Hide sensitive values"
switch says how many values become `•••` — keys from the list (`Authorization` and `Cookie` headers,
`email`, `token` fields, …) and secrets by their look (tokens, passwords in addresses, e-mails,
phones, cards). Settings → "Recording": "Hide sensitive values in a report" — ask (the default),
always or never, and the list of keys, comma-separated. In your files the log stays whole.

## Sharing the result

- **Copy** (Ctrl+C) — the image to the clipboard. The Copy button can be dragged into a chat or a
  folder — a PNG file goes there. Enter repeats the last action.
- **Export** (Ctrl+E or Ctrl+Shift+S, or Other ways next to Copy) — a sheet: the format as a card
  with a size estimate (PNG — lossless with transparency, JPEG — quality 1–100, WebP — the
  smallest: quality 1–100 or Lossless, transparency kept),
  scale 50 / 100 / 200 % or a width in px, metadata, transparent → white, the name and where to: the
  clipboard, a file or a flat copy in the library. "Remember" keeps the choice; Ctrl+Shift+E (and
  Enter after an export) repeats the last export without the sheet.
- The system Share, "Save as…" — a copy of the `.znimok` document anywhere.
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
