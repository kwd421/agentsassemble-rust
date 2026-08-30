use serde_json::json;

use super::{AGENT_ID, authority, fixture, running_authority, save_stored_session, stored_session};

#[tokio::test]
async fn api_turns_preserve_creation_context_after_store_reopen() {
    let (store, principal, directory) = fixture().await;
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
            &json!({"content": "@Terra archive project code ORCHID-71"}),
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
            "Understood.",
            "",
        )
        .await
        .unwrap_or_else(|error| panic!("complete first API turn: {error}"));
    assert_eq!(stored_session(&store).await.public.bootstrap_cutoff_seq, 0);

    drop(store);
    let store = super::SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("reopen API context store: {error}"));

    let second = store
        .execute_message_with_turn(
            &principal,
            "api-context-second",
            "message.send",
            &json!({"content": "@Terra which project code did I give you?"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign second API turn: {error}"));
    let room_view = &second
        .assignments
        .first()
        .unwrap_or_else(|| panic!("second API message must be assigned"))
        .room_view;

    assert!(room_view.contains("archive project code ORCHID-71"));
    assert!(room_view.contains("Understood."));
    assert!(room_view.contains("which project code did I give you?"));
}
