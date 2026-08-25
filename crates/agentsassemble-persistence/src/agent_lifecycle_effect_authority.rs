use agentsassemble_domain::{AuthenticatedPrincipal, DurableAgentSession, canonical_payload_hash};
use chrono::Utc;
use serde_json::Value;

use crate::{
    PersistenceError, SqliteStore,
    agent_lifecycle::{AgentStartEffect, AgentStopEffect, load_session, save_session},
    agent_lifecycle_authority::{lifecycle_operation_id, payload_agent_id, require_intent},
    agent_lifecycle_reservations::load_lifecycle_reservation,
    authority::active_room_for_principal,
};

const START: &str = "agent.start";
const RESUME: &str = "agent.resume";
const STOP: &str = "agent.stop";

pub(crate) fn authorize_start_effect(
    session: &mut DurableAgentSession,
    operation_id: &str,
    runtime_handle_id: &str,
    runtime_owner_id: &str,
) -> Result<(), PersistenceError> {
    require_intent(
        session,
        START,
        operation_id,
        "prepared",
        "stale_start_authorization",
    )?;
    if runtime_handle_id.is_empty()
        || runtime_owner_id.is_empty()
        || (!session.runtime_handle_id.is_empty() && session.runtime_handle_id != runtime_handle_id)
        || (!session.runtime_owner_id.is_empty() && session.runtime_owner_id != runtime_owner_id)
    {
        return Err(rejected(
            "runtime_owner_mismatch",
            "Provider start reservation does not match durable runtime authority.",
        ));
    }
    runtime_handle_id.clone_into(&mut session.runtime_handle_id);
    runtime_owner_id.clone_into(&mut session.runtime_owner_id);
    "available".clone_into(&mut session.public.status);
    session.public.enabled = true;
    if !matches!(
        session.public.runtime_status.as_str(),
        "starting" | "idle" | "busy" | "paused"
    ) {
        "starting".clone_into(&mut session.public.runtime_status);
    }
    session.public.last_error.clear();
    session.public.last_error_code.clear();
    session.public.recovery_required = false;
    "effect_inflight".clone_into(&mut session.lifecycle_intent_status);
    Ok(())
}

impl SqliteStore {
    /// Persists the exact runtime identity and an effect-inflight phase before provider I/O.
    ///
    /// # Errors
    ///
    /// Returns an exact reservation, authority, state, or persistence failure.
    #[allow(clippy::too_many_arguments)]
    pub async fn authorize_agent_start_effect(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        operation_id: &str,
        command_action: &str,
        runtime_handle_id: &str,
        runtime_owner_id: &str,
    ) -> Result<AgentStartEffect, PersistenceError> {
        if !matches!(command_action, START | RESUME) {
            return Err(rejected(
                "bad_request",
                "Provider start authorization has an invalid command action.",
            ));
        }
        let agent_id = payload_agent_id(payload)?;
        let payload_hash = canonical_payload_hash(payload);
        let expected_operation_id = lifecycle_operation_id(principal, request_id, command_action);
        if operation_id != expected_operation_id {
            return Err(rejected(
                "stale_start_authorization",
                "Provider start authorization does not match its request.",
            ));
        }
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        let reservation = load_lifecycle_reservation(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
        )
        .await?
        .ok_or_else(|| {
            rejected(
                "stale_lifecycle_reservation",
                "Provider start reservation is missing.",
            )
        })?;
        if reservation.action != command_action
            || reservation.payload_hash != payload_hash
            || reservation.session_id != agent_id
            || reservation.operation_id != operation_id
            || reservation.status != "pending"
            || reservation.phase != "lifecycle_prepared"
            || reservation.prepared_result_json != "{}"
            || reservation.supervisor_generation != self.runtime_generation()
        {
            return Err(rejected(
                "stale_lifecycle_reservation",
                "Provider start reservation is not owned by this runtime.",
            ));
        }
        let mut session = load_session(&mut transaction, &principal.room_id, &agent_id).await?;
        authorize_start_effect(
            &mut session,
            operation_id,
            runtime_handle_id,
            runtime_owner_id,
        )?;
        save_session(&mut transaction, &session).await?;
        transaction.commit().await?;
        Ok(AgentStartEffect {
            operation_id: operation_id.to_owned(),
            session,
        })
    }

    /// Durably authorizes one exact stop effect before provider I/O.
    ///
    /// # Errors
    ///
    /// Returns a stale-effect, authority, or persistence failure.
    pub async fn authorize_agent_stop_effect(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        operation_id: &str,
    ) -> Result<AgentStopEffect, PersistenceError> {
        let agent_id = payload_agent_id(payload)?;
        let payload_hash = canonical_payload_hash(payload);
        let expected_operation_id = lifecycle_operation_id(principal, request_id, STOP);
        if operation_id != expected_operation_id {
            return Err(rejected(
                "stale_stop_authorization",
                "Provider stop authorization does not match its request.",
            ));
        }
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        let reservation = load_lifecycle_reservation(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
        )
        .await?
        .ok_or_else(|| {
            rejected(
                "stale_lifecycle_reservation",
                "Provider stop reservation is missing.",
            )
        })?;
        if reservation.action != STOP
            || reservation.payload_hash != payload_hash
            || reservation.session_id != agent_id
            || reservation.operation_id != operation_id
            || reservation.status != "pending"
            || reservation.phase != "lifecycle_prepared"
            || reservation.prepared_result_json != "{}"
            || reservation.supervisor_generation != self.runtime_generation()
        {
            return Err(rejected(
                "stale_lifecycle_reservation",
                "Provider stop reservation is not owned by this runtime.",
            ));
        }
        let mut session = load_session(&mut transaction, &principal.room_id, &agent_id).await?;
        require_intent(
            &session,
            STOP,
            operation_id,
            "prepared",
            "stale_stop_authorization",
        )?;
        let effect = stop_effect(&session)?;
        "stopping".clone_into(&mut session.public.runtime_status);
        session.public.enabled = false;
        "effect_inflight".clone_into(&mut session.lifecycle_intent_status);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        transaction.commit().await?;
        Ok(effect)
    }
}

pub(crate) fn stop_effect(
    session: &DurableAgentSession,
) -> Result<AgentStopEffect, PersistenceError> {
    if session.runtime_handle_id.is_empty() || session.runtime_owner_id.is_empty() {
        return Err(rejected(
            "runtime_handle_unavailable",
            "Provider shutdown requires an exact handle owned by the current supervisor.",
        ));
    }
    Ok(AgentStopEffect {
        operation_id: session.lifecycle_intent_id.clone(),
        session_id: session.public.session_id.clone(),
        runtime_handle_id: session.runtime_handle_id.clone(),
        runtime_owner_id: session.runtime_owner_id.clone(),
    })
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}
