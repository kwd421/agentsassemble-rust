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
        .turn("turn-1", "hello")
        .await
        .unwrap_or_else(|error| panic!("complete turn: {error}"));
    assert_eq!(
        (turn.provider_turn_id.as_str(), turn.content.as_str()),
        ("provider-turn-1", "answer")
    );
    host.await
        .unwrap_or_else(|error| panic!("join host: {error}"));
}
