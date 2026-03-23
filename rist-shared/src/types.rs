//! Shared domain and IPC data types.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::de::{self};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
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
        Self(Uuid::nil())
    }
}

/// Supported agent families.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    Unknown,
}

impl Serialize for AgentType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = match self {
            Self::Custom(_) => serializer.serialize_struct("AgentType", 2)?,
            _ => serializer.serialize_struct("AgentType", 1)?,
        };
        match self {
            Self::Claude => state.serialize_field("kind", "claude")?,
            Self::Codex => state.serialize_field("kind", "codex")?,
            Self::Gemini => state.serialize_field("kind", "gemini")?,
            Self::Custom(name) => {
                state.serialize_field("kind", "custom")?;
                state.serialize_field("value", name)?;
            }
            Self::Unknown => state.serialize_field("kind", "unknown")?,
        }
        state.end()
    }
}

impl<'de> Deserialize<'de> for AgentType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawAgentType {
            kind: String,
            #[serde(default)]
            value: Option<String>,
        }

        let raw = RawAgentType::deserialize(deserializer)?;
        Ok(match raw.kind.as_str() {
            "claude" => Self::Claude,
            "codex" => Self::Codex,
            "gemini" => Self::Gemini,
            "custom" => raw.value.map(Self::Custom).unwrap_or(Self::Unknown),
            _ => Self::Unknown,
        })
    }
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

impl fmt::Display for AgentStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Thinking => "thinking",
            Self::Waiting => "waiting",
            Self::Stuck => "stuck",
            Self::Done => "done",
            Self::Error => "error",
            Self::Unknown => "unknown",
        })
    }
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
    /// Receive all broadcast events.
    All,
    /// Forward-compatible fallback for unknown values.
    #[serde(other)]
    Unknown,
}

/// Lifecycle hook trigger points.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    /// Before spawning an agent.
    PreSpawn,
    /// After an agent becomes idle following output.
    PostOutput,
    /// Before merging an agent worktree.
    PreMerge,
    /// After merging an agent worktree.
    PostMerge,
    /// When an agent appears stuck.
    OnStuck,
    /// Before rotating agent context.
    OnRotation,
    /// Forward-compatible fallback for unknown values.
    #[serde(other)]
    Unknown,
}

/// Hook configuration loaded from `.ristretto/hooks.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookConfig {
    /// Trigger event for this hook.
    pub event: HookEvent,
    /// Shell command to execute.
    pub command: String,
    /// Whether failures should stop the remaining pipeline.
    #[serde(default)]
    pub blocking: bool,
    /// Max execution time in seconds.
    pub timeout_secs: u64,
    /// Optional text to prepend to the agent task at spawn time.
    #[serde(default)]
    pub inject_context: Option<String>,
    /// Optional debounce interval in seconds.
    #[serde(default)]
    pub min_interval_secs: Option<u64>,
}

/// Captured result from a hook execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookResult {
    /// Whether the hook completed successfully.
    pub success: bool,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
    /// Total runtime in milliseconds.
    pub duration_ms: u64,
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
#[derive(Debug, Clone, PartialEq)]
pub struct ContextUsage {
    /// Estimated number of used tokens.
    pub estimated_tokens: u64,
    /// Maximum context tokens available.
    pub max_tokens: u64,
    /// Used context as a percentage in the `[0.0, 100.0]` range.
    pub percentage: f64,
}

impl Serialize for ContextUsage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.percentage.is_nan() {
            return Err(serde::ser::Error::custom("percentage must not be NaN"));
        }
        let mut state = serializer.serialize_struct("ContextUsage", 3)?;
        state.serialize_field("estimated_tokens", &self.estimated_tokens)?;
        state.serialize_field("max_tokens", &self.max_tokens)?;
        state.serialize_field("percentage", &self.percentage.clamp(0.0, 100.0))?;
        state.end()
    }
}

/// Output filtering mode for command responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterMode {
    /// Command-aware filtering and summarization.
    Smart,
    /// Keep the first `max_lines` lines.
    Head,
    /// Keep the last `max_lines` lines.
    Tail,
    /// Do not truncate or summarize output.
    None,
}

/// Filter rule applied to matching command strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterRule {
    /// Glob-style command pattern.
    pub pattern: String,
    /// Filtering strategy.
    #[serde(default)]
    pub mode: Option<FilterMode>,
    /// Optional line budget override.
    #[serde(default)]
    pub max_lines: Option<usize>,
}

/// Command-output filtering configuration loaded from `filters.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterConfig {
    /// Default line budget for unmatched commands.
    pub max_lines: usize,
    /// Ordered rule list.
    #[serde(default)]
    pub filters: Vec<FilterRule>,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            max_lines: 200,
            filters: vec![
                FilterRule {
                    pattern: "cargo test*".to_owned(),
                    mode: Some(FilterMode::Smart),
                    max_lines: None,
                },
                FilterRule {
                    pattern: "cargo clippy*".to_owned(),
                    mode: Some(FilterMode::Smart),
                    max_lines: None,
                },
                FilterRule {
                    pattern: "cargo build*".to_owned(),
                    mode: Some(FilterMode::Smart),
                    max_lines: None,
                },
                FilterRule {
                    pattern: "git log*".to_owned(),
                    mode: Some(FilterMode::Tail),
                    max_lines: Some(20),
                },
                FilterRule {
                    pattern: "git diff*".to_owned(),
                    mode: Some(FilterMode::Head),
                    max_lines: Some(200),
                },
            ],
        }
    }
}

/// Raw vs filtered byte counts for a command execution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputStats {
    /// Bytes captured before filtering.
    pub raw_bytes: u64,
    /// Bytes returned after filtering.
    pub filtered_bytes: u64,
    /// Whether a filter or summary changed the output.
    pub filter_applied: bool,
}

/// Estimated context-budget breakdown for an agent session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBudget {
    /// Task text, context injection, and `HANDOFF.md`.
    pub injected_tokens: u64,
    /// Estimated MCP tool-schema overhead.
    pub mcp_overhead_tokens: u64,
    /// Cumulative post-filter tool output.
    pub tool_output_tokens: u64,
    /// Maximum supported context for the agent family.
    pub max_context: u64,
    /// Human-readable warnings for disproportionate usage.
    #[serde(default)]
    pub alerts: Vec<String>,
}

impl ContextBudget {
    /// Returns the total estimated tokens consumed by tracked sources.
    #[must_use]
    pub fn total_tokens(&self) -> u64 {
        self.injected_tokens
            .saturating_add(self.mcp_overhead_tokens)
            .saturating_add(self.tool_output_tokens)
    }

    /// Returns total usage percentage in the `[0.0, 100.0]` range.
    #[must_use]
    pub fn total_percentage(&self) -> f64 {
        percentage(self.total_tokens(), self.max_context)
    }

    /// Returns injected-context percentage.
    #[must_use]
    pub fn injected_percentage(&self) -> f64 {
        percentage(self.injected_tokens, self.max_context)
    }

    /// Returns MCP-overhead percentage.
    #[must_use]
    pub fn mcp_percentage(&self) -> f64 {
        percentage(self.mcp_overhead_tokens, self.max_context)
    }

    /// Returns tool-output percentage.
    #[must_use]
    pub fn tool_output_percentage(&self) -> f64 {
        percentage(self.tool_output_tokens, self.max_context)
    }
}

impl<'de> Deserialize<'de> for ContextUsage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawContextUsage {
            estimated_tokens: u64,
            max_tokens: u64,
            percentage: f64,
        }

        let raw = RawContextUsage::deserialize(deserializer)?;
        if raw.percentage.is_nan() {
            return Err(de::Error::custom("percentage must not be NaN"));
        }
        Ok(Self {
            estimated_tokens: raw.estimated_tokens,
            max_tokens: raw.max_tokens,
            percentage: raw.percentage.clamp(0.0, 100.0),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffStatus {
    pub available: bool,
    pub pending: bool,
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
#[derive(Debug, Clone, PartialEq)]
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

impl Serialize for Task {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        validate_task_id(&self.id).map_err(serde::ser::Error::custom)?;
        let mut state = serializer.serialize_struct("Task", 9)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("title", &self.title)?;
        state.serialize_field("description", &self.description)?;
        state.serialize_field("status", &self.status)?;
        state.serialize_field("priority", &self.priority)?;
        state.serialize_field("agent_type", &self.agent_type)?;
        state.serialize_field("owner", &self.owner)?;
        state.serialize_field("depends_on", &self.depends_on)?;
        state.serialize_field("file_ownership", &self.file_ownership)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Task {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawTask {
            id: String,
            title: String,
            description: Option<String>,
            status: TaskStatus,
            priority: Priority,
            agent_type: Option<AgentType>,
            owner: Option<SessionId>,
            #[serde(default)]
            depends_on: Vec<String>,
            #[serde(default)]
            file_ownership: Vec<PathBuf>,
        }

        let raw = RawTask::deserialize(deserializer)?;
        validate_task_id(&raw.id).map_err(de::Error::custom)?;
        Ok(Self {
            id: raw.id,
            title: raw.title,
            description: raw.description,
            status: raw.status,
            priority: raw.priority,
            agent_type: raw.agent_type,
            owner: raw.owner,
            depends_on: raw.depends_on,
            file_ownership: raw.file_ownership,
        })
    }
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

fn validate_task_id(id: &str) -> Result<(), &'static str> {
    if id.trim().is_empty() {
        return Err("task id must not be empty");
    }
    Ok(())
}

fn percentage(value: u64, max: u64) -> f64 {
    if max == 0 {
        0.0
    } else {
        ((value as f64 / max as f64) * 100.0).clamp(0.0, 100.0)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::{
        AgentStatus, AgentType, ContextUsage, HookConfig, HookEvent, HookResult, MessageType,
        Priority, SessionId, Task, TaskStatus,
    };

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

    #[test]
    fn session_id_default_is_nil() {
        assert_eq!(SessionId::default().0, Uuid::nil());
    }

    #[test]
    fn context_usage_clamps_percentage() {
        let usage: ContextUsage = serde_json::from_value(json!({
            "estimated_tokens": 1,
            "max_tokens": 2,
            "percentage": 125.5
        }))
        .expect("deserialize");
        assert_eq!(usage.percentage, 100.0);
    }

    #[test]
    fn context_usage_rejects_nan() {
        let error = serde_json::to_value(ContextUsage {
            estimated_tokens: 1,
            max_tokens: 2,
            percentage: f64::NAN,
        })
        .expect_err("nan must fail");
        assert!(error.to_string().contains("NaN"));
    }

    #[test]
    fn task_id_must_be_non_empty() {
        let error = serde_json::from_value::<Task>(json!({
            "id": "   ",
            "title": "Title",
            "description": null,
            "status": "pending",
            "priority": "medium",
            "agent_type": null,
            "owner": null,
            "depends_on": [],
            "file_ownership": []
        }))
        .expect_err("empty id must fail");
        assert!(error.to_string().contains("task id"));
    }

    #[test]
    fn hook_types_roundtrip() {
        let config = HookConfig {
            event: HookEvent::PreSpawn,
            command: "echo ready".to_owned(),
            blocking: true,
            timeout_secs: 5,
            inject_context: Some("Follow repo rules.".to_owned()),
            min_interval_secs: Some(30),
        };
        let encoded = serde_json::to_value(&config).expect("serialize config");
        let decoded: HookConfig = serde_json::from_value(encoded).expect("deserialize config");
        assert_eq!(decoded, config);

        let result = HookResult {
            success: true,
            stdout: "ok".to_owned(),
            stderr: String::new(),
            duration_ms: 12,
        };
        let encoded = serde_json::to_value(&result).expect("serialize result");
        let decoded: HookResult = serde_json::from_value(encoded).expect("deserialize result");
        assert_eq!(decoded, result);
    }
}
