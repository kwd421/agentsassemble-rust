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

/// Issues an exact preference-read credential for the canonical local room human.
///
/// # Errors
///
/// Returns a bounded room, identity, persistence, or ticket-capacity error.
pub async fn issue_preferences_read_ticket(
    state: &AppState,
    requested_room_id: &str,
) -> Result<OperatorHttpTicketResponse, TicketIssueError> {
    let identity = resolve_local_room_user(state, requested_room_id).await?;
    let issued = state
        .tickets
        .issue_preferences_read(identity.room_id, identity.user_id, identity.participant_id)
        .await
        .map_err(|_| TicketIssueError::Unavailable)?;
    Ok(operator_http_response(state, issued))
}

/// Issues an exact preference-write credential for the canonical local room human.
///
/// # Errors
///
/// Returns a bounded room, identity, persistence, or ticket-capacity error.
pub async fn issue_preferences_write_ticket(
    state: &AppState,
    requested_room_id: &str,
) -> Result<OperatorHttpTicketResponse, TicketIssueError> {
    let identity = resolve_local_room_user(state, requested_room_id).await?;
    let issued = state
        .tickets
        .issue_preferences_write(identity.room_id, identity.user_id, identity.participant_id)
        .await
        .map_err(|_| TicketIssueError::Unavailable)?;
    Ok(operator_http_response(state, issued))
}

/// Issues an exact invite-create credential for the current local room manager.
///
/// # Errors
///
/// Returns a bounded room, manager, persistence, or ticket-capacity error.
pub async fn issue_human_invite_create_ticket(
    state: &AppState,
    requested_room_id: &str,
) -> Result<OperatorHttpTicketResponse, TicketIssueError> {
    let identity = resolve_local_room_manager(state, requested_room_id).await?;
    let issued = state
        .tickets
        .issue_human_invite_create(identity.room_id, identity.user_id, identity.participant_id)
        .await
        .map_err(|_| TicketIssueError::Unavailable)?;
    Ok(operator_http_response(state, issued))
}

/// Issues an exact invite-revoke credential for the current local room manager.
///
/// # Errors
///
/// Returns a bounded room, manager, persistence, or ticket-capacity error.
pub async fn issue_human_invite_revoke_ticket(
    state: &AppState,
    requested_room_id: &str,
) -> Result<OperatorHttpTicketResponse, TicketIssueError> {
    let identity = resolve_local_room_manager(state, requested_room_id).await?;
    let issued = state
        .tickets
        .issue_human_invite_revoke(identity.room_id, identity.user_id, identity.participant_id)
        .await
        .map_err(|_| TicketIssueError::Unavailable)?;
    Ok(operator_http_response(state, issued))
}

/// Issues the server-wide settings-directory read credential for the local operator.
///
/// # Errors
///
/// Returns a bootstrap, persistence, or bounded ticket-capacity error.
pub async fn issue_settings_directory_read_ticket(
    state: &AppState,
) -> Result<OperatorHttpTicketResponse, TicketIssueError> {
    state
        .store
        .require_local_bootstrap_complete()
        .await
        .map_err(map_bootstrap_error)?;
    let issued = state
        .tickets
        .issue_settings_directory_read(LOCAL_OPERATOR_USER_ID.to_owned())
        .await
        .map_err(|_| TicketIssueError::Unavailable)?;
    Ok(operator_http_response(state, issued))
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

async fn resolve_local_room_user(
    state: &AppState,
    requested_room_id: &str,
) -> Result<agentsassemble_persistence::RoomUserIdentity, TicketIssueError> {
    let room_id = validate_room_id(requested_room_id)
        .map_err(|error| TicketIssueError::InvalidRoom(error.message))?;
    state
        .store
        .authorize_room_user(
            &room_id,
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .map_err(map_room_identity_error)
}

async fn resolve_local_room_manager(
    state: &AppState,
    requested_room_id: &str,
) -> Result<agentsassemble_persistence::RoomUserIdentity, TicketIssueError> {
    let room_id = validate_room_id(requested_room_id)
        .map_err(|error| TicketIssueError::InvalidRoom(error.message))?;
    state
        .store
        .authorize_local_room_manager(
            &room_id,
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .map_err(map_room_identity_error)
}

fn operator_http_response(
    state: &AppState,
    issued: crate::IssuedTicket,
) -> OperatorHttpTicketResponse {
    OperatorHttpTicketResponse {
        ticket: issued.ticket,
        ttl_seconds: state.tickets.ttl_seconds(),
    }
}

fn map_room_identity_error(error: PersistenceError) -> TicketIssueError {
    match error {
        PersistenceError::RoomMissing => TicketIssueError::RoomMissing,
        PersistenceError::ParticipantMissing
        | PersistenceError::CommandRejected {
            code:
                "session_revoked"
                | "room_inactive"
                | "user_profile_missing"
                | "profile_authority_mismatch",
            ..
        } => TicketIssueError::ParticipantInactive,
        error => map_bootstrap_error(error),
    }
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

#[cfg(test)]
mod tests {
    use agentsassemble_persistence::PersistenceError;

    use super::{TicketIssueError, map_room_identity_error};

    #[test]
    fn room_identity_mapper_preserves_bootstrap_rejections() {
        for code in ["bootstrap_required", "bootstrap_repair_required"] {
            let error = PersistenceError::CommandRejected {
                code,
                message: "bootstrap authority is unavailable".to_owned(),
            };
            assert!(matches!(
                map_room_identity_error(error),
                TicketIssueError::BootstrapIncomplete
            ));
        }
    }
}
