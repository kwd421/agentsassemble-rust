use agentsassemble_domain::{AuthenticatedPrincipal, ClientKind, DurableAgentSession, InviteScope};
use chrono::{DateTime, Utc};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};
use uuid::Uuid;

use crate::{PersistenceError, authority::load_active_membership, provider_requests::rejected};

pub(crate) async fn authorize_resolution_in(
    tx: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    row: &SqliteRow,
    now: DateTime<Utc>,
) -> Result<DurableAgentSession, PersistenceError> {
    if principal.client_kind != ClientKind::Browser
        || principal.invite_scope != InviteScope::ReadWrite
        || !principal.capabilities.message_send
        || principal.participant_id != row.get::<&str, _>("owner_id")
    {
        return Err(rejected(
            "permission_denied",
            "Only the current human owner may answer.",
        ));
    }
    let session =
        crate::agent_lifecycle::load_session(tx, &principal.room_id, row.get("session_id")).await?;
    require_pending_authority_in(tx, &session, row, now).await?;
    Ok(session)
}

pub(crate) async fn require_pending_authority_in(
    tx: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    row: &SqliteRow,
    now: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    let (_, agent) =
        load_active_membership(tx, &session.public.room_id, &session.public.participant_id).await?;
    let (_, owner) = load_active_membership(tx, &session.public.room_id, &agent.owner_id).await?;
    if owner.participant_type != "human"
        || owner.muted
        || agent.muted
        || agent.owner_id != row.get::<&str, _>("owner_id")
    {
        return Err(rejected(
            "permission_denied",
            "Provider request owner authority has ended.",
        ));
    }
    require_execution_in(tx, session, row, now).await
}

pub(crate) async fn require_execution_in(
    tx: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    row: &SqliteRow,
    now: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    let generation = u64::try_from(row.get::<i64, _>("turn_generation"))
        .map_err(|_| rejected("invalid_state", "Stored request generation is invalid."))?;
    let execution = row.get("execution_id");
    if session.public.external_owned {
        let connection_id = row
            .get::<Option<&str>, _>("connection_id")
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| rejected("invalid_state", "Stored request connection is missing."))?;
        let fingerprint: Vec<u8> = sqlx::query_scalar("SELECT session_fingerprint FROM room_attendee_invites WHERE room_id=? AND participant_id=?")
            .bind(&session.public.room_id).bind(&session.public.participant_id)
            .fetch_optional(&mut **tx).await?
            .ok_or_else(|| rejected("session_revoked", "Attendee admission is unavailable."))?;
        let fingerprint = fingerprint
            .try_into()
            .map_err(|_| rejected("invalid_state", "Stored attendee identity is invalid."))?;
        crate::attendee_connection::authorize_current_in(tx, &fingerprint, connection_id, now)
            .await?;
        crate::turn_authority::require_room_tool_turn_authority(
            tx,
            session,
            &session.public.active_turn_id,
            session.input_up_to_seq,
            generation,
            execution,
        )
        .await
    } else {
        crate::turn_authority::require_provider_room_tool_authority(
            tx,
            session,
            &session.public.active_turn_id,
            session.input_up_to_seq,
            generation,
            execution,
        )
        .await
    }
}
