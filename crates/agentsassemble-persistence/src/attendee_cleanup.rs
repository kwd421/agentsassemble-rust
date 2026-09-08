use agentsassemble_domain::{AgentRuntimeStatus, DurableAgentSession, canonical_payload_hash};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    AgentTurnCommit, CommandOutcome, PersistenceError, RoomCommandMutation, RoomRuntimeCleanupKey,
    SqliteStore,
    agent_lifecycle::{load_session, save_session},
    attendee_invites::{parse_uuid, rejected},
    command_admission::{admit_non_lifecycle_command, store_command_result},
    room_runtime_cleanup::{cleanup_exists, finish_cleanup_in},
};

/// Revocation ends room access, but cannot prevent the external owner reporting exact cleanup.
/// This sealed authority grants no room read, command, ready or turn-result operation.
#[derive(Clone)]
pub struct AttendeeCleanupAuthorization {
    fingerprint: [u8; 32],
    room_uid: Uuid,
    key: RoomRuntimeCleanupKey,
}

impl AttendeeCleanupAuthorization {
    #[must_use]
    pub fn room_id(&self) -> &str {
        &self.key.room_id
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttendeeCleanupDelivery {
    pub cleanup_id: Uuid,
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub runtime_lease_token: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttendeeCleanupReport {
    pub request_id: Uuid,
    pub stopped: AttendeeCleanupDelivery,
}

pub(crate) async fn request_in(
    tx: &mut Transaction<'_, Sqlite>,
    session: &mut DurableAgentSession,
) -> Result<(), PersistenceError> {
    sqlx::query("INSERT INTO attendee_cleanup_controls(room_id,session_id,cleanup_id) VALUES(?,?,?) ON CONFLICT(room_id,session_id) DO UPDATE SET cleanup_id=excluded.cleanup_id")
        .bind(&session.public.room_id).bind(&session.public.session_id).bind(Uuid::new_v4().to_string()).execute(&mut **tx).await?;
    // No ready report ever transferred a runtime identity to this admitted session.
    if session.runtime_handle_id.is_empty()
        && session.runtime_owner_id.is_empty()
        && session.runtime_lease_token.is_empty()
        && !session.public.provider_session_active
    {
        session.public.runtime_status = AgentRuntimeStatus::Stopped;
    }
    Ok(())
}

impl SqliteStore {
    /// Authenticates original attendee custody for cleanup only, including after expiry/removal.
    ///
    /// # Errors
    /// Rejects unknown credentials, replaced room incarnations and changed provider custody.
    pub async fn authorize_attendee_cleanup(
        &self,
        fingerprint: &[u8; 32],
    ) -> Result<AttendeeCleanupAuthorization, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let authority = authorize_in(&mut tx, fingerprint).await?;
        tx.commit().await?;
        Ok(authority)
    }

    /// Reads the pending exact external stop request without disclosing room context.
    ///
    /// # Errors
    /// Rejects changed custody and corrupt cleanup state.
    pub async fn load_attendee_cleanup(
        &self,
        authority: &AttendeeCleanupAuthorization,
    ) -> Result<Option<AttendeeCleanupDelivery>, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        revalidate_in(&mut tx, authority).await?;
        let delivery = load_delivery(&mut tx, &authority.key).await?;
        tx.commit().await?;
        Ok(delivery)
    }

    /// Atomically commits external runtime absence, canonical turn/removal completion and retry receipt.
    ///
    /// # Errors
    /// Rejects stale stop identity, changed retries and unresolved canonical cleanup.
    pub async fn record_attendee_cleanup(
        &self,
        authority: &AttendeeCleanupAuthorization,
        report: &AttendeeCleanupReport,
    ) -> Result<RoomCommandMutation, PersistenceError> {
        if report.request_id.is_nil() {
            return Err(rejected(
                "bad_request",
                "A cleanup report UUID is required.",
            ));
        }
        let payload = serde_json::to_value(report)?;
        let hash = canonical_payload_hash(&payload);
        let request = report.request_id.to_string();
        let action = "bridge.cleanup.report";
        let key = &authority.key;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        revalidate_in(&mut tx, authority).await?;
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut tx,
            &key.room_id,
            &key.session_id,
            &request,
            action,
            &hash,
            crate::room_write_budget::command_size(&request, action, &payload)?,
        )
        .await?
        {
            tx.commit().await?;
            return Ok(RoomCommandMutation {
                outcome,
                assignments: Vec::new(),
            });
        }
        let expected = load_delivery(&mut tx, key)
            .await?
            .ok_or_else(stale_cleanup)?;
        if report.stopped != expected {
            return Err(stale_cleanup());
        }
        let mut commit = Box::pin(checkpoint_absence(&mut tx, key)).await?;
        let finished = finish_cleanup_in(&mut tx, key)
            .await?
            .ok_or_else(stale_cleanup)?;
        commit.events.extend(finished.events);
        commit.next_assignments.extend(finished.next_assignments);
        let event = commit.events.last().cloned().ok_or_else(stale_cleanup)?;
        let result = json!({"event":event, "events":commit.events});
        store_command_result(
            &mut tx,
            (&key.room_id, &key.session_id),
            &request,
            action,
            &hash,
            &result,
        )
        .await?;
        tx.commit().await?;
        Ok(RoomCommandMutation {
            outcome: CommandOutcome {
                result,
                event,
                events: commit.events,
                deduplicated: false,
            },
            assignments: commit.next_assignments,
        })
    }
}

async fn authorize_in(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
) -> Result<AttendeeCleanupAuthorization, PersistenceError> {
    let row = sqlx::query("SELECT room_id,room_uid,participant_id,provider_kind FROM room_attendee_invites WHERE session_fingerprint=?")
        .bind(fingerprint.as_slice()).fetch_optional(&mut **tx).await?.ok_or_else(stale_cleanup)?;
    let key = RoomRuntimeCleanupKey {
        room_id: row.get("room_id"),
        session_id: row.get("participant_id"),
    };
    let session = load_session(tx, &key.room_id, &key.session_id).await?;
    let room_uid = parse_uuid(row.get("room_uid"))?;
    let (room, _) = crate::room_turns::support::load_room_with_settings(tx, &key.room_id).await?;
    if room_uid != room.room_uid
        || !session.public.external_owned
        || session.public.process_ownership != "external"
        || session.public.participant_id != key.session_id
        || session.public.provider_kind != row.get::<String, _>("provider_kind")
    {
        return Err(stale_cleanup());
    }
    Ok(AttendeeCleanupAuthorization {
        fingerprint: *fingerprint,
        room_uid,
        key,
    })
}

async fn revalidate_in(
    tx: &mut Transaction<'_, Sqlite>,
    expected: &AttendeeCleanupAuthorization,
) -> Result<(), PersistenceError> {
    let current = authorize_in(tx, &expected.fingerprint).await?;
    if current.key != expected.key || current.room_uid != expected.room_uid {
        return Err(stale_cleanup());
    }
    Ok(())
}

async fn load_delivery(
    tx: &mut Transaction<'_, Sqlite>,
    key: &RoomRuntimeCleanupKey,
) -> Result<Option<AttendeeCleanupDelivery>, PersistenceError> {
    if !cleanup_exists(tx, &key.room_id, &key.session_id).await? {
        return Ok(None);
    }
    let cleanup_id: String = sqlx::query_scalar(
        "SELECT cleanup_id FROM attendee_cleanup_controls WHERE room_id=? AND session_id=?",
    )
    .bind(&key.room_id)
    .bind(&key.session_id)
    .fetch_one(&mut **tx)
    .await?;
    let session = load_session(tx, &key.room_id, &key.session_id).await?;
    Ok(Some(AttendeeCleanupDelivery {
        cleanup_id: parse_uuid(&cleanup_id)?,
        runtime_handle_id: session.runtime_handle_id,
        runtime_owner_id: session.runtime_owner_id,
        runtime_lease_token: session.runtime_lease_token,
    }))
}

async fn checkpoint_absence(
    tx: &mut Transaction<'_, Sqlite>,
    key: &RoomRuntimeCleanupKey,
) -> Result<AgentTurnCommit, PersistenceError> {
    let mut session = load_session(tx, &key.room_id, &key.session_id).await?;
    if session.lifecycle_intent_action == agentsassemble_domain::AgentLifecycleAction::Stop {
        return crate::attendee_stop::confirm_in(tx, session).await;
    }
    if let Some(candidate) = crate::provider_turn_reconciliation::load_active_candidate_in(
        tx,
        &key.room_id,
        &key.session_id,
    )
    .await?
    {
        return crate::provider_turn_reconciliation::finalize_runtime_gone_in(
            tx,
            &candidate,
            crate::provider_turn_reconciliation::FloorProgression::Defer,
        )
        .await;
    }
    if !crate::agent_lifecycle_authority::lifecycle_intent_is_empty(&session) {
        return Err(stale_cleanup());
    }
    crate::agent_reconciliation::stop_after_confirmed_absence(&mut session)?;
    save_session(tx, &session).await?;
    let mut participant =
        crate::agent_lifecycle::load_participant(tx, &key.room_id, &key.session_id).await?;
    participant.detach_runtime(Utc::now());
    crate::participant_rows::save_participant_exact(
        tx,
        &key.room_id,
        &key.session_id,
        &participant,
    )
    .await?;
    Ok(AgentTurnCommit {
        events: Vec::new(),
        next_assignments: Vec::new(),
    })
}

fn stale_cleanup() -> PersistenceError {
    rejected(
        "external_cleanup_changed",
        "The exact external cleanup authority is unavailable or changed.",
    )
}
