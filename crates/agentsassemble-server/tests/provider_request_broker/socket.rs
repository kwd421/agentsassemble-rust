use super::{
    TestResult, admitted_human, assigned_request, human_invite, ready_report, receive_result,
    room_socket_peer,
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};
use uuid::Uuid;

type Peer =
    room_socket_peer::RoomSocketPeer<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[tokio::test]
async fn attendee_wire_delivers_once_and_replacement_closes_the_old_request() -> TestResult {
    let (store, human, bearer) = admitted_human().await?;
    let server = human_invite::start(store.clone()).await;
    let (_, mut request, attendee_bearer) = assigned_request(&store, &human).await?;
    let mut attendee = connect(&server.base_url, &attendee_bearer).await?;
    ready(&mut attendee).await;
    let id = request.request.provider_request_id;
    let command =
        json!({"action":"provider_request_open", "request_id":Uuid::new_v4(), "request":request});
    attendee.send_json(&command).await;
    let opened = receive(&mut attendee, "ack").await;
    assert_eq!(opened["deduplicated"], false);
    attendee.send_json(&command).await;
    let replay = receive(&mut attendee, "ack").await;
    assert_eq!(replay["deduplicated"], true);
    assert_eq!(opened["event_id"], replay["event_id"]);
    let mut owner =
        human_invite::open_session_socket(&reqwest::Client::new(), &server.base_url, &bearer).await;
    owner.send_json(&json!({"op":"command", "request_id":id, "action":"provider.request.resolve", "payload":{"response_kind":"answers", "answers":{"secret":["external-wire-secret"]}}})).await;
    receive_result(&mut owner, "ack").await;
    let response = receive(&mut attendee, "provider_response").await;
    assert_eq!(response["provider_request_id"], id.to_string());
    assert_eq!(
        response["resolution"]["answers"]["secret"],
        json!(["external-wire-secret"])
    );
    assert_eq!(
        store.pending_provider_request_ids("general").await?,
        vec![id]
    );
    let mut delivered = json!({"action":"provider_request_delivered", "request_id":Uuid::new_v4(), "provider_request_id":id, "delivered":true});
    attendee.send_json(&delivered).await;
    assert_eq!(receive(&mut attendee, "ack").await["deduplicated"], false);
    attendee.send_json(&delivered).await;
    assert_eq!(receive(&mut attendee, "ack").await["deduplicated"], true);
    delivered["delivered"] = json!(false);
    attendee.send_json(&delivered).await;
    assert_eq!(
        receive(&mut attendee, "nack").await["error"]["code"],
        "command_conflict"
    );
    assert!(
        store
            .pending_provider_request_ids("general")
            .await?
            .is_empty()
    );
    assert!(
        !serde_json::to_string(&store.snapshot("general", 0, 200).await?.events)?
            .contains("external-wire-secret")
    );

    request.request.provider_request_id = Uuid::new_v4();
    let next =
        json!({"action":"provider_request_open", "request_id":Uuid::new_v4(), "request":request});
    attendee.send_json(&next).await;
    receive(&mut attendee, "ack").await;
    let mut replacement = connect(&server.base_url, &attendee_bearer).await?;
    ready(&mut replacement).await;
    replacement.send_json(&next).await;
    assert_eq!(
        receive(&mut replacement, "nack").await["error"]["code"],
        "command_conflict"
    );
    assert!(
        store
            .pending_provider_request_ids("general")
            .await?
            .is_empty()
    );
    replacement.close().await;
    owner.close().await;
    server.stop().await;
    Ok(())
}

async fn connect(base: &str, bearer: &str) -> Result<Peer, Box<dyn std::error::Error>> {
    let mut request = format!(
        "{}/api/room-attendee/ws",
        base.replacen("http://", "ws://", 1)
    )
    .into_client_request()?;
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {bearer}").parse()?);
    let mut peer = Peer::new(connect_async(request).await?.0);
    receive(&mut peer, "connected").await;
    Ok(peer)
}

async fn ready(peer: &mut Peer) {
    peer.send_json(
        &json!({"action":"ready", "request_id":Uuid::new_v4(), "report":ready_report()}),
    )
    .await;
    receive(peer, "ack").await;
    receive(peer, "turn").await;
}

async fn receive(peer: &mut Peer, expected: &str) -> Value {
    let frame = peer.receive_json_with_timeout(Duration::from_secs(2)).await;
    assert_eq!(frame["type"], expected);
    frame
}
