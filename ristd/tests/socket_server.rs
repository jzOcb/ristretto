use std::sync::Arc;

use tempfile::tempdir;
use tokio::sync::Mutex;

use rist_shared::protocol::{Event, Request, Response, decode_frame_async, encode_frame_async};
use rist_shared::AgentType;
use ristd::pty_manager::PtyManager;
use ristd::session_store::SessionStore;
use ristd::socket_server::SocketServer;

#[tokio::test]
async fn spawn_then_list() {
    let temp = tempdir().expect("tempdir");
    let socket_path = temp.path().join("daemon.sock");
    let sessions_path = temp.path().join("sessions.json");

    let server = SocketServer::bind(
        &socket_path,
        Arc::new(Mutex::new(PtyManager::new())),
        Arc::new(Mutex::new(SessionStore::new(sessions_path))),
    )
    .await
    .expect("bind");
    let server_task = tokio::spawn(server.run());

    let mut stream = tokio::net::UnixStream::connect(&socket_path)
        .await
        .expect("connect");
    encode_frame_async(
        &mut stream,
        &Request::SpawnAgent {
            agent_type: AgentType::Codex,
            task: "test task".to_owned(),
            repo_path: Some(temp.path().to_path_buf()),
            file_ownership: Vec::new(),
        },
    )
    .await
    .expect("spawn request");
    let spawned: Response = decode_frame_async(&mut stream).await.expect("spawn response");
    let id = match spawned {
        Response::AgentSpawned { id } => id,
        other => panic!("unexpected response: {other:?}"),
    };

    let _: Event = decode_frame_async(&mut stream).await.expect("event");

    encode_frame_async(&mut stream, &Request::ListAgents)
        .await
        .expect("list request");
    let listed: Response = decode_frame_async(&mut stream).await.expect("list response");
    match listed {
        Response::AgentList { agents } => {
            assert_eq!(agents.len(), 1);
            assert_eq!(agents[0].id, id);
        }
        other => panic!("unexpected response: {other:?}"),
    }

    server_task.abort();
}
