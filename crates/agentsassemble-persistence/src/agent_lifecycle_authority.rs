use agentsassemble_domain::{
    AuthenticatedPrincipal, ClientKind, DurableAgentSession, clean_identifier, stable_identity_hash,
};
use serde_json::Value;

use crate::{AgentRuntimeStarted, PersistenceError};

const AGENT_ID_KEYS: [&str; 3] = ["agent_id", "participant_id", "session_id"];

pub(crate) fn payload_agent_id(payload: &Value) -> Result<String, PersistenceError> {
    let object = payload
        .as_object()
        .ok_or_else(|| rejected("payload must be an object."))?;
    if object
        .keys()
        .any(|key| !AGENT_ID_KEYS.contains(&key.as_str()))
    {
        return Err(rejected("payload contains an unsupported field."));
    }
    let supplied = AGENT_ID_KEYS
        .iter()
        .filter_map(|key| object.get(*key))
        .collect::<Vec<_>>();
    if supplied.len() != 1 {
        return Err(rejected("exactly one agent_id alias is required."));
    }
    let Some(raw) = supplied[0].as_str() else {
        return Err(rejected("agent_id must be a string."));
    };
    let agent_id = clean_identifier(raw, 128);
    if agent_id.is_empty()
        || agent_id != raw
        || raw.chars().count() > 128
        || raw.chars().any(char::is_control)
    {
        return Err(rejected("agent_id is invalid."));
    }
    Ok(agent_id)
}

pub(crate) fn lifecycle_operation_id(
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    action: &str,
) -> String {
    stable_identity_hash(&(
        "agentsassemble-lifecycle-operation-v1",
        principal.room_id.as_str(),
        principal.principal_id.as_str(),
        request_id,
        action,
    ))
}

pub(crate) fn authorize_control(
    principal: &AuthenticatedPrincipal,
) -> Result<(), PersistenceError> {
    if principal.client_kind == ClientKind::AgentBridge || !principal.capabilities.agent_control {
        return Err(rejected_code(
            "permission_denied",
            "agent.control permission is required.",
        ));
    }
    Ok(())
}

pub(crate) fn validate_runtime_started(
    session: &DurableAgentSession,
    started: &AgentRuntimeStarted,
) -> Result<(), PersistenceError> {
    if started.runtime_handle_id.is_empty() {
        return Err(rejected_code(
            "runtime_start_unconfirmed",
            "Provider start did not return an owned runtime handle.",
        ));
    }
    if started.runtime_owner_id.is_empty() {
        return Err(rejected_code(
            "runtime_start_unconfirmed",
            "Provider start did not return its supervisor ownership identity.",
        ));
    }
    if started.provider_session_active && started.provider_session_id.is_empty() {
        return Err(rejected_code(
            "provider_session_unconfirmed",
            "Provider start reported an active session without its identity.",
        ));
    }
    if started.provider_session_reused && !started.provider_session_active {
        return Err(rejected_code(
            "provider_session_unconfirmed",
            "An inactive provider session cannot be reported as reused.",
        ));
    }
    if started.provider_session_reused
        && (session.provider_session_id.is_empty()
            || started.provider_session_id != session.provider_session_id)
    {
        return Err(rejected_code(
            "provider_session_mismatch",
            "A reused provider session must preserve its durable identity.",
        ));
    }
    Ok(())
}

pub(crate) fn matching_prepared_intent(
    session: &DurableAgentSession,
    action: &str,
    operation_id: &str,
) -> Result<bool, PersistenceError> {
    if lifecycle_intent_is_empty(session) {
        return Ok(false);
    }
    require_matching_operation(session, action, operation_id)?;
    if session.lifecycle_intent_status != "prepared" {
        return Err(rejected_code(
            "invalid_state",
            "Stored provider lifecycle intent is invalid.",
        ));
    }
    Ok(true)
}

pub(crate) fn require_matching_operation(
    session: &DurableAgentSession,
    action: &str,
    operation_id: &str,
) -> Result<(), PersistenceError> {
    if session.lifecycle_intent_action == action && session.lifecycle_intent_id == operation_id {
        return Ok(());
    }
    Err(rejected_code(
        "operation_in_progress",
        "Another provider lifecycle operation is still in progress.",
    ))
}

pub(crate) fn lifecycle_intent_is_empty(session: &DurableAgentSession) -> bool {
    session.lifecycle_intent_action.is_empty()
        && session.lifecycle_intent_id.is_empty()
        && session.lifecycle_intent_status.is_empty()
}

pub(crate) fn require_intent(
    session: &DurableAgentSession,
    action: &str,
    operation_id: &str,
    status: &str,
    code: &'static str,
) -> Result<(), PersistenceError> {
    let action = action.strip_prefix("agent.").unwrap_or(action);
    if session.lifecycle_intent_action != action
        || session.lifecycle_intent_id != operation_id
        || session.lifecycle_intent_status != status
    {
        return Err(rejected_code(
            code,
            "Provider lifecycle confirmation does not match the active operation.",
        ));
    }
    Ok(())
}

fn rejected(message: impl Into<String>) -> PersistenceError {
    rejected_code("bad_request", message)
}

fn rejected_code(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope};
    use serde_json::json;

    use super::{lifecycle_operation_id, payload_agent_id};

    fn principal(room_id: &str, principal_id: &str) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            principal_id: principal_id.to_owned(),
            participant_id: "operator-local".to_owned(),
            display_name: "Host".to_owned(),
            room_id: room_id.to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        }
    }

    #[test]
    fn lifecycle_payload_has_one_exact_authority_key() {
        assert_eq!(
            payload_agent_id(&json!({"agent_id": "agent-1"}))
                .unwrap_or_else(|error| panic!("parse exact agent id: {error}")),
            "agent-1"
        );
        for payload in [
            json!({"agent_id": " agent-1 "}),
            json!({"agent_id": 1}),
            json!({"agent_id": "agent-1", "session_id": "agent-1"}),
            json!({"agent_id": "agent-1", "force": true}),
        ] {
            assert!(payload_agent_id(&payload).is_err());
        }
    }

    #[test]
    fn operation_identity_binds_every_external_effect_scope() {
        let base =
            lifecycle_operation_id(&principal("room-a", "operator-a"), "request", "agent.start");
        assert_eq!(
            base,
            lifecycle_operation_id(&principal("room-a", "operator-a"), "request", "agent.start")
        );
        assert_ne!(
            base,
            lifecycle_operation_id(&principal("room-b", "operator-a"), "request", "agent.start")
        );
        assert_ne!(
            base,
            lifecycle_operation_id(&principal("room-a", "operator-b"), "request", "agent.start")
        );
        assert_ne!(
            base,
            lifecycle_operation_id(
                &principal("room-a", "operator-a"),
                " request ",
                "agent.start"
            )
        );
        assert_ne!(
            base,
            lifecycle_operation_id(&principal("room-a", "operator-a"), "request", "agent.stop")
        );
    }
}
