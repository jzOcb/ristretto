mod daemon;
mod pty;

use std::process::Stdio;
use std::time::Duration;

use daemon::DaemonClient;
use pty::{bytes_to_text, AgentOutputPayload};
use rist_shared::protocol::Event;
use rist_shared::{AgentInfo, AgentType, SessionId, Task};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::process::Command;

#[derive(Default)]
pub struct AppState;

#[derive(Debug, Serialize, Clone)]
struct ConnectionPayload {
    connected: bool,
    message: String,
}

#[derive(Debug, Serialize, Clone)]
struct AgentStatusPayload {
    agent_id: String,
    old_status: String,
    new_status: String,
    exit_code: Option<i32>,
}

#[derive(Debug, Serialize, Clone)]
struct TaskUpdatePayload {
    task_id: String,
    status: String,
}

#[derive(Debug, Serialize, Clone)]
struct ContextWarningPayload {
    agent_id: String,
    usage_pct: f64,
}

#[derive(Debug, Serialize, Clone)]
struct LoopDetectedPayload {
    agent_id: String,
    pattern: String,
}

async fn with_client<T, F, Fut>(_state: &State<'_, AppState>, action: F) -> Result<T, String>
where
    F: FnOnce(DaemonClient) -> Fut,
    Fut: std::future::Future<Output = std::io::Result<T>>,
{
    let client = DaemonClient::connect().await.map_err(|error| error.to_string())?;
    action(client).await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn list_agents(state: State<'_, AppState>) -> Result<Vec<AgentInfo>, String> {
    with_client(&state, |mut client| async move { client.list_agents().await }).await
}

#[tauri::command]
async fn spawn_agent(
    state: State<'_, AppState>,
    agent_type: AgentType,
    task: String,
) -> Result<String, String> {
    with_client(&state, |mut client| async move {
        client.spawn_agent(agent_type, task).await.map(|id| id.0.to_string())
    })
    .await
}

#[tauri::command]
async fn kill_agent(state: State<'_, AppState>, agent_id: String) -> Result<(), String> {
    let session_id = parse_session_id(&agent_id)?;
    with_client(&state, |mut client| async move { client.kill_agent(session_id).await }).await
}

#[tauri::command]
async fn get_task_graph(state: State<'_, AppState>) -> Result<Vec<Task>, String> {
    with_client(&state, |mut client| async move { client.read_task_graph().await }).await
}

#[tauri::command]
async fn write_to_pty(
    state: State<'_, AppState>,
    agent_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    let session_id = parse_session_id(&agent_id)?;
    with_client(&state, |mut client| async move {
        client.write_to_agent(session_id, bytes_to_text(&data)).await
    })
    .await
}

#[tauri::command]
async fn resize_pty(
    state: State<'_, AppState>,
    agent_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let session_id = parse_session_id(&agent_id)?;
    with_client(&state, |mut client| async move { client.resize(session_id, cols, rows).await }).await
}

#[tauri::command]
async fn get_agent_buffer(state: State<'_, AppState>, agent_id: String) -> Result<String, String> {
    let session_id = parse_session_id(&agent_id)?;
    with_client(&state, |mut client| async move { client.get_buffer(session_id).await }).await
}

#[tauri::command]
async fn start_daemon() -> Result<(), String> {
    let mut command = Command::new("ristd");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    command.spawn().map(|_| ()).map_err(|error| error.to_string())
}

fn parse_session_id(value: &str) -> Result<SessionId, String> {
    uuid::Uuid::parse_str(value)
        .map(SessionId)
        .map_err(|error| error.to_string())
}

fn emit_connection(handle: &AppHandle, connected: bool, message: impl Into<String>) {
    let _ = handle.emit(
        "daemon-connection",
        ConnectionPayload {
            connected,
            message: message.into(),
        },
    );
}

async fn event_loop(handle: AppHandle) {
    loop {
        match DaemonClient::connect_subscribed().await {
            Ok(mut client) => {
                emit_connection(&handle, true, "Connected to ristd");

                loop {
                    match client.read_event().await {
                        Ok(Event::PtyData { id, data }) => {
                            let _ = handle.emit(
                                "agent-output",
                                AgentOutputPayload {
                                    agent_id: id.0.to_string(),
                                    data: bytes_to_text(&data),
                                },
                            );
                        }
                        Ok(Event::StatusChange { id, old, new }) => {
                            let _ = handle.emit(
                                "agent-status",
                                AgentStatusPayload {
                                    agent_id: id.0.to_string(),
                                    old_status: old.to_string(),
                                    new_status: new.to_string(),
                                    exit_code: None,
                                },
                            );
                        }
                        Ok(Event::AgentExited { id, exit_code }) => {
                            let _ = handle.emit(
                                "agent-status",
                                AgentStatusPayload {
                                    agent_id: id.0.to_string(),
                                    old_status: "working".to_owned(),
                                    new_status: if exit_code == 0 {
                                        "done".to_owned()
                                    } else {
                                        "error".to_owned()
                                    },
                                    exit_code: Some(exit_code),
                                },
                            );
                        }
                        Ok(Event::TaskUpdate { task_id, status }) => {
                            let _ = handle.emit(
                                "task-update",
                                TaskUpdatePayload {
                                    task_id,
                                    status: format!("{status:?}").to_lowercase(),
                                },
                            );
                        }
                        Ok(Event::ContextWarning { id, usage_pct }) => {
                            let _ = handle.emit(
                                "context-warning",
                                ContextWarningPayload {
                                    agent_id: id.0.to_string(),
                                    usage_pct,
                                },
                            );
                        }
                        Ok(Event::LoopDetected { id, pattern }) => {
                            let _ = handle.emit(
                                "loop-detected",
                                LoopDetectedPayload {
                                    agent_id: id.0.to_string(),
                                    pattern,
                                },
                            );
                        }
                        Ok(Event::Unknown) => {}
                        Err(error) => {
                            emit_connection(&handle, false, error.to_string());
                            break;
                        }
                    }
                }
            }
            Err(error) => {
                emit_connection(&handle, false, error.to_string());
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState)
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                event_loop(handle).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_agents,
            spawn_agent,
            kill_agent,
            get_task_graph,
            write_to_pty,
            resize_pty,
            get_agent_buffer,
            start_daemon
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
