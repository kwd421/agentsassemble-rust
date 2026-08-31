use agentsassemble_domain::{
    AuthenticatedPrincipal, DurableAgentSession, ParticipantStatus, canonical_payload_hash,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};

use crate::{
    CommandOutcome, PersistenceError, ProviderTurnExecution, ProviderTurnExecutionPhase,
    ProviderTurnInterruptCause, ProviderTurnInterruptEffect, SqliteStore,
    agent_lifecycle::{load_participant, load_session, save_session},
    agent_lifecycle_authority::{authorize_control, lifecycle_intent_is_empty, payload_agent_id},
    agent_lifecycle_events::{append_state_event, store_result},
    authority::active_room_for_principal,
    command_admission::{admit_non_lifecycle_command, inspect_non_lifecycle_command},
    provider_turn_effect::{load_optional_effect_in, prepare_interrupt_effect},
    provider_turn_execution::load_execution_in,
    room_write_budget::command_size,
    turn_authority::active_turn_authority,
};

const ACTION: &str = "agent.interrupt";
pub(crate) const INTERRUPTED_CODE: &str = "interrupted";
pub(crate) const INTERRUPTED_MESSAGE: &str =
    "The provider turn was interrupted by a room operator.";

#[derive(Debug, Clone)]
pub struct AgentInterruptMutation {
    pub outcome: CommandOutcome,
    pub interrupt_effect: Option<ProviderTurnInterruptEffect>,
}

#[derive(Debug, Clone)]
pub enum AgentInterruptPlan {
    Outcome(Box<CommandOutcome>),
    Interruptible(Box<DurableAgentSession>),
}

impl SqliteStore {
    /// Checks replay and exact durable turn authority before live provider capability proof.
    ///
    /// # Errors
    ///
    /// Returns authorization, replay-conflict, active-turn, or storage failures.
    pub async fn prepare_agent_interrupt(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<AgentInterruptPlan, PersistenceError> {
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
            ACTION,
            &payload_hash,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(AgentInterruptPlan::Outcome(Box::new(outcome)));
        }
        let (session, _) =
            load_interruptible_session(&mut transaction, &principal.room_id, &agent_id).await?;
        transaction.commit().await?;
        Ok(AgentInterruptPlan::Interruptible(Box::new(session)))
    }

    /// Durably accepts one exact busy-turn interrupt before provider I/O.
    ///
    /// # Errors
    ///
    /// Returns authorization, replay, active-turn, or storage failures.
    pub async fn execute_agent_interrupt(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<AgentInterruptMutation, PersistenceError> {
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
            ACTION,
            &payload_hash,
            command_size(request_id, ACTION, payload)?,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(AgentInterruptMutation {
                outcome,
                interrupt_effect: None,
            });
        }
        let (mut session, execution) =
            load_interruptible_session(&mut transaction, &principal.room_id, &agent_id).await?;
        let effect = prepare_interrupt_effect(
            &mut transaction,
            &execution,
            ProviderTurnInterruptCause::AgentInterrupt,
        )
        .await?;
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        let event = append_state_event(&mut transaction, principal, &session.public).await?;
        let events = vec![event.clone()];
        let result = json!({
            "agent_session": session.public,
            "interrupt_requested": true,
            "events": events,
            "event": event,
        });
        let outcome = store_result(
            &mut transaction,
            principal,
            request_id,
            ACTION,
            payload_hash,
            result,
            events,
        )
        .await?;
        transaction.commit().await?;
        Ok(AgentInterruptMutation {
            outcome,
            interrupt_effect: Some(effect),
        })
    }
}

async fn load_interruptible_session(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    agent_id: &str,
) -> Result<(DurableAgentSession, ProviderTurnExecution), PersistenceError> {
    let session = load_session(transaction, room_id, agent_id).await?;
    let participant =
        load_participant(transaction, room_id, &session.public.participant_id).await?;
    require_busy_session(&session, &participant, agent_id)?;
    let execution =
        load_execution_in(transaction, room_id, agent_id, session.turn_generation).await?;
    if load_optional_effect_in(transaction, room_id, agent_id, session.turn_generation)
        .await?
        .is_some()
    {
        return Err(rejected(
            "provider_turn_interrupt_in_progress",
            "The exact provider turn already has an interrupt owner.",
        ));
    }
    require_interruptible(&session, &execution, agent_id)?;
    Ok((session, execution))
}

fn require_busy_session(
    session: &DurableAgentSession,
    participant: &agentsassemble_domain::Participant,
    agent_id: &str,
) -> Result<(), PersistenceError> {
    if session.public.session_id != agent_id
        || session.public.participant_id != agent_id
        || session.public.status != "attached"
        || session.public.runtime_status != "busy"
        || !session.public.enabled
        || !session.public.provider_session_active
        || session.provider_session_id.is_empty()
        || participant.room_id != session.public.room_id
        || participant.participant_id != agent_id
        || participant.status != ParticipantStatus::Joined
        || participant.muted
    {
        return Err(rejected(
            "agent_not_busy",
            "The Agent Session does not own an interruptible busy turn.",
        ));
    }
    Ok(())
}

fn require_interruptible(
    session: &DurableAgentSession,
    execution: &ProviderTurnExecution,
    agent_id: &str,
) -> Result<(), PersistenceError> {
    if active_turn_authority(session) != Ok(true)
        || !lifecycle_intent_is_empty(session)
        || session.runtime_handle_id.is_empty()
        || session.runtime_owner_id.is_empty()
        || session.runtime_lease_token.is_empty()
        || execution.room_id != session.public.room_id
        || execution.session_id != agent_id
        || execution.participant_id != agent_id
        || execution.turn_generation != session.turn_generation
        || execution.turn_id != session.public.active_turn_id
        || execution.runtime_handle_id != session.runtime_handle_id
        || execution.runtime_owner_id != session.runtime_owner_id
        || execution.runtime_lease_token != session.runtime_lease_token
        || !matches!(
            execution.phase,
            ProviderTurnExecutionPhase::Assigned
                | ProviderTurnExecutionPhase::StartDispatching
                | ProviderTurnExecutionPhase::Running
        )
    {
        return Err(rejected(
            "stale_provider_turn",
            "The provider turn no longer matches durable room authority.",
        ));
    }
    Ok(())
}

fn rejected(code: &'static str, message: &'static str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}
