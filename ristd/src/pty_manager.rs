//! PTY-backed agent session management.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use portable_pty::{native_pty_system, Child, MasterPty, PtySize};
use rist_shared::protocol::Event;
use rist_shared::{AgentInfo, AgentStatus, AgentType, SessionId};

use crate::agent_adapter::{
    agent_type_key, AgentAdapter, ClaudeCodeAdapter, CodexAdapter, DefaultAdapter, GeminiAdapter,
};
use crate::ring_buffer::RingBuffer;

const STATUS_SAMPLE_BYTES: usize = 8192;

struct PtySession {
    info: AgentInfo,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    ring_buffer: Arc<Mutex<RingBuffer>>,
    last_output_at: Arc<Mutex<Option<DateTime<Utc>>>>,
    adapter_key: String,
    exited: bool,
}

/// PTY session manager for active agents.
pub struct PtyManager {
    sessions: HashMap<SessionId, PtySession>,
    adapters: HashMap<String, Box<dyn AgentAdapter>>,
    pending_events: VecDeque<Event>,
}

impl PtyManager {
    /// Creates an empty PTY manager with the built-in adapters registered.
    #[must_use]
    pub fn new() -> Self {
        let mut manager = Self {
            sessions: HashMap::new(),
            adapters: HashMap::new(),
            pending_events: VecDeque::new(),
        };
        manager.register_adapter(AgentType::Claude, Box::new(ClaudeCodeAdapter));
        manager.register_adapter(AgentType::Codex, Box::new(CodexAdapter));
        manager.register_adapter(AgentType::Gemini, Box::new(GeminiAdapter));
        manager
    }

    /// Registers or replaces the adapter for an agent family.
    pub fn register_adapter(&mut self, agent_type: AgentType, adapter: Box<dyn AgentAdapter>) {
        self.adapters.insert(agent_type_key(&agent_type), adapter);
    }

    /// Spawns a PTY-backed agent session.
    pub fn spawn_agent(
        &mut self,
        agent_type: AgentType,
        task: String,
        workdir: PathBuf,
        file_ownership: Vec<PathBuf>,
    ) -> io::Result<SessionId> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io::Error::other)?;

        let adapter_key = agent_type_key(&agent_type);
        let command = self
            .adapters
            .get(&adapter_key)
            .map(|adapter| adapter.build_command(&task, &workdir, None))
            .unwrap_or_else(|| {
                DefaultAdapter::new(agent_type.clone()).build_command(&task, &workdir, None)
            });

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(io::Error::other)?;
        let reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
        let writer = pair.master.take_writer().map_err(io::Error::other)?;
        let ring_buffer = Arc::new(Mutex::new(RingBuffer::default()));
        let ring_buffer_for_thread = Arc::clone(&ring_buffer);
        let last_output_at = Arc::new(Mutex::new(Some(Utc::now())));
        let last_output_for_thread = Arc::clone(&last_output_at);

        thread::spawn(move || {
            let mut reader = reader;
            let mut chunk = [0_u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(count) => {
                        if let Ok(mut buffer) = ring_buffer_for_thread.lock() {
                            buffer.push(&chunk[..count]);
                        }
                        if let Ok(mut last_output) = last_output_for_thread.lock() {
                            *last_output = Some(Utc::now());
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let id = SessionId::new();
        let now = Utc::now();
        let info = AgentInfo {
            id,
            agent_type,
            task,
            status: AgentStatus::Working,
            workdir,
            branch: None,
            file_ownership,
            created_at: now,
            last_output_at: Some(now),
            context_usage: None,
            exit_code: None,
            metadata: HashMap::new(),
        };

        self.sessions.insert(
            id,
            PtySession {
                info,
                master: pair.master,
                writer,
                child,
                ring_buffer,
                last_output_at,
                adapter_key,
                exited: false,
            },
        );
        Ok(id)
    }

    /// Terminates an agent using `SIGTERM`, then `SIGKILL` after a grace period.
    pub fn kill_agent(&mut self, id: SessionId) -> io::Result<()> {
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        if session.exited {
            return Ok(());
        }

        let exit_status = if let Some(process_id) = session.child.process_id() {
            let pid = Pid::from_raw(process_id as i32);
            kill(pid, Signal::SIGTERM).map_err(io::Error::other)?;
            thread::sleep(Duration::from_secs(2));
            if session
                .child
                .try_wait()
                .map_err(io::Error::other)?
                .is_none()
            {
                kill(pid, Signal::SIGKILL).map_err(io::Error::other)?;
            }
            session.child.wait().map_err(io::Error::other)?
        } else {
            session.child.kill().map_err(io::Error::other)?;
            session.child.wait().map_err(io::Error::other)?
        };

        Self::apply_exit_state(session, &mut self.pending_events, exit_status);
        Ok(())
    }

    /// Polls all child processes and updates in-memory status/exit metadata.
    pub fn check_exits(&mut self) {
        let now = Utc::now();

        for session in self.sessions.values_mut() {
            Self::refresh_runtime_state(session, &self.adapters, &mut self.pending_events, now);

            if session.exited {
                continue;
            }

            if let Ok(Some(exit_status)) = session.child.try_wait() {
                Self::apply_exit_state(session, &mut self.pending_events, exit_status);
            }
        }
    }

    /// Drains pending daemon events generated by PTY state changes.
    pub fn drain_events(&mut self) -> Vec<Event> {
        self.pending_events.drain(..).collect()
    }

    /// Returns the last `lines` lines for the selected session.
    pub fn get_output(&self, id: SessionId, lines: usize) -> io::Result<Vec<String>> {
        let session = self
            .sessions
            .get(&id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        let buffer = session
            .ring_buffer
            .lock()
            .map_err(|_| io::Error::other("ring buffer poisoned"))?;
        Ok(buffer.tail_lines(lines))
    }

    /// Writes text to the session PTY.
    pub fn write_to_agent(&mut self, id: SessionId, text: &str) -> io::Result<()> {
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        session.writer.write_all(text.as_bytes())?;
        session.writer.flush()
    }

    /// Resizes the session PTY.
    pub fn resize(&mut self, id: SessionId, cols: u16, rows: u16) -> io::Result<()> {
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io::Error::other)
    }

    /// Returns the complete buffered contents for a session.
    pub fn get_buffer(&self, id: SessionId) -> io::Result<Vec<u8>> {
        let session = self
            .sessions
            .get(&id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        let buffer = session
            .ring_buffer
            .lock()
            .map_err(|_| io::Error::other("ring buffer poisoned"))?;
        Ok(buffer.snapshot())
    }

    /// Lists the currently known agents.
    #[must_use]
    pub fn list_agents(&self) -> Vec<AgentInfo> {
        self.sessions
            .values()
            .map(|session| {
                let mut info = session.info.clone();
                info.last_output_at = session.last_output_at.lock().ok().and_then(|guard| *guard);
                info
            })
            .collect()
    }

    fn refresh_runtime_state(
        session: &mut PtySession,
        adapters: &HashMap<String, Box<dyn AgentAdapter>>,
        pending_events: &mut VecDeque<Event>,
        now: DateTime<Utc>,
    ) {
        session.info.last_output_at = session.last_output_at.lock().ok().and_then(|guard| *guard);
        if session.exited {
            return;
        }

        let recent_output = session
            .ring_buffer
            .lock()
            .map(|buffer| buffer.tail(STATUS_SAMPLE_BYTES))
            .unwrap_or_default();
        let elapsed = session
            .info
            .last_output_at
            .and_then(|last_output| (now - last_output).to_std().ok())
            .unwrap_or_default();

        let detected = if let Some(adapter) = adapters.get(&session.adapter_key) {
            adapter.detect_status(&recent_output, elapsed)
        } else {
            DefaultAdapter::new(session.info.agent_type.clone())
                .detect_status(&recent_output, elapsed)
        };
        if detected != session.info.status {
            let old = session.info.status.clone();
            session.info.status = detected.clone();
            pending_events.push_back(Event::StatusChange {
                id: session.info.id,
                old,
                new: detected,
            });
        }
    }

    fn apply_exit_state(
        session: &mut PtySession,
        pending_events: &mut VecDeque<Event>,
        exit_status: portable_pty::ExitStatus,
    ) {
        session.exited = true;
        let exit_code = i32::try_from(exit_status.exit_code()).unwrap_or(i32::MAX);
        session.info.exit_code = Some(exit_code);
        let next_status = if exit_status.success() {
            AgentStatus::Done
        } else {
            AgentStatus::Error
        };
        if session.info.status != next_status {
            let old = session.info.status.clone();
            session.info.status = next_status.clone();
            pending_events.push_back(Event::StatusChange {
                id: session.info.id,
                old,
                new: next_status,
            });
        }
        pending_events.push_back(Event::AgentExited {
            id: session.info.id,
            exit_code,
        });
    }
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::thread;
    use std::time::Duration;

    use portable_pty::CommandBuilder;
    use rist_shared::protocol::Event;
    use rist_shared::{AgentStatus, AgentType};

    use crate::agent_adapter::AgentAdapter;

    use super::PtyManager;

    #[derive(Debug)]
    struct TestAdapter;

    impl AgentAdapter for TestAdapter {
        fn name(&self) -> &str {
            "test"
        }

        fn build_command(
            &self,
            _task: &str,
            workdir: &Path,
            _mcp_config: Option<&Path>,
        ) -> CommandBuilder {
            let mut command = CommandBuilder::new("sh");
            command.args(["-lc", "printf 'done\\n'; exit 0"]);
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

    #[test]
    fn check_exits_records_finished_processes() {
        let mut manager = PtyManager::new();
        manager.register_adapter(AgentType::Custom("test".to_owned()), Box::new(TestAdapter));

        let temp = tempfile::tempdir().expect("tempdir");
        let id = manager
            .spawn_agent(
                AgentType::Custom("test".to_owned()),
                "task".to_owned(),
                temp.path().to_path_buf(),
                Vec::new(),
            )
            .expect("spawn");

        thread::sleep(Duration::from_millis(100));
        manager.check_exits();

        let agent = manager
            .list_agents()
            .into_iter()
            .find(|agent| agent.id == id)
            .expect("agent present");
        assert_eq!(agent.status, AgentStatus::Done);
        assert_eq!(agent.exit_code, Some(0));

        let events = manager.drain_events();
        assert!(events.iter().any(|event| matches!(
            event,
            Event::AgentExited { id: event_id, exit_code: 0 } if *event_id == id
        )));
    }
}
