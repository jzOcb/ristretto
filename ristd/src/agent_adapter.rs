//! Agent-specific command construction and output heuristics.

use std::path::Path;
use std::time::Duration;

use portable_pty::CommandBuilder;
use rist_shared::{AgentStatus, AgentType};

const THINKING_AFTER: Duration = Duration::from_secs(5);
const STUCK_AFTER: Duration = Duration::from_secs(5 * 60);

/// Builds commands and interprets output for a specific agent family.
pub trait AgentAdapter: Send + Sync {
    /// Returns a human-readable adapter name.
    fn name(&self) -> &str;

    /// Builds the command used to launch the agent for `task`.
    fn build_command(
        &self,
        task: &str,
        workdir: &Path,
        mcp_config: Option<&Path>,
    ) -> CommandBuilder;

    /// Detects the current agent status from recent output and idle time.
    fn detect_status(&self, recent_output: &[u8], elapsed: Duration) -> AgentStatus;

    /// Detects a repeated-output loop pattern, if any.
    fn detect_loop(&self, recent_output: &[u8]) -> Option<String>;
}

/// Adapter for Anthropic Claude Code.
#[derive(Debug, Default)]
pub struct ClaudeCodeAdapter;

impl AgentAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &str {
        "claude"
    }

    fn build_command(
        &self,
        task: &str,
        workdir: &Path,
        mcp_config: Option<&Path>,
    ) -> CommandBuilder {
        let mut command = CommandBuilder::new("claude");
        command.arg("--print");
        if let Some(config) = mcp_config {
            command.arg("--mcp-config");
            command.arg(config);
        }
        command.arg(task);
        command.cwd(workdir);
        command
    }

    fn detect_status(&self, recent_output: &[u8], elapsed: Duration) -> AgentStatus {
        detect_status_with_patterns(
            recent_output,
            elapsed,
            &[
                "allow",
                "approve",
                "press enter",
                "waiting for input",
                "y/n",
            ],
            &["thinking", "analyzing", "planning", "reviewing"],
            &["reading", "writing", "editing", "searching", "running"],
        )
    }

    fn detect_loop(&self, recent_output: &[u8]) -> Option<String> {
        detect_repeated_line_loop(recent_output)
    }
}

/// Adapter for the local Codex CLI.
#[derive(Debug, Default)]
pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn name(&self) -> &str {
        "codex"
    }

    fn build_command(
        &self,
        task: &str,
        workdir: &Path,
        _mcp_config: Option<&Path>,
    ) -> CommandBuilder {
        let mut command = CommandBuilder::new("codex");
        command.args(["exec", "--skip-git-repo-check", "--cd"]);
        command.arg(workdir);
        command.arg(task);
        command.cwd(workdir);
        command
    }

    fn detect_status(&self, recent_output: &[u8], elapsed: Duration) -> AgentStatus {
        detect_status_with_patterns(
            recent_output,
            elapsed,
            &["approve", "confirmation", "continue?", "allow"],
            &["thinking", "analyzing", "planning"],
            &["running", "reading", "editing", "applying", "searching"],
        )
    }

    fn detect_loop(&self, recent_output: &[u8]) -> Option<String> {
        detect_repeated_line_loop(recent_output)
    }
}

/// Adapter for the Gemini CLI.
#[derive(Debug, Default)]
pub struct GeminiAdapter;

impl AgentAdapter for GeminiAdapter {
    fn name(&self) -> &str {
        "gemini"
    }

    fn build_command(
        &self,
        task: &str,
        workdir: &Path,
        _mcp_config: Option<&Path>,
    ) -> CommandBuilder {
        let mut command = CommandBuilder::new("gemini");
        command.arg(task);
        command.cwd(workdir);
        command
    }

    fn detect_status(&self, recent_output: &[u8], elapsed: Duration) -> AgentStatus {
        detect_status_with_patterns(
            recent_output,
            elapsed,
            &["waiting", "enter your choice", "confirm"],
            &["thinking", "analyzing", "reasoning"],
            &["reading", "writing", "updating", "executing"],
        )
    }

    fn detect_loop(&self, recent_output: &[u8]) -> Option<String> {
        detect_repeated_line_loop(recent_output)
    }
}

/// Fallback adapter used when no built-in or configured adapter exists.
#[derive(Debug, Clone)]
pub struct DefaultAdapter {
    agent_type: AgentType,
}

impl DefaultAdapter {
    /// Creates a fallback adapter for `agent_type`.
    #[must_use]
    pub fn new(agent_type: AgentType) -> Self {
        Self { agent_type }
    }
}

impl AgentAdapter for DefaultAdapter {
    fn name(&self) -> &str {
        "default"
    }

    fn build_command(
        &self,
        _task: &str,
        workdir: &Path,
        _mcp_config: Option<&Path>,
    ) -> CommandBuilder {
        let mut command = CommandBuilder::new("sh");
        command.args(["-lc"]);
        command.arg(format!(
            "echo No adapter for {}; exec sleep 3600",
            shell_escape(agent_type_label(&self.agent_type))
        ));
        command.cwd(workdir);
        command
    }

    fn detect_status(&self, recent_output: &[u8], elapsed: Duration) -> AgentStatus {
        detect_status_with_patterns(recent_output, elapsed, &[], &[], &["no adapter"])
    }

    fn detect_loop(&self, recent_output: &[u8]) -> Option<String> {
        detect_repeated_line_loop(recent_output)
    }
}

/// Returns the stable registry key for an [`AgentType`].
#[must_use]
pub fn agent_type_key(agent_type: &AgentType) -> String {
    match agent_type {
        AgentType::Claude => "claude".to_owned(),
        AgentType::Codex => "codex".to_owned(),
        AgentType::Gemini => "gemini".to_owned(),
        AgentType::Custom(name) => format!("custom:{name}"),
        AgentType::Unknown => "unknown".to_owned(),
    }
}

fn agent_type_label(agent_type: &AgentType) -> &str {
    match agent_type {
        AgentType::Claude => "claude",
        AgentType::Codex => "codex",
        AgentType::Gemini => "gemini",
        AgentType::Custom(name) => name,
        AgentType::Unknown => "unknown",
    }
}

fn shell_escape(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }

    let escaped = value.replace('\'', r#"'\''"#);
    format!("'{escaped}'")
}

fn detect_status_with_patterns(
    recent_output: &[u8],
    elapsed: Duration,
    waiting_patterns: &[&str],
    thinking_patterns: &[&str],
    working_patterns: &[&str],
) -> AgentStatus {
    let output = String::from_utf8_lossy(recent_output).to_ascii_lowercase();

    if waiting_patterns
        .iter()
        .any(|pattern| output.contains(pattern))
    {
        return AgentStatus::Waiting;
    }
    if thinking_patterns
        .iter()
        .any(|pattern| output.contains(pattern))
    {
        return AgentStatus::Thinking;
    }
    if working_patterns
        .iter()
        .any(|pattern| output.contains(pattern))
    {
        return AgentStatus::Working;
    }
    if elapsed >= STUCK_AFTER {
        return AgentStatus::Stuck;
    }
    if elapsed >= THINKING_AFTER {
        return AgentStatus::Thinking;
    }
    AgentStatus::Working
}

fn detect_repeated_line_loop(recent_output: &[u8]) -> Option<String> {
    let output = String::from_utf8_lossy(recent_output);
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let window = lines.iter().rev().take(3).copied().collect::<Vec<_>>();
    if window.len() == 3 && window.windows(2).all(|pair| pair[0] == pair[1]) {
        return Some(format!("repeated line: {}", window[0]));
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use rist_shared::AgentStatus;

    use super::{AgentAdapter, ClaudeCodeAdapter, CodexAdapter, DefaultAdapter, GeminiAdapter};

    #[test]
    fn claude_builds_print_command() {
        let command = ClaudeCodeAdapter.build_command("solve it", Path::new("/tmp"), None);
        assert_eq!(command.get_argv()[0].to_string_lossy(), "claude");
        assert_eq!(command.get_argv()[1].to_string_lossy(), "--print");
        assert_eq!(command.get_argv()[2].to_string_lossy(), "solve it");
    }

    #[test]
    fn codex_uses_exec_subcommand() {
        let command = CodexAdapter.build_command("fix tests", Path::new("/tmp"), None);
        let argv = command
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            argv[..4].to_vec(),
            vec![
                "codex".to_owned(),
                "exec".to_owned(),
                "--skip-git-repo-check".to_owned(),
                "--cd".to_owned()
            ]
        );
    }

    #[test]
    fn gemini_passes_task_directly() {
        let command = GeminiAdapter.build_command("summarize", Path::new("/tmp"), None);
        assert_eq!(command.get_argv()[0].to_string_lossy(), "gemini");
        assert_eq!(command.get_argv()[1].to_string_lossy(), "summarize");
    }

    #[test]
    fn adapters_detect_waiting_and_loops() {
        let adapter = ClaudeCodeAdapter;
        assert_eq!(
            adapter.detect_status(b"Press Enter to continue", Duration::from_secs(1)),
            AgentStatus::Waiting
        );
        assert_eq!(
            adapter.detect_loop(b"same\nsame\nsame\n"),
            Some("repeated line: same".to_owned())
        );
    }

    #[test]
    fn default_adapter_mentions_missing_adapter() {
        let command = DefaultAdapter::new(rist_shared::AgentType::Custom("foo".to_owned()))
            .build_command("", Path::new("/tmp"), None);
        assert!(command.get_argv()[2]
            .to_string_lossy()
            .contains("No adapter for foo"));
    }
}
