use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, AuthenticatedPrincipal, ClientKind, LOCAL_OPERATOR_PARTICIPANT_ID, Participant,
    ParticipantStatus, RoomEvent, canonical_payload_hash, clean_identifier,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    CommandOutcome, PersistenceError, RoomRuntimeCleanupKey, SqliteStore,
    agent_lifecycle::{load_participant, load_session},
    agent_lifecycle_events::store_result,
    authority::active_room_for_principal,
    command_admission::admit_non_lifecycle_command,
    human_session_authority::fixed_session_fingerprint,
    participant_rows::save_participant_exact,
    room_event_sequence::next_sequence,
    room_runtime_cleanup::request_runtime_cleanup,
    room_turns::support::{insert_event, session_state_event},
    room_write_budget::command_size,
};

#[derive(Debug, Clone)]
pub struct ParticipantRemovalMutation {
    pub outcome: CommandOutcome,
    pub revoked_session_fingerprints: Vec<[u8; 32]>,
    pub cleanup: Option<RoomRuntimeCleanupKey>,
}

impl SqliteStore {
    /// Commits exact participant removal and access revocation before runtime cleanup.
    ///
    /// # Errors
    /// Rejects invalid authority, owner removal, conflicting replay and malformed targets.
    pub async fn execute_participant_removal(
        &self,
        authorization: crate::RoomMutationAuthority<'_>,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<ParticipantRemovalMutation, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let principal = &authorization.resolve(&mut transaction).await?;
        let (status, event_type) = removal_action(action)?;
        let target_id = removal_target(payload)?;
        if principal.client_kind == ClientKind::AgentBridge || !principal.capabilities.room_manage {
            return Err(rejected(
                "permission_denied",
                "Room moderation permission is required.",
            ));
        }
        if target_id == LOCAL_OPERATOR_PARTICIPANT_ID {
            return Err(rejected(
                "owner_removal_denied",
                "The local room owner cannot be removed.",
            ));
        }
        active_room_for_principal(&mut transaction, principal).await?;
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
            return Ok(ParticipantRemovalMutation {
                outcome,
                revoked_session_fingerprints: Vec::new(),
                cleanup: None,
            });
        }
        let mut participant =
            load_participant(&mut transaction, &principal.room_id, &target_id).await?;
        require_removal_transition(participant.status, status)?;
        let mut events = Vec::new();
        let cleanup = if participant.participant_type == "agent"
            && !crate::connector_session::owns_participant(
                &mut transaction,
                &principal.room_id,
                &target_id,
            )
            .await?
        {
            let mut session =
                load_session(&mut transaction, &principal.room_id, &target_id).await?;
            request_runtime_cleanup(&mut transaction, &mut session).await?;
            events.push(session_state_event(&mut transaction, &session).await?);
            Some(RoomRuntimeCleanupKey {
                room_id: principal.room_id.clone(),
                session_id: target_id.clone(),
            })
        } else {
            None
        };
        let revoked_session_fingerprints =
            revoke_participant_access(&mut transaction, &principal.room_id, &target_id).await?;
        participant.status = status;
        participant.updated_at = Utc::now();
        save_participant_exact(
            &mut transaction,
            &principal.room_id,
            &target_id,
            &participant,
        )
        .await?;
        let event = removal_event(&mut transaction, principal, &participant, event_type).await?;
        insert_event(&mut transaction, &event).await?;
        events.push(event.clone());
        let result = json!({
            "participant": participant,
            "revoked_sessions": revoked_session_fingerprints.len(),
            "cleanup_pending": cleanup.is_some(),
            "event": event,
            "event_seq": event.seq,
            "events": events,
        });
        let outcome = store_result(
            &mut transaction,
            principal,
            request_id,
            action,
            payload_hash,
            result,
            events,
        )
        .await?;
        transaction.commit().await?;
        Ok(ParticipantRemovalMutation {
            outcome,
            revoked_session_fingerprints,
            cleanup,
        })
    }
}

// Only fresh commands reach this check; exact replay returns before loading current state.
fn require_removal_transition(
    current: ParticipantStatus,
    requested: ParticipantStatus,
) -> Result<(), PersistenceError> {
    if current == ParticipantStatus::Exported {
        return Err(rejected(
            "participant_exported",
            "This participant was permanently exported from the room.",
        ));
    }
    if current == requested {
        return Err(rejected(
            "participant_already_removed",
            "The participant already has this removal status.",
        ));
    }
    Ok(())
}

fn removal_action(action: &str) -> Result<(ParticipantStatus, &'static str), PersistenceError> {
    match action {
        "participant.kick" => Ok((ParticipantStatus::Kicked, "participant_kicked")),
        "participant.export" => Ok((ParticipantStatus::Exported, "participant_exported")),
        _ => Err(rejected(
            "unknown_action",
            "This participant removal action is unsupported.",
        )),
    }
}

fn removal_target(payload: &Value) -> Result<String, PersistenceError> {
    let Some(fields) = payload.as_object() else {
        return Err(rejected(
            "bad_request",
            "Participant removal requires an object.",
        ));
    };
    let Some(target) = fields.get("participant_id").and_then(Value::as_str) else {
        return Err(rejected("bad_request", "participant_id is required."));
    };
    if fields.len() != 1 || target.is_empty() || clean_identifier(target, 128) != target {
        return Err(rejected(
            "bad_request",
            "Participant removal requires one exact participant_id.",
        ));
    }
    Ok(target.to_owned())
}

pub(crate) async fn revoke_participant_access(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<Vec<[u8; 32]>, PersistenceError> {
    let mut fingerprints = sqlx::query_scalar::<_, Vec<u8>>(
        "UPDATE human_room_sessions SET state = 'ended' WHERE room_id = ? AND participant_id = ? AND state = 'active' RETURNING session_fingerprint",
    ).bind(room_id).bind(participant_id).fetch_all(&mut **transaction).await?;
    fingerprints.extend(sqlx::query_scalar::<_, Vec<u8>>("UPDATE room_connector_invites SET revoked=1 WHERE room_id=? AND participant_id=? AND revoked=0 RETURNING session_fingerprint")
        .bind(room_id).bind(participant_id).fetch_all(&mut **transaction).await?);
    fingerprints.extend(sqlx::query_scalar::<_, Option<Vec<u8>>>("UPDATE room_attendee_invites SET revoked=1 WHERE room_id=? AND participant_id=? AND revoked=0 RETURNING session_fingerprint")
        .bind(room_id).bind(participant_id).fetch_all(&mut **transaction).await?.into_iter().flatten());
    sqlx::query("UPDATE room_invites SET revoked = 1 WHERE room_id = ? AND base_participant_id = ? AND revoked = 0")
        .bind(room_id).bind(participant_id).execute(&mut **transaction).await?;
    Box::pin(crate::provider_request_lifecycle::cancel_participant_in(
        transaction,
        room_id,
        participant_id,
    ))
    .await?;
    fingerprints
        .into_iter()
        .map(fixed_session_fingerprint)
        .collect()
}

async fn removal_event(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
    event_type: &str,
) -> Result<RoomEvent, PersistenceError> {
    Ok(RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: next_sequence(transaction, &principal.room_id).await?,
        created_at: participant.updated_at,
        room_id: principal.room_id.clone(),
        event_type: event_type.to_owned(),
        actor: Actor {
            participant_id: principal.participant_id.clone(),
            participant_type: "human".to_owned(),
        },
        participant_id: Some(participant.participant_id.clone()),
        participant_type: Some(participant.participant_type.clone()),
        actor_id: Some(principal.participant_id.clone()),
        actor_type: Some("human".to_owned()),
        display_name: Some(participant.display_name.clone()),
        content: None,
        message_kind: None,
        extra: BTreeMap::from([("participant".to_owned(), json!(participant))]),
    })
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}
