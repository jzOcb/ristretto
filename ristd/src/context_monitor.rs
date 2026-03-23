//! Context window monitoring and rotation prompts for agent sessions.

use std::fs;
use std::path::PathBuf;

use rist_shared::{AgentInfo, AgentType, ContextBudget};

const DEFAULT_ROTATION_THRESHOLD: f64 = 80.0;
const DEFAULT_MCP_TOOL_COUNT: u64 = 13;
const DEFAULT_MCP_SCHEMA_BYTES: u64 = 512;
const PROGRESS_FILE_NAME: &str = "PROGRESS.md";

/// Monitors agent context usage and prepares rotation prompts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextMonitor {
    /// Threshold percentage to trigger context rotation (default 80.0).
    rotation_threshold: f64,
}

impl ContextMonitor {
    /// Creates a monitor with `threshold`, falling back to the default for invalid values.
    #[must_use]
    pub fn new(threshold: f64) -> Self {
        let rotation_threshold = if (0.0..=100.0).contains(&threshold) && threshold > 0.0 {
            threshold
        } else {
            DEFAULT_ROTATION_THRESHOLD
        };
        Self { rotation_threshold }
    }

    /// Checks an agent's output for context window indicators and estimates usage percentage.
    #[must_use]
    pub fn estimate_usage(&self, agent: &AgentInfo, output: &[u8]) -> f64 {
        let text = String::from_utf8_lossy(output);

        let explicit_patterns = [
            ("context window", "window"),
            ("context limit", "limit"),
            ("token", "token"),
        ];
        for (needle, unit) in explicit_patterns {
            if let Some(value) = percentage_after_phrase(&text, needle) {
                return value;
            }
            if let Some((used, max)) = ratio_near_phrase(&text, needle, unit) {
                return pct_from_ratio(used, max);
            }
        }

        if let Some((used, max)) = ratio_near_phrase(&text, "token", "token") {
            return pct_from_ratio(used, max);
        }

        let approx_tokens = (output.len() / 4) as f64;
        let max_tokens = default_max_tokens(agent) as f64;
        ((approx_tokens / max_tokens) * 100.0).clamp(0.0, 100.0)
    }

    /// Returns true if the estimated usage crosses the rotation threshold.
    #[must_use]
    pub fn should_rotate(&self, usage_pct: f64) -> bool {
        usage_pct >= self.rotation_threshold
    }

    /// Computes a context budget breakdown for an agent.
    #[must_use]
    pub fn context_budget(
        &self,
        agent: &AgentInfo,
        filtered_tool_output_bytes: u64,
    ) -> ContextBudget {
        let injected_tokens = self.injected_tokens(agent);
        let mcp_overhead_tokens = self.mcp_overhead_tokens(agent);
        let tool_output_tokens = bytes_to_tokens(filtered_tool_output_bytes);
        let max_context = default_max_tokens(agent);

        let mut budget = ContextBudget {
            injected_tokens,
            mcp_overhead_tokens,
            tool_output_tokens,
            max_context,
            alerts: Vec::new(),
        };
        budget.alerts = self.alerts_for(&budget);
        budget
    }

    /// Generates threshold-based warnings for a budget snapshot.
    #[must_use]
    pub fn alerts_for(&self, budget: &ContextBudget) -> Vec<String> {
        let mut alerts = Vec::new();
        if budget.mcp_percentage() > 12.5 {
            alerts.push("warning: MCP overhead exceeds 12.5% of max context".to_owned());
        }
        if budget.tool_output_percentage() > 15.0 {
            alerts.push(
                "suggestion: tool output exceeds 15%; enable stronger output filtering".to_owned(),
            );
        }
        if budget.injected_percentage() > 5.0 {
            alerts.push(
                "suggestion: injected context exceeds 5%; review context injection".to_owned(),
            );
        }
        alerts
    }

    /// Generates a context rotation prompt that preserves task state and next steps.
    #[must_use]
    pub fn rotation_prompt(
        &self,
        agent: &AgentInfo,
        recent_output: &[String],
        progress_file: Option<&str>,
    ) -> String {
        let progress_path = progress_file
            .map(PathBuf::from)
            .unwrap_or_else(|| agent.workdir.join(PROGRESS_FILE_NAME));
        let progress = fs::read_to_string(&progress_path).ok();
        let modified_files = changed_files(&agent.workdir);
        let decisions = summarize_lines(recent_output, &["decid", "chose", "implemented"]);
        let next_steps = summarize_lines(recent_output, &["next", "todo", "remaining"]);

        let mut prompt = String::new();
        prompt.push_str("Context rotation required. Preserve state before continuing.\n\n");
        prompt.push_str("Current task:\n");
        prompt.push_str(&format!("- {}\n\n", agent.task));

        prompt.push_str("Progress so far:\n");
        if let Some(progress) = progress
            .as_deref()
            .filter(|content| !content.trim().is_empty())
        {
            prompt.push_str(progress.trim());
            prompt.push('\n');
        } else {
            prompt.push_str("- No PROGRESS.md content found.\n");
        }
        prompt.push('\n');

        prompt.push_str("Files modified so far:\n");
        if modified_files.is_empty() {
            prompt.push_str("- No modified files detected.\n");
        } else {
            for file in modified_files {
                prompt.push_str(&format!("- {file}\n"));
            }
        }
        prompt.push('\n');

        prompt.push_str("Key decisions made:\n");
        if decisions.is_empty() {
            prompt.push_str("- Summarize the most important implementation choices.\n");
        } else {
            for line in decisions {
                prompt.push_str(&format!("- {line}\n"));
            }
        }
        prompt.push('\n');

        prompt.push_str("Next steps:\n");
        if next_steps.is_empty() {
            prompt.push_str("- Describe the next concrete actions for the replacement agent.\n");
        } else {
            for line in next_steps {
                prompt.push_str(&format!("- {line}\n"));
            }
        }
        prompt.push('\n');
        prompt.push_str(
            "Reply with a concise handoff summary that a fresh agent can continue from immediately.\n",
        );
        prompt
    }
}

impl Default for ContextMonitor {
    fn default() -> Self {
        Self::new(DEFAULT_ROTATION_THRESHOLD)
    }
}

fn default_max_tokens(agent: &AgentInfo) -> u64 {
    match &agent.agent_type {
        AgentType::Claude => 200_000,
        AgentType::Codex => 256_000,
        AgentType::Gemini => 1_000_000,
        AgentType::Custom(_) | AgentType::Unknown => 200_000,
    }
}

fn bytes_to_tokens(bytes: u64) -> u64 {
    (bytes.saturating_add(3)) / 4
}

impl ContextMonitor {
    fn injected_tokens(&self, agent: &AgentInfo) -> u64 {
        bytes_to_tokens(u64::try_from(agent.task.len()).unwrap_or(u64::MAX))
            .saturating_add(file_token_estimate(agent.workdir.join("RISTRETTO.md")))
            .saturating_add(file_token_estimate(agent.workdir.join("HANDOFF.md")))
    }

    fn mcp_overhead_tokens(&self, agent: &AgentInfo) -> u64 {
        let tool_count = agent
            .metadata
            .get("mcp_tool_count")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_MCP_TOOL_COUNT);
        let avg_schema_bytes = agent
            .metadata
            .get("mcp_avg_schema_bytes")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_MCP_SCHEMA_BYTES);
        bytes_to_tokens(tool_count.saturating_mul(avg_schema_bytes))
    }
}

fn file_token_estimate(path: PathBuf) -> u64 {
    fs::read(path)
        .ok()
        .map(|bytes| bytes_to_tokens(u64::try_from(bytes.len()).unwrap_or(u64::MAX)))
        .unwrap_or(0)
}

fn percentage_after_phrase(text: &str, phrase: &str) -> Option<f64> {
    text.lines()
        .find(|line| line.to_ascii_lowercase().contains(phrase))
        .and_then(parse_percentage)
}

fn parse_percentage(line: &str) -> Option<f64> {
    let mut number = String::new();
    for ch in line.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            number.push(ch);
        } else if ch == '%' && !number.is_empty() {
            return number
                .parse::<f64>()
                .ok()
                .map(|value| value.clamp(0.0, 100.0));
        } else if !number.is_empty() {
            number.clear();
        }
    }
    None
}

fn ratio_near_phrase(text: &str, phrase: &str, unit: &str) -> Option<(u64, u64)> {
    text.lines()
        .find(|line| line.to_ascii_lowercase().contains(phrase))
        .and_then(|line| parse_ratio(line, unit))
}

fn parse_ratio(line: &str, unit: &str) -> Option<(u64, u64)> {
    let lower = line.to_ascii_lowercase();
    let cleaned = lower.replace(',', "");
    let unit = unit.to_ascii_lowercase();
    for token in cleaned.split_whitespace() {
        if let Some((left, right)) = token.split_once('/') {
            let left = left.trim_matches(|ch: char| !ch.is_ascii_digit());
            let right = right.trim_matches(|ch: char| !ch.is_ascii_digit());
            if !left.is_empty() && !right.is_empty() {
                let used = left.parse::<u64>().ok()?;
                let max = right.parse::<u64>().ok()?;
                return Some((used, max));
            }
        }
    }

    let numbers = cleaned
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u64>().ok())
        .collect::<Vec<_>>();
    if cleaned.contains(&unit) && numbers.len() >= 2 {
        Some((numbers[0], numbers[1]))
    } else {
        None
    }
}

fn pct_from_ratio(used: u64, max: u64) -> f64 {
    if max == 0 {
        0.0
    } else {
        ((used as f64 / max as f64) * 100.0).clamp(0.0, 100.0)
    }
}

fn summarize_lines(lines: &[String], keywords: &[&str]) -> Vec<String> {
    lines
        .iter()
        .rev()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            keywords.iter().any(|keyword| lower.contains(keyword))
        })
        .take(4)
        .map(|line| line.trim().to_owned())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn changed_files(workdir: &PathBuf) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["status", "--short"])
        .current_dir(workdir)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.get(3..).map(str::trim))
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    use chrono::Utc;
    use rist_shared::{AgentInfo, AgentStatus, AgentType, SessionId};
    use tempfile::tempdir;

    use super::ContextMonitor;

    fn agent(workdir: PathBuf, agent_type: AgentType, task: &str) -> AgentInfo {
        AgentInfo {
            id: SessionId::new(),
            agent_type,
            task: task.to_owned(),
            status: AgentStatus::Working,
            workdir,
            branch: Some("rist/test".to_owned()),
            file_ownership: vec![PathBuf::from("src/example.rs")],
            created_at: Utc::now(),
            last_output_at: Some(Utc::now()),
            context_usage: None,
            exit_code: None,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn estimate_usage_reads_claude_token_counts() {
        let monitor = ContextMonitor::default();
        let info = agent(PathBuf::from("."), AgentType::Claude, "task");
        let output = b"Claude Code context window usage: 160000/200000 tokens";

        let usage = monitor.estimate_usage(&info, output);

        assert!((usage - 80.0).abs() < f64::EPSILON);
    }

    #[test]
    fn should_rotate_respects_threshold() {
        let monitor = ContextMonitor::new(80.0);

        assert!(!monitor.should_rotate(79.9));
        assert!(monitor.should_rotate(80.0));
        assert!(monitor.should_rotate(95.0));
    }

    #[test]
    fn rotation_prompt_includes_task_and_progress() {
        let temp = tempdir().expect("tempdir");
        fs::write(
            temp.path().join("PROGRESS.md"),
            "Implemented parser\nRemaining: tests",
        )
        .expect("write progress");
        let info = agent(
            temp.path().to_path_buf(),
            AgentType::Codex,
            "Implement intelligence layer",
        );
        let recent_output = vec![
            "Decision: use a ring buffer for recent output".to_owned(),
            "Next: add restart handling".to_owned(),
        ];

        let prompt = ContextMonitor::default().rotation_prompt(&info, &recent_output, None);

        assert!(prompt.contains("Implement intelligence layer"));
        assert!(prompt.contains("Implemented parser"));
        assert!(prompt.contains("Key decisions made"));
        assert!(prompt.contains("Next steps"));
    }

    #[test]
    fn context_budget_calculates_breakdown() {
        let temp = tempdir().expect("tempdir");
        fs::write(temp.path().join("RISTRETTO.md"), "context".repeat(80)).expect("context");
        fs::write(temp.path().join("HANDOFF.md"), "handoff".repeat(40)).expect("handoff");
        let info = agent(
            temp.path().to_path_buf(),
            AgentType::Codex,
            "implement budget",
        );
        let monitor = ContextMonitor::default();

        let budget = monitor.context_budget(&info, 800);

        assert_eq!(budget.max_context, 256_000);
        assert!(budget.injected_tokens > 0);
        assert!(budget.mcp_overhead_tokens > 0);
        assert_eq!(budget.tool_output_tokens, 200);
    }

    #[test]
    fn alert_thresholds_are_reported() {
        let monitor = ContextMonitor::default();
        let alerts = monitor.alerts_for(&rist_shared::ContextBudget {
            injected_tokens: 20_000,
            mcp_overhead_tokens: 30_000,
            tool_output_tokens: 40_000,
            max_context: 200_000,
            alerts: Vec::new(),
        });

        assert_eq!(alerts.len(), 3);
        assert!(alerts.iter().any(|alert| alert.contains("MCP overhead")));
        assert!(alerts
            .iter()
            .any(|alert| alert.contains("output filtering")));
        assert!(alerts
            .iter()
            .any(|alert| alert.contains("context injection")));
    }
}
