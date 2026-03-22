//! Unix socket IPC server for daemon clients.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, mpsc};

use rist_shared::protocol::{decode_frame_async, encode_frame_async, Event, Request, Response};
use rist_shared::{AgentInfo, AgentType, SessionId};

use crate::pty_manager::PtyManager;
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
    clients: Arc<Mutex<HashMap<usize, mpsc::UnboundedSender<ServerFrame>>>>,
    next_client_id: AtomicUsize,
}

impl SocketServer {
    /// Binds a new Unix socket server to `path`.
    pub async fn bind(
        path: &Path,
        pty_manager: Arc<Mutex<PtyManager>>,
        session_store: Arc<Mutex<SessionStore>>,
    ) -> io::Result<Self> {
        let listener = UnixListener::bind(path)?;
        Ok(Self {
            listener,
            pty_manager,
            session_store,
            clients: Arc::new(Mutex::new(HashMap::new())),
            next_client_id: AtomicUsize::new(1),
        })
    }

    /// Runs the accept loop until the task is cancelled.
    pub async fn run(self) -> io::Result<()> {
        loop {
            let (stream, _) = self.listener.accept().await?;
            let client_id = self.next_client_id.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = mpsc::unbounded_channel();
            self.clients.lock().await.insert(client_id, tx);

            let pty_manager = Arc::clone(&self.pty_manager);
            let session_store = Arc::clone(&self.session_store);
            let clients = Arc::clone(&self.clients);
            tokio::spawn(async move {
                let _ = handle_connection(client_id, stream, rx, pty_manager, session_store, clients).await;
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
    clients: Arc<Mutex<HashMap<usize, mpsc::UnboundedSender<ServerFrame>>>>,
) -> io::Result<()> {
    let (mut reader, mut writer) = stream.into_split();
    let writer_task = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            match frame {
                ServerFrame::Response(response) => encode_frame_async(&mut writer, &response).await?,
                ServerFrame::Event(event) => encode_frame_async(&mut writer, &event).await?,
            }
        }
        Ok::<(), io::Error>(())
    });

    loop {
        let request: Request = match decode_frame_async(&mut reader).await {
            Ok(request) => request,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error),
        };
        let response = dispatch_request(&request, &pty_manager, &session_store).await;
        let sender = {
            let clients_guard = clients.lock().await;
            clients_guard.get(&client_id).cloned()
        };
        if let Some(sender) = sender {
            let _ = sender.send(ServerFrame::Response(response.clone()));
            if let Request::SpawnAgent { .. } = request {
                if let Response::AgentSpawned { id } = response {
                    let _ = sender.send(ServerFrame::Event(Event::StatusChange {
                        id,
                        old: rist_shared::AgentStatus::Idle,
                        new: rist_shared::AgentStatus::Working,
                    }));
                }
            }
        }
    }

    clients.lock().await.remove(&client_id);
    writer_task.abort();
    Ok(())
}

async fn dispatch_request(
    request: &Request,
    pty_manager: &Arc<Mutex<PtyManager>>,
    session_store: &Arc<Mutex<SessionStore>>,
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
            let workdir = repo_path
                .clone()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let mut manager = pty_manager.lock().await;
            match manager.spawn_agent(
                agent_type.clone(),
                task.clone(),
                workdir,
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
            let mut manager = pty_manager.lock().await;
            match manager.kill_agent(*id) {
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
        Request::GetBuffer { id } => {
            let manager = pty_manager.lock().await;
            match manager.get_buffer(*id) {
                Ok(bytes) => Response::Output {
                    lines: String::from_utf8_lossy(&bytes).lines().map(ToOwned::to_owned).collect(),
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
        _ => Response::Error {
            message: "request not implemented in Phase 1A".to_owned(),
        },
    }
}

fn placeholder_agent(id: SessionId, agent_type: AgentType, task: String) -> AgentInfo {
    AgentInfo {
        id,
        agent_type,
        task,
        status: rist_shared::AgentStatus::Working,
        workdir: PathBuf::from("."),
        branch: None,
        file_ownership: Vec::new(),
        created_at: chrono::Utc::now(),
        last_output_at: None,
        context_usage: None,
        exit_code: None,
        metadata: HashMap::new(),
    }
}
