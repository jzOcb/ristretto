//! Crossterm event handling and keybinding dispatch.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::{self, UnboundedReceiver};

use rist_shared::AgentType;

use crate::app::{App, InputMode, ViewMode};
use rist::daemon_client::DaemonClient;

/// Terminal events consumed by the main TUI loop.
#[derive(Debug, Clone, Copy)]
pub enum TerminalEvent {
    /// A keyboard event.
    Key(KeyEvent),
    /// A terminal resize event.
    Resize(u16, u16),
}

/// Starts a background thread that forwards crossterm input into a Tokio channel.
#[must_use]
pub fn spawn_terminal_events() -> UnboundedReceiver<TerminalEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || loop {
        match event::poll(Duration::from_millis(100)) {
            Ok(true) => match event::read() {
                Ok(CrosstermEvent::Key(key)) => {
                    if tx.send(TerminalEvent::Key(key)).is_err() {
                        break;
                    }
                }
                Ok(CrosstermEvent::Resize(cols, rows)) => {
                    if tx.send(TerminalEvent::Resize(cols, rows)).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            },
            Ok(false) => {}
            Err(_) => break,
        }
    });
    rx
}

/// Applies a terminal event and returns `false` when the TUI should exit.
pub async fn handle_terminal_event(
    app: &mut App,
    client: &DaemonClient,
    event: TerminalEvent,
) -> io::Result<bool> {
    match event {
        TerminalEvent::Resize(cols, rows) => {
            if let Some(agent) = app.focused_agent_info() {
                let _ = client
                    .resize_agent(agent.id, cols, rows.saturating_sub(6))
                    .await;
            }
            if let Some(agent) = app.split_agent_info() {
                let _ = client
                    .resize_agent(agent.id, cols / 2, rows.saturating_sub(6))
                    .await;
            }
            Ok(true)
        }
        TerminalEvent::Key(key) => match app.input_mode {
            InputMode::Normal => handle_normal_mode(app, client, key).await,
            InputMode::Typing => handle_typing_mode(app, client, key).await,
        },
    }
}

async fn handle_normal_mode(
    app: &mut App,
    client: &DaemonClient,
    key: KeyEvent,
) -> io::Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        app.running = false;
        return Ok(false);
    }

    match key.code {
        KeyCode::Char('G') => {
            app.view_mode = ViewMode::Graph;
            Ok(true)
        }
        KeyCode::Char('L') => {
            app.view_mode = ViewMode::List;
            Ok(true)
        }
        KeyCode::Char('T') => {
            app.view_mode = ViewMode::Terminal;
            Ok(true)
        }
        KeyCode::Char('F') => {
            app.show_file_overlay = !app.show_file_overlay;
            Ok(true)
        }
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.running = false;
            Ok(false)
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            let id = client
                .spawn_agent(
                    AgentType::Codex,
                    "Interactive Codex session started from rist".to_owned(),
                )
                .await?;
            app.set_status_message(format!("spawned agent {}", id.0));
            if let Ok(agents) = client.list_agents().await {
                app.refresh_agents(agents);
            }
            app.refresh_visible_outputs(client).await;
            Ok(true)
        }
        KeyCode::Char('K') => {
            if let Some(agent) = app.focused_agent_info() {
                client.kill_agent(agent.id).await?;
                app.set_status_message(format!("killed {}", agent.task));
            }
            Ok(true)
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            if app.view_mode == ViewMode::List {
                app.toggle_split();
            }
            Ok(true)
        }
        KeyCode::Char('p') | KeyCode::Char('P') => {
            if app.view_mode == ViewMode::List {
                app.show_task_graph = !app.show_task_graph;
            }
            Ok(true)
        }
        KeyCode::Char('?') => {
            app.show_help = !app.show_help;
            Ok(true)
        }
        KeyCode::Tab => {
            if app.view_mode == ViewMode::Graph {
                app.cycle_task_selection(true);
            } else {
                app.cycle_focus();
            }
            Ok(true)
        }
        KeyCode::Enter => {
            if app.view_mode == ViewMode::Graph {
                app.toggle_expanded_node();
            } else {
                app.input_mode = InputMode::Typing;
                app.input_buffer.clear();
            }
            Ok(true)
        }
        KeyCode::Esc => {
            if app.view_mode == ViewMode::Graph {
                app.collapse_expanded_node();
            }
            Ok(true)
        }
        KeyCode::Left => {
            if app.view_mode == ViewMode::Graph {
                app.move_task_selection(-1, 0);
            }
            Ok(true)
        }
        KeyCode::Right => {
            if app.view_mode == ViewMode::Graph {
                app.move_task_selection(1, 0);
            }
            Ok(true)
        }
        KeyCode::Up => {
            if app.view_mode == ViewMode::Graph {
                app.move_task_selection(0, -1);
            }
            Ok(true)
        }
        KeyCode::Down => {
            if app.view_mode == ViewMode::Graph {
                app.move_task_selection(0, 1);
            }
            Ok(true)
        }
        KeyCode::Char('h') => {
            if app.view_mode == ViewMode::Graph {
                app.dag_scroll.0 = app.dag_scroll.0.saturating_sub(4);
            }
            Ok(true)
        }
        KeyCode::Char('j') => {
            if app.view_mode == ViewMode::Graph {
                app.dag_scroll.1 = app.dag_scroll.1.saturating_add(2);
            }
            Ok(true)
        }
        KeyCode::Char('k') => {
            if app.view_mode == ViewMode::Graph {
                app.dag_scroll.1 = app.dag_scroll.1.saturating_sub(2);
                return Ok(true);
            }
            if let Some(agent) = app.focused_agent_info() {
                client.kill_agent(agent.id).await?;
                app.set_status_message(format!("killed {}", agent.task));
            }
            Ok(true)
        }
        KeyCode::Char('l') => {
            if app.view_mode == ViewMode::Graph {
                app.dag_scroll.0 = app.dag_scroll.0.saturating_add(4);
            }
            Ok(true)
        }
        KeyCode::Char(ch) if ch.is_ascii_digit() && ch != '0' => {
            let index = ch.to_digit(10).unwrap_or(1) as usize - 1;
            if app.view_mode == ViewMode::Graph {
                let ordered = app
                    .dag_layout
                    .iter()
                    .flatten()
                    .map(|(task_id, _)| task_id.clone())
                    .collect::<Vec<_>>();
                if let Some(task_id) = ordered.get(index) {
                    app.selected_task = Some(task_id.clone());
                }
            } else {
                app.focus_index(index);
            }
            Ok(true)
        }
        _ => Ok(true),
    }
}

async fn handle_typing_mode(
    app: &mut App,
    client: &DaemonClient,
    key: KeyEvent,
) -> io::Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        if let Some(agent) = app.focused_agent_info() {
            client.write_to_agent(agent.id, "\u{3}").await?;
            app.set_status_message("sent ctrl-c to agent");
        }
        return Ok(true);
    }

    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            app.input_buffer.clear();
        }
        KeyCode::Enter => {
            if let Some(agent) = app.focused_agent_info() {
                let input = format!("{}\n", app.input_buffer);
                client.write_to_agent(agent.id, input).await?;
                app.set_status_message("input sent");
            }
            app.input_buffer.clear();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            app.input_buffer.pop();
        }
        KeyCode::Char(ch) => {
            if !key.modifiers.contains(KeyModifiers::CONTROL) {
                app.input_buffer.push(ch);
            }
        }
        _ => {}
    }

    Ok(true)
}
