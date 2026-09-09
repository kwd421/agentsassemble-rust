use agentsassemble_server::connector_mcp::transport::{serve_remote, serve_stdio};
use clap::{Parser, Subcommand};

#[path = "../attendee_cli.rs"]
mod attendee;
#[path = "../release_health_cli.rs"]
mod release_health;
#[path = "../runtime_restart_cli.rs"]
mod runtime_restart;

#[derive(Parser)]
#[command(name = "assemble", about = "AgentsAssemble external room clients")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Restart the local runtime or inspect its durable result.
    RollingRestart(runtime_restart::RuntimeRestart),
    /// Inspect the frontend build actually served by a running runtime.
    FrontendInfo {
        #[arg(long)]
        server: String,
    },
    ReleaseHealth {
        #[command(subcommand)]
        command: release_health::ReleaseHealth,
    },
    Room {
        #[command(subcommand)]
        command: RoomCommand,
    },
}
#[derive(Subcommand)]
enum RoomCommand {
    /// Join a provider-bound invitation read privately from stdin.
    Attend(attendee::Attend),
    /// Connect the current external AI conversation using MCP over stdin/stdout.
    ConnectorMcp,
    /// Serve loopback MCP for remote AI conversations through a user-owned tunnel.
    ConnectorMcpRemote {
        #[arg(long, default_value_t = 8788)]
        port: u16,
        /// Exact allowed room server base URL; repeat for multiple servers.
        #[arg(long = "allow-room-server", required = true)]
        allowed_servers: Vec<String>,
    },
}

fn main() -> anyhow::Result<()> {
    #[cfg(unix)]
    if let Some(code) = agentsassemble_provider::run_process_helper_if_requested() {
        std::process::exit(code);
    }
    let cli = Cli::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(run(cli.command));
    // MCP and network tasks are joined by their transport owner. Tokio stdin uses
    // an OS-blocking read that cannot be cancelled; it must not prevent this
    // dedicated connector process from exiting while its caller keeps stdin open.
    runtime.shutdown_background();
    result
}

async fn run(command: Command) -> anyhow::Result<()> {
    match command {
        Command::RollingRestart(args) => runtime_restart::run(args).await,
        Command::FrontendInfo { server } => {
            let version = agentsassemble_server::runtime_version::read(&server).await?;
            println!("{}", serde_json::to_string_pretty(&version)?);
            Ok(())
        }
        Command::ReleaseHealth { command } => release_health::run(command).await,
        Command::Room {
            command: RoomCommand::Attend(args),
        } => attendee::run(args).await,
        Command::Room {
            command: RoomCommand::ConnectorMcp,
        } => serve_stdio().await,
        Command::Room {
            command:
                RoomCommand::ConnectorMcpRemote {
                    port,
                    allowed_servers,
                },
        } => serve_remote(port, allowed_servers).await,
    }
}
