//! Application state for the Ristretto terminal client.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use rist_shared::protocol::Event;
use rist_shared::{AgentInfo, AgentStatus, ContextBudget, SessionId, TaskGraph};

use rist::daemon_client::{ClientEvent, DaemonClient};

/// Top-level application state for the TUI.
pub struct App {
    /// Current agent snapshot fetched from the daemon.
    pub agents: Vec<AgentInfo>,
    /// Index of the focused agent in [`Self::agents`].
    pub focused_agent: Option<usize>,
    /// Active top-level presentation mode.
    pub view_mode: ViewMode,
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
    /// Current planner task graph snapshot.
    pub task_graph: TaskGraph,
    /// Current daemon file ownership map.
    pub file_ownership: HashMap<PathBuf, SessionId>,
    /// Selected task id in graph mode.
    pub selected_task: Option<String>,
    /// Expanded task id whose terminal is visible in graph mode.
    pub expanded_node: Option<String>,
    /// Current graph scroll offset as `(horizontal, vertical)`.
    pub dag_scroll: (i16, i16),
    /// Whether the file ownership overlay is visible.
    pub show_file_overlay: bool,
    /// Whether the main loop should keep running.
    pub running: bool,
    locale: String,
    status_message: String,
    task_events: Vec<String>,
    output_states: HashMap<SessionId, OutputState>,
    context_budgets: HashMap<SessionId, ContextBudget>,
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

/// Available top-level UI modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// DAG-first task graph view.
    Graph,
    /// Classic v0.1 sidebar and panel layout.
    List,
    /// Full-screen terminal for the focused or selected agent.
    Terminal,
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
            view_mode: ViewMode::Graph,
            layout_mode: LayoutMode::SidebarSingle,
            show_task_graph: false,
            show_help: false,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            agent_outputs: HashMap::new(),
            task_graph: TaskGraph {
                tasks: Vec::new(),
                updated_at: chrono::Utc::now(),
            },
            file_ownership: HashMap::new(),
            selected_task: None,
            expanded_node: None,
            dag_scroll: (0, 0),
            show_file_overlay: false,
            running: true,
            locale,
            status_message: String::new(),
            task_events: Vec::new(),
            output_states: HashMap::new(),
            context_budgets: HashMap::new(),
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

    /// Returns the graph-selected or expanded agent, if any task owns one.
    #[must_use]
    pub fn graph_agent_info(&self) -> Option<&AgentInfo> {
        self.expanded_node
            .as_deref()
            .or(self.selected_task.as_deref())
            .and_then(|task_id| {
                self.task_graph
                    .tasks
                    .iter()
                    .find(|task| task.id == task_id)
                    .and_then(|task| task.owner)
            })
            .and_then(|owner| self.agents.iter().find(|agent| agent.id == owner))
            .or_else(|| self.focused_agent_info())
    }

    /// Returns output for the graph-selected or expanded task owner.
    #[must_use]
    pub fn graph_output(&self) -> Option<&[String]> {
        self.graph_agent_info()
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

    /// Replaces the task graph snapshot and keeps graph selection valid.
    pub fn refresh_task_graph(&mut self, task_graph: TaskGraph) {
        self.task_graph = task_graph;

        let contains_task =
            |task_id: &str| self.task_graph.tasks.iter().any(|task| task.id == task_id);
        if self.selected_task.as_deref().is_some_and(contains_task) {
            return;
        }

        self.selected_task = self.task_graph.tasks.first().map(|task| task.id.clone());
        if self
            .expanded_node
            .as_deref()
            .is_some_and(|task_id| !contains_task(task_id))
        {
            self.expanded_node = None;
        }
    }

    /// Replaces the file ownership snapshot.
    pub fn refresh_file_ownership(&mut self, file_ownership: HashMap<PathBuf, SessionId>) {
        self.file_ownership = file_ownership;
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
        if let Some(agent) = self.graph_agent_info() {
            if let Ok(lines) = client.get_output(agent.id, 200).await {
                self.replace_output(agent.id, lines);
            }
        }
    }

    /// Refreshes context budgets for the visible agent panes.
    pub async fn refresh_visible_context_budgets(&mut self, client: &DaemonClient) {
        if let Some(agent) = self.focused_agent_info() {
            if let Ok(budget) = client.get_context_budget(agent.id).await {
                self.context_budgets.insert(agent.id, budget);
            }
        }
        if let Some(agent) = self.split_agent_info() {
            if let Ok(budget) = client.get_context_budget(agent.id).await {
                self.context_budgets.insert(agent.id, budget);
            }
        }
        if let Some(agent) = self.graph_agent_info() {
            if let Ok(budget) = client.get_context_budget(agent.id).await {
                self.context_budgets.insert(agent.id, budget);
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
                self.format_context_line(agent.id),
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

    /// Returns task ids grouped by depth with their top-left positions.
    #[must_use]
    pub fn compute_dag_layout(&self, task_graph: &TaskGraph) -> Vec<Vec<(String, (u16, u16))>> {
        let mut task_map = HashMap::new();
        let mut indegree = HashMap::new();
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();

        for task in &task_graph.tasks {
            task_map.insert(task.id.as_str(), task);
            indegree.insert(task.id.as_str(), 0usize);
            adjacency.entry(task.id.as_str()).or_default();
        }

        for task in &task_graph.tasks {
            for dep in &task.depends_on {
                if task_map.contains_key(dep.as_str()) {
                    *indegree.entry(task.id.as_str()).or_default() += 1;
                    adjacency
                        .entry(dep.as_str())
                        .or_default()
                        .push(task.id.as_str());
                }
            }
        }

        let mut frontier = indegree
            .iter()
            .filter_map(|(task_id, degree)| (*degree == 0).then_some(*task_id))
            .collect::<Vec<_>>();
        frontier.sort_unstable();

        let mut levels = Vec::new();
        let mut seen = HashSet::new();
        while !frontier.is_empty() {
            let current = std::mem::take(&mut frontier);
            let mut current_level = current;
            current_level.sort_unstable();

            let mut next = Vec::new();
            for task_id in current_level.iter().copied() {
                seen.insert(task_id);
                if let Some(children) = adjacency.get(task_id) {
                    for child in children {
                        if let Some(value) = indegree.get_mut(child) {
                            *value = value.saturating_sub(1);
                            if *value == 0 {
                                next.push(*child);
                            }
                        }
                    }
                }
            }

            let depth = levels.len() as u16;
            let column = current_level
                .iter()
                .enumerate()
                .map(|(row, task_id)| {
                    (
                        (*task_id).to_owned(),
                        (depth.saturating_mul(18), row as u16 * 6),
                    )
                })
                .collect::<Vec<_>>();
            levels.push(column);

            next.sort_unstable();
            next.dedup();
            frontier = next;
        }

        let mut unresolved = task_graph
            .tasks
            .iter()
            .filter(|task| !seen.contains(task.id.as_str()))
            .map(|task| task.id.clone())
            .collect::<Vec<_>>();
        unresolved.sort();
        if !unresolved.is_empty() {
            let depth = levels.len() as u16;
            levels.push(
                unresolved
                    .into_iter()
                    .enumerate()
                    .map(|(row, task_id)| (task_id, (depth.saturating_mul(18), row as u16 * 6)))
                    .collect(),
            );
        }

        levels
    }

    /// Selects the next task in topological order.
    pub fn cycle_task_selection(&mut self, forward: bool) {
        let ordered = self
            .compute_dag_layout(&self.task_graph)
            .into_iter()
            .flatten()
            .map(|(task_id, _)| task_id)
            .collect::<Vec<_>>();
        if ordered.is_empty() {
            self.selected_task = None;
            return;
        }

        let index = self
            .selected_task
            .as_ref()
            .and_then(|task_id| ordered.iter().position(|candidate| candidate == task_id))
            .unwrap_or(0);
        let next_index = if forward {
            (index + 1) % ordered.len()
        } else if index == 0 {
            ordered.len() - 1
        } else {
            index - 1
        };
        self.selected_task = Some(ordered[next_index].clone());
    }

    /// Moves graph selection by one column or row.
    pub fn move_task_selection(&mut self, delta_x: i16, delta_y: i16) {
        let layout = self.compute_dag_layout(&self.task_graph);
        let selected = self.selected_task.clone().or_else(|| {
            layout
                .first()
                .and_then(|column| column.first().map(|(task_id, _)| task_id.clone()))
        });
        let Some(selected) = selected else {
            self.selected_task = None;
            return;
        };

        let mut location = None;
        for (column_index, column) in layout.iter().enumerate() {
            if let Some(row_index) = column.iter().position(|(task_id, _)| *task_id == selected) {
                location = Some((column_index, row_index));
                break;
            }
        }

        let Some((column_index, row_index)) = location else {
            self.selected_task = layout
                .first()
                .and_then(|column| column.first().map(|(task_id, _)| task_id.clone()));
            return;
        };

        let target_column = (column_index as i16 + delta_x)
            .clamp(0, layout.len().saturating_sub(1) as i16) as usize;
        let target_rows = &layout[target_column];
        let target_row = (row_index as i16 + delta_y)
            .clamp(0, target_rows.len().saturating_sub(1) as i16) as usize;
        self.selected_task = Some(target_rows[target_row].0.clone());
    }

    /// Expands or collapses the selected node terminal.
    pub fn toggle_expanded_node(&mut self) {
        self.expanded_node = match (&self.expanded_node, &self.selected_task) {
            (Some(expanded), Some(selected)) if expanded == selected => None,
            (_, Some(selected)) => Some(selected.clone()),
            _ => None,
        };
    }

    /// Collapses the graph terminal pane.
    pub fn collapse_expanded_node(&mut self) {
        self.expanded_node = None;
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
        self.context_budgets
            .retain(|session_id, _| active_ids.contains(session_id));
    }

    fn replace_output(&mut self, id: SessionId, lines: Vec<String>) {
        self.agent_outputs.insert(id, lines);
        self.output_states.remove(&id);
    }

    fn format_context_line(&self, id: SessionId) -> String {
        self.context_budgets.get(&id).map_or_else(
            || "Context: n/a".to_owned(),
            |budget| {
                format!(
                    "Context: {:.0}% | Injected: {:.1}% | MCP: {:.1}% | Tool output: {:.1}%",
                    budget.total_percentage(),
                    budget.injected_percentage(),
                    budget.mcp_percentage(),
                    budget.tool_output_percentage(),
                )
            },
        )
    }

    /// Returns a short context summary suitable for compact graph nodes.
    #[must_use]
    pub fn compact_context_line(&self, id: SessionId) -> String {
        self.context_budgets.get(&id).map_or_else(
            || "ctx: n/a".to_owned(),
            |budget| format!("ctx: {:.0}%", budget.total_percentage()),
        )
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

    use super::{App, LayoutMode, ViewMode};

    fn sample_agent(task: &str) -> AgentInfo {
        serde_json::from_value(json!({
            "id": SessionId::new(),
            "agent_type": AgentType::Codex,
            "model": null,
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
    fn app_starts_in_graph_view() {
        let app = App::new("en".to_owned());
        assert_eq!(app.view_mode, ViewMode::Graph);
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

    #[test]
    fn compute_dag_layout_groups_tasks_left_to_right() {
        let mut app = App::new("en".to_owned());
        app.refresh_task_graph(rist_shared::TaskGraph {
            tasks: vec![
                serde_json::from_value(json!({
                    "id": "t1",
                    "title": "Start",
                    "description": null,
                    "status": "pending",
                    "priority": "medium",
                    "agent_type": null,
                    "owner": null,
                    "depends_on": [],
                    "file_ownership": []
                }))
                .expect("task"),
                serde_json::from_value(json!({
                    "id": "t2",
                    "title": "Finish",
                    "description": null,
                    "status": "pending",
                    "priority": "medium",
                    "agent_type": null,
                    "owner": null,
                    "depends_on": ["t1"],
                    "file_ownership": []
                }))
                .expect("task"),
            ],
            updated_at: chrono::Utc::now(),
        });

        let layout = app.compute_dag_layout(&app.task_graph);
        assert_eq!(layout.len(), 2);
        assert_eq!(layout[0][0].0, "t1");
        assert_eq!(layout[1][0].0, "t2");
        assert!(layout[1][0].1 .0 > layout[0][0].1 .0);
    }
}
