# Znimok privacy policy

> Draft (28 September 2026, ZK-84) — takes effect with the first public release after the owner's review.
> Українською: [privacy.md](privacy.md).

**In short:** Znimok runs on your computer. It collects no telemetry, usage statistics or analytics, shows
no ads and sends nothing anywhere without your action. The only network request the app makes on its own
is the update check, and you can switch it off.

## What Znimok processes

- **Screenshots and screen recordings** you take, and everything you add to them: annotations, titles,
  descriptions, tags.
- **Source details**: window and program name, display size and number, capture date — so the library can
  show and search them.
- **Recognised text** in screenshots (OCR), if you turned on search by text. Recognition runs on the device.
- **App settings.**
- **Action log** of AI agents and the assistant: time, command, whether a screenshot was involved, token
  count and cost estimate — **no images**; kept for 90 days.
- While recording video (v2), only if you turned it on: system and microphone audio, cursor movement and
  clicks, the DevTools log from your browser through the Znimok extension.

## Where it is stored

Everything stays on your device:

| Data | Location |
|---|---|
| Library (screenshots, videos, reports) | the folder you chose; by default `%LOCALAPPDATA%\Znimok\Library` (Windows), `~/Library/Application Support/Znimok/Library` (macOS) |
| Library index and thumbnails | the app's cache folder |
| Settings, action log, agent keys | the app's configuration folder |
| Cloud model API key | the system store: Windows Credential Manager / macOS Keychain |
| Crash reports | a local app folder |

If you keep the library folder on a cloud drive (OneDrive, iCloud Drive, Google Drive, a NAS), that
service syncs the files under its own terms. Znimok itself sends nothing there.

## When data leaves your device

Only in these cases:

1. **Update check** (can be switched off in Settings → Privacy). Znimok asks GitHub Releases for the number
   of the latest version and downloads the update if you install it. GitHub sees your IP address and the
   request itself, as with opening any web page. Nothing about you or your screenshots is sent.
2. **You share a result yourself**: copy to the clipboard, save a file, use the system Share menu or (later)
   the Slack, Jira, Telegram, Redmine integrations. You decide what goes where.
3. **A cloud model with your own key** (the Ctrl+K assistant and other features you turned on). Without a
   key the cloud is unavailable. With a key:
   - without a separate question, only the text of your command and the document structure without pixels
     are sent (annotations, sizes, title, tags);
   - an image is sent only if the command needs it, after masking on the device and a "Send the screenshot?"
     dialog that previews exactly what will be sent;
   - the data is processed by the model provider (Anthropic) under its policy and your agreement with it;
     Znimok has no access to it;
   - before the first cloud action Znimok explains what is paid and roughly how much it costs.
4. **AI agents on your computer** (Claude Code, Claude Desktop and others over MCP). Access is off until you
   turn it on. Each agent gets data only within the permissions you gave ("only now", "this session",
   "always"); a yellow indicator is shown while it captures, and every action is logged. What an agent does
   with the data afterwards is up to its developer and your settings of that agent.
5. **A crash report** is sent only by you: the app keeps it locally and may offer to open an issue page on
   GitHub. You decide what to attach.

## The browser extension (Znimok — DevTools log)

The extension for Chrome and Edge exists for one thing: while Znimok records the screen, it records the
browser tab's developer log next to the video, so a bug can be watched together with what the page did.

- **When it works.** Only while a Znimok recording is running (or you start one from the extension's
  button). Outside a recording it reads nothing. It attaches to the active tab only; Chrome shows its own
  bar "Znimok started debugging this browser" for exactly that time.
- **What it reads in that tab.** What the browser's DevTools show: console messages and errors with their
  stack, network requests and responses (addresses, headers including cookies and authorization, request
  bodies, response bodies up to 4 MB, timings, WebSocket frames), page navigations and the page's
  `dataLayer` / `gtag` events. This can include personal or secret data the page sends — the extension
  exists for debugging, and records it in full on purpose.
- **Where it goes.** Only to the Znimok app on the same computer, through the browser's Native Messaging
  channel; the app stores it inside the recording's document (`.znimok`) on your disk. The extension
  sends nothing to the internet, to us or to anyone else, keeps no copies of its own, and has no
  analytics.
- **Your control.** Turn the browser log off in Znimok's Settings → Recording, or remove the extension.
  Before sharing a recording, Znimok's export can hide secrets (cookies, tokens, passwords) in the log,
  or leave the log out; deleting the document deletes the log.
- **Permissions.** `debugger` (to read the DevTools data of the recorded tab), `tabs` and `activeTab`
  (which tab is active, its address and title), `scripting` (a short mark in the tab's title so Znimok can
  find that window for "Record this window"), `nativeMessaging` (the channel to the app), `storage` (the
  extension's own settings), `alarms` (keeping the connection alive during a recording),
  `contextMenus` (the "Record this window" item).

## What Znimok does not do

- no telemetry, usage statistics, analytics or device identifiers — and no switch for them, because there is
  nothing to switch on;
- no ads, no selling or passing data to third parties;
- no keyboard interception: hotkeys are registered through the OS, without global hooks;
- no hidden screen or audio recording: recording is always visible (frame, tray state, control card).

## Metadata in files you share

On export Znimok can write the title, description, author and date into the file (you see this in the
export dialog). Window and program names and file paths are not written into files for sharing. "Remove all
metadata" strips everything except what is technically required.

## Your control

- Switch off update checks, cloud features and agent access in Settings.
- Revoke agent permissions: Agents page → Revoke all permissions.
- Remove the API key: Settings → Privacy.
- Delete screenshots in the library (to the OS trash) or simply delete the files in the library folder.
- Remove everything: uninstall the app, delete the library, cache and configuration folders; the key —
  in the system credential store.

## Children

Znimok is not specifically aimed at children and collects no data about any users, including children.

## Changes to this policy

Changes are published with a release in the [V-Plum/znimok](https://github.com/V-Plum/znimok) repository
and in the release notes. A new network request is described here before the release that introduces it.

## Contact

Vadym Slyva, v.v.plum@gmail.com — for privacy questions and vulnerability reports.
