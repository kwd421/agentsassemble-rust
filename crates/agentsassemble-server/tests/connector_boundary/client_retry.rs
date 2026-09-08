use super::{Client, InviteScope, Uuid, fixture, human_invite, json};
use agentsassemble_protocol::RoomAction;
use agentsassemble_server::connector_client::RoomConnectorClient;
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    response::Response,
};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct Relay {
    upstream: String,
    http: Client,
    lost: Arc<AtomicU8>,
}

#[tokio::test]
async fn lost_admission_and_command_responses_recover_the_original_receipts()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let cancellation = CancellationToken::new();
    let stopping = cancellation.clone();
    let relay = Router::new().fallback(forward).with_state(Relay {
        upstream: server.base_url.clone(),
        http: Client::new(),
        lost: Arc::new(AtomicU8::new(0)),
    });
    let task = tokio::spawn(async move {
        axum::serve(listener, relay)
            .with_graceful_shutdown(stopping.cancelled_owned())
            .await
    });
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let client = RoomConnectorClient::new(
        &format!("http://{address}/join?token={}", invite.invite_bearer),
        "Retry AI",
        None,
    )?;
    assert!(client.join().await.is_err());
    client.join().await?;
    let payload = json!({"content":"One committed contribution"});
    assert!(
        client
            .command(RoomAction::MessageSend, payload.clone())
            .await
            .is_err()
    );
    let different = client
        .command(
            RoomAction::MessageSend,
            json!({"content":"Different request"}),
        )
        .await;
    assert!(different.is_err_and(|error| error.code == "previous_connector_command_unresolved"));
    let receipt = client.command(RoomAction::MessageSend, payload).await?;
    assert_eq!(receipt["deduplicated"], true);
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot
            .participants
            .iter()
            .filter(|participant| participant.participant_type == "agent")
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.content.as_deref() == Some("One committed contribution"))
            .count(),
        1
    );
    client
        .command(RoomAction::ParticipantLeave, json!({}))
        .await?;
    client.close();
    cancellation.cancel();
    task.await??;
    server.stop().await;
    Ok(())
}

async fn forward(State(relay): State<Relay>, request: Request) -> Response {
    let (mut parts, body) = request.into_parts();
    parts.headers.remove("host");
    let path = parts.uri.path();
    let mask = match path {
        "/api/room-connector/join" => 1,
        "/api/room-connector/command" => 2,
        _ => 0,
    };
    let body = axum::body::to_bytes(body, 65536)
        .await
        .unwrap_or_else(|error| panic!("relay body: {error}"));
    let response = relay
        .http
        .request(parts.method, format!("{}{}", relay.upstream, parts.uri))
        .headers(parts.headers)
        .body(body)
        .send()
        .await
        .unwrap_or_else(|_| panic!("relay request failed"));
    let status = response.status();
    assert!(status.is_success(), "upstream status {status} on {path}");
    let bytes = response
        .bytes()
        .await
        .unwrap_or_else(|_| panic!("relay response failed"));
    let body = if mask != 0 && relay.lost.fetch_or(mask, Ordering::SeqCst) & mask == 0 {
        Body::from("{")
    } else {
        Body::from(bytes)
    };
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(body)
        .unwrap_or_else(|error| panic!("relay response: {error}"))
}
