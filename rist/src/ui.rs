//! ratatui rendering for the Ristretto terminal client.

use std::collections::HashMap;

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Widget, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use rist_shared::i18n::tr;
use rist_shared::{AgentInfo, AgentStatus, SessionId, Task, TaskGraph, TaskStatus};

use crate::app::{App, InputMode, LayoutMode, ViewMode};

const NODE_WIDTH: u16 = 14;
const NODE_HEIGHT: u16 = 4;

/// Renders the complete TUI frame.
pub fn render(frame: &mut Frame<'_>, app: &App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    match app.view_mode {
        ViewMode::Graph => render_graph_view(frame, areas[0], app),
        ViewMode::List => render_list_view(frame, areas[0], app),
        ViewMode::Terminal => render_terminal_view(frame, areas[0], app),
    }
    render_mode_bar(frame, areas[1], app);

    if app.show_help {
        render_help_overlay(frame, app);
    }
}

fn render_list_view(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(1)])
        .split(area);
    let content = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(7)])
        .split(main[1]);

    render_sidebar(frame, main[0], app);
    render_main_panels(frame, content[0], app);
    render_task_panel(frame, content[1], app);
}

fn render_graph_view(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let graph_height = if app.expanded_node.is_some() {
        Constraint::Percentage(65)
    } else {
        Constraint::Min(10)
    };

    let mut constraints = vec![graph_height];
    if app.expanded_node.is_some() {
        constraints.push(Constraint::Percentage(35));
    }
    if app.show_file_overlay {
        constraints.push(Constraint::Length(8));
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    frame.render_widget(GraphWidget::new(app), sections[0]);

    let mut section_index = 1;
    if app.expanded_node.is_some() {
        render_graph_terminal_panel(frame, sections[section_index], app);
        section_index += 1;
    }
    if app.show_file_overlay {
        render_file_overlay(frame, sections[section_index], app);
    }
}

fn render_terminal_view(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let title = app.graph_agent_info().map_or_else(
        || "Agent Terminal".to_owned(),
        |agent| {
            let (symbol, _) = status_style(&agent.status);
            let model = agent
                .model
                .as_deref()
                .map_or(String::new(), |model| format!(" ({model})"));
            format!(
                "{symbol} {}",
                truncate_text(&format!("{}{}", short_task(agent), model), 40)
            )
        },
    );
    render_output_panel(frame, area, &title, app.graph_output());
}

fn render_graph_terminal_panel(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let title = app.expanded_node.as_deref().map_or_else(
        || "Task Terminal".to_owned(),
        |task_id| format!("Task Terminal: {task_id}"),
    );
    render_output_panel(frame, area, &title, app.graph_output());
}

fn render_file_overlay(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let lines = file_overlay_lines(app)
        .into_iter()
        .map(Line::from)
        .collect::<Vec<_>>();
    let panel = Paragraph::new(lines)
        .block(
            Block::default()
                .title("File Ownership")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(panel, area);
}

fn file_overlay_lines(app: &App) -> Vec<String> {
    if app.task_graph.tasks.is_empty() && app.file_ownership.is_empty() {
        return vec!["No file ownership data available.".to_owned()];
    }

    let task_by_owner = app
        .task_graph
        .tasks
        .iter()
        .filter_map(|task| task.owner.map(|owner| (owner, task)))
        .collect::<HashMap<SessionId, &Task>>();

    let mut lines = Vec::new();
    let mut owned_paths = app
        .file_ownership
        .iter()
        .map(|(path, owner)| (path.display().to_string(), *owner))
        .collect::<Vec<_>>();
    owned_paths.sort_by(|left, right| left.0.cmp(&right.0));

    for (path, owner) in owned_paths {
        if let Some(task) = task_by_owner.get(&owner) {
            let (symbol, _) = task_status_style(&task.status);
            lines.push(format!("{path} -> {} {symbol}", task.id));
        } else {
            lines.push(format!("{path} -> {owner:?}"));
        }
    }

    for task in &app.task_graph.tasks {
        for path in &task.file_ownership {
            let display = path.display().to_string();
            if !app.file_ownership.contains_key(path) {
                let (symbol, _) = task_status_style(&task.status);
                lines.push(format!("! {display} -> {} {symbol}", task.id));
            }
        }
    }

    if lines.is_empty() {
        lines.push("No file ownership data available.".to_owned());
    }
    lines.sort();
    lines
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let items = app
        .agents
        .iter()
        .enumerate()
        .map(|(index, agent)| {
            let (symbol, color) = status_style(&agent.status);
            let marker = if Some(index) == app.focused_agent {
                ">"
            } else {
                " "
            };
            let label = truncate_text(
                &format!("{} {}", index + 1, short_task(agent)),
                area.width.saturating_sub(6) as usize,
            );
            ListItem::new(Line::from(vec![
                Span::raw(marker),
                Span::raw(" "),
                Span::styled(symbol, Style::default().fg(color)),
                Span::raw(" "),
                Span::raw(label),
            ]))
        })
        .collect::<Vec<_>>();

    let sidebar = List::new(items).block(Block::default().title("Agents").borders(Borders::ALL));
    frame.render_widget(sidebar, area);
}

fn render_main_panels(frame: &mut Frame<'_>, area: Rect, app: &App) {
    match app.layout_mode {
        LayoutMode::SidebarSingle => {
            render_output_panel(frame, area, "Agent Terminal Output", app.focused_output());
        }
        LayoutMode::SidebarSplit | LayoutMode::Grid => {
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);
            render_output_panel(frame, panes[0], "Focused Agent", app.focused_output());
            render_output_panel(frame, panes[1], "Split Agent", app.split_output());
        }
    }
}

fn render_output_panel(frame: &mut Frame<'_>, area: Rect, title: &str, output: Option<&[String]>) {
    let output = output.unwrap_or(&[]);
    let height = area.height.saturating_sub(2) as usize;
    let start = output.len().saturating_sub(height);
    let lines = output[start..]
        .iter()
        .map(|line| Line::from(line.as_str()))
        .collect::<Vec<_>>();

    let widget = Paragraph::new(lines)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);
}

fn render_task_panel(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let title = if app.show_task_graph {
        "Task Graph"
    } else {
        "Status"
    };
    let lines = app
        .task_panel_lines()
        .into_iter()
        .map(Line::from)
        .collect::<Vec<_>>();
    let panel = Paragraph::new(lines)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(panel, area);
}

fn render_mode_bar(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let mode = match app.input_mode {
        InputMode::Normal => "NORMAL",
        InputMode::Typing => "TYPING",
    };
    let current = match app.view_mode {
        ViewMode::Graph => "graph",
        ViewMode::List => "list",
        ViewMode::Terminal => "term",
    };
    let input_suffix = if app.input_mode == InputMode::Typing {
        format!(" | stdin: {}", app.input_buffer)
    } else {
        String::new()
    };
    let left = format!("[G]raph [L]ist [T]erm [F]iles [?]Help  {mode} {current}{input_suffix}");
    let right = format!("agents:{}", app.agents.len());
    let width = area.width as usize;
    let left = truncate_text(&left, width.saturating_sub(right.len() + 1));
    let padding = width.saturating_sub(left.len() + right.len());
    let line = format!("{left}{}{right}", " ".repeat(padding));
    let bar = Paragraph::new(line).style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Gray)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(bar, area);
}

fn render_help_overlay(frame: &mut Frame<'_>, app: &App) {
    let width = frame.area().width.saturating_sub(10).min(60);
    let height = 14;
    let area = centered_rect(width, height, frame.area());
    let lines = vec![
        Line::from("G  switch to graph view"),
        Line::from("L  switch to list view"),
        Line::from("T  switch to terminal view"),
        Line::from("F  toggle file overlay"),
        Line::from("Tab / arrows  move focus"),
        Line::from("Enter  type to agent or expand graph task"),
        Line::from("Esc  leave typing mode or collapse graph task"),
        Line::from("D  toggle split layout in list view"),
        Line::from("P  toggle task panel in list view"),
        Line::from("N / K  spawn or kill focused agent"),
        Line::from("Ctrl-C / Q  quit TUI"),
        Line::from(format!("locale: {}  {}", app.locale(), tr("tui.help"))),
    ];
    let overlay = Paragraph::new(lines)
        .alignment(Alignment::Left)
        .block(Block::default().title("Help").borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(overlay, area);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height.saturating_sub(height) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(area.width.saturating_sub(width) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1]);
    horizontal[1]
}

fn short_task(agent: &AgentInfo) -> String {
    if agent.task.is_empty() {
        agent.id.0.to_string()
    } else {
        agent.task.clone()
    }
}

fn truncate_text(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_owned();
    }

    let mut result = String::new();
    let mut width = 0;
    for ch in text.chars() {
        let ch_width = UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]));
        if width + ch_width + 1 > max_width {
            break;
        }
        result.push(ch);
        width += ch_width;
    }
    result.push('…');
    result
}

fn status_style(status: &AgentStatus) -> (&'static str, Color) {
    match status {
        AgentStatus::Working => ("●", Color::Green),
        AgentStatus::Thinking => ("◐", Color::Blue),
        AgentStatus::Waiting => ("◉", Color::Yellow),
        AgentStatus::Stuck => ("⊘", Color::Red),
        AgentStatus::Idle => ("○", Color::DarkGray),
        AgentStatus::Done => ("✓", Color::Green),
        AgentStatus::Error => ("✗", Color::Red),
        AgentStatus::Unknown => ("○", Color::Gray),
    }
}

fn task_status_style(status: &TaskStatus) -> (&'static str, Color) {
    match status {
        TaskStatus::Done => ("✓", Color::Green),
        TaskStatus::Working | TaskStatus::Assigned => ("●", Color::Blue),
        TaskStatus::Pending => ("○", Color::DarkGray),
        TaskStatus::Blocked => ("⊘", Color::Red),
        TaskStatus::Review => ("◐", Color::Yellow),
        TaskStatus::Unknown => ("○", Color::Gray),
    }
}

struct GraphWidget<'a> {
    app: &'a App,
}

impl<'a> GraphWidget<'a> {
    fn new(app: &'a App) -> Self {
        Self { app }
    }
}

impl Widget for GraphWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default().title("Task Graph").borders(Borders::ALL);
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.width < NODE_WIDTH || inner.height < NODE_HEIGHT {
            return;
        }

        let layout = self.app.compute_dag_layout(&self.app.task_graph);
        let positions = layout
            .iter()
            .flatten()
            .cloned()
            .collect::<HashMap<String, (u16, u16)>>();

        draw_edges(
            buf,
            inner,
            &self.app.task_graph,
            &positions,
            self.app.dag_scroll,
        );

        for task in &self.app.task_graph.tasks {
            if let Some(&(x, y)) = positions.get(&task.id) {
                draw_task_node(buf, inner, x, y, task, self.app, self.app.dag_scroll);
            }
        }

        if self.app.task_graph.tasks.is_empty() {
            buf.set_string(
                inner.x,
                inner.y,
                "No tasks loaded.",
                Style::default().fg(Color::DarkGray),
            );
        }
    }
}

fn draw_edges(
    buf: &mut Buffer,
    area: Rect,
    graph: &TaskGraph,
    positions: &HashMap<String, (u16, u16)>,
    scroll: (i16, i16),
) {
    for task in &graph.tasks {
        let Some(&(to_x, to_y)) = positions.get(&task.id) else {
            continue;
        };
        let target_x = to_x as i16 - scroll.0;
        let target_y = to_y as i16 - scroll.1;
        let arrow_x = target_x + NODE_WIDTH as i16;
        let center_y = target_y + 1;

        for dependency in &task.depends_on {
            let Some(&(from_x, from_y)) = positions.get(dependency) else {
                continue;
            };
            let source_x = from_x as i16 - scroll.0;
            let source_y = from_y as i16 - scroll.1;
            let start_x = source_x + NODE_WIDTH as i16;
            let start_y = source_y + 1;
            let mid_x = ((start_x + target_x) / 2).max(start_x + 1);

            draw_horizontal(
                buf,
                area,
                start_x,
                mid_x,
                start_y,
                '─',
                Style::default().fg(Color::DarkGray),
            );

            if start_y < center_y {
                draw_symbol(
                    buf,
                    area,
                    mid_x,
                    start_y,
                    "┐",
                    Style::default().fg(Color::DarkGray),
                );
                draw_vertical(
                    buf,
                    area,
                    mid_x,
                    start_y + 1,
                    center_y - 1,
                    '│',
                    Style::default().fg(Color::DarkGray),
                );
                draw_symbol(
                    buf,
                    area,
                    mid_x,
                    center_y,
                    "└",
                    Style::default().fg(Color::DarkGray),
                );
            } else if start_y > center_y {
                draw_symbol(
                    buf,
                    area,
                    mid_x,
                    center_y,
                    "┐",
                    Style::default().fg(Color::DarkGray),
                );
                draw_vertical(
                    buf,
                    area,
                    mid_x,
                    center_y + 1,
                    start_y - 1,
                    '│',
                    Style::default().fg(Color::DarkGray),
                );
                draw_symbol(
                    buf,
                    area,
                    mid_x,
                    start_y,
                    "└",
                    Style::default().fg(Color::DarkGray),
                );
            }

            if mid_x < arrow_x {
                draw_horizontal(
                    buf,
                    area,
                    mid_x + 1,
                    arrow_x.saturating_sub(1),
                    center_y,
                    '─',
                    Style::default().fg(Color::DarkGray),
                );
            }
            draw_symbol(
                buf,
                area,
                arrow_x,
                center_y,
                "→",
                Style::default().fg(Color::DarkGray),
            );
        }
    }
}

fn draw_horizontal(
    buf: &mut Buffer,
    area: Rect,
    start_x: i16,
    end_x: i16,
    y: i16,
    symbol: char,
    style: Style,
) {
    if start_x > end_x {
        return;
    }
    for x in start_x..=end_x {
        draw_symbol(buf, area, x, y, &symbol.to_string(), style);
    }
}

fn draw_vertical(
    buf: &mut Buffer,
    area: Rect,
    x: i16,
    start_y: i16,
    end_y: i16,
    symbol: char,
    style: Style,
) {
    if start_y > end_y {
        return;
    }
    for y in start_y..=end_y {
        draw_symbol(buf, area, x, y, &symbol.to_string(), style);
    }
}

fn draw_symbol(buf: &mut Buffer, area: Rect, x: i16, y: i16, symbol: &str, style: Style) {
    if x < area.x as i16
        || y < area.y as i16
        || x >= (area.x + area.width) as i16
        || y >= (area.y + area.height) as i16
    {
        return;
    }
    buf[(x as u16, y as u16)]
        .set_symbol(symbol)
        .set_style(style);
}

fn draw_task_node(
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    task: &Task,
    app: &App,
    scroll: (i16, i16),
) {
    let node_x = area.x as i16 + x as i16 - scroll.0;
    let node_y = area.y as i16 + y as i16 - scroll.1;
    let selected = app.selected_task.as_deref() == Some(task.id.as_str());
    let expanded = app.expanded_node.as_deref() == Some(task.id.as_str());
    let (symbol, color) = task_status_style(&task.status);
    let border_style = if selected || expanded {
        Style::default()
            .fg(color)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(color)
    };

    let border = [
        ("┌", 0, 0),
        ("┐", NODE_WIDTH as i16 - 1, 0),
        ("└", 0, NODE_HEIGHT as i16 - 1),
        ("┘", NODE_WIDTH as i16 - 1, NODE_HEIGHT as i16 - 1),
    ];
    for (corner, dx, dy) in border {
        draw_symbol(buf, area, node_x + dx, node_y + dy, corner, border_style);
    }
    draw_horizontal(
        buf,
        area,
        node_x + 1,
        node_x + NODE_WIDTH as i16 - 2,
        node_y,
        '─',
        border_style,
    );
    draw_horizontal(
        buf,
        area,
        node_x + 1,
        node_x + NODE_WIDTH as i16 - 2,
        node_y + NODE_HEIGHT as i16 - 1,
        '─',
        border_style,
    );
    draw_vertical(
        buf,
        area,
        node_x,
        node_y + 1,
        node_y + NODE_HEIGHT as i16 - 2,
        '│',
        border_style,
    );
    draw_vertical(
        buf,
        area,
        node_x + NODE_WIDTH as i16 - 1,
        node_y + 1,
        node_y + NODE_HEIGHT as i16 - 2,
        '│',
        border_style,
    );

    let owner_agent = task
        .owner
        .and_then(|owner| app.agents.iter().find(|agent| agent.id == owner));
    let model_line = owner_agent
        .and_then(|agent| agent.model.clone())
        .or_else(|| owner_agent.map(|agent| format!("{:?}", agent.agent_type)))
        .or_else(|| {
            task.agent_type
                .as_ref()
                .map(|agent_type| format!("{agent_type:?}"))
        })
        .unwrap_or_else(|| "unassigned".to_owned());
    let context_line = owner_agent
        .map(|agent| app.compact_context_line(agent.id))
        .unwrap_or_else(|| "ctx: n/a".to_owned());

    let title = truncate_text(
        &format!("{symbol} {}: {}", task.id, task.title),
        (NODE_WIDTH - 2) as usize,
    );
    let model_line = truncate_text(
        &format!("{model_line} {context_line}"),
        (NODE_WIDTH - 2) as usize,
    );
    let text_style = Style::default().fg(color);

    if node_x >= area.x as i16 && node_y >= area.y as i16 {
        buf.set_string(node_x as u16 + 1, node_y as u16 + 1, title, text_style);
    }
    if node_y + 2 < (area.y + area.height) as i16 {
        buf.set_string(node_x as u16 + 1, node_y as u16 + 2, model_line, text_style);
    }
}
