use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use agentsassemble_persistence::{
    LocalBootstrapPhase as PersistenceBootstrapPhase, LocalBootstrapStatus, PersistenceError,
};
use agentsassemble_protocol::{
    LocalBootstrapGrant, LocalBootstrapPhase, LocalControlRequest, LocalControlResponse,
    ServerProductSurface,
};
use agentsassemble_server::{
    AppState, ManagerRoomAuthorityRequest, StableEntryConfig, TicketIssueError,
    issue_attendee_invite_create_ticket, issue_central_registration_ticket,
    issue_connector_invite_create_ticket, issue_human_invite_create_ticket,
    issue_human_invite_revoke_ticket, issue_local_operator_http_ticket, issue_local_ticket,
    issue_preferences_read_ticket, issue_preferences_write_ticket,
    issue_settings_directory_read_ticket,
};
use anyhow::Context;
use clap::Parser;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

mod agent_avatar_control;
mod appearance_control;
mod chat_read_control;
#[cfg(unix)]
mod control_input;
mod message_attachments_control;
mod message_pins_control;
mod runtime_ready;
#[cfg(unix)]
mod runtime_reexec;
mod runtime_startup;

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
    #[arg(long = agentsassemble_server::runtime_image::PREFLIGHT_ARGUMENT, hide = true)]
    runtime_preflight: bool,
    #[arg(long, default_value = ".agentsassemble-rust/runtime.sqlite3")]
    database: PathBuf,
    #[arg(long)]
    frontend: Option<PathBuf>,
    #[arg(long, hide = true)]
    frontend_build: Option<String>,
    #[arg(long, hide = true)]
    restart_source: Option<PathBuf>,
    #[arg(long, hide = true)]
    reexec_operation: Option<String>,
    #[arg(long, hide = true)]
    reexec_image: Option<String>,
    #[arg(long, hide = true)]
    reexec_control: Option<String>,
    #[arg(long)]
    desktop_native_registration: bool,
    #[arg(long)]
    stable_entry_config: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    run_internal_provider_mode();
    #[cfg(unix)]
    let inherited = runtime_reexec::InheritedListeners::take()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    if let Some(code) = runtime.block_on(agentsassemble_provider::run_managed_bridge_if_requested())
    {
        std::process::exit(code);
    }
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();
    if args.runtime_preflight {
        println!(
            "{}",
            serde_json::to_string(
                &agentsassemble_server::runtime_image::RuntimePreflight::current()
            )?
        );
        return Ok(());
    }
    let exit = runtime.block_on(runtime_startup::run(
        args,
        #[cfg(unix)]
        inherited,
    ))?;
    drop(runtime);
    #[cfg(unix)]
    if let Some(restart) = exit.restart {
        return restart.execute();
    }
    #[cfg(not(unix))]
    let _ = exit;
    Ok(())
}

fn run_internal_provider_mode() {
    #[cfg(unix)]
    if let Some(code) = agentsassemble_provider::run_process_helper_if_requested() {
        std::process::exit(code);
    }
}

async fn run_control_pipe<R, W>(
    reader: &mut R,
    writer: &mut W,
    state: AppState,
    cancellation: &CancellationToken,
) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    loop {
        let Some(line) = read_control_line(reader, cancellation).await? else {
            return Ok(());
        };
        let response = control_response(&state, &line).await;
        write_json_line(writer, &response).await?;
    }
}

async fn provider_discovery_control_response(
    state: &AppState,
    request_id: String,
    provider_id: String,
    force: bool,
) -> LocalControlResponse {
    if state
        .store
        .require_local_bootstrap_complete()
        .await
        .is_err()
    {
        return LocalControlResponse::Error {
            request_id,
            code: "bootstrap_required".to_owned(),
            message: "Local bootstrap is required.".to_owned(),
        };
    }
    match state
        .provider_catalog
        .request_discovery(&provider_id, force)
    {
        Ok(generation) => LocalControlResponse::ProviderDiscoveryOk {
            request_id,
            provider_id,
            generation,
        },
        Err(error) => LocalControlResponse::Error {
            request_id,
            code: "provider_discovery_unavailable".to_owned(),
            message: error.to_string(),
        },
    }
}

async fn control_response(state: &AppState, line: &[u8]) -> LocalControlResponse {
    let (request_id, request) = match parse_control_request(line) {
        Ok(request) => request,
        Err(error) => return *error,
    };
    match request {
        LocalControlRequest::DiscoverLocalProvider {
            provider_id, force, ..
        } => provider_discovery_control_response(state, request_id, provider_id, force).await,
        LocalControlRequest::CentralLogin {
            action,
            state: login_state,
            ..
        } => {
            agentsassemble_server::central_login_control(state, request_id, action, &login_state)
                .await
        }
        LocalControlRequest::InspectBootstrap { .. } => {
            bootstrap_control_response(state, request_id, None).await
        }
        LocalControlRequest::InitializeBootstrap { display_name, .. } => {
            bootstrap_control_response(state, request_id, Some(&display_name)).await
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
        request @ (LocalControlRequest::IssueSideChatReadTicket { .. }
        | LocalControlRequest::IssueMessageSearchReadTicket { .. }) => {
            chat_read_control::response(state, request_id, request).await
        }
        request @ (LocalControlRequest::IssueMessageAttachmentUploadTicket { .. }
        | LocalControlRequest::IssueMessageAttachmentReadTicket { .. }) => {
            message_attachments_control::response(state, request_id, request).await
        }
        request @ (LocalControlRequest::IssueAttendeeInviteCreateTicket { .. }
        | LocalControlRequest::IssueConnectorInviteCreateTicket { .. }
        | LocalControlRequest::IssueHumanInviteCreateTicket { .. }
        | LocalControlRequest::IssueHumanInviteRevokeTicket { .. }) => {
            invite_ticket_control_request(state, request_id, request).await
        }
        request @ LocalControlRequest::IssueAgentAvatarUploadTicket { .. } => {
            agent_avatar_control::response(state, request_id, request).await
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
) -> Result<(String, LocalControlRequest), Box<LocalControlResponse>> {
    let request = serde_json::from_slice::<LocalControlRequest>(line).map_err(|_| {
        Box::new(LocalControlResponse::Error {
            request_id: String::new(),
            code: "control_request_invalid".to_owned(),
            message: "Control request JSON is invalid.".to_owned(),
        })
    })?;
    let request_id = control_request_id(&request).to_owned();
    if valid_control_request_id(&request_id) {
        Ok((request_id, request))
    } else {
        Err(Box::new(LocalControlResponse::Error {
            request_id,
            code: "request_id_invalid".to_owned(),
            message: "Control request id is invalid.".to_owned(),
        }))
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

async fn bootstrap_control_response(
    state: &AppState,
    request_id: String,
    display_name: Option<&str>,
) -> LocalControlResponse {
    let result = match display_name {
        Some(name) => state
            .store
            .bootstrap_local_authority(&request_id, name)
            .await
            .map(|commit| (commit.status, commit.deduplicated)),
        None => state
            .store
            .local_bootstrap_status()
            .await
            .map(|status| (status, false)),
    };
    match result {
        Ok((status, deduplicated)) => LocalControlResponse::BootstrapOk {
            request_id,
            bootstrap: Box::new(bootstrap_grant(
                status,
                deduplicated,
                &state.server_product_surface,
            )),
        },
        Err(error) => bootstrap_control_error(request_id, error),
    }
}

enum InviteTicketRequest {
    ConnectorCreate(ManagerRoomAuthorityRequest),
    AttendeeCreate(ManagerRoomAuthorityRequest),
    Create(ManagerRoomAuthorityRequest),
    Revoke(ManagerRoomAuthorityRequest),
}

async fn invite_ticket_control_request(
    state: &AppState,
    request_id: String,
    request: LocalControlRequest,
) -> LocalControlResponse {
    let request = match request {
        LocalControlRequest::IssueAttendeeInviteCreateTicket {
            server_id,
            authority_lineage_id,
            meeting_id,
            room_uid,
            ..
        } => InviteTicketRequest::AttendeeCreate(ManagerRoomAuthorityRequest {
            server_id,
            authority_lineage_id,
            room_id: meeting_id,
            room_uid,
        }),
        LocalControlRequest::IssueConnectorInviteCreateTicket {
            server_id,
            authority_lineage_id,
            meeting_id,
            room_uid,
            ..
        } => InviteTicketRequest::ConnectorCreate(ManagerRoomAuthorityRequest {
            server_id,
            authority_lineage_id,
            room_id: meeting_id,
            room_uid,
        }),
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
    let result = match request {
        InviteTicketRequest::AttendeeCreate(authority) => {
            issue_attendee_invite_create_ticket(state, &authority)
                .await
                .map(|ticket| LocalControlResponse::AttendeeInviteCreateOk {
                    request_id: request_id.clone(),
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
                })
        }
        InviteTicketRequest::ConnectorCreate(authority) => {
            issue_connector_invite_create_ticket(state, &authority)
                .await
                .map(|ticket| LocalControlResponse::ConnectorInviteCreateOk {
                    request_id: request_id.clone(),
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
                })
        }
        InviteTicketRequest::Create(authority) => {
            issue_human_invite_create_ticket(state, &authority)
                .await
                .map(|ticket| LocalControlResponse::HumanInviteCreateOk {
                    request_id: request_id.clone(),
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
                })
        }
        InviteTicketRequest::Revoke(authority) => {
            issue_human_invite_revoke_ticket(state, &authority)
                .await
                .map(|ticket| LocalControlResponse::HumanInviteRevokeOk {
                    request_id: request_id.clone(),
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
                })
        }
    };
    result.unwrap_or_else(|error| control_error(request_id, error))
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
        LocalControlRequest::CentralLogin { request_id, .. }
        | LocalControlRequest::InspectBootstrap { request_id }
        | LocalControlRequest::InitializeBootstrap { request_id, .. }
        | LocalControlRequest::IssueTicket { request_id, .. }
        | LocalControlRequest::IssueOperatorHttpTicket { request_id }
        | LocalControlRequest::DiscoverLocalProvider { request_id, .. }
        | LocalControlRequest::IssuePreferencesReadTicket { request_id, .. }
        | LocalControlRequest::IssuePreferencesWriteTicket { request_id, .. }
        | LocalControlRequest::IssueMessagePinsReadTicket { request_id, .. }
        | LocalControlRequest::IssueMessagePinsWriteTicket { request_id, .. }
        | LocalControlRequest::IssueMessageSearchReadTicket { request_id, .. }
        | LocalControlRequest::IssueSideChatReadTicket { request_id, .. }
        | LocalControlRequest::IssueMessageAttachmentUploadTicket { request_id, .. }
        | LocalControlRequest::IssueMessageAttachmentReadTicket { request_id, .. }
        | LocalControlRequest::IssueAttendeeInviteCreateTicket { request_id, .. }
        | LocalControlRequest::IssueConnectorInviteCreateTicket { request_id, .. }
        | LocalControlRequest::IssueHumanInviteCreateTicket { request_id, .. }
        | LocalControlRequest::IssueHumanInviteRevokeTicket { request_id, .. }
        | LocalControlRequest::IssueAgentAvatarUploadTicket { request_id, .. }
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
        PersistenceError::CommandRejected { code, message } => (code.into_owned(), message),
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
    let (code, message): (std::borrow::Cow<'static, str>, String) = match error {
        TicketIssueError::InvalidRoom(message) | TicketIssueError::InvalidAsset(message) => {
            ("bad_request".into(), message)
        }
        TicketIssueError::RoomMissing => {
            ("room_not_found".into(), "Room does not exist.".to_owned())
        }
        TicketIssueError::ParticipantInactive => (
            "session_revoked".into(),
            "The local operator is not an active room participant.".to_owned(),
        ),
        TicketIssueError::BootstrapIncomplete => (
            "bootstrap_required".into(),
            "Local identity bootstrap is not complete.".to_owned(),
        ),
        TicketIssueError::AuthorityMismatch => (
            "room_authority_changed".into(),
            "The selected room authority is no longer current.".to_owned(),
        ),
        TicketIssueError::Persistence(PersistenceError::CommandRejected { code, message })
            if matches!(
                code.as_bytes(),
                b"muted" | b"permission_denied" | b"session_revoked"
            ) =>
        {
            (code, message)
        }
        TicketIssueError::Persistence(_) => (
            "persistence_failed".into(),
            "Persistence operation failed.".to_owned(),
        ),
        TicketIssueError::Unavailable => (
            "unavailable".into(),
            "Ticket capacity is unavailable.".to_owned(),
        ),
    };
    LocalControlResponse::Error {
        request_id,
        code: code.into_owned(),
        message,
    }
}

async fn read_control_line<R: AsyncRead + Unpin>(
    reader: &mut R,
    cancellation: &CancellationToken,
) -> anyhow::Result<Option<Vec<u8>>> {
    let mut byte = [0_u8; 1];
    let count = tokio::select! {
        biased;
        () = cancellation.cancelled() => return Ok(None),
        count = reader.read(&mut byte) => count.context("read parent control pipe")?,
    };
    if count == 0 {
        return Ok(None);
    }
    // Once the first byte is consumed, complete this frame before allowing handoff.
    // A stalled parent gets a bounded error, never a silently discarded request prefix.
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut line = Vec::with_capacity(256);
        for _ in 0..=MAX_CONTROL_MESSAGE_BYTES {
            if byte[0] == b'\n' {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(line));
            }
            line.push(byte[0]);
            if line.len() > MAX_CONTROL_MESSAGE_BYTES {
                anyhow::bail!("control request exceeds {MAX_CONTROL_MESSAGE_BYTES} bytes");
            }
            if reader
                .read(&mut byte)
                .await
                .context("read parent control frame")?
                == 0
            {
                anyhow::bail!("control pipe closed during a request");
            }
        }
        anyhow::bail!("control request exceeds {MAX_CONTROL_MESSAGE_BYTES} bytes")
    })
    .await
    .context("parent control frame exceeded its deadline")?
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

#[cfg(test)]
mod control_frame_tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn cancellation_finishes_started_frame_and_preserves_the_next_frame() -> anyhow::Result<()>
    {
        let (mut writer, mut reader) = tokio::io::duplex(128);
        let cancellation = CancellationToken::new();
        writer.write_all(b"first").await?;
        {
            let frame = read_control_line(&mut reader, &cancellation);
            tokio::pin!(frame);
            assert!(futures_util::poll!(&mut frame).is_pending());
            cancellation.cancel();
            assert!(futures_util::poll!(&mut frame).is_pending());
            writer.write_all(b"\nnext\n").await?;
            assert_eq!(frame.await?, Some(b"first".to_vec()));
        }
        assert!(
            read_control_line(&mut reader, &cancellation)
                .await?
                .is_none()
        );
        let next = CancellationToken::new();
        assert_eq!(
            read_control_line(&mut reader, &next).await?,
            Some(b"next".to_vec())
        );
        writer.write_all(b"partial").await?;
        let partial = read_control_line(&mut reader, &next);
        tokio::pin!(partial);
        assert!(futures_util::poll!(&mut partial).is_pending());
        tokio::time::advance(Duration::from_secs(10)).await;
        assert!(partial.await.is_err());
        Ok(())
    }
}
