use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::{Actor, AuthenticatedPrincipal, RoomEvent};

const PRIVATE_PUBLIC_KEYS: &[&str] = &[
    "absolute_path",
    "active_source_event_id",
    "argv",
    "bridge_handle_id",
    "bridge_pid",
    "command_configured",
    "config_path",
    "credentials",
    "env",
    "executable",
    "executable_identity",
    "file_path",
    "inflight_event_ids",
    "inflight_inputs",
    "lifecycle_intent_action",
    "lifecycle_intent_id",
    "lifecycle_intent_status",
    "path",
    "pending_event_ids",
    "pending_event_observation_kinds",
    "pending_inputs",
    "pending_provider_request",
    "pid",
    "provider_endpoint",
    "provider_observation_kind",
    "provider_session_id",
    "provider_turn_id",
    "reactivation_operation_id",
    "reported_provider_pid",
    "resolved_executable",
    "room_publication_digest",
    "room_publication_proof",
    "room_publication_turn_id",
    "runtime_handle_id",
    "runtime_lease_token",
    "runtime_owner_id",
    "runtime_profile_key",
    "runtime_profile_version",
    "stderr_path",
    "stderr_tail",
    "stdout_path",
    "terminal_tail",
    "ticket",
    "token",
    "workspace",
    "workspace_identity",
    "write_budget_stream_bytes",
    "write_budget_stream_commands",
    "write_budget_turn_id",
];

/// Projects one durable event for a viewer while preserving its room cursor.
#[must_use]
pub fn public_event_for_principal(
    event: &RoomEvent,
    principal: &AuthenticatedPrincipal,
) -> RoomEvent {
    let mut projected = event.clone();
    projected.extra = project_map(&event.extra);
    let mut projected = privacy_minimized_vote_transition(projected);
    if !room_event_is_owner_only(&projected) || event_is_visible_to(&projected, principal) {
        if room_event_is_owner_only(&projected) {
            projected.extra.remove("audience");
            projected
                .extra
                .insert("visibility".to_owned(), json!("owner"));
        }
        return projected;
    }
    RoomEvent {
        v: projected.v,
        id: projected.id,
        seq: projected.seq,
        created_at: projected.created_at,
        room_id: projected.room_id,
        event_type: "event_hidden".to_owned(),
        actor: Actor {
            participant_id: String::new(),
            participant_type: String::new(),
        },
        participant_id: None,
        participant_type: None,
        actor_id: None,
        actor_type: None,
        display_name: None,
        content: None,
        message_kind: None,
        extra: BTreeMap::from([("visibility".to_owned(), json!("owner"))]),
    }
}

/// Removes private fields from a public command result and projects embedded events.
///
/// # Errors
///
/// Returns a serialization error if a recognized embedded event cannot be encoded.
pub fn public_value_for_principal(
    value: &Value,
    principal: &AuthenticatedPrincipal,
) -> Result<Value, serde_json::Error> {
    if let Ok(event) = serde_json::from_value::<RoomEvent>(value.clone()) {
        return serde_json::to_value(public_event_for_principal(&event, principal));
    }
    Ok(match value {
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| public_value_for_principal(value, principal))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Object(values) => {
            let mut projected = Map::new();
            for (key, value) in values.iter().filter(|(key, _)| !is_private_key(key)) {
                projected.insert(key.clone(), public_value_for_principal(value, principal)?);
            }
            Value::Object(projected)
        }
        _ => value.clone(),
    })
}

fn project_map(values: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    values
        .iter()
        .filter(|(key, _)| !is_private_key(key))
        .map(|(key, value)| (key.clone(), project_value(value)))
        .collect()
}

fn project_value(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(project_value).collect()),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .filter(|(key, _)| !is_private_key(key))
                .map(|(key, value)| (key.clone(), project_value(value)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn is_private_key(key: &str) -> bool {
    PRIVATE_PUBLIC_KEYS.contains(&key)
}

/// Returns whether an event is restricted to its owning principal.
#[must_use]
pub fn room_event_is_owner_only(event: &RoomEvent) -> bool {
    ["visibility", "audience"].iter().any(|key| {
        event
            .extra
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(|value| value == "owner")
    })
}

fn event_is_visible_to(event: &RoomEvent, principal: &AuthenticatedPrincipal) -> bool {
    let participant = event.participant_id.as_deref().unwrap_or_default();
    let owner = event
        .extra
        .get("owner_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    [participant, owner].iter().any(|candidate| {
        !candidate.is_empty()
            && (*candidate == principal.principal_id || *candidate == principal.participant_id)
    })
}

/// Keeps only the canonical public marker for a vote transition.
///
/// The current ballot projection owns participant identity and choice. Durable history and command
/// replay need only the vote ID to refresh the corresponding poll card, so storing those private
/// fields would create deletion work without a reachable product consumer.
#[must_use]
pub fn privacy_minimized_vote_transition(mut event: RoomEvent) -> RoomEvent {
    let private_vote = event
        .message_kind
        .as_deref()
        .is_some_and(|kind| matches!(kind, "vote_cast" | "vote_withdraw" | "vote_close"));
    if !private_vote {
        return event;
    }
    event.actor.participant_id.clear();
    event.actor.participant_type.clear();
    event.participant_id = None;
    event.participant_type = None;
    event.actor_id = None;
    event.actor_type = None;
    event.display_name = None;
    event.content = Some(String::new());
    event.extra.retain(|key, _| key == "vote_id");
    event
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Utc;
    use serde_json::json;

    use super::{public_event_for_principal, public_value_for_principal};
    use crate::{Actor, AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, RoomEvent};

    fn principal(id: &str) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            principal_id: id.to_owned(),
            participant_id: format!("participant-{id}"),
            display_name: id.to_owned(),
            room_id: "room".to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: false,
            capabilities: CapabilitySet::for_principal(
                ClientKind::Browser,
                InviteScope::ReadWrite,
                false,
            ),
        }
    }

    fn owner_event() -> RoomEvent {
        RoomEvent {
            v: 1,
            id: "event-1".to_owned(),
            seq: 7,
            created_at: Utc::now(),
            room_id: "room".to_owned(),
            event_type: "provider_request_opened".to_owned(),
            actor: Actor {
                participant_id: "agent".to_owned(),
                participant_type: "agent".to_owned(),
            },
            participant_id: Some("participant-owner".to_owned()),
            participant_type: Some("human".to_owned()),
            actor_id: Some("agent".to_owned()),
            actor_type: Some("agent".to_owned()),
            display_name: Some("Owner".to_owned()),
            content: Some("private".to_owned()),
            message_kind: None,
            extra: BTreeMap::from([
                ("visibility".to_owned(), json!("owner")),
                ("provider_turn_id".to_owned(), json!("private-turn")),
                ("workspace".to_owned(), json!("/private/workspace")),
            ]),
        }
    }

    #[test]
    fn non_owner_gets_hidden_event_with_same_cursor() {
        let projected = public_event_for_principal(&owner_event(), &principal("viewer"));
        assert_eq!(projected.event_type, "event_hidden");
        assert_eq!(projected.seq, 7);
        assert_eq!(projected.id, "event-1");
        assert_eq!(projected.room_id, "room");
        assert!(projected.content.is_none());
    }

    #[test]
    fn owner_gets_redacted_event_and_command_result() {
        let projected = public_event_for_principal(&owner_event(), &principal("owner"));
        assert_eq!(projected.event_type, "provider_request_opened");
        assert!(!projected.extra.contains_key("provider_turn_id"));
        assert!(!projected.extra.contains_key("workspace"));
        let result = public_value_for_principal(
            &json!({
                "event": owner_event(),
                "agent_session": {
                    "session_id": "agent",
                    "runtime_handle_id": "private-handle",
                    "nested": {"provider_session_id": "private-session"}
                }
            }),
            &principal("owner"),
        )
        .unwrap_or_else(|error| panic!("project command result: {error}"));
        assert!(result["agent_session"].get("runtime_handle_id").is_none());
        assert!(
            result["agent_session"]["nested"]
                .get("provider_session_id")
                .is_none()
        );
    }

    #[test]
    fn agent_bridge_never_receives_private_ballot_identity_or_choice() {
        let mut bridge = principal("bridge");
        bridge.client_kind = ClientKind::AgentBridge;
        bridge.capabilities =
            CapabilitySet::for_principal(ClientKind::AgentBridge, InviteScope::ReadWrite, false);
        let mut event = owner_event();
        event.event_type = "message_final".to_owned();
        event.message_kind = Some("vote_cast".to_owned());
        event.extra.remove("visibility");
        event.extra.insert("vote_id".to_owned(), json!("poll-1"));
        event
            .extra
            .insert("vote_choice".to_owned(), json!("Secret"));

        let projected = public_event_for_principal(&event, &bridge);
        assert!(projected.actor.participant_id.is_empty());
        assert!(projected.participant_id.is_none());
        assert!(projected.display_name.is_none());
        assert!(!projected.extra.contains_key("vote_choice"));
        assert_eq!(projected.extra.get("vote_id"), Some(&json!("poll-1")));
    }
}
