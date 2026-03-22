//! Shared domain and IPC data types.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable session identifier used across the daemon, TUI, and MCP servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub Uuid);

impl SessionId {
    /// Creates a new random session identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Supported agent families.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AgentType {
    /// Anthropic Claude Code.
    Claude,
    /// OpenAI Codex CLI.
    Codex,
    /// Gemini CLI.
    Gemini,
    /// Arbitrary user-defined agent type.
    Custom(String),
    /// Forward-compatible fallback for unknown values.
    #[serde(other)]
    Unknown,
}

/// Live status for an agent session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// Session exists but is not actively processing.
    Idle,
    /// Agent is producing output or making progress.
    Working,
    /// Agent appears to be thinking without output.
    Thinking,
    /// Agent is waiting for human or external input.
    Waiting,
    /// Agent appears stuck.
    Stuck,
    /// Agent completed successfully.
    Done,
    /// Agent terminated with an error.
    Error,
    /// Forward-compatible fallback for unknown values.
    #[serde(other)]
    Unknown,
}

/// Execution status for a task in the task graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Task has not yet started.
    Pending,
    /// Task has been assigned to an agent.
    Assigned,
    /// Task is actively being worked.
    Working,
    /// Task is in review.
    Review,
    /// Task is finished.
    Done,
    /// Task is blocked.
    Blocked,
    /// Forward-compatible fallback for unknown values.
    #[serde(other)]
    Unknown,
}

/// Scheduling priority for a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Highest priority.
    Critical,
    /// High priority.
    High,
    /// Medium priority.
    Medium,
    /// Low priority.
    Low,
    /// Forward-compatible fallback for unknown values.
    #[serde(other)]
    Unknown,
}

/// Merge strategy placeholder used by merge requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeStrategy {
    /// Squash merge.
    Squash,
    /// Rebase merge.
    Rebase,
    /// Merge commit.
    MergeCommit,
    /// Forward-compatible fallback for unknown values.
    #[serde(other)]
    Unknown,
}

/// Review scope placeholder used by review requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewScope {
    /// Review all changes.
    Full,
    /// Review only owned files.
    OwnedFiles,
    /// Review specific task outputs.
    TaskOnly,
    /// Forward-compatible fallback for unknown values.
    #[serde(other)]
    Unknown,
}

/// Event subscription filter placeholder used by channel clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventFilter {
    /// Receive PTY data.
    PtyData,
    /// Receive status changes.
    StatusChange,
    /// Receive exit events.
    AgentExited,
    /// Receive task updates.
    TaskUpdate,
    /// Receive context warnings.
    ContextWarning,
    /// Receive loop detection events.
    LoopDetected,
    /// Forward-compatible fallback for unknown values.
    #[serde(other)]
    Unknown,
}

/// Structured inter-agent message category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// Task assignment.
    Task,
    /// Clarifying question.
    Question,
    /// Status update.
    StatusUpdate,
    /// Completion summary.
    Completion,
    /// Error report.
    Error,
    /// Forward-compatible fallback for unknown values.
    #[serde(other)]
    Unknown,
}

/// Tracks estimated context usage for an agent session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextUsage {
    /// Estimated number of used tokens.
    pub estimated_tokens: u64,
    /// Maximum context tokens available.
    pub max_tokens: u64,
    /// Used context as a percentage in the `[0.0, 100.0]` range.
    pub percentage: f64,
}

/// Persisted and broadcast session metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Session identifier.
    pub id: SessionId,
    /// Agent family.
    pub agent_type: AgentType,
    /// Human-readable task description.
    pub task: String,
    /// Current agent status.
    pub status: AgentStatus,
    /// Working directory or worktree path.
    pub workdir: PathBuf,
    /// Git branch name, when applicable.
    pub branch: Option<String>,
    /// Files or directories owned by the session.
    pub file_ownership: Vec<PathBuf>,
    /// Creation timestamp in UTC.
    pub created_at: DateTime<Utc>,
    /// Last output timestamp in UTC, if available.
    pub last_output_at: Option<DateTime<Utc>>,
    /// Optional context usage estimate.
    pub context_usage: Option<ContextUsage>,
    /// Exit code if the process has exited.
    pub exit_code: Option<i32>,
    /// Additional metadata reserved for future use.
    pub metadata: HashMap<String, String>,
}

/// A node in the planner task graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// Stable task identifier.
    pub id: String,
    /// Short task title.
    pub title: String,
    /// Optional longer description.
    pub description: Option<String>,
    /// Task status.
    pub status: TaskStatus,
    /// Task priority.
    pub priority: Priority,
    /// Preferred agent type, if pre-assigned.
    pub agent_type: Option<AgentType>,
    /// Owning session, if currently assigned.
    pub owner: Option<SessionId>,
    /// Declared dependencies.
    pub depends_on: Vec<String>,
    /// Owned files for this task.
    pub file_ownership: Vec<PathBuf>,
}

/// Whole planner task graph snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskGraph {
    /// All tasks in the graph.
    pub tasks: Vec<Task>,
    /// Last update time in UTC.
    pub updated_at: DateTime<Utc>,
}

/// Structured message exchanged between planner and agents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Stable message identifier.
    pub id: Uuid,
    /// Sender session identifier.
    pub from: SessionId,
    /// Recipient session identifier.
    pub to: SessionId,
    /// Message body.
    pub content: String,
    /// Message timestamp in UTC.
    pub timestamp: DateTime<Utc>,
    /// Message category.
    pub msg_type: MessageType,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{AgentStatus, AgentType, MessageType, Priority, TaskStatus};

    #[test]
    fn string_enums_support_unknown_fallback() {
        let status: AgentStatus = serde_json::from_value(json!("future_status")).expect("status");
        let task: TaskStatus = serde_json::from_value(json!("future_task")).expect("task");
        let priority: Priority =
            serde_json::from_value(json!("future_priority")).expect("priority");
        let message: MessageType =
            serde_json::from_value(json!("future_message")).expect("message");

        assert_eq!(status, AgentStatus::Unknown);
        assert_eq!(task, TaskStatus::Unknown);
        assert_eq!(priority, Priority::Unknown);
        assert_eq!(message, MessageType::Unknown);
    }

    #[test]
    fn agent_type_custom_roundtrip() {
        let encoded =
            serde_json::to_value(AgentType::Custom("my-agent".to_owned())).expect("serialize");
        let decoded: AgentType = serde_json::from_value(encoded).expect("deserialize");
        assert_eq!(decoded, AgentType::Custom("my-agent".to_owned()));
    }

    #[test]
    fn agent_type_unknown_fallback() {
        let decoded: AgentType =
            serde_json::from_value(json!({"kind":"future_agent"})).expect("deserialize");
        assert_eq!(decoded, AgentType::Unknown);
    }
}
