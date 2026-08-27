use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use agentsassemble_persistence::{
    LocalBootstrapPhase as PersistenceBootstrapPhase, LocalBootstrapStatus, PersistenceError,
    SqliteStore, secure_private_directory,
};
use agentsassemble_protocol::{
    LocalBootstrapGrant, LocalBootstrapPhase, LocalControlRequest, LocalControlResponse,
    ServerProductSurface,
};
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService};
use agentsassemble_server::{
    AppState, HostSecret, StableEntryConfig, TicketIssueError, TicketStore,
    issue_central_registration_ticket, issue_local_operator_http_ticket, issue_local_ticket,
    issue_preferences_read_ticket, issue_preferences_write_ticket,
    issue_settings_directory_read_ticket, local_bind_is_supported, serve,
};
use anyhow::Context;
use clap::Parser;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, Stdin},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

const MAX_CONTROL_SECRET_BYTES: usize = 128;
const MAX_CONTROL_MESSAGE_BYTES: usize = 4 * 1024;
const PUBLIC_URL_ENV: &str = "AGENTSASSEMBLE_PUBLIC_URL";
const TRUSTED_PROXY_TOKEN_ENV: &str = "AGENTSASSEMBLE_TRUSTED_PROXY_TOKEN";

#[derive(Debug, Parser)]
#[command(name = "agentsassemble-server")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:0")]
    bind: SocketAddr,
    #[arg(long, default_value = ".agentsassemble-rust/runtime.sqlite3")]
    database: PathBuf,
    #[arg(long)]
    frontend: Option<PathBuf>,
    #[arg(long)]
    desktop_native_registration: bool,
    #[arg(long)]
    stable_entry_config: Option<PathBuf>,
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
    let manual_public_ingress = manual_public_ingress_environment()?;
    let stable_entry = stable_entry_configuration(
        args.stable_entry_config.as_deref(),
        manual_public_ingress.is_some(),
    )?;
    let mut stdin = tokio::io::stdin();
    let host_token = read_control_secret(&mut stdin).await?;
    let host_secret = HostSecret::new(host_token)?;
    let cancellation = CancellationToken::new();
    if !local_bind_is_supported(args.bind) {
        anyhow::bail!("the local runtime may bind only to loopback");
    }
    if let Some(parent) = args.database.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create database directory {}", parent.display()))?;
        secure_private_directory(parent)
            .with_context(|| format!("secure database directory {}", parent.display()))?;
    }
    let store = open_store(&args).await?;
    let database_path = args
        .database
        .canonicalize()
        .with_context(|| format!("resolve database path {}", args.database.display()))?;
    ensure_parent_alive(&cancellation)?;
    let provider_adapter = ProviderAdapter::new();
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
    )
    .await?;
    if let Some((origin, proxy_secret)) = manual_public_ingress {
        state = state.with_manual_public_ingress(&origin, &proxy_secret)?;
    } else {
        let state_root = database_state_root(&database_path)?;
        state = state.with_managed_public_ingress(address, stable_entry, state_root);
    }
    if args.desktop_native_registration {
        state = state.with_central_registration();
    }
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

async fn open_store(args: &Args) -> anyhow::Result<SqliteStore> {
    Ok(SqliteStore::open_path(&args.database).await?)
}

fn database_state_root(database: &Path) -> anyhow::Result<&Path> {
    database.parent().context("database path has no state root")
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
        let response = control_response(&state, &line).await;
        if write_json_line(writer, &response).await.is_err() {
            return;
        }
    }
}

async fn control_response(state: &AppState, line: &[u8]) -> LocalControlResponse {
    let Ok(request) = serde_json::from_slice::<LocalControlRequest>(line) else {
        return LocalControlResponse::Error {
            request_id: String::new(),
            code: "control_request_invalid".to_owned(),
            message: "Control request JSON is invalid.".to_owned(),
        };
    };
    let request_id = control_request_id(&request).to_owned();
    if !valid_control_request_id(&request_id) {
        return LocalControlResponse::Error {
            request_id,
            code: "request_id_invalid".to_owned(),
            message: "Control request id is invalid.".to_owned(),
        };
    }
    match request {
        LocalControlRequest::InspectBootstrap { .. } => {
            match state.store.local_bootstrap_status().await {
                Ok(status) => LocalControlResponse::BootstrapOk {
                    request_id,
                    bootstrap: Box::new(bootstrap_grant(
                        status,
                        false,
                        &state.server_product_surface,
                    )),
                },
                Err(error) => bootstrap_control_error(request_id, error),
            }
        }
        LocalControlRequest::InitializeBootstrap { display_name, .. } => {
            match state
                .store
                .bootstrap_local_authority(&request_id, &display_name)
                .await
            {
                Ok(commit) => LocalControlResponse::BootstrapOk {
                    request_id,
                    bootstrap: Box::new(bootstrap_grant(
                        commit.status,
                        commit.deduplicated,
                        &state.server_product_surface,
                    )),
                },
                Err(error) => bootstrap_control_error(request_id, error),
            }
        }
        LocalControlRequest::IssueTicket { meeting_id, .. } => {
            match issue_local_ticket(state, &meeting_id).await {
                Ok(ticket) => LocalControlResponse::Ok {
                    request_id,
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
                    server_proof_key: ticket.server_proof_key,
                },
                Err(error) => control_error(request_id, error),
            }
        }
        LocalControlRequest::IssueOperatorHttpTicket { .. } => {
            match issue_local_operator_http_ticket(state).await {
                Ok(ticket) => LocalControlResponse::OperatorHttpOk {
                    request_id,
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
                },
                Err(error) => control_error(request_id, error),
            }
        }
        LocalControlRequest::IssuePreferencesReadTicket { meeting_id, .. } => {
            settings_ticket_control_response(
                state,
                request_id,
                SettingsTicketRequest::PreferencesRead(meeting_id),
            )
            .await
        }
        LocalControlRequest::IssuePreferencesWriteTicket { meeting_id, .. } => {
            settings_ticket_control_response(
                state,
                request_id,
                SettingsTicketRequest::PreferencesWrite(meeting_id),
            )
            .await
        }
        LocalControlRequest::IssueSettingsDirectoryReadTicket { .. } => {
            settings_ticket_control_response(
                state,
                request_id,
                SettingsTicketRequest::DirectoryRead,
            )
            .await
        }
        LocalControlRequest::IssueCentralRegistrationTicket { .. } => {
            central_registration_control_response(state, request_id).await
        }
    }
}

enum SettingsTicketRequest {
    PreferencesRead(String),
    PreferencesWrite(String),
    DirectoryRead,
}

async fn settings_ticket_control_response(
    state: &AppState,
    request_id: String,
    request: SettingsTicketRequest,
) -> LocalControlResponse {
    match request {
        SettingsTicketRequest::PreferencesRead(room_id) => {
            match issue_preferences_read_ticket(state, &room_id).await {
                Ok(ticket) => LocalControlResponse::PreferencesReadOk {
                    request_id,
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
                },
                Err(error) => control_error(request_id, error),
            }
        }
        SettingsTicketRequest::PreferencesWrite(room_id) => {
            match issue_preferences_write_ticket(state, &room_id).await {
                Ok(ticket) => LocalControlResponse::PreferencesWriteOk {
                    request_id,
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
                },
                Err(error) => control_error(request_id, error),
            }
        }
        SettingsTicketRequest::DirectoryRead => {
            match issue_settings_directory_read_ticket(state).await {
                Ok(ticket) => LocalControlResponse::SettingsDirectoryReadOk {
                    request_id,
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
                },
                Err(error) => control_error(request_id, error),
            }
        }
    }
}

fn control_request_id(request: &LocalControlRequest) -> &str {
    match request {
        LocalControlRequest::InspectBootstrap { request_id }
        | LocalControlRequest::InitializeBootstrap { request_id, .. }
        | LocalControlRequest::IssueTicket { request_id, .. }
        | LocalControlRequest::IssueOperatorHttpTicket { request_id }
        | LocalControlRequest::IssuePreferencesReadTicket { request_id, .. }
        | LocalControlRequest::IssuePreferencesWriteTicket { request_id, .. }
        | LocalControlRequest::IssueSettingsDirectoryReadTicket { request_id }
        | LocalControlRequest::IssueCentralRegistrationTicket { request_id } => request_id,
    }
}

async fn central_registration_control_response(
    state: &AppState,
    request_id: String,
) -> LocalControlResponse {
    match issue_central_registration_ticket(state).await {
        Ok(ticket) => {
            let (server_id, host_public_key_x, host_key_fingerprint) =
                state.central_registration_binding();
            LocalControlResponse::CentralRegistrationOk {
                request_id,
                ticket: ticket.ticket,
                ttl_seconds: ticket.ttl_seconds,
                server_id: server_id.to_owned(),
                host_public_key_x: host_public_key_x.to_owned(),
                host_key_fingerprint: host_key_fingerprint.to_owned(),
            }
        }
        Err(error) => control_error(request_id, error),
    }
}

fn valid_control_request_id(request_id: &str) -> bool {
    !request_id.is_empty() && request_id.len() <= 128
}

fn bootstrap_grant(
    status: LocalBootstrapStatus,
    deduplicated: bool,
    surface: &ServerProductSurface,
) -> LocalBootstrapGrant {
    let phase = match status.phase {
        PersistenceBootstrapPhase::Empty => LocalBootstrapPhase::Empty,
        PersistenceBootstrapPhase::Initializing => LocalBootstrapPhase::Initializing,
        PersistenceBootstrapPhase::Complete => LocalBootstrapPhase::Complete,
        PersistenceBootstrapPhase::RepairRequired => LocalBootstrapPhase::RepairRequired,
    };
    LocalBootstrapGrant {
        phase,
        authority_lineage_id: status.authority_lineage_id,
        server_id: status.server_id,
        server_product_surface_revision: surface.revision,
        server_product_surface_digest: surface.digest.clone(),
        profile: status.profile,
        deduplicated,
    }
}

fn bootstrap_control_error(request_id: String, error: PersistenceError) -> LocalControlResponse {
    let (code, message) = match error {
        PersistenceError::CommandRejected { code, message } => (code.to_owned(), message),
        _ => (
            "bootstrap_persistence_failed".to_owned(),
            "Local bootstrap authority could not be read or changed.".to_owned(),
        ),
    };
    LocalControlResponse::Error {
        request_id,
        code,
        message,
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
        TicketIssueError::BootstrapIncomplete => (
            "bootstrap_required",
            "Local identity bootstrap is not complete.".to_owned(),
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

fn manual_public_ingress_environment() -> anyhow::Result<Option<(String, String)>> {
    let origin = unicode_environment(PUBLIC_URL_ENV)?;
    let proxy_secret = unicode_environment(TRUSTED_PROXY_TOKEN_ENV)?;
    match (origin, proxy_secret) {
        (None, None) => Ok(None),
        (Some(origin), Some(proxy_secret)) => Ok(Some((origin, proxy_secret))),
        _ => anyhow::bail!(
            "{PUBLIC_URL_ENV} and {TRUSTED_PROXY_TOKEN_ENV} must be configured together"
        ),
    }
}

fn stable_entry_configuration(
    path: Option<&Path>,
    manual_public_ingress: bool,
) -> anyhow::Result<Option<StableEntryConfig>> {
    if manual_public_ingress && path.is_some() {
        anyhow::bail!("stable entry applies only to the managed public tunnel");
    }
    path.map(StableEntryConfig::load)
        .transpose()
        .map_err(Into::into)
}

fn unicode_environment(name: &str) -> anyhow::Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            anyhow::bail!("{name} must contain valid UTF-8")
        }
    }
}
