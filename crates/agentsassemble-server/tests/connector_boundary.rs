use agentsassemble_domain::{InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID};
use agentsassemble_persistence::{RoomManagerAuthority, SqliteStore};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use serde_json::{Value, json};
use uuid::Uuid;

#[path = "support/human_invite.rs"]
mod human_invite;
#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

#[tokio::test]
async fn connector_http_keeps_agent_custody_and_publishes_exact_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let client = Client::new();
    let mut events = server.rooms().subscribe("general").await;
    for scope in [InviteScope::ReadWrite, InviteScope::ReadOnly] {
        let invite = store
            .create_connector_invite(&manager, Uuid::new_v4(), scope, chrono::Utc::now())
            .await?;
        let body = json!({"request_id":Uuid::new_v4(),"client_secret":URL_SAFE_NO_PAD.encode([7;32]),"display_name":"Current conversation"});
        let joined = client
            .post(format!("{}/api/room-connector/join", server.base_url))
            .bearer_auth(&invite.invite_bearer)
            .json(&body)
            .send()
            .await?;
        assert_eq!(joined.status(), 200);
        assert_eq!(joined.headers()["cache-control"], "private, no-store");
        let joined: Value = joined.json().await?;
        let bearer = joined["session_bearer"]
            .as_str()
            .ok_or("session credential missing")?;
        let joined_event = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let event = events.recv().await?;
                if event.event_type == "participant_joined" {
                    return Ok::<_, tokio::sync::broadcast::error::RecvError>(event);
                }
            }
        })
        .await??;
        assert_eq!(joined_event.actor.participant_type, "agent");
        let endpoint = format!("{}/api/room-connector/command", server.base_url);
        let message = json!({"request_id":Uuid::new_v4().to_string(),"action":"message.send","payload":{"content":"Connector public proof"}});
        let wrong = client
            .post(&endpoint)
            .bearer_auth(&invite.invite_bearer)
            .json(&message)
            .send()
            .await?;
        assert_eq!(wrong.status(), 401);
        verify_commands(&client, &endpoint, bearer, scope, &message).await?;
    }
    server.stop().await;
    Ok(())
}

async fn verify_commands(
    client: &Client,
    endpoint: &str,
    bearer: &str,
    scope: InviteScope,
    message: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let sent = client
        .post(endpoint)
        .bearer_auth(bearer)
        .json(message)
        .send()
        .await?;
    if scope == InviteScope::ReadWrite {
        assert_eq!(sent.status(), 200);
        let sent: Value = sent.json().await?;
        assert_eq!(
            sent["result"]["event"]["actor"]["participant_type"],
            "agent"
        );
        let replay: Value = client
            .post(endpoint)
            .bearer_auth(bearer)
            .json(message)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(replay["deduplicated"], true);
        assert_eq!(replay["result"], sent["result"]);
    } else {
        assert_eq!(sent.status(), 403);
    }
    let denied = json!({"request_id":Uuid::new_v4().to_string(),"action":"side_chat.send","payload":{"content":"private"}});
    assert!(
        !client
            .post(endpoint)
            .bearer_auth(bearer)
            .json(&denied)
            .send()
            .await?
            .status()
            .is_success()
    );
    let leave =
        json!({"request_id":Uuid::new_v4().to_string(),"action":"participant.leave","payload":{}});
    assert_eq!(
        client
            .post(endpoint)
            .bearer_auth(bearer)
            .json(&leave)
            .send()
            .await?
            .status(),
        200
    );
    assert_eq!(
        client
            .post(endpoint)
            .bearer_auth(bearer)
            .json(message)
            .send()
            .await?
            .status(),
        403
    );
    Ok(())
}

async fn fixture() -> Result<(SqliteStore, RoomManagerAuthority), Box<dyn std::error::Error>> {
    let store = SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority(&Uuid::new_v4().to_string(), "Host")
        .await?;
    store
        .create_room_for_local_operator(&Uuid::new_v4().to_string(), "general", "General")
        .await?;
    let manager = RoomManagerAuthority::Local(
        store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await?,
    );
    Ok((store, manager))
}

#[path = "connector_boundary/reads.rs"]
mod reads;

#[path = "connector_boundary/client.rs"]
mod connector_client;

#[path = "connector_boundary/client_retry.rs"]
mod client_retry;
#[path = "support/lossy_http.rs"]
mod lossy_http;

#[path = "connector_boundary/mcp.rs"]
mod mcp;

#[path = "connector_boundary/mcp_remote.rs"]
mod mcp_remote;
