//! ratatui rendering for the Ristretto terminal client.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use rist_shared::i18n::tr;
use rist_shared::{AgentInfo, AgentStatus};

use crate::app::{App, InputMode, LayoutMode};

/// Renders the complete TUI frame.
pub fn render(frame: &mut Frame<'_>, app: &App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(1)])
        .split(areas[0]);
    let content = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(7)])
        .split(main[1]);

    render_sidebar(frame, main[0], app);
    render_main_panels(frame, content[0], app);
    render_task_panel(frame, content[1], app);
    render_status_bar(frame, areas[1], app);

    if app.show_help {
        render_help_overlay(frame, app);
    }
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

fn render_status_bar(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let mode = match app.input_mode {
        InputMode::Normal => "NORMAL",
        InputMode::Typing => "TYPING",
    };
    let input_suffix = if app.input_mode == InputMode::Typing {
        format!(" | stdin: {}", app.input_buffer)
    } else {
        String::new()
    };
    let left = format!(
        "[N]ew [K]ill [1-9]Focus [D]split [P]lan [?]Help  {}{}",
        mode, input_suffix
    );
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
    let height = 11;
    let area = centered_rect(width, height, frame.area());
    let lines = vec![
        Line::from("N  spawn a Codex agent"),
        Line::from("K  kill focused agent"),
        Line::from("1-9 / Tab  change focus"),
        Line::from("D  toggle split layout"),
        Line::from("P  toggle task graph panel"),
        Line::from("?  toggle this help"),
        Line::from("Enter  start typing to agent"),
        Line::from("Esc  leave typing mode"),
        Line::from("Ctrl-C  quit or send SIGINT in typing mode"),
        Line::from("Q  quit TUI"),
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
