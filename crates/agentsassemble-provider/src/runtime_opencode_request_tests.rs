use std::time::Duration;

use agentsassemble_domain::ProviderRequestResolution;
use serde_json::json;

use super::provider_turn_tests::{active_session, stop_and_release};
use super::{ProviderAdapter, ProviderTurnRequest, tests::fixture_session};
use crate::{ProviderRequestExchange, ProviderRequestIngress, profile::runtime_profile_key};

#[tokio::test]
async fn opencode_native_permission_waits_for_exact_http_and_room_receipts()
-> Result<(), Box<dyn std::error::Error>> {
    check_native_permissions("meeting_read_only", false).await
}

#[tokio::test]
async fn opencode_workspace_shell_keeps_human_approval() -> Result<(), Box<dyn std::error::Error>> {
    check_native_permissions("workspace_write", false).await
}

#[tokio::test]
async fn opencode_external_read_only_keeps_native_denial() -> Result<(), Box<dyn std::error::Error>>
{
    check_native_permissions("meeting_read_only", true).await
}

async fn check_native_permissions(
    mode: &str,
    external: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()?;
    let mut session = fixture_session(
        directory.path(),
        include_str!("runtime_opencode_request_fixture.py"),
    )
    .await;
    configure_session(&mut session, mode);
    session.public.external_owned = external;
    let adapter = ProviderAdapter::new();
    let started = adapter.start(&session).await?;
    let creation: serde_json::Value = serde_json::from_slice(&std::fs::read(
        directory.path().join("session-request.json"),
    )?)?;
    assert_eq!(
        creation["permission"][0]["action"],
        if mode == "meeting_read_only" {
            "deny"
        } else {
            "ask"
        }
    );
    assert_eq!(
        creation["permission"][1],
        json!({"permission":"bash", "pattern":"*", "action":if external { "deny" } else { "ask" }})
    );
    let mut active = active_session(&session, &started, "room-turn-1");
    if external {
        stop_and_release(&adapter, &active, &started).await;
        return Ok(());
    }
    for (index, instructions) in [
        Some("fixed room instructions"),
        Some("fixed room instructions"),
        Some("changed card"),
        None,
    ]
    .iter()
    .enumerate()
    {
        active.public.active_turn_id = format!("room-turn-{index}");
        let (ingress, mut commands) = ProviderRequestIngress::channel(1);
        let request = ProviderTurnRequest {
            session_instructions: instructions.map(str::to_owned),
            request_ingress: Some(ingress),
            turn_id: active.public.active_turn_id.clone(),
            turn_generation: 1,
            execution_id: uuid::Uuid::new_v4().to_string(),
            input: "Ask".to_owned(),
            room_observation: None,
        };
        let turn_adapter = adapter.clone();
        let turn_session = active.clone();
        let turn =
            tokio::spawn(async move { turn_adapter.send_turn(&turn_session, &request).await });
        let command = tokio::time::timeout(Duration::from_secs(10), commands.recv())
            .await?
            .ok_or("request channel closed")?;
        assert_eq!(command.session_id, active.public.session_id);
        assert!(command.shell_permission);
        let policy_denied = index == 3 && mode == "meeting_read_only";
        let (exchange, mut responder, mut delivery) = ProviderRequestExchange::channel();
        if policy_denied {
            command.complete(Err(crate::ProviderRequestExchangeError::ShellDenied));
        } else {
            command.complete(Ok(exchange));
            responder.respond(ProviderRequestResolution::Option {
                option_id: "once".to_owned(),
            })?;
            assert!(delivery.completion().await);
            assert!(!turn.is_finished());
            delivery.finish(Ok(()));
        }
        assert_eq!(turn.await??.provider_turn_id, "assistant-1");
        let reply: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.path().join("native-reply.json"))?)?;
        assert_eq!(
            reply,
            json!({"reply": if policy_denied { "reject" } else { "once" }})
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("room-instructions.txt"))?,
            instructions.unwrap_or_default()
        );
        let message: serde_json::Value = serde_json::from_slice(&std::fs::read(
            directory.path().join("prompt-request.json"),
        )?)?;
        assert_eq!(message["agent"], "build");
        assert_eq!(message["parts"][0]["text"], "Ask");
        assert!(!directory.path().join("opencode.json").exists());
    }
    let refreshes = std::fs::read_to_string(directory.path().join("agent-refreshes.jsonl"))?;
    assert_eq!(refreshes.lines().count(), 3);
    stop_and_release(&adapter, &active, &started).await;
    Ok(())
}

fn configure_session(session: &mut agentsassemble_domain::DurableAgentSession, mode: &str) {
    let registration = &crate::registration::OPENCODE_PROVIDER;
    registration
        .provider_kind
        .clone_into(&mut session.public.provider_kind);
    registration
        .runtime_kind
        .clone_into(&mut session.public.runtime_kind);
    mode.clone_into(&mut session.public.permission_mode);
    "opencode/fixture".clone_into(&mut session.public.model);
    registration
        .transport
        .clone_into(&mut session.public.transport);
    session.public.reasoning_effort.clear();
    session.public.service_tier.clear();
    session.runtime_profile_key = runtime_profile_key([
        &session.public.provider_kind,
        &session.public.runtime_kind,
        &session.executable,
        &session.executable_identity,
        &session.workspace,
        &session.workspace_identity,
        &session.provider_endpoint,
        &session.public.model,
        &session.public.reasoning_effort,
        &session.public.service_tier,
        &session.public.variant,
        &session.public.execution_harness,
        &session.public.permission_mode,
        &session.public.persona_card_id,
        &session.public.transport,
    ]);
}
