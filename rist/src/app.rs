//! Application state for the Ristretto terminal client.

use std::collections::{HashMap, HashSet};

use rist_shared::protocol::Event;
use rist_shared::{AgentInfo, AgentStatus, SessionId};

use rist::daemon_client::{ClientEvent, DaemonClient};

/// Top-level application state for the TUI.
pub struct App {
    /// Current agent snapshot fetched from the daemon.
    pub agents: Vec<AgentInfo>,
    /// Index of the focused agent in [`Self::agents`].
    pub focused_agent: Option<usize>,
    /// Current panel arrangement.
    pub layout_mode: LayoutMode,
    /// Whether the task graph/status panel is expanded.
    pub show_task_graph: bool,
    /// Whether the help overlay is visible.
    pub show_help: bool,
    /// Current keyboard input mode.
    pub input_mode: InputMode,
    /// Buffer for typed stdin input.
    pub input_buffer: String,
    /// Cached output lines keyed by session id.
    pub agent_outputs: HashMap<SessionId, Vec<String>>,
    /// Whether the main loop should keep running.
    pub running: bool,
    locale: String,
    status_message: String,
    task_events: Vec<String>,
    output_states: HashMap<SessionId, OutputState>,
}

/// Available main-pane layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Sidebar plus one focused terminal.
    SidebarSingle,
    /// Sidebar plus two terminals side by side.
    SidebarSplit,
    /// Reserved grid layout for future expansion.
    Grid,
}

/// Keyboard interaction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Normal navigation and command mode.
    Normal,
    /// Typing directly to the focused agent PTY.
    Typing,
}

impl App {
    /// Creates a fresh TUI state.
    #[must_use]
    pub fn new(locale: String) -> Self {
        Self {
            agents: Vec::new(),
            focused_agent: None,
            layout_mode: LayoutMode::SidebarSingle,
            show_task_graph: false,
            show_help: false,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            agent_outputs: HashMap::new(),
            running: true,
            locale,
            status_message: String::new(),
            task_events: Vec::new(),
            output_states: HashMap::new(),
        }
    }

    /// Returns the active locale code.
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Returns the focused agent, if any.
    #[must_use]
    pub fn focused_agent_info(&self) -> Option<&AgentInfo> {
        self.focused_agent.and_then(|index| self.agents.get(index))
    }

    /// Returns the focused agent output cache, if any.
    #[must_use]
    pub fn focused_output(&self) -> Option<&[String]> {
        self.focused_agent_info()
            .and_then(|agent| self.agent_outputs.get(&agent.id))
            .map(Vec::as_slice)
    }

    /// Returns the secondary split-pane agent, if applicable.
    #[must_use]
    pub fn split_agent_info(&self) -> Option<&AgentInfo> {
        match self.layout_mode {
            LayoutMode::SidebarSplit | LayoutMode::Grid => self
                .focused_agent
                .and_then(|index| {
                    if self.agents.len() < 2 {
                        None
                    } else {
                        Some((index + 1) % self.agents.len())
                    }
                })
                .and_then(|index| self.agents.get(index)),
            LayoutMode::SidebarSingle => None,
        }
    }

    /// Returns the secondary split-pane output cache, if applicable.
    #[must_use]
    pub fn split_output(&self) -> Option<&[String]> {
        self.split_agent_info()
            .and_then(|agent| self.agent_outputs.get(&agent.id))
            .map(Vec::as_slice)
    }

    /// Replaces the agent list and keeps focus valid.
    pub fn refresh_agents(&mut self, mut agents: Vec<AgentInfo>) {
        agents.sort_by_key(|agent| agent.created_at);
        self.agents = agents;
        self.prune_agent_state();

        self.focused_agent = match self.focused_agent {
            Some(index) if index < self.agents.len() => Some(index),
            _ if self.agents.is_empty() => None,
            _ => Some(0),
        };
    }

    /// Refreshes the cached output for the visible agent panes.
    pub async fn refresh_visible_outputs(&mut self, client: &DaemonClient) {
        if let Some(agent) = self.focused_agent_info() {
            if let Ok(lines) = client.get_output(agent.id, 200).await {
                self.replace_output(agent.id, lines);
            }
        }
        if let Some(agent) = self.split_agent_info() {
            if let Ok(lines) = client.get_output(agent.id, 200).await {
                self.replace_output(agent.id, lines);
            }
        }
    }

    /// Updates local state from a daemon-side client event.
    pub fn apply_client_event(&mut self, event: ClientEvent) {
        match event {
            ClientEvent::Connected => self.status_message = "daemon connected".to_owned(),
            ClientEvent::Disconnected(message) => {
                self.status_message = format!("daemon disconnected: {message}");
            }
            ClientEvent::Daemon(event) => self.apply_daemon_event(event),
        }
    }

    /// Records a transient status line.
    pub fn set_status_message(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
    }

    /// Returns lines for the status or task panel.
    #[must_use]
    pub fn task_panel_lines(&self) -> Vec<String> {
        if self.show_task_graph {
            if self.task_events.is_empty() {
                return vec![self.status_message.clone()];
            }
            let mut lines = vec![self.status_message.clone()];
            let start = self.task_events.len().saturating_sub(7);
            lines.extend(self.task_events[start..].iter().cloned());
            return lines;
        }

        match self.focused_agent_info() {
            Some(agent) => vec![
                format!("info: {}", self.status_message),
                format!("task: {}", agent.task),
                format!("workdir: {}", agent.workdir.display()),
                format!("status: {}", status_label(&agent.status)),
                format!(
                    "exit: {}",
                    agent
                        .exit_code
                        .map_or_else(|| "running".to_owned(), |code| code.to_string())
                ),
            ],
            None => vec![
                format!("info: {}", self.status_message),
                "No agents connected.".to_owned(),
            ],
        }
    }

    /// Moves focus to a specific agent index.
    pub fn focus_index(&mut self, index: usize) {
        if index < self.agents.len() {
            self.focused_agent = Some(index);
        }
    }

    /// Advances focus to the next known agent.
    pub fn cycle_focus(&mut self) {
        if self.agents.is_empty() {
            self.focused_agent = None;
            return;
        }
        self.focused_agent = Some(match self.focused_agent {
            Some(index) => (index + 1) % self.agents.len(),
            None => 0,
        });
    }

    /// Toggles the split layout.
    pub fn toggle_split(&mut self) {
        self.layout_mode = match self.layout_mode {
            LayoutMode::SidebarSingle => LayoutMode::SidebarSplit,
            LayoutMode::SidebarSplit => LayoutMode::Grid,
            LayoutMode::Grid => LayoutMode::SidebarSingle,
        };
    }

    /// Appends PTY output lines to the local cache.
    pub fn append_output(&mut self, id: SessionId, chunk: &[u8]) {
        let cache = self.agent_outputs.entry(id).or_default();
        let state = self.output_states.entry(id).or_default();
        state.utf8_tail.extend_from_slice(chunk);

        let decoded = match std::str::from_utf8(&state.utf8_tail) {
            Ok(_) => {
                let text = String::from_utf8_lossy(&state.utf8_tail).into_owned();
                state.utf8_tail.clear();
                text
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                let text = String::from_utf8_lossy(&state.utf8_tail[..valid_up_to]).into_owned();
                let tail = state.utf8_tail.split_off(valid_up_to);
                state.utf8_tail = tail;
                text
            }
        };

        for ch in decoded.chars() {
            state.push_char(cache, ch);
        }
        trim_lines(cache, 400);
    }

    fn apply_daemon_event(&mut self, event: Event) {
        match event {
            Event::PtyData { id, data } => self.append_output(id, &data),
            Event::StatusChange { id, new, .. } => {
                if let Some(agent) = self.agents.iter_mut().find(|agent| agent.id == id) {
                    agent.status = new;
                }
            }
            Event::AgentExited { id, exit_code } => {
                if let Some(agent) = self.agents.iter_mut().find(|agent| agent.id == id) {
                    agent.exit_code = Some(exit_code);
                    agent.status = if exit_code == 0 {
                        AgentStatus::Done
                    } else {
                        AgentStatus::Error
                    };
                }
            }
            Event::TaskUpdate { task_id, status } => {
                self.task_events.push(format!("task {task_id}: {status:?}"));
            }
            Event::ContextWarning { id, usage_pct } => {
                self.task_events
                    .push(format!("context warning {id:?}: {usage_pct:.1}%"));
            }
            Event::LoopDetected { id, pattern } => {
                self.task_events
                    .push(format!("loop detected {id:?}: {pattern}"));
            }
            Event::Unknown => {}
        }
        if self.task_events.len() > 64 {
            let overflow = self.task_events.len() - 64;
            self.task_events.drain(0..overflow);
        }
    }

    fn prune_agent_state(&mut self) {
        let active_ids = self
            .agents
            .iter()
            .map(|agent| agent.id)
            .collect::<HashSet<_>>();
        self.agent_outputs
            .retain(|session_id, _| active_ids.contains(session_id));
        self.output_states
            .retain(|session_id, _| active_ids.contains(session_id));
    }

    fn replace_output(&mut self, id: SessionId, lines: Vec<String>) {
        self.agent_outputs.insert(id, lines);
        self.output_states.remove(&id);
    }
}

#[derive(Default)]
struct OutputState {
    utf8_tail: Vec<u8>,
    ansi_state: AnsiState,
    line_open: bool,
}

#[derive(Clone, Copy, Default)]
enum AnsiState {
    #[default]
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

impl OutputState {
    fn push_char(&mut self, cache: &mut Vec<String>, ch: char) {
        match self.ansi_state {
            AnsiState::Ground => match ch {
                '\u{1b}' => self.ansi_state = AnsiState::Escape,
                '\n' => self.finish_line(cache),
                '\r' => self.reset_line(cache),
                '\u{8}' => {
                    if self.line_open {
                        if let Some(line) = cache.last_mut() {
                            line.pop();
                        }
                    }
                }
                ch if ch.is_control() => {}
                _ => {
                    self.open_line(cache);
                    if let Some(line) = cache.last_mut() {
                        line.push(ch);
                    }
                }
            },
            AnsiState::Escape => {
                self.ansi_state = match ch {
                    '[' => AnsiState::Csi,
                    ']' => AnsiState::Osc,
                    _ => AnsiState::Ground,
                };
            }
            AnsiState::Csi => {
                if ('@'..='~').contains(&ch) {
                    self.ansi_state = AnsiState::Ground;
                }
            }
            AnsiState::Osc => match ch {
                '\u{7}' => self.ansi_state = AnsiState::Ground,
                '\u{1b}' => self.ansi_state = AnsiState::OscEscape,
                _ => {}
            },
            AnsiState::OscEscape => {
                self.ansi_state = if ch == '\\' {
                    AnsiState::Ground
                } else {
                    AnsiState::Osc
                };
            }
        }
    }

    fn open_line(&mut self, cache: &mut Vec<String>) {
        if !self.line_open {
            cache.push(String::new());
            self.line_open = true;
        }
    }

    fn finish_line(&mut self, cache: &mut Vec<String>) {
        if !self.line_open {
            cache.push(String::new());
        }
        self.line_open = false;
    }

    fn reset_line(&mut self, cache: &mut [String]) {
        if self.line_open {
            if let Some(line) = cache.last_mut() {
                line.clear();
            }
        }
    }
}

fn trim_lines(cache: &mut Vec<String>, max_lines: usize) {
    if cache.len() > max_lines {
        let overflow = cache.len() - max_lines;
        cache.drain(0..overflow);
    }
}

fn status_label(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => "idle",
        AgentStatus::Working => "working",
        AgentStatus::Thinking => "thinking",
        AgentStatus::Waiting => "waiting",
        AgentStatus::Stuck => "stuck",
        AgentStatus::Done => "done",
        AgentStatus::Error => "error",
        AgentStatus::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use rist_shared::protocol::Event;
    use rist_shared::{AgentInfo, AgentStatus, AgentType, SessionId, TaskStatus};
    use serde_json::json;

    use super::{App, LayoutMode};

    fn sample_agent(task: &str) -> AgentInfo {
        serde_json::from_value(json!({
            "id": SessionId::new(),
            "agent_type": AgentType::Codex,
            "task": task,
            "status": AgentStatus::Idle,
            "workdir": PathBuf::from("/tmp"),
            "branch": null,
            "file_ownership": [],
            "created_at": "2024-01-01T00:00:00Z",
            "last_output_at": null,
            "context_usage": null,
            "exit_code": null,
            "metadata": HashMap::<String, String>::new(),
        }))
        .expect("sample agent should deserialize")
    }

    #[test]
    fn split_toggle_roundtrips() {
        let mut app = App::new("en".to_owned());
        assert_eq!(app.layout_mode, LayoutMode::SidebarSingle);
        app.toggle_split();
        assert_eq!(app.layout_mode, LayoutMode::SidebarSplit);
        app.toggle_split();
        assert_eq!(app.layout_mode, LayoutMode::Grid);
        app.toggle_split();
        assert_eq!(app.layout_mode, LayoutMode::SidebarSingle);
    }

    #[test]
    fn cycle_focus_handles_empty_agents() {
        let mut app = App::new("en".to_owned());
        app.cycle_focus();
        assert_eq!(app.focused_agent, None);
    }

    #[test]
    fn split_layout_hides_secondary_when_only_one_agent_exists() {
        let mut app = App::new("en".to_owned());
        app.layout_mode = LayoutMode::SidebarSplit;
        app.refresh_agents(vec![sample_agent("solo")]);
        assert!(app.split_agent_info().is_none());
    }

    #[test]
    fn refresh_agents_prunes_removed_output_state() {
        let mut app = App::new("en".to_owned());
        let first = sample_agent("first");
        let second = sample_agent("second");

        app.refresh_agents(vec![first.clone(), second.clone()]);
        app.append_output(first.id, b"hello");
        app.append_output(second.id, b"world");

        app.refresh_agents(vec![first]);

        assert!(app.agent_outputs.contains_key(&app.agents[0].id));
        assert_eq!(app.agent_outputs.len(), 1);
    }

    #[test]
    fn task_panel_preserves_chronological_order() {
        let mut app = App::new("en".to_owned());
        app.show_task_graph = true;
        app.set_status_message("status");
        app.apply_daemon_event(Event::TaskUpdate {
            task_id: "1".to_owned(),
            status: TaskStatus::Pending,
        });
        app.apply_daemon_event(Event::TaskUpdate {
            task_id: "2".to_owned(),
            status: TaskStatus::Working,
        });

        let lines = app.task_panel_lines();
        assert_eq!(lines[1], "task 1: Pending");
        assert_eq!(lines[2], "task 2: Working");
    }

    #[test]
    fn append_output_handles_chunked_utf8_and_ansi_sequences() {
        let mut app = App::new("en".to_owned());
        let id = SessionId::new();

        app.append_output(id, b"\x1b[31mhel");
        app.append_output(id, &[0x6c, 0x6f, 0x20, 0xF0, 0x9F]);
        app.append_output(id, &[0x98, 0x80, b'\n']);

        assert_eq!(
            app.agent_outputs.get(&id),
            Some(&vec!["hello 😀".to_owned()])
        );
    }
}
