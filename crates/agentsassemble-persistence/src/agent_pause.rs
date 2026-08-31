use agentsassemble_domain::{
    AuthenticatedPrincipal, DurableAgentSession, ParticipantStatus, canonical_payload_hash,
};
use chrono::Utc;
use serde_json::{Value, json};

use crate::{
    CommandOutcome, PersistenceError, SqliteStore,
    agent_lifecycle::{load_participant, load_session, save_session},
    agent_lifecycle_authority::{authorize_control, lifecycle_intent_is_empty, payload_agent_id},
    agent_lifecycle_events::{append_state_event, store_result},
    authority::active_room_for_principal,
    command_admission::{
        ExistingRequestIdentity, admit_non_lifecycle_command, existing_command,
        existing_request_identity, inspect_non_lifecycle_command,
    },
    room_write_budget::command_size,
    turn_authority::active_turn_authority,
};

const PAUSE: &str = "agent.pause";
const RESUME: &str = "agent.resume";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentResidentRuntime {
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub runtime_lease_token: String,
    pub runtime_profile_key: String,
}

#[derive(Debug, Clone)]
pub enum AgentResidentPlan {
    Outcome(Box<CommandOutcome>),
    Resident(Box<DurableAgentSession>),
    ProviderLaunch,
}

impl SqliteStore {
    /// Checks replay, authority, and durable idle state before live runtime proof.
    ///
    /// # Errors
    ///
    /// Returns authorization, replay-conflict, state-invariant, or storage failures.
    pub async fn prepare_agent_pause(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<AgentResidentPlan, PersistenceError> {
        self.prepare_resident_action(principal, request_id, payload, PAUSE, false)
            .await
    }

    /// Routes an exact paused session to live proof and every other resume to provider launch.
    ///
    /// # Errors
    ///
    /// Returns authorization, replay-conflict, state-invariant, or storage failures.
    pub async fn prepare_paused_agent_resume(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<AgentResidentPlan, PersistenceError> {
        self.prepare_resident_action(principal, request_id, payload, RESUME, true)
            .await
    }

    async fn prepare_resident_action(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        action: &'static str,
        resume: bool,
    ) -> Result<AgentResidentPlan, PersistenceError> {
        authorize_control(principal)?;
        let agent_id = payload_agent_id(payload)?;
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if resume {
            match existing_request_identity(
                &mut transaction,
                &principal.room_id,
                &principal.principal_id,
                request_id,
                action,
                &payload_hash,
            )
            .await?
            {
                Some(ExistingRequestIdentity::CommittedResult) => {
                    let outcome = existing_command(
                        &mut transaction,
                        &principal.room_id,
                        &principal.principal_id,
                        request_id,
                        action,
                        &payload_hash,
                    )
                    .await?
                    .ok_or_else(invalid_request_owner)?;
                    transaction.commit().await?;
                    return Ok(AgentResidentPlan::Outcome(Box::new(outcome)));
                }
                Some(
                    ExistingRequestIdentity::PendingLifecycle
                    | ExistingRequestIdentity::RejectedLifecycle,
                ) => {
                    transaction.commit().await?;
                    return Ok(AgentResidentPlan::ProviderLaunch);
                }
                None => {}
            }
        } else if let Some(outcome) = inspect_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            action,
            &payload_hash,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(AgentResidentPlan::Outcome(Box::new(outcome)));
        }
        let session = load_session(&mut transaction, &principal.room_id, &agent_id).await?;
        if resume && session.public.runtime_status != "paused" {
            transaction.commit().await?;
            return Ok(AgentResidentPlan::ProviderLaunch);
        }
        require_resident_state(
            &mut transaction,
            &session,
            &agent_id,
            if resume { "paused" } else { "idle" },
            !resume,
        )
        .await?;
        transaction.commit().await?;
        Ok(AgentResidentPlan::Resident(Box::new(session)))
    }

    /// Pauses one idle resident runtime without calling or replacing its provider.
    ///
    /// # Errors
    ///
    /// Returns authorization, replay, state-invariant, or storage failures.
    pub async fn execute_agent_pause(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        runtime: &AgentResidentRuntime,
    ) -> Result<CommandOutcome, PersistenceError> {
        authorize_control(principal)?;
        let agent_id = payload_agent_id(payload)?;
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            PAUSE,
            &payload_hash,
            command_size(request_id, PAUSE, payload)?,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(outcome);
        }
        let mut session = load_session(&mut transaction, &principal.room_id, &agent_id).await?;
        require_resident_state(&mut transaction, &session, &agent_id, "idle", true).await?;
        require_matching_runtime(&session, runtime)?;
        session.public.enabled = false;
        "paused".clone_into(&mut session.public.runtime_status);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        let event = append_state_event(&mut transaction, principal, &session.public).await?;
        let events = vec![event.clone()];
        let result = json!({
            "agent_session": session.public,
            "runtime_reused": true,
            "process_preserved": true,
            "events": events,
            "event": event,
        });
        let outcome = store_result(
            &mut transaction,
            principal,
            request_id,
            PAUSE,
            payload_hash,
            result,
            events,
        )
        .await?;
        transaction.commit().await?;
        Ok(outcome)
    }

    /// Resumes a paused resident runtime, or returns `None` when the existing
    /// provider-launch resume owner must handle the session instead.
    ///
    /// # Errors
    ///
    /// Returns authorization, replay, state-invariant, or storage failures.
    pub async fn resume_paused_agent(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        runtime: &AgentResidentRuntime,
    ) -> Result<Option<CommandOutcome>, PersistenceError> {
        authorize_control(principal)?;
        let agent_id = payload_agent_id(payload)?;
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if let Some(outcome) = inspect_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            RESUME,
            &payload_hash,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(Some(outcome));
        }
        let mut session = load_session(&mut transaction, &principal.room_id, &agent_id).await?;
        if session.public.runtime_status != "paused" {
            transaction.commit().await?;
            return Ok(None);
        }
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            RESUME,
            &payload_hash,
            command_size(request_id, RESUME, payload)?,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(Some(outcome));
        }
        require_resident_state(&mut transaction, &session, &agent_id, "paused", false).await?;
        require_matching_runtime(&session, runtime)?;
        session.public.enabled = true;
        "idle".clone_into(&mut session.public.runtime_status);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        let event = append_state_event(&mut transaction, principal, &session.public).await?;
        let events = vec![event.clone()];
        let result = json!({
            "agent_session": session.public,
            "runtime_reused": true,
            "process_reused": true,
            "events": events,
            "event": event,
        });
        let outcome = store_result(
            &mut transaction,
            principal,
            request_id,
            RESUME,
            payload_hash,
            result,
            events,
        )
        .await?;
        transaction.commit().await?;
        Ok(Some(outcome))
    }
}

fn invalid_request_owner() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "invalid_state",
        message: "A room request lost its durable result owner.".to_owned(),
    }
}

fn require_matching_runtime(
    session: &DurableAgentSession,
    runtime: &AgentResidentRuntime,
) -> Result<(), PersistenceError> {
    if session.runtime_handle_id == runtime.runtime_handle_id
        && session.runtime_owner_id == runtime.runtime_owner_id
        && session.runtime_lease_token == runtime.runtime_lease_token
        && session.runtime_profile_key == runtime.runtime_profile_key
    {
        Ok(())
    } else {
        Err(PersistenceError::CommandRejected {
            code: "resident_runtime_changed",
            message: "The resident provider runtime changed before the scheduling-state update."
                .to_owned(),
        })
    }
}

async fn require_resident_state(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    session: &DurableAgentSession,
    agent_id: &str,
    runtime_status: &str,
    enabled: bool,
) -> Result<(), PersistenceError> {
    let participant = load_participant(
        transaction,
        &session.public.room_id,
        &session.public.participant_id,
    )
    .await?;
    let valid = session.public.session_id == agent_id
        && session.public.participant_id == agent_id
        && participant.room_id == session.public.room_id
        && participant.participant_id == agent_id
        && participant.status == ParticipantStatus::Joined
        && session.public.status == "attached"
        && session.public.runtime_status == runtime_status
        && session.public.enabled == enabled
        && session.public.provider_session_active
        && !session.public.recovery_required
        && !session.public.external_owned
        && session.public.process_ownership == "server"
        && !session.provider_session_id.is_empty()
        && !session.runtime_handle_id.is_empty()
        && !session.runtime_owner_id.is_empty()
        && !session.runtime_lease_token.is_empty()
        && lifecycle_intent_is_empty(session)
        && active_turn_authority(session) == Ok(false);
    if valid {
        Ok(())
    } else {
        Err(PersistenceError::CommandRejected {
            code: "invalid_state",
            message:
                "Only a complete idle or paused resident Agent Session can change scheduling state."
                    .to_owned(),
        })
    }
}
