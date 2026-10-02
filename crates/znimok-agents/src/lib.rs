//! Znimok for AI agents (ZK-67, ZK-68, ZK-69).
//!
//! - [`mcp`] — the MCP server over stdio (`znimok mcp`), speaking both the stateless 2026-07-28
//!   protocol and the 2025-11-25 handshake.
//! - [`tools`] — the tools (PLAN §5.9): displays and windows, captures, annotate, export, library
//!   search and get, OCR, redact_pii; library documents as resources.
//! - [`backend`] — work happens here headless; captures on macOS, permission prompts and the
//!   activity indicator go to the running app over IPC (it is started in the background if needed).
//! - [`permissions`] and [`audit`] — «цей раз / ця сесія / завжди» per client and scope, and a
//!   90-day journal without contents.
//!
//! - [`handoff`] — «Передати агенту»: a hand-off folder (picture, context, brief) for Claude Code
//!   or the clipboard.
//!
//! MCP is off until the person switches it on (settings `agents.mcp_enabled`).

pub mod audit;
pub mod backend;
mod edit;
pub mod handoff;
pub mod library;
mod libtools;
pub mod mcp;
pub mod permissions;
pub mod tools;
mod vidtools;

use std::path::PathBuf;

/// The agent with the person's library, permissions, journal and settings.
pub fn agent() -> tools::Agent {
    let dirs = znimok_settings::Dirs::system();
    let data = dirs
        .as_ref()
        .map(|d| d.data.clone())
        .unwrap_or_else(std::env::temp_dir);
    let enabled = znimok_settings::Store::open_default()
        .map(|s| s.get().agents.mcp_enabled)
        .unwrap_or(false);
    tools::Agent {
        lib: library::Library::from_settings(),
        perms: permissions::Permissions::new(data.join("agents.json")),
        audit: Some(audit::Audit::open(data.join("agents-audit.jsonl"))),
        gui: backend::Gui::default(),
        capture: backend::capturer(),
        enabled,
        export_dir: data.join("Exports"),
    }
}

/// `znimok mcp`: serves stdin/stdout until the client closes them.
pub fn serve_stdio() -> std::io::Result<()> {
    let agent = agent();
    if !agent.enabled {
        eprintln!(
            "znimok mcp: the MCP server is switched off — tools will refuse until it is turned on \
             (Znimok → Settings → Agents, or `znimok agents enable`)"
        );
    }
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    mcp::Server::new(&agent).run(stdin.lock(), stdout.lock())
}

/// Where the agent's files live (for `znimok agents`).
pub fn data_dir() -> PathBuf {
    znimok_settings::Dirs::system()
        .map(|d| d.data)
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests;
