use std::{
    collections::BTreeSet,
    sync::{Arc, Barrier},
    time::Duration,
};

use futures_util::future::AbortHandle;
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, json};

use super::{
    ConnectionAuthentication, ConnectionRegistry, MAX_MCP_REQUEST_BYTES, MAX_PORTAL_CONNECTIONS,
};
use crate::room_portal::{ProviderTurnOutcome, RoomObservationStart, RoomPortal};

#[test]
fn bearer_authentication_is_atomic_with_unauthenticated_eviction() {
    for _ in 0..128 {
        let registry = Arc::new(ConnectionRegistry::default());
        let (abort, _registration) = AbortHandle::new_pair();
        let connection_id = registry.register(abort);
        let barrier = Arc::new(Barrier::new(2));
        let (authentication, evicted) = std::thread::scope(|scope| {
            let authentication_registry = registry.clone();
            let authentication_barrier = barrier.clone();
            let authentication = scope.spawn(move || {
                authentication_barrier.wait();
                authentication_registry.authenticate(
                    connection_id,
                    Some(b"Bearer portal-token"),
                    b"Bearer portal-token",
                )
            });
            let eviction_registry = registry.clone();
            let eviction = scope.spawn(move || {
                barrier.wait();
                eviction_registry.evict_oldest_unauthenticated()
            });
            (
                authentication
                    .join()
                    .unwrap_or_else(|_| panic!("join authentication race")),
                eviction
                    .join()
                    .unwrap_or_else(|_| panic!("join eviction race")),
            )
        });
        match authentication {
            ConnectionAuthentication::Authenticated => assert!(evicted.is_none()),
            ConnectionAuthentication::Gone => assert!(evicted.is_some()),
            ConnectionAuthentication::Unauthorized => {
                panic!("the exact bearer was rejected")
            }
        }
    }
}

#[tokio::test]
async fn loopback_mcp_requires_a_same_turn_read_before_commit() {
    let portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("create room portal fixture: {error}"));
    portal
        .begin_observation(RoomObservationStart {
            session_id: "agent-1",
            turn_id: "turn-1",
            input_up_to_seq: 7,
            durable_turn_generation: 1,
            execution_id: "00000000-0000-4000-8000-000000000001",
            room_view: "Room: General\n#7 Human: hello",
            attachment_ids: &[],
            attachment_ingress: None,
            allowed_agent_ids: &["agent-2".to_owned()],
            tabletop_tools: false,
            tool_ingress: None,
        })
        .unwrap_or_else(|error| panic!("begin room observation: {error}"));
    let client = ()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(portal.endpoint())
                .auth_header(portal.bearer_token()),
        ))
        .await
        .unwrap_or_else(|error| panic!("connect room portal MCP: {error}"));
    let names = client
        .list_all_tools()
        .await
        .unwrap_or_else(|error| panic!("list room portal tools: {error}"))
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "choose_random".to_owned(),
            "decline_to_speak".to_owned(),
            "publish_message".to_owned(),
            "read_attachment".to_owned(),
            "read_discussion".to_owned(),
            "read_message_context".to_owned(),
            "roll_dice".to_owned(),
            "search_messages".to_owned(),
        ])
    );
    let early = call_tool(
        &client,
        "publish_message",
        json!({"content": "  canonical reply  ", "next_agent_id": "unknown"}),
    )
    .await;
    assert_eq!(early.is_error, Some(true));
    let early_decline = call_tool(
        &client,
        "decline_to_speak",
        json!({"reason_code": "duplicate"}),
    )
    .await;
    assert_eq!(early_decline.is_error, Some(true));
    assert!(portal.finish_observation("turn-1", 7).is_err());
    let read = call_tool(&client, "read_discussion", json!({})).await;
    let read_text = read
        .content
        .first()
        .and_then(|content| content.as_text())
        .map(|content| content.text.as_str())
        .unwrap_or_default();
    assert!(read_text.contains("Human: hello"));
    assert!(portal.finish_observation("turn-1", 7).is_err());
    let published = call_tool(
        &client,
        "publish_message",
        json!({"content": "  canonical reply  ", "next_agent_id": "unknown"}),
    )
    .await;
    assert_ne!(published.is_error, Some(true));
    let duplicate = call_tool(
        &client,
        "decline_to_speak",
        json!({"reason_code": "duplicate"}),
    )
    .await;
    assert_eq!(duplicate.is_error, Some(true));
    assert_eq!(
        portal
            .finish_observation("turn-1", 7)
            .unwrap_or_else(|error| panic!("finish room observation: {error}")),
        ProviderTurnOutcome::Message {
            content: "canonical reply".to_owned(),
            target_agent_id: String::new(),
        }
    );
    let _ = client.cancel().await;
}

#[tokio::test]
async fn loopback_mcp_hides_capability_path_and_bounds_request_bodies() {
    let portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("create bounded portal fixture: {error}"));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_else(|error| panic!("build portal HTTP client: {error}"));
    let endpoint = reqwest::Url::parse(portal.endpoint())
        .unwrap_or_else(|error| panic!("parse portal endpoint: {error}"));
    let root = format!(
        "{}://{}:{}/",
        endpoint.scheme(),
        endpoint.host_str().unwrap_or("127.0.0.1"),
        endpoint.port().unwrap_or_default()
    );
    let address = format!(
        "{}:{}",
        endpoint.host_str().unwrap_or("127.0.0.1"),
        endpoint.port().unwrap_or_default()
    );
    let mut idle_connections = Vec::new();
    for _ in 0..16 {
        idle_connections.push(
            tokio::net::TcpStream::connect(&address)
                .await
                .unwrap_or_else(|error| panic!("open idle loopback connection: {error}")),
        );
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(portal.active_connection_count() <= MAX_PORTAL_CONNECTIONS);
    let admitted = client
        .post(portal.endpoint())
        .bearer_auth(portal.bearer_token())
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap_or_else(|error| panic!("probe authenticated admission: {error}"));
    assert_ne!(admitted.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_ne!(admitted.status(), reqwest::StatusCode::UNAUTHORIZED);
    let hidden = client
        .post(root)
        .body("{}")
        .send()
        .await
        .unwrap_or_else(|error| panic!("probe hidden portal route: {error}"));
    assert_eq!(hidden.status(), reqwest::StatusCode::NOT_FOUND);
    let unauthorized = client
        .post(portal.endpoint())
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap_or_else(|error| panic!("probe portal authorization: {error}"));
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
    let oversized = client
        .post(portal.endpoint())
        .bearer_auth(portal.bearer_token())
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .body("x".repeat(MAX_MCP_REQUEST_BYTES + 1))
        .send()
        .await
        .unwrap_or_else(|error| panic!("send oversized MCP body: {error}"));
    assert_eq!(oversized.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    drop(idle_connections);
}

async fn call_tool(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &'static str,
    arguments: serde_json::Value,
) -> rmcp::model::CallToolResult {
    let arguments = serde_json::from_value::<Map<String, serde_json::Value>>(arguments)
        .unwrap_or_else(|error| panic!("decode tool arguments: {error}"));
    client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments))
        .await
        .unwrap_or_else(|error| panic!("call {name}: {error}"))
}
