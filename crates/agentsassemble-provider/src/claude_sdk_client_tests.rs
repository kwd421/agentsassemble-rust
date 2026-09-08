use futures_util::{SinkExt, StreamExt};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

#[test]
fn requires_canonical_v4_session_identity() {
    const SESSION_ID: &str = "6a1843af-3a9d-44d3-8b0c-41672c83e0dd";
    assert!(super::valid_session_id(SESSION_ID, ""));
    assert!(super::valid_session_id(SESSION_ID, SESSION_ID));
    assert!(!super::valid_session_id("legacy-session-alias", ""));
    assert!(!super::valid_session_id(
        "00000000-0000-0000-0000-000000000000",
        ""
    ));
}

#[tokio::test]
async fn correlates_session_and_turn_receipts() {
    let (client_io, host_io) = tokio::io::duplex(16 * 1024);
    let (client_output, client_input) = tokio::io::split(client_io);
    let (host_input, host_output) = tokio::io::split(host_io);
    let host = tokio::spawn(async move {
        let mut input = FramedRead::new(host_input, LinesCodec::new());
        let mut output = FramedWrite::new(host_output, LinesCodec::new());
        let initialize = input
            .next()
            .await
            .transpose()
            .unwrap_or_default()
            .unwrap_or_default();
        let command: serde_json::Value = serde_json::from_str(&initialize).unwrap_or_default();
        assert_eq!(command["room_portal"]["bearer_token"], "private-token");
        output.send(serde_json::json!({"type":"ready","session_id":"6a1843af-3a9d-44d3-8b0c-41672c83e0dd","reused":false,"model":"claude-opus-5"}).to_string()).await.unwrap_or_else(|error| panic!("send ready: {error}"));
        let turn = input
            .next()
            .await
            .transpose()
            .unwrap_or_default()
            .unwrap_or_default();
        let turn: serde_json::Value = serde_json::from_str(&turn).unwrap_or_default();
        output.send(serde_json::json!({"type":"turn_result","turn_id":turn["turn_id"],"provider_turn_id":"provider-turn-1","session_id":"6a1843af-3a9d-44d3-8b0c-41672c83e0dd","content":"answer"}).to_string()).await.unwrap_or_else(|error| panic!("send turn: {error}"));
    });
    let (mut client, attachment) = super::ClaudeSdkClient::connect(
        client_input,
        client_output,
        "/workspace",
        "claude-opus-5",
        "high",
        "default",
        "meeting_read_only",
        "",
        "http://127.0.0.1:1/mcp".to_owned(),
        "private-token",
    )
    .await
    .unwrap_or_else(|error| panic!("connect client: {error:?}"));
    assert!(!attachment.reused);
    let turn = client
        .turn(
            "room-session",
            &crate::driver::ProviderTurnRequest {
                request_ingress: None,
                turn_id: "turn-1".to_owned(),
                turn_generation: 7,
                execution_id: "fixture-execution".to_owned(),
                input: "hello".to_owned(),
                room_observation: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("complete turn: {error}"));
    assert_eq!(
        (turn.provider_turn_id.as_str(), turn.content.as_str()),
        ("provider-turn-1", "answer")
    );
    host.await
        .unwrap_or_else(|error| panic!("join host: {error}"));
}

#[tokio::test]
async fn owner_answers_wait_for_native_delivery_and_durable_receipts() {
    use crate::{ProviderRequestExchange, ProviderRequestIngress};
    use agentsassemble_domain::ProviderRequestResolution;
    for cancel in [false, true] {
        let (client_io, host_io) = tokio::io::duplex(16 * 1024);
        let (output, input) = tokio::io::split(client_io);
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        let (observed_tx, observed_rx) = tokio::sync::oneshot::channel();
        let host = tokio::spawn(request_host(host_io, cancel, cancel_rx, observed_rx));
        let (mut client, _) = super::ClaudeSdkClient::connect(
            input,
            output,
            "/workspace",
            "claude-opus-5",
            "high",
            "default",
            "meeting_read_only",
            "",
            "http://127.0.0.1:1/mcp".to_owned(),
            "private-token",
        )
        .await
        .unwrap_or_else(|error| panic!("connect request host: {error:?}"));
        let (ingress, mut commands) = ProviderRequestIngress::channel(4);
        let request = crate::driver::ProviderTurnRequest {
            request_ingress: Some(ingress),
            turn_id: "turn-1".to_owned(),
            turn_generation: 7,
            execution_id: "exact-execution".to_owned(),
            input: "hello".to_owned(),
            room_observation: None,
        };
        let turn = client.turn("room-session", &request);
        tokio::pin!(turn);
        let mut brokers = Vec::new();
        for _ in 0..if cancel { 1 } else { 2 } {
            let command = tokio::select! {
                result = &mut turn => panic!("turn ended before request: {}", result.is_ok()),
                command = commands.recv() => command.unwrap_or_else(|| panic!("request missing")),
            };
            assert_eq!(command.session_id, "room-session");
            assert_eq!(command.turn_generation, 7);
            assert_eq!(command.execution_id, "exact-execution");
            let (exchange, responder, completion) = ProviderRequestExchange::channel();
            command.complete(Ok(exchange));
            brokers.push((responder, completion));
        }
        if cancel {
            cancel_tx
                .send(())
                .unwrap_or_else(|()| panic!("cancel signal"));
        }
        for (mut responder, mut completion) in brokers {
            if !cancel {
                responder
                    .respond(ProviderRequestResolution::Option {
                        option_id: "allow-once".to_owned(),
                    })
                    .unwrap_or_else(|error| panic!("answer: {error}"));
            }
            let delivered = tokio::select! {
                result = &mut turn => panic!("turn ended before delivery: {}", result.is_ok()),
                delivered = completion.completion() => delivered,
            };
            assert_eq!(delivered, !cancel);
            if !cancel {
                tokio::select! {
                    biased;
                    result = &mut turn => panic!("turn bypassed durable receipt: {}", result.is_ok()),
                    () = std::future::ready(()) => {},
                }
            }
            completion.finish(Ok(()));
        }
        if cancel {
            observed_tx
                .send(())
                .unwrap_or_else(|()| panic!("cancellation observation"));
        }
        assert_eq!(
            turn.await
                .unwrap_or_else(|error| panic!("request turn: {error}"))
                .content,
            "answer"
        );
        host.await
            .unwrap_or_else(|error| panic!("request host: {error}"));
    }
}

async fn request_host(
    io: tokio::io::DuplexStream,
    cancel: bool,
    cancel_rx: tokio::sync::oneshot::Receiver<()>,
    observed_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let (input, output) = tokio::io::split(io);
    let mut input = FramedRead::new(input, LinesCodec::new());
    let mut output = FramedWrite::new(output, LinesCodec::new());
    let _ = read_command(&mut input).await;
    send_event(&mut output, serde_json::json!({"type":"ready", "session_id":"6a1843af-3a9d-44d3-8b0c-41672c83e0dd", "reused":false, "model":"claude-opus-5"})).await;
    let _ = read_command(&mut input).await;
    let mut ids = std::collections::BTreeSet::new();
    for _ in 0..if cancel { 1 } else { 2 } {
        let id = uuid::Uuid::new_v4().to_string();
        ids.insert(id.clone());
        send_event(&mut output, serde_json::json!({
            "type":"provider_request", "turn_id":"turn-1", "request":{
                "provider_request_id":id, "request_kind":"permission", "title":"Approve tool",
                "description":"", "timeout_seconds":600, "prompt":{"response_kind":"option", "options":[
                    {"id":"allow-once", "label":"Allow once", "kind":"allow_once", "description":""}
                ]}
            }
        })).await;
    }
    if cancel {
        cancel_rx
            .await
            .unwrap_or_else(|error| panic!("cancel channel: {error}"));
        send_event(
            &mut output,
            serde_json::json!({"type":"request_cancelled", "request_id":ids.first()}),
        )
        .await;
    }
    while !ids.is_empty() {
        let answer = read_command(&mut input).await;
        if cancel {
            assert_eq!(answer["type"], "request_complete");
        } else {
            assert_eq!(answer["type"], "request_answer");
            assert_eq!(answer["resolution"]["option_id"], "allow-once");
            send_event(&mut output, serde_json::json!({"type":"request_delivered", "request_id":answer["request_id"], "delivered":true})).await;
            let receipt = read_command(&mut input).await;
            assert_eq!(receipt["type"], "request_complete");
            assert_eq!(receipt["request_id"], answer["request_id"]);
        }
        assert!(ids.remove(answer["request_id"].as_str().unwrap_or_default()));
    }
    if cancel {
        observed_rx
            .await
            .unwrap_or_else(|error| panic!("cancellation observation: {error}"));
    }
    send_event(&mut output, serde_json::json!({"type":"turn_result", "turn_id":"turn-1", "provider_turn_id":"provider-turn-1", "session_id":"6a1843af-3a9d-44d3-8b0c-41672c83e0dd", "content":"answer"})).await;
}

async fn read_command<I: tokio::io::AsyncRead + Unpin>(
    input: &mut FramedRead<I, LinesCodec>,
) -> serde_json::Value {
    let line = input
        .next()
        .await
        .unwrap_or_else(|| panic!("command absent"))
        .unwrap_or_else(|error| panic!("command read: {error}"));
    serde_json::from_str(&line).unwrap_or_else(|error| panic!("command JSON: {error}"))
}
async fn send_event<O: tokio::io::AsyncWrite + Unpin>(
    output: &mut FramedWrite<O, LinesCodec>,
    value: serde_json::Value,
) {
    output
        .send(value.to_string())
        .await
        .unwrap_or_else(|error| panic!("send event: {error}"));
}
