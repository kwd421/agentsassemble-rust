use std::{fs::File, path::Path};

use agentsassemble_domain::{
    AgentSessionDraft, AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
    LOCAL_OPERATOR_PARTICIPANT_ID, Participant, ParticipantStatus, Room, RoomSettings,
    stable_content_identity, stable_identity_hash,
};
use chrono::Utc;
use same_file::Handle;
use serde_json::json;

use crate::{AgentCreateStartPlan, AgentRuntimeStarted, SqliteStore};

async fn fixture() -> (SqliteStore, AuthenticatedPrincipal, tempfile::TempDir) {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("open store: {error}"));
    let now = Utc::now();
    store
        .initialize_room(
            &Room::new("general".to_owned(), "General".to_owned(), now),
            &RoomSettings::defaults("General".to_owned()),
            &Participant {
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
            },
        )
        .await
        .unwrap_or_else(|error| panic!("initialize room: {error}"));
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
        runtime_profile_key: "profile-create-start-1".to_owned(),
        transport: "stdio_jsonl".to_owned(),
    }
}

fn started() -> AgentRuntimeStarted {
    AgentRuntimeStarted {
        runtime_handle_id: "runtime-1".to_owned(),
        runtime_owner_id: "supervisor-1".to_owned(),
        provider_session_id: "provider-session-1".to_owned(),
        runtime_reused: false,
        provider_session_reused: false,
        provider_session_active: true,
    }
}

#[tokio::test]
async fn create_start_first_commit_replays_one_intent_and_preserves_result_shape() {
    let (store, principal, directory) = fixture().await;
    let payload = json!({"start": true, "provider_id": "codex"});
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
    assert_eq!(first.operation_id, replay.operation_id);
    assert_eq!(
        first.session.public.session_id,
        replay.session.public.session_id
    );
    assert_eq!(first.committed_events, replay.committed_events);
    assert_eq!(first.committed_events.len(), 1);
    assert_eq!(first.newly_committed_events, first.committed_events);
    assert!(replay.newly_committed_events.is_empty());

    let commit = store
        .complete_agent_create_start(
            &principal,
            "create-start-1",
            &payload,
            &first.operation_id,
            &started(),
        )
        .await
        .unwrap_or_else(|error| panic!("complete create/start: {error}"));
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

    let replay = store
        .inspect_agent_create_start(&principal, "create-start-1", &payload)
        .await
        .unwrap_or_else(|error| panic!("replay completed command: {error}"));
    let AgentCreateStartPlan::Outcome(replay) = replay else {
        panic!("completed create/start must replay its ACK");
    };
    assert!(replay.deduplicated);
    assert_eq!(replay.result, commit.outcome.result);
}

#[tokio::test]
async fn safe_start_failure_reuses_exact_creation_without_a_second_created_event() {
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
    let events = store
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
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .all(|event| event.event_type != "agent_session_created")
    );

    let retry = store
        .prepare_agent_create_start(
            &principal,
            "create-start-safe-failure",
            &payload,
            &draft(directory.path()),
        )
        .await
        .unwrap_or_else(|error| panic!("retry exact create/start: {error}"));
    let AgentCreateStartPlan::Start(retry) = retry else {
        panic!("safe failure must retry the exact created session");
    };
    assert_eq!(
        effect.session.public.session_id,
        retry.session.public.session_id
    );
    assert!(retry.committed_events.is_empty());
    let created_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM room_events WHERE json_extract(event_json, '$.type') = 'agent_session_created'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("count creation events: {error}"));
    assert_eq!(created_count, 1);
}

#[tokio::test]
async fn uncertain_create_start_keeps_reservation_and_blocks_blind_retransmission() {
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
        .mark_agent_start_unconfirmed(
            &principal,
            &effect.session.public.session_id,
            &effect.operation_id,
            "",
            "",
            "runtime_start_unconfirmed",
            "launch effect is ambiguous",
        )
        .await
        .unwrap_or_else(|error| panic!("mark uncertain start: {error}"));
    let error = store
        .inspect_agent_create_start(&principal, "create-start-uncertain", &payload)
        .await
        .err()
        .unwrap_or_else(|| panic!("uncertain effect without a handle must not retransmit"));
    assert!(matches!(
        error,
        crate::PersistenceError::CommandRejected {
            code: "runtime_effect_unconfirmed",
            ..
        }
    ));
    let reservations = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM lifecycle_command_reservations WHERE request_id = 'create-start-uncertain' AND phase = 'creation_committed'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("count retained reservation: {error}"));
    assert_eq!(reservations, 1);
}
