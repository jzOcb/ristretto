//! PTY-backed agent session management.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use rist_shared::{AgentInfo, AgentStatus, AgentType, SessionId};

use crate::ring_buffer::RingBuffer;

struct PtySession {
    info: AgentInfo,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send>,
    ring_buffer: Arc<Mutex<RingBuffer>>,
}

/// PTY session manager for active agents.
pub struct PtyManager {
    sessions: HashMap<SessionId, PtySession>,
}

impl PtyManager {
    /// Creates an empty PTY manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Spawns a placeholder PTY-backed agent session.
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
        let mut command = CommandBuilder::new("sh");
        command.arg("-lc");
        command.arg(format!(
            "printf '%s\\n' {}; exec sleep 3600",
            shell_escape(&format!("{:?} placeholder agent started for: {}", agent_type, task))
        ));
        command.cwd(workdir.clone());

        let child = pair.slave.spawn_command(command).map_err(io::Error::other)?;
        let reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
        let writer = pair.master.take_writer().map_err(io::Error::other)?;
        let ring_buffer = Arc::new(Mutex::new(RingBuffer::default()));
        let ring_buffer_for_thread = Arc::clone(&ring_buffer);

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
            },
        );
        Ok(id)
    }

    /// Terminates an agent using `SIGTERM`, then `SIGKILL` after a grace period.
    pub fn kill_agent(&mut self, id: SessionId) -> io::Result<()> {
        let mut session = self
            .sessions
            .remove(&id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        if let Some(process_id) = session.child.process_id() {
            let pid = Pid::from_raw(process_id as i32);
            kill(pid, Signal::SIGTERM).map_err(io::Error::other)?;
            thread::sleep(Duration::from_secs(2));
            if session.child.try_wait().map_err(io::Error::other)?.is_none() {
                kill(pid, Signal::SIGKILL).map_err(io::Error::other)?;
            }
        } else {
            session.child.kill().map_err(io::Error::other)?;
        }
        let _ = session.child.wait().map_err(io::Error::other)?;
        Ok(())
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
        Ok(buffer.drain_all())
    }

    /// Lists the currently known agents.
    #[must_use]
    pub fn list_agents(&self) -> Vec<AgentInfo> {
        self.sessions.values().map(|session| session.info.clone()).collect()
    }
}

fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
