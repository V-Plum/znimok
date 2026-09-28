# Packaging

- `mcpb/` — the MCP Bundle for Claude Desktop (ZK-74). `python packaging/mcpb/pack.py --win
  <znimok.exe> --mac <znimok> --out dist/znimok.mcpb`; the tool list is read from the built
  binary. Validated with the official `mcpb validate`. The release workflow (ZK-79) builds it.
- `skill/znimok/SKILL.md` — a skill for Claude (how to use the Znimok tools well); copy the
  folder to `~/.claude/skills/` or ship it with the plugin.

Agent-facing docs: `docs/AGENTS.md`, `docs/llms.txt`.
