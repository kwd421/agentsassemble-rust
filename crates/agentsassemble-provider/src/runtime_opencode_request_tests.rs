use std::time::Duration;

use agentsassemble_domain::ProviderRequestResolution;
use serde_json::json;

use super::provider_turn_tests::{active_session, stop_and_release};
use super::{ProviderAdapter, ProviderTurnRequest, tests::fixture_session};
use crate::{ProviderRequestExchange, ProviderRequestIngress, profile::runtime_profile_key};

#[tokio::test]
async fn opencode_native_permission_waits_for_exact_http_and_room_receipts()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()?;
    let mut session = fixture_session(
        directory.path(),
        include_str!("runtime_opencode_request_fixture.py"),
    )
    .await;
    let registration = &crate::registration::OPENCODE_PROVIDER;
    session.public.provider_kind = registration.provider_kind.to_owned();
    session.public.runtime_kind = registration.runtime_kind.to_owned();
    session.public.model = "opencode/fixture".to_owned();
    session.public.transport = registration.transport.to_owned();
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
    let adapter = ProviderAdapter::new();
    let started = adapter.start(&session).await?;
    let active = active_session(&session, &started, "room-turn-1");
    let (ingress, mut commands) = ProviderRequestIngress::channel(1);
    let request = ProviderTurnRequest {
        request_ingress: Some(ingress),
        turn_id: "room-turn-1".to_owned(),
        turn_generation: 1,
        execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        input: "Ask".to_owned(),
        room_observation: None,
    };
    let turn_adapter = adapter.clone();
    let turn_session = active.clone();
    let turn = tokio::spawn(async move { turn_adapter.send_turn(&turn_session, &request).await });
    let command = tokio::time::timeout(Duration::from_secs(10), commands.recv())
        .await?
        .ok_or("request channel closed")?;
    assert_eq!(command.session_id, active.public.session_id);
    let (exchange, mut responder, mut delivery) = ProviderRequestExchange::channel();
    command.complete(Ok(exchange));
    responder.respond(ProviderRequestResolution::Option {
        option_id: "once".to_owned(),
    })?;
    assert!(delivery.completion().await);
    assert!(!turn.is_finished());
    delivery.finish(Ok(()));
    assert_eq!(turn.await??.provider_turn_id, "assistant-1");
    let reply: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("native-reply.json"))?)?;
    assert_eq!(reply, json!({"reply": "once"}));
    stop_and_release(&adapter, &active, &started).await;
    Ok(())
}
