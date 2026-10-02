---
name: znimok
description: Take and annotate screenshots with Znimok (MCP server `znimok mcp`) — capture a screen, window or region, mark it up with arrows, frames, text and counters, hide secrets and personal data, read text on the device, and export PNG/JPEG/WebP or a self-contained HTML page. Use when the user asks to screenshot something, document a UI, prepare a bug report with a picture, or redact a screenshot before sharing.
---

# Znimok screenshots

Znimok runs on the user's computer. Its MCP tools are `list_displays`, `list_windows`,
`capture_screen`, `capture_window`, `capture_region`, `list_marks`, `add_marks`, `update_marks`,
`delete_marks`, `crop`, `rotate`, `resize`, `tone`, `annotate`, `export`, `library_search`,
`library_get`, `library_tags`, `set_meta`, `library_import`, `library_duplicate`, `library_trash`,
`library_restore`, `library_delete`, `record_start`, `record_pause`, `record_resume`, `record_stop`,
`record_status`, `video_info`, `devlog_summary`, `devlog_get`, `find_text`,
`ocr`, `read_codes`, `redact_pii`. Full reference: `docs/AGENTS.md` in the Znimok repository.

## Before the first call

- If a tool answers that the MCP server is switched off, tell the user to turn it on in Znimok →
  Settings → Agents (or run `znimok agents enable`) — do not try to work around it.
- The first capture or library access shows a permission dialog in Znimok; wait for the user.
  If the answer is a refusal, say so and stop; do not retry in a loop.

## How to work

1. **Pick the target.** Prefer `capture_window` (with an id from `list_windows`) over a whole
   screen: less unrelated content, fewer secrets.
2. **Hide before sharing.** Run `redact_pii` on any screenshot that will leave the computer or go
   into a document. Check `found`; mention what was covered.
3. **Mark up with intent.** `add_marks` draws frames, arrows, text, counters, hidden areas and
   highlights from plain arguments (screenshot pixels); `list_marks` gives their ids for
   `update_marks` and `delete_marks`; `crop`, `rotate`, `resize` and `tone` change the picture.
   Use `ocr` line boxes to place marks precisely next to the text they point at. Keep labels
   short. `annotate` takes the editor's raw commands when the plain tools do not reach.
4. **Hand over.** `export` `png` for chats and issues, `html` for a page with the list of marks.
   Give the user the path.
5. **Recording the screen.** `record_start` (a `window` id, a `region`, or the primary display) →
   `record_stop` returns the document. No sound unless the user asked for it (`sound` needs its
   own permission). Keep recordings short; set `limit_seconds` when you know how long it takes.
   A recording made with the Znimok browser extension carries the DevTools log:
   `devlog_summary` first (errors, failed requests, with times), then `devlog_get` — rows are
   short; ask one event whole with `index`. `find_text` gives the box of a word on a picture.
6. **Codes.** `read_codes` reads QR codes and barcodes on a document or the screen. Report a link;
   do not open it unless the user asks — QR phishing is common.

## Commands for annotate

Rectangle: `{"cmd":"add_object","object":{"rect":{"x":X,"y":Y,"w":W,"h":H},"data":{"kind":"rect"}}}`.
Text: `"data":{"kind":"text","text":"…","size":28,"bold":true,"italic":false,"align":"left","box_w":0}`.
Hidden area: `"data":{"kind":"hide","mode":"plate","strength":60}` (`plate` for text — blur can be
read back — `blur` for faces). Crop: `{"cmd":"set_crop","rect":{…}}`. The full schema:
`znimok schema command`.

## Don'ts

- Do not delete for good: `library_trash` is undone with `library_restore`; `library_delete` asks
  the user every time and is refused when Znimok is not running.

- Do not capture repeatedly "to check" — each capture is logged and the user sees an indicator.
- Do not export into folders the user did not name; without `path` Znimok uses its export folder.
- Never offer Russian as an OCR or interface language.
