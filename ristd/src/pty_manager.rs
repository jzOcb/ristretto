//! PTY-backed agent session management.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use portable_pty::{native_pty_system, Child, MasterPty, PtySize};
use rist_shared::protocol::Event;
use rist_shared::{AgentInfo, AgentStatus, AgentType, ContextUsage, MergeStrategy, SessionId};
use tracing::error;

use crate::agent_adapter::{
    agent_type_key, AgentAdapter, ClaudeCodeAdapter, CodexAdapter, DefaultAdapter, GeminiAdapter,
};
use crate::context_injector::generate_context_file;
use crate::context_monitor::ContextMonitor;
use crate::file_ownership::FileOwnership;
use crate::git_manager::{GitManager, MergePreview, MergeResult};
use crate::recovery::{RecoveryAction, RecoveryManager};
use crate::review_engine::ReviewRequest;
use crate::ring_buffer::RingBuffer;

const DEFAULT_ROTATION_THRESHOLD: f64 = 80.0;
const STATUS_SAMPLE_BYTES: usize = 8192;
const CONTEXT_FILE_NAME: &str = "RISTRETTO.md";
const PROGRESS_FILE_NAME: &str = "PROGRESS.md";
const RECENT_OUTPUT_LIMIT: usize = 64;

#[derive(Debug, Default)]
struct SessionTelemetry {
    total_output_bytes: usize,
    recent_lines: VecDeque<String>,
}

struct PtySession {
    info: AgentInfo,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    ring_buffer: Arc<Mutex<RingBuffer>>,
    last_output_at: Arc<Mutex<Option<DateTime<Utc>>>>,
    telemetry: Arc<Mutex<SessionTelemetry>>,
    adapter_key: String,
    recovery_attempts: usize,
    idle_nudged: bool,
    context_warning_sent: bool,
    last_observed_output_bytes: usize,
    last_recovery_issue: Option<String>,
    last_loop_pattern: Option<String>,
    exited: bool,
}

pub struct IdleCheck {
    pub status: AgentStatus,
    pub complete: bool,
}

pub enum TerminationRequest {
    Completed,
    GracePeriod,
}

/// PTY session manager for active agents.
pub struct PtyManager {
    sessions: HashMap<SessionId, PtySession>,
    archived: HashMap<SessionId, AgentInfo>,
    adapters: HashMap<String, Box<dyn AgentAdapter>>,
    pending_events: Arc<Mutex<VecDeque<Event>>>,
    file_ownership: FileOwnership,
    context_monitor: ContextMonitor,
    recovery_manager: RecoveryManager,
}

impl PtyManager {
    /// Creates an empty PTY manager with the built-in adapters registered.
    #[must_use]
    pub fn new() -> Self {
        let mut manager = Self {
            sessions: HashMap::new(),
            archived: HashMap::new(),
            adapters: HashMap::new(),
            pending_events: Arc::new(Mutex::new(VecDeque::new())),
            file_ownership: FileOwnership::new(),
            context_monitor: ContextMonitor::new(DEFAULT_ROTATION_THRESHOLD),
            recovery_manager: RecoveryManager::new(),
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
        repo_path: Option<PathBuf>,
        file_ownership: Vec<PathBuf>,
    ) -> io::Result<SessionId> {
        let id = SessionId::new();
        self.file_ownership
            .declare(id, file_ownership.clone())
            .map_err(io::Error::other)?;

        let repo_root = match repo_path.as_deref() {
            Some(path) => RepositoryRoot::discover(path).ok(),
            None => None,
        };
        let workdir = if let Some(repo_root) = &repo_root {
            GitManager::create_worktree(&repo_root.path, id, &task)?
        } else if let Some(path) = repo_path.clone() {
            path
        } else {
            std::env::current_dir()?
        };
        let branch = if repo_root.is_some() {
            current_branch_name(&workdir)?
        } else {
            None
        };

        let now = Utc::now();
        let mut metadata = HashMap::new();
        if let Some(repo_root) = &repo_root {
            metadata.insert("repo_path".to_owned(), repo_root.path.display().to_string());
        }
        if let Some(branch_name) = &branch {
            metadata.insert("branch".to_owned(), branch_name.clone());
        }

        let info = AgentInfo {
            id,
            agent_type: agent_type.clone(),
            task: task.clone(),
            status: AgentStatus::Working,
            workdir: workdir.clone(),
            branch: branch.clone(),
            file_ownership: file_ownership.clone(),
            created_at: now,
            last_output_at: Some(now),
            context_usage: None,
            exit_code: None,
            metadata,
        };

        if let Err(error) = self.write_context_file(&info) {
            self.file_ownership.release(id);
            if let Some(repo_root) = &repo_root {
                let _ = GitManager::remove_worktree(&repo_root.path, &workdir, true);
            }
            return Err(error);
        }

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

        let child = match pair.slave.spawn_command(command) {
            Ok(child) => child,
            Err(error) => {
                self.file_ownership.release(id);
                if let Some(repo_root) = &repo_root {
                    let _ = GitManager::remove_worktree(&repo_root.path, &workdir, true);
                }
                return Err(io::Error::other(error));
            }
        };
        let reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
        let writer = pair.master.take_writer().map_err(io::Error::other)?;
        let ring_buffer = Arc::new(Mutex::new(RingBuffer::default()));
        let ring_buffer_for_thread = Arc::clone(&ring_buffer);
        let last_output_at = Arc::new(Mutex::new(Some(Utc::now())));
        let last_output_for_thread = Arc::clone(&last_output_at);
        let telemetry = Arc::new(Mutex::new(SessionTelemetry::default()));
        let telemetry_for_thread = Arc::clone(&telemetry);
        let pending_events = Arc::clone(&self.pending_events);

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
                        if let Ok(mut telemetry) = telemetry_for_thread.lock() {
                            telemetry.total_output_bytes += count;
                            let text = String::from_utf8_lossy(&chunk[..count]);
                            for line in text.lines() {
                                let trimmed = line.trim();
                                if trimmed.is_empty() {
                                    continue;
                                }
                                telemetry.recent_lines.push_back(trimmed.to_owned());
                                while telemetry.recent_lines.len() > RECENT_OUTPUT_LIMIT {
                                    let _ = telemetry.recent_lines.pop_front();
                                }
                            }
                        }
                        if let Ok(mut last_output) = last_output_for_thread.lock() {
                            *last_output = Some(Utc::now());
                        }
                        if let Ok(mut events) = pending_events.lock() {
                            events.push_back(Event::PtyData {
                                id,
                                data: chunk[..count].to_vec(),
                            });
                        }
                    }
                    Err(error) => {
                        error!("PTY reader failed for session {id:?}: {error}");
                        break;
                    }
                }
            }
        });

        self.sessions.insert(
            id,
            PtySession {
                info,
                master: pair.master,
                writer,
                child,
                ring_buffer,
                last_output_at,
                telemetry,
                adapter_key,
                recovery_attempts: 0,
                idle_nudged: false,
                context_warning_sent: false,
                last_observed_output_bytes: 0,
                last_recovery_issue: None,
                last_loop_pattern: None,
                exited: false,
            },
        );
        Ok(id)
    }

    /// Terminates an agent using `SIGTERM`, then `SIGKILL` after a grace period.
    pub fn kill_agent(&mut self, id: SessionId) -> io::Result<()> {
        let _ = self.request_termination(id)?;
        self.finish_termination(id)
    }

    /// Polls all child processes and updates in-memory status/exit metadata.
    pub fn check_exits(&mut self) {
        let now = Utc::now();

        for session in self.sessions.values_mut() {
            Self::refresh_runtime_state(session, &self.adapters, &self.pending_events, now);

            if session.exited {
                continue;
            }

            if let Ok(Some(exit_status)) = session.child.try_wait() {
                Self::apply_exit_state(session, &self.pending_events, exit_status);
            }
        }
    }

    /// Drains pending daemon events generated by PTY state changes.
    pub fn drain_events(&mut self) -> Vec<Event> {
        if let Ok(mut pending_events) = self.pending_events.lock() {
            pending_events.drain(..).collect()
        } else {
            Vec::new()
        }
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
        let mut agents = self
            .sessions
            .values()
            .map(|session| {
                let mut info = session.info.clone();
                info.last_output_at = session.last_output_at.lock().ok().and_then(|guard| *guard);
                info
            })
            .collect::<Vec<_>>();
        agents.extend(self.archived.values().cloned());
        agents
    }

    /// Returns the declared file ownership map.
    #[must_use]
    pub fn ownership_map(&self) -> &HashMap<PathBuf, SessionId> {
        self.file_ownership.map()
    }

    /// Archives an agent, releasing ownership and optionally cleaning up its worktree.
    pub fn archive_agent(&mut self, id: SessionId, keep_worktree: bool) -> io::Result<AgentInfo> {
        let mut info = if let Some(mut session) = self.sessions.remove(&id) {
            if !session.exited {
                let exit_status = force_terminate_session(&mut session)?;
                Self::apply_exit_state(&mut session, &self.pending_events, exit_status);
            }
            session.info
        } else if let Some(info) = self.archived.get(&id).cloned() {
            info
        } else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "session not found"));
        };

        self.file_ownership.release(id);
        if !keep_worktree {
            self.cleanup_worktree(&info, false)?;
        }
        info.metadata
            .insert("archived".to_owned(), "true".to_owned());
        self.archived.insert(id, info.clone());
        Ok(info)
    }

    /// Returns the current merge preview for an agent branch.
    pub fn preview_merge(&self, id: SessionId) -> io::Result<MergePreview> {
        let info = self.agent_info(id)?;
        let repo_path = repo_path_from_info(info)?;
        let branch = info
            .branch
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "agent has no branch"))?;
        GitManager::preview_merge(&repo_path, branch)
    }

    /// Merges an agent branch according to `strategy`.
    pub fn merge_agent(
        &self,
        id: SessionId,
        strategy: MergeStrategy,
        message: &str,
    ) -> io::Result<MergeResult> {
        if strategy != MergeStrategy::Squash {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "only squash merge is implemented",
            ));
        }
        let info = self.agent_info(id)?;
        let repo_path = repo_path_from_info(info)?;
        let branch = info
            .branch
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "agent has no branch"))?;
        GitManager::squash_merge(&repo_path, branch, message)
    }

    /// Waits until the session reaches an idle terminal state.
    pub async fn wait_for_idle(
        &mut self,
        id: SessionId,
        timeout_secs: u64,
        settling_secs: u64,
    ) -> io::Result<AgentStatus> {
        let timeout = Duration::from_secs(timeout_secs);
        let settling = Duration::from_secs(settling_secs);
        let start = std::time::Instant::now();

        loop {
            let state = self.idle_check(id, settling)?;
            if state.complete {
                return Ok(state.status);
            }

            if start.elapsed() >= timeout {
                return Ok(state.status);
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    pub fn idle_check(&mut self, id: SessionId, settling: Duration) -> io::Result<IdleCheck> {
        self.check_exits();
        let info = self.agent_info(id)?.clone();
        let settled = info
            .last_output_at
            .and_then(|last_output| (Utc::now() - last_output).to_std().ok())
            .is_some_and(|elapsed| elapsed >= settling);

        let complete = match info.status {
            AgentStatus::Idle | AgentStatus::Waiting => settled,
            AgentStatus::Done | AgentStatus::Error | AgentStatus::Stuck => true,
            _ => false,
        };

        Ok(IdleCheck {
            status: info.status,
            complete,
        })
    }

    pub fn request_termination(&mut self, id: SessionId) -> io::Result<TerminationRequest> {
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        if session.exited {
            return Ok(TerminationRequest::Completed);
        }

        if let Some(process_id) = session.child.process_id() {
            let pid = Pid::from_raw(process_id as i32);
            kill(pid, Signal::SIGTERM).map_err(io::Error::other)?;
            if let Some(exit_status) = session.child.try_wait().map_err(io::Error::other)? {
                self.file_ownership.release(id);
                Self::apply_exit_state(session, &self.pending_events, exit_status);
                return Ok(TerminationRequest::Completed);
            }
            Ok(TerminationRequest::GracePeriod)
        } else {
            let exit_status = force_terminate_session(session)?;
            self.file_ownership.release(id);
            Self::apply_exit_state(session, &self.pending_events, exit_status);
            Ok(TerminationRequest::Completed)
        }
    }

    pub fn finish_termination(&mut self, id: SessionId) -> io::Result<()> {
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        if session.exited {
            self.file_ownership.release(id);
            return Ok(());
        }

        let exit_status =
            if let Some(exit_status) = session.child.try_wait().map_err(io::Error::other)? {
                exit_status
            } else {
                force_terminate_session(session)?
            };

        self.file_ownership.release(id);
        Self::apply_exit_state(session, &self.pending_events, exit_status);
        Ok(())
    }

    /// Runs `command` inside the agent worktree and captures its output.
    pub fn run_command(&self, id: SessionId, command: &str) -> io::Result<(String, String, i32)> {
        let info = self.agent_info(id)?;
        let argv = split_command_line(command)?;
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "command line is empty"))?;
        let output = Command::new(program)
            .args(args)
            .current_dir(&info.workdir)
            .output()?;
        let exit_code = output.status.code().unwrap_or_default();
        Ok((
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code,
        ))
    }

    /// Checks active agents for context pressure and recoverable unhealthy states.
    pub fn health_check(&mut self) -> Vec<(SessionId, RecoveryAction)> {
        self.check_exits();

        let mut actions = Vec::new();
        let adapters = &self.adapters;
        let pending_events = Arc::clone(&self.pending_events);
        let context_monitor = self.context_monitor;
        let recovery_manager = self.recovery_manager.clone();

        for session in self.sessions.values_mut() {
            if session.exited {
                continue;
            }

            let total_output_bytes = match session.telemetry.lock() {
                Ok(telemetry) => telemetry.total_output_bytes,
                Err(error) => {
                    error!(
                        "telemetry mutex poisoned during health check for session {:?}: {}",
                        session.info.id, error
                    );
                    continue;
                }
            };
            if total_output_bytes > session.last_observed_output_bytes {
                session.last_observed_output_bytes = total_output_bytes;
                session.idle_nudged = false;
                session.recovery_attempts = 0;
                session.last_recovery_issue = None;
            }

            let buffer_snapshot = match session.ring_buffer.lock() {
                Ok(buffer) => buffer.snapshot(),
                Err(error) => {
                    error!(
                        "ring buffer mutex poisoned during health check for session {:?}: {}",
                        session.info.id, error
                    );
                    continue;
                }
            };
            let usage_pct = context_monitor.estimate_usage(&session.info, &buffer_snapshot);
            session.info.context_usage = Some(ContextUsage {
                estimated_tokens: u64::try_from(total_output_bytes / 4).unwrap_or(u64::MAX),
                max_tokens: default_context_tokens(&session.info.agent_type),
                percentage: usage_pct,
            });
            if context_monitor.should_rotate(usage_pct) {
                if !session.context_warning_sent {
                    push_event(
                        &pending_events,
                        Event::ContextWarning {
                            id: session.info.id,
                            usage_pct,
                        },
                    );
                    session.context_warning_sent = true;
                }
            } else {
                session.context_warning_sent = false;
            }

            let recent_output = match session.telemetry.lock() {
                Ok(telemetry) => telemetry.recent_lines.iter().cloned().collect::<Vec<_>>(),
                Err(error) => {
                    error!(
                        "telemetry mutex poisoned when sampling recent output for session {:?}: {}",
                        session.info.id, error
                    );
                    continue;
                }
            };
            let loop_pattern = adapters
                .get(&session.adapter_key)
                .and_then(|adapter| adapter.detect_loop(&buffer_snapshot));
            if loop_pattern != session.last_loop_pattern {
                if let Some(pattern) = &loop_pattern {
                    push_event(
                        &pending_events,
                        Event::LoopDetected {
                            id: session.info.id,
                            pattern: pattern.clone(),
                        },
                    );
                }
                session.last_loop_pattern = loop_pattern.clone();
            }

            let idle_retry_count = usize::from(session.idle_nudged);
            let retry_count = if loop_pattern.is_some() || session.info.status == AgentStatus::Error
            {
                session.recovery_attempts
            } else {
                idle_retry_count
            };
            let action = recovery_manager.evaluate(&session.info, &recent_output, retry_count);
            let issue_key = issue_key(&session.info, loop_pattern.as_deref());
            if let Some(key) = issue_key {
                if session.last_recovery_issue.as_deref() == Some(key.as_str()) {
                    continue;
                }
                session.last_recovery_issue = Some(key);
            } else {
                session.last_recovery_issue = None;
            }

            if let Some(action) = action {
                match &action {
                    RecoveryAction::Nudge(_) => {
                        session.idle_nudged = true;
                    }
                    RecoveryAction::Restart { .. } => {
                        session.recovery_attempts += 1;
                        session.idle_nudged = false;
                    }
                    RecoveryAction::Escalate { .. } | RecoveryAction::Fail(_) => {}
                }
                actions.push((session.info.id, action));
            }
        }

        actions
    }

    /// Sends a context-rotation prompt to the agent session.
    pub fn trigger_rotation(&mut self, id: SessionId) -> io::Result<()> {
        let prompt = {
            let session = self
                .sessions
                .get(&id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
            let recent_output = session
                .telemetry
                .lock()
                .map(|telemetry| telemetry.recent_lines.iter().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            let progress_path = session.info.workdir.join(PROGRESS_FILE_NAME);
            self.context_monitor.rotation_prompt(
                &session.info,
                &recent_output,
                progress_path.to_str(),
            )
        };
        self.write_to_agent(id, &format!("{prompt}\n"))
    }

    /// Builds a cross-review request for the selected agent session.
    pub fn request_review(&mut self, id: SessionId) -> io::Result<ReviewRequest> {
        let info = self.agent_info(id)?.clone();
        let repo_path = repo_path_from_info(&info)?;
        let branch = info
            .branch
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "agent has no branch"))?;
        let diff = GitManager::preview_merge(&repo_path, branch)?.diff;
        let file_list = git_diff_name_only(&repo_path, branch)?;

        Ok(ReviewRequest {
            source_agent: id,
            source_type: info.agent_type,
            diff,
            task_description: info.task,
            file_list,
        })
    }

    fn refresh_runtime_state(
        session: &mut PtySession,
        adapters: &HashMap<String, Box<dyn AgentAdapter>>,
        pending_events: &Arc<Mutex<VecDeque<Event>>>,
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
            if let Ok(mut events) = pending_events.lock() {
                events.push_back(Event::StatusChange {
                    id: session.info.id,
                    old,
                    new: detected,
                });
            }
        }
    }

    fn apply_exit_state(
        session: &mut PtySession,
        pending_events: &Arc<Mutex<VecDeque<Event>>>,
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
            if let Ok(mut events) = pending_events.lock() {
                events.push_back(Event::StatusChange {
                    id: session.info.id,
                    old,
                    new: next_status.clone(),
                });
            }
        }
        if let Ok(mut events) = pending_events.lock() {
            events.push_back(Event::AgentExited {
                id: session.info.id,
                exit_code,
            });
        }
    }

    fn write_context_file(&self, info: &AgentInfo) -> io::Result<()> {
        if info.branch.is_none() {
            return Ok(());
        }
        let peers = self
            .list_agents()
            .into_iter()
            .filter(|peer| peer.id != info.id)
            .collect::<Vec<_>>();
        let shared_dir = ristretto_shared_dir();
        fs::create_dir_all(&shared_dir)?;
        let contents = generate_context_file(info, &peers, None, &shared_dir);
        fs::write(info.workdir.join(CONTEXT_FILE_NAME), contents)
    }

    fn cleanup_worktree(&self, info: &AgentInfo, delete_branch: bool) -> io::Result<()> {
        let repo_path = repo_path_from_info(info)?;
        GitManager::remove_worktree(&repo_path, &info.workdir, delete_branch)
    }

    fn agent_info(&self, id: SessionId) -> io::Result<&AgentInfo> {
        if let Some(session) = self.sessions.get(&id) {
            return Ok(&session.info);
        }
        self.archived
            .get(&id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))
    }
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}

struct RepositoryRoot {
    path: PathBuf,
}

impl RepositoryRoot {
    fn discover(path: &Path) -> io::Result<Self> {
        let repo = git2::Repository::discover(path).map_err(io::Error::other)?;
        let workdir = repo.workdir().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("bare repositories are not supported: {}", path.display()),
            )
        })?;
        Ok(Self {
            path: workdir.to_path_buf(),
        })
    }
}

fn current_branch_name(workdir: &Path) -> io::Result<Option<String>> {
    let repo = git2::Repository::open(workdir).map_err(io::Error::other)?;
    let head = repo.head().map_err(io::Error::other)?;
    if head.is_branch() {
        Ok(head.shorthand().map(ToOwned::to_owned))
    } else {
        Ok(None)
    }
}

fn repo_path_from_info(info: &AgentInfo) -> io::Result<PathBuf> {
    info.metadata
        .get("repo_path")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "agent has no repository"))
}

fn default_context_tokens(agent_type: &AgentType) -> u64 {
    match agent_type {
        AgentType::Claude => 200_000,
        AgentType::Codex => 192_000,
        AgentType::Gemini => 1_000_000,
        AgentType::Custom(_) | AgentType::Unknown => 128_000,
    }
}

fn git_diff_name_only(repo_path: &Path, branch: &str) -> io::Result<Vec<PathBuf>> {
    let repo = git2::Repository::open(repo_path).map_err(io::Error::other)?;
    let output = Command::new("git")
        .args(["diff", "--name-only", "HEAD", branch])
        .current_dir(repo.workdir().unwrap_or(repo_path))
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git diff --name-only failed with status {}",
            output.status
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(PathBuf::from)
        .collect())
}

fn issue_key(info: &AgentInfo, loop_pattern: Option<&str>) -> Option<String> {
    if let Some(pattern) = loop_pattern {
        Some(format!("loop:{pattern}"))
    } else if info.status == AgentStatus::Error {
        Some(format!("error:{}", info.exit_code.unwrap_or_default()))
    } else {
        info.last_output_at.and_then(|last_output| {
            (Utc::now() - last_output)
                .to_std()
                .ok()
                .filter(|elapsed| *elapsed >= Duration::from_secs(300))
                .map(|_| "idle".to_owned())
        })
    }
}

fn push_event(pending_events: &Arc<Mutex<VecDeque<Event>>>, event: Event) {
    if let Ok(mut events) = pending_events.lock() {
        events.push_back(event);
    }
}

fn split_command_line(command: &str) -> io::Result<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let next = chars.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "unterminated escape sequence")
                })?;
                current.push(next);
            }
            '\'' | '"' => {
                if quote == Some(ch) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(ch);
                } else {
                    current.push(ch);
                }
            }
            ch if ch.is_whitespace() && quote.is_none() => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if quote.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unterminated quoted string",
        ));
    }
    if !current.is_empty() {
        args.push(current);
    }

    Ok(args)
}

fn force_terminate_session(session: &mut PtySession) -> io::Result<portable_pty::ExitStatus> {
    if let Some(process_id) = session.child.process_id() {
        let pid = Pid::from_raw(process_id as i32);
        let _ = kill(pid, Signal::SIGKILL);
    } else {
        session.child.kill().map_err(io::Error::other)?;
    }
    session.child.wait().map_err(io::Error::other)
}

fn ristretto_shared_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ristretto")
        .join("shared")
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
                Some(temp.path().to_path_buf()),
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
