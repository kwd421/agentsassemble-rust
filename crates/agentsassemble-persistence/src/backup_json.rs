//! Field decoding at the hostile backup boundary. Existing domain deserializers
//! remain the schema owners; `serde_ignored` closes permissive nested structs.
use crate::PersistenceError;
use agentsassemble_domain::{AgentSession, DurableAgentSession, RoomEvent};
use serde::{
    Serialize,
    de::{DeserializeOwned, IntoDeserializer},
};
use serde_json::Value;

pub(crate) fn invalid() -> PersistenceError {
    PersistenceError::InvalidBackup
}

pub(crate) fn strict<T: DeserializeOwned>(value: Value) -> Result<T, PersistenceError> {
    let mut unknown = false;
    let decoded = serde_ignored::deserialize(value.into_deserializer(), |_| unknown = true)
        .map_err(|_| invalid())?;
    if unknown {
        return Err(invalid());
    }
    Ok(decoded)
}

fn canonical<T: DeserializeOwned + Serialize>(value: &mut Value) -> Result<(), PersistenceError> {
    *value = serde_json::to_value(strict::<T>(value.take())?)?;
    Ok(())
}

// RoomEvent's flattened public extension map has an explicit import allowlist.
// Structured values are decoded by their producer's type; scalar extension fields
// cannot smuggle another JSON object. New producer fields must be classified here.
pub(crate) fn event(value: Value) -> Result<RoomEvent, PersistenceError> {
    let mut event: RoomEvent = strict(value)?;
    if event.v != 1 || event.seq < 1 || event.room_id.is_empty() || event.id.is_empty() {
        return Err(invalid());
    }
    for (field, value) in &mut event.extra {
        match field.as_str() {
            "actor_participant_id"
            | "on_behalf_of_participant_id"
            | "action"
            | "target_id"
            | "request_id"
            | "target_channel_id" => {
                strict::<String>(value.clone())?;
            }
            "pinned" => {
                strict::<bool>(value.clone())?;
            }

            "participant" => canonical::<agentsassemble_domain::Participant>(value)?,
            "room" => canonical::<agentsassemble_domain::Room>(value)?,
            "room_settings" => canonical::<agentsassemble_domain::PublicRoomSettings>(value)?,
            "settings" => canonical::<agentsassemble_domain::RoomSettings>(value)?,
            "agent_session" => canonical::<AgentSession>(value)?,
            "provider_request" => canonical::<agentsassemble_domain::ProviderRequest>(value)?,
            "resolution" => canonical::<agentsassemble_domain::DurableProviderResolution>(value)?,
            "attachments" => canonical::<Vec<crate::MessageAttachmentMetadata>>(value)?,
            "vote_options" => canonical::<Vec<String>>(value)?,
            "details" => exact_scalar_object(
                value,
                &[
                    "notation",
                    "rolls",
                    "modifier",
                    "total",
                    "choice",
                    "index",
                    "option_count",
                ],
            )?,
            "diagnostics" => exact_scalar_object(value, &["assignment_error_code"])?,
            "session_id"
            | "provider_kind"
            | "attendee_invite_id"
            | "runtime_status"
            | "error_code"
            | "message_source"
            | "phase"
            | "provider_context_up_to_seq"
            | "provider_turn_id"
            | "reason_code"
            | "reply_to_event_id"
            | "source_event_id"
            | "status"
            | "target_agent_id"
            | "turn_id"
            | "muted"
            | "role"
            | "label"
            | "avatar_image_url"
            | "avatar_label"
            | "profile_revision"
            | "reason"
            | "expires_at"
            | "owner_id"
            | "visibility"
            | "provider_request_id"
            | "state"
            | "operation"
            | "room_result_id"
            | "room_result_kind"
            | "source_participant_id"
            | "source_turn_id"
            | "channel_id"
            | "vote_choice"
            | "vote_deadline_at"
            | "vote_duration_seconds"
            | "vote_id"
            | "vote_question"
            | "deleted_at"
            | "edited_at"
            | "message_deleted"
            | "target_event_id"
            | "target_seq" => {
                if value.is_object() || value.is_array() {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
    }
    Ok(event)
}

fn exact_scalar_object(value: &Value, fields: &[&str]) -> Result<(), PersistenceError> {
    let object = value.as_object().ok_or_else(invalid)?;
    for (key, value) in object {
        if !fields.contains(&key.as_str())
            || value.is_object()
            || value
                .as_array()
                .is_some_and(|items| items.iter().any(|v| v.is_object() || v.is_array()))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(crate) fn stop_agent(session: &mut DurableAgentSession) {
    use agentsassemble_domain::{AgentRuntimeStatus, AgentSessionStatus};
    for field in [
        &mut session.executable,
        &mut session.executable_identity,
        &mut session.workspace,
        &mut session.workspace_identity,
        &mut session.provider_endpoint,
        &mut session.runtime_profile_key,
        &mut session.provider_session_id,
        &mut session.runtime_handle_id,
        &mut session.runtime_owner_id,
        &mut session.runtime_lease_token,
        &mut session.lifecycle_intent_id,
        &mut session.public.active_turn_id,
    ] {
        field.clear();
    }
    session.schedule_requested = false;
    session.lifecycle_intent_action = agentsassemble_domain::AgentLifecycleAction::default();
    session.lifecycle_intent_status = agentsassemble_domain::AgentLifecycleIntentStatus::default();
    session.public.turn_phase = agentsassemble_domain::AgentTurnPhase::default();
    if session.public.status != AgentSessionStatus::Detached {
        session.public.status = AgentSessionStatus::Unavailable;
    }
    session.public.runtime_status = AgentRuntimeStatus::Stopped;
    session.public.enabled = false;
    session.public.provider_session_active = false;
    session.public.provider_session_reused = false;
    session.public.external_retained_interrupt = None;
    session.public.execution_os = None;
    // Retain unfinished inputs for explicit inspection/restart, never schedule them.
    session.public.recovery_required = true;
    session.public.last_error_code = "server_restored".into();
    session.public.last_error =
        "복원한 AI예요. 로그인과 작업 폴더를 확인한 뒤 직접 시작해 주세요.".into();
}

pub(crate) fn admission(mut value: Value) -> Result<Value, PersistenceError> {
    let mut result: crate::HumanAdmissionResult = strict(value.take())?;
    let guide = result.guide.as_object().ok_or_else(invalid)?;
    if guide
        .keys()
        .any(|k| !["welcome", "how_to", "etiquette", "session"].contains(&k.as_str()))
    {
        return Err(invalid());
    }
    // Account removal deliberately erases identifying usage guidance to {}.
    // That exact redacted shape is product data; partial/unknown guides still fail.
    if !guide.is_empty() {
        for field in ["how_to", "etiquette"] {
            strict::<Vec<String>>(guide.get(field).cloned().ok_or_else(invalid)?)?;
        }
        strict::<String>(guide.get("welcome").cloned().ok_or_else(invalid)?)?;
        exact_scalar_object(
            guide.get("session").ok_or_else(invalid)?,
            &["expires_in_seconds", "rejoin"],
        )?;
    }
    result.request_id.clear();
    result.client_id.clear();
    result.expires_at = chrono::DateTime::UNIX_EPOCH;
    Ok(serde_json::to_value(result)?)
}

// Idempotent command receipts preserve outcomes, including deletion tombstones.
// They cannot carry live credentials or pending lifecycle effects across a move.
pub(crate) fn receipt(mut value: Value) -> Result<Value, PersistenceError> {
    let object = value.as_object_mut().ok_or_else(invalid)?;
    for (field, value) in object {
        match field.as_str() {
            "target_id" | "event_id" => {
                strict::<String>(value.clone())?;
            }

            "start" => {
                // Creation stores one canonical launch receipt, not another creation.
                if value.get("start").is_some() {
                    return Err(invalid());
                }
                *value = receipt(value.take())?;
            }
            "process" => {
                exact_scalar_object(
                    value,
                    &[
                        "stopped",
                        "alive",
                        "ownership",
                        "already_stopped",
                        "confirmed",
                    ],
                )?;
                strict::<bool>(value.get("stopped").cloned().ok_or_else(invalid)?)?;
                strict::<bool>(value.get("alive").cloned().ok_or_else(invalid)?)?;
                strict::<String>(value.get("ownership").cloned().ok_or_else(invalid)?)?;
                for field in ["already_stopped", "confirmed"] {
                    if let Some(value) = value.get(field) {
                        strict::<bool>(value.clone())?;
                    }
                }
            }
            "cleanup_pending" | "runtime_reused" => {
                strict::<bool>(value.clone())?;
            }
            "revoked_sessions" => {
                strict::<u64>(value.clone())?;
            }
            "event" => *value = serde_json::to_value(event(value.take())?)?,
            "events" => {
                let items = value.as_array_mut().ok_or_else(invalid)?;
                for item in items {
                    *item = serde_json::to_value(event(item.take())?)?;
                }
            }
            "agent_session" => canonical::<AgentSession>(value)?,
            "participant" => canonical::<agentsassemble_domain::Participant>(value)?,
            "room" => canonical::<agentsassemble_domain::Room>(value)?,
            "room_settings" => canonical::<agentsassemble_domain::PublicRoomSettings>(value)?,
            "settings" => canonical::<agentsassemble_domain::RoomSettings>(value)?,
            "tool_result" => canonical::<agentsassemble_domain::RoomRandomResult>(value)?,
            "vote" => canonical::<agentsassemble_domain::VoteSummary>(value)?,
            "status" | "room_id" | "room_uid" | "event_seq" | "session_id" | "participant_id"
            | "deduplicated" | "request_id" | "state" | "deleted" | "settings_revision"
            | "action" | "error_code" | "message" | "stopped" | "reason" | "created"
            | "target_event_id" | "target_seq" | "profile_revision" | "muted" | "role"
            | "accepted" | "pending" | "operation_id" | "runtime_status" | "removed" | "left"
            | "detached" | "enabled" | "success" | "scheduled" | "paused" => {
                if value.is_object() || value.is_array() {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lifecycle_receipts_retain_outcomes_without_admitting_private_extensions()
    -> Result<(), PersistenceError> {
        let stopped = json!({"process":{"stopped":true,"alive":false,"ownership":"server","confirmed":true},"revoked_sessions":0});
        assert_eq!(receipt(stopped.clone())?, stopped);
        for value in [
            json!({"process":{"stopped":true,"alive":false,"ownership":"server","secret":"no"}}),
            json!({"process":{"stopped":[],"alive":false,"ownership":"server"}}),
            json!({"start":{"start":{}}}),
            json!({"start":{"secret":"no"}}),
            json!({"revoked_sessions":"credential"}),
            json!({"cleanup_pending":{"secret":"no"}}),
        ] {
            assert!(receipt(value).is_err());
        }
        Ok(())
    }
}
