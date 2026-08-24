use agentsassemble_domain::{
    AuthenticatedPrincipal, LOCAL_OPERATOR_PARTICIPANT_ID, RoomRandomRequest, RoomRandomResult,
    RoomSettings, public_settings,
};
use serde_json::json;

use super::{AGENT_ID, assert_rejection_code, fixture};
use crate::{ProviderRoomRandomCommit, SqliteStore};

#[tokio::test]
async fn human_room_random_is_tabletop_only_atomic_and_replayable() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"notation": "2d6+1", "reason": "initiative"});
    let request = RoomRandomRequest::parse("room.random.roll", &payload)
        .unwrap_or_else(|error| panic!("parse room random request: {error}"));
    let result = RoomRandomResult::RollDice {
        notation: "2d6+1".to_owned(),
        rolls: vec![2, 5],
        modifier: 1,
        total: 8,
    };
    let Err(unavailable) = store
        .execute_room_random_command(
            &principal,
            "random-chat",
            "room.random.roll",
            &payload,
            &result,
        )
        .await
    else {
        panic!("chat mode must reject room randomness");
    };
    assert_rejection_code(&unavailable, "room_random_unavailable");

    enable_tabletop(&store, &principal).await;
    let committed = store
        .execute_room_random_command(
            &principal,
            "random-tabletop",
            "room.random.roll",
            &payload,
            &result,
        )
        .await
        .unwrap_or_else(|error| panic!("commit human room random result: {error}"));
    assert_eq!(committed.event.extra["operation"], "roll_dice");
    assert_eq!(
        committed.event.extra["source_participant_id"],
        LOCAL_OPERATOR_PARTICIPANT_ID
    );
    assert_eq!(request.operation(), "roll_dice");
    let replay = store
        .execute_room_random_command(
            &principal,
            "random-tabletop",
            "room.random.roll",
            &payload,
            &result,
        )
        .await
        .unwrap_or_else(|error| panic!("replay human room random result: {error}"));
    assert!(replay.deduplicated);
    assert_eq!(replay.result, committed.result);
    let result_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM room_events WHERE event_json LIKE '%room_tool_result%'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("count room random events: {error}"));
    assert_eq!(result_count, 1);
    let write_count = sqlx::query_scalar::<_, i64>(
        "SELECT command_count FROM room_write_budgets WHERE room_id = 'general'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read human room write budget: {error}"));
    assert_eq!(
        write_count, 2,
        "replay and rejected chat-mode use must not be charged"
    );
}

#[tokio::test]
async fn provider_room_random_revalidates_turn_and_enforces_durable_budget() {
    let (store, principal, _directory) = fixture().await;
    enable_tabletop(&store, &principal).await;
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "random-provider-turn",
            "message.send",
            &json!({"content": "@Terra roll for the room"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign provider room turn: {error}"));
    let assignment = mutation
        .assignments
        .first()
        .unwrap_or_else(|| panic!("provider must receive an active tabletop turn"));
    assert!(assignment.tabletop_tools);
    assert!(assignment.provider_input.contains("roll_dice"));
    let request = RoomRandomRequest::Choose {
        options: vec!["north".to_owned(), "south".to_owned()],
        reason: String::new(),
    };
    let result = RoomRandomResult::ChooseRandom {
        choice: "south".to_owned(),
        index: 1,
        option_count: 2,
        options: vec!["north".to_owned(), "south".to_owned()],
    };
    for index in 0..32 {
        let result_id = format!("result-{index:032x}");
        store
            .commit_provider_room_random(ProviderRoomRandomCommit {
                room_id: "general",
                session_id: AGENT_ID,
                turn_id: &assignment.turn_id,
                input_up_to_seq: assignment.session.input_up_to_seq,
                result_id: &result_id,
                request: &request,
                result: &result,
            })
            .await
            .unwrap_or_else(|error| panic!("commit provider result {index}: {error}"));
    }
    let Err(exhausted) = store
        .commit_provider_room_random(ProviderRoomRandomCommit {
            room_id: "general",
            session_id: AGENT_ID,
            turn_id: &assignment.turn_id,
            input_up_to_seq: assignment.session.input_up_to_seq,
            result_id: "result-00000000000000000000000000000020",
            request: &request,
            result: &result,
        })
        .await
    else {
        panic!("the thirty-third provider room result must fail");
    };
    assert_rejection_code(&exhausted, "room_random_budget_exhausted");
    let committed = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM room_turn_tool_results WHERE room_id = 'general' AND turn_id = ?",
    )
    .bind(&assignment.turn_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("count durable provider room results: {error}"));
    assert_eq!(committed, 32);
    let write_count = sqlx::query_scalar::<_, i64>(
        "SELECT command_count FROM room_write_budgets WHERE room_id = 'general'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read provider room write budget: {error}"));
    assert_eq!(
        write_count, 34,
        "settings, source message, and every provider result share one durable room budget"
    );
}

async fn enable_tabletop(store: &SqliteStore, principal: &AuthenticatedPrincipal) {
    let revision = public_settings(&RoomSettings::defaults("General"))
        .unwrap_or_else(|error| panic!("derive settings revision: {error}"))
        .settings_revision;
    store
        .execute_room_settings_update(
            principal,
            "enable-tabletop",
            &json!({"expected_revision": revision, "tool_mode": "tabletop"}),
        )
        .await
        .unwrap_or_else(|error| panic!("enable tabletop mode: {error}"));
}
