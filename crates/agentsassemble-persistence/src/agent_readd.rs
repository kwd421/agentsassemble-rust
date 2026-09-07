use std::collections::BTreeMap;

use agentsassemble_domain::{
    AgentRuntimeStatus, AgentSessionStatus, AuthenticatedPrincipal, DurableAgentSession,
    Participant, ParticipantStatus,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};

use crate::{
    CommandOutcome, PersistenceError,
    agent_lifecycle::{save_participant, save_session},
    agent_lifecycle_authority::{payload_agent_id, payload_agent_id_with_fields},
    agent_lifecycle_events::{append_session_event, store_result},
};

pub(crate) const READD: &str = "agent.readd";

pub(crate) fn launch_payload(
    payload: &Value,
    action: &str,
) -> Result<(String, bool), PersistenceError> {
    match action {
        "agent.start" | "agent.resume" => payload_agent_id(payload).map(|id| (id, true)),
        READD => {
            let id = payload_agent_id_with_fields(payload, &["start", "start_now"])?;
            let mut start = None;
            for name in ["start", "start_now"] {
                if let Some(value) = payload.get(name) {
                    let value = value
                        .as_bool()
                        .ok_or_else(|| rejected("bad_request", "start must be a boolean."))?;
                    if start.is_some_and(|previous| previous != value) {
                        return Err(rejected("bad_request", "start aliases conflict."));
                    }
                    start = Some(value);
                }
            }
            Ok((id, start.unwrap_or(false)))
        }
        _ => Err(rejected(
            "bad_request",
            "The Agent Session launch action is invalid.",
        )),
    }
}

pub(crate) fn require_readdable(
    session: &DurableAgentSession,
    participant: &Participant,
) -> Result<(), PersistenceError> {
    if session.public.external_owned
        || session.public.process_ownership != "server"
        || participant.participant_type != "agent"
    {
        return Err(rejected(
            "runtime_unavailable",
            "Only server-owned Agent Sessions can be added back.",
        ));
    }
    if session.public.enabled
        || session.public.recovery_required
        || session.public.provider_session_active
        || !matches!(
            session.public.runtime_status,
            AgentRuntimeStatus::Stopped | AgentRuntimeStatus::Error
        )
        || !matches!(
            participant.status,
            ParticipantStatus::Detached | ParticipantStatus::Kicked
        )
        || !session.public.active_turn_id.is_empty()
        || !session.runtime_handle_id.is_empty()
        || !session.runtime_owner_id.is_empty()
        || !session.runtime_lease_token.is_empty()
        || !session.inflight_inputs.is_empty()
    {
        return Err(rejected(
            "readd_invalid_state",
            "The Agent Session must have confirmed inactive custody before re-add.",
        ));
    }
    Ok(())
}

pub(crate) async fn commit_readd_listing(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload_hash: String,
    session: &mut DurableAgentSession,
    participant: &mut Participant,
) -> Result<CommandOutcome, PersistenceError> {
    session.public.status = AgentSessionStatus::Available;
    session.public.runtime_status = AgentRuntimeStatus::Stopped;
    session.public.last_error.clear();
    session.public.last_error_code.clear();
    session.public.updated_at = Utc::now();
    participant.status = ParticipantStatus::Detached;
    participant.updated_at = session.public.updated_at;
    save_session(transaction, session).await?;
    save_participant(transaction, participant).await?;
    let event = append_session_event(
        transaction,
        principal,
        &session.public,
        "agent_session_reactivated",
        BTreeMap::from([
            ("session_id".to_owned(), json!(session.public.session_id)),
            ("agent_session".to_owned(), json!(session.public)),
            ("participant".to_owned(), json!(participant)),
        ]),
        chrono::Utc::now(),
    )
    .await?;
    let events = vec![event];
    let result = json!({
        "status": "readded",
        "agent_session": session.public,
        "participant": participant,
        "events": events,
        "event": events.last(),
    });
    store_result(
        transaction,
        principal,
        request_id,
        READD,
        payload_hash,
        result,
        events,
    )
    .await
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}
