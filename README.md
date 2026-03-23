# ☕ Ristretto

**The most concentrated shot of multi-agent orchestration.**

Run Claude Code, Codex, Gemini CLI, or any CLI agent in parallel — with automatic task decomposition, file ownership isolation, and terminal-native UI.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

## Why Ristretto?

Running one coding agent is easy. Running three or four at once is where things break down. Work overlaps, branches diverge, context windows bloat, and the final merge turns into a manual conflict-resolution exercise. Most tools stop at "spawn more agents" and leave coordination, isolation, and recovery to the operator.

Ristretto solves that by putting a daemon in the middle. `ristd` manages isolated git worktrees, keeps track of file ownership before edits collide, coordinates a shared task graph, and exposes the entire system through a terminal UI and MCP tools. You get parallelism without chaos.

## Features

- 🤖 Agent-agnostic: Claude Code, Codex, Gemini CLI, any CLI
- 📺 Terminal-native TUI: sidebar, split panes, works over SSH
- 🌳 Git worktree isolation: each agent gets its own branch
- 📁 File ownership: prevents merge conflicts before they happen
- 🔄 Context rotation at 80%: cache-friendly agent management
- 🔍 Cross-model review: adversarial review between different models
- 🔧 Auto-recovery: stuck/loop detection with nudge → restart → escalate
- 📡 MCP server: 17 tools for programmatic orchestration
- 📢 Push events: real-time notifications via MCP channel, webhooks, files
- 🌐 Bilingual: English + Chinese from Day 1
- ⚡ Rust: zero-GC daemon, portable-pty, <10ms IPC
- 🪝 Lifecycle hooks: 6 events, pipeline execution, debounce, context injection
- 🔽 Output filtering: smart truncation for cargo test/clippy/build/git
- 📊 Context budget: injected/MCP/tool output breakdown with threshold alerts
- 📋 HANDOFF.md: auto-generated on context rotation for session continuity

## Architecture

```text
┌─────────┐  ┌─────────┐  ┌──────────┐
│  rist   │  │ rist-mcp│  │rist-channel│
│  (TUI)  │  │  (MCP)  │  │ (events) │
└────┬────┘  └────┬────┘  └────┬─────┘
     │            │            │
     └────────────┼────────────┘
                  │ Unix Socket
           ┌──────┴──────┐
           │    ristd    │
           │  (daemon)   │
           └──────┬──────┘
                  │ PTY
     ┌────────────┼────────────┐
     │            │            │
┌────┴────┐ ┌────┴────┐ ┌──────┴───┐
│ Claude  │ │ Codex   │ │ Gemini   │
│  Code   │ │  CLI    │ │   CLI    │
└─────────┘ └─────────┘ └──────────┘
```

## Quick Start

```bash
cargo install ristretto-cli
# Start the daemon
ristd &
# Open the TUI
rist
# Or use MCP tools from Claude Code
claude --mcp-server rist-mcp
```

To build from source today, see [Development](#development).

## MCP Tools

| Tool | Description |
| --- | --- |
| `spawn_agent` | Start a new agent session in an isolated worktree with optional file ownership claims. |
| `list_agents` | List active and archived agent sessions. |
| `get_agent_output` | Fetch recent output lines from an agent session. |
| `write_to_agent` | Send stdin text to a running agent. |
| `kill_agent` | Terminate an agent session immediately. |
| `archive_agent` | Archive a finished session and optionally keep its worktree around. |
| `wait_for_idle` | Block until an agent becomes idle, done, or errored. |
| `run_command` | Execute a shell command inside an agent's isolated worktree. |
| `read_task_graph` | Read the current planner task graph. |
| `write_task_graph` | Replace the shared task graph with an updated plan. |
| `get_file_ownership` | Inspect the live file-to-agent ownership map. |
| `merge_agent` | Preview or execute merge of an agent branch back into the main line. |
| `context_budget` | Read context budget breakdown for an agent session. |
| `run_hooks` | Manually trigger lifecycle hooks for a specific event. |
| `list_hooks` | List configured lifecycle hooks for an agent session. |
| `handoff_status` | Check handoff state for a session. |
| `handoff_inject` | Re-queue stored handoff for injection on next spawn. |

## Key Concepts

### Task Graph & Planner

Ristretto treats a project as a graph of tasks rather than a pile of prompts. The planner tracks dependencies, status, priority, and ownership so agents can work in parallel without stepping on the same deliverable.

### File Ownership Model

Each task can claim files or paths up front. The daemon uses that ownership map to prevent overlapping edits before they become git conflicts, which is much cheaper than resolving merge failures after the fact.

### Context Rotation

Long-running agents degrade as context grows. Ristretto rotates agents before they hit the cliff, targeting roughly 80% context utilization to preserve cache locality and keep prompts responsive.

### Cross-Model Review

One model writes, another model reviews. That adversarial loop catches weak assumptions and model-specific blind spots earlier than single-model self-review, especially on risky refactors and system boundaries.

### Auto-Recovery

Agents get stuck. Ristretto watches for loops, stalls, and dead sessions, then applies a recovery ladder: nudge first, restart second, escalate only when automation fails.

### Lifecycle Hooks

Configured via `.ristretto/hooks.toml`. 6 events: `pre_spawn`, `post_output`, `pre_merge`, `post_merge`, `on_stuck`, `on_rotation`. Multiple hooks per event run as a pipeline (fail-fast when blocking). Supports debounce (`min_interval_secs`), context injection (`inject_context`), and audit logging to `.ristretto/hook-audit.jsonl`.

Example:

```toml
[[hooks]]
event = "pre_merge"
command = "cargo test && cargo clippy -- -D warnings"
blocking = true
timeout_secs = 120
inject_context = "CRITICAL: All tests must pass before merge"
```

### Output Filtering

Configured via `.ristretto/filters.toml`. Smart truncation for noisy output:

- `cargo test` success → one-line summary; failure → full backtrace
- `cargo clippy` clean → "✓ no warnings"; warnings → full output
- `cargo build` success → "✓ compiled successfully"
- `git log/diff` → configurable max entries/lines
- Unknown commands → tail truncation at configurable `max_lines` (default 200)

### Context Budget

Tracks token usage breakdown: injected tokens (task prompt, `RISTRETTO.md`, `HANDOFF.md`), MCP overhead (tool schemas), and tool output tokens. Alerts when MCP > 12.5%, tool output > 15%, or injected > 5% of max context. Exposed via `context_budget` MCP tool.

### HANDOFF.md

On context rotation, the daemon prompts the agent to write a `HANDOFF.md` with current state, decisions, and next steps. Falls back to auto-generation from `PROGRESS.md` + recent output on timeout. The handoff is automatically injected into the next agent spawned in the same worktree.

## Configuration

Ristretto splits configuration between user-global state in `~/.ristretto/` and project-local settings in `.ristretto/` at the repo root.

- `~/.ristretto/config.toml`: daemon configuration
- `~/.ristretto/channel.toml`: event routing for `rist-channel`
- `~/.ristretto/daemon.sock`: Unix socket used by `rist`, `rist-mcp`, and `rist-channel`
- `~/.ristretto/sessions.json`: persisted session metadata
- `~/.ristretto/task_graph.json`: persisted planner state
- `.ristretto/hooks.toml`: lifecycle hook definitions (project-local)
- `.ristretto/filters.toml`: output filter rules (project-local)
- `.ristretto/hook-audit.jsonl`: hook execution audit log (auto-generated)

Typical settings include socket path overrides, logging level, event routing, task/session persistence, lifecycle automation, and output filtering.

## Development

Build from source:

```bash
cargo build
```

Run the full local check suite:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps
```

Start the main components manually during development:

```bash
cargo run -p ristd
cargo run -p rist
cargo run -p rist-mcp
cargo run -p rist-channel
```

## License

MIT

## Credits

Inspired by `raccoon-mcp` and Thariq's multi-agent orchestration patterns.
