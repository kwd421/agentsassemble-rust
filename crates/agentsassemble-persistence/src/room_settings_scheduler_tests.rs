use agentsassemble_domain::{
    ParticipantRole, RoomInputDeliveryKind, RoomSettings, public_settings,
};
use serde_json::json;

use super::{
    SECOND_AGENT_ID, assert_rejection_code, attached_session, fixture, insert_agent, participant,
};
use crate::{PersistenceError, SqliteStore};

#[tokio::test]
async fn mode_transitions_preserve_delivery_kind_and_ambient_parallelism() {
    let (store, principal, _directory) = fixture().await;
    insert_second_agent(&store).await;
    start_ordered_turn(&store, &principal).await;
    let initial_revision = public_settings(&RoomSettings::defaults("General"))
        .unwrap_or_else(|error| panic!("initial settings revision: {error}"))
        .settings_revision;
    let ambient_settings = update_mode(
        &store,
        &principal,
        "settings-ambient",
        &initial_revision,
        "ambient",
    )
    .await
    .unwrap_or_else(|error| panic!("switch to ambient: {error}"));
    let Err(stale) = update_mode(
        &store,
        &principal,
        "settings-stale",
        &initial_revision,
        "ordered",
    )
    .await
    else {
        panic!("the stale settings revision must not write");
    };
    assert_rejection_code(&stale, "settings_conflict");

    let ambient = store
        .execute_message_with_turn(
            &principal,
            "transition-ambient",
            "message.send",
            &json!({"content": "both agents observe this ambient message"}),
        )
        .await
        .unwrap_or_else(|error| panic!("route ambient message: {error}"));
    assert_eq!(ambient.assignments.len(), 1);
    assert_eq!(
        ambient.assignments[0].session.public.session_id,
        SECOND_AGENT_ID
    );
    assert_eq!(
        ambient.assignments[0].delivery_kind,
        RoomInputDeliveryKind::AmbientObservation
    );
    let terra = stored_session(&store, super::AGENT_ID).await;
    let flash = stored_session(&store, SECOND_AGENT_ID).await;
    assert!(!terra.public.active_turn_id.is_empty());
    assert!(!flash.public.active_turn_id.is_empty());
    assert_eq!(
        terra.inflight_inputs[0].delivery_kind,
        RoomInputDeliveryKind::OrderedObservation
    );
    assert_eq!(
        terra.pending_inputs[0].delivery_kind,
        RoomInputDeliveryKind::AmbientObservation
    );

    let ambient_revision = ambient_settings
        .result
        .pointer("/room_settings/settings_revision")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("ambient settings result must carry its revision"));
    update_mode(
        &store,
        &principal,
        "settings-ordered",
        ambient_revision,
        "ordered",
    )
    .await
    .unwrap_or_else(|error| panic!("switch back to ordered: {error}"));
    let later = store
        .execute_message_with_turn(
            &principal,
            "transition-ordered-later",
            "message.send",
            &json!({"content": "@Terra queue after the mode transition"}),
        )
        .await
        .unwrap_or_else(|error| panic!("queue later ordered message: {error}"));
    assert!(later.assignments.is_empty());
    let terra = stored_session(&store, super::AGENT_ID).await;
    assert_eq!(
        terra
            .pending_inputs
            .iter()
            .map(|input| input.delivery_kind)
            .collect::<Vec<_>>(),
        [
            RoomInputDeliveryKind::AmbientObservation,
            RoomInputDeliveryKind::OrderedObservation,
        ]
    );
}

#[tokio::test]
async fn settings_result_binds_event_sequence_and_replays() {
    let (store, principal, _directory) = fixture().await;
    let initial_revision = public_settings(&RoomSettings::defaults("General"))
        .unwrap_or_else(|error| panic!("initial settings revision: {error}"))
        .settings_revision;
    let committed = update_mode(
        &store,
        &principal,
        "settings-sequence",
        &initial_revision,
        "ambient",
    )
    .await
    .unwrap_or_else(|error| panic!("update settings: {error}"));
    assert_eq!(committed.result["event_seq"], json!(committed.event.seq));

    let replay = update_mode(
        &store,
        &principal,
        "settings-sequence",
        &initial_revision,
        "ambient",
    )
    .await
    .unwrap_or_else(|error| panic!("replay settings: {error}"));
    assert!(replay.deduplicated);
    assert_eq!(replay.result, committed.result);
    assert_eq!(replay.event.seq, committed.event.seq);
}

async fn insert_second_agent(store: &SqliteStore) {
    let now = chrono::Utc::now();
    let second_participant = participant(
        SECOND_AGENT_ID,
        "Flash",
        "agent",
        ParticipantRole::Agent,
        now,
    );
    let mut second_session = attached_session(now);
    second_session.public.session_id = SECOND_AGENT_ID.to_owned();
    second_session.public.participant_id = SECOND_AGENT_ID.to_owned();
    second_session.public.display_name = "Flash".to_owned();
    second_session.provider_session_id = "provider-thread-2".to_owned();
    second_session.runtime_handle_id = "owned-runtime-2".to_owned();
    second_session.runtime_profile_key = "profile-2".to_owned();
    insert_agent(store, &second_participant, &second_session).await;
}

async fn start_ordered_turn(
    store: &SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
) {
    let ordered = store
        .execute_message_with_turn(
            principal,
            "transition-ordered",
            "message.send",
            &json!({"content": "@Terra hold the ordered floor"}),
        )
        .await
        .unwrap_or_else(|error| panic!("start ordered turn: {error}"));
    assert_eq!(ordered.assignments.len(), 1);
}

async fn update_mode(
    store: &SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    request_id: &str,
    revision: &str,
    mode: &str,
) -> Result<crate::CommandOutcome, PersistenceError> {
    store
        .execute_room_settings_update(
            principal,
            request_id,
            &json!({"expected_revision": revision, "conversation_mode": mode}),
        )
        .await
}

async fn stored_session(
    store: &SqliteStore,
    session_id: &str,
) -> agentsassemble_domain::DurableAgentSession {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(session_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("load stored session {session_id}: {error}"));
    serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("decode stored session {session_id}: {error}"))
}
