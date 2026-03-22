//! Async Unix-socket client for the Ristretto daemon.

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};

use rist_shared::protocol::{decode_frame_async, encode_frame_async, Event, Request, Response};
use rist_shared::{AgentInfo, AgentType, EventFilter, SessionId};

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
            frame = decode_frame_async::<_, Value>(&mut reader) => {
                match frame {
                    Ok(frame) => {
                        if is_response_frame(&frame) {
                            let response: Response = match serde_json::from_value(frame) {
                                Ok(response) => response,
                                Err(error) => {
                                    let io_error = io::Error::new(io::ErrorKind::InvalidData, error);
                                    let _ = events_tx.send(ClientEvent::Disconnected(io_error.to_string()));
                                    fail_all_pending(&pending, io_error).await;
                                    return true;
                                }
                            };
                            if let Some(sender) = pending.lock().await.pop_front() {
                                let _ = sender.send(response_to_result(response));
                            }
                        } else if is_event_frame(&frame) {
                            if let Ok(event) = serde_json::from_value::<Event>(frame) {
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

fn is_response_frame(frame: &Value) -> bool {
    frame
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| {
            matches!(
                kind,
                "pong"
                    | "agent_spawned"
                    | "agent_list"
                    | "output"
                    | "task_graph"
                    | "file_ownership"
                    | "merge_preview"
                    | "merge_result"
                    | "command_output"
                    | "ok"
                    | "error"
                    | "unknown"
            )
        })
}

fn is_event_frame(frame: &Value) -> bool {
    frame
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| {
            matches!(
                kind,
                "pty_data"
                    | "status_change"
                    | "agent_exited"
                    | "task_update"
                    | "context_warning"
                    | "loop_detected"
                    | "unknown"
            )
        })
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
