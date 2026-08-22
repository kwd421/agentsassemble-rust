use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, ParticipantStatus, validate_room_id,
};
use agentsassemble_persistence::PersistenceError;
use agentsassemble_protocol::TicketResponse;
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
    let room_id = validate_room_id(requested_room_id)
        .map_err(|error| TicketIssueError::InvalidRoom(error.message))?;
    if !state
        .store
        .room_exists(&room_id)
        .await
        .map_err(TicketIssueError::Persistence)?
    {
        return Err(TicketIssueError::RoomMissing);
    }
    let participant = state
        .store
        .participant(&room_id, LOCAL_OPERATOR_PARTICIPANT_ID)
        .await
        .map_err(TicketIssueError::Persistence)?;
    if participant.status != ParticipantStatus::Joined {
        return Err(TicketIssueError::ParticipantInactive);
    }
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
            capabilities: CapabilitySet::local_operator(client_kind, invite_scope),
        })
        .await
        .map_err(|_| TicketIssueError::Unavailable)?;
    Ok(TicketResponse {
        ticket: issued.ticket,
        ttl_seconds: state.tickets.ttl_seconds(),
        server_proof_key: issued.proof_key,
    })
}
