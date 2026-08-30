use serde_json::json;

use super::{AGENT_ID, authority, fixture, running_authority, save_stored_session, stored_session};

#[tokio::test]
async fn api_turns_rebuild_bounded_visible_context_from_canonical_room_events() {
    let (store, principal, _directory) = fixture().await;
    let mut session = stored_session(&store).await;
    session.public.provider_kind = "deepseek_api".to_owned();
    session.public.runtime_kind = "api".to_owned();
    session.public.transport = "https".to_owned();
    session.public.provider_session_reused = false;
    save_stored_session(&store, &session).await;

    let first = store
        .execute_message_with_turn(
            &principal,
            "api-context-first",
            "message.send",
            &json!({"content": "@Terra remember the amber lantern"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign first API turn: {error}"));
    let first_assignment = first
        .assignments
        .first()
        .unwrap_or_else(|| panic!("first API message must be assigned"));
    let first_start = running_authority(&store, first_assignment, "api-provider-turn-1").await;
    store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            authority(&first_start, "api-provider-turn-1", None),
            "I will remember the amber lantern.",
            "",
        )
        .await
        .unwrap_or_else(|error| panic!("complete first API turn: {error}"));

    let second = store
        .execute_message_with_turn(
            &principal,
            "api-context-second",
            "message.send",
            &json!({"content": "@Terra what color was it?"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign second API turn: {error}"));
    let room_view = &second
        .assignments
        .first()
        .unwrap_or_else(|| panic!("second API message must be assigned"))
        .room_view;

    assert!(room_view.contains("remember the amber lantern"));
    assert!(room_view.contains("I will remember the amber lantern."));
    assert!(room_view.contains("what color was it?"));
}
