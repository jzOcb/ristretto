# Changelog

## v0.2.0 (2026-03-23)

### Phase 5: Six-Layer Framework Features

Inspired by Tw93's "You Don't Know Claude Code" six-layer framework analysis.

**Lifecycle Hooks** 🪝
- 6 hook events: `pre_spawn`, `post_output`, `pre_merge`, `post_merge`, `on_stuck`, `on_rotation`
- Pipeline execution with fail-fast on blocking hooks
- Debounce support (`min_interval_secs`) for high-frequency events
- Context injection (`inject_context`) — hook text prepended to agent prompts
- Audit logging to `.ristretto/hook-audit.jsonl`
- Config via `.ristretto/hooks.toml`

**Output Filtering** 🔽
- Smart truncation for Rust toolchain (cargo test/clippy/build)
- Success → one-line summary; failure → full output preserved
- Git log/diff → configurable max entries
- Unknown commands → tail truncation (default 200 lines)
- Config via `.ristretto/filters.toml`

**Context Budget** 📊
- Token usage breakdown: injected / MCP overhead / tool output
- Alert thresholds: MCP >12.5%, tool output >15%, injected >5%
- New MCP tool: `context_budget`
- TUI status display

**HANDOFF.md** 📋
- Auto-generated on context rotation
- Agent prompted to write structured handoff before rotation
- Fallback generation from PROGRESS.md + recent output
- Auto-injected into next agent in same worktree

### Quality
- Full repo audit: 54 findings found and fixed (7 critical, 13 high)
- Test coverage: 71 → 102+ tests
- MCP tools: 12 → 17
- CI: fmt + clippy + test (all green)

### Critical Fixes (from audit)
- Async deadlocks: `thread::sleep` → `tokio::time::sleep` in async contexts
- Command injection: structured argv instead of string concatenation
- IPC frame cap: 16MB maximum frame size
- `detect_conflicts` parser: fixed incorrect merge-tree output parsing
- Subscribe validation: fixed inverted event filter logic

## v0.1.0 (2026-03-22)

Initial release — daemon, TUI, MCP server, channel system, planner, file ownership, git integration, context rotation, cross-model review, auto-recovery.
