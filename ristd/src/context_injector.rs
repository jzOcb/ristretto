//! Context file generation for spawned agent worktrees.

use std::path::Path;

use rist_shared::{AgentInfo, TaskGraph};

/// Generates the `RISTRETTO.md` content written into an agent worktree.
#[must_use]
pub fn generate_context_file(
    agent: &AgentInfo,
    peers: &[AgentInfo],
    task_graph: Option<&TaskGraph>,
    shared_dir: &Path,
) -> String {
    let task_entry = task_graph.and_then(|graph| {
        graph
            .tasks
            .iter()
            .find(|task| task.owner.is_some_and(|owner| owner == agent.id))
    });

    let priority = task_entry
        .map(|task| format!("{:?}", task.priority).to_ascii_lowercase())
        .unwrap_or_else(|| "unspecified".to_owned());

    let mut markdown = String::from("# Ristretto Agent Context\n\n");
    markdown.push_str("## Your Identity\n");
    markdown.push_str(&format!("- **Agent ID:** {}\n", agent.id.0));
    markdown.push_str(&format!("- **Agent Type:** {:?}\n", agent.agent_type));
    markdown.push_str(&format!("- **Task:** {}\n", agent.task));
    markdown.push_str(&format!("- **Priority:** {priority}\n\n"));

    markdown.push_str("## File Ownership\n");
    markdown.push_str("You exclusively own these files/directories:\n");
    if agent.file_ownership.is_empty() {
        markdown.push_str("- None declared\n");
    } else {
        for path in &agent.file_ownership {
            markdown.push_str(&format!("- `{}`\n", path.display()));
        }
    }
    markdown.push('\n');
    markdown.push_str("⚠️ Do NOT modify files outside your ownership list.\n\n");

    markdown.push_str("## Other Agents\n");
    if peers.is_empty() {
        markdown.push_str("- No peer agents\n");
    } else {
        for peer in peers {
            let owned = if peer.file_ownership.is_empty() {
                "none".to_owned()
            } else {
                peer.file_ownership
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            markdown.push_str(&format!(
                "- **{}** ({:?}): \"{}\" — {:?}\n",
                peer.id.0, peer.agent_type, peer.task, peer.status
            ));
            markdown.push_str(&format!("  - Owns: {owned}\n"));
        }
    }
    markdown.push('\n');

    markdown.push_str("## Task Dependencies\n");
    if let Some(task) = task_entry {
        if task.depends_on.is_empty() {
            markdown.push_str("No declared dependencies.\n");
        } else {
            markdown.push_str("These tasks must complete before yours:\n");
            for dependency_id in &task.depends_on {
                if let Some(dependency) = task_graph.and_then(|graph| {
                    graph
                        .tasks
                        .iter()
                        .find(|candidate| candidate.id == *dependency_id)
                }) {
                    markdown.push_str(&format!(
                        "- {}: {} — {:?}\n",
                        dependency.id, dependency.title, dependency.status
                    ));
                } else {
                    markdown.push_str(&format!("- {dependency_id}\n"));
                }
            }
        }
    } else {
        markdown.push_str("No task graph context available.\n");
    }
    markdown.push('\n');

    markdown.push_str("## Communication\n");
    markdown.push_str("- Write progress to `PROGRESS.md` in this directory\n");
    markdown.push_str("- To signal completion: write `RISTRETTO_DONE: <summary>` to stdout\n");
    markdown.push_str("- To signal blocker: write `RISTRETTO_BLOCKED: <reason>` to stdout\n");
    markdown.push_str(&format!(
        "- Shared context available at: {}\n\n",
        shared_dir.display()
    ));

    markdown.push_str("## Rules\n");
    markdown.push_str("1. Stay within your file ownership boundaries\n");
    markdown.push_str("2. Write clean, tested code\n");
    markdown.push_str("3. Update `PROGRESS.md` periodically\n");
    markdown.push_str("4. Signal completion when done\n");
    markdown
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use chrono::Utc;
    use rist_shared::{
        AgentInfo, AgentStatus, AgentType, Priority, SessionId, Task, TaskGraph, TaskStatus,
    };

    use super::generate_context_file;

    fn agent(id: SessionId, task: &str, files: Vec<PathBuf>) -> AgentInfo {
        AgentInfo {
            id,
            agent_type: AgentType::Codex,
            task: task.to_owned(),
            status: AgentStatus::Working,
            workdir: PathBuf::from("/tmp/worktree"),
            branch: Some("rist/test".to_owned()),
            file_ownership: files,
            created_at: Utc::now(),
            last_output_at: Some(Utc::now()),
            context_usage: None,
            exit_code: None,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn context_file_contains_agent_and_peer_sections() {
        let owner = SessionId::new();
        let peer_id = SessionId::new();
        let agent_info = agent(
            owner,
            "Implement daemon merge flow",
            vec![PathBuf::from("src/main.rs")],
        );
        let peer = agent(
            peer_id,
            "Review protocol",
            vec![PathBuf::from("src/lib.rs")],
        );
        let graph = TaskGraph {
            tasks: vec![
                Task {
                    id: "task-1".to_owned(),
                    title: "Implement daemon merge flow".to_owned(),
                    description: None,
                    status: TaskStatus::Working,
                    priority: Priority::High,
                    agent_type: Some(AgentType::Codex),
                    owner: Some(owner),
                    depends_on: vec!["task-0".to_owned()],
                    file_ownership: vec![PathBuf::from("src/main.rs")],
                },
                Task {
                    id: "task-0".to_owned(),
                    title: "Protocol review".to_owned(),
                    description: None,
                    status: TaskStatus::Done,
                    priority: Priority::Medium,
                    agent_type: Some(AgentType::Codex),
                    owner: Some(peer_id),
                    depends_on: Vec::new(),
                    file_ownership: vec![PathBuf::from("src/lib.rs")],
                },
            ],
            updated_at: Utc::now(),
        };

        let markdown =
            generate_context_file(&agent_info, &[peer], Some(&graph), Path::new("/tmp/shared"));

        assert!(markdown.contains("# Ristretto Agent Context"));
        assert!(markdown.contains("Implement daemon merge flow"));
        assert!(markdown.contains("Review protocol"));
        assert!(markdown.contains("src/main.rs"));
        assert!(markdown.contains("task-0: Protocol review"));
        assert!(markdown.contains("/tmp/shared"));
    }
}
