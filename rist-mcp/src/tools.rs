//! MCP tool definitions and daemon-backed handlers.

use std::path::PathBuf;

use serde_json::{json, Value};
use uuid::Uuid;

use rist::daemon_client::DaemonClient;
use rist_shared::{AgentType, MergeStrategy, Priority, SessionId, Task, TaskStatus};

/// Returns the static MCP tool catalog.
#[must_use]
pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "spawn_agent",
            "Spawn a new agent session in an isolated worktree.",
            json!({
                "type": "object",
                "properties": {
                    "agent_type": {
                        "type": "string",
                        "description": "Built-ins: claude_code, codex, gemini. Any other non-empty string is treated as a custom agent type."
                    },
                    "task": { "type": "string" },
                    "repo_path": { "type": "string" },
                    "file_ownership": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["agent_type", "task"],
                "additionalProperties": false
            }),
        ),
        tool(
            "list_agents",
            "List all active and archived agent sessions.",
            empty_schema(),
        ),
        tool(
            "get_agent_output",
            "Get recent output lines from an agent session.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "lines": { "type": "integer", "minimum": 1, "default": 50 }
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        ),
        tool(
            "write_to_agent",
            "Write text to an agent's stdin.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "text": { "type": "string" }
                },
                "required": ["session_id", "text"],
                "additionalProperties": false
            }),
        ),
        tool(
            "kill_agent",
            "Terminate an agent session.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        ),
        tool(
            "archive_agent",
            "Archive a completed agent session and release ownership.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "keep_worktree": { "type": "boolean", "default": true }
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        ),
        tool(
            "wait_for_idle",
            "Block until an agent reaches idle, done, or error.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "timeout_secs": { "type": "integer", "minimum": 1, "default": 300 }
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        ),
        tool(
            "run_command",
            "Run a shell command in an agent's worktree.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "command": { "type": "string" }
                },
                "required": ["session_id", "command"],
                "additionalProperties": false
            }),
        ),
        tool(
            "context_budget",
            "Read the context budget breakdown for an agent session.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        ),
        tool(
            "read_task_graph",
            "Read the current task graph.",
            empty_schema(),
        ),
        tool(
            "write_task_graph",
            "Replace the current task graph.",
            json!({
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
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "assigned", "working", "review", "done", "blocked"]
                                },
                                "priority": {
                                    "type": "string",
                                    "enum": ["critical", "high", "medium", "low"]
                                },
                                "agent_type": {
                                    "type": "string",
                                    "description": "Built-ins: claude_code, codex, gemini. Any other non-empty string is treated as a custom agent type."
                                },
                                "depends_on": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                },
                                "file_ownership": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                }
                            },
                            "required": ["id", "title"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["tasks"],
                "additionalProperties": false
            }),
        ),
        tool(
            "get_file_ownership",
            "Get the current file ownership map.",
            empty_schema(),
        ),
        tool(
            "merge_agent",
            "Preview or execute merge of an agent branch.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "preview_only": { "type": "boolean", "default": true },
                    "strategy": {
                        "type": "string",
                        "enum": ["merge", "rebase", "squash"],
                        "default": "squash"
                    }
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        ),
    ]
}

/// Executes a daemon-backed tool handler.
pub async fn handle_tool_call(
    client: &DaemonClient,
    name: &str,
    arguments: Value,
) -> Result<Value, String> {
    match name {
        "spawn_agent" => {
            let agent_type = parse_agent_type(required_str(&arguments, "agent_type")?)?;
            let task = required_str(&arguments, "task")?.to_owned();
            let repo_path = optional_str(&arguments, "repo_path").map(PathBuf::from);
            let file_ownership = optional_string_array(&arguments, "file_ownership")?
                .into_iter()
                .map(PathBuf::from)
                .collect();
            let session_id = client
                .spawn_agent_with_options(agent_type, task, repo_path, file_ownership)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({ "session_id": session_id.0.to_string() }))
        }
        "list_agents" => {
            let agents = client
                .list_agents()
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({
                "agents": agents.into_iter().map(|agent| {
                    json!({
                        "id": agent.id.0.to_string(),
                        "type": agent_type_name(&agent.agent_type),
                        "task": agent.task,
                        "status": agent_status_name(&agent.status),
                        "workdir": agent.workdir.display().to_string(),
                        "branch": agent.branch,
                        "files_owned": agent.file_ownership.into_iter().map(path_string).collect::<Vec<_>>(),
                    })
                }).collect::<Vec<_>>()
            }))
        }
        "get_agent_output" => {
            let session_id = parse_session_id(required_str(&arguments, "session_id")?)?;
            let lines = optional_u64(&arguments, "lines")?.unwrap_or(50) as usize;
            let output = client
                .get_output(session_id, lines)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({ "output": output }))
        }
        "write_to_agent" => {
            let session_id = parse_session_id(required_str(&arguments, "session_id")?)?;
            let text = required_str(&arguments, "text")?;
            client
                .write_to_agent(session_id, text)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({ "success": true }))
        }
        "kill_agent" => {
            let session_id = parse_session_id(required_str(&arguments, "session_id")?)?;
            client
                .kill_agent(session_id)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({ "success": true }))
        }
        "archive_agent" => {
            let session_id = parse_session_id(required_str(&arguments, "session_id")?)?;
            let keep_worktree = optional_bool(&arguments, "keep_worktree").unwrap_or(true);
            client
                .archive_agent(session_id, keep_worktree)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({ "success": true }))
        }
        "wait_for_idle" => {
            let session_id = parse_session_id(required_str(&arguments, "session_id")?)?;
            let timeout_secs = optional_u64(&arguments, "timeout_secs")?.unwrap_or(300);
            let (status, timed_out) = client
                .wait_for_idle(session_id, timeout_secs)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({
                "status": agent_status_name(&status),
                "timed_out": timed_out
            }))
        }
        "run_command" => {
            let session_id = parse_session_id(required_str(&arguments, "session_id")?)?;
            let command = required_str(&arguments, "command")?.to_owned();
            let (stdout, stderr, exit_code) = client
                .run_command(session_id, command)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({
                "stdout": stdout,
                "stderr": stderr,
                "exit_code": exit_code,
            }))
        }
        "context_budget" => {
            let session_id = parse_session_id(required_str(&arguments, "session_id")?)?;
            let budget = client
                .get_context_budget(session_id)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({
                "max_context": budget.max_context,
                "injected_tokens": budget.injected_tokens,
                "mcp_overhead_tokens": budget.mcp_overhead_tokens,
                "tool_output_tokens": budget.tool_output_tokens,
                "alerts": budget.alerts,
                "total_percentage": budget.total_percentage(),
                "injected_percentage": budget.injected_percentage(),
                "mcp_percentage": budget.mcp_percentage(),
                "tool_output_percentage": budget.tool_output_percentage(),
            }))
        }
        "read_task_graph" => {
            let tasks = client
                .read_task_graph()
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({
                "tasks": tasks.into_iter().map(task_to_json).collect::<Vec<_>>()
            }))
        }
        "write_task_graph" => {
            let tasks_value = arguments
                .get("tasks")
                .and_then(Value::as_array)
                .ok_or_else(|| "missing required field: tasks".to_owned())?;
            let tasks = tasks_value
                .iter()
                .map(parse_task)
                .collect::<Result<Vec<_>, _>>()?;
            client
                .write_task_graph(tasks)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({ "success": true }))
        }
        "get_file_ownership" => {
            let ownership = client
                .get_file_ownership()
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({
                "ownership": ownership.into_iter().map(|(path, session_id)| {
                    (path.display().to_string(), Value::String(session_id.0.to_string()))
                }).collect::<serde_json::Map<String, Value>>()
            }))
        }
        "merge_agent" => {
            let session_id = parse_session_id(required_str(&arguments, "session_id")?)?;
            let preview_only = optional_bool(&arguments, "preview_only").unwrap_or(true);
            let strategy =
                parse_merge_strategy(optional_str(&arguments, "strategy").unwrap_or("squash"))?;
            if preview_only {
                let (diff, conflicts) = client
                    .preview_merge(session_id)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(json!({
                    "preview_only": true,
                    "diff": diff,
                    "conflicts": conflicts,
                }))
            } else {
                let (success, message) = client
                    .merge_agent(session_id, strategy)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(json!({
                    "preview_only": false,
                    "success": success,
                    "message": message,
                }))
            }
        }
        _ => Err(format!("unknown tool: {name}")),
    }
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
    })
}

fn empty_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn parse_task(value: &Value) -> Result<Task, String> {
    Ok(Task {
        id: required_non_empty_str(value, "id")?.to_owned(),
        title: required_str(value, "title")?.to_owned(),
        description: optional_str(value, "description").map(ToOwned::to_owned),
        status: parse_task_status(optional_str(value, "status").unwrap_or("pending"))?,
        priority: parse_priority(optional_str(value, "priority").unwrap_or("medium"))?,
        agent_type: optional_str(value, "agent_type")
            .map(parse_agent_type)
            .transpose()?,
        owner: None,
        depends_on: optional_string_array(value, "depends_on")?,
        file_ownership: optional_string_array(value, "file_ownership")?
            .into_iter()
            .map(PathBuf::from)
            .collect(),
    })
}

fn task_to_json(task: Task) -> Value {
    json!({
        "id": task.id,
        "title": task.title,
        "description": task.description,
        "status": task_status_name(&task.status),
        "priority": priority_name(&task.priority),
        "agent_type": task.agent_type.as_ref().map(agent_type_name),
        "owner": task.owner.map(|owner| owner.0.to_string()),
        "file_ownership": task.file_ownership.into_iter().map(path_string).collect::<Vec<_>>(),
        "depends_on": task.depends_on,
    })
}

fn parse_session_id(value: &str) -> Result<SessionId, String> {
    Uuid::parse_str(value)
        .map(SessionId)
        .map_err(|error| error.to_string())
}

fn parse_agent_type(value: &str) -> Result<AgentType, String> {
    match value {
        "claude_code" => Ok(AgentType::Claude),
        "codex" => Ok(AgentType::Codex),
        "gemini" => Ok(AgentType::Gemini),
        other if other.trim().is_empty() => Err("agent_type must not be empty".to_owned()),
        other => Ok(AgentType::Custom(other.to_owned())),
    }
}

fn parse_task_status(value: &str) -> Result<TaskStatus, String> {
    match value {
        "pending" => Ok(TaskStatus::Pending),
        "assigned" => Ok(TaskStatus::Assigned),
        "working" => Ok(TaskStatus::Working),
        "review" => Ok(TaskStatus::Review),
        "done" => Ok(TaskStatus::Done),
        "blocked" => Ok(TaskStatus::Blocked),
        other => Err(format!("unsupported task status: {other}")),
    }
}

fn parse_priority(value: &str) -> Result<Priority, String> {
    match value {
        "critical" => Ok(Priority::Critical),
        "high" => Ok(Priority::High),
        "medium" => Ok(Priority::Medium),
        "low" => Ok(Priority::Low),
        other => Err(format!("unsupported priority: {other}")),
    }
}

fn parse_merge_strategy(value: &str) -> Result<MergeStrategy, String> {
    match value {
        "squash" => Ok(MergeStrategy::Squash),
        "rebase" => Ok(MergeStrategy::Rebase),
        "merge" => Ok(MergeStrategy::MergeCommit),
        other => Err(format!("unsupported merge strategy: {other}")),
    }
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing required field: {key}"))
}

fn required_non_empty_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    let raw = required_str(value, key)?;
    if raw.trim().is_empty() {
        return Err(format!("field {key} must not be empty"));
    }
    Ok(raw)
}

fn optional_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn optional_bool(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

fn optional_u64(value: &Value, key: &str) -> Result<Option<u64>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("field {key} must be a non-negative integer")),
        Some(_) => Err(format!("field {key} must be a non-negative integer")),
    }
}

fn optional_string_array(value: &Value, key: &str) -> Result<Vec<String>, String> {
    match value.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| format!("field {key} must contain only strings"))
            })
            .collect(),
        Some(_) => Err(format!("field {key} must be an array")),
    }
}

fn agent_type_name(agent_type: &AgentType) -> String {
    match agent_type {
        AgentType::Claude => "claude_code".to_owned(),
        AgentType::Codex => "codex".to_owned(),
        AgentType::Gemini => "gemini".to_owned(),
        AgentType::Custom(name) => name.clone(),
        AgentType::Unknown => "unknown".to_owned(),
    }
}

fn task_status_name(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::Assigned => "assigned",
        TaskStatus::Working => "working",
        TaskStatus::Review => "review",
        TaskStatus::Done => "done",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Unknown => "unknown",
    }
}

fn priority_name(priority: &Priority) -> &'static str {
    match priority {
        Priority::Critical => "critical",
        Priority::High => "high",
        Priority::Medium => "medium",
        Priority::Low => "low",
        Priority::Unknown => "unknown",
    }
}

fn agent_status_name(status: &rist_shared::AgentStatus) -> &'static str {
    match status {
        rist_shared::AgentStatus::Idle => "idle",
        rist_shared::AgentStatus::Working => "working",
        rist_shared::AgentStatus::Thinking => "thinking",
        rist_shared::AgentStatus::Waiting => "waiting",
        rist_shared::AgentStatus::Stuck => "stuck",
        rist_shared::AgentStatus::Done => "done",
        rist_shared::AgentStatus::Error => "error",
        rist_shared::AgentStatus::Unknown => "unknown",
    }
}

fn path_string(path: PathBuf) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;

    use portable_pty::CommandBuilder;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    use rist_shared::AgentStatus;
    use ristd::agent_adapter::AgentAdapter;
    use ristd::planner::TaskPlanner;
    use ristd::pty_manager::PtyManager;
    use ristd::session_store::SessionStore;
    use ristd::socket_server::SocketServer;

    use super::*;

    #[derive(Debug)]
    struct TestAdapter;

    impl AgentAdapter for TestAdapter {
        fn name(&self) -> &str {
            "test"
        }

        fn build_command(
            &self,
            _task: &str,
            workdir: &Path,
            _mcp_config: Option<&Path>,
        ) -> CommandBuilder {
            let mut command = CommandBuilder::new("sh");
            command.args(["-lc", "printf 'ready\\n'; exit 0"]);
            command.cwd(workdir);
            command
        }

        fn detect_status(&self, _recent_output: &[u8], _elapsed: Duration) -> AgentStatus {
            AgentStatus::Working
        }

        fn detect_loop(&self, _recent_output: &[u8]) -> Option<String> {
            None
        }
    }

    #[test]
    fn tool_definitions_are_valid_json_schema_objects() {
        for tool in tool_definitions() {
            assert!(tool.get("name").and_then(Value::as_str).is_some());
            assert!(tool.get("description").and_then(Value::as_str).is_some());
            assert_eq!(
                tool.get("inputSchema")
                    .and_then(|schema| schema.get("type"))
                    .and_then(Value::as_str),
                Some("object")
            );
        }
    }

    #[tokio::test]
    async fn spawn_agent_handler_creates_agent() {
        let temp = tempdir().expect("tempdir");
        let socket_path = temp.path().join("daemon.sock");
        let sessions_path = temp.path().join("sessions.json");
        let task_graph_path = temp.path().join("task_graph.json");
        let mut manager = PtyManager::new();
        manager.register_adapter(
            AgentType::Custom("custom".to_owned()),
            Box::new(TestAdapter),
        );

        let server = match SocketServer::bind(
            &socket_path,
            Arc::new(Mutex::new(manager)),
            Arc::new(Mutex::new(SessionStore::new(sessions_path))),
            Arc::new(Mutex::new(TaskPlanner::new(task_graph_path))),
        )
        .await
        {
            Ok(server) => server,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("bind: {error}"),
        };
        let server_task = tokio::spawn(server.run());

        let client = DaemonClient::connect(socket_path.clone())
            .await
            .expect("connect");
        let result = handle_tool_call(
            &client,
            "spawn_agent",
            json!({
                "agent_type": "custom",
                "task": "test task",
                "repo_path": temp.path().display().to_string(),
            }),
        )
        .await
        .expect("tool call");

        let session_id = result
            .get("session_id")
            .and_then(Value::as_str)
            .expect("session id");
        let listed = handle_tool_call(&client, "list_agents", json!({}))
            .await
            .expect("list");
        let agents = listed
            .get("agents")
            .and_then(Value::as_array)
            .expect("agents");
        assert_eq!(agents.len(), 1);
        assert_eq!(
            agents[0].get("id").and_then(Value::as_str),
            Some(session_id)
        );

        server_task.abort();
    }

    #[test]
    fn parse_agent_type_supports_builtins_and_custom() {
        assert_eq!(parse_agent_type("claude_code"), Ok(AgentType::Claude));
        assert_eq!(parse_agent_type("codex"), Ok(AgentType::Codex));
        assert_eq!(parse_agent_type("gemini"), Ok(AgentType::Gemini));
        assert_eq!(
            parse_agent_type("my-custom-agent"),
            Ok(AgentType::Custom("my-custom-agent".to_owned()))
        );
        assert_eq!(
            parse_agent_type("   ").expect_err("empty should fail"),
            "agent_type must not be empty"
        );
    }

    #[test]
    fn parse_task_rejects_empty_id() {
        let error = parse_task(&json!({
            "id": "   ",
            "title": "Title",
        }))
        .expect_err("empty id should fail");

        assert_eq!(error, "field id must not be empty");
    }

    #[test]
    fn parse_task_preserves_optional_fields() {
        let task = parse_task(&json!({
            "id": "task-1",
            "title": "Title",
            "description": "Detailed work",
            "status": "review",
            "priority": "high",
            "agent_type": "codex",
            "depends_on": ["task-0"],
            "file_ownership": ["src/main.rs"]
        }))
        .expect("task");

        assert_eq!(task.id, "task-1");
        assert_eq!(task.description.as_deref(), Some("Detailed work"));
        assert_eq!(task.agent_type, Some(AgentType::Codex));
        assert_eq!(task.depends_on, vec!["task-0".to_owned()]);
        assert_eq!(task.file_ownership, vec![PathBuf::from("src/main.rs")]);
    }

    #[test]
    fn task_to_json_preserves_description_and_agent_type() {
        let value = task_to_json(Task {
            id: "task-1".to_owned(),
            title: "Title".to_owned(),
            description: Some("Detailed work".to_owned()),
            status: TaskStatus::Assigned,
            priority: Priority::Critical,
            agent_type: Some(AgentType::Custom("planner".to_owned())),
            owner: Some(SessionId::new()),
            depends_on: vec!["task-0".to_owned()],
            file_ownership: vec![PathBuf::from("src/main.rs")],
        });

        assert_eq!(
            value.get("description").and_then(Value::as_str),
            Some("Detailed work")
        );
        assert_eq!(
            value.get("agent_type").and_then(Value::as_str),
            Some("planner")
        );
        assert_eq!(
            value.get("status").and_then(Value::as_str),
            Some("assigned")
        );
        assert_eq!(
            value.get("priority").and_then(Value::as_str),
            Some("critical")
        );
    }

    #[test]
    fn optional_u64_rejects_negative_and_float_values() {
        let negative = optional_u64(&json!({ "lines": -1 }), "lines").expect_err("negative");
        let float = optional_u64(&json!({ "lines": 1.5 }), "lines").expect_err("float");

        assert_eq!(negative, "field lines must be a non-negative integer");
        assert_eq!(float, "field lines must be a non-negative integer");
    }

    #[test]
    fn required_non_empty_str_validates_presence_and_content() {
        assert_eq!(
            required_non_empty_str(&json!({ "id": "task-1" }), "id").expect("value"),
            "task-1"
        );
        assert_eq!(
            required_non_empty_str(&json!({ "id": "" }), "id").expect_err("empty"),
            "field id must not be empty"
        );
        assert_eq!(
            required_non_empty_str(&json!({}), "id").expect_err("missing"),
            "missing required field: id"
        );
    }
}
