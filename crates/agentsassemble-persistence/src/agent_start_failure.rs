use agentsassemble_domain::{
    AuthenticatedPrincipal, RoomEvent, canonical_payload_hash, redact_persisted_diagnostic_text,
};
use chrono::Utc;
use serde_json::Value;

use crate::{
    AgentLaunchFailureCommit, PersistenceError, SqliteStore,
    agent_lifecycle::{clear_intent, load_session, save_session},
    agent_lifecycle_authority::{payload_agent_id, require_intent},
    agent_lifecycle_events::{append_error_event, append_state_event},
    agent_lifecycle_reservations::{LifecycleReservation, reject_lifecycle_command},
    authority::active_room_for_principal,
};

const START: &str = "agent.start";
const RESUME: &str = "agent.resume";
const PUBLIC_LIFECYCLE_ERROR_LIMIT: usize = 512;

impl SqliteStore {
    /// Moves a failed start out of its prepared intent without hiding the error.
    ///
    /// # Errors
    ///
    /// Returns a stale-effect rejection or persistence failure.
    pub async fn fail_agent_start(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        operation_id: &str,
        error_code: &'static str,
        message: &str,
    ) -> Result<AgentLaunchFailureCommit, PersistenceError> {
        self.fail_agent_launch(
            principal,
            request_id,
            payload,
            operation_id,
            error_code,
            message,
            START,
        )
        .await
    }

    /// Moves a failed resume out of its prepared launch intent.
    ///
    /// # Errors
    ///
    /// Returns a stale-effect rejection or persistence failure.
    #[allow(clippy::too_many_arguments)]
    pub async fn fail_agent_resume(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        operation_id: &str,
        error_code: &'static str,
        message: &str,
    ) -> Result<AgentLaunchFailureCommit, PersistenceError> {
        self.fail_agent_launch(
            principal,
            request_id,
            payload,
            operation_id,
            error_code,
            message,
            RESUME,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn fail_agent_launch(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        operation_id: &str,
        error_code: &'static str,
        message: &str,
        command_action: &'static str,
    ) -> Result<AgentLaunchFailureCommit, PersistenceError> {
        let agent_id = payload_agent_id(payload)?;
        let payload_hash = canonical_payload_hash(payload);
        self.fail_agent_launch_command(
            principal,
            request_id,
            &payload_hash,
            &agent_id,
            operation_id,
            error_code,
            message,
            command_action,
            "lifecycle_prepared",
            "{}",
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn fail_agent_launch_command(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload_hash: &str,
        agent_id: &str,
        operation_id: &str,
        error_code: &'static str,
        message: &str,
        command_action: &'static str,
        reservation_phase: &str,
        prepared_result_json: &str,
    ) -> Result<AgentLaunchFailureCommit, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        let mut session = load_session(&mut transaction, &principal.room_id, agent_id).await?;
        require_intent(
            &session,
            START,
            operation_id,
            "prepared",
            "stale_start_confirmation",
        )?;
        let reservation = LifecycleReservation {
            principal,
            request_id,
            action: command_action,
            payload_hash,
            session_id: agent_id,
            operation_id,
            phase: reservation_phase,
            prepared_result_json,
        };
        "unavailable".clone_into(&mut session.public.status);
        session.public.enabled = false;
        "error".clone_into(&mut session.public.runtime_status);
        session.public.provider_session_active = false;
        session.public.last_error =
            redact_persisted_diagnostic_text(message, PUBLIC_LIFECYCLE_ERROR_LIMIT);
        if session.public.last_error.is_empty() {
            "Provider launch failed.".clone_into(&mut session.public.last_error);
        }
        session.public.last_error_code = error_code.to_owned();
        reject_lifecycle_command(
            &mut transaction,
            &reservation,
            &session.public.last_error_code,
            &session.public.last_error,
        )
        .await?;
        session.runtime_handle_id.clear();
        session.runtime_owner_id.clear();
        clear_intent(&mut session);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        let error = append_error_event(
            &mut transaction,
            principal,
            &session.public,
            error_code,
            &session.public.last_error,
        )
        .await?;
        let state = append_state_event(&mut transaction, principal, &session.public).await?;
        let commit = AgentLaunchFailureCommit {
            events: vec![error, state],
            code: session.public.last_error_code,
            message: session.public.last_error,
        };
        transaction.commit().await?;
        Ok(commit)
    }

    /// Retains an exact start operation when process creation may have taken effect.
    ///
    /// # Errors
    ///
    /// Returns a stale-effect rejection or persistence failure.
    #[allow(clippy::too_many_arguments)]
    pub async fn mark_agent_start_unconfirmed(
        &self,
        principal: &AuthenticatedPrincipal,
        agent_id: &str,
        operation_id: &str,
        runtime_handle_id: &str,
        runtime_owner_id: &str,
        error_code: &'static str,
        message: &str,
    ) -> Result<Vec<RoomEvent>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        let mut session = load_session(&mut transaction, &principal.room_id, agent_id).await?;
        require_intent(
            &session,
            START,
            operation_id,
            "prepared",
            "stale_start_confirmation",
        )?;
        "unavailable".clone_into(&mut session.public.status);
        session.public.enabled = false;
        "disconnected".clone_into(&mut session.public.runtime_status);
        session.public.provider_session_active = false;
        session.public.provider_session_reused = false;
        session.public.last_error =
            redact_persisted_diagnostic_text(message, PUBLIC_LIFECYCLE_ERROR_LIMIT);
        if session.public.last_error.is_empty() {
            "Provider launch could not be confirmed.".clone_into(&mut session.public.last_error);
        }
        session.public.last_error_code = error_code.to_owned();
        session.public.recovery_required = true;
        if !runtime_handle_id.is_empty() {
            session.runtime_handle_id = runtime_handle_id.to_owned();
        }
        if !runtime_owner_id.is_empty() {
            session.runtime_owner_id = runtime_owner_id.to_owned();
        }
        "unconfirmed".clone_into(&mut session.lifecycle_intent_status);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        let error = append_error_event(
            &mut transaction,
            principal,
            &session.public,
            error_code,
            &session.public.last_error,
        )
        .await?;
        let state = append_state_event(&mut transaction, principal, &session.public).await?;
        transaction.commit().await?;
        Ok(vec![error, state])
    }
}
