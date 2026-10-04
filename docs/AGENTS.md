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
always** — and offers to allow everything at once (every scope but the sound of a recording and
sending out, which are asked for on their own).
Scopes:

| Scope | Tools |
|---|---|
| `capture` | `list_targets`, `capture`, `read_codes` on the screen |
| `library_read` | `library_search`, `library_get`, `list_marks`, `video_info`, `video_frames`, `devlog`, `ocr`, `export`, `hand_over`, `read_codes` on a document, resources |
| `library_write` | `marks`, `transform`, `annotate`, `redact_pii`, `set_meta`, `library_edit`, `library_delete` (asks every time) |
| `record` | `record` |
| `record_audio` | `record` with `sound` other than `none` (asked on top of `record`) |
| `share` | `share_targets`, `share` — sending documents out to the person's connected services; never part of «everything» |
| `settings` | `settings` — only the white-listed ones (ZK-239); turning the sound of recordings on asks `record_audio` too |

`record_status` and `app_state` need no permission. If Znimok is not running, nobody can be asked
and the call is refused with a hint; the person can allow a client ahead of time:

```sh
znimok agents allow "Claude Code" capture library_read library_write record
znimok agents list
znimok agents revoke "Claude Code"      # or --all
znimok agents log --last 20             # who did what, when (90 days, no contents)
```

The client name is what the client reports about itself; Znimok shows it as such.

**Fewer questions on the client's side.** Every tool carries the MCP annotations (`readOnlyHint`,
`destructiveHint`, `idempotentHint`, `openWorldHint: false` — everything is local), and there are
25 tools, one per job, reads apart from writes. In Claude Desktop's connector settings a good
start is: reads (`list_targets`, `library_search`, `library_get`, `list_marks`, `video_info`,
`video_frames`, `devlog`, `ocr`, `read_codes`, `record_status`, `app_state`, `share_targets`) — *Always allow*; writes — *Needs
approval*; `library_delete` — approval every time.

## Tools

Arguments marked `?` are optional. `document` is a library document's id (the first 8+
characters do), or a path to a `.znimok` file. Every write returns the resulting picture.

| Tool | Arguments | Returns |
|---|---|---|
| `list_targets` | — | the displays (id, name, bounds, scale, primary) and the visible windows in front-to-back order (id, title, app, bounds) |
| `capture` | `target` screen / window / active_window / region; `display?`, `window?`, `x? y? width? height?`; `delay_seconds?` (≤ 30) | a new document: id, size, the picture; for the window in front — which window it was |
| `record` | `action` start / pause / resume / stop; for start: `display?` or `window?` or `region?`, `sound?` none / system / microphone / both, `devtools_log?` (true), `limit_seconds?` (300, ≤ 3600) | start: recording, size, limit; stop: the recording as a library document (also one that ended by its limit) |
| `record_status` | — | recording or not, the agent's or the person's, paused, length so far, the last finished document |
| `library_search` | `query?`, `kind?` screenshot / video, `tags?`, `pinned?`, `has_log?`, `since?` / `until?` (YYYY-MM-DD), `trash?`, `limit?`, `with_tags?` | documents: id, name, size, created, source, tags, kind, pinned, duration, whether it has the browser log; with `with_tags` — every tag with its count |
| `library_get` | `document`, `scale?` | the document's facts and its picture with marks |
| `library_edit` | `action` import / duplicate / trash / restore; `path?` (import), `document?`, `name?` | the resulting document (trash: what went where) |
| `library_delete` | `document` | deletes for good; the person confirms in the Znimok window every time |
| `set_meta` | `document`, `name?`, `description?`, `tags?` / `add_tags?` / `remove_tags?`, `author?`, `copyright?`, `pinned?` | the document |
| `list_marks` | `document` | every mark: id, kind, box, text, colour… |
| `marks` | `document`, `delete?` {ids / all}, `update?` [{ids, dx, dy, x, y, width, height, text, color…, on a recording `from_ms` / `to_ms` / `always`}], `add?` [marks in plain words: rect, ellipse, arrow, line, pen, text, counter, hide, highlighter, stamp; on a recording each may take `from_ms` / `to_ms`] — in that order | the picture |
| `transform` | `document`, `crop?` {x, y, width, height / reset}, `rotate?` {turn right / left / half, mirror}, `resize?` {width / height / percent / canvas}, `tone?` {exposure, gamma, contrast / reset} — in that order. A recording: crop and tone as for a picture, `resize` is its size on export, `rotate` is refused; `video?` {`restore` [{from_ms, to_ms}] / `restore_all`, `cut` [{from_ms, to_ms}], `trim` {from_ms, to_ms / reset}, `mute` [{track, muted}]} (ZK-239) | the picture; a recording also `video` (kept, cuts, trim, size, tracks) |
| `annotate` | `document`, `commands` (the editor's document commands as JSON) | the picture |
| `redact_pii` | `document`, `apply?` (true), `kinds?` | what was (or would be) hidden |
| `ocr` | `document`, `languages?`, `find?` | the text with the box of every line; with `find` — only the lines that contain it |
| `read_codes` | `document?` (else the screen) | QR codes and barcodes: text, kind, box |
| `video_info` | `document` (a recording) | length, size, frame rate, trims and cuts, sound tracks, marks with their times, clicks, whether it has the DevTools log |
| `devlog` | `document`, `part?` summary / events, `kinds?`, `errors_only?`, `query?`, `from_ms?` / `to_ms?`, `limit?` (≤ 500), `offset?`, `index?` | summary: counts, errors, failed requests, navigations, dataLayer events with times; events: rows in time order; index: one event whole |
| `export` | `document`, `format`; a screenshot: png / jpeg / webp / html; a recording: mp4 (`sound?`), gif (`gif_width?`, `gif_fps?`), html, report, zreport (`language?` uk / en, `hide?`), or png / jpeg / webp of the frame at `at_ms`; `path?` | the file's path (never over a file); a recording's html is the report page when it has the DevTools log |
| `video_frames` | `document` (a recording), `at_ms?` [times, ≤ 8] or `count?` (4) | the frames as pictures with the marks of their moment, and their times |
| `hand_over` | `document`, `to` editor / clipboard | opens the document in Znimok's editor, or copies the picture |
| `share_targets` | `target?` | the connected services: key, name (a service can have several accounts), whether it needs a place, the place used last; with `target` — its places (Slack channels, Jira / Redmine projects, Telegram chats). No tokens |
| `share` | `document`, `target` (a key from `share_targets`), `what?` image / document for a screenshot, video / report / document / logs for a recording, `place?` (a channel, project, issue or chat; the last one when left out), `text?`, `hide?` | queued: Znimok sends it itself (and tries again when the network is away); the person sees it |
| `app_state` | — | whether Znimok runs; the page, the document, the tool, a recording, an agent at work; `settings` — the ones an agent may change |
| `settings` | any of: `fps` (30 / 60), `quality` (small / normal / high), `open_editor`, `devtools_log`, `hide_keys` (or `hide_keys_add` / `hide_keys_remove`), `shot_prefix`, `video_prefix`, `system_sound`, `microphone` — nothing else | what changed, the values now (through the app when it runs, else into `settings.json`) |

### Editing a recording (ZK-239)

Times are ms of the recording as recorded — what `video_info` and `devlog` give. The video stream
in the file is never rewritten: cuts, trim, muted tracks and the size on export are edits that an
`export` applies (and the person sees in the editor, with undo). Speed is not something Znimok
changes yet.

```json
{"document": "…", "video": {"cut": [{"from_ms": 4200, "to_ms": 9800}], "trim": {"from_ms": 600},
 "mute": [{"track": 1, "muted": true}]}, "resize": {"width": 1280}}
```

A mark on a recording shows the whole time unless it has `from_ms` / `to_ms`; `update` with
`always: true` shows it the whole time again.

### Marks in plain words

`marks` (add) takes each mark as a small object; colours are `#RRGGBB` or a name (`red`, `orange`,
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

The capture tools take `delay_seconds` (up to 30): time for the person to bring the right window
forward or open a menu.

## Resources

Each library document is `znimok://library/<id>` (its picture with the marks, PNG); a recording
with a browser log also has `znimok://library/<id>/log` (JSON). Frames of a recording are a
template, `znimok://library/<id>/frame/<n>` — frame `n` as recorded (0-based; `video_info` gives
the fps), full size, with the marks of its moment (ZK-239). All need `library_read`.

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

**Document a settings screen.** `list_targets` → `capture` the app's window → `ocr` to find the
labels → `annotate` with numbered counters and short texts next to the controls → `export` as
`html` (one self-contained page with the list of marks) or `png`.

**A bug report.** `capture` a region around the problem → `redact_pii` (keys, e-mails, cards,
faces covered) → `annotate` a frame and an arrow at the error → `export` `png` → attach the file
to the issue.

**Record a bug.** `list_targets` → `record` (start) with the browser's `window` → do or ask the
person to do the steps → `record` (stop) → `devlog` on the returned document. Sound is off
unless the person allows `record_audio`; a recording stops by itself at its time limit.

**Read a bug recording.** `library_search` with `has_log: true` → `video_info` → `devlog`
(the errors and failed requests with their times) → `devlog` with `index` for the one that
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
