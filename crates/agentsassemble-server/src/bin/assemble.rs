use agentsassemble_server::connector_mcp::transport::{serve_remote, serve_stdio};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "assemble", about = "AgentsAssemble external room clients")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Room {
        #[command(subcommand)]
        command: RoomCommand,
    },
}
#[derive(Subcommand)]
enum RoomCommand {
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
