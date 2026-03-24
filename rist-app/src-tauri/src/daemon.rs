use std::io;
use std::path::PathBuf;

use dirs::home_dir;
use rist_shared::protocol::{decode_frame_async, encode_frame_async, Event, Request, Response};
use rist_shared::{AgentInfo, AgentType, EventFilter, SessionId, Task};
use tokio::net::UnixStream;

pub struct DaemonClient {
    stream: UnixStream,
}

impl DaemonClient {
    pub async fn connect() -> io::Result<Self> {
        let socket_path = socket_path()?;
        let stream = UnixStream::connect(socket_path).await?;
        Ok(Self { stream })
    }

    pub async fn connect_subscribed() -> io::Result<Self> {
        let mut client = Self::connect().await?;
        let response = client
            .request(Request::Subscribe {
                events: vec![
                    EventFilter::PtyData,
                    EventFilter::StatusChange,
                    EventFilter::AgentExited,
                    EventFilter::TaskUpdate,
                    EventFilter::ContextWarning,
                    EventFilter::LoopDetected,
                ],
            })
            .await?;
        match response {
            Response::Ok => Ok(client),
            Response::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected subscribe response: {other:?}"),
            )),
        }
    }

    pub async fn request(&mut self, request: Request) -> io::Result<Response> {
        encode_frame_async(&mut self.stream, &request).await?;
        decode_frame_async(&mut self.stream).await
    }

    pub async fn read_event(&mut self) -> io::Result<Event> {
        decode_frame_async(&mut self.stream).await
    }

    pub async fn list_agents(&mut self) -> io::Result<Vec<AgentInfo>> {
        match self.request(Request::ListAgents).await? {
            Response::AgentList { agents } => Ok(agents),
            Response::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected list_agents response: {other:?}"),
            )),
        }
    }

    pub async fn spawn_agent(&mut self, agent_type: AgentType, task: String) -> io::Result<SessionId> {
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
            Response::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected spawn_agent response: {other:?}"),
            )),
        }
    }

    pub async fn kill_agent(&mut self, id: SessionId) -> io::Result<()> {
        match self.request(Request::KillAgent { id }).await? {
            Response::Ok => Ok(()),
            Response::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected kill_agent response: {other:?}"),
            )),
        }
    }

    pub async fn read_task_graph(&mut self) -> io::Result<Vec<Task>> {
        match self.request(Request::ReadTaskGraph).await? {
            Response::TaskGraph { tasks } => Ok(tasks),
            Response::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected read_task_graph response: {other:?}"),
            )),
        }
    }

    pub async fn write_to_agent(&mut self, id: SessionId, text: String) -> io::Result<()> {
        match self.request(Request::WriteToAgent { id, text }).await? {
            Response::Ok => Ok(()),
            Response::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected write_to_agent response: {other:?}"),
            )),
        }
    }

    pub async fn resize(&mut self, id: SessionId, cols: u16, rows: u16) -> io::Result<()> {
        match self.request(Request::Resize { id, cols, rows }).await? {
            Response::Ok => Ok(()),
            Response::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected resize response: {other:?}"),
            )),
        }
    }

    pub async fn get_buffer(&mut self, id: SessionId) -> io::Result<String> {
        match self.request(Request::GetBuffer { id }).await? {
            Response::Output { lines } => Ok(lines.join("\n")),
            Response::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected get_buffer response: {other:?}"),
            )),
        }
    }
}

pub fn socket_path() -> io::Result<PathBuf> {
    home_dir()
        .map(|path| path.join(".ristretto/daemon.sock"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory not found"))
}
