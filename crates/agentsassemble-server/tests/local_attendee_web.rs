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

use crate::support::attendee;
use crate::support::human_invite;
use crate::support::provider_fixture;

#[tokio::test]
async fn local_http_creation_uses_only_its_local_operator_and_preserves_remote_membership_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let (room_store, invite) = attendee::fixture().await?;
    let room_server = human_invite::start(room_store.clone()).await;
    let local_store = SqliteStore::open("sqlite::memory:").await?;
    local_store
        .bootstrap_local_authority(&Uuid::new_v4().to_string(), "Local operator")
        .await?;
    // Computer B remains a local operator after the account's server is A.
    local_store.restrict_hosting(false).await?;
    let directory = tempfile::tempdir()?;
    let catalog = ProviderCatalogService::fixed(full_access_catalog(directory.path())?);
    let tickets = TicketStore::new(std::time::Duration::from_secs(30), 16);
    let state = AppState::local(local_store.clone(), tickets.clone(), catalog).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let cancellation = CancellationToken::new();
    let server = tokio::spawn(serve(listener, state, cancellation.clone(), async {
        Ok(())
    }));
    let client = Client::new();
    assert_non_host_catalog(&client, &base, &tickets, &local_store).await?;
    let id = Uuid::new_v4();
    let route = format!("{base}/api/local-attendees");
    let input = json!({"request_id":id,"room_id":"general","room_uid":invite.room_uid,
        "invite_url":format!("{}/join?token={}", room_server.base_url, invite.invite_bearer),
        "creation":{"provider_id":"codex","display_name":"Local HTTP draft","workspace":directory.path(),
            "catalog_revision":"catalog-boundary-1","permission_mode":"full_access","start":false}});
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

async fn assert_non_host_catalog(
    client: &Client,
    base: &str,
    tickets: &TicketStore,
    local_store: &SqliteStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let catalog_response = client
        .get(format!("{base}/api/provider-catalog"))
        .bearer_auth(operator_ticket(tickets).await?)
        .send()
        .await?;
    assert_eq!(catalog_response.status(), StatusCode::OK);
    assert_eq!(
        local_store.hosting_restriction().await?.as_deref(),
        Some("device")
    );
    assert!(local_store.registration_epoch().await?.is_none());
    Ok(())
}

fn full_access_catalog(
    directory: &std::path::Path,
) -> Result<agentsassemble_domain::ProviderCatalog, Box<dyn std::error::Error>> {
    let mut catalog = provider_fixture::agent_catalog(directory, None);
    catalog.providers[0]
        .controls
        .iter_mut()
        .find(|control| control.key == "permission_mode")
        .ok_or("permission control")?
        .options
        .push(agentsassemble_domain::ProviderControlOption {
            value: "full_access".into(),
            label: "전체 액세스".into(),
            metadata: std::collections::BTreeMap::default(),
        });
    Ok(catalog)
}
