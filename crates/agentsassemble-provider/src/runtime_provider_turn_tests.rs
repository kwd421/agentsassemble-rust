use std::{path::Path, time::Duration};

use serde_json::Value;

use super::{ProviderAdapter, ProviderRuntimeStarted, ProviderTurnRequest, tests::fixture_session};

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
        input: "Read the canonical room context and answer.".to_owned(),
    };
    let completed = adapter
        .send_turn(&active, &request)
        .await
        .unwrap_or_else(|error| panic!("complete provider turn: {error}"));
    assert_eq!(completed.turn_id, "room-turn-1");
    assert_eq!(completed.provider_turn_id, "provider-turn-1");
    assert_eq!(completed.content, "final answer");

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
                input: "Finish when the thread becomes idle.".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("infer provider turn completion: {error}"));
    assert_eq!(completed.content, "idle answer");
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
        input: "Continue exactly once.".to_owned(),
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
    assert_eq!(completed.content, "continued answer");
    let requests = requests(&transcript);
    assert_eq!(
        request_methods(&requests),
        ["initialize", "initialized", "thread/start", "turn/start"]
    );
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
    let pending_adapter = adapter.clone();
    let pending_session = active.clone();
    let pending = tokio::spawn(async move {
        pending_adapter
            .send_turn(
                &pending_session,
                &ProviderTurnRequest {
                    turn_id: "room-turn-1".to_owned(),
                    input: "Wait for shutdown.".to_owned(),
                },
            )
            .await
    });
    wait_for_file(&seen).await;
    tokio::time::timeout(
        Duration::from_secs(5),
        adapter.stop(
            &active.public.room_id,
            &active.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
        ),
    )
    .await
    .unwrap_or_else(|_| panic!("owned stop waited on the provider inactivity deadline"))
    .unwrap_or_else(|error| panic!("stop blocked provider turn: {error}"));
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

async fn assert_turn_start_error(response: &str, expected_code: &str) {
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create failed provider-turn fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = turn_fixture(&transcript, "", response, "");
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start failed provider-turn fixture: {error}"));
    let active = active_session(&session, &started, "room-turn-1");
    let request = ProviderTurnRequest {
        turn_id: "room-turn-1".to_owned(),
        input: "This must not be sent twice.".to_owned(),
    };
    for _ in 0..2 {
        let Err(error) = adapter.send_turn(&active, &request).await else {
            panic!("unconfirmed provider turn must remain failed closed");
        };
        assert_eq!(error.code, expected_code);
        assert!(error.effect_uncertain);
    }
    let requests = requests(&transcript);
    assert_eq!(
        request_methods(&requests),
        ["initialize", "initialized", "thread/start", "turn/start"]
    );
    stop_and_release(&adapter, &active, &started).await;
}

fn turn_fixture(
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

fn active_session(
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
}

fn requests(path: &Path) -> Vec<Value> {
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

async fn stop_and_release(
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
        )
        .await
        .unwrap_or_else(|error| panic!("stop provider-turn fixture: {error}"));
    adapter
        .release_confirmed_stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
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
