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
| `capture` | `list_windows`, `capture_screen`, `capture_window`, `capture_region` |
| `library_read` | `library_search`, `library_get`, `ocr`, `export`, resources |
| `library_write` | `annotate`, `redact_pii` |

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
| `export` | `document`, `format` (`png`/`jpeg`/`webp`/`html`), `path?` | the written file |
| `library_search` | `query?`, `tag?`, `limit?` (≤ 200) | documents, newest first |
| `library_get` | `document` | picture + metadata |
| `ocr` | `document`, `languages?` (e.g. `["uk","en"]`) | text and line boxes, on the device |
| `redact_pii` | `document`, `apply?` (true), `faces?` (true) | what was found; with `apply` covered by Hide marks and saved |

Resources: `resources/list` lists the library, `resources/read` gives a document as PNG.

### Annotate: commands

The same commands the app uses (`crates/znimok-core/schema/`). Frequent ones:

```json
{"cmd": "add_object", "object": {"rect": {"x": 40, "y": 60, "w": 300, "h": 120}, "data": {"kind": "rect"}}}
{"cmd": "add_object", "object": {"rect": {"x": 380, "y": 90, "w": 1, "h": 1},
  "data": {"kind": "text", "text": "Натисніть тут", "size": 28, "bold": true, "italic": false, "align": "left", "box_w": 0}}}
{"cmd": "add_object", "object": {"rect": {"x": 20, "y": 20, "w": 200, "h": 30}, "data": {"kind": "hide", "mode": "plate", "strength": 60}}}
{"cmd": "set_crop", "rect": {"x": 0, "y": 0, "w": 800, "h": 600}}
```

## Scenarios

**Document a settings screen.** `list_windows` → `capture_window` the app → `ocr` to find the
labels → `annotate` with numbered counters and short texts next to the controls → `export` as
`html` (one self-contained page with the list of marks) or `png`.

**A bug report.** `capture_region` around the problem → `redact_pii` (keys, e-mails, cards,
faces covered) → `annotate` a frame and an arrow at the error → `export` `png` → attach the file
to the issue.

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
