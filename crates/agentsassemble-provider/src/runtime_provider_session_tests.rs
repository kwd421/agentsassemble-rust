use std::{path::Path, time::Duration};

use serde_json::Value;

use super::{ProviderAdapter, tests::fixture_session};

#[tokio::test]
async fn codex_thread_starts_once_then_resumes_its_durable_identity() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create provider-session fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = transcript_fixture(&transcript, "");
    let session = fixture_session(directory.path(), &script).await;
    let first_adapter = ProviderAdapter::new();
    let first = first_adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start provider thread: {error}"));
    assert_eq!(first.provider_session_id, "thread-1");
    assert!(first.provider_session_active);
    assert!(!first.provider_session_reused);

    let cached = first_adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("reuse cached provider thread: {error}"));
    assert!(cached.runtime_reused);
    assert!(!cached.provider_session_reused);
    assert_eq!(
        request_methods(&transcript),
        ["initialize", "initialized", "thread/start"]
    );

    stop_and_release(&first_adapter, &session, &first).await;
    let mut durable = session.clone();
    durable.provider_session_id = first.provider_session_id.clone();
    let resumed_adapter = ProviderAdapter::new();
    let resumed = resumed_adapter
        .start(&durable)
        .await
        .unwrap_or_else(|error| panic!("resume durable provider thread: {error}"));
    assert_eq!(resumed.provider_session_id, "thread-1");
    assert!(resumed.provider_session_active);
    assert!(resumed.provider_session_reused);
    let requests = requests(&transcript);
    assert_eq!(requests[2]["params"]["cwd"], session.workspace);
    assert_eq!(requests[2]["params"]["model"], "gpt-5.6-terra");
    assert_eq!(requests[2]["params"]["approvalPolicy"], "never");
    assert_eq!(requests[2]["params"]["sandbox"], "read-only");
    assert_eq!(requests[5]["method"], "thread/resume");
    assert_eq!(requests[5]["params"]["threadId"], "thread-1");
    stop_and_release(&resumed_adapter, &durable, &resumed).await;
}

#[tokio::test]
async fn cancelled_thread_start_is_read_on_retry_without_retransmission() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create cancelled-thread fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let seen = directory.path().join("thread-start-seen");
    let release = directory.path().join("thread-start-release");
    let script = transcript_fixture(
        &transcript,
        &format!(
            "printf seen > {}\nwhile [ ! -f {} ]; do :; done\n",
            shell_quote(&seen),
            shell_quote(&release),
        ),
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let pending_adapter = adapter.clone();
    let pending_session = session.clone();
    let pending = tokio::spawn(async move { pending_adapter.start(&pending_session).await });
    wait_for_file(&seen).await;
    pending.abort();
    let _ = pending.await;
    std::fs::write(&release, b"release")
        .unwrap_or_else(|error| panic!("release provider response: {error}"));

    let recovered = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("recover pending thread/start response: {error}"));
    assert_eq!(recovered.provider_session_id, "thread-1");
    assert_eq!(
        request_methods(&transcript),
        ["initialize", "initialized", "thread/start"]
    );
    stop_and_release(&adapter, &session, &recovered).await;
}

#[tokio::test]
async fn durable_resume_rejects_a_changed_provider_identity() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create resume-mismatch fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let response =
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"thread\":{\"id\":\"thread-other\"}}}";
    let script = transcript_fixture_with_response(&transcript, "", response);
    let mut session = fixture_session(directory.path(), &script).await;
    "thread-1".clone_into(&mut session.provider_session_id);
    let adapter = ProviderAdapter::new();
    let Err(error) = adapter.start(&session).await else {
        panic!("changed resume identity must be rejected");
    };
    assert_eq!(error.code, "provider_session_mismatch");
    assert!(error.effect_uncertain);
    assert_eq!(request_methods(&transcript)[2], "thread/resume");
    adapter
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown resume-mismatch fixture: {error}"));
}

#[tokio::test]
async fn durable_resume_requires_an_observed_response_identity() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    assert_resume_error(
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}",
        "provider_session_unconfirmed",
    )
    .await;
}

#[tokio::test]
async fn durable_resume_rejects_conflicting_response_aliases() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    assert_resume_error(
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"thread\":{\"id\":\"thread-1\"},\"threadId\":\"thread-other\"}}",
        "provider_session_mismatch",
    )
    .await;
}

#[tokio::test]
async fn definitive_initialize_rejection_is_not_retransmitted() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    assert_initialization_error(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-1,\"message\":\"rejected\"}}",
        "provider_request_rejected",
    )
    .await;
}

#[tokio::test]
async fn definitive_malformed_initialize_response_is_not_retransmitted() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    assert_initialization_error(
        "{\"jsonrpc\":\"2.0\",\"id\":1}",
        "provider_protocol_invalid",
    )
    .await;
}

#[tokio::test]
async fn attachment_rejects_an_observed_model_mismatch_without_retransmission() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let response = "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"thread\":{\"id\":\"thread-1\",\"model\":\"other-model\"}}}";
    assert_new_attachment_error(response, "provider_model_mismatch").await;
}

#[tokio::test]
async fn attachment_accepts_original_params_identity_and_exact_model() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create params-identity fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let response = "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{},\"params\":{\"thread\":{\"id\":\"thread-1\",\"model\":\"gpt-5.6-terra\"}}}";
    let script = transcript_fixture_with_response(&transcript, "", response);
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("attach params identity: {error}"));
    assert_eq!(started.provider_session_id, "thread-1");
    stop_and_release(&adapter, &session, &started).await;
}

#[tokio::test]
async fn missing_thread_start_identity_is_poisoned_without_a_second_request() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create missing-identity fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let response = "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}";
    let script = transcript_fixture_with_response(&transcript, "", response);
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let Err(error) = adapter.start(&session).await else {
        panic!("missing provider identity must fail closed");
    };
    assert_eq!(error.code, "provider_session_unconfirmed");
    assert!(error.effect_uncertain);
    let Err(reuse_error) = adapter.start(&session).await else {
        panic!("poisoned provider runtime must not be reused");
    };
    assert_eq!(reuse_error.code, "provider_runtime_restart_required");
    assert!(reuse_error.effect_uncertain);
    assert_eq!(
        request_methods(&transcript),
        ["initialize", "initialized", "thread/start"]
    );
    adapter
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown missing-identity fixture: {error}"));
}

fn transcript_fixture(transcript: &Path, before_response: &str) -> String {
    transcript_fixture_with_response(
        transcript,
        before_response,
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"thread\":{\"id\":\"thread-1\"}}}",
    )
}

fn transcript_fixture_with_response(
    transcript: &Path,
    before_response: &str,
    response: &str,
) -> String {
    format!(
        "#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' \"$initialize\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nIFS= read -r initialized\nprintf '%s\\n' \"$initialized\" >> {log}\nIFS= read -r thread\nprintf '%s\\n' \"$thread\" >> {log}\n{before}printf '%s\\n' {response}\nIFS= read -r forever\n",
        log = shell_quote(transcript),
        before = before_response,
        response = shell_quote_text(response),
    )
}

async fn assert_resume_error(response: &str, expected_code: &str) {
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create rejected-resume fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = transcript_fixture_with_response(&transcript, "", response);
    let mut session = fixture_session(directory.path(), &script).await;
    "thread-1".clone_into(&mut session.provider_session_id);
    let adapter = ProviderAdapter::new();
    let Err(error) = adapter.start(&session).await else {
        panic!("unconfirmed resume identity must fail closed");
    };
    assert_eq!(error.code, expected_code);
    assert!(error.effect_uncertain);
    let Err(reuse_error) = adapter.start(&session).await else {
        panic!("poisoned resume runtime must not be reused");
    };
    assert_eq!(reuse_error.code, "provider_runtime_restart_required");
    assert!(reuse_error.effect_uncertain);
    assert_eq!(
        request_methods(&transcript),
        ["initialize", "initialized", "thread/resume"]
    );
    adapter
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown rejected-resume fixture: {error}"));
}

async fn assert_initialization_error(response: &str, expected_code: &str) {
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create rejected-initialize fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = format!(
        "#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' \"$initialize\" >> {log}\nprintf '%s\\n' {response}\nIFS= read -r forever\n",
        log = shell_quote(&transcript),
        response = shell_quote_text(response),
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let Err(error) = adapter.start(&session).await else {
        panic!("definitive initialize failure must fail closed");
    };
    assert_eq!(error.code, expected_code);
    assert!(error.effect_uncertain);
    let Err(reuse_error) = adapter.start(&session).await else {
        panic!("poisoned initialized runtime must not be reused");
    };
    assert_eq!(reuse_error.code, "provider_runtime_restart_required");
    assert!(reuse_error.effect_uncertain);
    assert_eq!(request_methods(&transcript), ["initialize"]);
    adapter
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown rejected-initialize fixture: {error}"));
}

async fn assert_new_attachment_error(response: &str, expected_code: &str) {
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create rejected-attachment fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = transcript_fixture_with_response(&transcript, "", response);
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    for _ in 0..2 {
        let Err(error) = adapter.start(&session).await else {
            panic!("invalid provider attachment must remain failed closed");
        };
        assert_eq!(error.code, expected_code);
        assert!(error.effect_uncertain);
    }
    assert_eq!(
        request_methods(&transcript),
        ["initialize", "initialized", "thread/start"]
    );
    adapter
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown rejected-attachment fixture: {error}"));
}

fn requests(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read provider transcript: {error}"))
        .lines()
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("decode provider transcript: {error}"))
        })
        .collect()
}

fn request_methods(path: &Path) -> Vec<&'static str> {
    requests(path)
        .into_iter()
        .map(|request| match request["method"].as_str() {
            Some("initialize") => "initialize",
            Some("initialized") => "initialized",
            Some("thread/start") => "thread/start",
            Some("thread/resume") => "thread/resume",
            other => panic!("unexpected provider method: {other:?}"),
        })
        .collect()
}

async fn stop_and_release(
    adapter: &ProviderAdapter,
    session: &agentsassemble_domain::DurableAgentSession,
    started: &super::ProviderRuntimeStarted,
) {
    adapter
        .stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
        )
        .await
        .unwrap_or_else(|error| panic!("stop provider-session fixture: {error}"));
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
    panic!("provider fixture did not publish its request marker");
}

fn shell_quote(path: &Path) -> String {
    shell_quote_text(&path.to_string_lossy())
}

fn shell_quote_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
