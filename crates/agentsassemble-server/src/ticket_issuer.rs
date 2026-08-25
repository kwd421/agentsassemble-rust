use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, validate_room_id,
};
use agentsassemble_persistence::PersistenceError;
use agentsassemble_protocol::{OperatorHttpTicketResponse, TicketResponse};
use thiserror::Error;

use crate::AppState;

#[derive(Debug, Error)]
pub enum TicketIssueError {
    #[error("{0}")]
    InvalidRoom(String),
    #[error("room does not exist")]
    RoomMissing,
    #[error("local operator is not an active room participant")]
    ParticipantInactive,
    #[error("local bootstrap is not complete")]
    BootstrapIncomplete,
    #[error("persistence operation failed")]
    Persistence(#[source] PersistenceError),
    #[error("ticket capacity is unavailable")]
    Unavailable,
}

/// Issues a one-use browser ticket for the active local room operator.
///
/// # Errors
///
/// Returns a bounded validation, membership, persistence, or capacity error.
pub async fn issue_local_ticket(
    state: &AppState,
    requested_room_id: &str,
) -> Result<TicketResponse, TicketIssueError> {
    state
        .store
        .require_local_bootstrap_complete()
        .await
        .map_err(map_bootstrap_error)?;
    let room_id = validate_room_id(requested_room_id)
        .map_err(|error| TicketIssueError::InvalidRoom(error.message))?;
    let participant = state
        .store
        .active_participant(&room_id, LOCAL_OPERATOR_PARTICIPANT_ID)
        .await
        .map_err(|error| match error {
            PersistenceError::RoomMissing => TicketIssueError::RoomMissing,
            PersistenceError::ParticipantMissing
            | PersistenceError::CommandRejected {
                code: "session_revoked" | "room_inactive",
                ..
            } => TicketIssueError::ParticipantInactive,
            error => TicketIssueError::Persistence(error),
        })?;
    let client_kind = ClientKind::Browser;
    let invite_scope = InviteScope::ReadWrite;
    let issued = state
        .tickets
        .issue(AuthenticatedPrincipal {
            principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
            participant_id: participant.participant_id,
            display_name: participant.display_name,
            room_id,
            client_kind,
            invite_scope,
            is_operator: true,
            capabilities: CapabilitySet::for_principal(client_kind, invite_scope, true),
        })
        .await
        .map_err(|_| TicketIssueError::Unavailable)?;
    Ok(TicketResponse {
        ticket: issued.ticket,
        ttl_seconds: state.tickets.ttl_seconds(),
        server_proof_key: issued.proof_key,
    })
}

/// Issues a private-control-derived one-use credential for server-wide operator HTTP routes.
///
/// # Errors
///
/// Returns a bounded ticket-capacity error. The caller must already own the private control pipe.
pub async fn issue_local_operator_http_ticket(
    state: &AppState,
) -> Result<OperatorHttpTicketResponse, TicketIssueError> {
    state
        .store
        .require_local_bootstrap_complete()
        .await
        .map_err(map_bootstrap_error)?;
    let issued = state
        .tickets
        .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
        .await
        .map_err(|_| TicketIssueError::Unavailable)?;
    Ok(OperatorHttpTicketResponse {
        ticket: issued.ticket,
        ttl_seconds: state.tickets.ttl_seconds(),
    })
}

/// Issues a private-control-derived credential for the exact central-registration route.
///
/// # Errors
///
/// Returns a bootstrap, persistence, or bounded ticket-capacity error.
pub async fn issue_central_registration_ticket(
    state: &AppState,
) -> Result<OperatorHttpTicketResponse, TicketIssueError> {
    state
        .store
        .require_local_bootstrap_complete()
        .await
        .map_err(map_bootstrap_error)?;
    let issued = state
        .tickets
        .issue_central_registration(LOCAL_OPERATOR_USER_ID.to_owned())
        .await
        .map_err(|_| TicketIssueError::Unavailable)?;
    Ok(OperatorHttpTicketResponse {
        ticket: issued.ticket,
        ttl_seconds: state.tickets.ttl_seconds(),
    })
}

fn map_bootstrap_error(error: PersistenceError) -> TicketIssueError {
    match error {
        PersistenceError::CommandRejected {
            code: "bootstrap_required" | "bootstrap_repair_required",
            ..
        } => TicketIssueError::BootstrapIncomplete,
        error => TicketIssueError::Persistence(error),
    }
}
