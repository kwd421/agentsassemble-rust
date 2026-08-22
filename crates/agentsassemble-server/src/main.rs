use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use agentsassemble_domain::{Participant, ParticipantStatus, Room, RoomSettings};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_server::{AppState, RoomRuntime, TicketStore, serve};
use anyhow::Context;
use chrono::Utc;
use clap::Parser;
use serde_json::json;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "agentsassemble-server")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:0")]
    bind: SocketAddr,
    #[arg(long, default_value = ".agentsassemble-rust/runtime.sqlite3")]
    database: PathBuf,
    #[arg(long)]
    bootstrap_room: Option<String>,
    #[arg(long, env = "AGENTSASSEMBLE_HOST_TOKEN", default_value = "")]
    host_token: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();
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
    if let Some(room_id) = args.bootstrap_room.as_deref() {
        bootstrap(&store, room_id).await?;
    }
    let listener = TcpListener::bind(args.bind).await?;
    let address = listener.local_addr()?;
    let cancellation = CancellationToken::new();
    let signal = cancellation.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal.cancel();
        }
    });
    let state = AppState {
        rooms: RoomRuntime::new(store.clone()),
        store,
        tickets: TicketStore::new(Duration::from_secs(30), 4_096),
        host_token: Arc::from(args.host_token),
    };
    println!(
        "{}",
        serde_json::to_string(&json!({
            "status": "ready",
            "runtime": "rust",
            "address": format!("http://{address}"),
            "database": database_path,
            "pid": std::process::id(),
        }))?
    );
    serve(listener, state, cancellation).await?;
    Ok(())
}

async fn bootstrap(store: &SqliteStore, room_id: &str) -> anyhow::Result<()> {
    let now = Utc::now();
    let label = room_id.replace(['-', '_'], " ");
    let room = Room::new(room_id.to_owned(), label.clone(), now);
    let participant = Participant {
        room_id: room_id.to_owned(),
        participant_id: "host".to_owned(),
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
