# Task: Build Ristretto Desktop App (Tauri v2)

## What You're Building
A native macOS desktop app for AI agent orchestration. NOT a terminal emulator — an **AI agent operations center**.

## Project Location
Build everything inside `~/Projects/ristretto/rist-app/`

## Tech Stack
- Tauri v2 (Rust backend)
- React 19 + TypeScript + Vite
- Tailwind CSS v4
- @xyflow/react (React Flow v12) — DAG visualization
- @xterm/xterm + @xterm/addon-fit — raw terminal fallback
- zustand — state management

## Architecture

```
rist-app/
├── src-tauri/          # Rust backend
│   ├── src/
│   │   ├── main.rs     # Tauri entry point
│   │   ├── lib.rs      # Tauri commands
│   │   ├── daemon.rs   # ristd socket client
│   │   └── pty.rs      # PTY data bridge
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                # React frontend
│   ├── App.tsx
│   ├── main.tsx
│   ├── stores/
│   │   └── agent-store.ts    # Zustand store
│   ├── components/
│   │   ├── DagPanel.tsx      # React Flow DAG
│   │   ├── DagNode.tsx       # Custom node component
│   │   ├── AgentView.tsx     # Structured output + raw terminal
│   │   ├── AgentBar.tsx      # Bottom agent management bar
│   │   ├── CommandCard.tsx   # Structured command output card
│   │   ├── DiffViewer.tsx    # Inline diff viewer
│   │   ├── TestResults.tsx   # Test pass/fail display
│   │   ├── ErrorCard.tsx     # Error display with stack trace
│   │   ├── RawTerminal.tsx   # xterm.js wrapper (toggle)
│   │   ├── CommandPalette.tsx # Cmd+K palette
│   │   └── StatusBar.tsx     # Connection status, agent count
│   ├── hooks/
│   │   ├── use-daemon.ts     # Tauri IPC hook
│   │   └── use-keyboard.ts   # Keyboard shortcut hook
│   └── lib/
│       ├── output-parser.ts  # Parse agent output into structured blocks
│       └── types.ts          # TypeScript types matching rist-shared
├── index.html
├── package.json
├── tsconfig.json
├── vite.config.ts
└── tailwind.config.ts
```

## Step-by-Step Implementation

### Step 1: Tauri + React Scaffold
```bash
cd ~/Projects/ristretto
npm create tauri-app@latest rist-app -- --template react-ts --manager npm
cd rist-app
npm install
```
Verify: `npm run tauri dev` opens a window.

### Step 2: Install Frontend Dependencies
```bash
npm install @xyflow/react @xterm/xterm @xterm/addon-fit @xterm/addon-search zustand tailwindcss @tailwindcss/vite
```

### Step 3: Tauri Rust Backend (src-tauri/)

#### src-tauri/src/daemon.rs
Connect to ristd Unix socket at `~/.ristretto/daemon.sock`:
- Send/receive length-prefixed JSON frames (existing protocol in rist-shared)
- Commands: ListAgents, SpawnAgent, KillAgent, GetTaskGraph, SubscribeEvents
- Event stream: AgentOutput, TaskStatusChanged, FileOwnershipChanged

```rust
use std::path::PathBuf;
use tokio::net::UnixStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use serde::{Deserialize, Serialize};

pub struct DaemonClient {
    stream: UnixStream,
}

impl DaemonClient {
    pub async fn connect() -> Result<Self, Box<dyn std::error::Error>> {
        let sock = dirs::home_dir().unwrap().join(".ristretto/daemon.sock");
        let stream = UnixStream::connect(sock).await?;
        Ok(Self { stream })
    }
    
    pub async fn send_request(&mut self, req: &str) -> Result<String, Box<dyn std::error::Error>> {
        let bytes = req.as_bytes();
        let len = (bytes.len() as u32).to_be_bytes();
        self.stream.write_all(&len).await?;
        self.stream.write_all(bytes).await?;
        
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await?;
        let resp_len = u32::from_be_bytes(len_buf) as usize;
        let mut resp_buf = vec![0u8; resp_len];
        self.stream.read_exact(&mut resp_buf).await?;
        Ok(String::from_utf8(resp_buf)?)
    }
}
```

#### src-tauri/src/lib.rs — Tauri Commands
```rust
#[tauri::command]
async fn list_agents(state: State<'_, AppState>) -> Result<Vec<AgentInfo>, String> { ... }

#[tauri::command]
async fn spawn_agent(state: State<'_, AppState>, agent_type: String, task: String) -> Result<String, String> { ... }

#[tauri::command]
async fn kill_agent(state: State<'_, AppState>, agent_id: String) -> Result<(), String> { ... }

#[tauri::command]
async fn get_task_graph(state: State<'_, AppState>) -> Result<TaskGraph, String> { ... }

#[tauri::command]
async fn write_to_pty(state: State<'_, AppState>, agent_id: String, data: Vec<u8>) -> Result<(), String> { ... }
```

Event stream: use `tauri::Emitter` to push ristd events to the frontend:
```rust
// In a background task:
loop {
    let event = daemon.read_event().await;
    app_handle.emit("agent-output", &event).unwrap();
    app_handle.emit("task-update", &event).unwrap();
}
```

### Step 4: React Frontend

#### Layout (App.tsx)
```
┌─────────────────────────────────────────────────────┐
│ StatusBar (connection status, agent count)           │
├─────────────────────┬───────────────────────────────┤
│                     │                               │
│   DAG Panel         │   Agent View                  │
│   (React Flow)      │   (structured output)         │
│   40% width         │   60% width                   │
│                     │                               │
├─────────────────────┴───────────────────────────────┤
│ Agent Bar (agent tabs + spawn button + Cmd+K)       │
└─────────────────────────────────────────────────────┘
```

Use CSS Grid. Dark theme. Resizable panels (drag divider).

#### DagPanel.tsx
- React Flow with dagre auto-layout
- Custom DagNode component: 
  - Status dot (green=done, blue=running, gray=pending, red=error)
  - Task title (truncated)
  - Agent model badge (small, e.g., "opus", "codex")
  - Context usage mini bar
- Edges: animated when task is running
- Click node → select agent in AgentView
- Pan + zoom

#### AgentView.tsx — THE CORE COMPONENT
Default view is **structured output**. Toggle to raw terminal.

Output parser (`output-parser.ts`) converts raw agent PTY output into blocks:
- **CommandBlock**: `$ cargo test` → expandable card with command + status
- **DiffBlock**: file diffs → syntax-highlighted inline diff
- **TestBlock**: test results → pass/fail summary with expandable details
- **ErrorBlock**: errors → red card with stack trace (collapsed by default)
- **TextBlock**: everything else → styled text

Each block is collapsible. Newest at bottom. Auto-scroll with "jump to bottom" button.

**Raw terminal toggle**: Button in top-right of AgentView. When active, renders xterm.js with full PTY. User can type directly into the agent.

#### AgentBar.tsx
- Horizontal bar at bottom
- Tab for each agent: icon + name + status dot
- Click tab = switch AgentView to that agent
- [+] button → spawn dialog (select model, enter task)
- Cmd+K → CommandPalette overlay

#### CommandPalette.tsx
- Modal overlay (like VS Code Cmd+K)
- Actions: spawn agent, kill agent, switch agent, toggle raw terminal, switch view
- Fuzzy search

### Step 5: Keyboard Shortcuts (use-keyboard.ts)
| Key | Action |
|-----|--------|
| Cmd+K | Command palette |
| Cmd+T | New agent |
| Cmd+W | Close/kill focused agent |
| Cmd+1-9 | Switch to agent N |
| Cmd+[ / Cmd+] | Previous/next agent |
| Cmd+R | Toggle raw terminal |
| Cmd+D | Toggle DAG panel |
| Escape | Close palette / unfocus terminal |

### Step 6: Styling
- Dark theme (zinc-900 background, zinc-800 panels)
- Monospace font for output (JetBrains Mono or system mono)
- Status colors: green (#22c55e), blue (#3b82f6), red (#ef4444), yellow (#eab308), gray (#6b7280)
- Smooth transitions, no janky resizes
- Tailwind CSS throughout

### Step 7: Build & Package
```bash
npm run tauri build
```
This produces `target/release/bundle/macos/Ristretto.app`

## Critical Constraints
1. The app must connect to ristd daemon via Unix socket. If ristd is not running, show a "Start Daemon" button.
2. All types must match rist-shared types (AgentInfo, Task, TaskStatus, etc.) — read `../rist-shared/src/types.rs` for the canonical definitions.
3. The structured output parser is best-effort — unrecognized output falls through as TextBlock.
4. xterm.js is lazy-loaded (only when user toggles raw mode).
5. Dark theme only for v0.3.
6. macOS only for v0.3.

## Verification
After building:
1. `npm run tauri dev` — app launches, shows "Connecting to daemon..." or "Daemon not running"
2. If ristd is running: agents list populates, DAG renders, clicking agent shows output
3. Cmd+K opens command palette
4. Cmd+T opens spawn dialog
5. Raw terminal toggle works
6. No console errors, no panics
