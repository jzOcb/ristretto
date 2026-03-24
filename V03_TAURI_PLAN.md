# Ristretto v0.3 — Tauri Desktop Terminal App

## Problem Statement
Ristretto v0.2 is a TUI tool that runs inside an existing terminal. Users want a standalone terminal app — like Warp or Raccoon — where AI agents run natively. The current TUI has limited UI flexibility (ratatui can't do rich interactive graphics) and requires users to already have a terminal emulator installed.

## Goal
Build Ristretto as a native desktop app (macOS first) with real terminal emulation, DAG visualization, and multi-agent orchestration. Ship as a .app that users download and run directly.

## Architecture Decision: Tauri v2 + xterm.js (Phase A) → libghostty (Phase B)

### Why Tauri + xterm.js (not Electron, not pure Rust GPU)

**vs Electron:**
- Tauri binary ~5MB vs Electron ~150MB
- Tauri uses system webview, not bundled Chromium
- Rust backend = ristd daemon is already Rust, zero FFI overhead
- Lower memory footprint (50-100MB vs 300MB+)

**vs Pure Rust GPU (Warp approach):**
- Warp spent $100M+ and years building custom UI framework with Metal shaders
- Drawing interactive DAG + Canvas in pure Rust GPU = massive engineering cost
- Web tech (React + Canvas/SVG) is 100x faster to iterate on for rich UI
- Agent output is text streams, not vim at 240hz — xterm.js performance is sufficient

**vs Fork WezTerm/Alacritty:**
- These are terminal emulators, not app frameworks
- Adding rich UI panels (DAG, file ownership, agent management) in their rendering pipelines = fighting the architecture
- Ristretto's value is the orchestration UI, not the terminal emulator

### Why libghostty later (Phase B, if needed):
- xterm.js handles 99% of agent use cases fine
- libghostty is a zero-dep embeddable terminal library (Zig + C ABI)
- Can replace xterm.js terminal panels without changing any UI code
- Only needed if users report terminal rendering issues (vim, tmux inside agents)

## Technical Architecture

```
┌─ Ristretto.app (Tauri v2 shell) ─────────────────────────┐
│                                                            │
│  ┌─ Frontend (React + TypeScript) ──────────────────────┐ │
│  │                                                       │ │
│  │  ┌─ DAG Panel ──────────┐  ┌─ Terminal Panel ──────┐ │ │
│  │  │ React Flow / D3      │  │ xterm.js              │ │ │
│  │  │ Interactive nodes     │  │ Per-agent PTY         │ │ │
│  │  │ Edge routing          │  │ Scrollback buffer     │ │ │
│  │  │ Status colors         │  │ Search/selection      │ │ │
│  │  └──────────────────────┘  └───────────────────────┘ │ │
│  │                                                       │ │
│  │  ┌─ Agent Bar ──────────────────────────────────────┐ │ │
│  │  │ 🟢 Claude  🔵 Codex  🟡 Gemini  [+ New Agent]  │ │ │
│  │  │ Model badge | Context gauge | Status             │ │ │
│  │  └──────────────────────────────────────────────────┘ │ │
│  │                                                       │ │
│  │  ┌─ File Ownership Panel ───────────────────────────┐ │ │
│  │  │ Tree view with agent color coding                │ │ │
│  │  │ Conflict warnings                                │ │ │
│  │  └──────────────────────────────────────────────────┘ │ │
│  └───────────────────────────────────────────────────────┘ │
│                                                            │
│  ┌─ Tauri Rust Backend ─────────────────────────────────┐ │
│  │  IPC Commands:                                        │ │
│  │  - list_agents, spawn_agent, kill_agent               │ │
│  │  - get_task_graph, get_file_ownership                 │ │
│  │  - resize_pty, write_to_pty, subscribe_events         │ │
│  │                                                       │ │
│  │  Unix Socket Client → ristd daemon                    │ │
│  └───────────────────────────────────────────────────────┘ │
│                                                            │
└────────────────────────────────────────────────────────────┘
         │
         │ Unix socket IPC
         ▼
┌─ ristd daemon (existing, unchanged) ──────────────────────┐
│  PTY manager | Planner | File ownership | Hooks            │
│  Git manager | Review engine | Recovery | Context monitor  │
└────────────────────────────────────────────────────────────┘
```

## Component Breakdown

### 1. Tauri Shell (rist-app/)
- Tauri v2 with Rust backend
- Backend is a thin IPC bridge to ristd's Unix socket
- All existing ristd protocol reused (Request/Response/Event frames)
- Auto-starts ristd daemon if not running
- Packages as .app (macOS), .AppImage (Linux), .exe (Windows)

### 2. Frontend (rist-app/src/)
- React 19 + TypeScript
- Tailwind CSS for styling
- React Flow (or @xyflow/react) for DAG visualization
- xterm.js + @xterm/addon-fit for terminal panels
- Zustand for state management
- IPC via Tauri's invoke/listen APIs

### 3. DAG Visualization
- React Flow provides: interactive nodes, edge routing, pan/zoom, selection
- Custom node component: status icon + title + model badge + context gauge
- Edge styles by dependency type
- Automatic dagre layout (left-to-right, same as our topo_sort)
- Click node → expand terminal panel for that agent
- Real-time updates via ristd event stream

### 4. Terminal Panels
- Each agent gets its own xterm.js instance
- Connected to agent's PTY via Tauri IPC → ristd
- Split view: 2-4 terminals visible simultaneously
- Tab bar for agents not currently visible
- Full terminal features: scrollback, selection, search, copy/paste

### 5. Agent Management Bar
- Spawn new agents with model selection dropdown
- Per-agent: model badge, context usage gauge, status indicator
- Quick actions: kill, restart, send input
- Cost tracking (token usage, estimated cost)

### 6. File Ownership Panel
- Tree view of project files
- Color-coded by owning agent
- Conflict indicators for files touched by multiple agents
- Click to see diff of agent's changes

## Implementation Plan

### Phase 1: Scaffold (Day 1-2)
- `cargo tauri init` in rist-app/ directory
- React + TypeScript + Tailwind setup
- Tauri Rust backend: connect to ristd socket
- Basic IPC: list_agents, spawn_agent
- Verify: app launches, connects to daemon, shows agent list

### Phase 2: Terminal Panels (Day 3-5)
- xterm.js integration with Tauri IPC for PTY data
- Per-agent terminal tabs
- Bidirectional I/O: type in xterm → write to agent PTY
- Terminal resize handling
- Split view (2 terminals side by side)

### Phase 3: DAG Visualization (Day 6-8)
- React Flow integration
- Custom node components with status/model/context
- Auto-layout with dagre
- Real-time updates from ristd events
- Click node to focus terminal

### Phase 4: Agent Management (Day 9-10)
- Spawn dialog with model selection
- Agent bar with status indicators
- Kill/restart actions
- Context usage gauges

### Phase 5: File Ownership (Day 11-12)
- File tree component
- Agent color coding
- Conflict detection display

### Phase 6: Polish & Package (Day 13-14)
- macOS .app packaging with code signing
- Auto-start ristd daemon
- Settings panel (theme, keybindings, model defaults)
- Keyboard shortcuts (Cmd+T new agent, Cmd+W close, etc.)

## Dependencies (Frontend)
- react, react-dom (^19)
- @tauri-apps/api (v2)
- @xyflow/react (React Flow v12) — DAG
- @xterm/xterm, @xterm/addon-fit, @xterm/addon-search — terminal
- zustand — state management
- tailwindcss — styling
- dagre — graph layout algorithm

## Dependencies (Rust/Tauri)
- tauri (v2)
- rist-shared (existing crate) — types, protocol
- tokio — async runtime
- serde, serde_json — serialization

## What We Reuse From v0.1/v0.2
- ristd daemon (100% unchanged)
- Unix socket protocol (Request/Response/Event)
- topo_sort algorithm (rist-shared)
- Task, AgentInfo, FileOwnership types (rist-shared)
- All hooks, git manager, review engine, recovery, context monitor

## What's New
- rist-app/ — Tauri desktop app crate
- Frontend React app
- Tauri IPC bridge (Rust → ristd socket)
- React Flow DAG renderer
- xterm.js terminal integration

## Non-Goals (v0.3)
- Model routing / smart model selection
- Collaboration / multi-user
- Cloud sync
- Plugin system
- Windows/Linux packaging (macOS first)
- libghostty integration (Phase B, after user feedback)

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| xterm.js perf with heavy output | Low | Medium | Throttle updates, virtualize scrollback |
| Tauri IPC latency for PTY data | Low | High | Binary frames, batch updates |
| React Flow perf with 50+ nodes | Low | Medium | Virtualization, lazy rendering |
| ristd protocol needs changes | Low | Low | Protocol is extensible (Unknown variants) |
| macOS code signing complexity | Medium | Medium | Use tauri-action for CI |
