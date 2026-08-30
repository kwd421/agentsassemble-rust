use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, Participant, ParticipantRole, ParticipantStatus,
};
use chrono::Utc;
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

    command(
        &store,
        &operator,
        "10000000-0000-4000-8000-000000000002",
        json!({"kind": "vote_cast", "vote_id": vote_id, "vote_choice": "yes"}),
    )
    .await;
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

async fn vote_event_count(store: &SqliteStore) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM room_events WHERE json_extract(event_json, '$.message_kind') IN ('vote', 'vote_cast', 'vote_withdraw', 'vote_close')",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("count vote events: {error}"))
}

fn assert_rejected<T>(result: &Result<T, PersistenceError>, expected: &str) {
    assert!(matches!(
        result,
        Err(PersistenceError::CommandRejected { code, .. }) if *code == expected
    ));
}
