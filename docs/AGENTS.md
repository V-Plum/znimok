# Znimok for AI agents

Znimok gives AI agents screenshots with hands: capture a screen, a window or a region, mark it up,
hide secrets and personal data, read the text and hand the result over — on the person's
computer, with their approval. It is an MCP server (`znimok mcp`, stdio) that speaks both the
stateless 2026-07-28 protocol and the 2025-11-25 handshake.

## Connect

MCP is **off** until the person switches it on: Znimok → Settings → Agents, or

```sh
znimok agents enable
```

| Client | How |
|---|---|
| Claude Code | `claude mcp add znimok -- znimok mcp` |
| Claude Desktop | open `znimok.mcpb` (Settings → Extensions), or add the JSON below |
| Any MCP client | command `znimok`, arguments `["mcp"]`, transport stdio |

```json
{ "mcpServers": { "znimok": { "command": "znimok", "args": ["mcp"] } } }
```

On Windows the command is `znimok.exe` (in the Znimok install folder); on macOS the `znimok`
command-line tool from the app bundle.

## Permissions

The first time a client uses a scope, Znimok asks the person — **this time / this session /
always**. Scopes:

| Scope | Tools |
|---|---|
| `capture` | `list_windows`, `capture_screen`, `capture_window`, `capture_region`, `read_codes` on the screen |
| `library_read` | `library_search`, `library_get`, `library_tags`, `list_marks`, `video_info`, `devlog_summary`, `devlog_get`, `find_text`, `ocr`, `export`, `read_codes` on a document, resources |
| `library_write` | `set_meta`, `library_import`, `library_duplicate`, `library_trash`, `library_restore`, `library_delete` (asks every time), `add_marks`, `update_marks`, `delete_marks`, `crop`, `rotate`, `resize`, `tone`, `annotate`, `redact_pii` |
| `record` | `record_start` (`record_pause`, `record_resume`, `record_stop`, `record_status` only touch the agent's own recording and ask nothing) |
| `record_audio` | `record_start` with `sound` other than `none` (asked on top of `record`) |

`list_displays` needs no permission. If Znimok is not running, nobody can be asked and the call is
refused with a hint; the person can allow a client ahead of time:

```sh
znimok agents allow "Claude Code" capture library_read library_write
znimok agents list
znimok agents revoke "Claude Code"      # or --all
znimok agents log --last 20             # who did what, when (90 days, no contents)
```

The client name is what the client reports about itself; Znimok shows it as such.

## Tools

Documents are identified by the `id` a capture or `library_search` returns (the first 8+
characters are enough) or by a path to a `.znimok` file. Coordinates are **screenshot pixels**,
origin top-left. Every picture comes back scaled to what models take (long edge ≤ 2576 px) as
PNG, plus a `resource_link` `znimok://library/<id>` to the original.

| Tool | Arguments | Result |
|---|---|---|
| `list_displays` | — | displays: `id`, `name`, `bounds` (desktop units), `scale`, `primary` |
| `list_windows` | — | visible windows: `id`, `title`, `app`, `bounds` |
| `capture_screen` | `display?` (id; primary by default) | new library document + picture |
| `capture_window` | `window` (id from `list_windows`) | the window without what covers it |
| `capture_region` | `x`, `y`, `width`, `height` (desktop units, one display) | new document + picture |
| `annotate` | `document`, `commands` (editor commands, see `znimok schema command`) | saved document + picture |
| `list_marks` | `document` | the marks: `id`, `kind`, box, `text`, `color`, `hidden`; the picture's size and crop |
| `add_marks` | `document`, `marks` (each: `kind` — `rect`, `ellipse`, `arrow`, `line`, `pen`, `text`, `counter`, `hide`, `highlighter`, `stamp` — and its place, see below) | saved document, the new ids (`created`), the picture |
| `update_marks` | `document`, `ids`, and what changes: `dx` / `dy`, `x` / `y` / `width` / `height`, `color`, `fill`, `line_width`, `opacity`, `text`, `size`, `bold`, `italic`, `hidden`, `name` | saved document + picture |
| `delete_marks` | `document`, `ids` or `all: true` | saved document + picture |
| `crop` | `document`, `x`, `y`, `width`, `height` (pixels of the whole picture) or `reset: true` | saved document + picture; nothing is thrown away |
| `rotate` | `document`, `turn` (`right`, `left`, `half`) and/or `mirror` (`horizontal`, `vertical`) | saved document + picture |
| `resize` | `document`, `width` and/or `height`, or `percent`; or `canvas` (`x`, `y`, `width`, `height`) with `fill` | saved document + picture |
| `tone` | `document`, `exposure` (stops), `gamma`, `contrast`, or `reset: true` | saved document + picture |
| `export` | `document`, `format` (`png`/`jpeg`/`webp`/`html`), `path?` | the written file |
| `library_search` | `query?`, `tag?`, `kind?` (`screenshot` / `video`), `pinned?`, `has_log?`, `since?` / `until?` (YYYY-MM-DD), `trash?`, `limit?` (≤ 200) | documents, newest first: `id`, `name`, `kind`, `tags`, `pinned`, `duration_ms`, `has_log`… |
| `library_tags` | — | the tags with the number of documents each |
| `set_meta` | `document`, and what changes: `name`, `description`, `tags` / `add_tags` / `remove_tags`, `author`, `copyright`, `pinned` | saved document + picture |
| `library_import` | `path` (PNG, JPEG, WebP or a `.znimok` file), `name?` | new document + picture |
| `library_duplicate` | `document`, `name?` | the copy (a recording stays a recording) |
| `library_trash` | `document` | moved to the trash — no question, `library_restore` brings it back |
| `library_restore` | `document` (id of a trashed one) | the document, back in the library |
| `library_delete` | `document` (in the library or the trash) | deleted for good — **the person confirms every time** in the Znimok window |
| `library_get` | `document` | picture + metadata |
| `record_start` | `display?` (the primary one by default) or `window?` (id) or `region?` {x, y, width, height}; `sound?` none / system / microphone / both; `devtools_log?` (true); `limit_seconds?` (300, at most 3600) | starts a screen recording; the app must be running; the person sees the recording frame and the bar with Stop |
| `record_pause`, `record_resume` | — | pause and resume the agent's recording |
| `record_stop` | — | stops it and returns the recording as a library document (also one that ended by its limit) |
| `record_status` | — | recording or not, the agent's or the person's, paused, length so far, the last finished document |
| `video_info` | `document` (a recording) | length, size, frame rate, trims and cuts, sound tracks, marks with their times, clicks, whether it has the DevTools log |
| `devlog_summary` | `document` | the browser log in short: counts by kind, errors, failed requests, navigations, dataLayer events — with times in the video |
| `devlog_get` | `document`, `kinds?`, `errors_only?`, `query?`, `from_ms?` / `to_ms?`, `limit?` (≤ 500), `offset?`, or `index` for one event whole | events in time order (`i`, `at_ms`, `kind`, `level`, and the kind's own fields); sensitive values hidden as the settings say |
| `find_text` | `document`, `text`, `languages?` | the lines that contain the text, with boxes in pixels |
| `ocr` | `document`, `languages?` (e.g. `["uk","en"]`) | text and line boxes, on the device |
| `read_codes` | `document`, or `display?`, or `x`, `y`, `width`, `height` (desktop units) | QR codes and barcodes, on the device: `text`, `kind` (`link` + `url`, `wifi` + `ssid`/`password`/`security`/`hidden`, `contact`, `event`, `email` + `address`, `phone` + `number`, `text`), `format`, `bounds`; the screen is read without adding a document (macOS: the app keeps its shot, `saved_as`) |
| `redact_pii` | `document`, `apply?` (true), `faces?` (true) | what was found; with `apply` covered by Hide marks and saved |

Resources: `resources/list` lists the library, `resources/read` gives a document as PNG; a
recording with the browser's log also has `znimok://library/<id>/log` — the log as JSON.

### Marks in plain words

`add_marks` takes each mark as a small object; colours are `#RRGGBB` or a name (`red`, `orange`,
`yellow`, `green`, `blue`, `violet`, `black`, `white`, `grey`):

```json
{"kind": "rect", "x": 40, "y": 60, "width": 300, "height": 120, "color": "red", "line_width": 4}
{"kind": "arrow", "from": [500, 300], "to": [360, 120]}
{"kind": "text", "x": 380, "y": 90, "text": "Натисніть тут", "size": 28, "bold": true}
{"kind": "counter", "x": 60, "y": 80, "shape": "circle"}
{"kind": "hide", "x": 20, "y": 20, "width": 200, "height": 30, "mode": "plate"}
{"kind": "highlighter", "x": 40, "y": 200, "width": 260, "height": 18}
```

Boxes (`rect`, `ellipse`, `hide`, `highlighter`) take `x`, `y`, `width`, `height`; `arrow` and
`line` take `from` and `to`; `pen` takes `points`; `text` takes its top-left corner; `counter`
and `stamp` take their centre. Counters number themselves in the order they are added.

### Annotate: commands

The same commands the app uses (`crates/znimok-core/schema/`). Frequent ones:

```json
{"cmd": "add_object", "object": {"rect": {"x": 40, "y": 60, "w": 300, "h": 120}, "data": {"kind": "rect"}}}
{"cmd": "add_object", "object": {"rect": {"x": 380, "y": 90, "w": 1, "h": 1},
  "data": {"kind": "text", "text": "Натисніть тут", "size": 28, "bold": true, "italic": false, "align": "left", "box_w": 0}}}
{"cmd": "add_object", "object": {"rect": {"x": 20, "y": 20, "w": 200, "h": 30}, "data": {"kind": "hide", "mode": "plate", "strength": 60}}}
{"cmd": "set_crop", "rect": {"x": 0, "y": 0, "w": 800, "h": 600}}
```

## Prompts

`prompts/list` offers ready scenarios (the client shows them as commands); each is a short plan
over the tools above:

| Prompt | Arguments | What it does |
|---|---|---|
| `bug_report` | `problem`, `window?` | capture, hide what is private, mark the problem, name and tag it, export a PNG |
| `document_screen` | `app` | capture a window, number its controls with counters, write the legend, export HTML |
| `redact_before_sharing` | `document?` | show what would be hidden, hide it, export |
| `read_recording` | `document?` | read a recording's DevTools log and say what failed and when |

## Scenarios

**Document a settings screen.** `list_windows` → `capture_window` the app → `ocr` to find the
labels → `annotate` with numbered counters and short texts next to the controls → `export` as
`html` (one self-contained page with the list of marks) or `png`.

**A bug report.** `capture_region` around the problem → `redact_pii` (keys, e-mails, cards,
faces covered) → `annotate` a frame and an arrow at the error → `export` `png` → attach the file
to the issue.

**Record a bug.** `list_windows` → `record_start` with the browser's `window` → do or ask the
person to do the steps → `record_stop` → `devlog_summary` on the returned document. Sound is off
unless the person allows `record_audio`; a recording stops by itself at its time limit.

**Read a bug recording.** `library_search` with `has_log: true` → `video_info` → `devlog_summary`
(the errors and failed requests with their times) → `devlog_get` with `index` for the one that
matters (its stack, headers and body) → say what went wrong and when in the video.

**Before sharing a screenshot.** `library_search` → `redact_pii` with `apply: false` to see what
would be hidden → `redact_pii` to apply → `export`.

## Good to know

- Nothing leaves the computer: OCR and masking run on the device; the MCP server never calls the
  network.
- The library is plain files; the app shows what agents add on its next scan.
- On macOS screenshots are taken by the Znimok app (the Screen Recording permission belongs to
  it); `znimok mcp` starts it in the background when needed.
- Ukrainian text recognition on Windows is not available from the OS (Windows OCR has no
  Ukrainian); ask for `["en"]` there, or use the app's cloud recognition with the person's consent.
