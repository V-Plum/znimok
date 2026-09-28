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
| `znimok schema [command\|query]` | JSON Schema of commands or queries |

Commands and queries are the ones the app itself uses (`crates/znimok-core/schema/`). Examples:

```sh
znimok apply shot.znimok \
  -c '{"cmd":"add_object","object":{"rect":{"x":100,"y":100,"w":300,"h":200},"data":{"kind":"rect"}}}' \
  -c '{"cmd":"set_crop","rect":{"x":50,"y":50,"w":800,"h":500}}'
znimok query shot.znimok '{"query":"hit_test","x":120,"y":130}'
```

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
