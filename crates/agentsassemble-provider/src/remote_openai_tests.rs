use std::{convert::Infallible, time::Duration};

use bytes::Bytes;
use http_body_util::{BodyExt as _, Full};
use hyper::{Request, Response, body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::mpsc};

use super::{RemoteOpenAiApi, RemoteOpenAiDriver};
use crate::{
    credentials::ProviderCredentialStore,
    custom_api::CUSTOM_API_SPEC,
    deepseek::DEEPSEEK_SPEC,
    driver::{DriverError, ProviderDriver, ProviderRoomObservation, ProviderTurnRequest},
    remote_openai_spec::{RemoteOpenAiEndpoint, RemoteOpenAiSpec},
    room_portal::ProviderTurnOutcome,
    test_support::durable_session,
};

fn tool_stream(model: &str, tool: &str, finish_reason: &str) -> String {
    let arguments = if tool == "publish_message" {
        json!({"content": "Bounded room reply", "next_agent_id": ""})
    } else {
        json!({})
    };
    let chunk = json!({
        "id": "completion-fixture",
        "model": model,
        "choices": [{"index": 0, "delta": {
            "role": "assistant",
            "tool_calls": [{"index": 0, "id": "call-fixture", "type": "function",
                "function": {"name": tool, "arguments": arguments.to_string()}}]
        }, "finish_reason": finish_reason}]
    });
    format!("data: {chunk}\n\ndata: [DONE]\n\n")
}

async fn api_fixture(
    responses: Vec<String>,
) -> (
    reqwest::Url,
    mpsc::Receiver<Value>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind fixture: {error}"));
    let endpoint = format!(
        "http://{}/chat/completions",
        listener
            .local_addr()
            .unwrap_or_else(|error| panic!("address: {error}"))
    );
    let (requests, captured) = mpsc::channel(4);
    let server = tokio::spawn(async move {
        for response in responses {
            let (stream, _) = listener
                .accept()
                .await
                .unwrap_or_else(|error| panic!("accept fixture request: {error}"));
            let requests = requests.clone();
            let service = service_fn(move |request: Request<Incoming>| {
                let requests = requests.clone();
                let response = response.clone();
                async move {
                    assert_eq!(request.method(), hyper::Method::POST);
                    assert_eq!(request.uri().path(), "/chat/completions");
                    assert_eq!(
                        request.headers()["authorization"],
                        "Bearer isolated-test-secret"
                    );
                    let body = request
                        .into_body()
                        .collect()
                        .await
                        .unwrap_or_else(|error| panic!("collect request: {error}"))
                        .to_bytes();
                    let payload = serde_json::from_slice::<Value>(&body)
                        .unwrap_or_else(|error| panic!("request JSON: {error}"));
                    requests
                        .send(payload)
                        .await
                        .unwrap_or_else(|error| panic!("capture request: {error}"));
                    Ok::<_, Infallible>(
                        Response::builder()
                            .header("content-type", "text/event-stream")
                            .body(Full::new(Bytes::from(response)))
                            .unwrap_or_else(|error| panic!("stream response: {error}")),
                    )
                }
            });
            http1::Builder::new()
                .keep_alive(false)
                .serve_connection(TokioIo::new(stream), service)
                .await
                .unwrap_or_else(|error| panic!("serve fixture response: {error}"));
        }
    });
    (
        endpoint
            .parse()
            .unwrap_or_else(|error| panic!("fixture URL: {error}")),
        captured,
        server,
    )
}

fn room_request(session_id: &str) -> ProviderTurnRequest {
    ProviderTurnRequest {
        request_ingress: None,
        turn_id: "turn".to_owned(),
        turn_generation: 1,
        execution_id: "00000000-0000-4000-8000-000000000099".to_owned(),
        input: "Reply to the bounded room view".to_owned(),
        room_observation: Some(ProviderRoomObservation {
            session_id: session_id.to_owned(),
            input_up_to_seq: 1,
            view: "#1 Human: reply once".to_owned(),
            attachment_ids: vec![],
            attachment_ingress: None,
            allowed_agent_ids: vec![],
            tabletop_tools: false,
            room_tool_ingress: None,
        }),
    }
}

#[tokio::test]
async fn portal_completion_preserves_read_and_publication_failure_codes() {
    let mut driver = RemoteOpenAiDriver::launch(
        &DEEPSEEK_SPEC,
        ProviderCredentialStore::isolated_test_store(),
    )
    .await
    .unwrap_or_else(|error| panic!("launch in-process room portal: {}", error.error));
    let request = room_request("session");
    driver
        .begin_room_observation(&request)
        .await
        .unwrap_or_else(|error| panic!("begin room observation: {error}"));
    let Err(error) = driver.finish_room_observation(&request).await else {
        panic!("accepted incomplete room observation");
    };
    assert_eq!(error.code, "room_observation_unconfirmed");

    driver
        .execute_tool(
            crate::openai_stream::ToolCall {
                id: "read".to_owned(),
                kind: "function",
                function: crate::openai_stream::ToolFunction {
                    name: "read_discussion".to_owned(),
                    arguments: "{}".to_owned(),
                },
            },
            false,
        )
        .await
        .unwrap_or_else(|error| panic!("read through authenticated MCP: {error}"));
    let Err(error) = driver.finish_room_observation(&request).await else {
        panic!("accepted incomplete room observation");
    };
    assert_eq!(error.code, "room_portal_publication_missing");
    driver
        .abort_room_observation()
        .await
        .unwrap_or_else(|error| panic!("release failed observation: {error}"));
    driver
        .stop()
        .await
        .unwrap_or_else(|error| panic!("stop room portal: {error}"));
}

// Only the external HTTP peer and credential store are isolated fixtures. The actual
// request builder, SSE decoder, driver, authenticated MCP portal, and read/publication
// receipt owners run unchanged. This does not exercise public HTTPS/DNS discovery.
async fn room_turn(
    spec: &'static RemoteOpenAiSpec,
    requested_model: &str,
    responses: Vec<String>,
) -> (Result<ProviderTurnOutcome, DriverError>, Vec<Value>) {
    let (endpoint, mut captured, server) = api_fixture(responses).await;
    let credentials = ProviderCredentialStore::isolated_test_store();
    credentials
        .set(
            spec.credential_id()
                .unwrap_or_else(|| panic!("bearer provider")),
            "isolated-test-secret",
        )
        .await
        .unwrap_or_else(|error| panic!("set isolated credential: {error}"));
    let mut session = durable_session(
        "room",
        "session",
        "API",
        spec.provider_kind,
        requested_model,
        "https",
    );
    session.public.runtime_kind = "api".to_owned();
    session.public.max_output_tokens = 4096;
    let authority = if spec.endpoint == RemoteOpenAiEndpoint::AgentSession {
        session.provider_endpoint = "https://openrouter.ai/api/v1".to_owned();
        Some(session.provider_endpoint.clone())
    } else {
        None
    };
    let mut driver = RemoteOpenAiDriver::launch_with_api(
        spec,
        credentials,
        RemoteOpenAiApi {
            spec,
            client: reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap_or_else(|error| panic!("fixture client: {error}")),
            endpoint,
            session_endpoint_authority: authority,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("launch real portal: {}", error.error));
    let attachment = driver
        .attach_session(&session)
        .await
        .unwrap_or_else(|error| panic!("attach session: {error}"));
    let request = room_request(&session.public.session_id);
    driver
        .begin_room_observation(&request)
        .await
        .unwrap_or_else(|error| panic!("begin observation: {error}"));
    let sent =
        tokio::time::timeout(Duration::from_secs(5), driver.send_turn(&session, &request)).await;
    let outcome = match &sent {
        Ok(Ok(_)) => driver.finish_room_observation(&request).await,
        Ok(Err(error)) => Err(error.clone()),
        Err(_) => Err(DriverError::new(
            "test_timeout",
            "The fixture turn timed out.",
        )),
    };
    if outcome.is_err() {
        driver
            .abort_room_observation()
            .await
            .unwrap_or_else(|error| panic!("abort observation: {error}"));
    }
    let after = driver
        .attach_session(&session)
        .await
        .unwrap_or_else(|error| panic!("retain attachment: {error}"));
    driver
        .stop()
        .await
        .unwrap_or_else(|error| panic!("stop real portal: {error}"));
    server.abort();
    if let Err(error) = server.await {
        assert!(error.is_cancelled(), "fixture server failed: {error}");
    }
    if let Ok(Ok(completed)) = sent {
        assert_eq!(completed.turn_id, request.turn_id);
    }
    assert_eq!(attachment.provider_session_id, after.provider_session_id);
    assert_eq!(attachment.observed_model_id, None);
    assert!(
        !driver
            .is_alive()
            .await
            .unwrap_or_else(|error| panic!("stopped portal: {error}"))
    );
    let mut bodies = Vec::new();
    while let Ok(body) = captured.try_recv() {
        assert_eq!(body["model"], requested_model);
        assert_eq!(body["max_tokens"], 4096);
        bodies.push(body);
    }
    (outcome, bodies)
}

#[tokio::test]
async fn routed_custom_response_preserves_selection_and_reaches_room_publication() {
    for (spec, requested, resolved) in [
        (
            &CUSTOM_API_SPEC,
            "openrouter/auto",
            ["vendor/model-a", "vendor/model-b"],
        ),
        (
            &CUSTOM_API_SPEC,
            "fixed-model",
            ["fixed-model", "fixed-model"],
        ),
        (
            &DEEPSEEK_SPEC,
            "deepseek-v4-flash",
            ["deepseek-v4-flash", "deepseek-v4-flash"],
        ),
    ] {
        let (outcome, requests) = room_turn(
            spec,
            requested,
            vec![
                tool_stream(resolved[0], "read_discussion", "tool_calls"),
                tool_stream(resolved[1], "publish_message", "tool_calls"),
            ],
        )
        .await;
        assert_eq!(
            outcome,
            Ok(ProviderTurnOutcome::Message {
                content: "Bounded room reply".to_owned(),
                target_agent_id: String::new(),
            })
        );
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["messages"][2]["name"], "read_discussion");
        assert!(
            requests[1]["messages"][2]["content"]
                .as_str()
                .unwrap_or_else(|| panic!("read result"))
                .contains("#1 Human: reply once")
        );
    }
}

#[tokio::test]
async fn invalid_completion_and_unpermitted_tools_never_publish() {
    for (spec, model, responses, code) in [
        (
            &DEEPSEEK_SPEC,
            "deepseek-v4-flash",
            vec![tool_stream(
                "substituted-model",
                "read_discussion",
                "tool_calls",
            )],
            "provider_protocol_invalid",
        ),
        (
            &CUSTOM_API_SPEC,
            "openrouter/auto",
            vec![tool_stream("", "read_discussion", "tool_calls")],
            "provider_protocol_invalid",
        ),
        (
            &CUSTOM_API_SPEC,
            "openrouter/auto",
            vec![tool_stream("vendor/model", "read_discussion", "stop")],
            "provider_protocol_invalid",
        ),
        (
            &CUSTOM_API_SPEC,
            "openrouter/auto",
            vec![tool_stream("vendor/model", "publish_message", "tool_calls")],
            "provider_room_read_missing",
        ),
        (
            &CUSTOM_API_SPEC,
            "openrouter/auto",
            vec![
                tool_stream("vendor/model", "read_discussion", "tool_calls"),
                tool_stream("vendor/model", "roll_dice", "tool_calls"),
            ],
            "provider_tool_call_invalid",
        ),
    ] {
        let (outcome, _) = room_turn(spec, model, responses).await;
        let Err(error) = outcome else {
            panic!("accepted rejected turn");
        };
        assert_eq!(error.code, code);
    }
}
