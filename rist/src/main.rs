//! Terminal client for the Ristretto daemon.

mod app;
mod event;
mod ui;

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use app::App;
use clap::Parser;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use rist::daemon_client::DaemonClient;
use rist_shared::i18n::tr;
use tokio::time::MissedTickBehavior;

/// Command line arguments for the Ristretto TUI.
#[derive(Debug, Parser)]
#[command(name = "rist", version, about = "Ristretto terminal client")]
struct Args {
    /// Override the daemon Unix socket path.
    #[arg(long)]
    socket: Option<PathBuf>,
    /// Override the display locale.
    #[arg(long, value_parser = ["en", "zh-CN"])]
    lang: Option<String>,
}

fn ristretto_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ristretto")
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = stdout.execute(LeaveAlternateScreen);
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        restore_terminal();
        previous(panic_info);
    }));
}

fn init_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();
    if let Some(lang) = &args.lang {
        std::env::set_var("RISTRETTO_LANG", lang);
    }

    let socket_path = args
        .socket
        .unwrap_or_else(|| ristretto_dir().join("daemon.sock"));
    let client = match DaemonClient::connect(socket_path).await {
        Ok(client) => client,
        Err(_) => {
            eprintln!("Daemon not running. Start with: ristd");
            return Ok(());
        }
    };

    if client.ping().await.is_err() {
        eprintln!("Daemon not running. Start with: ristd");
        return Ok(());
    }

    install_panic_hook();
    let mut terminal = init_terminal()?;
    let mut event_rx = event::spawn_terminal_events();
    let mut client_events = client.subscribe();
    let mut app = App::new(
        args.lang
            .unwrap_or_else(rist_shared::i18n::preferred_locale),
    );

    app.set_status_message(tr("tui.help"));
    app.refresh_agents(client.list_agents().await?);
    app.refresh_visible_outputs(&client).await;

    let mut agent_tick = tokio::time::interval(Duration::from_secs(1));
    agent_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut output_tick = tokio::time::interval(Duration::from_secs(2));
    output_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    while app.running {
        terminal.draw(|frame| ui::render(frame, &app))?;

        tokio::select! {
            Some(term_event) = event_rx.recv() => {
                if !event::handle_terminal_event(&mut app, &client, term_event).await? {
                    break;
                }
            }
            Ok(client_event) = client_events.recv() => {
                app.apply_client_event(client_event);
            }
            _ = agent_tick.tick() => {
                match client.list_agents().await {
                    Ok(agents) => app.refresh_agents(agents),
                    Err(error) => app.set_status_message(format!("daemon: {error}")),
                }
            }
            _ = output_tick.tick() => {
                app.refresh_visible_outputs(&client).await;
            }
        }
    }

    restore_terminal();
    let _ = client.disconnect().await;
    Ok(())
}
