# `znimok` — command line

The same layers as the app, without a window: `znimok-format` reads and writes documents,
`znimok-core` commands change them, `znimok-render` draws them. For scripts, CI and agents.
Add `--json` to any command for machine-readable output (errors go to stderr as
`{"error": "...", "code": N}`).

| Command | What it does |
|---|---|
| `znimok info FILE` | name, frame size, marks, id, source, tags — reads only the head of the file |
| `znimok new IMAGE -o OUT.znimok [--name N]` | a document from PNG, JPEG or WebP |
| `znimok render FILE -o OUT.png [--scale S]` | the frame with its marks, 1:1 or scaled (0 < S ≤ 16) |
| `znimok export FILE -o OUT.{png,jpg,webp} [--format F] [--quality Q]` | JPEG: transparent → white, quality 92 by default; WebP: lossless |
| `znimok apply FILE -c JSON [-c JSON…] [--stdin] [-o OUT]` | applies commands in order and saves; all or nothing — nothing is saved if any command fails |
| `znimok query FILE JSON` | answers a query, always as JSON |
| `znimok library list [--dir D]` | documents in a folder, newest first; recognised by content, not by extension. Default folder: `$ZNIMOK_LIBRARY`, else `%LOCALAPPDATA%\Znimok\Library` / `~/Library/Application Support/Znimok/Library` |
| `znimok codes FILE` | QR codes and barcodes on a document (as drawn) or a PNG / JPEG / WebP picture, read on this device |
| `znimok text FILE` | the text on a document (the picture itself, not the marks; what is hidden stays hidden) or a picture, read on this device — no AI, no network; `--json` adds each line's box and the languages used |
| `znimok schema [command\|query]` | JSON Schema of commands or queries |

Commands and queries are the ones the app itself uses (`crates/znimok-core/schema/`). Examples:

```sh
znimok apply shot.znimok \
  -c '{"cmd":"add_object","object":{"rect":{"x":100,"y":100,"w":300,"h":200},"data":{"kind":"rect"}}}' \
  -c '{"cmd":"set_crop","rect":{"x":50,"y":50,"w":800,"h":500}}'
znimok query shot.znimok '{"query":"hit_test","x":120,"y":130}'
```

## Agents (MCP)

`znimok mcp` is an MCP server on standard input/output for AI agents — it speaks the stateless
2026-07-28 protocol and the 2025-11-25 handshake. Tools: `list_displays`, `list_windows`,
`capture_screen`, `capture_window`, `capture_region`, `annotate`, `export`, `library_search`,
`library_get`, `ocr`, `redact_pii`; library documents are resources `znimok://library/<id>`.

```sh
claude mcp add znimok -- znimok mcp      # Claude Code
znimok agents enable                     # MCP is off until switched on
znimok agents list                       # on/off and lasting permissions per client
znimok agents allow "Claude Code" capture library_read
znimok agents revoke "Claude Code"       # or --all
znimok agents log --last 20              # the journal: who, what, when (90 days, no contents)
```

The first use of a scope (`capture`, `library_read`, `library_write`, `settings`) by a client is
approved by the person in the app («цей раз / ця сесія / завжди»); without the app running, only
what was allowed with `znimok agents allow` works. See `docs/IPC.md` for what the app answers.

## Hand a screenshot to an agent

`znimok handoff shot.znimok --note "Why is this button grey?"` prepares a hand-off folder
(`<data>/Handoff/<id>/`: `screenshot.png`, `context.json`, `handoff.md`) and opens Claude Code in a
new terminal with a prompt that names `handoff.md` — or, with `--to clipboard` or when Claude
Code is not installed, puts the picture, the file and the brief on the clipboard.

By default the picture is **a copy with secrets, personal data and faces hidden** and the
recognised text has secrets masked (the library document is not changed); `--no-redact` hands it
over as it is. Masking can only hide what text recognition found — look at the copy before
sending anything sensitive. `--dry-run` prepares the folder and prints the brief without running
anything.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 2 | wrong usage (unknown option, missing argument, unknown output format) |
| 3 | a file cannot be read or written |
| 4 | not a Znimok document |
| 5 | made by a newer Znimok |
| 6 | the document is damaged |
| 7 | a command or query was rejected (the message says why) |
