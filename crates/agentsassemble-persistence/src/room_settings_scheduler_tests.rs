use crate::RoomMutationAuthority::TrustedPrincipal;
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

#[tokio::test]
async fn a_declined_ordered_turn_hands_the_floor_to_the_next_agent() {
    let (store, principal, _directory) = fixture().await;
    insert_second_agent(&store).await;
    let routed = store
        .execute_message_with_turn(
            &principal,
            "ordered-decline",
            "message.send",
            &json!({"content": "둘이 말좀 주고받아봐"}),
        )
        .await
        .unwrap_or_else(|error| panic!("route ordered message: {error}"));
    assert_eq!(routed.assignments.len(), 1);
    let first = routed.assignments[0].clone();

    let declined = decline_assignment(&store, &first, "decline-1", "not_addressed").await;

    // The floor moves to the only other agent, and its turn carries the same message.
    assert_eq!(declined.next_assignments.len(), 1);
    let second = &declined.next_assignments[0];
    assert_ne!(
        second.session.public.session_id,
        first.session.public.session_id
    );

    // A second decline ends the exchange instead of handing the floor back.
    let ended = decline_assignment(&store, second, "decline-2", "nothing_useful_to_add").await;
    assert!(ended.next_assignments.is_empty());
}

#[tokio::test]
async fn delayed_decline_merges_older_input_before_newer_pending_message() {
    let (store, principal, _directory) = fixture().await;
    insert_second_agent(&store).await;
    let first = send_message(&store, &principal, "older", "@Terra older message").await;
    let later = send_message(&store, &principal, "newer", "@Flash newer message").await;
    assert!(later.assignments.is_empty());
    let committed = decline_assignment(
        &store,
        &first.assignments[0],
        "decline-older",
        "nothing_useful_to_add",
    )
    .await;
    assert_eq!(committed.next_assignments.len(), 1);
    let next = &committed.next_assignments[0];
    assert_eq!(next.session.public.session_id, SECOND_AGENT_ID);
    assert_eq!(next.session.input_up_to_seq, later.outcome.event.seq);
    assert_eq!(
        next.session
            .inflight_inputs
            .iter()
            .map(|input| input.event_id.as_str())
            .collect::<Vec<_>>(),
        [
            first.outcome.event.id.as_str(),
            later.outcome.event.id.as_str()
        ],
    );
    let older_position = next
        .room_view
        .find("@Terra older message")
        .unwrap_or_else(|| panic!("older input missing from observation"));
    let newer_position = next
        .room_view
        .find("@Flash newer message")
        .unwrap_or_else(|| panic!("newer input missing from observation"));
    assert!(older_position < newer_position);
    let terra = stored_session(&store, super::AGENT_ID).await;
    assert_eq!(
        terra.public.runtime_status,
        agentsassemble_domain::AgentRuntimeStatus::Idle
    );
    assert_eq!(terra.public.last_provider_sync_seq, first.outcome.event.seq);

    let continued = decline_assignment(&store, next, "decline-both", "nothing_useful_to_add").await;
    assert_eq!(continued.next_assignments.len(), 1);
    let ended = decline_assignment(
        &store,
        &continued.next_assignments[0],
        "decline-newer",
        "nothing_useful_to_add",
    )
    .await;
    assert!(ended.next_assignments.is_empty());
    for session_id in [super::AGENT_ID, SECOND_AGENT_ID] {
        let session = stored_session(&store, session_id).await;
        assert_eq!(
            session.public.runtime_status,
            agentsassemble_domain::AgentRuntimeStatus::Idle
        );
        assert_eq!(
            session.public.last_provider_sync_seq,
            later.outcome.event.seq
        );
        assert!(session.pending_inputs.is_empty());
        assert!(session.inflight_inputs.is_empty());
    }
}

#[tokio::test]
async fn delayed_decline_excludes_already_completed_observation() {
    decline_after_newer_observation(true).await;
}

#[tokio::test]
async fn delayed_decline_excludes_observation_still_in_flight() {
    decline_after_newer_observation(false).await;
}

async fn decline_after_newer_observation(complete_newer_first: bool) {
    let (store, principal, _directory) = fixture().await;
    insert_second_agent(&store).await;
    let first = send_message(&store, &principal, "older", "@Terra older message").await;
    let revision = public_settings(&RoomSettings::defaults("General"))
        .unwrap_or_else(|error| panic!("read default settings: {error}"))
        .settings_revision;
    let ambient = update_mode(&store, &principal, "ambient", &revision, "ambient")
        .await
        .unwrap_or_else(|error| panic!("enable ambient mode: {error}"));
    let later = send_message(&store, &principal, "newer", "newer shared context").await;
    assert_eq!(later.assignments.len(), 1);
    let second = &later.assignments[0];
    assert_eq!(second.session.public.session_id, SECOND_AGENT_ID);
    assert!(second.room_view.contains("@Terra older message"));
    if complete_newer_first {
        let done = decline_assignment(&store, second, "newer-done", "nothing_useful_to_add").await;
        assert!(done.next_assignments.is_empty());
    }
    let revision = ambient
        .result
        .pointer("/room_settings/settings_revision")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("ambient settings revision missing"));
    update_mode(&store, &principal, "ordered", revision, "ordered")
        .await
        .unwrap_or_else(|error| panic!("restore ordered mode: {error}"));
    let done = decline_assignment(
        &store,
        &first.assignments[0],
        "older-done",
        "nothing_useful_to_add",
    )
    .await;
    let flash = stored_session(&store, SECOND_AGENT_ID).await;
    assert!(
        flash.pending_inputs.is_empty(),
        "old source is already within Flash's observation"
    );
    let terra_next = if complete_newer_first {
        assert_eq!(flash.public.last_provider_sync_seq, later.outcome.event.seq);
        assert_eq!(done.next_assignments.len(), 1);
        done.next_assignments[0].clone()
    } else {
        assert!(done.next_assignments.is_empty());
        let finished =
            decline_assignment(&store, second, "newer-done", "nothing_useful_to_add").await;
        assert_eq!(finished.next_assignments.len(), 1);
        finished.next_assignments[0].clone()
    };
    assert_eq!(terra_next.session.public.session_id, super::AGENT_ID);
    assert_eq!(terra_next.session.input_up_to_seq, later.outcome.event.seq);
    let ended = decline_assignment(
        &store,
        &terra_next,
        "terra-newer-done",
        "nothing_useful_to_add",
    )
    .await;
    assert!(ended.next_assignments.is_empty());
}

#[tokio::test]
async fn enqueue_rejects_preexisting_nonchronological_pending_authority() {
    let (store, principal, _directory) = fixture().await;
    insert_second_agent(&store).await;
    start_ordered_turn(&store, &principal).await;
    send_message(&store, &principal, "one", "@Flash first pending message").await;
    send_message(&store, &principal, "two", "@Flash second pending message").await;
    let mut corrupted = stored_session(&store, SECOND_AGENT_ID).await;
    corrupted.pending_inputs.reverse();
    sqlx::query(
        "UPDATE agent_sessions SET session_json = ? WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(
        serde_json::to_string(&corrupted)
            .unwrap_or_else(|error| panic!("encode corrupt queue fixture: {error}")),
    )
    .bind(SECOND_AGENT_ID)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("store corrupt queue fixture: {error}"));
    let error = store
        .execute_message_with_turn(
            &principal,
            "three",
            "message.send",
            &json!({"content": "@Flash next message"}),
        )
        .await
        .err()
        .unwrap_or_else(|| panic!("corrupt stored authority must be rejected"));
    assert_rejection_code(&error, "queued_room_event_invalid");
    assert_eq!(
        stored_session(&store, SECOND_AGENT_ID).await.pending_inputs,
        corrupted.pending_inputs
    );
}

async fn send_message(
    store: &SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    request_id: &str,
    content: &str,
) -> crate::RoomCommandMutation {
    store
        .execute_message_with_turn(
            principal,
            request_id,
            "message.send",
            &json!({"content": content}),
        )
        .await
        .unwrap_or_else(|error| panic!("send {request_id}: {error}"))
}

async fn decline_assignment(
    store: &SqliteStore,
    assignment: &crate::AgentTurnAssignment,
    provider_turn_id: &str,
    reason_code: &str,
) -> crate::AgentTurnCommit {
    let start = store
        .authorize_provider_turn_start(
            &assignment.session.public.room_id,
            &assignment.session.public.session_id,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize provider turn: {error}"));
    store
        .mark_provider_turn_running(&start, provider_turn_id)
        .await
        .unwrap_or_else(|error| panic!("mark provider turn running: {error}"));
    store
        .decline_agent_turn(
            &start.room_id,
            &start.session_id,
            crate::ProviderTurnAuthority {
                room_id: &start.room_id,
                session_id: &start.session_id,
                turn_id: &start.turn_id,
                turn_generation: start.turn_generation,
                execution_id: &start.execution_id,
                start_dispatch_nonce: &start.start_dispatch_nonce,
                runtime_handle_id: &start.runtime_handle_id,
                runtime_owner_id: &start.runtime_owner_id,
                runtime_lease_token: &start.runtime_lease_token,
                provider_turn_id,
                provider_session_id: None,
            },
            reason_code,
        )
        .await
        .unwrap_or_else(|error| panic!("decline turn: {error}"))
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
            TrustedPrincipal(principal),
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
