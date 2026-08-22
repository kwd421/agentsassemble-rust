use agentsassemble_domain::{
    AgentSession, AuthenticatedPrincipal, CURRENT_RUNTIME_PROFILE_VERSION, CapabilitySet,
    ClientKind, DurableAgentSession, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, Participant,
    ParticipantStatus, Room, RoomSettings,
};
use chrono::Utc;
use serde_json::json;

use crate::{AgentRuntimeStarted, AgentStartPlan, AgentStopPlan, PersistenceError, SqliteStore};

const AGENT_ID: &str = "codex-00000000-0000-5000-8000-000000000001";

async fn fixture() -> (SqliteStore, AuthenticatedPrincipal, tempfile::TempDir) {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("open store: {error}"));
    let now = Utc::now();
    let host = Participant {
        room_id: "general".to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "Host".to_owned(),
        participant_type: "human".to_owned(),
        status: ParticipantStatus::Joined,
        role: "host".to_owned(),
        owner_id: String::new(),
        muted: false,
        created_at: now,
        updated_at: now,
    };
    store
        .initialize_room(
            &Room::new("general".to_owned(), "General".to_owned(), now),
            &RoomSettings::defaults("General".to_owned()),
            &host,
        )
        .await
        .unwrap_or_else(|error| panic!("initialize room: {error}"));
    seed_agent(&store, now).await;
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

async fn seed_agent(store: &SqliteStore, now: chrono::DateTime<Utc>) {
    let participant = Participant {
        room_id: "general".to_owned(),
        participant_id: AGENT_ID.to_owned(),
        display_name: "Terra".to_owned(),
        participant_type: "agent".to_owned(),
        status: ParticipantStatus::Detached,
        role: "agent".to_owned(),
        owner_id: "operator-local-user".to_owned(),
        muted: false,
        created_at: now,
        updated_at: now,
    };
    let session = DurableAgentSession {
        public: AgentSession {
            room_id: "general".to_owned(),
            session_id: AGENT_ID.to_owned(),
            participant_id: AGENT_ID.to_owned(),
            display_name: "Terra".to_owned(),
            status: "available".to_owned(),
            runtime_status: "stopped".to_owned(),
            enabled: false,
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
            provider_session_active: false,
            provider_session_reused: false,
            created_at: now,
            updated_at: now,
        },
        executable: "/owned/codex".to_owned(),
        executable_identity: "executable-identity".to_owned(),
        workspace: "/owned/workspace".to_owned(),
        workspace_identity: "workspace-identity".to_owned(),
        runtime_profile_key: "profile-1".to_owned(),
        runtime_profile_version: CURRENT_RUNTIME_PROFILE_VERSION,
        provider_session_id: String::new(),
        runtime_handle_id: String::new(),
        pending_event_ids: vec!["pending-1".to_owned()],
        inflight_event_ids: Vec::new(),
        lifecycle_intent_action: String::new(),
        lifecycle_intent_id: String::new(),
        lifecycle_intent_status: String::new(),
    };
    sqlx::query(
        "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
    )
    .bind("general")
    .bind(AGENT_ID)
    .bind(
        serde_json::to_string(&participant).unwrap_or_else(|error| panic!("encode agent: {error}")),
    )
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
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One scenario crosses the persisted start/effect/stop recovery boundary.
async fn lifecycle_preserves_provider_identity_and_finalizes_stop_once() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let prepared = store
        .prepare_agent_start(&principal, "start-lifecycle", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"));
    let AgentStartPlan::Start(effect) = prepared else {
        panic!("stopped session must require a start effect");
    };
    assert!(effect.operation_id.starts_with("identity-v1-"));
    assert_ne!(effect.operation_id, "start-lifecycle");
    assert_eq!(effect.session.public.runtime_status, "starting");
    assert_eq!(effect.session.lifecycle_intent_status, "prepared");
    let started = AgentRuntimeStarted {
        runtime_handle_id: "owned-runtime-1".to_owned(),
        provider_session_id: "provider-thread-1".to_owned(),
        runtime_reused: false,
        provider_session_reused: false,
        provider_session_active: true,
    };
    let outcome = store
        .complete_agent_start(
            &principal,
            "start-lifecycle",
            &payload,
            &effect.operation_id,
            &started,
        )
        .await
        .unwrap_or_else(|error| panic!("complete start: {error}"));
    assert_eq!(
        outcome
            .events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        [
            "participant_joined",
            "session_attached",
            "agent_session_state"
        ]
    );
    assert!(
        outcome.result["agent_session"]
            .get("provider_session_id")
            .is_none()
    );
    assert_eq!(
        outcome.result["agent_session"]["provider_session_active"],
        true
    );
    let replay = store
        .prepare_agent_start(&principal, "start-lifecycle", &payload)
        .await
        .unwrap_or_else(|error| panic!("replay start: {error}"));
    let AgentStartPlan::Outcome(replay) = replay else {
        panic!("completed start must replay without another effect");
    };
    assert!(replay.deduplicated);

    let stop = store
        .prepare_agent_stop(&principal, "stop-lifecycle", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare stop: {error}"));
    let AgentStopPlan::Stop(effect) = stop else {
        panic!("running session must require a stop effect");
    };
    assert_eq!(effect.runtime_handle_id, "owned-runtime-1");
    store
        .record_agent_stop_effect("general", &effect.session_id, &effect.operation_id)
        .await
        .unwrap_or_else(|error| panic!("record stop effect: {error}"));
    assert!(matches!(
        store
            .prepare_agent_stop(&principal, "stop-lifecycle", &payload)
            .await
            .unwrap_or_else(|error| panic!("recover stop: {error}")),
        AgentStopPlan::Finalize
    ));
    let stopped = store
        .finalize_agent_stop(&principal, "stop-lifecycle", &payload)
        .await
        .unwrap_or_else(|error| panic!("finalize stop: {error}"));
    assert_eq!(stopped.result["agent_session"]["runtime_status"], "stopped");
    let durable = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read stopped session: {error}"));
    let durable: DurableAgentSession = serde_json::from_str(&durable)
        .unwrap_or_else(|error| panic!("decode stopped session: {error}"));
    assert_eq!(durable.provider_session_id, "provider-thread-1");
    assert!(durable.runtime_handle_id.is_empty());
    assert!(durable.lifecycle_intent_action.is_empty());
}

#[tokio::test]
async fn stale_start_completion_fails_closed_and_visible_failure_clears_intent() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_start(&principal, "start-failed", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must require a start effect");
    };
    let stale = store
        .complete_agent_start(
            &principal,
            "start-failed",
            &payload,
            "different-operation",
            &AgentRuntimeStarted {
                runtime_handle_id: "owned-runtime".to_owned(),
                provider_session_id: "provider-thread".to_owned(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await;
    assert!(matches!(
        stale,
        Err(PersistenceError::CommandRejected {
            code: "stale_start_confirmation",
            ..
        })
    ));
    let events = store
        .fail_agent_start(
            &principal,
            AGENT_ID,
            &effect.operation_id,
            "runtime_start_failed",
            "/Users/alice/private/bin/codex:\nAuthorization: Bearer secret-provider-token",
        )
        .await
        .unwrap_or_else(|error| panic!("record start failure: {error}"));
    assert_eq!(events[0].event_type, "error");
    assert_eq!(
        events[0].content.as_deref(),
        Some("[local path]\n[redacted]")
    );
    let snapshot = store
        .snapshot("general", 0, 200)
        .await
        .unwrap_or_else(|error| panic!("snapshot failed start: {error}"));
    let session = &snapshot.agent_sessions[0];
    assert_eq!(session.runtime_status, "error");
    assert_eq!(session.last_error_code, "runtime_start_failed");
    assert_eq!(session.last_error, "[local path]\n[redacted]");
    assert!(!session.last_error.contains("alice"));
    assert!(!session.last_error.contains("secret-provider-token"));
    assert!(!session.enabled);
}

#[tokio::test]
async fn provider_process_presence_does_not_imply_a_provider_conversation() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "start-without-thread", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must require start");
    };
    let invalid = store
        .complete_agent_start(
            &principal,
            "start-without-thread",
            &payload,
            &start.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: "owned-app-server".to_owned(),
                provider_session_id: String::new(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await;
    assert!(matches!(
        invalid,
        Err(PersistenceError::CommandRejected {
            code: "provider_session_unconfirmed",
            ..
        })
    ));
    let outcome = store
        .complete_agent_start(
            &principal,
            "start-without-thread",
            &payload,
            &start.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: "owned-app-server".to_owned(),
                provider_session_id: String::new(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: false,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("complete process-only start: {error}"));
    assert_eq!(
        outcome.result["agent_session"]["provider_session_active"],
        false
    );
}

#[tokio::test]
async fn unversioned_runtime_profile_fails_before_a_start_effect() {
    let (store, principal, _directory) = fixture().await;
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read session: {error}"));
    let mut session: DurableAgentSession =
        serde_json::from_str(&encoded).unwrap_or_else(|error| panic!("decode session: {error}"));
    session.runtime_profile_version = 0;
    sqlx::query(
        "UPDATE agent_sessions SET session_json = ? WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(serde_json::to_string(&session).unwrap_or_else(|error| panic!("encode session: {error}")))
    .bind(AGENT_ID)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("write legacy session: {error}"));

    let outcome = store
        .prepare_agent_start(&principal, "start-legacy", &json!({"agent_id": AGENT_ID}))
        .await;
    assert!(matches!(
        outcome,
        Err(PersistenceError::CommandRejected {
            code: "profile_migration_required",
            ..
        })
    ));
    let snapshot = store
        .snapshot("general", 0, 10)
        .await
        .unwrap_or_else(|error| panic!("snapshot legacy session: {error}"));
    assert_eq!(snapshot.agent_sessions[0].runtime_status, "stopped");
}

#[tokio::test]
async fn ambiguous_stop_becomes_a_redacted_recoverable_disconnect() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "start-before-ambiguous-stop", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must require start");
    };
    store
        .complete_agent_start(
            &principal,
            "start-before-ambiguous-stop",
            &payload,
            &start.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: "runtime-before-ambiguous-stop".to_owned(),
                provider_session_id: "provider-thread-preserved".to_owned(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("complete start: {error}"));
    let AgentStopPlan::Stop(stop) = store
        .prepare_agent_stop(&principal, "ambiguous-stop", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare stop: {error}"))
    else {
        panic!("running session must require stop");
    };
    let events = store
        .mark_agent_stop_unconfirmed(
            &principal,
            AGENT_ID,
            &stop.operation_id,
            "runtime_stop_unconfirmed",
            "/Users/alice/private/provider Authorization: Bearer private-stop-token",
        )
        .await
        .unwrap_or_else(|error| panic!("mark ambiguous stop: {error}"));
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "error");
    assert!(
        !events[0]
            .content
            .as_deref()
            .unwrap_or_default()
            .contains("alice")
    );
    assert!(
        !events[0]
            .content
            .as_deref()
            .unwrap_or_default()
            .contains("private-stop-token")
    );

    let snapshot = store
        .snapshot("general", 0, 200)
        .await
        .unwrap_or_else(|error| panic!("snapshot ambiguous stop: {error}"));
    let session = &snapshot.agent_sessions[0];
    assert_eq!(session.runtime_status, "disconnected");
    assert_eq!(session.last_error_code, "runtime_stop_unconfirmed");
    assert!(session.recovery_required);
    assert!(!session.provider_session_active);
    assert_eq!(
        store
            .participant("general", AGENT_ID)
            .await
            .unwrap_or_else(|error| panic!("read detached participant: {error}"))
            .status,
        ParticipantStatus::Detached
    );
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read ambiguous durable session: {error}"));
    let durable = serde_json::from_str::<DurableAgentSession>(&encoded)
        .unwrap_or_else(|error| panic!("decode ambiguous durable session: {error}"));
    assert_eq!(durable.provider_session_id, "provider-thread-preserved");
    assert!(durable.runtime_handle_id.is_empty());
    assert!(durable.lifecycle_intent_action.is_empty());
}

#[tokio::test]
async fn startup_reconciliation_disconnects_only_live_looking_sessions() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "start-before-restart", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must require start");
    };
    store
        .complete_agent_start(
            &principal,
            "start-before-restart",
            &payload,
            &start.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: "lost-owned-runtime".to_owned(),
                provider_session_id: "provider-thread-survives".to_owned(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("complete start: {error}"));

    assert_eq!(
        store
            .reconcile_agent_sessions_after_restart()
            .await
            .unwrap_or_else(|error| panic!("reconcile restart: {error}")),
        1
    );
    assert_eq!(
        store
            .reconcile_agent_sessions_after_restart()
            .await
            .unwrap_or_else(|error| panic!("repeat reconciliation: {error}")),
        0
    );
    let snapshot = store
        .snapshot("general", 0, 200)
        .await
        .unwrap_or_else(|error| panic!("snapshot restart: {error}"));
    let session = &snapshot.agent_sessions[0];
    assert_eq!(session.runtime_status, "disconnected");
    assert_eq!(session.last_error_code, "server_restarted");
    assert!(session.recovery_required);
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read reconciled durable session: {error}"));
    let durable = serde_json::from_str::<DurableAgentSession>(&encoded)
        .unwrap_or_else(|error| panic!("decode reconciled durable session: {error}"));
    assert_eq!(durable.provider_session_id, "provider-thread-survives");
    assert!(durable.runtime_handle_id.is_empty());
}
