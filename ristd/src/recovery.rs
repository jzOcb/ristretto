//! Agent health evaluation and recovery prompt generation.

use std::time::Duration;

use chrono::Utc;
use rist_shared::{AgentInfo, AgentStatus, AgentType};

/// Evaluates active sessions and recommends recovery actions.
#[derive(Debug, Clone)]
pub struct RecoveryManager {
    /// Max seconds before an idle agent is considered stuck.
    idle_timeout: Duration,
    /// Max consecutive identical outputs before loop detection.
    loop_threshold: usize,
    /// Max retries before giving up.
    max_retries: usize,
}

/// Recovery action to apply to an unhealthy session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Send a prompt intended to unstick the agent.
    Nudge(String),
    /// Restart the agent with preserved context.
    Restart { preserve_progress: bool },
    /// Escalate the task to a different agent type.
    Escalate { to: AgentType, reason: String },
    /// Mark the agent as failed permanently.
    Fail(String),
}

impl RecoveryManager {
    /// Creates a recovery manager with conservative defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            idle_timeout: Duration::from_secs(300),
            loop_threshold: 5,
            max_retries: 3,
        }
    }

    /// Evaluates agent health and recommends the next recovery action, if any.
    #[must_use]
    pub fn evaluate(
        &self,
        agent: &AgentInfo,
        recent_output: &[String],
        retry_count: usize,
    ) -> Option<RecoveryAction> {
        if is_looping(recent_output, self.loop_threshold) {
            return if retry_count < self.max_retries {
                Some(RecoveryAction::Restart {
                    preserve_progress: true,
                })
            } else {
                Some(RecoveryAction::Escalate {
                    to: alternate_agent(&agent.agent_type),
                    reason: "agent entered a repeated output loop".to_owned(),
                })
            };
        }

        if agent.status == AgentStatus::Error {
            return if retry_count < self.max_retries {
                Some(RecoveryAction::Restart {
                    preserve_progress: true,
                })
            } else {
                Some(RecoveryAction::Escalate {
                    to: alternate_agent(&agent.agent_type),
                    reason: "agent exited with errors repeatedly".to_owned(),
                })
            };
        }

        let idle_for = agent
            .last_output_at
            .and_then(|last| (Utc::now() - last).to_std().ok());
        if idle_for.is_some_and(|idle| idle >= self.idle_timeout) {
            return if retry_count == 0 {
                Some(RecoveryAction::Nudge(self.nudge_prompt(
                    agent,
                    recent_output.last().map_or("", String::as_str),
                )))
            } else if retry_count < self.max_retries {
                Some(RecoveryAction::Restart {
                    preserve_progress: true,
                })
            } else {
                Some(RecoveryAction::Fail(
                    "agent remained idle after repeated recovery attempts".to_owned(),
                ))
            };
        }

        None
    }

    /// Builds a prompt tailored to the last visible stuck pattern.
    #[must_use]
    pub fn nudge_prompt(&self, agent: &AgentInfo, last_output: &str) -> String {
        if last_output.trim().is_empty() {
            format!(
                "You appear idle on '{}'. Report your current blocker or continue with the next concrete step now.",
                agent.task
            )
        } else {
            format!(
                "You appear stuck on '{}'. Your last visible output was: '{}'. \
State the blocker in one sentence, then continue with the next concrete action.",
                agent.task,
                last_output.trim()
            )
        }
    }

    /// Builds a restart prompt that preserves context from the failed attempt.
    #[must_use]
    pub fn restart_prompt(
        &self,
        agent: &AgentInfo,
        progress: &str,
        failure_reason: &str,
    ) -> String {
        format!(
            "Restarting work on '{}'.\n\
Previous attempt failed because: {}\n\n\
Preserved progress:\n{}\n\n\
Resume from the latest completed work, avoid the previous failure mode, and continue with the next concrete step.",
            agent.task, failure_reason, progress
        )
    }
}

impl Default for RecoveryManager {
    fn default() -> Self {
        Self::new()
    }
}

fn is_looping(recent_output: &[String], loop_threshold: usize) -> bool {
    if recent_output.len() < loop_threshold {
        return false;
    }
    let last = recent_output.last().map(|line| line.trim()).unwrap_or_default();
    !last.is_empty()
        && recent_output
            .iter()
            .rev()
            .take(loop_threshold)
            .all(|line| line.trim() == last)
}

fn alternate_agent(agent_type: &AgentType) -> AgentType {
    match agent_type {
        AgentType::Claude => AgentType::Codex,
        AgentType::Codex | AgentType::Gemini | AgentType::Custom(_) | AgentType::Unknown => {
            AgentType::Claude
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use chrono::{Duration as ChronoDuration, Utc};
    use rist_shared::{AgentInfo, AgentStatus, AgentType, SessionId};

    use super::{RecoveryAction, RecoveryManager};

    fn agent(status: AgentStatus, last_output_at: chrono::DateTime<Utc>) -> AgentInfo {
        AgentInfo {
            id: SessionId::new(),
            agent_type: AgentType::Codex,
            task: "Implement recovery".to_owned(),
            status,
            workdir: PathBuf::from("."),
            branch: Some("rist/test".to_owned()),
            file_ownership: Vec::new(),
            created_at: Utc::now(),
            last_output_at: Some(last_output_at),
            context_usage: None,
            exit_code: None,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn evaluate_detects_stuck_agent_and_nudges_first() {
        let manager = RecoveryManager::new();
        let idle_agent = agent(AgentStatus::Working, Utc::now() - ChronoDuration::minutes(10));

        let action = manager.evaluate(&idle_agent, &["thinking".to_owned()], 0);

        assert!(matches!(action, Some(RecoveryAction::Nudge(_))));
    }

    #[test]
    fn evaluate_detects_loop_and_restarts() {
        let manager = RecoveryManager::new();
        let loop_agent = agent(AgentStatus::Working, Utc::now());
        let repeated = vec!["repeat".to_owned(); 5];

        let action = manager.evaluate(&loop_agent, &repeated, 1);

        assert_eq!(
            action,
            Some(RecoveryAction::Restart {
                preserve_progress: true,
            })
        );
    }

    #[test]
    fn evaluate_escalates_after_max_retries() {
        let manager = RecoveryManager::new();
        let errored = agent(AgentStatus::Error, Utc::now());

        let action = manager.evaluate(&errored, &[], 3);

        assert!(matches!(action, Some(RecoveryAction::Escalate { .. })));
    }

    #[test]
    fn nudge_prompt_mentions_stuck_pattern() {
        let manager = RecoveryManager::new();
        let idle_agent = agent(AgentStatus::Working, Utc::now());

        let prompt = manager.nudge_prompt(&idle_agent, "waiting on test output");

        assert!(prompt.contains("waiting on test output"));
        assert!(prompt.contains("Implement recovery"));
    }

    #[test]
    fn restart_prompt_includes_progress() {
        let manager = RecoveryManager::new();
        let idle_agent = agent(AgentStatus::Working, Utc::now());

        let prompt =
            manager.restart_prompt(&idle_agent, "parser added, tests pending", "loop detected");

        assert!(prompt.contains("parser added, tests pending"));
        assert!(prompt.contains("loop detected"));
    }
}
