#![cfg(unix)]
use agentsassemble_domain::{
    LOCAL_OPERATOR_USER_ID, LocalAttendeePhase as Phase, LocalAttendeeStatus,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, TicketStore, serve};
use reqwest::{Client, StatusCode};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[path = "support/attendee.rs"]
mod attendee;
#[path = "support/human_invite.rs"]
mod human_invite;
#[path = "support/provider_fixture.rs"]
mod provider_fixture;
#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

#[tokio::test]
async fn local_http_creation_uses_only_its_local_operator_and_preserves_remote_membership_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let (room_store, invite) = attendee::fixture().await?;
    let room_server = human_invite::start(room_store.clone()).await;
    let local_store = SqliteStore::open("sqlite::memory:").await?;
    local_store
        .bootstrap_local_authority(&Uuid::new_v4().to_string(), "Local operator")
        .await?;
    let directory = tempfile::tempdir()?;
    let catalog =
        ProviderCatalogService::fixed(provider_fixture::agent_catalog(directory.path(), None));
    let tickets = TicketStore::new(std::time::Duration::from_secs(30), 16);
    let state = AppState::local(local_store.clone(), tickets.clone(), catalog).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let cancellation = CancellationToken::new();
    let server = tokio::spawn(serve(listener, state, cancellation.clone(), async {
        Ok(())
    }));
    let client = Client::new();
    let id = Uuid::new_v4();
    let route = format!("{base}/api/local-attendees");
    let input = json!({"request_id":id,"room_id":"general","room_uid":invite.room_uid,
        "invite_url":format!("{}/join?token={}", room_server.base_url, invite.invite_bearer),
        "creation":{"provider_id":"codex","display_name":"Local HTTP draft","workspace":directory.path(),
            "catalog_revision":"catalog-boundary-1","start":false}});
    reject_before_admission(&client, &route, &input, &tickets, &room_store).await?;
    let token = operator_ticket(&tickets).await?;
    let response = client
        .post(&route)
        .bearer_auth(&token)
        .header("Origin", "tauri://localhost")
        .json(&input)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(
        response.headers()["access-control-allow-origin"],
        "tauri://localhost"
    );
    let admitted: LocalAttendeeStatus = response.json().await?;
    assert_eq!(admitted.phase, Phase::Admitted);
    let status_route = format!("{route}/{id}");
    assert_eq!(
        client
            .get(&status_route)
            .bearer_auth(token)
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let observed: LocalAttendeeStatus = client
        .get(&status_route)
        .bearer_auth(operator_ticket(&tickets).await?)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(observed, admitted);
    let snapshot = room_store.snapshot("general", 0, 200).await?;
    assert_eq!(snapshot.agent_sessions.len(), 1);
    assert_eq!(
        Some(snapshot.agent_sessions[0].participant_id.as_str()),
        admitted.participant_id.as_deref()
    );
    let event = snapshot
        .events
        .iter()
        .find(|event| event.event_type == "agent_session_created")
        .ok_or("creation event missing")?;
    assert_eq!(
        event.extra["attendee_invite_id"],
        invite.invite_id.to_string()
    );
    assert!(!serde_json::to_string(event)?.contains(&invite.invite_bearer));
    let stopped: LocalAttendeeStatus = client
        .post(&status_route)
        .bearer_auth(operator_ticket(&tickets).await?)
        .json(&json!({"action":"cancel"}))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(stopped.phase, Phase::Stopped);
    assert_eq!(stopped.participant_id, admitted.participant_id);
    cancellation.cancel();
    server.await??;
    room_server.stop().await;
    Ok(())
}

async fn reject_before_admission(
    client: &Client,
    route: &str,
    input: &serde_json::Value,
    tickets: &TicketStore,
    room_store: &SqliteStore,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        client.post(route).body("invalid").send().await?.status(),
        StatusCode::UNAUTHORIZED
    );
    let wrong = tickets
        .issue_settings_directory_read(LOCAL_OPERATOR_USER_ID.to_owned())
        .await?
        .ticket;
    assert_eq!(
        client
            .post(route)
            .bearer_auth(wrong)
            .json(input)
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let mut invalid_selection = input.clone();
    invalid_selection["creation"]["model"] = "absent-local-model".into();
    let rejected = client
        .post(route)
        .bearer_auth(operator_ticket(tickets).await?)
        .json(&invalid_selection)
        .send()
        .await?;
    assert_eq!(rejected.status(), StatusCode::CONFLICT);
    assert!(
        room_store
            .snapshot("general", 0, 200)
            .await?
            .agent_sessions
            .is_empty()
    );
    Ok(())
}

async fn operator_ticket(tickets: &TicketStore) -> Result<String, Box<dyn std::error::Error>> {
    Ok(tickets
        .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
        .await?
        .ticket)
}
