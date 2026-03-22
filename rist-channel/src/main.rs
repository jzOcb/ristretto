//! Channel daemon that subscribes to ristd events and fans them out to targets.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use clap::Parser;
use serde::Deserialize;

use rist::daemon_client::{ClientEvent, DaemonClient};
use rist_channel::event_router::{EventRouter, RouteTarget};
use rist_channel::transports::{
    EventTransport, FileTransport, McpChannelTransport, StdinTransport, WebhookTransport,
};
use rist_shared::protocol::Event;
use rist_shared::EventFilter;

/// CLI arguments for `rist-channel`.
#[derive(Debug, Parser)]
#[command(
    name = "rist-channel",
    version,
    about = "Push ristd events to external channels"
)]
struct Cli {
    /// Path to the daemon Unix socket.
    #[arg(long)]
    socket: Option<PathBuf>,
    /// Path to the channel configuration file.
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
struct ChannelConfig {
    #[serde(default)]
    default_routes: Vec<ConfigRoute>,
    #[serde(default)]
    webhooks: Vec<WebhookConfig>,
}

#[derive(Debug, Deserialize)]
struct ConfigRoute {
    event: EventFilter,
    transport: ConfigTransport,
    path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConfigTransport {
    File,
    Webhook,
}

#[derive(Debug, Deserialize)]
struct WebhookConfig {
    url: String,
    events: Vec<EventFilter>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let cli = Cli::parse();
    let socket_path = cli.socket.unwrap_or_else(default_socket_path);
    let config_path = cli.config.unwrap_or_else(default_config_path);

    let client = DaemonClient::connect(socket_path).await?;
    let mut events = client.subscribe();
    let mut router = load_router(&config_path)?;

    while let Ok(client_event) = events.recv().await {
        if let ClientEvent::Daemon(event) = client_event {
            dispatch_event(&client, &router, &event).await;

            if let Event::AgentExited { id, .. } = event {
                router.remove_session(id);
            }
        }
    }

    Ok(())
}

fn load_router(path: &Path) -> io::Result<EventRouter> {
    let config = load_config(path)?;
    let mut router = EventRouter::new();

    for route in config.default_routes {
        match route.transport {
            ConfigTransport::File => {
                let path = route.path.unwrap_or_else(default_notification_path);
                router.add_route(route.event, RouteTarget::FileNotification { path });
            }
            ConfigTransport::Webhook => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "default_routes.transport = \"webhook\" is unsupported; use [[webhooks]]",
                ));
            }
        }
    }

    for webhook in config.webhooks {
        for event in webhook.events {
            router.add_route(
                event,
                RouteTarget::Webhook {
                    url: webhook.url.clone(),
                },
            );
        }
    }

    Ok(router)
}

fn load_config(path: &Path) -> io::Result<ChannelConfig> {
    if !path.exists() {
        return Ok(ChannelConfig::default());
    }

    let raw = fs::read_to_string(path)?;
    toml::from_str(&raw).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid channel config {}: {error}", path.display()),
        )
    })
}

async fn dispatch_event(client: &DaemonClient, router: &EventRouter, event: &Event) {
    let mcp_transport = McpChannelTransport;

    for target in router.route(event) {
        let result = match target {
            RouteTarget::FileNotification { path } => FileTransport::write_event(path, event),
            RouteTarget::Webhook { url } => WebhookTransport::post_event(url, event),
            RouteTarget::AgentStdin { session_id } => {
                StdinTransport::write_event(client, *session_id, event).await
            }
            RouteTarget::McpChannel { .. } => {
                let payload = McpChannelTransport::format_event(event);
                mcp_transport.push(target, &payload)
            }
        };

        if let Err(error) = result {
            let rendered_target = render_target(target);
            eprintln!("failed to deliver event to {rendered_target}: {error}");
        }
    }
}

fn default_socket_path() -> PathBuf {
    ristretto_dir().join("daemon.sock")
}

fn default_config_path() -> PathBuf {
    ristretto_dir().join("channel.toml")
}

fn default_notification_path() -> PathBuf {
    ristretto_dir().join("channel-events.jsonl")
}

fn ristretto_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ristretto")
}

fn render_target(target: &RouteTarget) -> String {
    match target {
        RouteTarget::FileNotification { path } => path.display().to_string(),
        RouteTarget::Webhook { url } => url.clone(),
        RouteTarget::AgentStdin { session_id } | RouteTarget::McpChannel { session_id } => {
            session_id.0.to_string()
        }
    }
}
