use std::{path::Path, time::Duration};

use serde_json::Value;

use super::{ProviderAdapter, ProviderRuntimeStarted, ProviderTurnRequest, tests::fixture_session};
use crate::room_portal::ProviderTurnOutcome;

fn assert_message(completed: &super::ProviderTurnCompleted, expected: &str) {
    assert_eq!(
        completed.outcome,
        ProviderTurnOutcome::Message {
            content: expected.to_owned(),
            target_agent_id: String::new(),
        }
    );
}

#[tokio::test]
async fn codex_turn_uses_original_settings_and_returns_one_canonical_final() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create provider-turn fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = turn_fixture(
        &transcript,
        "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"thread/status/changed\",\"params\":{\"threadId\":\"thread-1\",\"status\":\"active\"}}'\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{},\"params\":{\"turn\":{\"id\":\"provider-turn-1\"}}}",
        concat!(
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"agent_message/delta\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\",\"delta\":\"draft \"}}'\n",
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"item/completed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\",\"item\":{\"type\":\"agentMessage\",\"text\":\"final answer\"}}}'\n",
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\"}}'\n",
        ),
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start provider turn fixture: {error}"));
    let active = active_session(&session, &started, "room-turn-1");
    let request = ProviderTurnRequest {
        turn_id: "room-turn-1".to_owned(),
        turn_generation: 1,
        execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        input: "Read the canonical room context and answer.".to_owned(),
        room_observation: None,
    };
    let completed = adapter
        .send_turn(&active, &request)
        .await
        .unwrap_or_else(|error| panic!("complete provider turn: {error}"));
    assert_eq!(completed.turn_id, "room-turn-1");
    assert_eq!(completed.provider_turn_id, "provider-turn-1");
    assert_message(&completed, "final answer");

    let replay = adapter
        .send_turn(&active, &request)
        .await
        .unwrap_or_else(|error| panic!("replay completed provider turn: {error}"));
    assert_eq!(replay, completed);
    let requests = requests(&transcript);
    assert_eq!(
        request_methods(&requests),
        ["initialize", "initialized", "thread/start", "turn/start"]
    );
    let turn = &requests[3]["params"];
    assert_eq!(turn["threadId"], "thread-1");
    assert_eq!(turn["input"][0]["type"], "text");
    assert_eq!(turn["input"][0]["text"], request.input);
    assert_eq!(turn["metadata"]["source"], "agentsassemble_agent_session");
    assert_eq!(turn["cwd"], session.workspace);
    assert_eq!(turn["model"], "gpt-5.6-terra");
    assert_eq!(turn["effort"], "high");
    assert_eq!(turn["approvalPolicy"], "never");
    assert_eq!(turn["sandboxPolicy"]["type"], "readOnly");
    assert_eq!(turn["sandboxPolicy"]["networkAccess"], false);
    stop_and_release(&adapter, &active, &started).await;
}

#[tokio::test]
async fn codex_turn_infers_completion_after_final_message_and_thread_idle() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create inferred provider-turn fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = turn_fixture(
        &transcript,
        "",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"turn\":{\"id\":\"provider-turn-1\"}}}",
        concat!(
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"agent_message/completed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\",\"text\":\"idle answer\"}}'\n",
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"thread/status/changed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\",\"status\":{\"type\":\"idle\"}}}'\n",
        ),
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start inferred provider-turn fixture: {error}"));
    let active = active_session(&session, &started, "room-turn-1");
    let completed = adapter
        .send_turn(
            &active,
            &ProviderTurnRequest {
                turn_id: "room-turn-1".to_owned(),
                turn_generation: 1,
                execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
                input: "Finish when the thread becomes idle.".to_owned(),
                room_observation: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("infer provider turn completion: {error}"));
    assert_message(&completed, "idle answer");
    stop_and_release(&adapter, &active, &started).await;
}

#[tokio::test]
async fn nullable_hook_turn_identity_does_not_poison_an_active_turn() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create nullable-hook provider-turn fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = turn_fixture(
        &transcript,
        "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"hook/started\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":null,\"run\":{}}}'\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"turn\":{\"id\":\"provider-turn-1\"}}}",
        concat!(
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"agent_message/completed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\",\"text\":\"answer after hook\"}}'\n",
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\"}}'\n",
        ),
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start nullable-hook provider-turn fixture: {error}"));
    let active = active_session(&session, &started, "room-turn-1");
    let completed = adapter
        .send_turn(
            &active,
            &ProviderTurnRequest {
                turn_id: "room-turn-1".to_owned(),
                turn_generation: 1,
                execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
                input: "Continue after the thread hook.".to_owned(),
                room_observation: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("complete provider turn after hook: {error}"));
    assert_message(&completed, "answer after hook");
    stop_and_release(&adapter, &active, &started).await;
}

#[tokio::test]
async fn cancelled_codex_turn_start_continues_without_retransmission() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create cancelled provider-turn fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let seen = directory.path().join("turn-seen");
    let release = directory.path().join("turn-release");
    let before_response = format!(
        "printf seen > {}\nwhile [ ! -f {} ]; do :; done\n",
        shell_quote(&seen),
        shell_quote(&release),
    );
    let script = turn_fixture(
        &transcript,
        &before_response,
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"turn\":{\"id\":\"provider-turn-1\"}}}",
        concat!(
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"agent_message/completed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\",\"text\":\"continued answer\"}}'\n",
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\"}}'\n",
        ),
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start cancelled provider-turn fixture: {error}"));
    let active = active_session(&session, &started, "room-turn-1");
    let request = ProviderTurnRequest {
        turn_id: "room-turn-1".to_owned(),
        turn_generation: 1,
        execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        input: "Continue exactly once.".to_owned(),
        room_observation: None,
    };
    let pending_adapter = adapter.clone();
    let pending_session = active.clone();
    let pending_request = request.clone();
    let pending = tokio::spawn(async move {
        pending_adapter
            .send_turn(&pending_session, &pending_request)
            .await
    });
    wait_for_file(&seen).await;
    pending.abort();
    let _ = pending.await;
    std::fs::write(&release, b"release")
        .unwrap_or_else(|error| panic!("release provider turn response: {error}"));

    let completed = adapter
        .send_turn(&active, &request)
        .await
        .unwrap_or_else(|error| panic!("recover provider turn response: {error}"));
    assert_message(&completed, "continued answer");
    let requests = requests(&transcript);
    assert_eq!(
        request_methods(&requests),
        ["initialize", "initialized", "thread/start", "turn/start"]
    );
    stop_and_release(&adapter, &active, &started).await;
}

#[tokio::test]
async fn exact_codex_turn_interrupt_uses_official_identity_and_retains_runtime() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create exact interrupt fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let seen = directory.path().join("turn-started");
    let script = exact_interrupt_fixture(&transcript, &seen);
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start exact interrupt fixture: {error}"));
    let active = active_session(&session, &started, "room-turn-1");
    let request = ProviderTurnRequest {
        turn_id: "room-turn-1".to_owned(),
        turn_generation: 1,
        execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        input: "Remain active until interrupted.".to_owned(),
        room_observation: None,
    };
    let prepared = adapter
        .prepare_turn(&active, &request)
        .await
        .unwrap_or_else(|error| panic!("prepare exact interrupt turn: {error}"));
    let authority = prepared.exact_authority();
    let turn_adapter = adapter.clone();
    let turn_session = active.clone();
    let turn_request = request.clone();
    let turn = tokio::spawn(async move {
        turn_adapter
            .send_prepared_turn(prepared, &turn_session, &turn_request)
            .await
    });
    wait_for_file(&seen).await;
    let mut control = adapter
        .begin_exact_turn(&authority)
        .await
        .unwrap_or_else(|error| panic!("resolve exact interrupt control: {error}"));
    assert_eq!(
        control.disposition,
        super::ProviderTurnInterruptDisposition::Started
    );
    control.request_interrupt();
    control
        .wait_quiesced(Duration::from_secs(5))
        .await
        .unwrap_or_else(|error| panic!("prove retained-runtime interruption: {error}"));
    let completed = turn
        .await
        .unwrap_or_else(|error| panic!("join exact interrupt owner: {error}"));
    let Err(error) = completed else {
        panic!("interrupted provider turn must not publish a completion");
    };
    assert_eq!(error.code, "provider_turn_interrupted");
    assert!(!error.effect_uncertain);
    assert!(!error.runtime_stopped);
    let recorded = requests(&transcript);
    assert_eq!(
        request_methods(&recorded),
        [
            "initialize",
            "initialized",
            "thread/start",
            "turn/start",
            "turn/interrupt",
        ]
    );
    assert_eq!(recorded[4]["params"]["threadId"], "thread-1");
    assert_eq!(recorded[4]["params"]["turnId"], "provider-turn-1");
    stop_and_release(&adapter, &active, &started).await;
}

#[tokio::test]
async fn owned_stop_cancels_a_blocked_turn_without_waiting_for_inactivity() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create blocked provider-turn fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let seen = directory.path().join("turn-acknowledged");
    let script = turn_fixture(
        &transcript,
        "",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"turn\":{\"id\":\"provider-turn-1\"}}}",
        &format!("printf seen > {}\n", shell_quote(&seen)),
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start blocked provider-turn fixture: {error}"));
    let active = active_session(&session, &started, "room-turn-1");
    let request = ProviderTurnRequest {
        turn_id: "room-turn-1".to_owned(),
        turn_generation: 1,
        execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        input: "Wait for shutdown.".to_owned(),
        room_observation: None,
    };
    let prepared = adapter
        .prepare_turn(&active, &request)
        .await
        .unwrap_or_else(|error| panic!("prepare blocked provider turn: {error}"));
    let authority = prepared.exact_authority();
    let pending_adapter = adapter.clone();
    let pending_session = active.clone();
    let pending_request = request.clone();
    let pending = tokio::spawn(async move {
        pending_adapter
            .send_prepared_turn(prepared, &pending_session, &pending_request)
            .await
    });
    wait_for_file(&seen).await;
    let mut control = adapter
        .begin_exact_turn(&authority)
        .await
        .unwrap_or_else(|error| panic!("capture blocked provider turn: {error}"));
    tokio::time::timeout(
        Duration::from_secs(5),
        adapter.stop(
            &active.public.room_id,
            &active.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        ),
    )
    .await
    .unwrap_or_else(|_| panic!("owned stop waited on the provider inactivity deadline"))
    .unwrap_or_else(|error| panic!("stop blocked provider turn: {error}"));
    assert_eq!(
        control
            .wait_quiesced(Duration::from_secs(1))
            .await
            .unwrap_or_else(|error| panic!("prove exact stopped runtime: {error}")),
        super::ProviderTurnQuiescence::RuntimeGone
    );
    let turn_result = pending
        .await
        .unwrap_or_else(|error| panic!("join cancelled provider turn: {error}"));
    let Err(turn_error) = turn_result else {
        panic!("provider turn must observe owned shutdown cancellation");
    };
    assert_eq!(turn_error.code, "provider_turn_cancelled");
    adapter
        .release_confirmed_stop(
            &active.public.room_id,
            &active.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await;
}

#[tokio::test]
async fn missing_codex_turn_identity_is_poisoned_without_a_second_turn() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    assert_turn_start_error(
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{}}",
        "provider_turn_unconfirmed",
    )
    .await;
}

#[tokio::test]
async fn conflicting_codex_turn_aliases_are_poisoned_without_a_second_turn() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    assert_turn_start_error(
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"turn\":{\"id\":\"provider-turn-1\"},\"turnId\":\"provider-turn-other\"}}",
        "provider_turn_mismatch",
    )
    .await;
}

#[tokio::test]
async fn changed_codex_turn_model_is_poisoned_without_a_second_turn() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    assert_turn_start_error(
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"turn\":{\"id\":\"provider-turn-1\",\"model\":\"other-model\"}}}",
        "provider_model_mismatch",
    )
    .await;
}

#[tokio::test]
async fn rerouted_codex_turn_model_is_poisoned_without_a_second_turn() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let notifications = concat!(
        "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"model/rerouted\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\",\"fromModel\":\"gpt-5.6-terra\",\"toModel\":\"other-model\"}}'\n",
        "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"agent_message/completed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\",\"text\":\"wrong-model answer\"}}'\n",
        "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\"}}'\n",
    );
    assert_turn_error(
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"turn\":{\"id\":\"provider-turn-1\"}}}",
        "",
        notifications,
        "provider_model_mismatch",
    )
    .await;
}

#[tokio::test]
async fn unscoped_codex_output_is_poisoned_without_a_second_turn() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let before_response = concat!(
        "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"agent_message/completed\",\"params\":{\"text\":\"stale turn output\"}}'\n",
        "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"turn/completed\",\"params\":{}}'\n",
    );
    assert_turn_error(
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"turn\":{\"id\":\"provider-turn-1\"}}}",
        before_response,
        "",
        "provider_protocol_invalid",
    )
    .await;
}

#[tokio::test]
async fn reused_codex_provider_turn_identity_is_poisoned() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create reused provider-turn fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = reused_turn_fixture(&transcript);
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start reused provider-turn fixture: {error}"));
    let first = active_session(&session, &started, "room-turn-1");
    adapter
        .send_turn(
            &first,
            &ProviderTurnRequest {
                turn_id: "room-turn-1".to_owned(),
                turn_generation: 1,
                execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
                input: "First answer.".to_owned(),
                room_observation: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("complete first provider turn: {error}"));
    let second = active_session(&session, &started, "room-turn-2");
    let request = ProviderTurnRequest {
        turn_id: "room-turn-2".to_owned(),
        turn_generation: 1,
        execution_id: "22222222-2222-4222-8222-222222222222".to_owned(),
        input: "Second answer.".to_owned(),
        room_observation: None,
    };
    let Err(error) = adapter.send_turn(&second, &request).await else {
        panic!("reused provider turn identity must fail closed");
    };
    assert_eq!(error.code, "provider_turn_reused");
    assert!(!error.effect_uncertain);
    assert!(error.runtime_stopped);
    let requests = requests(&transcript);
    assert_eq!(
        request_methods(&requests),
        [
            "initialize",
            "initialized",
            "thread/start",
            "turn/start",
            "turn/start"
        ]
    );
    adapter
        .release_confirmed_stop(
            &second.public.room_id,
            &second.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await;
    let Err(replay_error) = adapter.send_turn(&second, &request).await else {
        panic!("stopped poisoned runtime must not accept another turn");
    };
    assert_eq!(replay_error.code, "runtime_owner_mismatch");
}

async fn assert_turn_start_error(response: &str, expected_code: &str) {
    assert_turn_error(response, "", "", expected_code).await;
}

async fn assert_turn_error(
    response: &str,
    before_response: &str,
    notifications: &str,
    expected_code: &str,
) {
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create failed provider-turn fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = turn_fixture(&transcript, before_response, response, notifications);
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start failed provider-turn fixture: {error}"));
    let active = active_session(&session, &started, "room-turn-1");
    let request = ProviderTurnRequest {
        turn_id: "room-turn-1".to_owned(),
        turn_generation: 1,
        execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        input: "This must not be sent twice.".to_owned(),
        room_observation: None,
    };
    let Err(error) = adapter.send_turn(&active, &request).await else {
        panic!("unconfirmed provider turn must fail closed");
    };
    assert_eq!(error.code, expected_code);
    assert!(!error.effect_uncertain);
    assert!(error.runtime_stopped);
    let requests = requests(&transcript);
    assert_eq!(
        request_methods(&requests),
        ["initialize", "initialized", "thread/start", "turn/start"]
    );
    adapter
        .release_confirmed_stop(
            &active.public.room_id,
            &active.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await;
    let Err(replay_error) = adapter.send_turn(&active, &request).await else {
        panic!("stopped poisoned runtime must not accept another turn");
    };
    assert_eq!(replay_error.code, "runtime_owner_mismatch");
}

fn reused_turn_fixture(transcript: &Path) -> String {
    format!(
        "#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' \"$initialize\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nIFS= read -r initialized\nprintf '%s\\n' \"$initialized\" >> {log}\nIFS= read -r thread\nprintf '%s\\n' \"$thread\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"thread\":{{\"id\":\"thread-1\"}}}}}}'\nIFS= read -r first_turn\nprintf '%s\\n' \"$first_turn\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{{\"turn\":{{\"id\":\"provider-turn-1\"}}}}}}'\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"method\":\"agent_message/completed\",\"params\":{{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\",\"text\":\"first answer\"}}}}'\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"method\":\"turn/completed\",\"params\":{{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\"}}}}'\nIFS= read -r second_turn\nprintf '%s\\n' \"$second_turn\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":4,\"result\":{{\"turn\":{{\"id\":\"provider-turn-1\"}}}}}}'\nIFS= read -r forever\n",
        log = shell_quote(transcript),
    )
}

pub(super) fn turn_fixture(
    transcript: &Path,
    before_turn_response: &str,
    turn_response: &str,
    notifications: &str,
) -> String {
    format!(
        "#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' \"$initialize\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nIFS= read -r initialized\nprintf '%s\\n' \"$initialized\" >> {log}\nIFS= read -r thread\nprintf '%s\\n' \"$thread\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"thread\":{{\"id\":\"thread-1\"}}}}}}'\nIFS= read -r turn\nprintf '%s\\n' \"$turn\" >> {log}\n{before}printf '%s\\n' {response}\n{notifications}IFS= read -r forever\n",
        log = shell_quote(transcript),
        before = before_turn_response,
        response = shell_quote_text(turn_response),
        notifications = notifications,
    )
}

fn exact_interrupt_fixture(transcript: &Path, seen: &Path) -> String {
    format!(
        "#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' \"$initialize\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nIFS= read -r initialized\nprintf '%s\\n' \"$initialized\" >> {log}\nIFS= read -r thread\nprintf '%s\\n' \"$thread\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"thread\":{{\"id\":\"thread-1\"}}}}}}'\nIFS= read -r turn\nprintf '%s\\n' \"$turn\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{{\"turn\":{{\"id\":\"provider-turn-1\"}}}}}}'\nprintf seen > {seen}\nIFS= read -r interrupt\nprintf '%s\\n' \"$interrupt\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":4,\"result\":{{}}}}'\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"method\":\"turn/completed\",\"params\":{{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\"}}}}'\nIFS= read -r forever\n",
        log = shell_quote(transcript),
        seen = shell_quote(seen),
    )
}

pub(super) fn active_session(
    session: &agentsassemble_domain::DurableAgentSession,
    started: &ProviderRuntimeStarted,
    turn_id: &str,
) -> agentsassemble_domain::DurableAgentSession {
    let mut active = session.clone();
    "attached".clone_into(&mut active.public.status);
    "busy".clone_into(&mut active.public.runtime_status);
    active.public.provider_session_active = true;
    turn_id.clone_into(&mut active.public.active_turn_id);
    "thinking".clone_into(&mut active.public.turn_phase);
    active
        .provider_session_id
        .clone_from(&started.provider_session_id);
    active
        .runtime_handle_id
        .clone_from(&started.runtime_handle_id);
    active
        .runtime_owner_id
        .clone_from(&started.runtime_owner_id);
    active
        .runtime_lease_token
        .clone_from(&started.runtime_lease_token);
    active.turn_generation = 1;
    active
}

pub(super) fn requests(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read provider-turn transcript: {error}"))
        .lines()
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("decode provider-turn transcript: {error}"))
        })
        .collect()
}

fn request_methods(requests: &[Value]) -> Vec<&str> {
    requests
        .iter()
        .map(|request| {
            request["method"]
                .as_str()
                .unwrap_or_else(|| panic!("provider request method is missing"))
        })
        .collect()
}

pub(super) async fn stop_and_release(
    adapter: &ProviderAdapter,
    session: &agentsassemble_domain::DurableAgentSession,
    started: &ProviderRuntimeStarted,
) {
    adapter
        .stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await
        .unwrap_or_else(|error| panic!("stop provider-turn fixture: {error}"));
    adapter
        .release_confirmed_stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await;
}

async fn wait_for_file(path: &Path) {
    for _ in 0..200 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("provider-turn fixture did not publish its request marker");
}

fn shell_quote(path: &Path) -> String {
    shell_quote_text(&path.to_string_lossy())
}

fn shell_quote_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
