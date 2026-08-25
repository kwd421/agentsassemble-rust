use agentsassemble_domain::{
    AgentSession, AuthenticatedPrincipal, CURRENT_RUNTIME_PROFILE_VERSION, CapabilitySet,
    ClientKind, DurableAgentSession, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, Participant,
    ParticipantRole, ParticipantStatus, QueuedRoomInput, RoomInputDeliveryKind,
};
use chrono::Utc;
use serde_json::{Value, json};

use crate::{AgentRuntimeStarted, AgentStartPlan, AgentStopPlan, PersistenceError, SqliteStore};

pub(super) const AGENT_ID: &str = "codex-00000000-0000-5000-8000-000000000001";

pub(super) async fn fixture() -> (SqliteStore, AuthenticatedPrincipal, tempfile::TempDir) {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("open store: {error}"));
    let now = Utc::now();
    store
        .bootstrap_local_authority("ecaa2428-2d0d-4b0f-8b63-49c17732728a", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap identity: {error}"));
    store
        .create_room_for_local_operator(
            "20000000-0000-4000-8000-000000000002",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create room: {error}"));
    seed_agent(&store, now).await;
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

async fn seed_agent(store: &SqliteStore, now: chrono::DateTime<Utc>) {
    let participant = Participant {
        room_id: "general".to_owned(),
        participant_id: AGENT_ID.to_owned(),
        display_name: "Terra".to_owned(),
        avatar_image_url: String::new(),
        participant_type: "agent".to_owned(),
        status: ParticipantStatus::Detached,
        role: ParticipantRole::Agent,
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
        runtime_owner_id: String::new(),
        runtime_lease_token: String::new(),
        turn_generation: 0,
        schedule_requested: false,
        pending_inputs: vec![QueuedRoomInput {
            event_id: "pending-1".to_owned(),
            delivery_kind: RoomInputDeliveryKind::OrderedObservation,
        }],
        inflight_inputs: Vec::new(),
        active_source_event_id: String::new(),
        input_up_to_event_id: String::new(),
        input_up_to_seq: 0,
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
        runtime_owner_id: "supervisor-instance-1".to_owned(),
        runtime_lease_token: "lease-generation-1".to_owned(),
        provider_session_id: "provider-thread-1".to_owned(),
        runtime_reused: false,
        provider_session_reused: false,
        provider_session_active: true,
    };
    let effect = store
        .authorize_agent_start_effect(
            &principal,
            "start-lifecycle",
            &payload,
            &effect.operation_id,
            "agent.start",
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize start effect: {error}"));
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
    let joined_event = outcome
        .events
        .iter()
        .find(|event| event.event_type == "participant_joined")
        .unwrap_or_else(|| panic!("launch outcome omitted participant_joined"));
    assert_eq!(
        joined_event.extra["participant"]["participant_id"],
        AGENT_ID
    );
    assert_eq!(joined_event.extra["participant"]["status"], "joined");
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
    assert_eq!(effect.runtime_owner_id, "supervisor-instance-1");
    let effect = store
        .authorize_agent_stop_effect(&principal, "stop-lifecycle", &payload, &effect.operation_id)
        .await
        .unwrap_or_else(|error| panic!("authorize stop effect: {error}"));
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
    let reservations = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM lifecycle_command_reservations WHERE room_id = 'general'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("inspect completed reservations: {error}"));
    assert_eq!(reservations, 0);
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
    let unowned = store
        .complete_agent_start(
            &principal,
            "start-without-thread",
            &payload,
            &start.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: "owned-app-server".to_owned(),
                runtime_owner_id: String::new(),
                runtime_lease_token: "lease-generation-1".to_owned(),
                provider_session_id: String::new(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: false,
            },
        )
        .await;
    assert!(matches!(
        unowned,
        Err(PersistenceError::CommandRejected {
            code: "runtime_start_unconfirmed",
            ..
        })
    ));
    store
        .authorize_agent_start_effect(
            &principal,
            "start-without-thread",
            &payload,
            &start.operation_id,
            "agent.start",
            "owned-app-server",
            "supervisor-instance-1",
            "lease-generation-1",
        )
        .await
        .unwrap_or_else(|error| panic!("authorize process-only start: {error}"));
    let invalid = store
        .complete_agent_start(
            &principal,
            "start-without-thread",
            &payload,
            &start.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: "owned-app-server".to_owned(),
                runtime_owner_id: "supervisor-instance-1".to_owned(),
                runtime_lease_token: "lease-generation-1".to_owned(),
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
                runtime_owner_id: "supervisor-instance-1".to_owned(),
                runtime_lease_token: "lease-generation-1".to_owned(),
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
    .unwrap_or_else(|error| panic!("write unsupported profile version: {error}"));

    let outcome = store
        .prepare_agent_start(
            &principal,
            "start-unsupported-profile",
            &json!({"agent_id": AGENT_ID}),
        )
        .await;
    assert!(matches!(
        outcome,
        Err(PersistenceError::CommandRejected {
            code: "runtime_profile_unsupported",
            ..
        })
    ));
    let snapshot = store
        .snapshot("general", 0, 10)
        .await
        .unwrap_or_else(|error| panic!("snapshot rejected session: {error}"));
    assert_eq!(snapshot.agent_sessions[0].runtime_status, "stopped");

    let mut unsupported: serde_json::Value = serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("decode current session document: {error}"));
    unsupported["default_responder"] = json!(true);
    sqlx::query(
        "UPDATE agent_sessions SET session_json = ? WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(
        serde_json::to_string(&unsupported)
            .unwrap_or_else(|error| panic!("encode unsupported session field: {error}")),
    )
    .bind(AGENT_ID)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("write unsupported session field: {error}"));
    assert!(matches!(
        store
            .prepare_agent_start(
                &principal,
                "start-unsupported-field",
                &json!({"agent_id": AGENT_ID}),
            )
            .await,
        Err(PersistenceError::Json(_))
    ));
}

#[tokio::test]
async fn ambiguous_stop_becomes_a_redacted_recoverable_disconnect() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let events = mark_ambiguous_stop(&store, &principal, &payload).await;
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
    assert_eq!(durable.runtime_handle_id, "runtime-before-ambiguous-stop");
    assert_eq!(durable.runtime_owner_id, "supervisor-instance-1");
    assert_eq!(durable.lifecycle_intent_action, "stop");
    assert_eq!(durable.lifecycle_intent_status, "unconfirmed");
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "replacement-start", &payload)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "operation_in_progress",
            ..
        })
    ));
}

#[tokio::test]
async fn restart_retains_ambiguous_stop_authority_until_gone_is_proven() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    mark_ambiguous_stop(&store, &principal, &payload).await;
    assert!(matches!(
        store
            .prepare_agent_stop(&principal, "ambiguous-stop", &payload)
            .await,
        Err(PersistenceError::CommandUnresolved {
            code: "runtime_effect_unconfirmed",
            ..
        })
    ));
    assert_eq!(
        store
            .reconcile_agent_sessions_after_restart()
            .await
            .unwrap_or_else(|error| panic!("retain ambiguous owner: {error}")),
        1
    );
    assert!(matches!(
        store
            .prepare_agent_stop(&principal, "ambiguous-stop", &payload)
            .await,
        Err(PersistenceError::CommandUnresolved {
            code: "runtime_effect_unconfirmed",
            ..
        })
    ));
    assert_ambiguous_owner_was_retained(&store).await;
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "replacement-after-restart", &payload)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "operation_in_progress",
            ..
        })
    ));
}

async fn assert_ambiguous_owner_was_retained(store: &SqliteStore) {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read retained ambiguous session: {error}"));
    let retained = serde_json::from_str::<DurableAgentSession>(&encoded)
        .unwrap_or_else(|error| panic!("decode retained ambiguous session: {error}"));
    assert_eq!(retained.runtime_handle_id, "runtime-before-ambiguous-stop");
    assert_eq!(retained.runtime_owner_id, "supervisor-instance-1");
    assert_eq!(retained.lifecycle_intent_action, "stop");
    assert_eq!(retained.lifecycle_intent_status, "unconfirmed");
    assert_eq!(
        retained.public.last_error_code,
        "runtime_authority_uncertain"
    );
    assert!(retained.public.recovery_required);
    let reservation_status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM lifecycle_command_reservations WHERE request_id = 'ambiguous-stop'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read retained reservation: {error}"));
    assert_eq!(reservation_status, "pending");
}

async fn mark_ambiguous_stop(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    payload: &Value,
) -> Vec<agentsassemble_domain::RoomEvent> {
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(principal, "start-before-ambiguous-stop", payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must require start");
    };
    store
        .authorize_agent_start_effect(
            principal,
            "start-before-ambiguous-stop",
            payload,
            &start.operation_id,
            "agent.start",
            "runtime-before-ambiguous-stop",
            "supervisor-instance-1",
            "lease-generation-1",
        )
        .await
        .unwrap_or_else(|error| panic!("authorize start: {error}"));
    store
        .complete_agent_start(
            principal,
            "start-before-ambiguous-stop",
            payload,
            &start.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: "runtime-before-ambiguous-stop".to_owned(),
                runtime_owner_id: "supervisor-instance-1".to_owned(),
                runtime_lease_token: "lease-generation-1".to_owned(),
                provider_session_id: "provider-thread-preserved".to_owned(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("complete start: {error}"));
    let AgentStopPlan::Stop(stop) = store
        .prepare_agent_stop(principal, "ambiguous-stop", payload)
        .await
        .unwrap_or_else(|error| panic!("prepare stop: {error}"))
    else {
        panic!("running session must require stop");
    };
    store
        .authorize_agent_stop_effect(principal, "ambiguous-stop", payload, &stop.operation_id)
        .await
        .unwrap_or_else(|error| panic!("authorize ambiguous stop: {error}"));
    store
        .mark_agent_stop_unconfirmed(
            principal,
            AGENT_ID,
            &stop.operation_id,
            "runtime_stop_unconfirmed",
            "/Users/alice/private/provider Authorization: Bearer private-stop-token",
        )
        .await
        .unwrap_or_else(|error| panic!("mark ambiguous stop: {error}"))
}

#[tokio::test]
async fn startup_reconciliation_retains_ambiguous_runtime_authority() {
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
        .authorize_agent_start_effect(
            &principal,
            "start-before-restart",
            &payload,
            &start.operation_id,
            "agent.start",
            "lost-owned-runtime",
            "supervisor-instance-1",
            "lease-generation-1",
        )
        .await
        .unwrap_or_else(|error| panic!("authorize start: {error}"));
    store
        .complete_agent_start(
            &principal,
            "start-before-restart",
            &payload,
            &start.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: "lost-owned-runtime".to_owned(),
                runtime_owner_id: "supervisor-instance-1".to_owned(),
                runtime_lease_token: "lease-generation-1".to_owned(),
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
        1
    );
    let snapshot = store
        .snapshot("general", 0, 200)
        .await
        .unwrap_or_else(|error| panic!("snapshot restart: {error}"));
    let session = &snapshot.agent_sessions[0];
    assert_eq!(session.runtime_status, "disconnected");
    assert_eq!(session.last_error_code, "runtime_authority_uncertain");
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
    assert_eq!(durable.runtime_handle_id, "lost-owned-runtime");
    assert_eq!(durable.runtime_owner_id, "supervisor-instance-1");
}
