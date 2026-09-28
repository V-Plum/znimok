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

## Agents and the settings

`agents.mcp_enabled` in `settings.json` switches MCP on (off by default); `znimok agents
enable|disable|list|allow|revoke|log` does the same from a terminal. The «Агенти» page reads
`znimok_agents::permissions::Permissions::clients()`, revokes with `revoke` / `revoke_all`, and
shows the journal from `znimok_agents::audit::Audit::entries()` (90 days, no contents).
