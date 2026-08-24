use std::collections::BTreeMap;

use agentsassemble_domain::{AuthenticatedPrincipal, DurableAgentSession, Participant};
use serde_json::json;
use sqlx::{Sqlite, Transaction};

use crate::{
    CommandOutcome, PersistenceError,
    agent_lifecycle_events::{append_session_event, append_state_event, store_result},
};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn commit_launch_result(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload_hash: String,
    session: &DurableAgentSession,
    participant: &Participant,
    joined: bool,
    runtime_reused: bool,
    command_action: &'static str,
) -> Result<CommandOutcome, PersistenceError> {
    let events = append_launch_events(transaction, principal, session, participant, joined).await?;
    let result = launch_result(session, runtime_reused, &events);
    store_result(
        transaction,
        principal,
        request_id,
        command_action,
        payload_hash,
        result,
        events,
    )
    .await
}

pub(crate) async fn append_launch_events(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    session: &DurableAgentSession,
    participant: &Participant,
    joined: bool,
) -> Result<Vec<agentsassemble_domain::RoomEvent>, PersistenceError> {
    let mut events = Vec::with_capacity(3);
    if joined {
        events.push(
            append_session_event(
                transaction,
                principal,
                &session.public,
                "participant_joined",
                BTreeMap::from([("participant".to_owned(), json!(participant))]),
            )
            .await?,
        );
    }
    events.push(
        append_session_event(
            transaction,
            principal,
            &session.public,
            "session_attached",
            BTreeMap::new(),
        )
        .await?,
    );
    events.push(append_state_event(transaction, principal, &session.public).await?);
    Ok(events)
}

pub(crate) fn launch_result(
    session: &DurableAgentSession,
    runtime_reused: bool,
    events: &[agentsassemble_domain::RoomEvent],
) -> serde_json::Value {
    json!({
        "agent_session": session.public,
        "runtime_reused": runtime_reused,
        "events": events,
        "event": events.last(),
    })
}
