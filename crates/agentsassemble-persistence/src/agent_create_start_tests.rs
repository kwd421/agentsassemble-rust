use std::{fs::File, path::Path};

use agentsassemble_domain::{
    AgentSession, AgentSessionDraft, AuthenticatedPrincipal, CapabilitySet, ClientKind,
    InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, RoomEvent, stable_content_identity,
    stable_identity_hash,
};
use same_file::Handle;
use serde_json::json;

use crate::{
    AgentCreateStartPlan, AgentRuntimeStarted, AgentStartPlan, PersistenceError,
    RuntimeReconciliationObservation, SqliteStore,
};

async fn fixture() -> (SqliteStore, AuthenticatedPrincipal, tempfile::TempDir) {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("open store: {error}"));
    store
        .bootstrap_local_authority("90b4b9d3-c12e-4495-9955-f0f70d44e55c", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap identity: {error}"));
    store
        .create_room_for_local_operator(
            "20000000-0000-4000-8000-000000000001",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create room: {error}"));
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

fn draft(workspace: &Path) -> AgentSessionDraft {
    let executable = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .unwrap_or_else(|error| panic!("canonical executable: {error}"));
    let mut file =
        File::open(&executable).unwrap_or_else(|error| panic!("open executable: {error}"));
    let executable_handle = Handle::from_file(
        file.try_clone()
            .unwrap_or_else(|error| panic!("clone executable: {error}")),
    )
    .unwrap_or_else(|error| panic!("identify executable: {error}"));
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|error| panic!("canonical workspace: {error}"));
    AgentSessionDraft {
        agent_id: "codex-00000000-0000-5000-8000-000000000099".to_owned(),
        display_name: "Terra".to_owned(),
        provider_kind: "opencode_server".to_owned(),
        runtime_kind: "live_cli".to_owned(),
        executable: executable.to_string_lossy().into_owned(),
        executable_identity: stable_content_identity(&executable_handle, &mut file)
            .unwrap_or_else(|error| panic!("hash executable: {error}")),
        workspace: workspace.to_string_lossy().into_owned(),
        workspace_identity: stable_identity_hash(
            &Handle::from_path(&workspace)
                .unwrap_or_else(|error| panic!("identify workspace: {error}")),
        ),
        model: "gpt-5.6-terra".to_owned(),
        reasoning_effort: "medium".to_owned(),
        service_tier: "default".to_owned(),
        variant: String::new(),
        execution_harness: "builtin".to_owned(),
        permission_mode: "meeting_read_only".to_owned(),
        max_output_tokens: 0,
        catalog_revision: "catalog-1".to_owned(),
        persona_card_id: String::new(),
        runtime_profile_key: "profile-create-start-1".to_owned(),
        transport: "stdio_jsonl".to_owned(),
    }
}

fn started() -> AgentRuntimeStarted {
    AgentRuntimeStarted {
        runtime_handle_id: "runtime-1".to_owned(),
        runtime_owner_id: "supervisor-1".to_owned(),
        runtime_lease_token: "lease-generation-1".to_owned(),
        provider_session_id: "provider-session-1".to_owned(),
        runtime_reused: false,
        provider_session_reused: false,
        provider_session_active: true,
    }
}

fn assert_starting_creation_projection(event: &RoomEvent, session: &AgentSession) {
    assert_eq!(
        event.extra.get("agent_session"),
        Some(&json!(session)),
        "the creation event must expose the exact public session stored by its commit"
    );
    assert_eq!(event.extra["agent_session"]["runtime_status"], "starting");
    assert_eq!(event.extra["agent_session"]["enabled"], true);
}

async fn assert_principal_budget(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    payload: &serde_json::Value,
    expected: bool,
) {
    let actual = store
        .command_requires_principal_budget(principal, "create-start-1", "agent.create", payload)
        .await
        .unwrap_or_else(|error| panic!("inspect principal budget: {error}"));
    assert_eq!(actual, expected);
}

async fn authorize_create_start(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload: &serde_json::Value,
    effect: &crate::AgentCreateStartEffect,
    runtime_handle_id: &str,
    runtime_owner_id: &str,
) {
    store
        .authorize_agent_create_start_effect(
            principal,
            request_id,
            payload,
            &effect.operation_id,
            (runtime_handle_id, runtime_owner_id, "lease-generation-1"),
        )
        .await
        .unwrap_or_else(|error| panic!("authorize create/start: {error}"));
}

fn assert_completed_create_start(commit: &crate::AgentCreateStartCommit) {
    assert_eq!(commit.outcome.result["status"], "created");
    assert_eq!(
        commit.outcome.result["agent_session"]["runtime_status"],
        "stopped"
    );
    assert_eq!(commit.outcome.result["participant"]["status"], "detached");
    assert_eq!(
        commit.outcome.result["start"]["agent_session"]["runtime_status"],
        "idle"
    );
    assert_eq!(
        commit.committed_events[0].event_type,
        "agent_session_created"
    );
    assert_eq!(commit.outcome.events, commit.committed_events);
    assert_eq!(commit.newly_committed_events, commit.committed_events[1..]);
    assert!(
        commit
            .newly_committed_events
            .iter()
            .all(|event| event.event_type != "agent_session_created")
    );
}

#[tokio::test]
async fn create_start_first_commit_replays_one_intent_and_preserves_result_shape() {
    let (store, principal, directory) = fixture().await;
    let payload = json!({"start": true, "provider_id": "codex"});
    assert_principal_budget(&store, &principal, &payload, true).await;
    assert!(matches!(
        store
            .inspect_agent_create_start(&principal, "create-start-1", &payload)
            .await
            .unwrap_or_else(|error| panic!("inspect new command: {error}")),
        AgentCreateStartPlan::Select
    ));
    let first = store
        .prepare_agent_create_start(
            &principal,
            "create-start-1",
            &payload,
            &draft(directory.path()),
        )
        .await
        .unwrap_or_else(|error| panic!("prepare create/start: {error}"));
    let AgentCreateStartPlan::Start(first) = first else {
        panic!("new create/start must own a start effect");
    };
    let replay = store
        .inspect_agent_create_start(&principal, "create-start-1", &payload)
        .await
        .unwrap_or_else(|error| panic!("inspect committed intent: {error}"));
    let AgentCreateStartPlan::Start(replay) = replay else {
        panic!("committed create/start must resume its exact effect");
    };
    assert_principal_budget(&store, &principal, &payload, false).await;
    assert_eq!(first.operation_id, replay.operation_id);
    assert_eq!(
        first.session.public.session_id,
        replay.session.public.session_id
    );
    assert_eq!(first.committed_events, replay.committed_events);
    assert_eq!(first.committed_events.len(), 1);
    assert_eq!(first.newly_committed_events, first.committed_events);
    assert!(replay.newly_committed_events.is_empty());
    assert_starting_creation_projection(&first.committed_events[0], &first.session.public);

    let started = started();
    authorize_create_start(
        &store,
        &principal,
        "create-start-1",
        &payload,
        &first,
        &started.runtime_handle_id,
        &started.runtime_owner_id,
    )
    .await;

    let commit = store
        .complete_agent_create_start(
            &principal,
            "create-start-1",
            &payload,
            &first.operation_id,
            &started,
        )
        .await
        .unwrap_or_else(|error| panic!("complete create/start: {error}"));
    assert_completed_create_start(&commit);

    let replay = store
        .inspect_agent_create_start(&principal, "create-start-1", &payload)
        .await
        .unwrap_or_else(|error| panic!("replay completed command: {error}"));
    let AgentCreateStartPlan::Outcome(replay) = replay else {
        panic!("completed create/start must replay its ACK");
    };
    assert!(replay.deduplicated);
    assert_eq!(replay.result, commit.outcome.result);
    assert_principal_budget(&store, &principal, &payload, false).await;
    let write_count = sqlx::query_scalar::<_, i64>(
        "SELECT command_count FROM room_write_budgets WHERE room_id = 'general'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read lifecycle write budget: {error}"));
    assert_eq!(
        write_count, 1,
        "intent resume and completed replay must reuse one write admission"
    );
}

#[tokio::test]
async fn safe_start_failure_replays_the_same_terminal_rejection() {
    let (store, principal, directory) = fixture().await;
    let payload = json!({"start": true, "provider_id": "codex"});
    let plan = store
        .prepare_agent_create_start(
            &principal,
            "create-start-safe-failure",
            &payload,
            &draft(directory.path()),
        )
        .await
        .unwrap_or_else(|error| panic!("prepare create/start: {error}"));
    let AgentCreateStartPlan::Start(effect) = plan else {
        panic!("new create/start must own a start effect");
    };
    store
        .authorize_agent_create_start_effect(
            &principal,
            "create-start-safe-failure",
            &payload,
            &effect.operation_id,
            ("runtime-1", "supervisor-1", "lease-generation-1"),
        )
        .await
        .unwrap_or_else(|error| panic!("authorize create/start: {error}"));
    let failure = store
        .fail_agent_create_start(
            &principal,
            "create-start-safe-failure",
            &payload,
            &effect,
            "provider_launch_failed",
            "launch failed safely",
        )
        .await
        .unwrap_or_else(|error| panic!("record safe failure: {error}"));
    assert_eq!(failure.events.len(), 2);
    assert!(
        failure
            .events
            .iter()
            .all(|event| event.event_type != "agent_session_created")
    );
    let event_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_events")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count terminal failure events: {error}"));

    assert!(matches!(
        store
            .inspect_agent_create_start(&principal, "create-start-safe-failure", &payload)
            .await,
        Err(crate::PersistenceError::StoredCommandRejected { code, message })
            if code == failure.code && message == failure.message
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_events")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("recount terminal failure events: {error}")),
        event_count
    );
    let reservation = sqlx::query_as::<_, (String, String, String)>(
        "SELECT status, failure_code, failure_message FROM lifecycle_command_reservations WHERE request_id = 'create-start-safe-failure'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read durable rejection: {error}"));
    assert_eq!(reservation.0, "rejected");
    assert_eq!(reservation.1, failure.code);
    assert_eq!(reservation.2, failure.message);
    let write_count = sqlx::query_scalar::<_, i64>(
        "SELECT command_count FROM room_write_budgets WHERE room_id = 'general'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read terminal failure budget: {error}"));
    assert_eq!(write_count, 1);
    let created_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM room_events WHERE json_extract(event_json, '$.type') = 'agent_session_created'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("count creation events: {error}"));
    assert_eq!(created_count, 1);
}

#[tokio::test]
async fn restart_uncertain_create_start_keeps_one_unresolved_request() {
    let (store, principal, directory) = fixture().await;
    let payload = json!({"start": true, "provider_id": "codex"});
    let plan = store
        .prepare_agent_create_start(
            &principal,
            "create-start-uncertain",
            &payload,
            &draft(directory.path()),
        )
        .await
        .unwrap_or_else(|error| panic!("prepare create/start: {error}"));
    let AgentCreateStartPlan::Start(effect) = plan else {
        panic!("new create/start must own a start effect");
    };
    store
        .authorize_agent_create_start_effect(
            &principal,
            "create-start-uncertain",
            &payload,
            &effect.operation_id,
            (
                "uncertain-runtime",
                "supervisor-instance-1",
                "lease-generation-1",
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("authorize create/start: {error}"));
    store
        .mark_agent_start_unconfirmed(
            &principal,
            &effect.session.public.session_id,
            &effect.operation_id,
            "uncertain-runtime",
            "supervisor-instance-1",
            "runtime_start_unconfirmed",
            "launch effect is ambiguous",
        )
        .await
        .unwrap_or_else(|error| panic!("mark uncertain start: {error}"));
    assert_eq!(
        store
            .reconcile_agent_sessions_after_restart()
            .await
            .unwrap_or_else(|error| panic!("reconcile uncertain start: {error}")),
        1
    );
    let event_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_events")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count recovery events: {error}"));
    for _ in 0..2 {
        assert!(matches!(
            store
                .inspect_agent_create_start(&principal, "create-start-uncertain", &payload)
                .await,
            Err(crate::PersistenceError::CommandUnresolved {
                code: "runtime_effect_unconfirmed",
                ..
            })
        ));
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_events")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("recount recovery events: {error}")),
        event_count
    );
    let reservations = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM lifecycle_command_reservations WHERE request_id = 'create-start-uncertain' AND phase = 'creation_committed'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("count retained reservation: {error}"));
    assert_eq!(reservations, 1);
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(&effect.session.public.session_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read retained session: {error}"));
    let session = serde_json::from_str::<agentsassemble_domain::DurableAgentSession>(&encoded)
        .unwrap_or_else(|error| panic!("decode retained session: {error}"));
    assert_eq!(session.lifecycle_intent_status, "unconfirmed");
}

#[tokio::test]
async fn startup_gone_keeps_created_identity_and_terminalizes_its_old_start() {
    let (store, principal, directory) = fixture().await;
    let payload = json!({"start": true, "provider_id": "codex"});
    let AgentCreateStartPlan::Start(effect) = store
        .prepare_agent_create_start(
            &principal,
            "create-start-abandoned",
            &payload,
            &draft(directory.path()),
        )
        .await
        .unwrap_or_else(|error| panic!("prepare create/start: {error}"))
    else {
        panic!("new create/start must own a start effect");
    };
    store
        .authorize_agent_create_start_effect(
            &principal,
            "create-start-abandoned",
            &payload,
            &effect.operation_id,
            (
                "create-runtime-abandoned",
                "supervisor-dead",
                "lease-generation-1",
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("authorize create/start: {error}"));
    store
        .mark_agent_start_unconfirmed(
            &principal,
            &effect.session.public.session_id,
            &effect.operation_id,
            "create-runtime-abandoned",
            "supervisor-dead",
            "runtime_start_unconfirmed",
            "launch outcome was uncertain",
        )
        .await
        .unwrap_or_else(|error| panic!("mark create/start unconfirmed: {error}"));
    let candidate = store
        .load_runtime_reconciliation_candidates()
        .await
        .unwrap_or_else(|error| panic!("load create/start candidate: {error}"))
        .pop()
        .unwrap_or_else(|| panic!("unconfirmed create/start must be a candidate"));
    store
        .apply_runtime_reconciliation(&candidate, &RuntimeReconciliationObservation::Gone)
        .await
        .unwrap_or_else(|error| panic!("terminalize create/start: {error}"));
    assert!(matches!(
        store
            .inspect_agent_create_start(&principal, "create-start-abandoned", &payload)
            .await,
        Err(PersistenceError::StoredCommandRejected {
            code,
            ..
        }) if code == "runtime_start_recovered_gone"
    ));
    let retry_payload = json!({"agent_id": effect.session.public.session_id});
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "start-created-after-recovery", &retry_payload)
            .await
            .unwrap_or_else(|error| panic!("start retained created session: {error}")),
        AgentStartPlan::Start(_)
    ));
}
