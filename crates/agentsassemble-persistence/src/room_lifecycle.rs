use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
    LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ParticipantStatus, Room, RoomEvent,
    RoomStatus, canonical_payload_hash,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    CommandOutcome, PersistenceError, RoomRuntimeCleanupKey, SqliteStore,
    agent_lifecycle::load_session,
    agent_lifecycle_events::store_result,
    bootstrap::require_complete_bootstrap_in_transaction,
    command_admission::admit_non_lifecycle_command,
    human_session_authority::fixed_session_fingerprint,
    participant_rows::load_participant_by_key,
    profile_store::load_profile_for_identity,
    room_event_sequence::next_sequence,
    room_runtime_cleanup::request_runtime_cleanup,
    room_turns::support::{insert_event, load_room_with_settings},
    room_write_budget::command_size,
};

#[derive(Debug)]
pub struct RoomLifecycleMutation {
    pub outcome: CommandOutcome,
    pub revoked_session_fingerprints: Vec<[u8; 32]>,
    pub cleanup: Vec<RoomRuntimeCleanupKey>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecyclePayload {
    room_uid: Uuid,
    archived: Option<bool>,
}

pub(crate) fn is_room_lifecycle_action(action: &str) -> bool {
    matches!(action, "room.close" | "room.archive")
}

impl SqliteStore {
    /// Resolves local management authority for active, closed or archived rooms.
    /// Ordinary socket and provider admission continues to require an active room.
    ///
    /// # Errors
    /// Rejects any identity other than the bootstrapped local human owner.
    pub async fn resolve_room_lifecycle_principal(
        &self,
        credential: &AuthenticatedPrincipal,
    ) -> Result<AuthenticatedPrincipal, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let principal = resolve_manager(&mut transaction, credential).await?;
        transaction.commit().await?;
        Ok(principal)
    }

    /// Commits room inactivity and revokes access before requesting external cleanup.
    ///
    /// # Errors
    /// Rejects stale incarnations, unauthorized identities, conflicting requests and
    /// reopening a closed room or an archive whose exact cleanup remains unresolved.
    pub async fn execute_room_lifecycle(
        &self,
        credential: &AuthenticatedPrincipal,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<RoomLifecycleMutation, PersistenceError> {
        let parsed = parse_payload(action, payload)?;
        let mut transaction = self.pool.begin().await?;
        let principal = resolve_manager(&mut transaction, credential).await?;
        let (mut room, _) = load_room_with_settings(&mut transaction, &principal.room_id).await?;
        if room.room_uid != parsed.room_uid {
            return Err(rejected(
                "room_incarnation_changed",
                "The exact room no longer exists.",
            ));
        }
        let payload_hash = canonical_payload_hash(payload);
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            action,
            &payload_hash,
            command_size(request_id, action, payload)?,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(RoomLifecycleMutation {
                outcome,
                revoked_session_fingerprints: Vec::new(),
                cleanup: Vec::new(),
            });
        }
        let status = target_status(action, &parsed, &room)?;
        if status == RoomStatus::Active
            && room_cleanup_pending(&mut transaction, &room.room_id).await?
        {
            return Err(rejected(
                "runtime_cleanup_pending",
                "Exact runtime cleanup must finish before restoring this room.",
            ));
        }
        let mut events = Vec::new();
        let (cleanup, revoked_session_fingerprints) = if status == RoomStatus::Active {
            (Vec::new(), Vec::new())
        } else {
            let cleanup = request_room_cleanup(&mut transaction, &room.room_id).await?;
            let revoked = revoke_room_access(&mut transaction, &room.room_id).await?;
            (cleanup, revoked)
        };
        room.status = status;
        room.updated_at = Utc::now();
        sqlx::query("UPDATE rooms SET room_json = ? WHERE room_id = ?")
            .bind(serde_json::to_string(&room)?)
            .bind(&room.room_id)
            .execute(&mut *transaction)
            .await?;
        let event = lifecycle_event(&mut transaction, &principal, &room).await?;
        insert_event(&mut transaction, &event).await?;
        events.push(event.clone());
        let result = json!({
            "room": room, "cleanup_pending": !cleanup.is_empty(),
            "revoked_sessions": revoked_session_fingerprints.len(),
            "event": event, "event_seq": event.seq, "events": events,
        });
        let outcome = store_result(
            &mut transaction,
            &principal,
            request_id,
            action,
            payload_hash,
            result,
            events,
        )
        .await?;
        transaction.commit().await?;
        Ok(RoomLifecycleMutation {
            outcome,
            revoked_session_fingerprints,
            cleanup,
        })
    }
}

pub(crate) async fn resolve_manager(
    transaction: &mut Transaction<'_, Sqlite>,
    credential: &AuthenticatedPrincipal,
) -> Result<AuthenticatedPrincipal, PersistenceError> {
    let principal = resolve_local_owner(transaction, credential).await?;
    require_manager_membership(transaction, principal).await
}

pub(crate) async fn require_manager_membership(
    transaction: &mut Transaction<'_, Sqlite>,
    mut principal: AuthenticatedPrincipal,
) -> Result<AuthenticatedPrincipal, PersistenceError> {
    let participant =
        load_participant_by_key(transaction, &principal.room_id, &principal.participant_id)
            .await?
            .ok_or(PersistenceError::ParticipantMissing)?;
    if participant.room_id != principal.room_id
        || participant.participant_id != principal.participant_id
        || participant.participant_type != "human"
        || participant.status != ParticipantStatus::Joined
    {
        return Err(rejected(
            "session_revoked",
            "The exact room owner membership is invalid.",
        ));
    }
    principal.display_name = participant.display_name;
    Ok(principal)
}

pub(crate) async fn resolve_local_owner(
    transaction: &mut Transaction<'_, Sqlite>,
    credential: &AuthenticatedPrincipal,
) -> Result<AuthenticatedPrincipal, PersistenceError> {
    if !credential.is_operator
        || credential.client_kind != ClientKind::Browser
        || credential.principal_id != LOCAL_OPERATOR_USER_ID
        || credential.participant_id != LOCAL_OPERATOR_PARTICIPANT_ID
    {
        return Err(rejected(
            "permission_denied",
            "Only the local room owner may manage room lifecycle.",
        ));
    }
    require_complete_bootstrap_in_transaction(transaction).await?;
    let profile = load_profile_for_identity(
        transaction,
        &credential.principal_id,
        &credential.participant_id,
    )
    .await?;
    let mut principal = credential.clone();
    principal.display_name = profile.display_name;
    principal.capabilities =
        CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite);
    Ok(principal)
}

fn parse_payload(action: &str, payload: &Value) -> Result<LifecyclePayload, PersistenceError> {
    let parsed: LifecyclePayload = serde_json::from_value(payload.clone()).map_err(|_| {
        rejected(
            "bad_request",
            "An exact room_uid and action-specific fields are required.",
        )
    })?;
    if !is_room_lifecycle_action(action)
        || (action == "room.archive") != parsed.archived.is_some()
        || (action == "room.close" && payload.get("archived").is_some())
    {
        return Err(rejected(
            "bad_request",
            "Room lifecycle fields do not match this action.",
        ));
    }
    Ok(parsed)
}

fn target_status(
    action: &str,
    payload: &LifecyclePayload,
    room: &Room,
) -> Result<RoomStatus, PersistenceError> {
    if action == "room.close" {
        return Ok(RoomStatus::Closed);
    }
    if room.status == RoomStatus::Closed {
        return Err(rejected(
            "room_closed",
            "A closed room cannot be archived or restored.",
        ));
    }
    Ok(if payload.archived == Some(true) {
        RoomStatus::Archived
    } else {
        RoomStatus::Active
    })
}

pub(crate) async fn room_cleanup_pending(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<bool, PersistenceError> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM room_runtime_cleanup WHERE room_id = ?)")
            .bind(room_id)
            .fetch_one(&mut **transaction)
            .await?,
    )
}

pub(crate) async fn request_room_cleanup(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<Vec<RoomRuntimeCleanupKey>, PersistenceError> {
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT session_id FROM agent_sessions WHERE room_id = ? ORDER BY session_id",
    )
    .bind(room_id)
    .fetch_all(&mut **transaction)
    .await?;
    let mut keys = Vec::with_capacity(ids.len());
    for session_id in ids {
        let mut session = load_session(transaction, room_id, &session_id).await?;
        request_runtime_cleanup(transaction, &mut session).await?;
        keys.push(RoomRuntimeCleanupKey {
            room_id: room_id.to_owned(),
            session_id,
        });
    }
    Ok(keys)
}

pub(crate) async fn revoke_room_access(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<Vec<[u8; 32]>, PersistenceError> {
    let fingerprints = sqlx::query_scalar::<_, Vec<u8>>("UPDATE human_room_sessions SET state = 'ended' WHERE room_id = ? AND state = 'active' RETURNING session_fingerprint")
        .bind(room_id).fetch_all(&mut **transaction).await?;
    sqlx::query("UPDATE room_invites SET revoked = 1 WHERE room_id = ? AND revoked = 0")
        .bind(room_id)
        .execute(&mut **transaction)
        .await?;
    fingerprints
        .into_iter()
        .map(fixed_session_fingerprint)
        .collect()
}

pub(crate) async fn lifecycle_event(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    room: &Room,
) -> Result<RoomEvent, PersistenceError> {
    Ok(RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: next_sequence(transaction, &room.room_id).await?,
        created_at: room.updated_at,
        room_id: room.room_id.clone(),
        event_type: match room.status {
            RoomStatus::Active => "room_restored",
            RoomStatus::Archived => "room_archived",
            RoomStatus::Closed => "room_closed",
        }
        .to_owned(),
        actor: Actor {
            participant_id: principal.participant_id.clone(),
            participant_type: "human".to_owned(),
        },
        participant_id: None,
        participant_type: None,
        actor_id: Some(principal.participant_id.clone()),
        actor_type: Some("human".to_owned()),
        display_name: None,
        content: None,
        message_kind: None,
        extra: BTreeMap::from([("room".to_owned(), json!(room))]),
    })
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}
