//! Unix socket IPC server for daemon clients.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;

use rist_shared::protocol::{decode_frame_async, encode_frame_async, Event, Request, Response};
use rist_shared::{AgentInfo, AgentStatus, AgentType, SessionId, TaskGraph};

use crate::planner::TaskPlanner;
use crate::pty_manager::PtyManager;
use crate::review_engine::ReviewEngine;
use crate::session_store::SessionStore;

enum ServerFrame {
    Response(Response),
    Event(Event),
}

/// Shared server state for all socket connections.
pub struct SocketServer {
    listener: UnixListener,
    pty_manager: Arc<Mutex<PtyManager>>,
    session_store: Arc<Mutex<SessionStore>>,
    planner: Arc<Mutex<TaskPlanner>>,
    clients: Arc<Mutex<HashMap<usize, mpsc::UnboundedSender<ServerFrame>>>>,
    next_client_id: AtomicUsize,
}

impl SocketServer {
    /// Binds a new Unix socket server to `path`.
    pub async fn bind(
        path: &Path,
        pty_manager: Arc<Mutex<PtyManager>>,
        session_store: Arc<Mutex<SessionStore>>,
        planner: Arc<Mutex<TaskPlanner>>,
    ) -> io::Result<Self> {
        let listener = UnixListener::bind(path)?;
        Ok(Self {
            listener,
            pty_manager,
            session_store,
            planner,
            clients: Arc::new(Mutex::new(HashMap::new())),
            next_client_id: AtomicUsize::new(1),
        })
    }

    /// Broadcasts an event to all connected clients.
    pub async fn broadcast_event(&self, event: Event) {
        broadcast_to_all(&self.clients, event).await;
    }

    /// Runs the accept loop until the task is cancelled.
    pub async fn run(self) -> io::Result<()> {
        let sync_pty_manager = Arc::clone(&self.pty_manager);
        let sync_session_store = Arc::clone(&self.session_store);
        let sync_clients = Arc::clone(&self.clients);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(200));
            loop {
                tick.tick().await;
                let _ =
                    sync_agent_state(&sync_pty_manager, &sync_session_store, &sync_clients).await;
            }
        });

        let health_pty_manager = Arc::clone(&self.pty_manager);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(5));
            loop {
                tick.tick().await;
                let actions = {
                    let mut manager = health_pty_manager.lock().await;
                    manager.health_check()
                };
                for (id, action) in actions {
                    if let crate::recovery::RecoveryAction::Nudge(prompt) = action {
                        let mut manager = health_pty_manager.lock().await;
                        let _ = manager.write_to_agent(id, &format!("{prompt}\n"));
                    }
                }
            }
        });

        loop {
            let (stream, _) = self.listener.accept().await?;
            let client_id = self.next_client_id.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = mpsc::unbounded_channel();
            self.clients.lock().await.insert(client_id, tx);

            let pty_manager = Arc::clone(&self.pty_manager);
            let session_store = Arc::clone(&self.session_store);
            let planner = Arc::clone(&self.planner);
            let clients = Arc::clone(&self.clients);
            tokio::spawn(async move {
                let _ = handle_connection(
                    client_id,
                    stream,
                    rx,
                    pty_manager,
                    session_store,
                    planner,
                    clients,
                )
                .await;
            });
        }
    }
}

async fn handle_connection(
    client_id: usize,
    stream: UnixStream,
    mut rx: mpsc::UnboundedReceiver<ServerFrame>,
    pty_manager: Arc<Mutex<PtyManager>>,
    session_store: Arc<Mutex<SessionStore>>,
    planner: Arc<Mutex<TaskPlanner>>,
    clients: Arc<Mutex<HashMap<usize, mpsc::UnboundedSender<ServerFrame>>>>,
) -> io::Result<()> {
    let (mut reader, mut writer) = stream.into_split();
    let writer_task = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            match frame {
                ServerFrame::Response(response) => {
                    encode_frame_async(&mut writer, &response).await?
                }
                ServerFrame::Event(event) => encode_frame_async(&mut writer, &event).await?,
            }
        }
        Ok::<(), io::Error>(())
    });

    loop {
        sync_agent_state(&pty_manager, &session_store, &clients).await?;

        let request: Request = match decode_frame_async(&mut reader).await {
            Ok(request) => request,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error),
        };
        let response = dispatch_request(&request, &pty_manager, &session_store, &planner).await;
        send_response(&clients, client_id, response.clone()).await;

        if let Request::SpawnAgent { .. } = request {
            if let Response::AgentSpawned { id } = response {
                broadcast_to_all(
                    &clients,
                    Event::StatusChange {
                        id,
                        old: AgentStatus::Idle,
                        new: AgentStatus::Working,
                    },
                )
                .await;
            }
        }

        sync_agent_state(&pty_manager, &session_store, &clients).await?;
    }

    clients.lock().await.remove(&client_id);
    writer_task.abort();
    Ok(())
}

async fn dispatch_request(
    request: &Request,
    pty_manager: &Arc<Mutex<PtyManager>>,
    session_store: &Arc<Mutex<SessionStore>>,
    planner: &Arc<Mutex<TaskPlanner>>,
) -> Response {
    match request {
        Request::Ping => Response::Pong {
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        Request::ListAgents => {
            let manager = pty_manager.lock().await;
            Response::AgentList {
                agents: manager.list_agents(),
            }
        }
        Request::SpawnAgent {
            agent_type,
            task,
            repo_path,
            file_ownership,
        } => {
            let mut manager = pty_manager.lock().await;
            match manager.spawn_agent(
                agent_type.clone(),
                task.clone(),
                repo_path.clone(),
                file_ownership.clone(),
            ) {
                Ok(id) => {
                    let agent_info = manager
                        .list_agents()
                        .into_iter()
                        .find(|agent| agent.id == id)
                        .unwrap_or_else(|| placeholder_agent(id, agent_type.clone(), task.clone()));
                    let mut store = session_store.lock().await;
                    store.update(agent_info);
                    if let Err(error) = store.save() {
                        return Response::Error {
                            message: error.to_string(),
                        };
                    }
                    Response::AgentSpawned { id }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::KillAgent { id } => {
            match kill_agent_async(pty_manager, *id).await {
                Ok(()) => Response::Ok,
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::GetOutput { id, lines } => {
            let manager = pty_manager.lock().await;
            match manager.get_output(*id, *lines) {
                Ok(lines) => Response::Output { lines },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::WriteToAgent { id, text } => {
            let mut manager = pty_manager.lock().await;
            match manager.write_to_agent(*id, text) {
                Ok(()) => Response::Ok,
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::ArchiveAgent { id, keep_worktree } => {
            match archive_agent_async(pty_manager, *id, *keep_worktree).await {
                Ok(agent_info) => {
                    let mut store = session_store.lock().await;
                    store.update(agent_info);
                    if let Err(error) = store.save() {
                        return Response::Error {
                            message: error.to_string(),
                        };
                    }
                    Response::Ok
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::WaitForIdle {
            id,
            timeout_secs,
            settling_secs,
        } => {
            match wait_for_idle_async(pty_manager, *id, *timeout_secs, *settling_secs).await {
                Ok(status) => Response::WaitStatus { status },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::RunCommand { id, command } => {
            let manager = pty_manager.lock().await;
            match manager.run_command(*id, command) {
                Ok((stdout, stderr, exit_code)) => Response::CommandOutput {
                    stdout,
                    stderr,
                    exit_code,
                },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::ReadTaskGraph => {
            let planner = planner.lock().await;
            Response::TaskGraph {
                tasks: planner.graph().tasks.clone(),
            }
        }
        Request::WriteTaskGraph { tasks } => {
            let mut planner = planner.lock().await;
            match planner.set_graph(TaskGraph {
                tasks: tasks.clone(),
                updated_at: Utc::now(),
            }) {
                Ok(()) => Response::Ok,
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::GetFileOwnership => {
            let manager = pty_manager.lock().await;
            Response::FileOwnership {
                map: manager.ownership_map().clone(),
            }
        }
        Request::MergeAgent {
            id,
            preview_only,
            strategy,
        } => {
            let manager = pty_manager.lock().await;
            if *preview_only {
                match manager.preview_merge(*id) {
                    Ok(preview) => Response::MergePreview {
                        diff: preview.diff,
                        conflicts: preview.conflicts,
                    },
                    Err(error) => Response::Error {
                        message: error.to_string(),
                    },
                }
            } else {
                match manager.merge_agent(
                    *id,
                    strategy.clone(),
                    &format!("Squash merge agent {id:?}"),
                ) {
                    Ok(result) => Response::MergeResult {
                        success: result.success,
                        message: result.message,
                    },
                    Err(error) => Response::Error {
                        message: error.to_string(),
                    },
                }
            }
        }
        Request::GetBuffer { id } => {
            let manager = pty_manager.lock().await;
            match manager.get_buffer(*id) {
                Ok(bytes) => Response::Output {
                    lines: String::from_utf8_lossy(&bytes)
                        .lines()
                        .map(ToOwned::to_owned)
                        .collect(),
                },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::Resize { id, cols, rows } => {
            let mut manager = pty_manager.lock().await;
            match manager.resize(*id, *cols, *rows) {
                Ok(()) => Response::Ok,
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::RequestReview {
            agent_id,
            reviewer_type,
            ..
        } => {
            let mut manager = pty_manager.lock().await;
            match manager.request_review(*agent_id) {
                Ok(review_request) => {
                    let engine = ReviewEngine::new();
                    let default_reviewer = engine.reviewer_for(&review_request.source_type);
                    let mut prompt = engine.build_review_prompt(&review_request);
                    if *reviewer_type != default_reviewer {
                        prompt =
                            format!("Preferred reviewer type: {:?}\n{}\n", reviewer_type, prompt);
                    }
                    Response::Output {
                        lines: prompt.lines().map(ToOwned::to_owned).collect(),
                    }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::Subscribe { .. } => Response::Ok,
        Request::Unknown => Response::Error {
            message: "request not implemented".to_owned(),
        },
    }
}

async fn kill_agent_async(
    pty_manager: &Arc<Mutex<PtyManager>>,
    id: SessionId,
) -> io::Result<()> {
    let needs_grace = {
        let mut manager = pty_manager.lock().await;
        matches!(
            manager.request_termination(id)?,
            crate::pty_manager::TerminationRequest::GracePeriod
        )
    };
    if needs_grace {
        sleep(Duration::from_secs(2)).await;
        let mut manager = pty_manager.lock().await;
        manager.finish_termination(id)?;
    }
    Ok(())
}

async fn archive_agent_async(
    pty_manager: &Arc<Mutex<PtyManager>>,
    id: SessionId,
    keep_worktree: bool,
) -> io::Result<AgentInfo> {
    let needs_grace = {
        let mut manager = pty_manager.lock().await;
        matches!(
            manager.request_termination(id)?,
            crate::pty_manager::TerminationRequest::GracePeriod
        )
    };
    if needs_grace {
        sleep(Duration::from_secs(1)).await;
        let mut manager = pty_manager.lock().await;
        manager.finish_termination(id)?;
    }

    let mut manager = pty_manager.lock().await;
    manager.archive_agent(id, keep_worktree)
}

async fn wait_for_idle_async(
    pty_manager: &Arc<Mutex<PtyManager>>,
    id: SessionId,
    timeout_secs: u64,
    settling_secs: u64,
) -> io::Result<AgentStatus> {
    let timeout = Duration::from_secs(timeout_secs);
    let settling = Duration::from_secs(settling_secs);
    let start = std::time::Instant::now();

    loop {
        let state = {
            let mut manager = pty_manager.lock().await;
            manager.idle_check(id, settling)?
        };

        if state.complete || start.elapsed() >= timeout {
            return Ok(state.status);
        }

        sleep(Duration::from_millis(200)).await;
    }
}

async fn sync_agent_state(
    pty_manager: &Arc<Mutex<PtyManager>>,
    session_store: &Arc<Mutex<SessionStore>>,
    clients: &Arc<Mutex<HashMap<usize, mpsc::UnboundedSender<ServerFrame>>>>,
) -> io::Result<()> {
    let (agents, events) = {
        let mut manager = pty_manager.lock().await;
        manager.check_exits();
        (manager.list_agents(), manager.drain_events())
    };

    if events.is_empty() {
        return Ok(());
    }

    {
        let mut store = session_store.lock().await;
        for agent in agents {
            store.update(agent);
        }
        store.save()?;
    }

    for event in events {
        broadcast_to_all(clients, event).await;
    }
    Ok(())
}

async fn send_response(
    clients: &Arc<Mutex<HashMap<usize, mpsc::UnboundedSender<ServerFrame>>>>,
    client_id: usize,
    response: Response,
) {
    let sender = {
        let clients_guard = clients.lock().await;
        clients_guard.get(&client_id).cloned()
    };
    if let Some(sender) = sender {
        let _ = sender.send(ServerFrame::Response(response));
    }
}

async fn broadcast_to_all(
    clients: &Arc<Mutex<HashMap<usize, mpsc::UnboundedSender<ServerFrame>>>>,
    event: Event,
) {
    let senders = {
        let clients_guard = clients.lock().await;
        clients_guard.values().cloned().collect::<Vec<_>>()
    };
    for sender in senders {
        let _ = sender.send(ServerFrame::Event(event.clone()));
    }
}

fn placeholder_agent(id: SessionId, agent_type: AgentType, task: String) -> AgentInfo {
    AgentInfo {
        id,
        agent_type,
        task,
        status: AgentStatus::Working,
        workdir: PathBuf::from("."),
        branch: None,
        file_ownership: Vec::new(),
        created_at: Utc::now(),
        last_output_at: None,
        context_usage: None,
        exit_code: None,
        metadata: HashMap::new(),
    }
}
