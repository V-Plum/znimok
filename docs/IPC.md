# IPC between Znimok processes

The running app is an IPC server (`znimok-ipc`: JSON-RPC 2.0, one message per line, named pipe
with a user-only DACL on Windows, a 0600 unix socket on macOS, a session token first). Other
Znimok processes — the MCP server (`znimok mcp`), the CLI, a second start of the app — are its
clients. This page lists the methods the app answers. Every one is optional: a client that gets
«method not found» falls back or tells the person what to do.

Start the server when the app starts (`znimok_ipc::Server::start(Config::default(), handler)`)
and keep it for the app's lifetime.

## `app.open` — open documents (ZK-75)

A second start with files (double-click on a `.znimok`) hands them over instead of opening a
second window: `znimok_ipc::forward_open(&Config::default(), &files)` → `true` means «taken».

```json
{"method": "app.open", "params": {"paths": ["C:\\Users\\u\\Znimok\\Library\\Znimok-….znimok"]}}
→ {"result": null}
```

Paths are absolute. Open each in the editor and bring the window forward.

## `agents.ask` — the permission dialog (ZK-69)

Shown when an agent uses a scope for the first time. `client` is the name the agent reports
about itself (show it as such — it is not verified).

```json
{"method": "agents.ask", "params": {"client": "Claude Code", "scope": "capture", "tool": "capture_screen"}}
→ {"result": {"grant": "once" | "session" | "always" | null}}
```

`null` (or an error) = refused. Scopes: `capture` (screenshots and the list of windows),
`library_read`, `library_write`, `settings`. The MCP server stores «always» itself
(`<data>/agents.json`) and keeps «session» for its own process; the app only asks.

## `agents.activity` — the indicator (ZK-69)

```json
{"method": "agents.activity", "params": {"client": "Claude Code", "tool": "capture_screen", "active": true}}
→ {"result": null}
```

While `active`, show the tray state and the «Агент знімає екран» plate for capture tools. A
second message with `active: false` ends it.

## `capture.displays`, `capture.windows`, `capture.take` — screenshots for agents (ZK-68)

On macOS only the app may capture (Screen Recording belongs to Znimok.app), so the MCP server
asks it. If the app is not running, `znimok mcp` starts it with
`open -g -b ua.plum.znimok.app --args --background` and waits for the IPC server.

```json
{"method": "capture.displays", "params": {}}  → {"result": [DisplayInfo, …]}
{"method": "capture.windows",  "params": {}}  → {"result": [WindowInfo, …]}
{"method": "capture.take", "params": {"target": CaptureTarget}}
→ {"result": {"path": "/Users/u/Library/Application Support/Znimok/Library/Znimok-….znimok"}}
```

`DisplayInfo`, `WindowInfo` and `CaptureTarget` are the `znimok-platform` types in their serde
form. `capture.take` saves the shot as a new library document (as a hotkey shot would, with
`meta.source` = `screen` / `window` / `region`) and returns its path; do not open the editor.
Windows does not need these: the MCP server captures there itself.

## The command layer — `capture.*`, `record.*`, `editor.*`, `video.scrub`, `app.state`, `app.wait` (ZK-213)

What the hotkeys, the tray and the editor's buttons do, for other programs: the Logi Options+
plugin first (ZK-118), the CLI and MCP later. Commands are queued and run on the UI thread a few
times a second; the answer comes at once and says only that the command was taken — a command the
current state does not allow (undo with nothing open, pause with no recording) does nothing, and
failures arrive as events.

```json
{"method": "capture.start", "params": {"mode": "region"}}   // region | screen | clipboard | editor | codes | text
{"method": "record.toggle"}  {"method": "record.pause"}  {"method": "record.resume"}  {"method": "record.stop"}
{"method": "editor.undo"}  {"method": "editor.redo"}
{"method": "editor.tool", "params": {"name": "rect"}}       // select rect ellipse line pen text hide highlighter counter stamp crop (or "index")
{"method": "editor.zoom", "params": {"step": "+1"}}         // +1 | -1 | fit | 100
{"method": "video.scrub", "params": {"frames": -5}}         // ±120 at most
→ {"result": {"queued": true, "state": {…}}}
```

`app.state` is what other programs see, refreshed by the UI thread after every round:

```json
{"method": "app.state"}
→ {"result": {"page": "library" | "editor" | "settings" | "overlay" | "recording",
              "document": "Знімок 2026-10-01 …" | null, "video": false,
              "tool": "rect", "zoom": 100, "can_undo": true, "can_redo": false,
              "recording": {"paused": false, "time": "0:12"}}}   // while recording
```

`app.wait` is a long poll like `devtools.wait`: the events after `since`, waiting up to
`timeout_ms` (≤ 30 000, default 15 000) for one — or for the state to change. Keep one call
waiting and pass the returned `seq` back; the last 200 events are kept for a late poller.

```json
{"method": "app.wait", "params": {"since": 41, "timeout_ms": 15000}}
→ {"result": {"seq": 43, "events": [{"seq": 42, "name": "shotTaken", "t": 1790000000000}, …], "state": {…}}}
```

Events: `shotTaken`, `copied`, `recordStart`, `recordStop`, `recordPause`, `recordResume`,
`exportDone`, `textRead`, `codesRead`, `failed`. `t` is the wall clock in ms. The plugin's haptic
event source maps these names to waveforms.

## Agents and the settings

`agents.mcp_enabled` in `settings.json` switches MCP on (off by default); `znimok agents
enable|disable|list|allow|revoke|log` does the same from a terminal. The «Агенти» page reads
`znimok_agents::permissions::Permissions::clients()`, revokes with `revoke` / `revoke_all`, and
shows the journal from `znimok_agents::audit::Audit::entries()` (90 days, no contents).
