//! Crossterm event handling and keybinding dispatch.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::{self, UnboundedReceiver};

use crate::app::{App, InputMode};
use crate::daemon_client::DaemonClient;

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
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.running = false;
            Ok(false)
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            app.set_status_message("spawn not implemented");
            Ok(true)
        }
        KeyCode::Char('k') | KeyCode::Char('K') => {
            if let Some(agent) = app.focused_agent_info() {
                client.kill_agent(agent.id).await?;
                app.set_status_message(format!("killed {}", agent.task));
            }
            Ok(true)
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            app.toggle_split();
            Ok(true)
        }
        KeyCode::Char('p') | KeyCode::Char('P') => {
            app.show_task_graph = !app.show_task_graph;
            Ok(true)
        }
        KeyCode::Char('?') => {
            app.show_help = !app.show_help;
            Ok(true)
        }
        KeyCode::Tab => {
            app.cycle_focus();
            Ok(true)
        }
        KeyCode::Enter => {
            app.input_mode = InputMode::Typing;
            app.input_buffer.clear();
            Ok(true)
        }
        KeyCode::Char(ch) if ch.is_ascii_digit() && ch != '0' => {
            let index = ch.to_digit(10).unwrap_or(1) as usize - 1;
            app.focus_index(index);
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
