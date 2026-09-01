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
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{
    AppState, ManagerRoomAuthorityRequest, StableEntryConfig, TicketIssueError, TicketStore,
    issue_central_registration_ticket, issue_human_invite_create_ticket,
    issue_human_invite_revoke_ticket, issue_local_operator_http_ticket, issue_local_ticket,
    issue_preferences_read_ticket, issue_preferences_write_ticket,
    issue_settings_directory_read_ticket, local_bind_is_supported, serve,
};
use anyhow::Context;
use clap::Parser;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpListener,
};

mod appearance_control;
mod message_attachments_control;
mod message_pins_control;
mod message_search_control;

use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

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
    let frontend_path = resolve_frontend_path(args.frontend.as_deref())?;
    let manual_public_ingress = manual_public_ingress_environment()?;
    let stable_entry = stable_entry_configuration(
        args.stable_entry_config.as_deref(),
        manual_public_ingress.is_some(),
    )?;
    let mut stdin = tokio::io::stdin();
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
    let listener = TcpListener::bind(args.bind).await?;
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
        ProviderCatalogService::discovering(),
    )
    .await?;
    state = configure_startup_surface(
        state,
        manual_public_ingress,
        address,
        stable_entry,
        &database_path,
        args.desktop_native_registration,
    )
    .await?;
    if let Some(frontend) = frontend_path.as_ref() {
        state = state.with_frontend(frontend.clone());
    }
    let mut stdout = tokio::io::stdout();
    if let Err(error) = write_json_line(
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
    .await
    {
        state
            .shutdown_public_ingress()
            .await
            .context("clean public ingress after readiness reporting failed")?;
        return Err(error);
    }
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

fn resolve_frontend_path(frontend: Option<&Path>) -> anyhow::Result<Option<PathBuf>> {
    let Some(frontend) = frontend else {
        return Ok(None);
    };
    let path = frontend
        .canonicalize()
        .with_context(|| format!("resolve frontend directory {}", frontend.display()))?;
    if !path.join("index.html").is_file() {
        anyhow::bail!("frontend directory {} has no index.html", path.display());
    }
    Ok(Some(path))
}

async fn configure_startup_surface(
    state: AppState,
    manual: Option<(String, String)>,
    listener: SocketAddr,
    stable_entry: Option<StableEntryConfig>,
    database: &Path,
    central_registration: bool,
) -> anyhow::Result<AppState> {
    let state = match manual {
        Some((origin, proxy_secret)) => {
            state.with_manual_public_ingress(listener, &origin, &proxy_secret)?
        }
        None => {
            state
                .with_managed_public_ingress(listener, stable_entry, database_state_root(database)?)
                .await?
        }
    };
    Ok(if central_registration {
        state.with_central_registration()
    } else {
        state
    })
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
    let (request_id, request) = match parse_control_request(line) {
        Ok(request) => request,
        Err((request_id, code, message)) => {
            return LocalControlResponse::Error {
                request_id,
                code: code.to_owned(),
                message: message.to_owned(),
            };
        }
    };
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
            initialize_bootstrap_control_response(state, request_id, &display_name).await
        }
        LocalControlRequest::IssueTicket { meeting_id, .. } => {
            match issue_local_ticket(state, &meeting_id).await {
                Ok(ticket) => LocalControlResponse::Ok {
                    request_id,
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
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
        request @ (LocalControlRequest::IssueMessagePinsReadTicket { .. }
        | LocalControlRequest::IssueMessagePinsWriteTicket { .. }) => {
            message_pins_control::response(state, request_id, request).await
        }
        request @ LocalControlRequest::IssueMessageSearchReadTicket { .. } => {
            message_search_control::response(state, request_id, request).await
        }
        request @ (LocalControlRequest::IssueMessageAttachmentUploadTicket { .. }
        | LocalControlRequest::IssueMessageAttachmentReadTicket { .. }) => {
            message_attachments_control::response(state, request_id, request).await
        }
        request @ (LocalControlRequest::IssueHumanInviteCreateTicket { .. }
        | LocalControlRequest::IssueHumanInviteRevokeTicket { .. }) => {
            invite_ticket_control_request(state, request_id, request).await
        }
        request @ (LocalControlRequest::IssueAppearanceUploadTicket { .. }
        | LocalControlRequest::IssueAppearancePendingReadTicket { .. }
        | LocalControlRequest::IssueAppearanceBoundReadTicket { .. }) => {
            appearance_control::response(state, request_id, request).await
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

fn parse_control_request(
    line: &[u8],
) -> Result<(String, LocalControlRequest), (String, &'static str, &'static str)> {
    let request = serde_json::from_slice::<LocalControlRequest>(line).map_err(|_| {
        (
            String::new(),
            "control_request_invalid",
            "Control request JSON is invalid.",
        )
    })?;
    let request_id = control_request_id(&request).to_owned();
    if valid_control_request_id(&request_id) {
        Ok((request_id, request))
    } else {
        Err((
            request_id,
            "request_id_invalid",
            "Control request id is invalid.",
        ))
    }
}

fn manager_request(
    server_id: String,
    authority_lineage_id: String,
    room_id: String,
    expected_room_uid: String,
) -> ManagerRoomAuthorityRequest {
    ManagerRoomAuthorityRequest {
        server_id,
        authority_lineage_id,
        room_id,
        room_uid: expected_room_uid,
    }
}

async fn initialize_bootstrap_control_response(
    state: &AppState,
    request_id: String,
    display_name: &str,
) -> LocalControlResponse {
    match state
        .store
        .bootstrap_local_authority(&request_id, display_name)
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

enum InviteTicketRequest {
    Create(ManagerRoomAuthorityRequest),
    Revoke(ManagerRoomAuthorityRequest),
}

async fn invite_ticket_control_request(
    state: &AppState,
    request_id: String,
    request: LocalControlRequest,
) -> LocalControlResponse {
    let request = match request {
        LocalControlRequest::IssueHumanInviteCreateTicket {
            server_id,
            authority_lineage_id,
            meeting_id,
            room_uid,
            ..
        } => InviteTicketRequest::Create(ManagerRoomAuthorityRequest {
            server_id,
            authority_lineage_id,
            room_id: meeting_id,
            room_uid,
        }),
        LocalControlRequest::IssueHumanInviteRevokeTicket {
            server_id,
            authority_lineage_id,
            meeting_id,
            room_uid,
            ..
        } => InviteTicketRequest::Revoke(ManagerRoomAuthorityRequest {
            server_id,
            authority_lineage_id,
            room_id: meeting_id,
            room_uid,
        }),
        _ => unreachable!("invite ticket helper accepts only invite ticket requests"),
    };
    invite_ticket_control_response(state, request_id, request).await
}

async fn invite_ticket_control_response(
    state: &AppState,
    request_id: String,
    request: InviteTicketRequest,
) -> LocalControlResponse {
    let (create, ticket) = match request {
        InviteTicketRequest::Create(authority) => (
            true,
            issue_human_invite_create_ticket(state, &authority).await,
        ),
        InviteTicketRequest::Revoke(authority) => (
            false,
            issue_human_invite_revoke_ticket(state, &authority).await,
        ),
    };
    match (create, ticket) {
        (true, Ok(ticket)) => LocalControlResponse::HumanInviteCreateOk {
            request_id,
            ticket: ticket.ticket,
            ttl_seconds: ticket.ttl_seconds,
        },
        (false, Ok(ticket)) => LocalControlResponse::HumanInviteRevokeOk {
            request_id,
            ticket: ticket.ticket,
            ttl_seconds: ticket.ttl_seconds,
        },
        (_, Err(error)) => control_error(request_id, error),
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
        | LocalControlRequest::IssueMessagePinsReadTicket { request_id, .. }
        | LocalControlRequest::IssueMessagePinsWriteTicket { request_id, .. }
        | LocalControlRequest::IssueMessageSearchReadTicket { request_id, .. }
        | LocalControlRequest::IssueMessageAttachmentUploadTicket { request_id, .. }
        | LocalControlRequest::IssueMessageAttachmentReadTicket { request_id, .. }
        | LocalControlRequest::IssueHumanInviteCreateTicket { request_id, .. }
        | LocalControlRequest::IssueHumanInviteRevokeTicket { request_id, .. }
        | LocalControlRequest::IssueAppearanceUploadTicket { request_id, .. }
        | LocalControlRequest::IssueAppearancePendingReadTicket { request_id, .. }
        | LocalControlRequest::IssueAppearanceBoundReadTicket { request_id, .. }
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
        TicketIssueError::InvalidRoom(message) | TicketIssueError::InvalidAsset(message) => {
            ("bad_request", message)
        }
        TicketIssueError::RoomMissing => ("room_not_found", "Room does not exist.".to_owned()),
        TicketIssueError::ParticipantInactive => (
            "session_revoked",
            "The local operator is not an active room participant.".to_owned(),
        ),
        TicketIssueError::BootstrapIncomplete => (
            "bootstrap_required",
            "Local identity bootstrap is not complete.".to_owned(),
        ),
        TicketIssueError::AuthorityMismatch => (
            "room_authority_changed",
            "The selected room authority is no longer current.".to_owned(),
        ),
        TicketIssueError::Persistence(PersistenceError::CommandRejected {
            code: code @ ("muted" | "permission_denied" | "session_revoked"),
            message,
        }) => (code, message),
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
