use agentsassemble_domain::{AuthenticatedPrincipal, clean_identifier, stable_identity_hash};
use serde_json::Value;

use crate::PersistenceError;

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

fn rejected(message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "bad_request",
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
