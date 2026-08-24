use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    time::Duration,
};

use agentsassemble_domain::{
    LOCAL_OPERATOR_PARTICIPANT_ID, Participant, ParticipantStatus, Room, RoomSettings,
};
use agentsassemble_persistence::{SqliteStore, secure_private_directory};
use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService};
use agentsassemble_server::{
    AppState, HostSecret, TicketIssueError, TicketStore, issue_local_ticket,
    reconcile_runtime_ownership, serve,
};
use anyhow::Context;
use chrono::Utc;
use clap::Parser;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, Stdin},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

const MAX_CONTROL_SECRET_BYTES: usize = 128;
const MAX_CONTROL_MESSAGE_BYTES: usize = 4 * 1024;

#[derive(Debug, Parser)]
#[command(name = "agentsassemble-server")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:0")]
    bind: SocketAddr,
    #[arg(long, default_value = ".agentsassemble-rust/runtime.sqlite3")]
    database: PathBuf,
    #[arg(long)]
    initialize_room: Option<String>,
    #[arg(long)]
    frontend: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if run_internal_provider_mode().await {
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();
    let mut stdin = tokio::io::stdin();
    let host_token = read_control_secret(&mut stdin).await?;
    let host_secret = HostSecret::new(host_token)?;
    let cancellation = CancellationToken::new();
    if args.bind.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) && !args.bind.ip().is_loopback() {
        anyhow::bail!("the local runtime may bind only to loopback");
    }
    if let Some(parent) = args.database.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create database directory {}", parent.display()))?;
        secure_private_directory(parent)
            .with_context(|| format!("secure database directory {}", parent.display()))?;
    }
    let store = SqliteStore::open_path(&args.database).await?;
    let database_path = args
        .database
        .canonicalize()
        .with_context(|| format!("resolve database path {}", args.database.display()))?;
    ensure_parent_alive(&cancellation)?;
    if store.was_created()
        && let Some(room_id) = args.initialize_room.as_deref()
    {
        initialize_room(&store, room_id).await?;
    }
    let provider_adapter = ProviderAdapter::new();
    let reconciled_sessions = reconcile_runtime_ownership(&store, &provider_adapter).await?;
    if reconciled_sessions > 0 {
        tracing::warn!(
            reconciled_sessions,
            "disconnected stale provider sessions before network admission"
        );
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
    let mut state = AppState::local_with_provider_adapter(
        store,
        TicketStore::new(Duration::from_secs(30), 4_096),
        host_secret,
        ProviderCatalogService::discovering(),
        provider_adapter,
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
    let mut stdout = tokio::io::stdout();
    write_json_line(
        &mut stdout,
        &serde_json::json!({
            "status": "ready",
            "runtime": "rust",
            "address": format!("http://{address}"),
            "database": database_path,
            "frontend": frontend_path,
            "pid": std::process::id(),
        }),
    )
    .await?;
    let control_state = state.clone();
    let control_cancellation = cancellation.clone();
    tokio::spawn(async move {
        run_control_pipe(
            &mut stdin,
            &mut stdout,
            control_state,
            &control_cancellation,
        )
        .await;
        control_cancellation.cancel();
    });
    serve(listener, state, cancellation).await?;
    Ok(())
}

async fn run_internal_provider_mode() -> bool {
    #[cfg(any(unix, windows))]
    if let Some(code) = agentsassemble_provider::run_room_helper_if_requested().await {
        std::process::exit(code);
    }
    #[cfg(unix)]
    if let Some(code) = agentsassemble_provider::run_process_helper_if_requested() {
        std::process::exit(code);
    }
    false
}

async fn run_control_pipe<R, W>(
    reader: &mut R,
    writer: &mut W,
    state: AppState,
    cancellation: &CancellationToken,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    loop {
        let line = tokio::select! {
            () = cancellation.cancelled() => return,
            line = read_control_line(reader) => line,
        };
        let Ok(Some(line)) = line else {
            return;
        };
        let response = match serde_json::from_slice::<LocalControlRequest>(&line) {
            Ok(LocalControlRequest::IssueTicket {
                request_id,
                meeting_id,
            }) if !request_id.is_empty() && request_id.len() <= 128 => {
                match issue_local_ticket(&state, &meeting_id).await {
                    Ok(ticket) => LocalControlResponse::Ok {
                        request_id,
                        ticket: ticket.ticket,
                        ttl_seconds: ticket.ttl_seconds,
                        server_proof_key: ticket.server_proof_key,
                    },
                    Err(error) => control_error(request_id, error),
                }
            }
            Ok(LocalControlRequest::IssueTicket { request_id, .. }) => {
                LocalControlResponse::Error {
                    request_id,
                    code: "request_id_invalid".to_owned(),
                    message: "Control request id is invalid.".to_owned(),
                }
            }
            Err(_) => LocalControlResponse::Error {
                request_id: String::new(),
                code: "control_request_invalid".to_owned(),
                message: "Control request JSON is invalid.".to_owned(),
            },
        };
        if write_json_line(writer, &response).await.is_err() {
            return;
        }
    }
}

fn control_error(request_id: String, error: TicketIssueError) -> LocalControlResponse {
    let (code, message) = match error {
        TicketIssueError::InvalidRoom(message) => ("bad_request", message),
        TicketIssueError::RoomMissing => ("room_not_found", "Room does not exist.".to_owned()),
        TicketIssueError::ParticipantInactive => (
            "session_revoked",
            "The local operator is not an active room participant.".to_owned(),
        ),
        TicketIssueError::Persistence(_) => (
            "persistence_failed",
            "Persistence operation failed.".to_owned(),
        ),
        TicketIssueError::Unavailable => {
            ("unavailable", "Ticket capacity is unavailable.".to_owned())
        }
    };
    LocalControlResponse::Error {
        request_id,
        code: code.to_owned(),
        message,
    }
}

async fn read_control_line<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> anyhow::Result<Option<Vec<u8>>> {
    let mut line = Vec::with_capacity(256);
    let mut byte = [0_u8; 1];
    for _ in 0..=MAX_CONTROL_MESSAGE_BYTES {
        let count = reader
            .read(&mut byte)
            .await
            .context("read parent control pipe")?;
        if count == 0 {
            return if line.is_empty() {
                Ok(None)
            } else {
                anyhow::bail!("control pipe closed during a request")
            };
        }
        if byte[0] == b'\n' {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(Some(line));
        }
        line.push(byte[0]);
    }
    anyhow::bail!("control request exceeds {MAX_CONTROL_MESSAGE_BYTES} bytes")
}

async fn write_json_line<W: AsyncWrite + Unpin>(
    writer: &mut W,
    value: &impl serde::Serialize,
) -> anyhow::Result<()> {
    let mut encoded = serde_json::to_vec(value)?;
    encoded.push(b'\n');
    writer.write_all(&encoded).await?;
    writer.flush().await?;
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

async fn initialize_room(store: &SqliteStore, room_id: &str) -> anyhow::Result<()> {
    let now = Utc::now();
    let label = room_id.replace(['-', '_'], " ");
    let room = Room::new(room_id.to_owned(), label.clone(), now);
    let participant = Participant {
        room_id: room_id.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "SeiNel".to_owned(),
        avatar_image_url: String::new(),
        participant_type: "human".to_owned(),
        status: ParticipantStatus::Joined,
        role: "host".to_owned(),
        owner_id: String::new(),
        muted: false,
        created_at: now,
        updated_at: now,
    };
    store
        .initialize_room(&room, &RoomSettings::defaults(label), &participant)
        .await?;
    Ok(())
}
