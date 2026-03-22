//! Application state for the Ristretto terminal client.

use std::collections::HashMap;

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
                    if self.agents.is_empty() {
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
                self.agent_outputs.insert(agent.id, lines);
            }
        }
        if let Some(agent) = self.split_agent_info() {
            if let Ok(lines) = client.get_output(agent.id, 200).await {
                self.agent_outputs.insert(agent.id, lines);
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
                return vec![
                    self.status_message.clone(),
                    "Task graph unavailable in Phase 1A daemon.".to_owned(),
                ];
            }
            let mut lines = vec![self.status_message.clone()];
            lines.extend(self.task_events.iter().rev().take(7).cloned());
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
        let text = String::from_utf8_lossy(chunk);
        let cache = self.agent_outputs.entry(id).or_default();
        for line in text.lines() {
            cache.push(line.to_owned());
        }
        if cache.len() > 400 {
            let overflow = cache.len() - 400;
            cache.drain(0..overflow);
        }
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
    use super::{App, LayoutMode};

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
}
