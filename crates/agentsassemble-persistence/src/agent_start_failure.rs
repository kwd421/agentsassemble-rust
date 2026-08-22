use agentsassemble_domain::{AuthenticatedPrincipal, RoomEvent, redact_persisted_diagnostic_text};
use chrono::Utc;

use crate::{
    PersistenceError, SqliteStore,
    agent_lifecycle::{clear_intent, load_session, save_session},
    agent_lifecycle_authority::require_intent,
    agent_lifecycle_events::{append_error_event, append_state_event},
    authority::active_room_for_principal,
};

const START: &str = "agent.start";
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
        agent_id: &str,
        operation_id: &str,
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
        "error".clone_into(&mut session.public.runtime_status);
        session.public.provider_session_active = false;
        session.public.last_error =
            redact_persisted_diagnostic_text(message, PUBLIC_LIFECYCLE_ERROR_LIMIT);
        if session.public.last_error.is_empty() {
            "Provider launch failed.".clone_into(&mut session.public.last_error);
        }
        session.public.last_error_code = error_code.to_owned();
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
        transaction.commit().await?;
        Ok(vec![error, state])
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
