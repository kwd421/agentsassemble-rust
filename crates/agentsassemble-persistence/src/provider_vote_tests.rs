use agentsassemble_domain::{
    MAX_VOTE_BALLOTS_PER_POLL, Participant, ParticipantRole, ParticipantStatus, VoteCommand,
};
use chrono::Utc;
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

#[tokio::test]
async fn full_poll_terminalizes_provider_cast_without_a_phantom_transition() {
    let (store, principal, _directory) = fixture().await;
    let vote_id = create_provider_vote(&store, &principal).await;
    fill_current_ballots(&store, &vote_id).await;
    let cast_start = assigned_turn(
        &store,
        &principal,
        "provider-full-vote-input",
        "@Terra cast after the poll fills",
        "provider-full-vote",
    )
    .await;
    let before_completion =
        sqlx::query_scalar::<_, i64>("SELECT MAX(seq) FROM room_events WHERE room_id = 'general'")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("read sequence before full provider vote: {error}"));
    let cast = VoteCommand::from_payload(&json!({
        "kind": "vote_cast",
        "vote_id": vote_id,
        "vote_choice": "Yes"
    }))
    .unwrap_or_else(|error| panic!("parse full provider cast: {error}"));

    let commit = store
        .complete_agent_vote_turn(
            "general",
            AGENT_ID,
            authority(&cast_start, "provider-full-vote", None),
            cast,
        )
        .await
        .unwrap_or_else(|error| panic!("terminalize full provider cast: {error}"));

    assert_eq!(
        event_types(&commit.events),
        ["error", "turn_finished", "agent_session_state"]
    );
    assert_eq!(
        commit.events[0].extra["error_code"],
        json!("vote_capacity_reached")
    );
    let phantom_casts = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM room_events WHERE room_id = 'general' AND seq > ? AND json_extract(event_json, '$.message_kind') = 'vote_cast'",
    )
    .bind(before_completion)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("count rejected provider vote transitions: {error}"));
    assert_eq!(phantom_casts, 0);
    let summary = store
        .local_room_vote_summary(
            "general",
            &principal.principal_id,
            &principal.participant_id,
            &vote_id,
        )
        .await
        .unwrap_or_else(|error| panic!("read full vote summary: {error}"));
    assert_eq!(summary.total_votes, MAX_VOTE_BALLOTS_PER_POLL);
    assert_eq!(summary.tallies["Yes"], MAX_VOTE_BALLOTS_PER_POLL);
    assert_eq!(summary.tallies["No"], 0);
}

async fn fill_current_ballots(store: &SqliteStore, vote_id: &str) {
    let now = Utc::now();
    for index in 0..MAX_VOTE_BALLOTS_PER_POLL {
        let participant_id = format!("departed-voter-{index}");
        let participant = Participant {
            room_id: "general".to_owned(),
            participant_id: participant_id.clone(),
            display_name: "Departed voter".to_owned(),
            avatar_image_url: String::new(),
            participant_type: "human".to_owned(),
            status: ParticipantStatus::Left,
            role: ParticipantRole::Human,
            owner_id: format!("departed-user-{index}"),
            muted: false,
            created_at: now,
            updated_at: now,
        };
        sqlx::query(
            "INSERT INTO participants(room_id, participant_id, participant_json) VALUES ('general', ?, ?)",
        )
        .bind(&participant_id)
        .bind(
            serde_json::to_string(&participant)
                .unwrap_or_else(|error| panic!("encode departed voter: {error}")),
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert departed voter: {error}"));
        sqlx::query(
            "INSERT INTO room_vote_ballots(room_id, vote_id, participant_id, choice_index) VALUES ('general', ?, ?, 0)",
        )
        .bind(vote_id)
        .bind(&participant_id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert full vote ballot: {error}"));
    }
    sqlx::query(
        "UPDATE room_vote_states SET tallies_json = ?, total_votes = ? WHERE room_id = 'general' AND vote_id = ?",
    )
    .bind(format!("[{MAX_VOTE_BALLOTS_PER_POLL},0]"))
    .bind(i64::try_from(MAX_VOTE_BALLOTS_PER_POLL).unwrap_or(i64::MAX))
    .bind(vote_id)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("fill vote projection: {error}"));
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
    assert!(casted.events[0].actor.participant_id.is_empty());
    assert_eq!(casted.events[0].extra["vote_id"], vote_id);
    assert!(!casted.events[0].extra.contains_key("vote_choice"));
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
