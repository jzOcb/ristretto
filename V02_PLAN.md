# Ristretto v0.2 — Graph-First TUI Redesign

## Goal
Replace the current sidebar+panel TUI layout with a **terminal-native DAG-centric UI** where the task graph is the primary view, agents are nodes on the graph, and terminal output is secondary (expand on select). This is Ristretto's key differentiator — no one has done terminal-native agent orchestration DAG visualization.

## Architecture Overview

```
Current (v0.1):
┌─Sidebar──┬─Terminal Output────────┐
│ Agent 1  │                        │
│ Agent 2  │  (focused agent PTY)   │
│ Agent 3  │                        │
├──────────┤────────────────────────┤
│          │  Status / Task Panel   │
└──────────┴────────────────────────┘

New (v0.2):
┌─DAG View (primary)────────────────┐
│                                    │
│   [T1:auth ✓] ──→ [T3:api ●]     │
│                     ↓              │
│   [T2:db ✓] ────→ [T4:test ◐]   │
│                     ↓              │
│                   [T5:deploy ○]   │
│                                    │
├─Agent Terminal (expand on select)──┤
│ $ cargo test...                    │
├─Info Bar───────────────────────────┤
│ [G]raph [L]ist [T]erminal  agents:5│
└────────────────────────────────────┘
```

## File Changes Summary

| File | Action | Description |
|------|--------|-------------|
| `rist/src/ui.rs` | **Major rewrite** | New DAG renderer, 3 view modes, file ownership overlay |
| `rist/src/app.rs` | **Modify** | New ViewMode enum, DAG layout state, expanded node tracking |
| `rist/src/event.rs` | **Modify** | New keybindings for view switching, node selection, DAG nav |
| `rist/src/main.rs` | **Minor** | No structural changes, just import updates |
| `rist-shared/src/types.rs` | **Minor** | Add `model` field to AgentInfo (reserved, optional) |
| `rist-shared/src/protocol.rs` | **No change** | Existing protocol sufficient |
| `ristd/src/planner.rs` | **Minor** | Add `topo_sort()` method for DAG layout ordering |

## Detailed Implementation Plan

### Phase 1: Data Model Changes

#### 1.1 Add ViewMode to app.rs
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// DAG-centric: task graph fills main area, terminal on select
    Graph,
    /// Classic sidebar + terminal (current v0.1 layout)
    List,
    /// Full-screen terminal for focused agent
    Terminal,
}
```

Replace `LayoutMode` with `ViewMode`. Keep `LayoutMode` as a sub-mode for List view backward compat.

#### 1.2 Add DAG layout state to App
```rust
pub struct App {
    // ... existing fields ...
    pub view_mode: ViewMode,
    pub selected_task: Option<String>,  // task_id in DAG
    pub expanded_node: Option<String>,  // task_id with terminal expanded
    pub dag_scroll: (i16, i16),         // horizontal, vertical scroll offset
    pub show_file_overlay: bool,        // file ownership heatmap toggle
    pub task_positions: HashMap<String, (u16, u16)>,  // computed node positions
}
```

#### 1.3 Add topo_sort to planner.rs
Add a `topo_sort(&self) -> Vec<Vec<&Task>>` method that returns tasks grouped by depth level (for horizontal DAG layout). Uses Kahn's algorithm on the existing `depends_on` edges.

#### 1.4 Add optional model field to AgentInfo
```rust
pub struct AgentInfo {
    // ... existing fields ...
    /// Model identifier for display (e.g., "opus", "codex"). Reserved for future.
    #[serde(default)]
    pub model: Option<String>,
}
```

### Phase 2: DAG Renderer (ui.rs major rewrite)

#### 2.1 Top-level render dispatch
```rust
pub fn render(frame: &mut Frame<'_>, app: &App) {
    match app.view_mode {
        ViewMode::Graph => render_graph_view(frame, app),
        ViewMode::List => render_list_view(frame, app),  // current v0.1 layout
        ViewMode::Terminal => render_terminal_view(frame, app),
    }
    // Always render mode bar at bottom
    render_mode_bar(frame, app);
    if app.show_help { render_help_overlay(frame, app); }
}
```

#### 2.2 Graph View Layout
```
┌─────────────────────────────────────────┐
│ DAG area (flexible height)              │
│                                         │
│  Depth 0        Depth 1       Depth 2   │
│  ┌──────┐      ┌──────┐     ┌───────┐  │
│  │T1 ✓  │─────→│T3 ●  │────→│T5 ○   │  │
│  │auth  │      │api   │     │deploy │  │
│  │opus  │      │sonnet│     │       │  │
│  └──────┘      └──────┘     └───────┘  │
│  ┌──────┐        ↑                      │
│  │T2 ✓  │────────┘                      │
│  │db    │      ┌──────┐                 │
│  └──────┘      │T4 ◐  │                │
│                │test  │                 │
│                └──────┘                 │
├─────────────────────────────────────────┤
│ Terminal output (shown when node        │
│ is expanded, 30% height)                │
├─────────────────────────────────────────┤
│ [G]raph [L]ist [T]erm [F]iles [?]Help  │
└─────────────────────────────────────────┘
```

#### 2.3 Node Rendering
Each task node is a box (width=14, height=4):
```
┌────────────┐
│ ● T3: api  │   ← status icon + task id + short title
│ sonnet     │   ← agent model (if assigned) or agent type
│ ctx: 45%   │   ← context usage gauge (if running)
└────────────┘
```

Selected node: double border or highlight color.
Node colors based on TaskStatus:
- Done → Green border
- Working → Blue border  
- Pending → Gray border
- Blocked → Red border
- Review → Yellow border

#### 2.4 Edge Rendering (ASCII arrows)
Edges drawn between nodes using box-drawing characters:
- Horizontal: `───`
- Arrow: `→`
- Vertical: `│`
- Corner: `┐`, `└`, `┘`, `┌`

Use a simple left-to-right layout:
1. Topological sort → depth levels
2. Each depth level is a column
3. Within column, tasks sorted by dependency count
4. Edges routed through inter-column gutters

#### 2.5 File Ownership Overlay
When `show_file_overlay` is true, render below the DAG:
```
┌─File Ownership──────────────────────────┐
│ src/auth.rs     → T1 (agent-1) ✓       │
│ src/api.rs      → T3 (agent-3) ●       │
│ src/db.rs       → T2 (agent-2) ✓       │
│ tests/          → T4 (agent-4) ◐       │
│ ⚠ src/main.rs   → UNCLAIMED             │
└─────────────────────────────────────────┘
```

### Phase 3: Keybindings & Navigation

#### 3.1 Global Mode Keys
| Key | Action |
|-----|--------|
| `G` | Switch to Graph view |
| `L` | Switch to List view (v0.1 layout) |
| `T` | Switch to Terminal view (fullscreen PTY) |
| `F` | Toggle file ownership overlay |
| `?` | Help |

#### 3.2 Graph View Navigation
| Key | Action |
|-----|--------|
| `Tab` / arrow keys | Move selection between nodes |
| `Enter` | Expand selected node → show terminal output |
| `Esc` | Collapse expanded terminal |
| `h/j/k/l` | Scroll DAG view |
| `N` | Spawn new agent (prompt for task) |
| `K` | Kill agent on selected task |
| `1-9` | Quick-select task by index |

#### 3.3 Event handler changes
Refactor `handle_normal_mode` to dispatch by `ViewMode`:
```rust
async fn handle_normal_mode(app: &mut App, client: &DaemonClient, key: KeyEvent) -> io::Result<bool> {
    // Global keys first (G, L, T, ?, Ctrl-C, Q)
    // Then view-specific keys
    match app.view_mode {
        ViewMode::Graph => handle_graph_keys(app, client, key).await,
        ViewMode::List => handle_list_keys(app, client, key).await,
        ViewMode::Terminal => handle_terminal_keys(app, client, key).await,
    }
}
```

### Phase 4: List View (preserve v0.1)

Keep the existing sidebar+terminal layout as `ViewMode::List` for users who prefer it. This is mostly renaming existing functions:
- `render_sidebar` → same
- `render_main_panels` → same
- `render_task_panel` → same

### Phase 5: Terminal View

Fullscreen PTY output for focused agent. Simple:
```
┌─Agent 3: api implementation (sonnet) ●──┐
│                                          │
│ $ cargo build --release                  │
│   Compiling ristretto v0.2.0             │
│   ...                                    │
│                                          │
├──────────────────────────────────────────┤
│ [G]raph [L]ist [T]erm  ctx:45%  agent:3 │
└──────────────────────────────────────────┘
```

## Implementation Order

1. **app.rs changes** — ViewMode, new state fields, task mapping helpers
2. **planner.rs** — topo_sort method  
3. **types.rs** — model field on AgentInfo
4. **ui.rs** — Graph renderer (biggest piece)
5. **event.rs** — New keybindings
6. **main.rs** — Minimal import changes

## Testing Strategy

- Unit tests for topo_sort (cycle detection, linear chain, diamond pattern)
- Unit tests for node position calculation
- Unit tests for edge routing
- Existing tests must still pass (List view is preserved)
- Manual TUI testing with `cargo run -p rist`

## Non-Goals (v0.2)

- Model routing / smart model selection (future)
- Hook execution timeline visualization (v0.3)
- Agent topology / merge relationship graph (v0.3)
- Context budget mini gauges on DAG nodes are stretch goal — can show "n/a" initially

## Constraints

- All rendering in ratatui — no external TUI deps
- Must compile with current workspace deps (ratatui 0.29, crossterm 0.28)
- UTF-8 safe, CJK-width aware (existing `truncate_text` pattern)
- Backward compatible: `ViewMode::List` = exact v0.1 behavior
