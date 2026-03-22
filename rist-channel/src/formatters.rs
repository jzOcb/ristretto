//! Formatting helpers for routed events.

use serde_json::json;

use rist_shared::protocol::Event;

/// Formats an event as a human-readable notification line.
#[must_use]
pub fn format_event_notification(event: &Event) -> String {
    match event {
        Event::StatusChange { id, old, new } => {
            format!(
                "[ristretto] Agent {} changed status: {} -> {}",
                id.0, old, new
            )
        }
        Event::AgentExited { id, exit_code } => {
            format!("[ristretto] Agent {} exited with code {}", id.0, exit_code)
        }
        Event::TaskUpdate { task_id, status } => {
            format!("[ristretto] Task {} is now {:?}", task_id, status)
        }
        Event::ContextWarning { id, usage_pct } => {
            format!(
                "[ristretto] Agent {} context usage at {:.0}%",
                id.0, usage_pct
            )
        }
        Event::LoopDetected { id, pattern } => {
            format!("[ristretto] Agent {} may be in a loop: {}", id.0, pattern)
        }
        Event::PtyData { id, data } => format!(
            "[ristretto] Agent {} emitted {} bytes of PTY output",
            id.0,
            data.len()
        ),
        Event::Unknown => String::new(),
    }
}

/// Formats an event as JSON for programmatic consumers.
#[must_use]
pub fn format_event_json(event: &Event) -> String {
    serde_json::to_string(event).unwrap_or_else(|_| json!({ "type": "unknown" }).to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use uuid::Uuid;

    use super::*;
    use rist_shared::{AgentStatus, SessionId, TaskStatus};

    #[test]
    fn notification_formats_each_supported_event() {
        let id = SessionId(Uuid::nil());

        assert!(format_event_notification(&Event::StatusChange {
            id,
            old: AgentStatus::Idle,
            new: AgentStatus::Working,
        })
        .contains("changed status"));
        assert!(
            format_event_notification(&Event::AgentExited { id, exit_code: 17 })
                .contains("exited with code 17")
        );
        assert!(format_event_notification(&Event::TaskUpdate {
            task_id: "task-1".to_owned(),
            status: TaskStatus::Done,
        })
        .contains("Task task-1"));
        assert!(format_event_notification(&Event::ContextWarning {
            id,
            usage_pct: 87.9,
        })
        .contains("88%"));
        assert!(format_event_notification(&Event::LoopDetected {
            id,
            pattern: "repeat".to_owned(),
        })
        .contains("repeat"));
        assert!(format_event_notification(&Event::PtyData {
            id,
            data: vec![1, 2, 3],
        })
        .contains("3 bytes"));
    }

    #[test]
    fn format_event_json_produces_valid_json() {
        let value: Value = serde_json::from_str(&format_event_json(&Event::TaskUpdate {
            task_id: "task-42".to_owned(),
            status: TaskStatus::Review,
        }))
        .expect("valid json");

        assert_eq!(
            value.get("type").and_then(Value::as_str),
            Some("task_update")
        );
        assert_eq!(
            value.get("task_id").and_then(Value::as_str),
            Some("task-42")
        );
    }

    #[test]
    fn unknown_event_notification_is_empty() {
        assert!(format_event_notification(&Event::Unknown).is_empty());
    }
}
