use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, MAX_VOTE_BALLOTS_PER_POLL, Participant, ParticipantRole,
    ParticipantStatus, RoomEvent, VoteSummary, vote_deadline_at,
};
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use sqlx::Row;

use crate::{PersistenceError, SqliteStore};

#[tokio::test]
async fn ballot_replacement_withdrawal_and_replay_update_one_projection() {
    let (store, operator) = fixture().await;
    let created = command(
        &store,
        &operator,
        "10000000-0000-4000-8000-000000000001",
        json!({
            "kind": "vote",
            "vote_question": "Ship this?",
            "vote_options": ["Yes", "No"]
        }),
    )
    .await;
    let vote_id = created.outcome.event.id.clone();
    assert_eq!(created.outcome.event.message_kind.as_deref(), Some("vote"));
    assert!(created.assignments.is_empty());

    let replay = command(
        &store,
        &operator,
        "10000000-0000-4000-8000-000000000001",
        json!({
            "kind": "vote",
            "vote_question": "Ship this?",
            "vote_options": ["Yes", "No"]
        }),
    )
    .await;
    assert!(replay.outcome.deduplicated);
    assert_eq!(replay.outcome.event.id, vote_id);

    let first_cast = command(
        &store,
        &operator,
        "10000000-0000-4000-8000-000000000002",
        json!({"kind": "vote_cast", "vote_id": vote_id, "vote_choice": "yes"}),
    )
    .await;
    assert_vote_transition_is_minimized(&first_cast.outcome.event, &vote_id);
    let stored_transition = sqlx::query_scalar::<_, String>(
        "SELECT event_json FROM room_events WHERE room_id = ? AND seq = ?",
    )
    .bind(&first_cast.outcome.event.room_id)
    .bind(first_cast.outcome.event.seq)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read minimized vote transition: {error}"));
    assert_eq!(
        serde_json::from_str::<RoomEvent>(&stored_transition)
            .unwrap_or_else(|error| panic!("decode minimized vote transition: {error}")),
        first_cast.outcome.event,
    );
    let stored_result = sqlx::query_scalar::<_, String>(
        "SELECT result_json FROM command_results WHERE room_id = ? AND principal_id = ? AND request_id = ?",
    )
    .bind(&operator.room_id)
    .bind(&operator.principal_id)
    .bind("10000000-0000-4000-8000-000000000002")
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read minimized vote replay result: {error}"));
    let stored_result: Value = serde_json::from_str(&stored_result)
        .unwrap_or_else(|error| panic!("decode minimized vote replay result: {error}"));
    let replay_event: RoomEvent = serde_json::from_value(stored_result["event"].clone())
        .unwrap_or_else(|error| panic!("decode minimized replay event: {error}"));
    assert_vote_transition_is_minimized(&replay_event, &vote_id);
    assert_vote_state(&store, &[1, 0], 1, Some(0)).await;

    command(
        &store,
        &operator,
        "10000000-0000-4000-8000-000000000003",
        json!({"kind": "vote_cast", "vote_id": vote_id, "vote_choice": "2"}),
    )
    .await;
    assert_vote_state(&store, &[0, 1], 1, Some(1)).await;

    let withdrawn = command(
        &store,
        &operator,
        "10000000-0000-4000-8000-000000000004",
        json!({"kind": "vote_withdraw", "vote_id": vote_id}),
    )
    .await;
    assert!(withdrawn.assignments.is_empty());
    assert_vote_state(&store, &[0, 0], 0, None).await;
    assert_eq!(vote_event_count(&store).await, 4);
}

#[tokio::test]
async fn distinct_ballots_are_bounded_without_blocking_replacement_or_withdrawal() {
    let (store, operator) = fixture().await;
    let created = command(
        &store,
        &operator,
        "31000000-0000-4000-8000-000000000001",
        json!({
            "kind": "vote",
            "vote_question": "Bound this poll?",
            "vote_options": ["Yes", "No"]
        }),
    )
    .await;
    let vote_id = created.outcome.event.id;
    let mut voters = Vec::new();
    for index in 0..MAX_VOTE_BALLOTS_PER_POLL {
        let participant_id = format!("bounded-voter-{index}");
        let voter = add_human(&store, &participant_id, "Voter").await;
        command(
            &store,
            &voter,
            format!("31100000-0000-4000-8000-{index:012}").as_str(),
            json!({"kind": "vote_cast", "vote_id": vote_id, "vote_choice": "Yes"}),
        )
        .await;
        voters.push(voter);
    }
    let overflow = add_human(&store, "bounded-voter-overflow", "Overflow").await;
    let before_rejection = persisted_write_counts(&store).await;
    assert_rejected(
        &store
            .execute_message_with_turn(
                &overflow,
                "31000000-0000-4000-8000-999999999997",
                "message.send",
                &json!({"kind": "vote_cast", "vote_id": vote_id, "vote_choice": "No"}),
            )
            .await,
        "vote_capacity_reached",
    );
    assert_eq!(persisted_write_counts(&store).await, before_rejection);

    command(
        &store,
        &voters[0],
        "31000000-0000-4000-8000-999999999998",
        json!({"kind": "vote_cast", "vote_id": vote_id, "vote_choice": "No"}),
    )
    .await;
    command(
        &store,
        &voters[0],
        "31000000-0000-4000-8000-999999999999",
        json!({"kind": "vote_withdraw", "vote_id": vote_id}),
    )
    .await;
    command(
        &store,
        &overflow,
        "31000000-0000-4000-8000-999999999996",
        json!({"kind": "vote_cast", "vote_id": vote_id, "vote_choice": "No"}),
    )
    .await;
    let summary = local_summary(&store, &vote_id).await;
    assert_eq!(summary.total_votes, MAX_VOTE_BALLOTS_PER_POLL);
    assert_eq!(summary.tallies["Yes"], MAX_VOTE_BALLOTS_PER_POLL - 1);
    assert_eq!(summary.tallies["No"], 1);
    store
        .execute_message_mutation(
            &operator,
            "31000000-0000-4000-8000-999999999995",
            "message.delete",
            &json!({"event_id": vote_id}),
        )
        .await
        .unwrap_or_else(|error| panic!("delete full vote projection: {error}"));
    let ballots = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_vote_ballots")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count deleted ballots: {error}"));
    assert_eq!(ballots, 0);
}

#[tokio::test]
async fn close_authority_and_invalid_projection_fail_without_partial_event() {
    let (store, operator) = fixture().await;
    let creator = add_human(&store, "creator", "Creator").await;
    let stranger = add_human(&store, "stranger", "Stranger").await;
    let created = command(
        &store,
        &creator,
        "20000000-0000-4000-8000-000000000001",
        json!({
            "kind": "vote",
            "vote_question": "Proceed?",
            "vote_options": ["Go", "Wait"],
            "vote_duration_seconds": 30
        }),
    )
    .await;
    let vote_id = created.outcome.event.id;
    let before_denial = event_count(&store).await;
    assert_rejected(
        &store
            .execute_message_with_turn(
                &stranger,
                "20000000-0000-4000-8000-000000000002",
                "message.send",
                &json!({"kind": "vote_close", "vote_id": vote_id}),
            )
            .await,
        "permission_denied",
    );
    assert_eq!(event_count(&store).await, before_denial);

    let closed = command(
        &store,
        &operator,
        "20000000-0000-4000-8000-000000000003",
        json!({"kind": "vote_close", "vote_id": vote_id}),
    )
    .await;
    assert_eq!(
        closed.outcome.event.message_kind.as_deref(),
        Some("vote_close")
    );
    assert!(closed.assignments.is_empty());
    let closed_summary = local_summary(&store, &vote_id).await;
    assert!(closed_summary.closed);
    assert_eq!(closed_summary.close_reason, "manual");
    assert_eq!(
        closed_summary.closed_at,
        closed.outcome.event.created_at.to_rfc3339()
    );
    assert_rejected(
        &store
            .execute_message_with_turn(
                &creator,
                "20000000-0000-4000-8000-000000000004",
                "message.send",
                &json!({"kind": "vote_cast", "vote_id": vote_id, "vote_choice": "Go"}),
            )
            .await,
        "vote_closed",
    );
    expire_vote(&store, &vote_id).await;
    let elapsed_closed_summary = local_summary(&store, &vote_id).await;
    assert_eq!(elapsed_closed_summary.close_reason, "deadline");
    assert_eq!(elapsed_closed_summary.closed_at, closed_summary.closed_at);

    let second = command(
        &store,
        &creator,
        "20000000-0000-4000-8000-000000000005",
        json!({"kind": "vote", "vote_question": "Again?", "vote_options": ["A", "B"]}),
    )
    .await;
    sqlx::query(
        "UPDATE room_vote_states SET tallies_json = '[1,0]', total_votes = 0 WHERE vote_id = ?",
    )
    .bind(&second.outcome.event.id)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("corrupt vote fixture: {error}"));
    let before_corruption_rejection = event_count(&store).await;
    assert_rejected(
        &store
            .execute_message_with_turn(
                &creator,
                "20000000-0000-4000-8000-000000000006",
                "message.send",
                &json!({"kind": "vote_cast", "vote_id": second.outcome.event.id, "vote_choice": "A"}),
            )
            .await,
        "invalid_state",
    );
    assert_eq!(event_count(&store).await, before_corruption_rejection);
}

#[tokio::test]
async fn expired_vote_keeps_its_distinct_public_rejection() {
    let (store, operator) = fixture().await;
    let created = command(
        &store,
        &operator,
        "21000000-0000-4000-8000-000000000001",
        json!({
            "kind": "vote",
            "vote_question": "Still open?",
            "vote_options": ["Yes", "No"],
            "vote_duration_seconds": 30
        }),
    )
    .await;
    expire_vote(&store, &created.outcome.event.id).await;
    let before_rejection = event_count(&store).await;

    assert_rejected(
        &store
            .execute_message_with_turn(
                &operator,
                "21000000-0000-4000-8000-000000000002",
                "message.send",
                &json!({
                    "kind": "vote_cast",
                    "vote_id": created.outcome.event.id,
                    "vote_choice": "Yes"
                }),
            )
            .await,
        "vote_expired",
    );
    assert_eq!(event_count(&store).await, before_rejection);
    let summary = local_summary(&store, &created.outcome.event.id).await;
    assert!(summary.closed);
    assert_eq!(summary.close_reason, "deadline");
    assert_eq!(summary.closed_at, summary.vote_deadline_at);
}

#[tokio::test]
async fn summary_is_read_only_and_discloses_only_the_current_viewers_ballot() {
    let (store, operator) = fixture().await;
    let voter = add_human(&store, "voter", "Voter").await;
    let created = command(
        &store,
        &operator,
        "22000000-0000-4000-8000-000000000001",
        json!({
            "kind": "vote",
            "vote_question": "Choose?",
            "vote_options": ["One", "Two"]
        }),
    )
    .await;
    command(
        &store,
        &operator,
        "22000000-0000-4000-8000-000000000002",
        json!({
            "kind": "vote_cast",
            "vote_id": created.outcome.event.id,
            "vote_choice": "One"
        }),
    )
    .await;
    command(
        &store,
        &voter,
        "22000000-0000-4000-8000-000000000003",
        json!({
            "kind": "vote_cast",
            "vote_id": created.outcome.event.id,
            "vote_choice": "Two"
        }),
    )
    .await;
    let before = persisted_write_counts(&store).await;

    let mine = local_summary(&store, &created.outcome.event.id).await;
    assert_eq!(mine.own_choice, "One");
    assert_eq!(mine.tallies.get("One"), Some(&1));
    assert_eq!(mine.tallies.get("Two"), Some(&1));
    assert_eq!(mine.total_votes, 2);
    assert_eq!(mine.created_by, "Host");
    assert!(!mine.closed);
    assert_eq!(persisted_write_counts(&store).await, before);
}

async fn fixture() -> (SqliteStore, AuthenticatedPrincipal) {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open vote fixture: {error}"));
    store
        .bootstrap_local_authority("30000000-0000-4000-8000-000000000001", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap vote fixture: {error}"));
    store
        .create_room_for_local_operator(
            "30000000-0000-4000-8000-000000000002",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create vote room: {error}"));
    let principal = AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "Host".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    (store, principal)
}

async fn add_human(store: &SqliteStore, id: &str, display_name: &str) -> AuthenticatedPrincipal {
    let now = Utc::now();
    let participant = Participant {
        room_id: "general".to_owned(),
        participant_id: id.to_owned(),
        display_name: display_name.to_owned(),
        avatar_image_url: String::new(),
        participant_type: "human".to_owned(),
        status: ParticipantStatus::Joined,
        role: ParticipantRole::Human,
        owner_id: format!("{id}-user"),
        muted: false,
        created_at: now,
        updated_at: now,
    };
    sqlx::query(
        "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
    )
    .bind("general")
    .bind(id)
    .bind(
        serde_json::to_string(&participant).unwrap_or_else(|error| panic!("encode human: {error}")),
    )
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert human: {error}"));
    AuthenticatedPrincipal {
        principal_id: participant.owner_id,
        participant_id: id.to_owned(),
        display_name: display_name.to_owned(),
        room_id: "general".to_owned(),
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

async fn command(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload: Value,
) -> crate::RoomCommandMutation {
    store
        .execute_message_with_turn(principal, request_id, "message.send", &payload)
        .await
        .unwrap_or_else(|error| panic!("execute vote command: {error}"))
}

async fn local_summary(store: &SqliteStore, vote_id: &str) -> VoteSummary {
    store
        .local_room_vote_summary(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            vote_id,
        )
        .await
        .unwrap_or_else(|error| panic!("read local vote summary: {error}"))
}

async fn expire_vote(store: &SqliteStore, vote_id: &str) {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT event_json FROM room_events WHERE room_id = 'general' AND json_extract(event_json, '$.id') = ?",
    )
    .bind(vote_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read vote event: {error}"));
    let mut event: RoomEvent =
        serde_json::from_str(&encoded).unwrap_or_else(|error| panic!("decode vote event: {error}"));
    event.created_at = Utc::now() - Duration::seconds(31);
    event.extra.insert(
        "vote_deadline_at".to_owned(),
        Value::String(
            vote_deadline_at(event.created_at, 30)
                .unwrap_or_else(|| panic!("timed vote must have a deadline"))
                .to_rfc3339(),
        ),
    );
    sqlx::query("UPDATE room_events SET event_json = ? WHERE room_id = ? AND seq = ?")
        .bind(
            serde_json::to_string(&event)
                .unwrap_or_else(|error| panic!("encode expired vote event: {error}")),
        )
        .bind(&event.room_id)
        .bind(event.seq)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("expire vote event: {error}"));
}

async fn assert_vote_state(
    store: &SqliteStore,
    expected_tallies: &[u64],
    expected_total: i64,
    expected_choice: Option<i64>,
) {
    let row = sqlx::query(
        "SELECT tallies_json, total_votes FROM room_vote_states ORDER BY poll_seq DESC LIMIT 1",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read vote state: {error}"));
    assert_eq!(
        serde_json::from_str::<Vec<u64>>(row.get::<String, _>("tallies_json").as_str())
            .unwrap_or_else(|error| panic!("decode tallies: {error}")),
        expected_tallies
    );
    assert_eq!(row.get::<i64, _>("total_votes"), expected_total);
    let choice = sqlx::query_scalar::<_, i64>(
        "SELECT choice_index FROM room_vote_ballots ORDER BY vote_id DESC LIMIT 1",
    )
    .fetch_optional(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read ballot: {error}"));
    assert_eq!(choice, expected_choice);
}

async fn event_count(store: &SqliteStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM room_events")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count events: {error}"))
}

async fn persisted_write_counts(store: &SqliteStore) -> (i64, i64) {
    let row = sqlx::query(
        "SELECT (SELECT COUNT(*) FROM room_events) AS events, (SELECT COUNT(*) FROM command_results) AS commands",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("count persisted vote writes: {error}"));
    (row.get("events"), row.get("commands"))
}

async fn vote_event_count(store: &SqliteStore) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM room_events WHERE json_extract(event_json, '$.message_kind') IN ('vote', 'vote_cast', 'vote_withdraw', 'vote_close')",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("count vote events: {error}"))
}

fn assert_vote_transition_is_minimized(event: &RoomEvent, vote_id: &str) {
    assert!(event.actor.participant_id.is_empty());
    assert!(event.actor.participant_type.is_empty());
    assert!(event.participant_id.is_none());
    assert!(event.participant_type.is_none());
    assert!(event.actor_id.is_none());
    assert!(event.actor_type.is_none());
    assert!(event.display_name.is_none());
    assert_eq!(event.content.as_deref(), Some(""));
    assert_eq!(event.extra.len(), 1);
    assert_eq!(event.extra["vote_id"], vote_id);
}

fn assert_rejected<T>(result: &Result<T, PersistenceError>, expected: &str) {
    assert!(matches!(
        result,
        Err(PersistenceError::CommandRejected { code, .. }) if *code == expected
    ));
}
