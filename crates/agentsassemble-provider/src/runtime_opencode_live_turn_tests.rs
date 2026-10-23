//! Opt-in real turn against the locally installed `OpenCode` 2 and a zero-cost model.
//!
//! Run with:
//! `cargo test -p agentsassemble-provider --lib opencode_live -- --ignored --nocapture`
use std::time::Duration;

use super::{
    ProviderAdapter, ProviderRoomObservation, ProviderTurnRequest,
    provider_turn_tests::{active_session, stop_and_release},
    tests::RUNTIME_TEST_LOCK,
};
use crate::{
    filesystem::canonical_workspace, profile::runtime_profile_key, test_support::durable_session,
};

const DEFAULT_EXECUTABLE: &str = "/opt/homebrew/bin/opencode";
const DEFAULT_MODEL: &str = "opencode/deepseek-v4.1-flash-free";

#[tokio::test]
#[ignore = "requires the locally installed OpenCode 2 and a zero-cost model"]
async fn opencode_live_free_model_turn_completes() -> Result<(), Box<dyn std::error::Error>> {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let executable =
        std::env::var("OPENCODE_LIVE_EXECUTABLE").unwrap_or_else(|_| DEFAULT_EXECUTABLE.to_owned());
    let model = std::env::var("OPENCODE_LIVE_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_owned());
    let executable = std::fs::canonicalize(&executable)?
        .to_string_lossy()
        .into_owned();
    let directory = tempfile::tempdir()?;
    let (workspace, workspace_identity) =
        canonical_workspace(directory.path().to_string_lossy().into_owned())
            .await
            .unwrap_or_else(|error| panic!("identify live workspace: {error:?}"));
    let mut session = durable_session(
        "live-room",
        "live-agent",
        "Live",
        "opencode_server",
        &model,
        "http",
    );
    session.public.runtime_kind = "opencode".to_owned();
    session.public.execution_harness = "builtin".to_owned();
    session.public.reasoning_effort.clear();
    session.public.permission_mode = "meeting_read_only".to_owned();
    session.executable = executable.clone();
    session.executable_identity =
        crate::filesystem::runtime_executable_identity("opencode_server", executable)
            .await
            .unwrap_or_else(|error| panic!("identify live executable: {error:?}"));
    session.workspace = workspace;
    session.workspace_identity = workspace_identity;
    session.runtime_profile_key = runtime_profile_key([
        session.public.provider_kind.as_str(),
        session.public.runtime_kind.as_str(),
        session.executable.as_str(),
        session.executable_identity.as_str(),
        session.workspace.as_str(),
        session.workspace_identity.as_str(),
        session.provider_endpoint.as_str(),
        session.public.model.as_str(),
        session.public.reasoning_effort.as_str(),
        session.public.service_tier.as_str(),
        session.public.variant.as_str(),
        session.public.execution_harness.as_str(),
        session.public.permission_mode.as_str(),
        session.public.persona_card_id.as_ref(),
        session.public.transport.as_str(),
    ]);
    let adapter = ProviderAdapter::new();
    let started = adapter.start(&session).await?;
    let active = active_session(&session, &started, "live-turn-1");
    let request = ProviderTurnRequest {
        context_tokens: None,
        vision: false,
        owner_started: false,
        memory: None,
        session_instructions: Some("You are a live test room member.".to_owned()),
        request_ingress: None,
        turn_id: "live-turn-1".to_owned(),
        turn_generation: 1,
        execution_id: uuid::Uuid::new_v4().to_string(),
        input: "First call agentsassemble_room_read_discussion with {}. Then call \
                agentsassemble_room_publish_message with content exactly \
                \"live portal reply\". Do not answer directly."
            .to_owned(),
        room_observation: Some(ProviderRoomObservation {
            session_id: "live-agent".to_owned(),
            input_up_to_seq: 1,
            view: "#1 Human: Say hello in the room.".to_owned(),
            attachment_ids: Vec::new(),
            attachment_ingress: None,
            allowed_agent_ids: Vec::new(),
            tabletop_tools: false,
            room_tool_ingress: None,
        }),
    };
    let completed =
        tokio::time::timeout(Duration::from_mins(3), adapter.send_turn(&active, &request))
            .await??;
    assert_eq!(completed.turn_id, "live-turn-1");
    assert!(
        completed.provider_turn_id.starts_with("msg_"),
        "provider turn id: {}",
        completed.provider_turn_id
    );
    match &completed.outcome {
        crate::room_portal::ProviderTurnOutcome::Message { content, .. } => {
            println!("LIVE_TURN_CONTENT={content}");
            assert!(!content.trim().is_empty());
        }
        other => panic!("expected a published message, got {other:?}"),
    }
    stop_and_release(&adapter, &active, &started).await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires the locally installed OpenCode 2 and a zero-cost model"]
async fn opencode_live_read_only_turn_cannot_execute_shell()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let executable =
        std::env::var("OPENCODE_LIVE_EXECUTABLE").unwrap_or_else(|_| DEFAULT_EXECUTABLE.to_owned());
    let model = std::env::var("OPENCODE_LIVE_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_owned());
    let executable = std::fs::canonicalize(&executable)?
        .to_string_lossy()
        .into_owned();
    let directory = tempfile::tempdir()?;
    let marker = directory.path().join("created-by-shell");
    let (workspace, workspace_identity) =
        canonical_workspace(directory.path().to_string_lossy().into_owned())
            .await
            .unwrap_or_else(|error| panic!("identify live workspace: {error:?}"));
    let mut session = durable_session(
        "live-room",
        "live-agent",
        "Live",
        "opencode_server",
        &model,
        "http",
    );
    session.public.runtime_kind = "opencode".to_owned();
    session.public.execution_harness = "builtin".to_owned();
    session.public.reasoning_effort.clear();
    session.public.permission_mode = "meeting_read_only".to_owned();
    session.executable = executable.clone();
    session.executable_identity =
        crate::filesystem::runtime_executable_identity("opencode_server", executable)
            .await
            .unwrap_or_else(|error| panic!("identify live executable: {error:?}"));
    session.workspace = workspace;
    session.workspace_identity = workspace_identity;
    session.runtime_profile_key = runtime_profile_key([
        session.public.provider_kind.as_str(),
        session.public.runtime_kind.as_str(),
        session.executable.as_str(),
        session.executable_identity.as_str(),
        session.workspace.as_str(),
        session.workspace_identity.as_str(),
        session.provider_endpoint.as_str(),
        session.public.model.as_str(),
        session.public.reasoning_effort.as_str(),
        session.public.service_tier.as_str(),
        session.public.variant.as_str(),
        session.public.execution_harness.as_str(),
        session.public.permission_mode.as_str(),
        session.public.persona_card_id.as_ref(),
        session.public.transport.as_str(),
    ]);
    let adapter = ProviderAdapter::new();
    let started = adapter.start(&session).await?;
    let active = active_session(&session, &started, "live-turn-1");
    let request = ProviderTurnRequest {
        context_tokens: None,
        vision: false,
        owner_started: false,
        memory: None,
        session_instructions: None,
        request_ingress: None,
        turn_id: "live-turn-1".to_owned(),
        turn_generation: 1,
        execution_id: uuid::Uuid::new_v4().to_string(),
        input: "Use the shell tool to run `touch created-by-shell` in the working directory, \
                then reply DONE."
            .to_owned(),
        room_observation: None,
    };
    let _ =
        tokio::time::timeout(Duration::from_mins(3), adapter.send_turn(&active, &request)).await;
    assert!(!marker.exists(), "read-only turn executed a shell write");
    stop_and_release(&adapter, &active, &started).await;
    Ok(())
}
