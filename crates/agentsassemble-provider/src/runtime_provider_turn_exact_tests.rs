use std::time::Duration;

use super::{
    ProviderAdapter, ProviderTurnRequest,
    provider_turn_tests::{active_session, requests, stop_and_release, turn_fixture},
    tests::fixture_session,
};

#[tokio::test]
async fn completed_turn_remains_exactly_owned_until_durable_terminal_release() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create retained-result fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = turn_fixture(
        &transcript,
        "",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"turn\":{\"id\":\"provider-turn-1\"}}}",
        concat!(
            "printf '%s\n' '{\"jsonrpc\":\"2.0\",\"method\":\"agent_message/completed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\",\"text\":\"retained answer\"}}'\n",
            "printf '%s\n' '{\"jsonrpc\":\"2.0\",\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\"}}'\n",
        ),
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start retained-result fixture: {error}"));
    let active = active_session(&session, &started, "room-turn-1");
    let request = ProviderTurnRequest {
        turn_id: "room-turn-1".to_owned(),
        turn_generation: 1,
        execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        input: "Retain this result until its durable commit.".to_owned(),
        room_observation: None,
    };
    let prepared = adapter
        .prepare_turn(&active, &request)
        .await
        .unwrap_or_else(|error| panic!("prepare retained result: {error}"));
    let authority = prepared.exact_authority();
    let completed = adapter
        .send_prepared_turn(prepared, &active, &request)
        .await
        .unwrap_or_else(|error| panic!("complete retained result: {error}"));

    assert!(adapter.owns_exact_turn(&authority).await);
    assert_eq!(
        adapter.retained_turn_result(&authority).await,
        Some(Ok(completed))
    );
    let control = adapter
        .begin_exact_turn(&authority)
        .await
        .unwrap_or_else(|error| panic!("resolve quiesced exact result: {error}"));
    assert_eq!(
        control.disposition,
        super::ProviderTurnInterruptDisposition::Quiesced
    );

    adapter.release_terminal_turn(&authority).await;
    assert!(!adapter.owns_exact_turn(&authority).await);
    let Err(error) = adapter.begin_exact_turn(&authority).await else {
        panic!("durably released turn must no longer have exact control");
    };
    assert_eq!(error.code, "stale_provider_turn");
    stop_and_release(&adapter, &active, &started).await;
}

#[tokio::test]
async fn unstarted_turn_remains_exactly_owned_until_durable_interrupt_release() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create unstarted-turn fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = turn_fixture(
        &transcript,
        "",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"turn\":{\"id\":\"provider-turn-1\"}}}",
        "",
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start unstarted-turn fixture: {error}"));
    let active = active_session(&session, &started, "room-turn-1");
    let request = ProviderTurnRequest {
        turn_id: "room-turn-1".to_owned(),
        turn_generation: 1,
        execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        input: "Do not dispatch this turn.".to_owned(),
        room_observation: None,
    };
    let prepared = adapter
        .prepare_turn(&active, &request)
        .await
        .unwrap_or_else(|error| panic!("prepare unstarted turn: {error}"));
    let authority = prepared.exact_authority();
    let mut control = adapter
        .begin_exact_turn(&authority)
        .await
        .unwrap_or_else(|error| panic!("capture unstarted turn: {error}"));
    assert_eq!(
        control.disposition,
        super::ProviderTurnInterruptDisposition::NotStarted
    );

    adapter.discard_prepared_turn(&prepared).await;
    assert_eq!(
        control
            .wait_quiesced(Duration::from_secs(1))
            .await
            .unwrap_or_else(|error| panic!("prove unstarted turn: {error}")),
        super::ProviderTurnQuiescence::RuntimeRetained
    );
    assert!(adapter.owns_exact_turn(&authority).await);
    assert_eq!(adapter.retained_turn_result(&authority).await, None);
    adapter.release_terminal_turn(&authority).await;
    assert!(!adapter.owns_exact_turn(&authority).await);
    assert!(
        requests(&transcript)
            .iter()
            .all(|request| { request["method"].as_str() != Some("turn/start") })
    );
    stop_and_release(&adapter, &active, &started).await;
}
