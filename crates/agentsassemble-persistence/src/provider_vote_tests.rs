use agentsassemble_domain::VoteCommand;
use serde_json::json;

use super::{AGENT_ID, authority, event_types, fixture, running_authority, stored_session};
use crate::{ProviderTurnExecutionPhase, ProviderTurnStartAuthority, SqliteStore};

#[tokio::test]
async fn provider_vote_actions_share_one_exact_terminal_turn_and_projection() {
    let (store, principal, _directory) = fixture().await;
    let vote_id = create_provider_vote(&store, &principal).await;
    cast_provider_vote(&store, &principal, &vote_id).await;
    close_provider_vote(&store, &principal, &vote_id).await;
}

#[tokio::test]
async fn closed_vote_rejection_terminalizes_the_exact_provider_turn() {
    let (store, principal, _directory) = fixture().await;
    let vote_id = create_provider_vote(&store, &principal).await;
    let cast_start = assigned_turn(
        &store,
        &principal,
        "provider-vote-race-input",
        "@Terra cast after the poll changes",
        "provider-vote-race",
    )
    .await;
    store
        .execute_message(
            &principal,
            "provider-vote-race-close",
            "message.send",
            &json!({"kind": "vote_close", "vote_id": vote_id}),
        )
        .await
        .unwrap_or_else(|error| panic!("close vote during provider turn: {error}"));
    let cast = VoteCommand::from_payload(&json!({
        "kind": "vote_cast",
        "vote_id": vote_id,
        "vote_choice": "Yes"
    }))
    .unwrap_or_else(|error| panic!("parse stale provider cast: {error}"));

    let commit = store
        .complete_agent_vote_turn(
            "general",
            AGENT_ID,
            authority(&cast_start, "provider-vote-race", None),
            cast,
        )
        .await
        .unwrap_or_else(|error| panic!("terminalize rejected provider cast: {error}"));

    assert_eq!(
        event_types(&commit.events),
        ["error", "turn_finished", "agent_session_state"]
    );
    assert_eq!(commit.events[0].extra["error_code"], json!("vote_closed"));
    assert_eq!(commit.events[1].extra["status"], json!("error"));
    assert_eq!(commit.events[1].extra["reason_code"], json!("vote_closed"));
    assert!(commit.next_assignments.is_empty());
    let session = stored_session(&store).await;
    assert_eq!(session.public.status, "attached");
    assert_eq!(session.public.runtime_status, "idle");
    assert!(!session.public.recovery_required);
    assert!(session.public.active_turn_id.is_empty());
    assert_eq!(
        store
            .provider_turn_execution("general", AGENT_ID, cast_start.turn_generation)
            .await
            .unwrap_or_else(|error| panic!("read rejected provider execution: {error}"))
            .phase,
        ProviderTurnExecutionPhase::Failed
    );
    assert!(
        store
            .load_active_provider_turn_reconciliation_candidate("general", AGENT_ID)
            .await
            .unwrap_or_else(|error| panic!("check rejected provider reconciliation: {error}"))
            .is_none()
    );
}

async fn create_provider_vote(
    store: &SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
) -> String {
    let create_start = assigned_turn(
        store,
        principal,
        "provider-vote-create-input",
        "@Terra create the poll",
        "provider-vote-create",
    )
    .await;
    let create = VoteCommand::from_payload(&json!({
        "kind": "vote",
        "vote_question": "Ship the provider tools?",
        "vote_options": ["Yes", "No"],
        "vote_duration_seconds": 300
    }))
    .unwrap_or_else(|error| panic!("parse provider vote: {error}"));
    let commit = store
        .complete_agent_vote_turn(
            "general",
            AGENT_ID,
            authority(&create_start, "provider-vote-create", None),
            create,
        )
        .await
        .unwrap_or_else(|error| panic!("complete provider vote create: {error}"));
    assert_eq!(
        event_types(&commit.events),
        ["message_final", "turn_finished", "agent_session_state"]
    );
    let poll = &commit.events[0];
    assert_eq!(poll.actor.participant_id, AGENT_ID);
    assert_eq!(poll.message_kind.as_deref(), Some("vote"));
    poll.id.clone()
}

async fn cast_provider_vote(
    store: &SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    vote_id: &str,
) {
    let cast_start = assigned_turn(
        store,
        principal,
        "provider-vote-cast-input",
        "@Terra cast the ballot",
        "provider-vote-cast",
    )
    .await;
    let cast = VoteCommand::from_payload(&json!({
        "kind": "vote_cast",
        "vote_id": vote_id,
        "vote_choice": "yes"
    }))
    .unwrap_or_else(|error| panic!("parse provider cast: {error}"));
    let casted = store
        .complete_agent_vote_turn(
            "general",
            AGENT_ID,
            authority(&cast_start, "provider-vote-cast", None),
            cast,
        )
        .await
        .unwrap_or_else(|error| panic!("complete provider cast: {error}"));
    assert_eq!(casted.events[0].message_kind.as_deref(), Some("vote_cast"));
    assert_eq!(casted.events[0].extra["vote_choice"], json!("Yes"));
    let summary = store
        .local_room_vote_summary(
            "general",
            &principal.principal_id,
            &principal.participant_id,
            vote_id,
        )
        .await
        .unwrap_or_else(|error| panic!("read provider vote summary: {error}"));
    assert_eq!(summary.tallies["Yes"], 1);
    assert_eq!(summary.own_choice, "");
}

async fn close_provider_vote(
    store: &SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    vote_id: &str,
) {
    let close_start = assigned_turn(
        store,
        principal,
        "provider-vote-close-input",
        "@Terra close your poll",
        "provider-vote-close",
    )
    .await;
    let close = VoteCommand::from_payload(&json!({
        "kind": "vote_close",
        "vote_id": vote_id
    }))
    .unwrap_or_else(|error| panic!("parse provider close: {error}"));
    store
        .complete_agent_vote_turn(
            "general",
            AGENT_ID,
            authority(&close_start, "provider-vote-close", None),
            close,
        )
        .await
        .unwrap_or_else(|error| panic!("complete provider close: {error}"));
    let summary = store
        .local_room_vote_summary(
            "general",
            &principal.principal_id,
            &principal.participant_id,
            vote_id,
        )
        .await
        .unwrap_or_else(|error| panic!("read closed provider vote: {error}"));
    assert!(summary.closed);
    assert_eq!(summary.close_reason, "manual");
}

async fn assigned_turn(
    store: &SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    request_id: &str,
    content: &str,
    provider_turn_id: &str,
) -> ProviderTurnStartAuthority {
    let mutation = store
        .execute_message_with_turn(
            principal,
            request_id,
            "message.send",
            &json!({"content": content}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign provider vote turn: {error}"));
    let assignment = mutation
        .assignments
        .first()
        .unwrap_or_else(|| panic!("provider vote input must assign Terra"));
    running_authority(store, assignment, provider_turn_id).await
}
