use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use rist_shared::AgentInfo;

const HANDOFF_FILE_NAME: &str = "HANDOFF.md";
const PROGRESS_FILE_NAME: &str = "PROGRESS.md";

pub enum HandoffResult {
    Written(String),
    Timeout(String),
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HandoffManager;

impl HandoffManager {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn wait_for_handoff(&self, workdir: &Path, timeout_secs: u64) -> io::Result<HandoffResult> {
        let handoff_path = workdir.join(HANDOFF_FILE_NAME);
        let started = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);
        let interval = Duration::from_secs(2);

        loop {
            if let Ok(contents) = fs::read_to_string(&handoff_path) {
                let trimmed = contents.trim();
                if !trimmed.is_empty() {
                    return Ok(HandoffResult::Written(trimmed.to_owned()));
                }
            }

            if started.elapsed() >= timeout {
                return Ok(HandoffResult::Timeout(String::new()));
            }

            thread::sleep(interval);
        }
    }

    pub fn generate_fallback(&self, agent_info: &AgentInfo, recent_output: &[String]) -> String {
        let progress = fs::read_to_string(agent_info.workdir.join(PROGRESS_FILE_NAME))
            .ok()
            .filter(|content| !content.trim().is_empty())
            .unwrap_or_else(|| "No PROGRESS.md content found.".to_owned());
        let git_status = git_status(&agent_info.workdir)
            .unwrap_or_else(|| "No git status available.".to_owned());
        let recent_output = if recent_output.is_empty() {
            "No recent output captured.".to_owned()
        } else {
            recent_output.join("\n")
        };

        format!(
            "# HANDOFF\n\n## Task\n{}\n\n## Progress\n{}\n\n## Git Status\n{}\n\n## Recent Output\n{}\n",
            agent_info.task.trim(),
            progress.trim(),
            git_status.trim(),
            recent_output.trim()
        )
    }

    #[must_use]
    pub fn inject_handoff(&self, task: &str, handoff: &str) -> String {
        let trimmed = handoff.trim();
        if trimmed.is_empty() {
            return task.to_owned();
        }
        format!("{trimmed}\n\n---\n\n{task}")
    }

    pub fn cleanup(&self, workdir: &Path) -> io::Result<()> {
        let handoff_path = workdir.join(HANDOFF_FILE_NAME);
        match fs::remove_file(handoff_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

fn git_status(workdir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["status", "--short"])
        .current_dir(workdir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        Some("Clean working tree.".to_owned())
    } else {
        Some(trimmed.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    use chrono::Utc;
    use rist_shared::{AgentStatus, AgentType, SessionId};
    use tempfile::tempdir;

    use super::{HandoffManager, HandoffResult};

    fn sample_agent(workdir: PathBuf) -> rist_shared::AgentInfo {
        rist_shared::AgentInfo {
            id: SessionId::new(),
            agent_type: AgentType::Codex,
            task: "Continue the feature".to_owned(),
            status: AgentStatus::Working,
            workdir,
            branch: Some("rist/test".to_owned()),
            file_ownership: Vec::new(),
            created_at: Utc::now(),
            last_output_at: Some(Utc::now()),
            context_usage: None,
            exit_code: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn wait_for_handoff_reads_file_within_timeout() {
        let manager = HandoffManager::new();
        let dir = tempdir().expect("tempdir");
        let handoff_path = dir.path().join("HANDOFF.md");

        let writer_path = handoff_path.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            fs::write(writer_path, "written handoff").expect("write handoff");
        });

        let result = manager
            .wait_for_handoff(dir.path(), 3)
            .expect("wait for handoff");

        match result {
            HandoffResult::Written(contents) => assert_eq!(contents, "written handoff"),
            HandoffResult::Timeout(_) => panic!("expected written handoff"),
        }
    }

    #[test]
    fn timeout_fallback_includes_git_status_and_progress() {
        let manager = HandoffManager::new();
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("PROGRESS.md"), "finished parser").expect("write progress");
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .expect("git init");
        fs::write(dir.path().join("src.txt"), "changed").expect("write file");

        let agent = sample_agent(dir.path().to_path_buf());
        let timeout = manager
            .wait_for_handoff(dir.path(), 0)
            .expect("wait for timeout");
        let fallback = match timeout {
            HandoffResult::Written(_) => panic!("expected timeout"),
            HandoffResult::Timeout(contents) if contents.is_empty() => {
                manager.generate_fallback(&agent, &["recent step".to_owned()])
            }
            HandoffResult::Timeout(contents) => contents,
        };

        assert!(fallback.contains("finished parser"));
        assert!(fallback.contains("src.txt"));
        assert!(fallback.contains("recent step"));
    }

    #[test]
    fn inject_handoff_prepends_correctly() {
        let manager = HandoffManager::new();
        let combined = manager.inject_handoff("original task", "handoff context");
        assert_eq!(combined, "handoff context\n\n---\n\noriginal task");
    }

    #[test]
    fn cleanup_removes_file() {
        let manager = HandoffManager::new();
        let dir = tempdir().expect("tempdir");
        let handoff_path = dir.path().join("HANDOFF.md");
        fs::write(&handoff_path, "cleanup me").expect("write handoff");

        manager.cleanup(dir.path()).expect("cleanup");

        assert!(!handoff_path.exists());
    }
}
