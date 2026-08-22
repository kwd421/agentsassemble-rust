use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    time::Duration,
};

use agentsassemble_domain::{
    LOCAL_OPERATOR_PARTICIPANT_ID, Participant, ParticipantStatus, Room, RoomSettings,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_server::{AppState, HostSecret, TicketStore, serve};
use anyhow::Context;
use chrono::Utc;
use clap::Parser;
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, Stdin},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

const MAX_CONTROL_SECRET_BYTES: usize = 128;

#[derive(Debug, Parser)]
#[command(name = "agentsassemble-server")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:0")]
    bind: SocketAddr,
    #[arg(long, default_value = ".agentsassemble-rust/runtime.sqlite3")]
    database: PathBuf,
    #[arg(long)]
    bootstrap_room: Option<String>,
    #[arg(long)]
    frontend: Option<PathBuf>,
    #[arg(long, env = "AGENTSASSEMBLE_HOST_TOKEN")]
    host_token: Option<String>,
    #[arg(long)]
    control_stdin: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();
    let (host_token, control_stdin) = if args.control_stdin {
        let mut stdin = tokio::io::stdin();
        let secret = read_control_secret(&mut stdin).await?;
        (secret, Some(stdin))
    } else {
        (
            args.host_token
                .context("--host-token or AGENTSASSEMBLE_HOST_TOKEN is required")?,
            None,
        )
    };
    let host_secret = HostSecret::new(host_token)?;
    let cancellation = CancellationToken::new();
    if let Some(mut stdin) = control_stdin {
        let parent_death = cancellation.clone();
        tokio::spawn(async move {
            let mut byte = [0_u8; 1];
            let _ = stdin.read(&mut byte).await;
            parent_death.cancel();
        });
    }
    if args.bind.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) && !args.bind.ip().is_loopback() {
        anyhow::bail!("the local runtime may bind only to loopback");
    }
    if let Some(parent) = args.database.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create database directory {}", parent.display()))?;
    }
    let database_path = args
        .database
        .canonicalize()
        .unwrap_or_else(|_| args.database.clone());
    let database_url = format!("sqlite://{}", args.database.display());
    let store = SqliteStore::open(&database_url).await?;
    ensure_parent_alive(&cancellation)?;
    if let Some(room_id) = args.bootstrap_room.as_deref() {
        bootstrap(&store, room_id).await?;
    }
    ensure_parent_alive(&cancellation)?;
    let listener = TcpListener::bind(args.bind).await?;
    ensure_parent_alive(&cancellation)?;
    let address = listener.local_addr()?;
    let signal = cancellation.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal.cancel();
        }
    });
    let mut state = AppState::local(
        store,
        TicketStore::new(Duration::from_secs(30), 4_096),
        host_secret,
    );
    let frontend_path = if let Some(frontend) = args.frontend {
        let path = frontend
            .canonicalize()
            .with_context(|| format!("resolve frontend directory {}", frontend.display()))?;
        if !path.join("index.html").is_file() {
            anyhow::bail!("frontend directory {} has no index.html", path.display());
        }
        state = state.with_frontend(path.clone());
        Some(path)
    } else {
        None
    };
    println!(
        "{}",
        serde_json::to_string(&json!({
            "status": "ready",
            "runtime": "rust",
            "address": format!("http://{address}"),
            "database": database_path,
            "frontend": frontend_path,
            "pid": std::process::id(),
        }))?
    );
    serve(listener, state, cancellation).await?;
    Ok(())
}

async fn read_control_secret(stdin: &mut Stdin) -> anyhow::Result<String> {
    let mut bytes = Vec::with_capacity(64);
    let mut byte = [0_u8; 1];
    for _ in 0..=MAX_CONTROL_SECRET_BYTES {
        let count = stdin
            .read(&mut byte)
            .await
            .context("read parent control pipe")?;
        if count == 0 {
            anyhow::bail!("parent control pipe closed before the host secret");
        }
        if byte[0] == b'\n' {
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            return String::from_utf8(bytes).context("parent control secret is not UTF-8");
        }
        bytes.push(byte[0]);
    }
    anyhow::bail!("parent control secret exceeds {MAX_CONTROL_SECRET_BYTES} bytes")
}

fn ensure_parent_alive(cancellation: &CancellationToken) -> anyhow::Result<()> {
    if cancellation.is_cancelled() {
        anyhow::bail!("parent control pipe closed during startup");
    }
    Ok(())
}

async fn bootstrap(store: &SqliteStore, room_id: &str) -> anyhow::Result<()> {
    let now = Utc::now();
    let label = room_id.replace(['-', '_'], " ");
    let room = Room::new(room_id.to_owned(), label.clone(), now);
    let participant = Participant {
        room_id: room_id.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "Host".to_owned(),
        participant_type: "human".to_owned(),
        status: ParticipantStatus::Joined,
        role: "host".to_owned(),
        owner_id: String::new(),
        muted: false,
        created_at: now,
        updated_at: now,
    };
    store
        .bootstrap_room(&room, &RoomSettings::defaults(label), &participant)
        .await?;
    Ok(())
}
