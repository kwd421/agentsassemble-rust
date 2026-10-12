use super::*;
use agentsassemble_domain::{
    InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_persistence::{RoomManagerAuthority, SqliteStore};
use agentsassemble_provider::ProviderCatalogService;
use serde_json::{Value, json};
use std::{convert::Infallible, time::Duration};
use tokio::sync::oneshot;
use uuid::Uuid;

fn config() -> StreamableHttpServerConfig {
    StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_allowed_hosts(["localhost"])
}

fn request(body: Body) -> Request {
    let mut request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-03-26")
        .body(body)
        .expect("request");
    request.extensions_mut().insert(
        crate::http_admission::HttpAdmission::default()
            .admit()
            .expect("connection"),
    );
    request
}

fn tool_body(name: &str, arguments: Value) -> String {
    json!({"jsonrpc":"2.0", "id":1, "method":"tools/call", "params":{"name":name, "arguments":arguments}}).to_string()
}

async fn call(connector: &ConnectorMcp, name: &str, arguments: Value) -> Value {
    let response = serve_request(
        connector.clone(),
        request(Body::from(tool_body(name, arguments))),
        config(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response");
    serde_json::from_slice(&bytes).expect("JSON")
}

fn result(reply: &Value) -> Value {
    assert_ne!(reply["result"]["isError"], true, "tool rejected: {reply}");
    serde_json::from_str(
        reply["result"]["content"][0]["text"]
            .as_str()
            .expect("text"),
    )
    .expect("result")
}

struct Fixture {
    state: AppState,
    connector: ConnectorMcp,
    base: String,
    server: tokio::task::JoinHandle<()>,
}
impl Fixture {
    async fn new() -> Self {
        let store = SqliteStore::open("sqlite::memory:").await.expect("store");
        store
            .bootstrap_local_authority(&Uuid::new_v4().to_string(), "Owner")
            .await
            .expect("authority");
        store
            .create_room_for_local_operator(&Uuid::new_v4().to_string(), "room", "Room")
            .await
            .expect("room");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        let state = AppState::local(
            store,
            crate::TicketStore::new(Duration::from_secs(30), 128),
            ProviderCatalogService::fixed(ProviderCatalog::default()),
        )
        .await
        .expect("state")
        .with_manual_public_ingress(
            address,
            "https://mcp-budget.example.test",
            "mcp-budget-proxy-fixture-00000001",
        )
        .expect("ingress");
        let connector = ConnectorMcp::hosted(state.clone());
        let server_state = state.clone();
        let server = tokio::spawn(async move {
            crate::serve(
                listener,
                server_state.clone(),
                server_state.shutdown.clone(),
                async { Ok(()) },
            )
            .await
            .expect("server");
        });
        Self {
            state,
            connector,
            base: format!("http://{address}"),
            server,
        }
    }
    async fn join(&self) -> Value {
        let manager = self
            .state
            .store
            .authorize_local_room_manager(
                "room",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await
            .expect("manager");
        let invite = self
            .state
            .store
            .create_connector_invite(
                &RoomManagerAuthority::Local(manager),
                Uuid::new_v4(),
                InviteScope::ReadWrite,
                chrono::Utc::now(),
            )
            .await
            .expect("invite");
        let mut args = json!({"invite_url":format!("{}/join?token={}", self.base, invite.invite_bearer), "display_name":"Budget test"});
        let prepared = result(&call(&self.connector, "room_join", args.clone()).await);
        args["connection_id"] = prepared["connection_id"].clone();
        result(&call(&self.connector, "room_join", args).await)
    }
    async fn stop(self) {
        self.connector.close();
        self.state.shutdown.cancel();
        self.server.await.expect("stopped");
    }
}

#[tokio::test]
async fn chunked_waits_release_envelopes_and_leave_all_upload_capacity_available() {
    let fixture = Fixture::new().await;
    let joined = fixture.join().await;
    let mut waits = Vec::new();
    let mut connections = Vec::new();
    for _ in 0..16 {
        let (sent, received) = oneshot::channel();
        let bytes = axum::body::Bytes::from(tool_body(
            "room_wait_next",
            json!({"connection_id":joined["connection_id"]}),
        ));
        let stream = futures_util::stream::once(async move { Ok::<_, Infallible>(bytes) });
        let eof = futures_util::stream::once(async move {
            let _ = sent.send(());
            Ok::<_, Infallible>(axum::body::Bytes::new())
        });
        use futures_util::StreamExt;
        let request = request(Body::from_stream(stream.chain(eof)));
        connections.push(
            request
                .extensions()
                .get::<crate::http_admission::HttpConnectionAdmission>()
                .expect("connection")
                .clone(),
        );
        waits.push(tokio::spawn(serve_request(
            fixture.connector.clone(),
            request,
            config(),
        )));
        received.await.expect("body parsed next");
        // Wait for dispatch to release this envelope before starting the next
        // request; all earlier tool calls remain outstanding on the same cursor.
        let available = fixture
            .connector
            .envelopes
            .small
            .clone()
            .acquire_many_owned(SMALL_ENVELOPE_BYTES as u32)
            .await
            .expect("dispatch released");
        drop(available);
    }
    assert!(waits.iter().all(|wait| !wait.is_finished()));
    // Exactly the whole logical pool is still available, despite 16 chunked waits.
    let (_, all) = fixture
        .state
        .upload_bodies
        .reserve_length(Some("unrelated"), 128 * 1024 * 1024, 128 * 1024 * 1024)
        .expect("whole upload pool available");
    drop(all);
    let other = fixture.join().await;
    let uploaded = result(&call(&fixture.connector, "room_upload_attachment", json!({"connection_id":other["connection_id"], "filename":"proof.txt", "content_type":"text/plain", "data_base64":"cHJvb2Y="})).await);
    assert!(uploaded["attachment"]["id"].is_string());
    assert!(connections[0].has_authenticated_wait());
    for wait in waits {
        wait.abort();
        let _ = wait.await;
    }
    // The HTTP future is gone, but RMCP owns a detached service task. Observe
    // the actual authenticated wait lifetime end before closing the connector.
    tokio::time::timeout(
        Duration::from_secs(5),
        connections[0].authenticated_wait_finished(),
    )
    .await
    .expect("detached wait cancelled");
    assert!(
        fixture
            .connector
            .envelopes
            .small
            .clone()
            .try_acquire_many_owned(SMALL_ENVELOPE_BYTES as u32)
            .is_ok()
    );
    fixture.stop().await;
}

#[tokio::test]
async fn hosted_upload_uses_resolved_principal_limit_and_releases_after_storage() {
    let fixture = Fixture::new().await;
    let joined = fixture.join().await;
    // Connector authority owns principal_id = participant_id; the private MCP
    // connection handle must never be used as the accounting identity.
    let principal = joined["participant_id"].as_str().expect("principal");
    let (_, first) = fixture
        .state
        .upload_bodies
        .reserve_length(Some(principal), 1, 1)
        .expect("first");
    let (_, second) = fixture
        .state
        .upload_bodies
        .reserve_length(Some(principal), 1, 1)
        .expect("second");
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let content = vec![42; 10 * 1024 * 1024];
    let args = json!({"connection_id":joined["connection_id"], "filename":"proof.bin", "content_type":"application/octet-stream", "data_base64":STANDARD.encode(&content)});
    let refused = call(&fixture.connector, "room_upload_attachment", args.clone()).await;
    assert_eq!(refused["result"]["isError"], true);
    assert!(
        refused["result"]["content"][0]["text"]
            .as_str()
            .expect("error")
            .contains(crate::upload_body_budget::ERROR_CODE)
    );
    drop(second);
    let uploaded = result(&call(&fixture.connector, "room_upload_attachment", args).await);
    assert_eq!(uploaded["attachment"]["size"], content.len());
    let (_, returned) = fixture
        .state
        .upload_bodies
        .reserve_length(Some(principal), 1, 1)
        .expect("storage released principal slot");
    drop((first, returned));
    assert!(
        fixture
            .state
            .upload_bodies
            .reserve_length(Some("other"), 128 * 1024 * 1024, 128 * 1024 * 1024)
            .is_ok()
    );
    fixture.stop().await;
}

#[tokio::test(start_paused = true)]
async fn stalled_envelopes_leave_other_clients_capacity_and_expire_without_progress() {
    let connector = ConnectorMcp::new(Some(vec!["http://127.0.0.1:1".into()])).expect("connector");
    // Even completely occupied large-envelope capacity cannot steal the control reserve.
    let large = connector
        .envelopes
        .large
        .clone()
        .try_acquire_many_owned(LARGE_ENVELOPE_BYTES as u32)
        .expect("large pool");
    let mut requests = Vec::new();
    for _ in 0..126 {
        let (sent, received) = oneshot::channel();
        let stream = futures_util::stream::once(async move {
            let _ = sent.send(());
            std::future::pending::<Result<axum::body::Bytes, Infallible>>().await
        });
        requests.push(tokio::spawn(serve_request(
            connector.clone(),
            request(Body::from_stream(stream)),
            config(),
        )));
        received.await.expect("body poll confirms admission");
    }
    let response = serve_request(
        connector.clone(),
        request(Body::from(
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}).to_string(),
        )),
        config(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(requests.iter().all(|request| !request.is_finished()));
    tokio::time::advance(PREPARSE_IDLE).await;
    for request in requests {
        assert_eq!(
            request.await.expect("expired").status(),
            StatusCode::REQUEST_TIMEOUT
        );
    }
    let all = connector
        .envelopes
        .small
        .clone()
        .try_acquire_many_owned(SMALL_ENVELOPE_BYTES as u32)
        .expect("all bytes returned");
    // A refused envelope never polls its body; no request waits for capacity.
    let body = futures_util::stream::poll_fn(
        |_| -> std::task::Poll<Option<Result<axum::body::Bytes, Infallible>>> {
            panic!("exhausted budget must not poll");
        },
    );
    assert_eq!(
        serve_request(
            connector.clone(),
            request(Body::from_stream(body)),
            config()
        )
        .await
        .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    drop((all, large));
    let response =
        serve_request(connector.clone(), request(Body::from("not JSON")), config()).await;
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(
        connector
            .envelopes
            .small
            .clone()
            .try_acquire_many_owned(SMALL_ENVELOPE_BYTES as u32)
            .is_ok()
    );
}

#[tokio::test]
async fn hosted_attachment_holds_weight_through_outer_body_drop_and_eof() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let fixture = Fixture::new().await;
    let joined = fixture.join().await;
    let content = vec![0xff; 128 * 1024];
    let uploaded = result(
        &call(
            &fixture.connector,
            "room_upload_attachment",
            json!({
                "connection_id":joined["connection_id"], "filename":"proof.bin",
                "content_type":"application/octet-stream", "data_base64":STANDARD.encode(&content)
            }),
        )
        .await,
    );
    let id = &uploaded["attachment"]["id"];
    call(
        &fixture.connector,
        "room_say",
        json!({
            "connection_id":joined["connection_id"], "request_id":Uuid::new_v4().to_string(),
            "content":"Published attachment", "attachment_ids":[id]
        }),
    )
    .await;
    let available = fixture.state.downloads.available();
    for drain in [false, true] {
        let response = serve_request(
            fixture.connector.clone(),
            request(Body::from(tool_body(
                "room_read_attachment",
                json!({
                    "connection_id":joined["connection_id"], "attachment_id":id
                }),
            ))),
            config(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body().into_data_stream();
        let first = body.next().await.expect("outer content").expect("body");
        assert_eq!(
            fixture.state.downloads.available(),
            available - (content.len() * 8 + 65536)
        );
        if drain {
            let mut encoded = first.to_vec();
            while let Some(piece) = body.next().await {
                encoded.extend_from_slice(&piece.expect("piece"));
            }
            let reply: Value = serde_json::from_slice(&encoded).expect("MCP response");
            assert_ne!(reply["result"]["isError"], true);
            let blob = reply["result"]["content"][1]["resource"]["blob"]
                .as_str()
                .expect("binary content");
            assert_eq!(STANDARD.decode(blob).expect("base64"), content);
        }
        drop(body);
        assert_eq!(fixture.state.downloads.available(), available);
    }
    fixture.stop().await;
}
