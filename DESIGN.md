# Ristretto — Architecture & Design Document
*The most concentrated shot of multi-agent orchestration*

> Ristretto is an open-source, terminal-native, agent-agnostic orchestrator for running 
> multiple code agents (Claude Code, Codex, Gemini CLI, any CLI) in parallel with 
> planner-driven task decomposition, file ownership, and daemon persistence.

---

## Table of Contents

1. [Vision & Positioning](#1-vision--positioning)
2. [Architecture Overview](#2-architecture-overview)
3. [Binary Structure](#3-binary-structure)
4. [Daemon (`ristd`)](#4-daemon-ristd)
5. [TUI (`rist`)](#5-tui-rist)
6. [MCP Server (`rist-mcp`)](#6-mcp-server-rist-mcp)
7. [Channel Server (`rist-channel`)](#7-channel-server-rist-channel)
8. [IPC Protocol](#8-ipc-protocol)
9. [Agent Adapters](#9-agent-adapters)
10. [Planner System](#10-planner-system)
11. [File Ownership Model](#11-file-ownership-model)
12. [Task Graph](#12-task-graph)
13. [Git Integration](#13-git-integration)
14. [Inter-Agent Communication](#14-inter-agent-communication)
15. [Activity Detection](#15-activity-detection)
16. [Context-Aware Scheduling](#16-context-aware-scheduling)
17. [i18n (EN/CN)](#17-i18n-encn)
18. [Configuration](#18-configuration)
19. [Directory Layout](#19-directory-layout)
20. [Crate Structure](#20-crate-structure)
21. [Implementation Phases](#21-implementation-phases)

---

## 1. Vision & Positioning

### Problem
The bottleneck in AI-assisted development is no longer the individual agent's capability. 
It's the **orchestration layer** — coordinating multiple agents, preventing conflicts, 
managing context exhaustion, and providing visibility.

### Solution
Ristretto extracts maximum output from minimum context. Like its namesake — a short shot 
of espresso with the highest concentration — Ristretto breaks big tasks into focused, 
bounded agent sessions that each fit in one context window.

### Positioning
- **Open source** (MIT license)
- **Terminal-first** — works over SSH, no GUI/Electron
- **Agent-agnostic** — Claude Code, Codex, Gemini CLI, any CLI
- **Planner-driven** — AI decomposes tasks; humans approve; agents execute
- **Production-grade** — daemon persistence, crash recovery, zero-GC

### Competitive Landscape

| | Raccoon | Conductor | CC Agent Teams | claude-peers-mcp | **Ristretto** |
|---|---|---|---|---|---|
| Interface | Tauri GUI | macOS GUI | Terminal | Terminal | **TUI (SSH)** |
| Agents | CC only | CC + Codex | CC only | CC only | **Any CLI** |
| Orchestration | MCP planner | Manual | Team lead | Peer-to-peer | **MCP planner** |
| File ownership | ❌ | ❌ | ❌ | ❌ | **✅** |
| Daemon persist | ✅ | ❌ | ❌ | Broker only | **✅** |
| Context mgmt | ❌ | ❌ | ❌ | ❌ | **✅** |
| Channel push | ❌ | ❌ | ❌ | Via MCP poll | **✅ native** |
| Open source | ❌ | ❌ | ✅ (CC part) | ✅ | **✅** |
| Cross-platform | macOS | macOS | All | All | **All** |

---

## 2. Architecture Overview

```
                          Human
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                      rist (TUI)                              │
│   ┌──────────┬───────────┬───────────┬────────────────────┐ │
│   │ Sidebar  │ Agent 1   │ Agent 2   │ Planner / Status   │ │
│   │          │ (PTY)     │ (PTY)     │                    │ │
│   │ [1] ● CC │           │           │ Task Graph         │ │
│   │ [2] ◐ CX │           │           │ File Ownership     │ │
│   │ [3] ○ GM │           │           │ Progress           │ │
│   │ [P] ★ PL │           │           │                    │ │
│   └──────────┴───────────┴───────────┴────────────────────┘ │
└──────────────────────────┬──────────────────────────────────┘
                           │ ~/.ristretto/daemon.sock
                           │ (4-byte len + JSON frames)
┌──────────────────────────▼──────────────────────────────────┐
│                    ristd (Daemon)                             │
│                                                              │
│  ┌────────────┐ ┌─────────────┐ ┌──────────────────────────┐│
│  │ PTY Manager│ │ Session     │ │ Message Bus              ││
│  │            │ │ Store       │ │                          ││
│  │ spawn/reap │ │ state/meta  │ │ planner ↔ agent          ││
│  │ ring buf   │ │ worktrees   │ │ agent ↔ agent (via fs)   ││
│  │ 64MB/sess  │ │ ownership   │ │ broadcast                ││
│  └────────────┘ └─────────────┘ └──────────────────────────┘│
│                                                              │
│  ┌──────────────────────┐ ┌────────────────────────────────┐│
│  │ rist-mcp (STDIO)     │ │ rist-channel (MCP channel)     ││
│  │ Planner tools        │ │ Push events to CC sessions     ││
│  │ per-spawn lifecycle  │ │                                ││
│  └──────────────────────┘ └────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

### Four binaries, one workspace:
1. **`ristd`** — daemon (long-running, owns PTYs)
2. **`rist`** — TUI client (connects to daemon, renders terminal)
3. **`rist-mcp`** — MCP server (STDIO, planner agent's tools)
4. **`rist-channel`** — MCP channel server (pushes events to CC sessions)

---

## 3. Binary Structure

| Binary | Lifetime | Restart Cost | Purpose |
|--------|----------|-------------|---------|
| `ristd` | Hours/days | **HIGH** — kills all PTYs | PTY owner, session state, IPC hub |
| `rist` | Minutes/hours | **FREE** — reattach | TUI rendering, user input |
| `rist-mcp` | Per-planner session | **FREE** — reconnects | MCP tools for planner agent |
| `rist-channel` | Per-CC session | **FREE** — reregisters | Push events to CC sessions |

Key insight from raccoon: **daemon owns all state; everything else reconnects.**

---

## 4. Daemon (`ristd`)

### 4.1 PTY Manager

Spawns and manages agent processes via `portable-pty`.

```rust
pub struct PtyManager {
    sessions: HashMap<SessionId, PtySession>,
}

pub struct PtySession {
    id: SessionId,
    master_fd: Box<dyn MasterPty>,      // portable-pty master
    child: Box<dyn Child>,              // child process handle
    ring_buffer: VecDeque<u8>,          // 64MB ring buffer
    status: AgentStatus,
    agent_type: AgentType,
    task: String,
    worktree: PathBuf,
    created_at: Instant,
    last_output: Instant,
    file_ownership: HashSet<PathBuf>,   // files this agent owns
    ui_sinks: Vec<UnboundedSender<PtyEvent>>,  // fan-out to TUI clients
}
```

**PTY spawn flow:**
1. Create git worktree (if repo-based)
2. Write agent context file (`RISTRETTO.md`) to worktree
3. Build command via AgentAdapter
4. `native_pty_system().openpty()` → spawn command
5. Spawn OS reader thread → push to ring buffer + all ui_sinks
6. Register session in SessionStore

**Ring buffer:** 64MB `VecDeque<u8>` per session. O(1) push_back, O(1) drain. 
On TUI reconnect, drain from ring buffer to reattach scrollback.

**Orphan reaper:** On daemon shutdown or agent kill:
1. Send SIGTERM to process group
2. Wait 2 seconds
3. Send SIGKILL if still alive
4. `waitpid()` to collect zombie
5. Optional: preserve worktree or clean up

### 4.2 Session Store

Persistent state for all sessions.

```rust
pub struct SessionStore {
    sessions: Vec<SessionMeta>,
    path: PathBuf,  // ~/.ristretto/sessions.json
}

pub struct SessionMeta {
    id: SessionId,
    agent_type: AgentType,
    task: String,
    status: AgentStatus,
    worktree: PathBuf,
    branch: String,
    file_ownership: Vec<PathBuf>,
    created_at: DateTime<Utc>,
    context_usage: Option<ContextUsage>,  // estimated tokens used
    exit_code: Option<i32>,
}
```

**Persistence:** Atomic write via tmp file + fsync + rename (raccoon's pattern).
Write on every state change. Daemon restart re-reads + reattaches to running PTYs where possible.

### 4.3 Message Bus

Append-only JSONL log + per-agent inbox for structured messages.

```rust
pub struct MessageBus {
    log: File,  // ~/.ristretto/messages.jsonl
    inboxes: HashMap<SessionId, VecDeque<Message>>,
}

pub struct Message {
    id: MessageId,
    from: SessionId,        // "planner" or agent id
    to: SessionId,          // target agent id
    content: String,
    timestamp: DateTime<Utc>,
    msg_type: MessageType,  // Task, Question, StatusUpdate, Completion, Error
}
```

### 4.4 Socket Server

Unix domain socket at `~/.ristretto/daemon.sock`.

**Wire format:** 4-byte big-endian length prefix + JSON frame (raccoon's exact protocol).

```
┌──────────┬─────────────────────┐
│ 4 bytes  │  JSON payload       │
│ (BE u32) │  (UTF-8)            │
│ len      │  { "type": "...",   │
│          │    "data": {...} }  │
└──────────┴─────────────────────┘
```

Multiple clients can connect simultaneously (TUI + multiple rist-mcp instances).

---

## 5. TUI (`rist`)

### 5.1 Layout

```
┌─────────────────────────────────────────────────────────────┐
│ Ristretto ☕                                    3 agents  ▶ │ ← Status bar
├──────────┬──────────────────────────────────────────────────┤
│          │                                                  │
│  Agents  │  Active Agent Terminal (PTY output)              │
│          │                                                  │
│  ● auth  │  $ claude --print "implement OAuth2..."          │
│  ◐ db    │  > Reading src/auth/mod.rs...                    │
│  ○ tests │  > Writing src/auth/oauth2.rs...                 │
│  ★ plan  │  > ...                                           │
│          │                                                  │
│          ├──────────────────────────────────────────────────┤
│  ──────  │                                                  │
│  Tasks   │  Planner / Task Graph                            │
│          │                                                  │
│  ✅ schema│  [1] ✅ Design DB schema (codex) — done          │
│  🔄 auth │  [2] 🔄 Implement auth API (claude) — working    │
│  ⏳ tests │  [3] ⏳ Write integration tests (claude) — queue  │
│  ⏳ front │  [4] ⏳ Build login frontend (gemini) — queue     │
│          │                                                  │
├──────────┴──────────────────────────────────────────────────┤
│ [N]ew [K]ill [1-9]Focus [D]split [M]erge [P]lan [?]Help    │ ← Keybind bar
└─────────────────────────────────────────────────────────────┘
```

### 5.2 Keybindings

| Key | Action |
|-----|--------|
| `N` | New agent (interactive prompt: type, task, repo) |
| `K` | Kill focused agent |
| `1-9` | Focus agent by number |
| `Tab` | Cycle agent focus |
| `D` | Toggle split view (horizontal/vertical) |
| `P` | Open planner panel |
| `M` | Merge focused agent's worktree |
| `T` | Toggle task graph view |
| `F` | File ownership map |
| `/` | Search across all agent outputs |
| `?` | Help / command palette |
| `Enter` | Write to focused agent's PTY stdin |
| `Ctrl-C` | Send SIGINT to focused agent |
| `Q` | Quit TUI (daemon keeps running) |

### 5.3 Status Indicators

| Symbol | Color | State | Detection |
|--------|-------|-------|-----------|
| `●` | Green | Working | Output in last 3s |
| `◐` | Blue | Thinking | Tool-call pattern detected |
| `◉` | Amber | Waiting | Input prompt detected (`? (y/n)`, etc.) |
| `⊘` | Red | Stuck | Working but no output >5min, or loop detected |
| `○` | Grey | Idle | No output >3s after prompt pattern |
| `✓` | Green | Done | Process exited code 0 |
| `✗` | Red | Error | Process exited non-zero |

### 5.4 Rendering

- **ratatui** with crossterm backend
- Unicode box drawing for borders
- Truecolor support with fallback to 256-color
- CJK text width via `unicode-width` crate
- 60fps equivalent refresh rate (event-driven, not polling)

---

## 6. MCP Server (`rist-mcp`)

STDIO JSONRPC server that provides orchestration tools to the planner agent.
Connects to `ristd` via daemon socket on each tool call.

### 6.1 Tools (12 tools — raccoon-inspired + Ristretto additions)

```json
{
  "tools": [
    {
      "name": "spawn_agent",
      "description": "Spawn a new coding agent with a specific task. Agents run in isolated git worktrees. Choose the best agent_type for each task: 'claude' for complex architecture, 'codex' for fast implementation, 'gemini' for large-context analysis, 'custom' for specialized CLIs.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "agent_type": { "type": "string", "enum": ["claude", "codex", "gemini", "custom"] },
          "task": { "type": "string", "description": "Clear, focused task description" },
          "repo_path": { "type": "string", "description": "Path to git repo (optional)" },
          "file_ownership": { 
            "type": "array", "items": { "type": "string" },
            "description": "Files/directories this agent exclusively owns. Other agents cannot modify these." 
          },
          "depends_on": {
            "type": "array", "items": { "type": "string" },
            "description": "Task IDs this task depends on (agent won't start until deps complete)"
          },
          "context_files": {
            "type": "array", "items": { "type": "string" },
            "description": "Files to inject into agent's worktree as read-only context"
          }
        },
        "required": ["agent_type", "task"]
      }
    },
    {
      "name": "list_agents",
      "description": "List all running and completed agents with their status, task, output summary, and file ownership."
    },
    {
      "name": "get_agent_output",
      "description": "Read recent terminal output from an agent. Use to monitor progress, detect issues, or gather results.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "agent_id": { "type": "string" },
          "lines": { "type": "integer", "default": 50 },
          "since": { "type": "string", "description": "ISO timestamp, only output after this time" }
        },
        "required": ["agent_id"]
      }
    },
    {
      "name": "write_to_agent",
      "description": "Send text to an agent's terminal input (stdin). Use for providing additional instructions, answering questions, or inter-agent coordination.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "agent_id": { "type": "string" },
          "text": { "type": "string" }
        },
        "required": ["agent_id", "text"]
      }
    },
    {
      "name": "archive_agent",
      "description": "Mark an agent as complete and archive its session. Worktree is preserved for merge.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "agent_id": { "type": "string" },
          "keep_worktree": { "type": "boolean", "default": true }
        },
        "required": ["agent_id"]
      }
    },
    {
      "name": "wait_for_idle",
      "description": "Block until an agent becomes idle (no output for settling_secs). Use after giving instructions to wait for completion.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "agent_id": { "type": "string" },
          "timeout_secs": { "type": "integer", "default": 300 },
          "settling_secs": { "type": "integer", "default": 5, "description": "Seconds of silence before considered idle" }
        },
        "required": ["agent_id"]
      }
    },
    {
      "name": "run_command",
      "description": "Execute a shell command in an agent's worktree. Use for inspection (ls, grep, diff, git status) without interrupting the agent.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "agent_id": { "type": "string" },
          "command": { "type": "string" }
        },
        "required": ["agent_id", "command"]
      }
    },
    {
      "name": "read_task_graph",
      "description": "Read the current task decomposition graph with status, dependencies, and assignments."
    },
    {
      "name": "write_task_graph",
      "description": "Update the task decomposition plan. Use at the start to break down the objective, then update as tasks complete.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "tasks": {
            "type": "array",
            "items": {
              "type": "object",
              "properties": {
                "id": { "type": "string" },
                "title": { "type": "string" },
                "description": { "type": "string" },
                "status": { "type": "string", "enum": ["pending", "assigned", "working", "review", "done", "blocked"] },
                "agent_id": { "type": "string" },
                "agent_type": { "type": "string" },
                "depends_on": { "type": "array", "items": { "type": "string" } },
                "file_ownership": { "type": "array", "items": { "type": "string" } },
                "priority": { "type": "string", "enum": ["critical", "high", "medium", "low"] }
              }
            }
          }
        },
        "required": ["tasks"]
      }
    },
    {
      "name": "get_file_ownership",
      "description": "View the current file ownership map. Shows which agent owns which files/directories."
    },
    {
      "name": "merge_agent",
      "description": "Merge an agent's worktree changes back to the main branch. Returns diff preview first, then merges on confirmation.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "agent_id": { "type": "string" },
          "preview_only": { "type": "boolean", "default": true },
          "strategy": { "type": "string", "enum": ["merge", "rebase", "squash"], "default": "squash" }
        },
        "required": ["agent_id"]
      }
    },
    {
      "name": "request_review",
      "description": "Spawn a review agent (different model recommended) to independently validate an agent's work.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "agent_id": { "type": "string", "description": "Agent whose work to review" },
          "reviewer_type": { "type": "string", "enum": ["claude", "codex", "gemini"], "description": "Use a different model than the original agent" },
          "review_scope": { "type": "string", "enum": ["full", "tests", "security", "performance"], "default": "full" }
        },
        "required": ["agent_id"]
      }
    }
  ]
}
```

### 6.2 Tool Design Principles (from Thariq)
- **Keep tools minimal** — 12 tools max. Use filesystem for everything else.
- **Tools are fixed at spawn** — never add/remove tools mid-session.
- **Descriptions are for the model** — they're trigger conditions, not API docs.
- **Model-friendly naming** — `spawn_agent` not `create_pty_session`.

---

## 7. Channel Server (`rist-channel`)

MCP server implementing `claude/channel` capability for pushing real-time events into 
Claude Code sessions. This enables Claude Code users to receive Ristretto events 
without polling.

### 7.1 Event Types

```typescript
// Push to Claude Code session as <channel source="ristretto" ...>
interface RistrettoEvent {
  type: "agent_completed" | "agent_stuck" | "agent_error" | 
        "plan_ready" | "review_ready" | "merge_conflict" |
        "context_warning" | "task_update";
  agent_id?: string;
  task?: string;
  summary: string;
  details?: string;
}
```

### 7.2 Architecture

```
Claude Code session
    │
    ├── rist-mcp (tools — CC calls these)
    │       │
    │       └── daemon.sock → ristd
    │
    └── rist-channel (push — events arrive automatically)
            │
            └── daemon.sock → ristd (subscribe to events)
```

### 7.3 Integration

```json
// .mcp.json in project root or ~/.claude.json
{
  "mcpServers": {
    "ristretto": {
      "command": "rist-mcp",
      "args": []
    },
    "ristretto-channel": {
      "command": "rist-channel", 
      "args": []
    }
  }
}
```

Claude Code launched with:
```bash
claude --channels server:ristretto-channel
```

---

## 8. IPC Protocol

### 8.1 Wire Format
4-byte big-endian length prefix + UTF-8 JSON frame. Identical to raccoon.

### 8.2 Request/Response Messages

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Request {
    Ping,
    SpawnAgent { agent_type: AgentType, task: String, repo_path: Option<PathBuf>, file_ownership: Vec<PathBuf> },
    KillAgent { id: SessionId },
    ListAgents,
    GetOutput { id: SessionId, lines: usize },
    WriteToAgent { id: SessionId, text: String },
    ArchiveAgent { id: SessionId, keep_worktree: bool },
    WaitForIdle { id: SessionId, timeout_secs: u64, settling_secs: u64 },
    RunCommand { id: SessionId, command: String },
    ReadTaskGraph,
    WriteTaskGraph { tasks: Vec<Task> },
    GetFileOwnership,
    MergeAgent { id: SessionId, preview_only: bool, strategy: MergeStrategy },
    RequestReview { agent_id: SessionId, reviewer_type: AgentType, scope: ReviewScope },
    Subscribe { events: Vec<EventFilter> },  // for rist-channel
    GetBuffer { id: SessionId },  // for TUI reattach
    Resize { id: SessionId, cols: u16, rows: u16 },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Response {
    Pong { version: String },
    AgentSpawned { id: SessionId },
    AgentList { agents: Vec<AgentInfo> },
    Output { lines: Vec<String> },
    TaskGraph { tasks: Vec<Task> },
    FileOwnership { map: HashMap<PathBuf, SessionId> },
    MergePreview { diff: String, conflicts: Vec<String> },
    MergeResult { success: bool, message: String },
    CommandOutput { stdout: String, stderr: String, exit_code: i32 },
    Ok,
    Error { message: String },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Event {
    PtyData { id: SessionId, data: Vec<u8> },
    StatusChange { id: SessionId, old: AgentStatus, new: AgentStatus },
    AgentExited { id: SessionId, exit_code: i32 },
    TaskUpdate { task_id: String, status: TaskStatus },
    ContextWarning { id: SessionId, usage_pct: f64 },
    LoopDetected { id: SessionId, pattern: String },
    #[serde(other)]
    Unknown,  // Forward-compatible (raccoon pattern)
}
```

### 8.3 Forward Compatibility
Every enum has `#[serde(other)] Unknown` — new message types don't break old binaries.
Version handshake on connect. Daemon and TUI can be different versions.

---

## 9. Agent Adapters

```rust
pub trait AgentAdapter: Send + Sync {
    /// Human-readable name
    fn name(&self) -> &str;
    
    /// Build the spawn command for this agent type
    fn build_command(
        &self,
        task: &str,
        workdir: &Path,
        env: &HashMap<String, String>,
        mcp_config: Option<&Path>,  // for agents that support MCP
    ) -> CommandBuilder;
    
    /// Detect agent status from recent output
    fn detect_status(&self, recent_output: &[u8], elapsed_since_output: Duration) -> AgentStatus;
    
    /// Detect if agent is in a loop
    fn detect_loop(&self, recent_output: &[u8]) -> Option<String>;
    
    /// Parse context usage from output (if agent reports it)
    fn parse_context_usage(&self, output: &[u8]) -> Option<ContextUsage>;
    
    /// Generate the context injection file content (RISTRETTO.md)
    fn context_file_content(&self, agent_info: &AgentInfo, peers: &[AgentInfo], task_graph: &TaskGraph) -> String;
}
```

### Built-in Adapters

**ClaudeCodeAdapter:**
```rust
fn build_command(&self, task, workdir, env, mcp_config) -> CommandBuilder {
    let mut cmd = Command::new("claude");
    cmd.arg("--print");  // non-interactive for planner-spawned
    // or interactive mode for TUI-spawned
    if let Some(mcp) = mcp_config {
        cmd.arg("--mcp-config").arg(mcp);
    }
    cmd.arg(task);
    cmd.current_dir(workdir);
    cmd
}

fn detect_status(&self, output, elapsed) -> AgentStatus {
    // Claude Code specific patterns:
    // "⏺" = working
    // "?" prompts = waiting
    // Exit patterns = done
}
```

**CodexAdapter:**
```rust
fn build_command(&self, task, workdir, env, _mcp) -> CommandBuilder {
    Command::new("codex")
        .arg("--task").arg(task)
        .arg("--auto-approve")  // or interactive
        .current_dir(workdir)
}
```

**GeminiAdapter:**
```rust
fn build_command(&self, task, workdir, env, _mcp) -> CommandBuilder {
    Command::new("gemini")
        .arg(task)
        .current_dir(workdir)
}
```

**CustomAdapter:**
```rust
// Configured via ristretto.toml
// [agents.myagent]
// command = "my-agent"
// args = ["--task", "{task}", "--workdir", "{workdir}"]
// detect_idle_pattern = "^>"
// detect_working_pattern = "Thinking..."
```

---

## 10. Planner System

### 10.1 How It Works

The planner is itself an agent (Claude Code or any capable model) with `rist-mcp` tools 
injected via MCP config. It receives a high-level objective and orchestrates workers.

```
User: "Build a user authentication system with OAuth2, database, and frontend"
  │
  ▼
Planner (Claude Code + rist-mcp tools):
  1. write_task_graph([
       { id: "T1", title: "Design DB schema", agent_type: "codex", files: ["src/db/"] },
       { id: "T2", title: "Implement OAuth2 API", agent_type: "claude", files: ["src/auth/"], depends: ["T1"] },
       { id: "T3", title: "Build login frontend", agent_type: "gemini", files: ["src/ui/"], depends: ["T1"] },
       { id: "T4", title: "Integration tests", agent_type: "claude", depends: ["T2", "T3"] },
     ])
  2. spawn_agent("codex", "Design DB schema for user auth...", files: ["src/db/"])
  3. wait_for_idle("agent-1")
  4. get_agent_output("agent-1") → review results
  5. spawn_agent("claude", "Implement OAuth2...", files: ["src/auth/"])
  6. spawn_agent("gemini", "Build login page...", files: ["src/ui/"])  // parallel!
  7. wait_for_idle("agent-2") + wait_for_idle("agent-3")
  8. request_review("agent-2", reviewer_type: "codex")  // cross-model review
  9. merge_agent("agent-1") → merge_agent("agent-2") → merge_agent("agent-3")
```

### 10.2 Planner System Prompt

Stored at `~/.ristretto/planner-prompt.md`. Injected via `--append-system-prompt`.

Key instructions:
- Always decompose before executing
- Assign file ownership to prevent conflicts
- Use different agent types for implementation vs review
- Monitor agents periodically (`get_agent_output`)
- Rotate agents approaching context limits
- Never change an agent's tools or system prompt mid-session
- Present plan to human for approval before spawning agents

### 10.3 Human-in-the-Loop

The planner proposes; the human approves:
1. Planner writes task graph
2. TUI displays plan with file ownership map
3. Human reviews: approve, modify, or reject
4. Only then does the planner spawn agents

This is critical — fully autonomous multi-agent systems make costly mistakes.

---

## 11. File Ownership Model

**The core innovation that prevents merge hell (Pain Point #2).**

### 11.1 Rules

1. Before spawning agents, planner declares file ownership in the task graph
2. Each file/directory can be owned by exactly ONE agent at a time
3. Agents receive their ownership list in `RISTRETTO.md` context file
4. The daemon enforces ownership: `run_command` and `merge_agent` check for violations
5. Shared files (e.g., `package.json`) can be designated as "shared" — changes require planner coordination

### 11.2 Ownership Map

```json
{
  "ownership": {
    "src/db/": "agent-1",
    "src/auth/": "agent-2",
    "src/ui/": "agent-3",
    "src/shared/types.ts": "shared"
  },
  "interfaces": {
    "src/auth/types.ts": {
      "owner": "agent-2",
      "contract": "Must export: User, Session, AuthResult types",
      "consumers": ["agent-1", "agent-3"]
    }
  }
}
```

### 11.3 Interface Contracts

For cross-agent dependencies, the planner defines interface contracts:
- Agent A defines types/interfaces first
- Agent B and C code against those interfaces
- At merge time, interfaces are verified

---

## 12. Task Graph

Directed acyclic graph (DAG) of tasks with dependencies, status, and agent assignment.

```rust
pub struct TaskGraph {
    tasks: Vec<Task>,
    created_at: DateTime<Utc>,
    objective: String,
}

pub struct Task {
    id: String,
    title: String,
    description: String,
    status: TaskStatus,       // pending, assigned, working, review, done, blocked
    agent_id: Option<SessionId>,
    agent_type: Option<AgentType>,
    depends_on: Vec<String>,  // task IDs
    file_ownership: Vec<PathBuf>,
    priority: Priority,
    estimated_complexity: Option<Complexity>,  // simple, moderate, complex
    result_summary: Option<String>,
}

pub enum TaskStatus {
    Pending,      // not yet started
    Assigned,     // agent spawned
    Working,      // agent actively producing output
    Review,       // under review by another agent
    Done,         // completed and verified
    Blocked,      // dependency not met or error
}
```

### Scheduling Rules
1. A task can only start when ALL `depends_on` tasks are `Done`
2. Independent tasks can run in parallel (up to a configurable concurrency limit)
3. The planner re-evaluates the graph after each task completion
4. Failed tasks can be retried with a different agent type

---

## 13. Git Integration

### 13.1 Worktree Management

```
my-project/                          # main working tree
├── .git/
├── .ristretto/
│   └── worktrees/
│       ├── agent-abc123/            # git worktree for agent 1
│       │   ├── RISTRETTO.md         # agent context (injected)
│       │   └── ... (full repo copy)
│       ├── agent-def456/            # git worktree for agent 2
│       └── agent-ghi789/            # git worktree for agent 3
└── src/
```

### 13.2 Workflow

1. **Spawn:** `git worktree add .ristretto/worktrees/<agent-id> -b rist/<agent-id>-<task-slug>`
2. **Work:** Agent works in isolated worktree
3. **Merge:** `git checkout main && git merge --squash rist/<agent-id>-<task-slug>`
4. **Cleanup:** `git worktree remove .ristretto/worktrees/<agent-id>`

### 13.3 Conflict Detection

Before merge, daemon runs:
```bash
git merge-tree $(git merge-base main rist/<branch>) main rist/<branch>
```
If conflicts detected → surface to planner → planner coordinates resolution.

---

## 14. Inter-Agent Communication

Three patterns, from most common to least:

### 14.1 File-System Communication (Primary)
Agents write results to their worktree. Planner reads across all worktrees.
- Agent writes `PROGRESS.md` in its worktree (auto-appended)
- Planner uses `run_command(agent_id, "cat PROGRESS.md")` to check
- Shared context: `~/.ristretto/shared/` directory (all agents can read)

### 14.2 Planner Relay (Structured)
Planner reads from one agent, writes to another:
```
get_agent_output("agent-1") → extract API schema
write_to_agent("agent-2", "Agent 1 defined these API types: ...")
```

### 14.3 Channel Push (Real-time)
Via `rist-channel`, events push to Claude Code sessions without polling:
```
Agent B completes → ristd emits Event::AgentExited
→ rist-channel pushes <channel source="ristretto">Agent B completed: auth API ready</channel>
→ Claude Code (planner) receives event immediately
```

---

## 15. Activity Detection

### 15.1 Output-Based Heuristics

```rust
pub fn detect_activity(
    output: &[u8],
    elapsed: Duration,
    agent_type: &AgentType,
) -> AgentStatus {
    // Universal patterns
    if elapsed > Duration::from_secs(300) && !has_recent_tool_call(output) {
        return AgentStatus::Stuck;
    }
    if matches_input_prompt(output) {
        return AgentStatus::Waiting;
    }
    if elapsed < Duration::from_secs(3) {
        return AgentStatus::Working;
    }
    
    // Agent-specific detection via adapter
    adapter.detect_status(output, elapsed)
}
```

### 15.2 Loop Detection

Same tool/command repeated 5+ times in a row (raccoon's heuristic):
```rust
pub fn detect_loop(recent_commands: &[String]) -> Option<String> {
    if recent_commands.len() >= 5 {
        let last = &recent_commands[recent_commands.len() - 1];
        if recent_commands.iter().rev().take(5).all(|c| c == last) {
            return Some(format!("Agent repeated '{}' 5 times", last));
        }
    }
    None
}
```

When loop detected → `AgentStatus::Stuck` + event to planner/TUI.

---

## 16. Context-Aware Scheduling

### 16.1 Context Usage Estimation

Since we can't read Claude's internal token count, estimate from output volume:
- Track total bytes written to and read from the agent
- Estimate tokens ≈ bytes / 4 (rough heuristic)
- Agent-specific: Claude Code sometimes reports usage in output
- Track time-in-session as a secondary proxy

### 16.2 Rotation Strategy

When context usage exceeds threshold (default 80%):
1. Signal planner: `ContextWarning { id, usage_pct: 82.0 }`
2. Planner creates a structured summary of agent's progress so far
3. Archive current agent (keep worktree)
4. Spawn new agent with summary + same worktree
5. New agent continues with fresh context

### 16.3 Why This Matters (Thariq insight)
Each agent has a stable prefix (system prompt + tools + project context). 
Context rotation at 80% means the new agent gets cache hits on this static prefix.
Waiting until 100% means the agent degrades before rotation.

---

## 17. i18n (EN/CN)

### 17.1 Implementation

Using `rust-i18n` crate with compile-time locale loading.

```rust
// shared/src/i18n.rs
rust_i18n::i18n!("locales");

// locales/en.toml
[agent]
spawning = "Spawning %{agent_type} agent for: %{task}"
completed = "Agent %{id} completed (exit code %{code})"
stuck = "⚠️ Agent %{id} appears stuck (no output for %{minutes}min)"
loop_detected = "🔄 Agent %{id} in loop: %{pattern}"

[planner]
decomposing = "Breaking down task into subtasks..."
plan_ready = "Plan ready — %{count} tasks, %{parallel} parallelizable"
approve = "Review plan and press Enter to approve, or 'e' to edit"

[tui]
status_bar = "Ristretto ☕ — %{count} agents"
help = "Press ? for help"

// locales/zh-CN.toml
[agent]
spawning = "正在启动 %{agent_type} 代理: %{task}"
completed = "代理 %{id} 已完成 (退出码 %{code})"
stuck = "⚠️ 代理 %{id} 疑似卡住 (已 %{minutes} 分钟无输出)"
loop_detected = "🔄 代理 %{id} 陷入循环: %{pattern}"

[planner]
decomposing = "正在分解任务..."
plan_ready = "计划就绪 — %{count} 个子任务，%{parallel} 个可并行"
approve = "检查计划后按 Enter 批准，按 'e' 编辑"

[tui]
status_bar = "Ristretto ☕ — %{count} 个代理"
help = "按 ? 查看帮助"
```

### 17.2 Language Detection
1. Check `RISTRETTO_LANG` env var
2. Check `LANG` / `LC_ALL` env vars
3. Default to `en`

---

## 18. Configuration

`~/.ristretto/config.toml` (or project-level `.ristretto/config.toml`)

```toml
[daemon]
socket_path = "~/.ristretto/daemon.sock"
ring_buffer_size = "64MB"
max_concurrent_agents = 10
auto_cleanup_worktrees = false

[planner]
default_model = "claude"
system_prompt = "~/.ristretto/planner-prompt.md"
auto_approve = false  # require human approval for plans
max_retries = 2

[agents.claude]
command = "claude"
args = ["--print"]
detect_idle_secs = 3
detect_stuck_mins = 5

[agents.codex]
command = "codex"
args = ["--auto-approve"]
detect_idle_secs = 5
detect_stuck_mins = 7

[agents.gemini]
command = "gemini"
args = []
detect_idle_secs = 3
detect_stuck_mins = 5

[agents.custom_myagent]
command = "/path/to/my-agent"
args = ["--task", "{task}"]
working_pattern = "Thinking..."
idle_pattern = "^>"

[git]
auto_worktree = true
merge_strategy = "squash"
branch_prefix = "rist/"
cleanup_on_archive = false

[context]
rotation_threshold = 0.8  # rotate at 80% context usage
token_estimate_ratio = 4  # bytes per token estimate

[tui]
language = "en"  # or "zh-CN"
color_mode = "truecolor"  # or "256" or "basic"
default_layout = "sidebar-split"  # or "sidebar-single" or "grid"

[channel]
enabled = true
event_types = ["agent_completed", "agent_stuck", "plan_ready", "merge_conflict"]
```

---

## 19. Directory Layout

```
~/.ristretto/
├── daemon.sock           # Unix domain socket
├── daemon.pid            # PID file with flock
├── config.toml           # Global config
├── sessions.json         # Persistent session state
├── messages.jsonl        # Message bus log
├── planner-prompt.md     # Default planner system prompt
├── shared/               # Shared context (all agents can read)
│   ├── interfaces/       # Interface contracts
│   └── context/          # Shared knowledge files
├── logs/
│   ├── daemon.log        # Daemon log
│   └── agents/           # Per-agent logs
│       ├── abc123.log
│       └── def456.log
└── cache/
    └── adapters/         # Cached adapter configs
```

Project-level:
```
my-project/
├── .ristretto/
│   ├── config.toml       # Project-level overrides
│   ├── task-graph.json   # Current task decomposition
│   ├── ownership.json    # File ownership map
│   └── worktrees/        # Git worktrees for agents
│       ├── agent-abc123/
│       └── agent-def456/
└── ...
```

---

## 20. Crate Structure

```toml
[workspace]
members = ["ristd", "rist", "rist-mcp", "rist-channel", "rist-shared"]
resolver = "2"

# rist-shared — IPC types, protocol, i18n
# ristd — daemon binary
# rist — TUI binary  
# rist-mcp — MCP server binary (STDIO)
# rist-channel — MCP channel server binary (STDIO)
```

### Dependency Graph
```
rist-shared (types, protocol, i18n)
    ↑            ↑           ↑            ↑
   ristd        rist      rist-mcp   rist-channel
```

### Key Dependencies

| Crate | Used By | Purpose |
|-------|---------|---------|
| `portable-pty` | ristd | PTY spawn/manage |
| `tokio` | all | Async runtime |
| `serde` + `serde_json` | all | Serialization |
| `ratatui` + `crossterm` | rist | TUI rendering |
| `nix` | ristd | Unix signals, flock |
| `tracing` | all | Structured logging |
| `rust-i18n` | rist-shared | EN/CN localization |
| `unicode-width` | rist | CJK text width |
| `git2` | ristd | Worktree management |
| `clap` | ristd, rist | CLI argument parsing |
| `uuid` | rist-shared | Session IDs |

---

## 21. Implementation Phases

### Phase 1: Foundation (Week 1-2)
**Goal: Daemon can spawn and manage PTY agents, TUI can display them**

- [ ] `rist-shared`: IPC types, protocol frame encoder/decoder, SessionId, AgentStatus
- [ ] `ristd`: Socket server, PTY manager (spawn/reap/ring-buffer), session store (in-memory + JSON persistence)
- [ ] `rist`: TUI connects to daemon, sidebar with agent list, single-pane PTY display, status indicators
- [ ] Agent adapters: Claude Code + Codex (basic)
- [ ] CLI: `rist start` (daemon), `rist` (TUI), `rist spawn claude "task"` (quick spawn)
- [ ] Tests: Unit tests for protocol, integration test for spawn → output → display

**Demo:** Open rist TUI, spawn 2 agents manually, see their output in split panes.

### Phase 2: Git & Orchestration (Week 3)
**Goal: Worktree isolation, MCP server, basic planner**

- [ ] Git worktree auto-creation and cleanup
- [ ] File ownership model (declare, enforce, display)
- [ ] `rist-mcp`: All 12 MCP tools, connects to daemon socket
- [ ] Planner system prompt (`planner-prompt.md`)
- [ ] Task graph (write/read/display in TUI)
- [ ] `wait_for_idle` with output settling
- [ ] Merge flow (preview + execute)

**Demo:** Give planner a complex task, it decomposes + spawns workers + merges results.

### Phase 3: Intelligence (Week 4)
**Goal: Activity detection, context management, channel push, review**

- [ ] Activity detection (idle/working/stuck/waiting/loop)
- [ ] Context-aware scheduling (usage estimation, rotation)
- [ ] Loop detection
- [ ] `rist-channel`: Push events to Claude Code sessions
- [ ] `request_review` tool (cross-model validation)
- [ ] Gemini CLI adapter
- [ ] Custom adapter support
- [ ] i18n (EN/CN throughout)

**Demo:** Full workflow — plan, spawn, detect stuck, rotate context, review, merge — with real-time channel push to CC.

### Phase 4: Polish & Launch (Week 5)
**Goal: Production quality, documentation, open source launch**

- [ ] Error handling, edge cases, crash recovery
- [ ] `rist doctor` — health check command
- [ ] README.md with demo GIF
- [ ] CONTRIBUTING.md
- [ ] GitHub Actions CI (build + test, Linux + macOS)
- [ ] Homebrew formula
- [ ] Crates.io publish
- [ ] Launch post (X, HN, Reddit)

---

## Appendix A: RISTRETTO.md (Agent Context Injection)

Every spawned agent gets this file in its worktree root:

```markdown
# Ristretto Agent Context

## Your Identity
- **Agent ID:** {{agent_id}}
- **Agent Type:** {{agent_type}}
- **Task:** {{task}}
- **Priority:** {{priority}}

## File Ownership
You exclusively own these files/directories:
{{#each file_ownership}}
- `{{this}}`
{{/each}}

⚠️ Do NOT modify files outside your ownership list.

## Other Agents
{{#each peers}}
- **{{this.id}}** ({{this.agent_type}}): "{{this.task}}" — {{this.status}}
  - Owns: {{this.file_ownership}}
{{/each}}

## Task Dependencies
{{#if depends_on}}
These tasks must complete before yours:
{{#each depends_on}}
- {{this.id}}: {{this.title}} — {{this.status}}
{{/each}}
{{/if}}

## Communication
- Write progress to `PROGRESS.md` in this directory
- To signal completion: write "RISTRETTO_DONE: <summary>" to stdout
- To signal blocker: write "RISTRETTO_BLOCKED: <reason>" to stdout
- Shared context available at: {{shared_dir}}

## Rules
1. Stay within your file ownership boundaries
2. Write clean, tested code
3. Update PROGRESS.md periodically
4. Signal completion when done
```

---

## Appendix B: Key Design Decisions Log

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust | Raccoon proves it, zero-GC for daemon, portable-pty ecosystem |
| TUI framework | ratatui | 12K stars, production-proven, SSH-friendly |
| IPC | Unix socket + len-prefix JSON | Raccoon's exact protocol, proven reliable |
| Planner integration | MCP server (STDIO) | Standard protocol, Claude Code native support |
| Event push | Claude Code Channels | Native push, no polling, official API |
| File ownership | Planner-declared, daemon-enforced | Solves merge hell (Pain Point #2) |
| Task model | DAG with dependencies | Thariq's evolution: TodoWrite → Task Tool |
| Agent detection | Output heuristics | Agent-agnostic, no internal API needed |
| Context rotation | 80% threshold | Cache-friendly (Thariq insight) |
| Merge strategy | Squash by default | Clean history, easier review |
| i18n | Compile-time (rust-i18n) | Zero runtime overhead, EN/CN day 1 |
| Config format | TOML | Rust ecosystem standard |
| Persistence | JSON + atomic writes | Simple, debuggable, raccoon-proven |
| Review model | Cross-model (different agent) | Thariq's adversarial-review pattern |
