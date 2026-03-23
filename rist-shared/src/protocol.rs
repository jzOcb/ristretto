//! Frame encoding and decoding for the daemon IPC socket.

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::types::{
    AgentInfo, AgentStatus, AgentType, ContextBudget, EventFilter, MergeStrategy, ReviewScope,
    SessionId, Task, TaskStatus,
};

pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Requests sent to the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Health check.
    Ping,
    /// Spawn a new agent session.
    SpawnAgent {
        /// Agent implementation to launch.
        agent_type: AgentType,
        /// Task prompt for the agent.
        task: String,
        /// Optional repository path or working directory.
        repo_path: Option<PathBuf>,
        /// Files owned by the new agent.
        file_ownership: Vec<PathBuf>,
    },
    /// Kill an active agent.
    KillAgent {
        /// Session identifier.
        id: SessionId,
    },
    /// List current agents.
    ListAgents,
    /// Read recent output lines for an agent.
    GetOutput {
        /// Session identifier.
        id: SessionId,
        /// Number of lines to return.
        lines: usize,
    },
    /// Write text to an agent PTY.
    WriteToAgent {
        /// Session identifier.
        id: SessionId,
        /// Input text to write.
        text: String,
    },
    /// Archive an agent session.
    ArchiveAgent {
        /// Session identifier.
        id: SessionId,
        /// Whether to keep the worktree.
        keep_worktree: bool,
    },
    /// Wait for a session to become idle.
    WaitForIdle {
        /// Session identifier.
        id: SessionId,
        /// Overall timeout.
        timeout_secs: u64,
        /// Required settling window.
        settling_secs: u64,
    },
    /// Run a command in the agent context.
    RunCommand {
        /// Session identifier.
        id: SessionId,
        /// Command line to run.
        command: String,
    },
    /// Read the detailed context budget for a session.
    GetContextBudget {
        /// Session identifier.
        id: SessionId,
    },
    /// Read the task graph.
    ReadTaskGraph,
    /// Replace the task graph.
    WriteTaskGraph {
        /// Full task list snapshot.
        tasks: Vec<Task>,
    },
    /// Read the file ownership map.
    GetFileOwnership,
    /// Merge an agent worktree.
    MergeAgent {
        /// Session identifier.
        id: SessionId,
        /// Only compute a preview.
        preview_only: bool,
        /// Merge strategy.
        strategy: MergeStrategy,
    },
    /// Request a review from another agent type.
    RequestReview {
        /// Session to review.
        agent_id: SessionId,
        /// Reviewer agent type.
        reviewer_type: AgentType,
        /// Scope of the review.
        scope: ReviewScope,
    },
    /// Subscribe to daemon events.
    Subscribe {
        /// Event filters to enable.
        events: Vec<EventFilter>,
    },
    /// Fetch the complete buffered output.
    GetBuffer {
        /// Session identifier.
        id: SessionId,
    },
    /// Resize the agent PTY.
    Resize {
        /// Session identifier.
        id: SessionId,
        /// New PTY columns.
        cols: u16,
        /// New PTY rows.
        rows: u16,
    },
    /// Forward-compatible fallback for unknown values.
    #[serde(other)]
    Unknown,
}

/// Responses sent by the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// Reply to [`Request::Ping`].
    Pong {
        /// Daemon version string.
        version: String,
    },
    /// Newly spawned agent identifier.
    AgentSpawned {
        /// Session identifier.
        id: SessionId,
    },
    /// Current agent list snapshot.
    AgentList {
        /// Active and archived agents.
        agents: Vec<AgentInfo>,
    },
    /// Recent output lines.
    Output {
        /// Text lines.
        lines: Vec<String>,
    },
    /// Current task graph snapshot.
    TaskGraph {
        /// Task list.
        tasks: Vec<Task>,
    },
    /// File ownership map snapshot.
    FileOwnership {
        /// Path ownership map.
        map: HashMap<PathBuf, SessionId>,
    },
    /// Merge preview result.
    MergePreview {
        /// Unified diff preview.
        diff: String,
        /// Conflict paths.
        conflicts: Vec<String>,
    },
    /// Merge execution result.
    MergeResult {
        /// Whether the merge succeeded.
        success: bool,
        /// Human-readable message.
        message: String,
    },
    /// Result of a command execution.
    CommandOutput {
        /// Captured stdout.
        stdout: String,
        /// Captured stderr.
        stderr: String,
        /// Exit code.
        exit_code: i32,
    },
    /// Detailed context budget breakdown.
    ContextBudget {
        /// Breakdown for the requested session.
        budget: ContextBudget,
    },
    /// Result of waiting for an agent state transition.
    WaitStatus {
        /// Final status observed by the waiter.
        status: AgentStatus,
        /// Whether the wait ended because the timeout expired.
        timed_out: bool,
    },
    /// Generic success response.
    Ok,
    /// Error response.
    Error {
        /// Human-readable error message.
        message: String,
    },
    /// Forward-compatible fallback for unknown values.
    #[serde(other)]
    Unknown,
}

/// Broadcast events sent by the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Raw PTY bytes received from an agent.
    PtyData {
        /// Session identifier.
        id: SessionId,
        /// Raw bytes.
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
    },
    /// Status transition for an agent.
    StatusChange {
        /// Session identifier.
        id: SessionId,
        /// Previous status.
        old: AgentStatus,
        /// New status.
        new: AgentStatus,
    },
    /// Agent process exited.
    AgentExited {
        /// Session identifier.
        id: SessionId,
        /// Process exit code.
        exit_code: i32,
    },
    /// Task graph status change.
    TaskUpdate {
        /// Task identifier.
        task_id: String,
        /// New task status.
        status: TaskStatus,
    },
    /// Agent context usage threshold warning.
    ContextWarning {
        /// Session identifier.
        id: SessionId,
        /// Percentage used.
        usage_pct: f64,
    },
    /// Agent loop detection signal.
    LoopDetected {
        /// Session identifier.
        id: SessionId,
        /// Human-readable pattern description.
        pattern: String,
    },
    /// Forward-compatible fallback for unknown values.
    #[serde(other)]
    Unknown,
}

/// Encodes a serializable payload into a length-prefixed JSON frame.
pub fn encode_frame(message: &impl Serialize) -> io::Result<Vec<u8>> {
    let payload = serde_json::to_vec(message)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let length = u32::try_from(payload.len()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("IPC payload exceeds u32 frame size: {error}"),
        )
    })?;
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Reads a length-prefixed JSON frame from a synchronous reader.
pub fn decode_frame<T>(reader: &mut impl Read) -> io::Result<T>
where
    T: DeserializeOwned,
{
    let mut len_buf = [0_u8; 4];
    reader.read_exact(&mut len_buf)?;
    let length = usize::try_from(u32::from_be_bytes(len_buf))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("IPC frame exceeds maximum size of {MAX_FRAME_BYTES} bytes"),
        ));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    serde_json::from_slice(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Writes a length-prefixed JSON frame to an asynchronous writer.
pub async fn encode_frame_async<W>(writer: &mut W, message: &impl Serialize) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let frame = encode_frame(message)?;
    writer.write_all(&frame).await?;
    writer.flush().await
}

/// Reads a length-prefixed JSON frame from an asynchronous reader.
pub async fn decode_frame_async<R, T>(reader: &mut R) -> io::Result<T>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut len_buf = [0_u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let length = usize::try_from(u32::from_be_bytes(len_buf))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("IPC frame exceeds maximum size of {MAX_FRAME_BYTES} bytes"),
        ));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

mod base64_bytes {
    use serde::de::{self, SeqAccess, Visitor};
    use serde::{Deserializer, Serializer};

    use std::fmt;
    use std::vec::Vec;

    pub fn serialize<S>(data: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&encode_base64(data))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BytesVisitor;

        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = Vec<u8>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a base64 string or byte array")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                decode_base64(value).map_err(E::custom)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut bytes = Vec::new();
                while let Some(byte) = seq.next_element::<u8>()? {
                    bytes.push(byte);
                }
                Ok(bytes)
            }
        }

        deserializer.deserialize_any(BytesVisitor)
    }

    fn encode_base64(data: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut encoded = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b0 = chunk[0];
            let b1 = *chunk.get(1).unwrap_or(&0);
            let b2 = *chunk.get(2).unwrap_or(&0);

            encoded.push(TABLE[(b0 >> 2) as usize] as char);
            encoded.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
            if chunk.len() > 1 {
                encoded.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
            } else {
                encoded.push('=');
            }
            if chunk.len() > 2 {
                encoded.push(TABLE[(b2 & 0x3f) as usize] as char);
            } else {
                encoded.push('=');
            }
        }
        encoded
    }

    fn decode_base64(value: &str) -> Result<Vec<u8>, &'static str> {
        let bytes = value.as_bytes();
        if !bytes.len().is_multiple_of(4) {
            return Err("invalid base64 length");
        }

        let mut decoded = Vec::with_capacity((bytes.len() / 4) * 3);
        for chunk in bytes.chunks_exact(4) {
            let mut values = [0_u8; 4];
            let mut padding = 0;
            for (index, byte) in chunk.iter().copied().enumerate() {
                values[index] = match byte {
                    b'A'..=b'Z' => byte - b'A',
                    b'a'..=b'z' => byte - b'a' + 26,
                    b'0'..=b'9' => byte - b'0' + 52,
                    b'+' => 62,
                    b'/' => 63,
                    b'=' => {
                        padding += 1;
                        0
                    }
                    _ => return Err("invalid base64 character"),
                };
            }

            decoded.push((values[0] << 2) | (values[1] >> 4));
            if padding < 2 {
                decoded.push((values[1] << 4) | (values[2] >> 2));
            }
            if padding == 0 {
                decoded.push((values[2] << 6) | values[3]);
            }
        }

        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{self, Cursor};
    use std::path::PathBuf;

    use chrono::Utc;
    use tokio::io::duplex;
    use uuid::Uuid;

    use crate::types::{AgentInfo, AgentStatus, AgentType, ContextUsage, EventFilter, SessionId};

    use super::{
        decode_frame, decode_frame_async, encode_frame, encode_frame_async, Request, Response,
    };

    fn sample_agent() -> AgentInfo {
        AgentInfo {
            id: SessionId::new(),
            agent_type: AgentType::Codex,
            task: "Implement tests".to_owned(),
            status: AgentStatus::Working,
            workdir: PathBuf::from("/tmp/project"),
            branch: Some("rist/tests".to_owned()),
            file_ownership: vec![PathBuf::from("src/lib.rs")],
            created_at: Utc::now(),
            last_output_at: Some(Utc::now()),
            context_usage: Some(ContextUsage {
                estimated_tokens: 256,
                max_tokens: 8_192,
                percentage: 3.125,
            }),
            exit_code: None,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn sync_roundtrip() {
        let response = Response::AgentList {
            agents: vec![sample_agent()],
        };
        let encoded = encode_frame(&response).expect("encode");
        let decoded: Response = decode_frame(&mut Cursor::new(encoded)).expect("decode");
        assert_eq!(response, decoded);
    }

    #[tokio::test]
    async fn async_roundtrip() {
        let request = Request::Subscribe {
            events: vec![EventFilter::PtyData],
        };
        let (mut client, mut server) = duplex(1024);
        encode_frame_async(&mut client, &request)
            .await
            .expect("encode");
        let decoded: Request = decode_frame_async(&mut server).await.expect("decode");
        assert_eq!(request, decoded);
    }

    #[test]
    fn request_unknown_variant_deserializes() {
        let decoded: Request = serde_json::from_str(r#"{"type":"future_request"}"#)
            .expect("deserialize unknown request");
        assert_eq!(decoded, Request::Unknown);
    }

    #[test]
    fn event_unknown_variant_deserializes() {
        let decoded: super::Event =
            serde_json::from_str(r#"{"type":"future_event"}"#).expect("deserialize unknown event");
        assert_eq!(decoded, super::Event::Unknown);
    }

    #[test]
    fn decode_frame_rejects_oversized_payload() {
        let mut bytes = Cursor::new((super::MAX_FRAME_BYTES as u32 + 1).to_be_bytes().to_vec());
        let error = decode_frame::<Response>(&mut bytes).expect_err("oversized frame must fail");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn pty_data_serializes_as_base64() {
        let encoded = serde_json::to_value(super::Event::PtyData {
            id: SessionId::new(),
            data: vec![1, 2, 3, 4],
        })
        .expect("serialize");
        assert_eq!(
            encoded.get("data").and_then(serde_json::Value::as_str),
            Some("AQIDBA==")
        );
    }

    #[test]
    fn pty_data_deserializes_legacy_integer_arrays() {
        let decoded: super::Event = serde_json::from_str(
            r#"{"type":"pty_data","id":"00000000-0000-0000-0000-000000000000","data":[1,2,3]}"#,
        )
        .expect("deserialize");
        assert_eq!(
            decoded,
            super::Event::PtyData {
                id: SessionId(Uuid::nil()),
                data: vec![1, 2, 3],
            }
        );
    }
}
