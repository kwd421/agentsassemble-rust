use std::time::Duration;

use agentsassemble_domain::InviteScope;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use serde_json::{Value, json};
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};
use uuid::Uuid;

#[path = "support/attendee.rs"]
mod attendee;
#[path = "support/human_invite.rs"]
mod human_invite;
#[path = "support/local_socket.rs"]
mod local_socket;
#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Peer =
    room_socket_peer::RoomSocketPeer<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[tokio::test]
async fn private_attendee_socket_resumes_exact_turn_and_replays_one_public_result() -> TestResult {
    let (store, invite) = attendee::fixture().await?;
    let human_invite = human_invite::persist_invite(
        &store,
        InviteScope::ReadWrite,
        1,
        "socket-human",
        "Socket Human",
    )
    .await;
    let server = human_invite::start(store.clone()).await;
    let client = Client::new();
    let joined: Value = client.post(format!("{}/api/room-attendee/join", server.base_url))
        .bearer_auth(&invite.invite_bearer)
        .json(&json!({"request_id":Uuid::new_v4(),"client_secret":URL_SAFE_NO_PAD.encode([9;32]),"provider":"codex","display_name":"Socket AI"}))
        .send().await?.error_for_status()?.json().await?;
    let bearer = joined["session_bearer"].as_str().ok_or("bearer missing")?;
    reject_wrong_purpose(&server.base_url, &invite.invite_bearer).await?;
    let mut old = connect(&server.base_url, bearer).await?;
    ready(&mut old).await;
    let mut human = human_input(&server.base_url, human_invite.invite_token()).await?;
    let first = old.receive_json_with_timeout(Duration::from_secs(2)).await;
    assert_eq!(first["type"], "turn");
    assert_eq!(first["assignment"]["resume"], false);
    assert!(
        first["assignment"]["input"]["room_view"]
            .as_str()
            .ok_or("view missing")?
            .contains("External socket reply please")
    );
    let mut current = connect(&server.base_url, bearer).await?;
    ready(&mut current).await;
    let resumed = current
        .receive_json_with_timeout(Duration::from_secs(2))
        .await;
    assert_eq!(resumed["assignment"]["resume"], true);
    assert_eq!(
        resumed["assignment"]["authority"],
        first["assignment"]["authority"]
    );
    assert_eq!(resumed["assignment"]["input"], first["assignment"]["input"]);
    assert!(tokio::time::timeout(Duration::from_secs(2), old.wait_closed()).await?);
    verify_started_and_result(&mut current, &resumed).await;
    let snapshot = store.snapshot("general", 0, 200).await?;
    let encoded = serde_json::to_string(
        &json!({"events":snapshot.events, "agent_sessions":snapshot.agent_sessions}),
    )?;
    assert_eq!(encoded.matches("External socket result").count(), 1);
    for private in [
        "external-runtime",
        "external-owner",
        "external-lease",
        bearer,
    ] {
        assert!(!encoded.contains(private));
    }
    current.close().await;
    human.close().await;
    server.stop().await;
    Ok(())
}

async fn connect(base: &str, bearer: &str) -> Result<Peer, Box<dyn std::error::Error>> {
    let url = format!("{}/api/room-attendee/ws", base.replace("http://", "ws://"));
    let mut request = url.into_client_request()?;
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {bearer}").parse()?);
    let (socket, response) = connect_async(request).await?;
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let mut peer = Peer::new(socket);
    assert_eq!(peer.receive_json().await["type"], "connected");
    Ok(peer)
}

async fn reject_wrong_purpose(base: &str, invite: &str) -> TestResult {
    let url = format!("{}/api/room-attendee/ws", base.replace("http://", "ws://"));
    for credential in [None, Some(invite)] {
        let mut request = url.clone().into_client_request()?;
        if let Some(credential) = credential {
            request
                .headers_mut()
                .insert("authorization", format!("Bearer {credential}").parse()?);
        }
        let result = connect_async(request).await;
        assert!(
            matches!(result, Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status() == 401)
        );
    }
    Ok(())
}

async fn ready(peer: &mut Peer) {
    peer.send_json(&json!({"action":"ready", "request_id":Uuid::new_v4(), "report":{
        "runtime_handle_id":"external-runtime", "runtime_owner_id":"external-owner", "runtime_lease_token":"external-lease",
        "provider_session_id":"external-session", "model":"contract-model", "reasoning_effort":"", "service_tier":"", "variant":"",
        "execution_harness":"builtin", "permission_mode":"meeting_read_only", "max_output_tokens":0
    }})).await;
    assert_eq!(peer.receive_json().await["type"], "ack");
}

async fn human_input(base: &str, invite: &str) -> Result<Peer, Box<dyn std::error::Error>> {
    let client = Client::new();
    let joined = human_invite::join(
        &client,
        base,
        invite,
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xB9; 32])),
        &Uuid::new_v4().to_string(),
        "Socket Human",
        "",
    )
    .await;
    let mut human = human_invite::open_session_socket(
        &client,
        base,
        human_invite::canonical_session_token(&joined),
    )
    .await;
    human.send_json(&json!({"op":"command", "request_id":"external-socket-input", "action":"message.send", "payload":{"content":"External socket reply please"}})).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let frame = human.receive_json().await;
            if frame["op"] == "ack" {
                break;
            }
            assert_ne!(frame["op"], "error");
        }
    })
    .await?;
    Ok(human)
}

async fn verify_started_and_result(peer: &mut Peer, turn: &Value) {
    let authority = &turn["assignment"]["authority"];
    let started = json!({"action":"started", "request_id":Uuid::new_v4(), "authority":authority, "provider_turn_id":"socket-provider-turn"});
    for _ in 0..2 {
        peer.send_json(&started).await;
        assert_eq!(peer.receive_json().await["type"], "ack");
    }
    let mut report = authority.clone();
    let fields = report
        .as_object_mut()
        .unwrap_or_else(|| panic!("assignment authority must be an object"));
    fields.remove("room_id");
    fields.remove("session_id");
    report["request_id"] = json!(Uuid::new_v4());
    report["provider_turn_id"] = json!("socket-provider-turn");
    report["provider_session_id"] = json!("external-session");
    report["outcome"] =
        json!({"kind":"message", "content":"External socket result", "target_agent_id":""});
    let request = json!({"action":"report", "report":report});
    peer.send_json(&request).await;
    let ack = peer.receive_json().await;
    assert_eq!(ack["type"], "ack");
    assert_eq!(ack["deduplicated"], false);
    peer.send_json(&request).await;
    let replay = peer.receive_json().await;
    assert_eq!(replay["type"], "ack");
    assert_eq!(replay["deduplicated"], true);
    assert_eq!(replay["event_id"], ack["event_id"]);
}

#[tokio::test]
async fn kicked_external_runtime_stays_pending_until_its_cleanup_report_is_published() -> TestResult
{
    let (store, invite) = attendee::fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let client = Client::new();
    let joined: Value = client.post(format!("{}/api/room-attendee/join", server.base_url))
        .bearer_auth(&invite.invite_bearer)
        .json(&json!({"request_id":Uuid::new_v4(),"client_secret":URL_SAFE_NO_PAD.encode([10;32]),"provider":"codex","display_name":"Cleanup AI"}))
        .send().await?.error_for_status()?.json().await?;
    let bearer = joined["session_bearer"].as_str().ok_or("bearer missing")?;
    let mut external = connect(&server.base_url, bearer).await?;
    ready(&mut external).await;
    let endpoint = format!("{}/api/room-attendee/cleanup", server.base_url);
    let before: Value = client
        .get(&endpoint)
        .bearer_auth(bearer)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(before["stop"].is_null());
    let mut manager = local_socket::connect(&server.base_url, server.state(), "general").await;
    manager.subscribe(0).await;
    manager.receive_json().await;
    manager.send_json(&json!({"op":"command", "request_id":"external-runtime-kick", "action":"participant.kick", "payload":{"participant_id":joined["participant_id"]}})).await;
    loop {
        let frame = manager
            .receive_json_with_timeout(Duration::from_secs(2))
            .await;
        if frame["op"] == "ack" {
            assert_eq!(frame["result"]["cleanup_pending"], true);
            break;
        }
        assert_ne!(frame["op"], "error");
    }
    assert!(tokio::time::timeout(Duration::from_secs(2), external.wait_closed()).await?);
    let pending = store.snapshot("general", 0, 200).await?;
    assert!(pending.agent_sessions[0].provider_session_active);
    assert_ne!(
        pending.agent_sessions[0].runtime_status,
        agentsassemble_domain::AgentRuntimeStatus::Stopped
    );
    let response = client
        .get(&endpoint)
        .bearer_auth(bearer)
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let stop: Value = response.json().await?;
    let report = json!({"request_id":Uuid::new_v4(), "stopped":stop["stop"]});
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(&invite.invite_bearer)
            .json(&report)
            .send()
            .await?
            .status(),
        401
    );
    let mut published = server.rooms().subscribe("general").await;
    let event_id = report_cleanup_retry(&client, &endpoint, bearer, &report).await?;
    let event = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = published.recv().await?;
            if event.id == event_id {
                return Ok::<_, tokio::sync::broadcast::error::RecvError>(event);
            }
        }
    })
    .await??;
    assert_eq!(event.id, event_id);
    assert_eq!(event.extra["agent_session"]["runtime_status"], "stopped");
    assert_eq!(
        event.extra["agent_session"]["provider_session_active"],
        false
    );
    let serialized = serde_json::to_string(&event)?;
    assert!(!serialized.contains("external-lease"));
    let completed: Value = client
        .get(&endpoint)
        .bearer_auth(bearer)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(completed["stop"].is_null());
    manager.close().await;
    server.stop().await;
    Ok(())
}

async fn report_cleanup_retry(
    client: &Client,
    endpoint: &str,
    bearer: &str,
    report: &Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut event_id = Value::Null;
    for deduplicated in [false, true] {
        let ack: Value = client
            .post(endpoint)
            .bearer_auth(bearer)
            .json(report)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(ack["deduplicated"], deduplicated);
        if deduplicated {
            assert_eq!(ack["event_id"], event_id);
        } else {
            event_id = ack["event_id"].clone();
        }
    }
    Ok(event_id)
}
