//! Transport implementations for routed events.

use std::fs::{create_dir_all, OpenOptions};
use std::io::{self, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use serde_json::json;

use rist::daemon_client::DaemonClient;
use rist_shared::protocol::Event;
use rist_shared::SessionId;

use crate::event_router::RouteTarget;
use crate::formatters::{format_event_json, format_event_notification};

/// Transport for Claude Code MCP channel payloads.
#[derive(Debug, Default)]
pub struct McpChannelTransport;

/// Transport for JSONL file notifications.
#[derive(Debug, Default)]
pub struct FileTransport;

/// Transport for HTTP webhook delivery.
#[derive(Debug, Default)]
pub struct WebhookTransport;

/// Transport for writing to agent stdin.
#[derive(Debug, Default)]
pub struct StdinTransport;

/// Trait for pushing already-formatted event payloads to a target.
pub trait EventTransport: Send + Sync {
    /// Pushes a formatted event message to the target.
    fn push(&self, target: &RouteTarget, message: &str) -> io::Result<()>;
}

impl EventTransport for McpChannelTransport {
    fn push(&self, target: &RouteTarget, message: &str) -> io::Result<()> {
        let RouteTarget::McpChannel { .. } = target else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "MCP channel transport requires an MCP target",
            ));
        };

        let mut stdout = io::stdout().lock();
        stdout.write_all(message.as_bytes())?;
        stdout.write_all(b"\n")?;
        stdout.flush()
    }
}

impl EventTransport for FileTransport {
    fn push(&self, target: &RouteTarget, message: &str) -> io::Result<()> {
        let RouteTarget::FileNotification { path } = target else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "file transport requires a file target",
            ));
        };

        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(message.as_bytes())?;
        file.write_all(b"\n")
    }
}

impl EventTransport for WebhookTransport {
    fn push(&self, target: &RouteTarget, message: &str) -> io::Result<()> {
        let RouteTarget::Webhook { url } = target else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "webhook transport requires a webhook target",
            ));
        };

        Self::post_payload(url, message)
    }
}

impl McpChannelTransport {
    /// Formats an event for Claude Code channel consumers.
    #[must_use]
    pub fn format_event(event: &Event) -> String {
        json!({
            "type": "ristretto.event",
            "data": event,
            "metadata": {
                "source": "rist-channel",
                "event_type": event_type_name(event),
                "notification": format_event_notification(event),
                "timestamp": Utc::now().to_rfc3339(),
            }
        })
        .to_string()
    }
}

impl FileTransport {
    /// Writes an event as JSONL to a notification file.
    pub fn write_event(path: &Path, event: &Event) -> io::Result<()> {
        let payload = json!({
            "timestamp": Utc::now().to_rfc3339(),
            "event": event,
        })
        .to_string();
        let transport = Self;
        transport.push(
            &RouteTarget::FileNotification {
                path: path.to_path_buf(),
            },
            &payload,
        )
    }
}

impl WebhookTransport {
    /// POSTs an event as JSON to an `http://` webhook endpoint.
    pub fn post_event(url: &str, event: &Event) -> io::Result<()> {
        Self::post_payload(url, &format_event_json(event))
    }

    fn post_payload(url: &str, payload: &str) -> io::Result<()> {
        let Some(rest) = url.strip_prefix("http://") else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "only http:// webhooks are supported",
            ));
        };

        let (authority, path) = match rest.split_once('/') {
            Some((authority, path)) => (authority, format!("/{}", path)),
            None => (rest, "/".to_owned()),
        };
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => {
                let parsed_port = port.parse::<u16>().map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("invalid webhook port: {error}"),
                    )
                })?;
                (host, parsed_port)
            }
            None => (authority, 80),
        };

        let body = payload.to_owned();
        let host = host.to_owned();
        std::thread::spawn(move || {
            let mut addrs = match (host.as_str(), port).to_socket_addrs() {
                Ok(addrs) => addrs,
                Err(_) => return,
            };
            let Some(addr) = addrs.next() else {
                return;
            };
            let mut stream = match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
                Ok(stream) => stream,
                Err(_) => return,
            };
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
            let request = format!(
                "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n{body}",
                path = path,
                host = host,
                length = body.len(),
                body = body,
            );
            let _ = stream.write_all(request.as_bytes());
            let _ = stream.flush();
        });

        Ok(())
    }
}

impl StdinTransport {
    /// Writes a formatted event notification to an agent stdin.
    pub async fn write_event(
        client: &DaemonClient,
        session_id: SessionId,
        event: &Event,
    ) -> io::Result<()> {
        let message = format_event_notification(event);
        let message = if message.is_empty() {
            format_event_json(event)
        } else {
            message
        };
        client
            .write_to_agent(session_id, format!("{message}\n"))
            .await
    }
}

fn event_type_name(event: &Event) -> &'static str {
    match event {
        Event::PtyData { .. } => "pty_data",
        Event::StatusChange { .. } => "status_change",
        Event::AgentExited { .. } => "agent_exited",
        Event::TaskUpdate { .. } => "task_update",
        Event::ContextWarning { .. } => "context_warning",
        Event::LoopDetected { .. } => "loop_detected",
        Event::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::Value;
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::*;
    use rist_shared::{AgentStatus, TaskStatus};

    #[test]
    fn file_transport_writes_valid_jsonl() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("events.jsonl");

        FileTransport::write_event(
            &path,
            &Event::TaskUpdate {
                task_id: "task-5".to_owned(),
                status: TaskStatus::Assigned,
            },
        )
        .expect("write jsonl");

        let content = fs::read_to_string(path).expect("read file");
        let line = content.lines().next().expect("jsonl line");
        let value: Value = serde_json::from_str(line).expect("valid json");
        assert!(value.get("timestamp").is_some());
        assert_eq!(
            value
                .get("event")
                .and_then(|event| event.get("type"))
                .and_then(Value::as_str),
            Some("task_update")
        );
    }

    #[test]
    fn mcp_channel_transport_format_matches_expected_schema() {
        let payload = McpChannelTransport::format_event(&Event::StatusChange {
            id: SessionId(Uuid::nil()),
            old: AgentStatus::Idle,
            new: AgentStatus::Working,
        });
        let value: Value = serde_json::from_str(&payload).expect("valid json");

        assert_eq!(
            value.get("type").and_then(Value::as_str),
            Some("ristretto.event")
        );
        assert_eq!(
            value
                .get("metadata")
                .and_then(|metadata| metadata.get("event_type"))
                .and_then(Value::as_str),
            Some("status_change")
        );
        assert!(value.get("data").is_some());
    }

    #[test]
    fn stdin_transport_uses_human_notification_text() {
        let formatted = format_event_notification(&Event::AgentExited {
            id: SessionId(Uuid::nil()),
            exit_code: 9,
        });

        assert!(formatted.contains("exited with code 9"));
    }
}
