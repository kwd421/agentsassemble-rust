use agentsassemble_domain::{
    AgentRuntimeStatus, AgentSessionStatus, CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession,
    ParticipantStatus,
};
use chrono::Utc;
use sqlx::{Sqlite, Transaction};

use crate::{
    AgentRuntimeStarted, PersistenceError, RuntimeRestartPhase as Phase, RuntimeRestartRecord,
    RuntimeRestartTarget, SqliteStore,
    agent_lifecycle::{apply_runtime_started, load_participant, load_session, save_session},
    agent_lifecycle_authority::validate_runtime_started,
    runtime_restart::{load_restart, save_restart, stale},
};

impl SqliteStore {
    /// Binds source-owned shutdown to the exact preflighted replacement image.
    ///
    /// # Errors
    /// Rejects stale ownership, invalid image identity, or storage failure.
    pub async fn begin_runtime_restart_drain(
        &self,
        operation_id: &str,
        candidate_identity: &str,
    ) -> Result<RuntimeRestartRecord, PersistenceError> {
        if candidate_identity.is_empty() {
            return Err(stale());
        }
        let mut tx = self.pool.begin().await?;
        let mut record = load_restart(&mut tx).await?.ok_or_else(stale)?;
        if record.operation_id != operation_id
            || record.source_generation != self.runtime_generation()
            || !matches!(record.phase, Phase::Quiescing | Phase::Draining)
            || record
                .candidate_identity
                .as_deref()
                .is_some_and(|id| id != candidate_identity)
        {
            return Err(stale());
        }
        record.phase = Phase::Draining;
        record.candidate_identity = Some(candidate_identity.to_owned());
        record.updated_at = Utc::now();
        save_restart(&mut tx, &record).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Claims reconstruction after the replacement has reconciled old runtime custody.
    ///
    /// # Errors
    /// Rejects the source generation, a different image, or remaining runtime authority.
    pub async fn begin_runtime_restart_recovery(
        &self,
        operation_id: &str,
        candidate_identity: &str,
    ) -> Result<RuntimeRestartRecord, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let mut record = load_restart(&mut tx).await?.ok_or_else(stale)?;
        if record.operation_id != operation_id
            || record.source_generation == self.runtime_generation()
            || record.candidate_identity.as_deref() != Some(candidate_identity)
            || !matches!(record.phase, Phase::Draining | Phase::Recovering)
        {
            return Err(stale());
        }
        if record.phase == Phase::Recovering
            && record.recovery_generation.as_deref() == Some(self.runtime_generation())
        {
            tx.commit().await?;
            return Ok(record);
        }
        for target in &record.targets {
            let session = load_target(&mut tx, target).await?;
            require_stopped(&session)?;
        }
        record.phase = Phase::Recovering;
        record.recovery_generation = Some(self.runtime_generation().to_owned());
        record.updated_at = Utc::now();
        save_restart(&mut tx, &record).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Loads one eligible target for a supervisor reservation, without authorizing launch.
    ///
    /// # Errors
    /// Rejects unrelated targets, stale generations, or custody not positively stopped.
    pub async fn runtime_restart_target(
        &self,
        operation_id: &str,
        room_id: &str,
        session_id: &str,
    ) -> Result<DurableAgentSession, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let record = recovery_record(&mut tx, operation_id, self.runtime_generation()).await?;
        let target = target(&record, room_id, session_id)?;
        let session = load_target(&mut tx, target).await?;
        require_stopped(&session)?;
        tx.commit().await?;
        Ok(session)
    }

    /// Persists exact supervisor custody before reconstruction may cross the provider boundary.
    ///
    /// # Errors
    /// Rejects mismatched targets, incomplete identity, duplicate launches, or stale authority.
    pub async fn authorize_runtime_restart_target(
        &self,
        operation_id: &str,
        requested: &RuntimeRestartTarget,
        handle_id: &str,
        owner_id: &str,
        lease_token: &str,
    ) -> Result<DurableAgentSession, PersistenceError> {
        if handle_id.is_empty() || owner_id.is_empty() || lease_token.is_empty() {
            return Err(stale());
        }
        let mut tx = self.pool.begin().await?;
        let record = recovery_record(&mut tx, operation_id, self.runtime_generation()).await?;
        let target = target(&record, &requested.room_id, &requested.session_id)?;
        let mut session = load_target(&mut tx, target).await?;
        require_stopped(&session)?;
        handle_id.clone_into(&mut session.runtime_handle_id);
        owner_id.clone_into(&mut session.runtime_owner_id);
        lease_token.clone_into(&mut session.runtime_lease_token);
        session.public.status = AgentSessionStatus::Available;
        session.public.runtime_status = AgentRuntimeStatus::Starting;
        session.public.enabled = true;
        session.public.last_error.clear();
        session.public.last_error_code.clear();
        session.public.updated_at = Utc::now();
        save_session(&mut tx, &session).await?;
        tx.commit().await?;
        Ok(session)
    }

    /// Applies an actual supervisor confirmation while retaining the global floor barrier.
    ///
    /// # Errors
    /// Rejects unconfirmed or mismatched custody and stale reconstruction ownership.
    pub async fn complete_runtime_restart_target(
        &self,
        operation_id: &str,
        room_id: &str,
        session_id: &str,
        started: &AgentRuntimeStarted,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let record = recovery_record(&mut tx, operation_id, self.runtime_generation()).await?;
        let target = target(&record, room_id, session_id)?;
        let mut session = load_target(&mut tx, target).await?;
        validate_runtime_started(&session, started)?;
        if !started.provider_session_active
            || !matches!(
                session.public.runtime_status,
                AgentRuntimeStatus::Starting
                    | AgentRuntimeStatus::Idle
                    | AgentRuntimeStatus::Paused
            )
        {
            return Err(stale());
        }
        apply_runtime_started(&mut session, started);
        if target.paused {
            session.public.enabled = false;
            session.public.runtime_status = AgentRuntimeStatus::Paused;
        }
        save_session(&mut tx, &session).await?;
        let mut participant = load_participant(&mut tx, room_id, session_id).await?;
        participant.status = ParticipantStatus::Joined;
        participant.updated_at = Utc::now();
        crate::participant_rows::save_participant_exact(&mut tx, room_id, session_id, &participant)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Records terminal failure only after this owner's captured custody is positively stopped.
    ///
    /// # Errors
    /// Rejects stale ownership or any remaining or uncertain runtime custody.
    pub async fn fail_runtime_restart_after_cleanup(
        &self,
        operation_id: &str,
    ) -> Result<RuntimeRestartRecord, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let mut record = load_restart(&mut tx).await?.ok_or_else(stale)?;
        let owned = match record.phase {
            Phase::Draining => record.source_generation == self.runtime_generation(),
            Phase::Recovering | Phase::Failed => {
                record.recovery_generation.as_deref() == Some(self.runtime_generation())
                    || (record.recovery_generation.is_none()
                        && record.source_generation == self.runtime_generation())
            }
            _ => false,
        };
        if record.operation_id != operation_id || !owned {
            return Err(stale());
        }
        for target in &record.targets {
            let session = load_session(&mut tx, &target.room_id, &target.session_id).await?;
            require_stopped(&session)?;
        }
        record.phase = Phase::Failed;
        record.updated_at = Utc::now();
        save_restart(&mut tx, &record).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Ends reconstruction only after every captured target has confirmed its required state.
    /// The coordinator owns the single floor wakeup after this commit.
    ///
    /// # Errors
    /// Returns stale generation, incomplete reconstruction, or persistence failure.
    pub async fn complete_runtime_restart(
        &self,
        operation_id: &str,
    ) -> Result<RuntimeRestartRecord, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let mut record = recovery_record(&mut tx, operation_id, self.runtime_generation()).await?;
        for target in &record.targets {
            let session = load_target(&mut tx, target).await?;
            if session.public.status != AgentSessionStatus::Attached
                || !session.public.provider_session_active
                || session.public.enabled == target.paused
                || session.public.runtime_status
                    != if target.paused {
                        AgentRuntimeStatus::Paused
                    } else {
                        AgentRuntimeStatus::Idle
                    }
                || session.runtime_handle_id.is_empty()
                || session.runtime_owner_id.is_empty()
                || session.runtime_lease_token.is_empty()
                || session.public.recovery_required
            {
                return Err(stale());
            }
        }
        record.phase = Phase::Completed;
        record.updated_at = Utc::now();
        save_restart(&mut tx, &record).await?;
        tx.commit().await?;
        Ok(record)
    }
}

async fn recovery_record(
    tx: &mut Transaction<'_, Sqlite>,
    operation_id: &str,
    generation: &str,
) -> Result<RuntimeRestartRecord, PersistenceError> {
    let record = load_restart(tx).await?.ok_or_else(stale)?;
    if record.operation_id != operation_id
        || record.phase != Phase::Recovering
        || record.recovery_generation.as_deref() != Some(generation)
    {
        return Err(stale());
    }
    Ok(record)
}

fn target<'a>(
    record: &'a RuntimeRestartRecord,
    room_id: &str,
    session_id: &str,
) -> Result<&'a RuntimeRestartTarget, PersistenceError> {
    record
        .targets
        .iter()
        .find(|target| target.room_id == room_id && target.session_id == session_id)
        .ok_or_else(stale)
}

async fn load_target(
    tx: &mut Transaction<'_, Sqlite>,
    target: &RuntimeRestartTarget,
) -> Result<DurableAgentSession, PersistenceError> {
    crate::authority::load_active_room(tx, &target.room_id).await?;
    let session = load_session(tx, &target.room_id, &target.session_id).await?;
    let participant = load_participant(tx, &target.room_id, &target.session_id).await?;
    if matches!(
        participant.status,
        ParticipantStatus::Kicked | ParticipantStatus::Exported
    ) || session.public.external_owned
        || session.public.process_ownership != "server"
        || session.runtime_profile_version != CURRENT_RUNTIME_PROFILE_VERSION
        || !crate::agent_lifecycle_authority::lifecycle_intent_is_empty(&session)
        || crate::turn_authority::active_turn_authority(&session).map_err(|_| stale())?
    {
        return Err(stale());
    }
    Ok(session)
}

fn require_stopped(session: &DurableAgentSession) -> Result<(), PersistenceError> {
    if session.public.runtime_status != AgentRuntimeStatus::Stopped
        || session.public.provider_session_active
        || session.public.recovery_required
        || !session.runtime_handle_id.is_empty()
        || !session.runtime_owner_id.is_empty()
        || !session.runtime_lease_token.is_empty()
    {
        return Err(stale());
    }
    Ok(())
}
