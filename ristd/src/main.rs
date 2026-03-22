//! Ristretto daemon entry point.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use nix::fcntl::{Flock, FlockArg};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::Mutex;
use tracing::info;
use tracing_subscriber::EnvFilter;

use ristd::pty_manager::PtyManager;
use ristd::session_store::SessionStore;
use ristd::socket_server::SocketServer;

/// Command line arguments for the daemon.
#[derive(Debug, Parser)]
#[command(name = "ristd", version, about = "Ristretto daemon")]
struct Args {
    /// Override the Unix socket path.
    #[arg(long)]
    socket: Option<PathBuf>,
    /// Override the config file path.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Logging filter, for example `info` or `debug`.
    #[arg(long, default_value = "info")]
    log_level: String,
}

fn ristretto_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ristretto")
}

fn lock_pid_file(path: &Path) -> io::Result<Flock<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .read(true)
        .open(path)?;
    let mut file = Flock::lock(file, FlockArg::LockExclusiveNonblock)
        .map_err(|(_, error)| io::Error::new(io::ErrorKind::WouldBlock, error))?;
    use std::io::Write as _;
    file.write_all(format!("{}\n", std::process::id()).as_bytes())?;
    file.sync_all()?;
    Ok(file)
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(args.log_level.clone())),
        )
        .init();

    let base_dir = ristretto_dir();
    fs::create_dir_all(&base_dir)?;

    let _config_path = args.config.unwrap_or_else(|| base_dir.join("config.toml"));
    let socket_path = args.socket.unwrap_or_else(|| base_dir.join("daemon.sock"));
    let pid_path = base_dir.join("daemon.pid");
    let sessions_path = base_dir.join("sessions.json");

    let _pid_file = lock_pid_file(&pid_path)?;
    if socket_path.exists() {
        fs::remove_file(&socket_path)?;
    }

    let session_store = Arc::new(Mutex::new(SessionStore::load(&sessions_path)?));
    let pty_manager = Arc::new(Mutex::new(PtyManager::new()));
    let server = SocketServer::bind(
        &socket_path,
        Arc::clone(&pty_manager),
        Arc::clone(&session_store),
    )
    .await?;

    info!("ristd listening on {}", socket_path.display());

    let server_task = tokio::spawn(server.run());

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        result = server_task => {
            result.map_err(io::Error::other)??;
        }
        _ = sigint.recv() => {
            info!("received SIGINT");
        }
        _ = sigterm.recv() => {
            info!("received SIGTERM");
        }
    }

    if socket_path.exists() {
        fs::remove_file(&socket_path)?;
    }

    Ok(())
}
