use agentsassemble_domain::{
    AgentRuntimeStatus, AgentSessionStatus, DurableAgentSession, RoomStatus,
};
use chrono::Utc;
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    AgentTurnCommit, PersistenceError, RuntimeReconciliationCandidate, SqliteStore,
    agent_lifecycle::{load_session, require_valid_turn_authority, save_session},
    agent_lifecycle_authority::lifecycle_intent_is_empty,
    provider_turn_execution::blocking_execution_exists,
    room_turns::{
        assign_pending_in,
        support::{load_room_with_settings, session_state_event},
    },
};

const CLEANUP_PAGE_LIMIT: u8 = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomRuntimeCleanupKey {
    pub room_id: String,
    pub session_id: String,
}

#[derive(Debug)]
pub struct RoomRuntimeCleanupPage {
    pub keys: Vec<RoomRuntimeCleanupKey>,
    pub next_cursor: Option<RoomRuntimeCleanupKey>,
}

pub(crate) async fn request_runtime_cleanup(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &mut DurableAgentSession,
) -> Result<(), PersistenceError> {
    require_valid_turn_authority(session)?;
    sqlx::query("INSERT INTO room_runtime_cleanup(room_id, session_id) VALUES (?, ?) ON CONFLICT(room_id, session_id) DO NOTHING")
        .bind(&session.public.room_id)
        .bind(&session.public.session_id)
        .execute(&mut **transaction)
        .await?;
    session.public.enabled = false;
    session.public.status = AgentSessionStatus::Detached;
    session.schedule_requested = false;
    session.pending_inputs.clear();
    session.public.updated_at = Utc::now();
    save_session(transaction, session).await
}

pub(crate) async fn load_launch_session(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
) -> Result<DurableAgentSession, PersistenceError> {
    if cleanup_exists(transaction, room_id, session_id).await? {
        return Err(PersistenceError::CommandRejected {
            code: "runtime_cleanup_pending",
            message:
                "Removed runtime custody must be resolved before this Agent Session can launch."
                    .to_owned(),
        });
    }
    let session = load_session(transaction, room_id, session_id).await?;
    require_server_custody(&session)?;
    Ok(session)
}

pub(crate) fn require_server_custody(
    session: &DurableAgentSession,
) -> Result<(), PersistenceError> {
    if session.public.external_owned || session.public.process_ownership != "server" {
        return Err(PersistenceError::CommandRejected {
            code: "external_runtime_owned",
            message: "This provider runtime is owned by its external attendee.".to_owned(),
        });
    }
    Ok(())
}

pub(crate) async fn cleanup_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
) -> Result<bool, PersistenceError> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM room_runtime_cleanup WHERE room_id = ? AND session_id = ?)",
    )
    .bind(room_id)
    .bind(session_id)
    .fetch_one(&mut **transaction)
    .await?)
}

impl SqliteStore {
    /// Loads blocking turn custody and its pending removal fence in one transaction.
    ///
    /// # Errors
    /// Returns invalid stored turn authority or persistence failures.
    pub async fn load_room_runtime_cleanup_turn(
        &self,
        key: &RoomRuntimeCleanupKey,
    ) -> Result<Option<crate::ProviderTurnReconciliationCandidate>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        if !cleanup_exists(&mut transaction, &key.room_id, &key.session_id).await? {
            transaction.commit().await?;
            return Ok(None);
        }
        let candidate = crate::provider_turn_reconciliation::load_active_candidate_in(
            &mut transaction,
            &key.room_id,
            &key.session_id,
        )
        .await?;
        transaction.commit().await?;
        Ok(candidate)
    }

    /// Loads exact custody for a pending removal, including a disconnected session
    /// whose runtime absence still needs a positive observation.
    ///
    /// # Errors
    /// Returns invalid stored custody or persistence failures.
    pub async fn load_room_runtime_cleanup_candidate(
        &self,
        key: &RoomRuntimeCleanupKey,
    ) -> Result<Option<RuntimeReconciliationCandidate>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        if !cleanup_exists(&mut transaction, &key.room_id, &key.session_id).await?
            || blocking_execution_exists(&mut transaction, &key.room_id, &key.session_id).await?
        {
            transaction.commit().await?;
            return Ok(None);
        }
        let candidate = crate::agent_reconciliation::load_candidate(
            &mut transaction,
            &key.room_id,
            &key.session_id,
        )
        .await?;
        transaction.commit().await?;
        Ok(candidate)
    }

    /// Reads only pending removal work for the existing server recovery owner.
    ///
    /// # Errors
    /// Returns persistence failures without treating a failed scan as an empty page.
    pub async fn load_room_runtime_cleanup_page(
        &self,
        cursor: Option<&RoomRuntimeCleanupKey>,
    ) -> Result<RoomRuntimeCleanupPage, PersistenceError> {
        let room_id = cursor.map(|key| key.room_id.as_str());
        let session_id = cursor.map(|key| key.session_id.as_str());
        let rows = sqlx::query(
            "SELECT room_id, session_id FROM room_runtime_cleanup WHERE ? IS NULL OR room_id > ? OR (room_id = ? AND session_id > ?) ORDER BY room_id, session_id LIMIT ?",
        )
        .bind(room_id)
        .bind(room_id)
        .bind(room_id)
        .bind(session_id)
        .bind(i64::from(CLEANUP_PAGE_LIMIT))
        .fetch_all(&self.pool)
        .await?;
        let keys = rows
            .into_iter()
            .map(|row| RoomRuntimeCleanupKey {
                room_id: row.get("room_id"),
                session_id: row.get("session_id"),
            })
            .collect::<Vec<_>>();
        let next_cursor = if keys.len() == usize::from(CLEANUP_PAGE_LIMIT) {
            keys.last().cloned()
        } else {
            None
        };
        Ok(RoomRuntimeCleanupPage { keys, next_cursor })
    }

    /// Finishes removal only after the runtime and turn owners checkpoint absence.
    /// The pending row fences new launches throughout this transaction.
    ///
    /// # Errors
    /// Returns storage failures; unresolved custody remains pending and returns `None`.
    pub async fn finish_room_runtime_cleanup(
        &self,
        key: &RoomRuntimeCleanupKey,
    ) -> Result<Option<AgentTurnCommit>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        if !cleanup_exists(&mut transaction, &key.room_id, &key.session_id).await? {
            transaction.commit().await?;
            return Ok(Some(AgentTurnCommit {
                events: Vec::new(),
                next_assignments: Vec::new(),
            }));
        }
        let mut session = load_session(&mut transaction, &key.room_id, &key.session_id).await?;
        require_valid_turn_authority(&session)?;
        if !lifecycle_intent_is_empty(&session)
            || !matches!(
                session.public.runtime_status,
                AgentRuntimeStatus::Stopped | AgentRuntimeStatus::Error
            )
            || session.public.recovery_required
            || session.public.provider_session_active
            || session.public.provider_session_reused
            || !session.runtime_handle_id.is_empty()
            || !session.runtime_owner_id.is_empty()
            || !session.runtime_lease_token.is_empty()
            || !session.public.active_turn_id.is_empty()
            || blocking_execution_exists(&mut transaction, &key.room_id, &key.session_id).await?
        {
            transaction.commit().await?;
            return Ok(None);
        }
        let participant = crate::agent_lifecycle::load_participant(
            &mut transaction,
            &key.room_id,
            &key.session_id,
        )
        .await?;
        session.public.status =
            if participant.status == agentsassemble_domain::ParticipantStatus::Joined {
                AgentSessionStatus::Available
            } else {
                AgentSessionStatus::Detached
            };
        session.public.runtime_status = AgentRuntimeStatus::Stopped;
        session.public.enabled = false;
        session.public.provider_session_active = false;
        session.public.provider_session_reused = false;
        session.public.recovery_required = false;
        session.public.last_error.clear();
        session.public.last_error_code.clear();
        session.pending_inputs.clear();
        session.inflight_inputs.clear();
        session.schedule_requested = false;
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        sqlx::query("DELETE FROM room_runtime_cleanup WHERE room_id = ? AND session_id = ?")
            .bind(&key.room_id)
            .bind(&key.session_id)
            .execute(&mut *transaction)
            .await?;
        let event = session_state_event(&mut transaction, &session).await?;
        let (room, settings) = load_room_with_settings(&mut transaction, &key.room_id).await?;
        let mut commit = if room.status == RoomStatus::Active {
            assign_pending_in(&mut transaction, &room, &settings).await?
        } else {
            AgentTurnCommit {
                events: Vec::new(),
                next_assignments: Vec::new(),
            }
        };
        commit.events.insert(0, event);
        transaction.commit().await?;
        Ok(Some(commit))
    }
}
