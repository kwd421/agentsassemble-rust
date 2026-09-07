use agentsassemble_domain::{
    AgentLifecycleAction, AgentLifecycleIntentStatus, AgentRuntimeStatus, AgentSessionDraft,
    AgentSessionStatus, AuthenticatedPrincipal, CURRENT_RUNTIME_PROFILE_VERSION, ClientKind,
    DurableAgentSession, canonical_payload_hash,
};
use chrono::Utc;
use serde_json::{Value, json};

use crate::{
    CommandOutcome, PersistenceError, SqliteStore,
    agent_lifecycle::{load_participant, load_session, save_session},
    agent_lifecycle_events::{append_state_event, store_result},
    authority::active_room_for_principal,
    command_admission::admit_non_lifecycle_command,
    filesystem_authority::revalidate_runtime_authority,
    persona_library::resolve_persona_selection,
    room_write_budget::command_size,
};

const ACTION: &str = "agent.configure";

impl SqliteStore {
    /// Loads the exact private stopped profile used to validate an `agent.configure` request.
    ///
    /// # Errors
    ///
    /// Returns authorization, payload, or stopped-state failures.
    pub async fn agent_configuration_candidate(
        &self,
        principal: &AuthenticatedPrincipal,
        payload: &Value,
    ) -> Result<DurableAgentSession, PersistenceError> {
        require_agent_control(principal)?;
        let agent_id = required_agent_id(payload)?;
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        let session = load_session(&mut transaction, &principal.room_id, &agent_id).await?;
        require_stopped_profile(&session)?;
        transaction.commit().await?;
        Ok(session)
    }

    /// Atomically replaces a stopped session's private runtime profile and public projection.
    ///
    /// # Errors
    ///
    /// Returns authorization, replay, changed-authority, filesystem, or persistence failures.
    pub async fn execute_agent_configuration(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        expected_profile_key: &str,
        draft: &AgentSessionDraft,
    ) -> Result<CommandOutcome, PersistenceError> {
        require_agent_control(principal)?;
        revalidate_runtime_authority(draft).await?;
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
            return Ok(outcome);
        }
        let mut session =
            load_session(&mut transaction, &principal.room_id, &draft.agent_id).await?;
        require_stopped_profile(&session)?;
        if session.runtime_profile_key != expected_profile_key {
            return Err(rejected(
                "runtime_profile_changed",
                "The Agent Session runtime profile changed while it was being validated.",
            ));
        }
        if session.public.provider_kind != draft.provider_kind
            || session.public.runtime_kind != draft.runtime_kind
        {
            return Err(rejected(
                "provider_mismatch",
                "An existing Agent Session cannot change provider kind.",
            ));
        }

        let persona_card =
            resolve_persona_selection(&mut transaction, &draft.persona_card_id).await?;
        apply_draft(&mut session, draft, persona_card);
        save_session(&mut transaction, &session).await?;
        let participant = load_participant(
            &mut transaction,
            &principal.room_id,
            &session.public.participant_id,
        )
        .await?;
        let event = append_state_event(&mut transaction, principal, &session.public).await?;
        let events = vec![event.clone()];
        let result = json!({
            "status": "configured",
            "agent_session": session.public,
            "participant": participant,
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
        Ok(outcome)
    }
}

fn apply_draft(
    session: &mut DurableAgentSession,
    draft: &AgentSessionDraft,
    persona_card: Option<Box<agentsassemble_domain::PersonaAssetSummary>>,
) {
    session.public.status = AgentSessionStatus::Available;
    session.public.runtime_status = AgentRuntimeStatus::Stopped;
    session.public.enabled = false;
    session.public.model.clone_from(&draft.model);
    session
        .public
        .reasoning_effort
        .clone_from(&draft.reasoning_effort);
    session.public.service_tier.clone_from(&draft.service_tier);
    session.public.variant.clone_from(&draft.variant);
    session
        .public
        .execution_harness
        .clone_from(&draft.execution_harness);
    session
        .public
        .permission_mode
        .clone_from(&draft.permission_mode);
    session.public.max_output_tokens = draft.max_output_tokens;
    session
        .public
        .catalog_revision
        .clone_from(&draft.catalog_revision);
    session.public.persona_card_id = draft.persona_card_id.clone().into_boxed_str();
    session.public.persona_card = persona_card;
    session.public.transport.clone_from(&draft.transport);
    session.public.last_error.clear();
    session.public.last_error_code.clear();
    session.public.recovery_required = false;
    session.public.updated_at = Utc::now();
    session.executable.clone_from(&draft.executable);
    session
        .executable_identity
        .clone_from(&draft.executable_identity);
    session.workspace.clone_from(&draft.workspace);
    session
        .workspace_identity
        .clone_from(&draft.workspace_identity);
    session
        .provider_endpoint
        .clone_from(&draft.provider_endpoint);
    session
        .runtime_profile_key
        .clone_from(&draft.runtime_profile_key);
    session.runtime_profile_version = CURRENT_RUNTIME_PROFILE_VERSION;
    session.lifecycle_intent_action = AgentLifecycleAction::None;
    session.lifecycle_intent_id.clear();
    session.lifecycle_intent_status = AgentLifecycleIntentStatus::None;
}

fn require_agent_control(principal: &AuthenticatedPrincipal) -> Result<(), PersistenceError> {
    if principal.client_kind == ClientKind::AgentBridge || !principal.capabilities.agent_control {
        return Err(rejected(
            "permission_denied",
            "agent.control permission is required.",
        ));
    }
    Ok(())
}

fn required_agent_id(payload: &Value) -> Result<String, PersistenceError> {
    let value = payload
        .as_object()
        .and_then(|payload| payload.get("agent_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| rejected("bad_request", "agent_id is required."))?;
    if value.is_empty()
        || value.len() > 128
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(rejected("bad_request", "agent_id is invalid."));
    }
    Ok(value.to_owned())
}

fn require_stopped_profile(session: &DurableAgentSession) -> Result<(), PersistenceError> {
    let stopped = !session.public.enabled
        && matches!(
            session.public.runtime_status,
            AgentRuntimeStatus::Stopped
                | AgentRuntimeStatus::Error
                | AgentRuntimeStatus::Disconnected
        )
        && session.public.active_turn_id.is_empty()
        && session.runtime_handle_id.is_empty()
        && session.runtime_owner_id.is_empty()
        && session.runtime_lease_token.is_empty()
        && session.lifecycle_intent_action.is_none()
        && session.lifecycle_intent_id.is_empty()
        && session.lifecycle_intent_status.is_none();
    if !stopped {
        return Err(rejected(
            "runtime_profile_conflict",
            "Stop this Agent Session before changing its runtime settings.",
        ));
    }
    Ok(())
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}
