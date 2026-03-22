use std::sync::Arc;
use std::time::Duration;

use portable_pty::CommandBuilder;
use serde_json::Value;
use tempfile::tempdir;
use tokio::sync::Mutex;

use rist_shared::protocol::{decode_frame_async, encode_frame_async, Event, Request, Response};
use rist_shared::{AgentStatus, AgentType};
use ristd::agent_adapter::AgentAdapter;
use ristd::planner::TaskPlanner;
use ristd::pty_manager::PtyManager;
use ristd::session_store::SessionStore;
use ristd::socket_server::SocketServer;

#[derive(Debug)]
struct TestAdapter;

impl AgentAdapter for TestAdapter {
    fn name(&self) -> &str {
        "test"
    }

    fn build_command(
        &self,
        _task: &str,
        workdir: &std::path::Path,
        _mcp_config: Option<&std::path::Path>,
    ) -> CommandBuilder {
        let mut command = CommandBuilder::new("sh");
        command.args(["-lc", "printf 'ready\\n'; exit 0"]);
        command.cwd(workdir);
        command
    }

    fn detect_status(&self, _recent_output: &[u8], _elapsed: Duration) -> AgentStatus {
        AgentStatus::Working
    }

    fn detect_loop(&self, _recent_output: &[u8]) -> Option<String> {
        None
    }
}

async fn read_until_response(stream: &mut tokio::net::UnixStream, expected_type: &str) -> Response {
    loop {
        let frame: Value = decode_frame_async(stream).await.expect("frame");
        if frame
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|frame_type| frame_type == expected_type)
        {
            return serde_json::from_value(frame).expect("response decode");
        }
    }
}

#[tokio::test]
async fn spawn_then_list() {
    let temp = tempdir().expect("tempdir");
    let socket_path = temp.path().join("daemon.sock");
    let sessions_path = temp.path().join("sessions.json");
    let mut manager = PtyManager::new();
    manager.register_adapter(AgentType::Custom("test".to_owned()), Box::new(TestAdapter));

    let server = SocketServer::bind(
        &socket_path,
        Arc::new(Mutex::new(manager)),
        Arc::new(Mutex::new(SessionStore::new(sessions_path))),
        Arc::new(Mutex::new(TaskPlanner::new(
            temp.path().join("task_graph.json"),
        ))),
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
            agent_type: AgentType::Custom("test".to_owned()),
            task: "test task".to_owned(),
            repo_path: Some(temp.path().to_path_buf()),
            file_ownership: Vec::new(),
        },
    )
    .await
    .expect("spawn request");
    let spawned: Response = decode_frame_async(&mut stream)
        .await
        .expect("spawn response");
    let id = match spawned {
        Response::AgentSpawned { id } => id,
        other => panic!("unexpected response: {other:?}"),
    };

    let _: Event = decode_frame_async(&mut stream).await.expect("event");

    encode_frame_async(&mut stream, &Request::ListAgents)
        .await
        .expect("list request");
    let listed = read_until_response(&mut stream, "agent_list").await;
    match listed {
        Response::AgentList { agents } => {
            assert_eq!(agents.len(), 1);
            assert_eq!(agents[0].id, id);
        }
        other => panic!("unexpected response: {other:?}"),
    }

    server_task.abort();
}

#[tokio::test]
async fn spawn_events_broadcast_to_all_clients() {
    let temp = tempdir().expect("tempdir");
    let socket_path = temp.path().join("daemon.sock");
    let sessions_path = temp.path().join("sessions.json");
    let mut manager = PtyManager::new();
    manager.register_adapter(AgentType::Custom("test".to_owned()), Box::new(TestAdapter));

    let server = SocketServer::bind(
        &socket_path,
        Arc::new(Mutex::new(manager)),
        Arc::new(Mutex::new(SessionStore::new(sessions_path))),
        Arc::new(Mutex::new(TaskPlanner::new(
            temp.path().join("task_graph.json"),
        ))),
    )
    .await
    .expect("bind");
    let server_task = tokio::spawn(server.run());

    let mut first = tokio::net::UnixStream::connect(&socket_path)
        .await
        .expect("first connect");
    let mut second = tokio::net::UnixStream::connect(&socket_path)
        .await
        .expect("second connect");

    encode_frame_async(
        &mut first,
        &Request::SpawnAgent {
            agent_type: AgentType::Custom("test".to_owned()),
            task: "broadcast".to_owned(),
            repo_path: Some(temp.path().to_path_buf()),
            file_ownership: Vec::new(),
        },
    )
    .await
    .expect("spawn request");

    let _: Response = decode_frame_async(&mut first)
        .await
        .expect("spawn response");
    let first_event: Event = decode_frame_async(&mut first).await.expect("first event");
    let second_event: Event = decode_frame_async(&mut second).await.expect("second event");

    assert_eq!(first_event, second_event);

    server_task.abort();
}
