use std::collections::BTreeMap;

use serde::Deserialize;
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
    let owner_card = delegated_action_request_card(event);
    let mut projected = event.clone();
    projected.extra.retain(|key, _| !is_private_key(key));
    projected.extra.values_mut().for_each(project_value);
    let mut projected = privacy_minimized_vote_transition(projected);
    if !room_event_is_owner_only(&projected) || event_is_visible_to(&projected, principal) {
        if room_event_is_owner_only(&projected) {
            projected.extra.remove("audience");
            projected
                .extra
                .insert("visibility".to_owned(), json!("owner"));
            if let Some(card) = owner_card {
                projected
                    .extra
                    .insert("delegated_action_request".to_owned(), card.clone());
            }
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
    if let Ok(event) = RoomEvent::deserialize(value) {
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

fn project_value(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(project_value),
        Value::Object(values) => {
            values.retain(|key, _| !is_private_key(key));
            values.values_mut().for_each(project_value);
        }
        _ => {}
    }
}

fn is_private_key(key: &str) -> bool {
    PRIVATE_PUBLIC_KEYS.contains(&key)
}

/// Returns the stored confirmation card of the two delegated confirmation event types.
///
/// The card's catalog-selected draft and request params carry authority fields that
/// `PRIVATE_PUBLIC_KEYS` removes from every other projection. These two event types are
/// delivered to exactly one viewer - the bound owner's browser client, per
/// `event_is_visible_to` - whose client validates the whole request, so the owner-only
/// branch below restores the stored request verbatim for that viewer alone. Every other
/// viewer still receives `event_hidden` with nothing from the card.
fn delegated_action_request_card(event: &RoomEvent) -> Option<&Value> {
    if !matches!(
        event.event_type.as_str(),
        "delegated_action_requested" | "delegated_action_updated"
    ) {
        return None;
    }
    event.extra.get("delegated_action_request")
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
    if matches!(
        event.event_type.as_str(),
        "delegated_action_requested" | "delegated_action_updated"
    ) {
        return principal.client_kind == crate::ClientKind::Browser
            && event.extra.get("owner_id").and_then(Value::as_str)
                == Some(principal.participant_id.as_str());
    }
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

    use chrono::{Duration, Utc};
    use serde_json::json;

    use super::{public_event_for_principal, public_value_for_principal};
    use crate::{
        Actor, AgentSessionDraft, AuthenticatedPrincipal, CapabilitySet, ClientKind,
        ConfirmationState, ConfirmedAgentCreate, ConfirmedRoomAction, DelegatedActionRequest,
        InviteScope, RequestCreateAgent, RoomEvent,
    };

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
            event_type: "message_final".to_owned(),
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

    fn agent_bridge(id: &str) -> AuthenticatedPrincipal {
        let mut bridge = principal(id);
        bridge.client_kind = ClientKind::AgentBridge;
        bridge.capabilities =
            CapabilitySet::for_principal(ClientKind::AgentBridge, InviteScope::ReadWrite, false);
        bridge
    }

    fn delegated_create_card() -> DelegatedActionRequest {
        DelegatedActionRequest {
            confirmation_id: "confirmation-1".to_owned(),
            owner_participant_id: "participant-owner".to_owned(),
            owner_display_name: "Owner".to_owned(),
            delegate_participant_id: "participant-director".to_owned(),
            delegate_display_name: "Director".to_owned(),
            action: ConfirmedRoomAction::CreateAgent(ConfirmedAgentCreate {
                request_id: "request-1".to_owned(),
                params: RequestCreateAgent {
                    request_id: "request-1".to_owned(),
                    provider_id: "codex".to_owned(),
                    catalog_revision: "catalog-1".to_owned(),
                    display_name: "Researcher".to_owned(),
                    workspace: "/private/workspace".to_owned(),
                    model: Some("gpt-5".to_owned()),
                    reasoning_effort: None,
                    service_tier: None,
                    variant: None,
                    permission_mode: Some("default".to_owned()),
                    persona_card_id: None,
                    provider_endpoint: Some("https://provider.internal".to_owned()),
                    max_output_tokens: None,
                },
                draft: AgentSessionDraft {
                    agent_id: "agent-1".to_owned(),
                    display_name: "Researcher".to_owned(),
                    provider_kind: "codex".to_owned(),
                    runtime_kind: "cli".to_owned(),
                    connection_kind: "local".to_owned(),
                    executable: "/private/bin/codex".to_owned(),
                    executable_identity: "/private/bin/codex".to_owned(),
                    workspace: "/private/workspace".to_owned(),
                    workspace_identity: "/private/workspace-identity".to_owned(),
                    provider_endpoint: "https://provider.internal".to_owned(),
                    model: "gpt-5".to_owned(),
                    reasoning_effort: "medium".to_owned(),
                    service_tier: "default".to_owned(),
                    variant: "default".to_owned(),
                    execution_harness: "cli".to_owned(),
                    permission_mode: "default".to_owned(),
                    max_output_tokens: 8192,
                    catalog_revision: "catalog-1".to_owned(),
                    persona_card_id: "persona-1".to_owned(),
                    runtime_profile_key: "runtime-profile-key-1".to_owned(),
                    transport: "stdio".to_owned(),
                },
            }),
            target_name: "Researcher".to_owned(),
            consequences: "Creates a stopped agent.".to_owned(),
            created_at: Utc::now(),
            expires_at: Utc::now() + Duration::seconds(60),
            state: ConfirmationState::Pending,
        }
    }

    fn delegated_action_event() -> RoomEvent {
        let card = delegated_create_card();
        RoomEvent {
            v: 1,
            id: "event-2".to_owned(),
            seq: 8,
            created_at: Utc::now(),
            room_id: "room".to_owned(),
            event_type: "delegated_action_requested".to_owned(),
            actor: Actor {
                participant_id: "participant-director".to_owned(),
                participant_type: "agent".to_owned(),
            },
            participant_id: Some("participant-owner".to_owned()),
            participant_type: Some("human".to_owned()),
            actor_id: Some("participant-director".to_owned()),
            actor_type: Some("agent".to_owned()),
            display_name: None,
            content: None,
            message_kind: None,
            extra: serde_json::from_value(json!({
                "visibility": "owner",
                "owner_id": "participant-owner",
                "delegated_action_request": card,
            }))
            .unwrap_or_else(|error| panic!("delegated action event: {error}")),
        }
    }

    #[test]
    fn delegated_action_card_reaches_only_the_bound_owner_browser() {
        let event = delegated_action_event();
        let stored = event.extra["delegated_action_request"].clone();

        let projected = public_event_for_principal(&event, &principal("owner"));
        assert_eq!(projected.event_type, "delegated_action_requested");
        assert_eq!(projected.extra["delegated_action_request"], stored);
        let draft = &projected.extra["delegated_action_request"]["action"]["draft"];
        assert_eq!(draft["workspace"], json!("/private/workspace"));
        assert_eq!(draft["executable"], json!("/private/bin/codex"));
        assert_eq!(draft["runtime_profile_key"], json!("runtime-profile-key-1"));

        for viewer in [
            principal("viewer"),
            agent_bridge("director"),
            agent_bridge("owner"),
        ] {
            let hidden = public_event_for_principal(&event, &viewer);
            assert_eq!(
                hidden.event_type, "event_hidden",
                "viewer {}",
                viewer.principal_id
            );
            assert_eq!(hidden.extra.len(), 1);
            let serialized = serde_json::to_string(&hidden)
                .unwrap_or_else(|error| panic!("serialize hidden event: {error}"));
            for secret in [
                "/private/workspace",
                "/private/bin/codex",
                "/private/workspace-identity",
                "runtime-profile-key-1",
                "https://provider.internal",
            ] {
                assert!(
                    !serialized.contains(secret),
                    "{secret} leaked to {}",
                    viewer.principal_id
                );
            }
        }
    }

    #[test]
    fn non_delegated_owner_event_still_strips_the_card_keys() {
        let mut event = owner_event();
        event.extra.insert(
            "delegated_action_request".to_owned(),
            json!({
                "owner_participant_id": "participant-owner",
                "action": {
                    "action": "create_agent",
                    "draft": {
                        "workspace": "/private/workspace",
                        "runtime_profile_key": "runtime-profile-key-1",
                        "model": "gpt-5"
                    }
                }
            }),
        );

        let projected = public_event_for_principal(&event, &principal("owner"));
        assert_eq!(projected.event_type, "message_final");
        let draft = &projected.extra["delegated_action_request"]["action"]["draft"];
        assert!(draft.get("workspace").is_none());
        assert!(draft.get("runtime_profile_key").is_none());
        assert_eq!(draft["model"], json!("gpt-5"));
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
        let mut original = owner_event();
        original.extra.insert(
            "nested".to_owned(),
            json!({"items": [{"text": "retained", "token": "private"}]}),
        );
        let projected = public_event_for_principal(&original, &principal("owner"));
        assert_eq!(projected.event_type, "message_final");
        assert!(!projected.extra.contains_key("provider_turn_id"));
        assert!(!projected.extra.contains_key("workspace"));
        assert_eq!(
            projected.extra["nested"],
            json!({"items": [{"text": "retained"}]})
        );
        assert_eq!(original.extra["nested"]["items"][0]["token"], "private");
        let result = public_value_for_principal(
            &json!({
                "event": original,
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
