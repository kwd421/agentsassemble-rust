use super::*;
use agentsassemble_domain::{
    AgentSessionDraft, ConfirmationDecision, ConfirmationState, ConfirmedRoomAction,
    DecideDelegatedAction, RequestCreateAgent, stable_content_identity, stable_identity_hash,
};
use same_file::Handle;
use std::fs::File;
use std::path::Path;

fn params(workspace: &str) -> RequestCreateAgent {
    RequestCreateAgent {
        request_id: Uuid::new_v4().to_string(),
        provider_id: "codex".into(),
        catalog_revision: "catalog-1".into(),
        display_name: "Newcomer".into(),
        workspace: workspace.into(),
        model: None,
        reasoning_effort: None,
        service_tier: None,
        variant: None,
        permission_mode: Some("meeting_read_only".into()),
        persona_card_id: None,
        provider_endpoint: None,
        max_output_tokens: None,
    }
}

fn draft(workspace: &str, agent_id: &str, permission_mode: &str) -> AgentSessionDraft {
    let workspace =
        std::fs::canonicalize(workspace).unwrap_or_else(|error| panic!("workspace: {error}"));
    let executable = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .unwrap_or_else(|error| panic!("executable: {error}"));
    let (executable, executable_identity) = executable_authority(&executable);
    AgentSessionDraft {
        agent_id: agent_id.into(),
        display_name: "Newcomer".into(),
        provider_kind: "opencode_server".into(),
        runtime_kind: "opencode".into(),
        connection_kind: "native_cli_bridge".into(),
        executable,
        executable_identity,
        workspace: workspace.to_string_lossy().into_owned(),
        workspace_identity: stable_identity_hash(
            &Handle::from_path(&workspace).unwrap_or_else(|error| panic!("workspace id: {error}")),
        ),
        provider_endpoint: String::new(),
        model: "gpt-5.6-terra".into(),
        reasoning_effort: "medium".into(),
        service_tier: "default".into(),
        variant: String::new(),
        execution_harness: "builtin".into(),
        permission_mode: permission_mode.into(),
        max_output_tokens: 0,
        catalog_revision: "catalog-1".into(),
        persona_card_id: String::new(),
        runtime_profile_key: "profile-1".into(),
        transport: "stdio_jsonl".into(),
    }
}

fn executable_authority(executable: &Path) -> (String, String) {
    let mut file =
        File::open(executable).unwrap_or_else(|error| panic!("open executable: {error}"));
    let handle = Handle::from_file(
        file.try_clone()
            .unwrap_or_else(|error| panic!("clone executable: {error}")),
    )
    .unwrap_or_else(|error| panic!("identify executable: {error}"));
    let identity = stable_content_identity(&handle, &mut file)
        .unwrap_or_else(|error| panic!("hash executable: {error}"));
    (executable.to_string_lossy().into_owned(), identity)
}

async fn agent_count(store: &SqliteStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM agent_sessions WHERE room_id='general'")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count agents: {error}"))
}

async fn create_results(store: &SqliteStore, request_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM command_results WHERE room_id='general' AND request_id=? AND action='agent.create'",
    )
    .bind(request_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("count create results: {error}"))
}

async fn room_events(store: &SqliteStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM room_events WHERE room_id='general'")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count events: {error}"))
}

fn bound_create(request: &agentsassemble_domain::DelegatedActionRequest) -> ConfirmedRoomAction {
    request.action.clone()
}

#[tokio::test]
async fn unconfirmed_create_changes_nothing_and_only_owner_approves() -> TestResult {
    let (store, owner, dir) = fixture().await;
    let delegate = director(&store).await?;
    let workspace = dir.path().to_str().unwrap_or_else(|| panic!("workspace"));
    let params = params(workspace);
    let draft = draft(
        workspace,
        "created-00000000-0000-4000-8000-000000000001",
        "meeting_read_only",
    );
    let before = agent_count(&store).await;
    let pending = store
        .request_delegated_agent_create(&delegate, &params, &draft, |_, _, _| Ok(()))
        .await?
        .0;
    assert_eq!(pending["status"], "pending_confirmation");
    let id = pending["confirmation_id"].as_str().ok_or("confirmation")?;
    assert_eq!(agent_count(&store).await, before);
    assert_eq!(create_results(&store, &params.request_id).await, 0);
    // Exact retries return the same receipt without another debit or mutation.
    let retry = store
        .request_delegated_agent_create::<()>(&delegate, &params, &draft, |_, _, _| {
            panic!("retry charged")
        })
        .await?
        .0;
    assert_eq!(pending, retry);
    assert_eq!(agent_count(&store).await, before);
    // Changed parameters under the same identity conflict instead of executing.
    let mut changed = params.clone();
    changed.display_name = "Someone else".into();
    assert!(
        matches!(store.request_delegated_agent_create::<()>(&delegate, &changed, &draft, |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "request_conflict")
    );
    // The delegate itself cannot approve its own request.
    let mut impostor = owner.clone();
    impostor.participant_id = delegate.delegate_id().to_owned();
    let decision = DecideDelegatedAction {
        confirmation_id: id.into(),
        decision: ConfirmationDecision::Approve,
        full_access_acknowledged: None,
    };
    assert!(
        matches!(store.decide_delegated_action(RoomMutationAuthority::TrustedPrincipal(&impostor), &decision).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "permission_denied")
    );
    // Approval alone still creates nothing; the room-actor drive executes.
    let ack = store
        .decide_delegated_action(RoomMutationAuthority::TrustedPrincipal(&owner), &decision)
        .await?;
    assert_eq!(ack.result["state"], "executing");
    assert_eq!(agent_count(&store).await, before);
    assert_eq!(create_results(&store, &params.request_id).await, 0);
    // The drive creates one stopped agent owned by the owner, attributed to the AI.
    let (request, owner_reference, _) = store.approved_create_snapshot("general", id).await?;
    let ConfirmedRoomAction::CreateAgent(bound) = bound_create(&request) else {
        return Err("bound creation missing".into());
    };
    assert_eq!(bound.params, params);
    assert_eq!(bound.draft, draft);
    let payload = crate::create_execution_payload(&bound.params, &bound.draft, &request);
    let drive = store
        .drive_approved_agent_create("general", id, &bound.draft, &payload, &owner_reference)
        .await?;
    assert_eq!(drive.state, ConfirmationState::Succeeded);
    assert_eq!(drive.receipt["status"], "succeeded");
    assert_eq!(drive.receipt["target_id"], json!(draft.agent_id));
    assert_eq!(drive.receipt["confirmation_id"], json!(id));
    // The AI receipt carries only ids, never paths, identities or credentials.
    let mut receipt_keys: Vec<&str> = drive
        .receipt
        .as_object()
        .ok_or("receipt")?
        .keys()
        .map(String::as_str)
        .collect();
    receipt_keys.sort_unstable();
    assert_eq!(
        receipt_keys,
        vec![
            "action",
            "confirmation_id",
            "event_id",
            "request_id",
            "status",
            "target_id"
        ]
    );
    let created = store
        .snapshot("general", 0, 200)
        .await?
        .agent_sessions
        .into_iter()
        .find(|agent| agent.session_id == draft.agent_id)
        .ok_or("created agent missing")?;
    assert_eq!(
        created.runtime_status,
        agentsassemble_domain::AgentRuntimeStatus::Stopped
    );
    assert!(!created.enabled);
    let participant = store.participant("general", &draft.agent_id).await?;
    assert_eq!(participant.owner_id, owner.participant_id);
    assert_eq!(create_results(&store, &params.request_id).await, 1);
    let attributed: Vec<_> = store
        .snapshot("general", 0, 300)
        .await?
        .events
        .into_iter()
        .filter(|event| event.extra.get("confirmation_id") == Some(&json!(id)))
        .collect();
    assert!(attributed.len() >= 2);
    for event in &attributed {
        assert_eq!(event.actor.participant_id, delegate.delegate_id());
        assert_eq!(
            event.extra["on_behalf_of_participant_id"],
            json!(owner.participant_id)
        );
    }
    assert!(attributed.iter().any(|event| {
        event.message_kind.as_deref() == Some("system")
            && event
                .content
                .as_ref()
                .is_some_and(|text| text.contains("on behalf of"))
    }));
    // Re-driving the recorded execution returns the same receipt without new effects.
    let events_before = room_events(&store).await;
    let again = store
        .drive_approved_agent_create("general", id, &bound.draft, &payload, &owner_reference)
        .await?;
    assert_eq!(again.receipt, drive.receipt);
    assert!(again.new_events.is_empty());
    assert_eq!(agent_count(&store).await, before + 1);
    assert_eq!(room_events(&store).await, events_before);
    Ok(())
}
