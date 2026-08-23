use agentsassemble_domain::{
    AgentSession, AuthenticatedPrincipal, CURRENT_RUNTIME_PROFILE_VERSION, CapabilitySet,
    ClientKind, DurableAgentSession, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, Participant,
    ParticipantStatus, Room, RoomSettings,
};
use chrono::Utc;
use serde_json::json;

use crate::{PersistenceError, SqliteStore};

const AGENT_ID: &str = "codex-00000000-0000-5000-8000-000000000001";

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
    let Some(first_assignment) = first.assignment else {
        panic!("first message must assign Terra");
    };
    assert_eq!(first_assignment.session.public.session_id, AGENT_ID);
    assert!(
        first_assignment
            .provider_input
            .contains("@Terra take the first turn")
    );
    assert!(first_assignment.provider_input.contains("[Your turn]"));

    let replay = store
        .execute_message_with_turn(&principal, "message-1", "message.send", &first_payload)
        .await
        .unwrap_or_else(|error| panic!("replay first message: {error}"));
    assert!(replay.outcome.deduplicated);
    assert!(replay.assignment.is_none());

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
    assert!(second.assignment.is_none());

    let first_turn_id = first_assignment.turn_id.clone();
    let committed = store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            &first_turn_id,
            "provider-turn-1",
            "First provider final",
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
    let Some(next) = committed.next_assignment else {
        panic!("queued message must get the next turn");
    };
    assert_ne!(next.turn_id, first_turn_id);
    assert!(next.provider_input.contains("queue this while busy"));

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
    assert_eq!(stored.inflight_event_ids, [second.outcome.event.id]);

    let Err(stale) = store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            &first_turn_id,
            "provider-turn-stale",
            "must not publish",
        )
        .await
    else {
        panic!("old turn authority must not publish twice");
    };
    assert_rejection_code(&stale, "stale_provider_turn");
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
    let Some(assignment) = mutation.assignment else {
        panic!("source message must assign Terra");
    };
    let committed = store
        .fail_agent_turn(
            "general",
            AGENT_ID,
            &assignment.turn_id,
            "unknown_internal_failure",
            "/Users/alice/private/bin/codex --api-key=sk-live-example123456",
        )
        .await
        .unwrap_or_else(|error| panic!("fail provider turn: {error}"));
    assert_eq!(
        event_types(&committed.events),
        ["error", "turn_finished", "agent_session_state",]
    );
    assert!(committed.next_assignment.is_none());
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
    assert!(stored.inflight_event_ids.is_empty());
    assert_eq!(stored.pending_event_ids, [source_id]);
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
    assert_eq!(event_count, 0);
}

fn event_types(events: &[agentsassemble_domain::RoomEvent]) -> Vec<&str> {
    events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect()
}

fn assert_rejection_code(error: &PersistenceError, expected: &str) {
    let PersistenceError::CommandRejected { code, .. } = error else {
        panic!("expected command rejection, got {error}");
    };
    assert_eq!(*code, expected);
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

async fn fixture() -> (SqliteStore, AuthenticatedPrincipal, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("test directory: {error}"));
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("open store: {error}"));
    let now = Utc::now();
    let host = participant(LOCAL_OPERATOR_PARTICIPANT_ID, "Host", "human", "host", now);
    store
        .initialize_room(
            &Room::new("general".to_owned(), "General".to_owned(), now),
            &RoomSettings::defaults("General".to_owned()),
            &host,
        )
        .await
        .unwrap_or_else(|error| panic!("initialize room: {error}"));
    let agent = participant(AGENT_ID, "Terra", "agent", "agent", now);
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
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    (store, principal, directory)
}

fn participant(
    id: &str,
    name: &str,
    participant_type: &str,
    role: &str,
    now: chrono::DateTime<Utc>,
) -> Participant {
    Participant {
        room_id: "general".to_owned(),
        participant_id: id.to_owned(),
        display_name: name.to_owned(),
        participant_type: participant_type.to_owned(),
        status: ParticipantStatus::Joined,
        role: role.to_owned(),
        owner_id: "operator-local-user".to_owned(),
        muted: false,
        created_at: now,
        updated_at: now,
    }
}

fn attached_session(now: chrono::DateTime<Utc>) -> DurableAgentSession {
    DurableAgentSession {
        public: AgentSession {
            room_id: "general".to_owned(),
            session_id: AGENT_ID.to_owned(),
            participant_id: AGENT_ID.to_owned(),
            display_name: "Terra".to_owned(),
            status: "attached".to_owned(),
            runtime_status: "idle".to_owned(),
            enabled: true,
            provider_kind: "codex_live_session".to_owned(),
            runtime_kind: "live_cli".to_owned(),
            connection_kind: "native_cli_bridge".to_owned(),
            external_owned: false,
            process_ownership: "server".to_owned(),
            model: "gpt-5.6-terra".to_owned(),
            reasoning_effort: "medium".to_owned(),
            service_tier: "default".to_owned(),
            variant: String::new(),
            execution_harness: "builtin".to_owned(),
            permission_mode: "meeting_read_only".to_owned(),
            max_output_tokens: 0,
            catalog_revision: "catalog-1".to_owned(),
            transport: "stdio_jsonl".to_owned(),
            last_seen_event_id: String::new(),
            last_seen_seq: 0,
            last_provider_sync_event_id: String::new(),
            last_provider_sync_seq: 0,
            bootstrap_cutoff_seq: 0,
            turn_count: 0,
            active_turn_id: String::new(),
            turn_phase: String::new(),
            last_error: String::new(),
            last_error_code: String::new(),
            recovery_required: false,
            provider_session_active: true,
            provider_session_reused: false,
            created_at: now,
            updated_at: now,
        },
        executable: "/owned/codex".to_owned(),
        executable_identity: "owned-codex-identity".to_owned(),
        workspace: "/owned/workspace".to_owned(),
        workspace_identity: "owned-workspace-identity".to_owned(),
        runtime_profile_key: "profile-1".to_owned(),
        runtime_profile_version: CURRENT_RUNTIME_PROFILE_VERSION,
        provider_session_id: "provider-thread-1".to_owned(),
        runtime_handle_id: "owned-runtime-1".to_owned(),
        runtime_owner_id: "supervisor-instance-1".to_owned(),
        pending_event_ids: Vec::new(),
        inflight_event_ids: Vec::new(),
        active_source_event_id: String::new(),
        input_up_to_event_id: String::new(),
        input_up_to_seq: 0,
        lifecycle_intent_action: String::new(),
        lifecycle_intent_id: String::new(),
        lifecycle_intent_status: String::new(),
    }
}
