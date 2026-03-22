//! Async Unix-socket client for the Ristretto daemon.

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};

use rist_shared::protocol::{decode_frame_async, encode_frame_async, Event, Request, Response};
use rist_shared::{
    AgentInfo, AgentStatus, AgentType, EventFilter, MergeStrategy, SessionId, Task, TaskStatus,
};

/// Daemon-side updates forwarded to the TUI.
#[derive(Debug, Clone)]
pub enum ClientEvent {
    /// The socket connection is active.
    Connected,
    /// The socket connection was lost.
    Disconnected(String),
    /// A broadcast daemon event arrived.
    Daemon(Event),
}

#[derive(Debug)]
enum Command {
    Request {
        request: Request,
        respond_to: oneshot::Sender<io::Result<Response>>,
    },
    Disconnect,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DaemonFrame {
    Pong {
        version: String,
    },
    AgentSpawned {
        id: SessionId,
    },
    AgentList {
        agents: Vec<AgentInfo>,
    },
    Output {
        lines: Vec<String>,
    },
    TaskGraph {
        tasks: Vec<Task>,
    },
    FileOwnership {
        map: std::collections::HashMap<std::path::PathBuf, SessionId>,
    },
    MergePreview {
        diff: String,
        conflicts: Vec<String>,
    },
    MergeResult {
        success: bool,
        message: String,
    },
    CommandOutput {
        stdout: String,
        stderr: String,
        exit_code: i32,
    },
    Ok,
    Error {
        message: String,
    },
    PtyData {
        id: SessionId,
        data: Vec<u8>,
    },
    StatusChange {
        id: SessionId,
        old: AgentStatus,
        new: AgentStatus,
    },
    AgentExited {
        id: SessionId,
        exit_code: i32,
    },
    TaskUpdate {
        task_id: String,
        status: TaskStatus,
    },
    ContextWarning {
        id: SessionId,
        usage_pct: f64,
    },
    LoopDetected {
        id: SessionId,
        pattern: String,
    },
    #[serde(other)]
    Unknown,
}

/// Handle used by the TUI to talk to the daemon.
#[derive(Clone)]
pub struct DaemonClient {
    command_tx: mpsc::Sender<Command>,
    events_tx: broadcast::Sender<ClientEvent>,
}

impl DaemonClient {
    /// Connects to the daemon socket and spawns the reconnection loop.
    pub async fn connect(path: PathBuf) -> io::Result<Self> {
        let (command_tx, command_rx) = mpsc::channel(64);
        let (events_tx, _) = broadcast::channel(128);

        let mut initial_stream = UnixStream::connect(&path).await?;
        encode_frame_async(
            &mut initial_stream,
            &Request::Subscribe {
                events: vec![
                    EventFilter::PtyData,
                    EventFilter::StatusChange,
                    EventFilter::AgentExited,
                    EventFilter::TaskUpdate,
                    EventFilter::ContextWarning,
                    EventFilter::LoopDetected,
                ],
            },
        )
        .await?;
        let initial_response: Response = decode_frame_async(&mut initial_stream).await?;
        if !matches!(
            initial_response,
            Response::Error { .. } | Response::Ok | Response::Unknown
        ) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected subscribe response",
            ));
        }

        tokio::spawn(connection_task(
            path,
            command_rx,
            events_tx.clone(),
            Some(initial_stream),
        ));

        Ok(Self {
            command_tx,
            events_tx,
        })
    }

    /// Subscribes to daemon events and connection status changes.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ClientEvent> {
        self.events_tx.subscribe()
    }

    /// Requests the daemon version string.
    pub async fn ping(&self) -> io::Result<String> {
        match self.request(Request::Ping).await? {
            Response::Pong { version } => Ok(version),
            other => unexpected_response("ping", other),
        }
    }

    /// Lists known agents.
    pub async fn list_agents(&self) -> io::Result<Vec<AgentInfo>> {
        match self.request(Request::ListAgents).await? {
            Response::AgentList { agents } => Ok(agents),
            other => unexpected_response("list_agents", other),
        }
    }

    /// Returns the last `lines` lines of output for a session.
    pub async fn get_output(&self, id: SessionId, lines: usize) -> io::Result<Vec<String>> {
        match self.request(Request::GetOutput { id, lines }).await? {
            Response::Output { lines } => Ok(lines),
            other => unexpected_response("get_output", other),
        }
    }

    /// Kills an agent session.
    pub async fn kill_agent(&self, id: SessionId) -> io::Result<()> {
        match self.request(Request::KillAgent { id }).await? {
            Response::Ok => Ok(()),
            other => unexpected_response("kill_agent", other),
        }
    }

    /// Writes text to an agent PTY.
    pub async fn write_to_agent(&self, id: SessionId, text: impl Into<String>) -> io::Result<()> {
        match self
            .request(Request::WriteToAgent {
                id,
                text: text.into(),
            })
            .await?
        {
            Response::Ok => Ok(()),
            other => unexpected_response("write_to_agent", other),
        }
    }

    /// Resizes an agent PTY to match the current pane.
    pub async fn resize_agent(&self, id: SessionId, cols: u16, rows: u16) -> io::Result<()> {
        match self.request(Request::Resize { id, cols, rows }).await? {
            Response::Ok => Ok(()),
            other => unexpected_response("resize_agent", other),
        }
    }

    /// Spawns a new agent session.
    #[allow(dead_code)]
    pub async fn spawn_agent(&self, agent_type: AgentType, task: String) -> io::Result<SessionId> {
        match self
            .request(Request::SpawnAgent {
                agent_type,
                task,
                repo_path: None,
                file_ownership: Vec::new(),
            })
            .await?
        {
            Response::AgentSpawned { id } => Ok(id),
            other => unexpected_response("spawn_agent", other),
        }
    }

    /// Spawns a new agent session with full daemon parameters.
    pub async fn spawn_agent_with_options(
        &self,
        agent_type: AgentType,
        task: String,
        repo_path: Option<PathBuf>,
        file_ownership: Vec<PathBuf>,
    ) -> io::Result<SessionId> {
        match self
            .request(Request::SpawnAgent {
                agent_type,
                task,
                repo_path,
                file_ownership,
            })
            .await?
        {
            Response::AgentSpawned { id } => Ok(id),
            other => unexpected_response("spawn_agent_with_options", other),
        }
    }

    /// Archives an agent session.
    pub async fn archive_agent(&self, id: SessionId, keep_worktree: bool) -> io::Result<()> {
        match self
            .request(Request::ArchiveAgent { id, keep_worktree })
            .await?
        {
            Response::Ok => Ok(()),
            other => unexpected_response("archive_agent", other),
        }
    }

    /// Waits for an agent to reach an idle or terminal state.
    pub async fn wait_for_idle(&self, id: SessionId, timeout_secs: u64) -> io::Result<()> {
        match self
            .request(Request::WaitForIdle {
                id,
                timeout_secs,
                settling_secs: 5,
            })
            .await?
        {
            Response::Ok => Ok(()),
            other => unexpected_response("wait_for_idle", other),
        }
    }

    /// Runs a shell command in the agent's worktree.
    pub async fn run_command(
        &self,
        id: SessionId,
        command: String,
    ) -> io::Result<(String, String, i32)> {
        match self.request(Request::RunCommand { id, command }).await? {
            Response::CommandOutput {
                stdout,
                stderr,
                exit_code,
            } => Ok((stdout, stderr, exit_code)),
            other => unexpected_response("run_command", other),
        }
    }

    /// Returns the current planner task graph.
    pub async fn read_task_graph(&self) -> io::Result<Vec<Task>> {
        match self.request(Request::ReadTaskGraph).await? {
            Response::TaskGraph { tasks } => Ok(tasks),
            other => unexpected_response("read_task_graph", other),
        }
    }

    /// Replaces the planner task graph.
    pub async fn write_task_graph(&self, tasks: Vec<Task>) -> io::Result<()> {
        match self.request(Request::WriteTaskGraph { tasks }).await? {
            Response::Ok => Ok(()),
            other => unexpected_response("write_task_graph", other),
        }
    }

    /// Returns the daemon's current file-ownership map.
    pub async fn get_file_ownership(
        &self,
    ) -> io::Result<std::collections::HashMap<std::path::PathBuf, SessionId>> {
        match self.request(Request::GetFileOwnership).await? {
            Response::FileOwnership { map } => Ok(map),
            other => unexpected_response("get_file_ownership", other),
        }
    }

    /// Returns a merge preview for an agent branch.
    pub async fn preview_merge(&self, id: SessionId) -> io::Result<(String, Vec<String>)> {
        match self
            .request(Request::MergeAgent {
                id,
                preview_only: true,
                strategy: MergeStrategy::Squash,
            })
            .await?
        {
            Response::MergePreview { diff, conflicts } => Ok((diff, conflicts)),
            other => unexpected_response("preview_merge", other),
        }
    }

    /// Executes a merge for an agent branch.
    pub async fn merge_agent(
        &self,
        id: SessionId,
        strategy: MergeStrategy,
    ) -> io::Result<(bool, String)> {
        match self
            .request(Request::MergeAgent {
                id,
                preview_only: false,
                strategy,
            })
            .await?
        {
            Response::MergeResult { success, message } => Ok((success, message)),
            other => unexpected_response("merge_agent", other),
        }
    }

    /// Stops the background connection task.
    pub async fn disconnect(&self) -> io::Result<()> {
        self.command_tx
            .send(Command::Disconnect)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "client already closed"))
    }

    async fn request(&self, request: Request) -> io::Result<Response> {
        let (respond_to, rx) = oneshot::channel();
        self.command_tx
            .send(Command::Request {
                request,
                respond_to,
            })
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "daemon task stopped"))?;
        rx.await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "daemon task stopped"))?
    }
}

async fn connection_task(
    path: PathBuf,
    mut command_rx: mpsc::Receiver<Command>,
    events_tx: broadcast::Sender<ClientEvent>,
    initial_stream: Option<UnixStream>,
) {
    let mut cached_stream = initial_stream;

    loop {
        let stream = match take_or_connect(&path, &mut cached_stream).await {
            Ok(stream) => {
                let _ = events_tx.send(ClientEvent::Connected);
                stream
            }
            Err(error) => {
                fail_pending(&mut command_rx, &error).await;
                let _ = events_tx.send(ClientEvent::Disconnected(error.to_string()));
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        let result = run_connected_loop(stream, &mut command_rx, &events_tx).await;
        if !result {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn take_or_connect(
    path: &Path,
    cached_stream: &mut Option<UnixStream>,
) -> io::Result<UnixStream> {
    if let Some(stream) = cached_stream.take() {
        return Ok(stream);
    }
    let mut stream = UnixStream::connect(path).await?;
    encode_frame_async(
        &mut stream,
        &Request::Subscribe {
            events: vec![
                EventFilter::PtyData,
                EventFilter::StatusChange,
                EventFilter::AgentExited,
                EventFilter::TaskUpdate,
                EventFilter::ContextWarning,
                EventFilter::LoopDetected,
            ],
        },
    )
    .await?;
    let _: Response = decode_frame_async(&mut stream).await?;
    Ok(stream)
}

async fn run_connected_loop(
    stream: UnixStream,
    command_rx: &mut mpsc::Receiver<Command>,
    events_tx: &broadcast::Sender<ClientEvent>,
) -> bool {
    let (mut reader, mut writer) = stream.into_split();
    let pending: Arc<Mutex<VecDeque<oneshot::Sender<io::Result<Response>>>>> =
        Arc::new(Mutex::new(VecDeque::new()));

    loop {
        tokio::select! {
            maybe_command = command_rx.recv() => {
                match maybe_command {
                    Some(Command::Request { request, respond_to }) => {
                        if let Err(error) = encode_frame_async(&mut writer, &request).await {
                            let _ = respond_to.send(Err(io::Error::new(error.kind(), error.to_string())));
                            let _ = events_tx.send(ClientEvent::Disconnected(error.to_string()));
                            fail_all_pending(&pending, io::Error::new(error.kind(), error.to_string())).await;
                            return true;
                        }
                        pending.lock().await.push_back(respond_to);
                    }
                    Some(Command::Disconnect) | None => {
                        fail_all_pending(&pending, io::Error::new(io::ErrorKind::BrokenPipe, "client disconnected")).await;
                        return false;
                    }
                }
            }
            frame = decode_daemon_frame(&mut reader) => {
                match frame {
                    Ok(frame) => {
                        match frame {
                            FrameKind::Response(response) => {
                                if let Some(sender) = pending.lock().await.pop_front() {
                                    let _ = sender.send(response_to_result(response));
                                }
                            }
                            FrameKind::Event(event) => {
                                let _ = events_tx.send(ClientEvent::Daemon(event));
                            }
                        }
                    }
                    Err(error) => {
                        let _ = events_tx.send(ClientEvent::Disconnected(error.to_string()));
                        fail_all_pending(&pending, io::Error::new(error.kind(), error.to_string())).await;
                        return true;
                    }
                }
            }
        }
    }
}

enum FrameKind {
    Response(Response),
    Event(Event),
}

async fn decode_daemon_frame<R>(reader: &mut R) -> io::Result<FrameKind>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut len_buf = [0_u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let length = usize::try_from(u32::from_be_bytes(len_buf))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    let frame: DaemonFrame = serde_json::from_slice(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(frame.into_kind())
}

impl DaemonFrame {
    fn into_kind(self) -> FrameKind {
        match self {
            Self::Pong { version } => FrameKind::Response(Response::Pong { version }),
            Self::AgentSpawned { id } => FrameKind::Response(Response::AgentSpawned { id }),
            Self::AgentList { agents } => FrameKind::Response(Response::AgentList { agents }),
            Self::Output { lines } => FrameKind::Response(Response::Output { lines }),
            Self::TaskGraph { tasks } => FrameKind::Response(Response::TaskGraph { tasks }),
            Self::FileOwnership { map } => FrameKind::Response(Response::FileOwnership { map }),
            Self::MergePreview { diff, conflicts } => {
                FrameKind::Response(Response::MergePreview { diff, conflicts })
            }
            Self::MergeResult { success, message } => {
                FrameKind::Response(Response::MergeResult { success, message })
            }
            Self::CommandOutput {
                stdout,
                stderr,
                exit_code,
            } => FrameKind::Response(Response::CommandOutput {
                stdout,
                stderr,
                exit_code,
            }),
            Self::Ok => FrameKind::Response(Response::Ok),
            Self::Error { message } => FrameKind::Response(Response::Error { message }),
            Self::PtyData { id, data } => FrameKind::Event(Event::PtyData { id, data }),
            Self::StatusChange { id, old, new } => {
                FrameKind::Event(Event::StatusChange { id, old, new })
            }
            Self::AgentExited { id, exit_code } => {
                FrameKind::Event(Event::AgentExited { id, exit_code })
            }
            Self::TaskUpdate { task_id, status } => {
                FrameKind::Event(Event::TaskUpdate { task_id, status })
            }
            Self::ContextWarning { id, usage_pct } => {
                FrameKind::Event(Event::ContextWarning { id, usage_pct })
            }
            Self::LoopDetected { id, pattern } => {
                FrameKind::Event(Event::LoopDetected { id, pattern })
            }
            Self::Unknown => FrameKind::Response(Response::Unknown),
        }
    }
}

async fn fail_pending(command_rx: &mut mpsc::Receiver<Command>, error: &io::Error) {
    while let Ok(command) = command_rx.try_recv() {
        match command {
            Command::Request { respond_to, .. } => {
                let _ = respond_to.send(Err(io::Error::new(error.kind(), error.to_string())));
            }
            Command::Disconnect => break,
        }
    }
}

async fn fail_all_pending(
    pending: &Arc<Mutex<VecDeque<oneshot::Sender<io::Result<Response>>>>>,
    error: io::Error,
) {
    let mut pending = pending.lock().await;
    while let Some(sender) = pending.pop_front() {
        let _ = sender.send(Err(io::Error::new(error.kind(), error.to_string())));
    }
}

fn response_to_result(response: Response) -> io::Result<Response> {
    match response {
        Response::Error { message } => Err(io::Error::other(message)),
        other => Ok(other),
    }
}

fn unexpected_response<T>(operation: &str, response: Response) -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("unexpected response for {operation}: {response:?}"),
    ))
}
