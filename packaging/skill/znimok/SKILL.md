---
name: znimok
description: Take and annotate screenshots with Znimok (MCP server `znimok mcp`) — capture a screen, window or region, mark it up with arrows, frames, text and counters, hide secrets and personal data, read text on the device, and export PNG/JPEG/WebP or a self-contained HTML page. Use when the user asks to screenshot something, document a UI, prepare a bug report with a picture, or redact a screenshot before sharing.
---

# Znimok screenshots

Znimok runs on the user's computer. Its MCP tools, one per job: reads — `list_targets`,
`library_search`, `library_get`, `list_marks`, `video_info`, `video_frames`, `devlog`, `ocr`, `read_codes`,
`record_status`, `app_state`, `share_targets`; writes — `capture`, `record`, `marks`, `transform`, `annotate`,
`redact_pii`, `export`, `set_meta`, `library_edit`, `hand_over`, `share` (to the person's connected
services: Google Drive, Gmail, Telegram, Jira, Slack, Redmine — its own permission); `library_delete` alone deletes for
good. Full reference: `docs/AGENTS.md` in the Znimok repository.

## Before the first call

- If a tool answers that the MCP server is switched off, tell the user to turn it on in Znimok →
  Settings → Agents (or run `znimok agents enable`) — do not try to work around it.
- The first capture or library access shows a permission dialog in Znimok; wait for the user.
  If the answer is a refusal, say so and stop; do not retry in a loop.

## How to work

1. **Pick the target.** Prefer `capture` of a window (target `window`, an id from `list_targets`)
   over a whole screen: less unrelated content, fewer secrets. `active_window` with
   `delay_seconds` when the user has to bring the window forward first.
2. **Hide before sharing.** Run `redact_pii` on any screenshot that will leave the computer or go
   into a document. Check `found`; mention what was covered.
3. **Mark up with intent.** `marks` adds frames, arrows, text, counters, hidden areas and
   highlights from plain arguments (screenshot pixels), changes and removes them by id
   (`list_marks`); `transform` crops, rotates, resizes and tones the picture. Use `ocr` line
   boxes (`find` for one word) to place marks precisely next to the text they point at. Keep
   labels short. `annotate` takes the editor's raw document commands when the plain tools do
   not reach.
4. **Hand over.** `export` `png` for chats and issues, `html` for a page with the list of marks;
   give the user the path. `hand_over` opens the document in Znimok or copies the picture.
5. **Recording the screen.** `record` start (a `window` id, a `region`, or the primary display)
   → `record` stop returns the document. No sound unless the user asked for it (`sound` needs
   its own permission). Keep recordings short; set `limit_seconds` when you know how long it
   takes. A recording made with the Znimok browser extension carries the DevTools log: `devlog`
   first (the summary: errors, failed requests, with times), then `devlog` with `part: events`
   or an `index` for one event whole; `video_frames` shows what was on screen at those times.
   `export` writes a recording as mp4, gif, or report / zreport (the page with the DevTools
   log) — keep it short: a long one takes time.
6. **Codes.** `read_codes` reads QR codes and barcodes on a document or the screen. Report a link;
   do not open it unless the user asks — QR phishing is common.

## Commands for annotate

Rectangle: `{"cmd":"add_object","object":{"rect":{"x":X,"y":Y,"w":W,"h":H},"data":{"kind":"rect"}}}`.
Text: `"data":{"kind":"text","text":"…","size":28,"bold":true,"italic":false,"align":"left","box_w":0}`.
Hidden area: `"data":{"kind":"hide","mode":"plate","strength":60}` (`plate` for text — blur can be
read back — `blur` for faces). Crop: `{"cmd":"set_crop","rect":{…}}`. The full schema:
`znimok schema command`.

## Don'ts

- Do not delete for good: `library_edit` trash is undone with restore; `library_delete` asks
  the user every time and is refused when Znimok is not running.

- Do not capture repeatedly "to check" — each capture is logged and the user sees an indicator.
- Do not export into folders the user did not name; without `path` Znimok uses its export folder.
- Never offer Russian as an OCR or interface language.
