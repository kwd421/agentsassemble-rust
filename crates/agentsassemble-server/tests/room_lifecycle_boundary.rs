use agentsassemble_domain::{InviteScope, LOCAL_OPERATOR_USER_ID};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use serde_json::{Value, json};
use std::time::Duration;

mod support {
    pub mod human_invite;
    pub mod local_socket;
    pub mod room_socket_peer;
}

use support::human_invite::{
    RunningServer, canonical_session_token, fixture, join, open_session_socket, start,
};

#[tokio::test]
async fn http_lifecycle_revokes_access_and_restores_without_a_room_socket()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    let authority = store.local_bootstrap_status().await?;
    let uid = store.snapshot("general", 0, 20).await?.room.room_uid;
    let server = start(store.clone()).await;
    let client = Client::new();
    assert!(
        server
            .state()
            .server_product_surface
            .http_routes
            .iter()
            .any(|route| route.path == "/api/rooms/lifecycle")
    );
    assert!(
        server
            .state()
            .server_product_surface
            .websocket_actions
            .iter()
            .all(|action| !matches!(
                action,
                agentsassemble_protocol::RoomAction::RoomClose
                    | agentsassemble_protocol::RoomAction::RoomArchive
                    | agentsassemble_protocol::RoomAction::RoomDelete
            ))
    );
    let admitted = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x78; 32])),
        "b23e4567-e89b-12d3-a456-426614174000",
        "Archived Guest",
        "",
    )
    .await;
    let token = canonical_session_token(&admitted);
    let mut guest = open_session_socket(&client, &server.base_url, token).await;
    let mut manager =
        support::local_socket::connect(&server.base_url, server.state(), "general").await;
    manager.subscribe(0).await;
    manager.receive_json().await;
    let body = json!({"server_id": authority.server_id, "authority_lineage_id": authority.authority_lineage_id, "room_id": "general", "request_id": "archive-http", "action": "room.archive", "payload": {"room_uid": uid, "archived": true}});
    // HTTP-only lifecycle is not silently accepted on the socket adapter.
    manager.send_json(&json!({"op": "command", "request_id": "archive-socket", "action": "room.archive", "payload": body["payload"]})).await;
    let rejected = manager
        .receive_json_with_timeout(Duration::from_secs(5))
        .await;
    assert_eq!(rejected["op"], "nack");
    assert_eq!(rejected["error"]["code"], "unsupported_transport");
    let archived = change(&client, &server, &body).await?;
    let final_event = manager
        .receive_json_with_timeout(Duration::from_secs(5))
        .await;
    assert_eq!(final_event["op"], "event");
    assert_eq!(final_event["events"][0]["type"], "room_archived");
    assert_eq!(final_event["events"][0]["room"]["room_uid"], json!(uid));
    assert!(tokio::time::timeout(Duration::from_secs(5), manager.wait_closed()).await?);
    assert_eq!(archived["resolution"], "committed");
    assert_eq!(archived["result"]["room"]["status"], "archived");
    assert!(tokio::time::timeout(Duration::from_secs(5), guest.wait_closed()).await?);
    let refused = client
        .post(format!("{}/api/session-tickets/socket", server.base_url))
        .bearer_auth(token)
        .send()
        .await?;
    assert_eq!(refused.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert_eq!(change(&client, &server, &body).await?["deduplicated"], true);
    let mut restore = body.clone();
    restore["request_id"] = json!("restore-http");
    restore["payload"]["archived"] = json!(false);
    let restored = change(&client, &server, &restore).await?;
    assert_eq!(restored["result"]["room"]["status"], "active");
    let mut resumed =
        support::local_socket::connect(&server.base_url, server.state(), "general").await;
    assert_eq!(resumed.subscribe(0).await["op"], "subscribed");
    assert_eq!(resumed.receive_json().await["op"], "snapshot");
    resumed.close().await;
    server.stop().await;
    Ok(())
}

async fn change(
    client: &Client,
    server: &RunningServer,
    body: &Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let response = request_lifecycle(client, server, body).await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    Ok(response.json().await?)
}

async fn request_lifecycle(
    client: &Client,
    server: &RunningServer,
    body: &Value,
) -> Result<reqwest::Response, reqwest::Error> {
    let ticket = server
        .state()
        .tickets
        .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue owner ticket: {error}"))
        .ticket;
    client
        .post(format!("{}/api/rooms/lifecycle", server.base_url))
        .bearer_auth(ticket)
        .json(body)
        .send()
        .await
}

#[tokio::test]
async fn deletion_retires_room_runtime_and_http_replay_preserves_recreated_room()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, _credentials) = fixture(InviteScope::ReadOnly).await;
    let authority = store.local_bootstrap_status().await?;
    let room = store.snapshot("general", 0, 20).await?.room;
    let server = start(store.clone()).await;
    let client = Client::new();
    let mut manager =
        support::local_socket::connect(&server.base_url, server.state(), "general").await;
    manager.subscribe(0).await;
    manager.receive_json().await;
    let mut room_events = server.state().rooms.subscribe("general").await;
    let body = json!({"server_id": authority.server_id, "authority_lineage_id": authority.authority_lineage_id,
        "room_id": "general", "request_id": "delete-http", "action": "room.delete",
        "payload": {"room_uid": room.room_uid, "confirmation_name": room.label}});
    let response = request_lifecycle(&client, &server, &body).await?;
    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    let pending: Value = response.json().await?;
    assert_eq!(pending["resolution"], "unresolved");
    assert_eq!(pending["code"], "room_deletion_pending");
    let closed = manager
        .receive_json_with_timeout(Duration::from_secs(5))
        .await;
    assert_eq!(
        closed["events"][0]["room"]["room_uid"],
        json!(room.room_uid)
    );
    assert_eq!(closed["events"][0]["type"], "room_closed");
    assert!(tokio::time::timeout(Duration::from_secs(5), manager.wait_closed()).await?);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match room_events.recv().await {
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(error) => panic!("deleted room event stream: {error}"),
            }
        }
    })
    .await?;
    assert!(!store.room_exists("general").await?);
    let completed = change(&client, &server, &body).await?;
    assert_eq!(completed["result"]["deleted"], true);
    assert_eq!(completed["deduplicated"], true);
    let new = store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "general", &room.label)
        .await?;
    assert_ne!(new.room.room_uid, room.room_uid);
    assert_eq!(change(&client, &server, &body).await?, completed);
    assert_eq!(
        store.snapshot("general", 0, 20).await?.room.room_uid,
        new.room.room_uid
    );
    let mut replacement =
        support::local_socket::connect(&server.base_url, server.state(), "general").await;
    replacement.subscribe(0).await;
    assert_eq!(
        replacement.receive_json().await["room"]["room_uid"],
        json!(new.room.room_uid)
    );
    replacement.close().await;
    server.stop().await;
    Ok(())
}
