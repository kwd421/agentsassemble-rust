use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, DurableAgentSession, InviteScope,
    LOCAL_OPERATOR_PARTICIPANT_ID, Participant, ParticipantRole, QueuedRoomInput,
    RoomInputDeliveryKind,
};
use chrono::Utc;
use serde_json::json;

use crate::{PersistenceError, SqliteStore};

#[path = "room_turn_test_fixture.rs"]
mod room_turn_test_fixture;
use room_turn_test_fixture::{attached_session, participant};

const AGENT_ID: &str = "codex-00000000-0000-5000-8000-000000000001";
const SECOND_AGENT_ID: &str = "codex-00000000-0000-5000-8000-000000000002";
const SPEAKER_AGENT_ID: &str = "codex-00000000-0000-5000-8000-000000000003";

#[path = "provider_turn_mute_tests.rs"]
mod provider_turn_mute_tests;

#[tokio::test]
async fn ordered_floor_queue_limit_rejects_the_source_message_atomically() {
    let (store, principal, _directory) = fixture().await;
    let active = store
        .execute_message_with_turn(
            &principal,
            "queue-limit-active",
            "message.send",
            &json!({"content": "@Terra hold the floor"}),
        )
        .await
        .unwrap_or_else(|error| panic!("start active turn: {error}"));
    assert_eq!(active.assignments.len(), 1);

    let mut session = stored_session(&store).await;
    session.pending_inputs = (0..super::super::turn_queue::MAX_QUEUED_EVENT_IDS - 2)
        .map(|index| QueuedRoomInput {
            event_id: format!("queued-event-{index}"),
            delivery_kind: RoomInputDeliveryKind::OrderedObservation,
        })
        .collect();
    save_stored_session(&store, &session).await;

    store
        .execute_message_with_turn(
            &principal,
            "queue-limit-last-slot",
            "message.send",
            &json!({"content": "@Terra fill the last queue slot"}),
        )
        .await
        .unwrap_or_else(|error| panic!("fill final queue slot: {error}"));
    let before = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_events")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count events before overflow: {error}"));

    let Err(error) = store
        .execute_message_with_turn(
            &principal,
            "queue-limit-overflow",
            "message.send",
            &json!({"content": "@Terra this must roll back"}),
        )
        .await
    else {
        panic!("an oversized ordered-floor queue must reject the source message");
    };
    assert_rejection_code(&error, "ordered_floor_queue_full");
    let after = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_events")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count events after overflow: {error}"));
    assert_eq!(after, before);
    let stored = stored_session(&store).await;
    assert_eq!(
        stored
            .inflight_inputs
            .len()
            .saturating_add(stored.pending_inputs.len()),
        super::super::turn_queue::MAX_QUEUED_EVENT_IDS
    );
}

#[tokio::test]
async fn ordered_assignment_and_finalization_are_durable_and_exact() {
    let (store, principal, _directory) = fixture().await;
    let first_payload = json!({"content": "@Terra take the first turn"});
    let first = store
        .execute_message_with_turn(&principal, "message-1", "message.send", &first_payload)
        .await
        .unwrap_or_else(|error| panic!("commit first message: {error}"));
    assert_eq!(
        event_types(&first.outcome.events),
        [
            "message_final",
            "turn_started",
            "turn_state",
            "agent_session_state",
        ]
    );
    let first_assignment = first
        .assignments
        .first()
        .unwrap_or_else(|| panic!("first message must assign Terra"));
    assert_eq!(first_assignment.session.public.session_id, AGENT_ID);
    assert!(first_assignment.room_view.contains("take the first turn"));
    assert!(first_assignment.provider_input.contains("read_discussion"));
    let replay = store
        .execute_message_with_turn(&principal, "message-1", "message.send", &first_payload)
        .await
        .unwrap_or_else(|error| panic!("replay first message: {error}"));
    assert!(replay.outcome.deduplicated);
    assert!(replay.assignments.is_empty());
    let second = store
        .execute_message_with_turn(
            &principal,
            "message-2",
            "message.send",
            &json!({"content": "@Terra queue this while busy"}),
        )
        .await
        .unwrap_or_else(|error| panic!("commit second message: {error}"));
    assert_eq!(event_types(&second.outcome.events), ["message_final"]);
    assert!(second.assignments.is_empty());

    let first_turn_id = first_assignment.turn_id.clone();
    let first_start = running_authority(&store, first_assignment, "provider-turn-1").await;
    let committed = store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            authority(&first_start, "provider-turn-1", None),
            "First provider final",
            "",
        )
        .await
        .unwrap_or_else(|error| panic!("complete first provider turn: {error}"));
    assert_eq!(
        event_types(&committed.events),
        [
            "message_final",
            "turn_finished",
            "agent_session_state",
            "turn_started",
            "turn_state",
            "agent_session_state",
        ]
    );
    let next = committed
        .next_assignments
        .first()
        .unwrap_or_else(|| panic!("queued message must get the next turn"));
    assert_ne!(next.turn_id, first_turn_id);
    assert!(next.room_view.contains("queue this while busy"));

    let stored = stored_session(&store).await;
    assert_eq!(stored.public.active_turn_id, next.turn_id);
    assert_eq!(stored.public.turn_count, 1);
    assert_eq!(
        stored.public.last_provider_sync_event_id,
        first.outcome.event.id
    );
    assert_eq!(
        stored.public.last_provider_sync_seq,
        first.outcome.event.seq
    );
    assert_eq!(stored.active_source_event_id, second.outcome.event.id);
    assert_eq!(
        input_ids(&stored.inflight_inputs),
        [second.outcome.event.id]
    );

    let Err(stale) = store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            authority(&first_start, "provider-turn-stale", None),
            "must not publish",
            "",
        )
        .await
    else {
        panic!("old turn authority must not publish twice");
    };
    assert_rejection_code(&stale, "stale_provider_turn");
}

#[tokio::test]
async fn first_antigravity_final_promotes_the_native_session_id_atomically() {
    let (store, principal, _directory) = fixture().await;
    let mut session = stored_session(&store).await;
    "antigravity_live_session".clone_into(&mut session.public.provider_kind);
    "pty".clone_into(&mut session.public.transport);
    "pending-antigravity-codex-1".clone_into(&mut session.provider_session_id);
    save_stored_session(&store, &session).await;

    let mutation = store
        .execute_message_with_turn(
            &principal,
            "antigravity-first-message",
            "message.send",
            &json!({"content": "@Terra bind the native conversation"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign first Antigravity turn: {error}"));
    let assignment = mutation
        .assignments
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("first Antigravity turn must be assigned"));
    let first_start = running_authority(&store, &assignment, "provider-turn-antigravity-1").await;
    store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            authority(
                &first_start,
                "provider-turn-antigravity-1",
                Some("conversation-1"),
            ),
            "Native session attached",
            "",
        )
        .await
        .unwrap_or_else(|error| panic!("commit first Antigravity final: {error}"));
    assert_eq!(
        stored_session(&store).await.provider_session_id,
        "conversation-1"
    );

    let second = store
        .execute_message_with_turn(
            &principal,
            "antigravity-second-message",
            "message.send",
            &json!({"content": "@Terra keep the same conversation"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign second Antigravity turn: {error}"))
        .assignments
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("second Antigravity turn must be assigned"));
    let second_start = running_authority(&store, &second, "provider-turn-antigravity-2").await;
    let Err(error) = store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            authority(
                &second_start,
                "provider-turn-antigravity-2",
                Some("conversation-2"),
            ),
            "must roll back",
            "",
        )
        .await
    else {
        panic!("an attached Antigravity session must not change identity");
    };
    assert_rejection_code(&error, "provider_session_invalid");
    let stored = stored_session(&store).await;
    assert_eq!(stored.provider_session_id, "conversation-1");
    assert_eq!(stored.public.active_turn_id, second.turn_id);
}

#[tokio::test]
async fn provider_failure_restores_input_and_clears_active_authority() {
    let (store, principal, _directory) = fixture().await;
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "message-failure",
            "message.send",
            &json!({"content": "@Terra fail safely"}),
        )
        .await
        .unwrap_or_else(|error| panic!("commit source message: {error}"));
    let source_id = mutation.outcome.event.id;
    let assignment = mutation
        .assignments
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("source message must assign Terra"));
    let start = store
        .authorize_provider_turn_start(
            "general",
            AGENT_ID,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize failed provider turn: {error}"));
    let committed = store
        .fail_agent_turn(
            "general",
            AGENT_ID,
            authority(&start, "", None),
            "unknown_internal_failure",
            "/Users/alice/private/bin/codex --api-key=sk-live-example123456",
            None,
        )
        .await
        .unwrap_or_else(|error| panic!("fail provider turn: {error}"));
    assert_eq!(
        event_types(&committed.events),
        ["error", "turn_finished", "agent_session_state",]
    );
    assert!(committed.next_assignments.is_empty());
    let diagnostic = committed.events[0].content.as_deref().unwrap_or_default();
    assert!(!diagnostic.contains("alice"));
    assert!(!diagnostic.contains("sk-live"));
    assert!(diagnostic.contains("[local path]"));
    assert_eq!(
        committed.events[0].extra["error_code"],
        "provider_turn_failed"
    );

    let stored = stored_session(&store).await;
    assert_eq!(stored.public.status, "error");
    assert_eq!(stored.public.runtime_status, "error");
    assert!(stored.public.active_turn_id.is_empty());
    assert!(stored.active_source_event_id.is_empty());
    assert!(stored.input_up_to_event_id.is_empty());
    assert_eq!(stored.input_up_to_seq, 0);
    assert!(stored.inflight_inputs.is_empty());
    assert_eq!(input_ids(&stored.pending_inputs), [source_id]);
    assert!(stored.public.recovery_required);
}

#[tokio::test]
async fn inconsistent_turn_or_provider_cursor_authority_fails_the_message_transaction() {
    let (store, principal, _directory) = fixture().await;
    let mut session = stored_session(&store).await;
    session.public.last_provider_sync_event_id = "forged-cursor".to_owned();
    save_stored_session(&store, &session).await;
    let Err(cursor_error) = store
        .execute_message_with_turn(
            &principal,
            "bad-cursor-message",
            "message.send",
            &json!({"content": "must roll back"}),
        )
        .await
    else {
        panic!("forged provider cursor must reject the message transaction");
    };
    assert_rejection_code(&cursor_error, "provider_sync_cursor_mismatch");

    session.public.last_provider_sync_event_id.clear();
    session.active_source_event_id = "orphaned-source".to_owned();
    save_stored_session(&store, &session).await;
    let Err(turn_error) = store
        .execute_message_with_turn(
            &principal,
            "bad-turn-message",
            "message.send",
            &json!({"content": "must also roll back"}),
        )
        .await
    else {
        panic!("incomplete turn authority must reject the message transaction");
    };
    assert_rejection_code(&turn_error, "stored_turn_authority_invalid");
    let event_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_events")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count rolled-back events: {error}"));
    assert_eq!(event_count, 1);
}

#[tokio::test]
async fn stopped_direct_target_keeps_every_message_and_assigns_only_the_visible_prefix() {
    let (store, principal, _directory) = fixture().await;
    let mut stopped = stored_session(&store).await;
    "unavailable".clone_into(&mut stopped.public.status);
    "stopped".clone_into(&mut stopped.public.runtime_status);
    stopped.public.enabled = false;
    stopped.public.provider_session_active = false;
    save_stored_session(&store, &stopped).await;

    let mut event_ids = Vec::new();
    for index in 0..51 {
        let mutation = store
            .execute_message_with_turn(
                &principal,
                &format!("stopped-direct-{index}"),
                "message.send",
                &json!({"content": format!("@Terra queued message {index:02}")}),
            )
            .await
            .unwrap_or_else(|error| panic!("queue stopped direct target {index}: {error}"));
        assert!(mutation.assignments.is_empty());
        event_ids.push(mutation.outcome.event.id);
    }
    let queued = stored_session(&store).await;
    assert_eq!(input_ids(&queued.pending_inputs), event_ids);

    let mut attached = queued;
    "attached".clone_into(&mut attached.public.status);
    "idle".clone_into(&mut attached.public.runtime_status);
    attached.public.enabled = true;
    attached.public.provider_session_active = true;
    save_stored_session(&store, &attached).await;
    let commit = store
        .assign_pending_turn("general")
        .await
        .unwrap_or_else(|error| panic!("assign stopped direct backlog: {error}"))
        .unwrap_or_else(|| panic!("stopped direct backlog must become assignable"));
    let assignment = commit
        .next_assignments
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("backlog commit must carry an assignment"));
    assert_eq!(
        input_ids(&assignment.session.inflight_inputs),
        event_ids[..50]
    );
    assert_eq!(
        input_ids(&assignment.session.pending_inputs),
        event_ids[50..]
    );
    assert_eq!(assignment.session.input_up_to_event_id, event_ids[49]);
    assert!(assignment.room_view.contains("queued message 00"));
    assert!(assignment.room_view.contains("queued message 49"));
    assert!(!assignment.room_view.contains("queued message 50"));
}

#[tokio::test]
async fn final_body_mention_routes_to_the_named_agent() {
    let (store, principal, _directory) = fixture().await;
    let now = Utc::now();
    let second_participant = participant(
        SECOND_AGENT_ID,
        "Flash",
        "agent",
        ParticipantRole::Agent,
        now,
    );
    let mut second_session = attached_session(now);
    SECOND_AGENT_ID.clone_into(&mut second_session.public.session_id);
    SECOND_AGENT_ID.clone_into(&mut second_session.public.participant_id);
    "Flash".clone_into(&mut second_session.public.display_name);
    "provider-thread-2".clone_into(&mut second_session.provider_session_id);
    "owned-runtime-2".clone_into(&mut second_session.runtime_handle_id);
    insert_agent(&store, &second_participant, &second_session).await;

    let mutation = store
        .execute_message_with_turn(
            &principal,
            "final-mention-wins",
            "message.send",
            &json!({"content": "@Flash take the floor."}),
        )
        .await
        .unwrap_or_else(|error| panic!("route final direct mention: {error}"));
    let assignment = mutation
        .assignments
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("the final direct mention must assign a turn"));
    assert_eq!(assignment.session.public.session_id, SECOND_AGENT_ID);
}

#[tokio::test]
async fn character_bound_defers_whole_messages_instead_of_advancing_past_them() {
    let (store, principal, _directory) = fixture().await;
    let mut stopped = stored_session(&store).await;
    "unavailable".clone_into(&mut stopped.public.status);
    "stopped".clone_into(&mut stopped.public.runtime_status);
    stopped.public.enabled = false;
    stopped.public.provider_session_active = false;
    save_stored_session(&store, &stopped).await;

    let first_text = format!("@Terra first-visible {}", "a".repeat(10_900));
    let second_text = format!("@Terra second-deferred {}", "b".repeat(10_900));
    let first = store
        .execute_message_with_turn(
            &principal,
            "bounded-first",
            "message.send",
            &json!({"content": first_text}),
        )
        .await
        .unwrap_or_else(|error| panic!("queue first bounded input: {error}"));
    let second = store
        .execute_message_with_turn(
            &principal,
            "bounded-second",
            "message.send",
            &json!({"content": second_text}),
        )
        .await
        .unwrap_or_else(|error| panic!("queue second bounded input: {error}"));
    let mut attached = stored_session(&store).await;
    "attached".clone_into(&mut attached.public.status);
    "idle".clone_into(&mut attached.public.runtime_status);
    attached.public.enabled = true;
    attached.public.provider_session_active = true;
    save_stored_session(&store, &attached).await;

    let commit = store
        .assign_pending_turn("general")
        .await
        .unwrap_or_else(|error| panic!("assign character-bounded input: {error}"))
        .unwrap_or_else(|| panic!("bounded input must become assignable"));
    let assignment = commit
        .next_assignments
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("bounded input commit must carry an assignment"));
    assert_eq!(
        input_ids(&assignment.session.inflight_inputs).as_slice(),
        std::slice::from_ref(&first.outcome.event.id)
    );
    assert_eq!(
        input_ids(&assignment.session.pending_inputs).as_slice(),
        std::slice::from_ref(&second.outcome.event.id)
    );
    assert_eq!(
        assignment.session.input_up_to_event_id,
        first.outcome.event.id
    );
    assert!(assignment.room_view.contains("first-visible"));
    assert!(!assignment.room_view.contains("second-deferred"));
}

#[tokio::test]
async fn undirected_agent_message_prefers_an_eligible_director() {
    let (store, _principal, _directory) = fixture().await;
    let now = Utc::now();
    let mut director = participant(AGENT_ID, "Terra", "agent", ParticipantRole::Director, now);
    director.updated_at = now;
    sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = 'general' AND participant_id = ?",
    )
    .bind(
        serde_json::to_string(&director)
            .unwrap_or_else(|error| panic!("encode director participant: {error}")),
    )
    .bind(AGENT_ID)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("promote director participant: {error}"));

    let second_participant = participant(
        SECOND_AGENT_ID,
        "Flash",
        "agent",
        ParticipantRole::Agent,
        now,
    );
    let mut second_session = attached_session(now);
    SECOND_AGENT_ID.clone_into(&mut second_session.public.session_id);
    SECOND_AGENT_ID.clone_into(&mut second_session.public.participant_id);
    "Flash".clone_into(&mut second_session.public.display_name);
    "provider-thread-2".clone_into(&mut second_session.provider_session_id);
    "owned-runtime-2".clone_into(&mut second_session.runtime_handle_id);
    insert_agent(&store, &second_participant, &second_session).await;
    let speaker = participant(
        SPEAKER_AGENT_ID,
        "Worker",
        "agent",
        ParticipantRole::Agent,
        now,
    );
    sqlx::query(
        "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
    )
    .bind("general")
    .bind(SPEAKER_AGENT_ID)
    .bind(
        serde_json::to_string(&speaker)
            .unwrap_or_else(|error| panic!("encode speaker participant: {error}")),
    )
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert speaker participant: {error}"));
    let principal = AuthenticatedPrincipal {
        principal_id: "operator-local-user".to_owned(),
        participant_id: SPEAKER_AGENT_ID.to_owned(),
        display_name: "Worker".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "agent-undirected",
            "message.send",
            &json!({"content": "A room update without a direct handoff."}),
        )
        .await
        .unwrap_or_else(|error| panic!("route undirected agent message: {error}"));
    let assignment = mutation
        .assignments
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("undirected agent message must assign a director"));
    assert_eq!(assignment.session.public.session_id, AGENT_ID);
    assert!(assignment.room_view.contains(SECOND_AGENT_ID));
}

#[path = "room_random_tests.rs"]
mod room_random_tests;
#[path = "room_settings_scheduler_tests.rs"]
mod room_settings_scheduler_tests;

fn event_types(events: &[agentsassemble_domain::RoomEvent]) -> Vec<&str> {
    events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect()
}

fn input_ids(inputs: &[QueuedRoomInput]) -> Vec<String> {
    inputs.iter().map(|input| input.event_id.clone()).collect()
}

fn assert_rejection_code(error: &PersistenceError, expected: &str) {
    let PersistenceError::CommandRejected { code, .. } = error else {
        panic!("expected command rejection, got {error}");
    };
    assert_eq!(*code, expected);
}

fn authority<'a>(
    start: &'a crate::ProviderTurnStartAuthority,
    provider_turn_id: &'a str,
    provider_session_id: Option<&'a str>,
) -> super::ProviderTurnAuthority<'a> {
    super::ProviderTurnAuthority {
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
        provider_session_id,
    }
}

async fn running_authority(
    store: &SqliteStore,
    assignment: &super::AgentTurnAssignment,
    provider_turn_id: &str,
) -> crate::ProviderTurnStartAuthority {
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
    start
}

async fn stored_session(store: &SqliteStore) -> DurableAgentSession {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("load stored session: {error}"));
    serde_json::from_str(&encoded).unwrap_or_else(|error| panic!("decode stored session: {error}"))
}

async fn save_stored_session(store: &SqliteStore, session: &DurableAgentSession) {
    sqlx::query(
        "UPDATE agent_sessions SET session_json = ? WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(
        serde_json::to_string(session)
            .unwrap_or_else(|error| panic!("encode stored session: {error}")),
    )
    .bind(AGENT_ID)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("save stored session: {error}"));
}

async fn insert_agent(
    store: &SqliteStore,
    participant: &Participant,
    session: &DurableAgentSession,
) {
    sqlx::query(
        "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
    )
    .bind("general")
    .bind(&participant.participant_id)
    .bind(
        serde_json::to_string(participant)
            .unwrap_or_else(|error| panic!("encode additional agent: {error}")),
    )
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert additional agent: {error}"));
    sqlx::query("INSERT INTO agent_sessions(room_id, session_id, session_json) VALUES (?, ?, ?)")
        .bind("general")
        .bind(&session.public.session_id)
        .bind(
            serde_json::to_string(session)
                .unwrap_or_else(|error| panic!("encode additional session: {error}")),
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert additional session: {error}"));
}

async fn fixture() -> (SqliteStore, AuthenticatedPrincipal, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("test directory: {error}"));
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("open store: {error}"));
    let now = Utc::now();
    store
        .bootstrap_local_authority("166538ea-8477-4bb4-a07c-b7193457175e", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap identity: {error}"));
    store
        .create_room_for_local_operator(
            "20000000-0000-4000-8000-000000000003",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create room: {error}"));
    let agent = participant(AGENT_ID, "Terra", "agent", ParticipantRole::Agent, now);
    let session = attached_session(now);
    sqlx::query(
        "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
    )
    .bind("general")
    .bind(AGENT_ID)
    .bind(serde_json::to_string(&agent).unwrap_or_else(|error| panic!("encode agent: {error}")))
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert agent: {error}"));
    sqlx::query("INSERT INTO agent_sessions(room_id, session_id, session_json) VALUES (?, ?, ?)")
        .bind("general")
        .bind(AGENT_ID)
        .bind(
            serde_json::to_string(&session)
                .unwrap_or_else(|error| panic!("encode session: {error}")),
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert session: {error}"));
    let principal = AuthenticatedPrincipal {
        principal_id: "operator-local-user".to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "Host".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    (store, principal, directory)
}
