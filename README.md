# Ristretto ☕

> The most concentrated shot of multi-agent orchestration.

A terminal-native, agent-agnostic orchestrator for running multiple code agents (Claude Code, Codex, Gemini CLI, any CLI) in parallel with planner-driven task decomposition, file ownership, and daemon persistence.

## Features

- **Agent-agnostic** — works with Claude Code, Codex, Gemini CLI, or any CLI agent
- **Planner-driven** — AI decomposes tasks; humans approve; agents execute
- **File ownership** — prevents merge hell by declaring who owns what
- **Daemon persistence** — agents survive terminal disconnect
- **Terminal-first** — works over SSH, no GUI required
- **MCP + Channel** — integrate directly into Claude Code sessions
- **i18n** — English and Chinese from day 1

## Architecture

```
rist (TUI)  ←→  ristd (daemon)  ←→  rist-mcp (planner tools)
                                 ←→  rist-channel (event push)
```

## Status

🚧 Under active development

## License

MIT
