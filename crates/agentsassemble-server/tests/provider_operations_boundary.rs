use std::time::Duration;

use agentsassemble_domain::{LOCAL_OPERATOR_USER_ID, ProviderCatalog};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, TicketStore, serve};
use reqwest::{Client, StatusCode};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn refresh_requires_exact_operator_ticket_and_publishes_owned_catalog()
-> Result<(), Box<dyn std::error::Error>> {
    let store = SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority("f45c29c4-16a5-4a34-810d-929f7f9867a6", "Operator")
        .await?;
    let tickets = TicketStore::new(Duration::from_secs(30), 16);
    // This provider only discovers static local metadata; no account or executable runs.
    let catalog = ProviderCatalogService::discovering_selected(
        "custom_api",
        agentsassemble_provider::ProviderCredentialStore::production(),
    )?;
    let root = tempfile::tempdir()?;
    let state = AppState::local_with_provider_state_root(
        store,
        tickets.clone(),
        catalog.clone(),
        root.path(),
        agentsassemble_provider::ProviderCredentialStore::production(),
    )
    .await?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let route = format!(
        "http://{}/api/provider-catalog/refresh",
        listener.local_addr()?
    );
    let cancellation = CancellationToken::new();
    let server = tokio::spawn(serve(listener, state, cancellation.clone(), async {
        Ok(())
    }));
    let client = Client::new();
    let wrong = tickets
        .issue_settings_directory_read(LOCAL_OPERATOR_USER_ID.to_owned())
        .await?
        .ticket;
    let rejected = client
        .post(&route)
        .bearer_auth(&wrong)
        .body("invalid")
        .send()
        .await?;
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    assert!(
        tickets
            .consume_settings_directory_read(&wrong)
            .await
            .is_err()
    );
    let malformed = client
        .post(&route)
        .bearer_auth(
            tickets
                .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
                .await?
                .ticket,
        )
        .body("invalid")
        .send()
        .await?;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    let token = tickets
        .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
        .await?
        .ticket;
    let refreshed = client
        .post(&route)
        .bearer_auth(&token)
        .header("origin", "tauri://localhost")
        .send()
        .await?;
    assert_eq!(refreshed.status(), StatusCode::OK);
    assert_eq!(refreshed.headers()["cache-control"], "private, no-store");
    assert_eq!(
        refreshed.headers()["access-control-allow-origin"],
        "tauri://localhost"
    );
    let response: ProviderCatalog = refreshed.json().await?;
    assert_eq!(response, catalog.snapshot());
    assert_eq!(response.providers.len(), 1);
    assert_eq!(response.providers[0].id, "custom_api");
    assert_eq!(
        client
            .post(&route)
            .bearer_auth(token)
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_provider_setup_authority(&client, &route, &tickets, &catalog).await?;
    assert_restart_authority(&client, &route, &tickets).await?;
    assert_login_authority(&client, &route, &tickets).await?;
    assert_usage_authority(&client, &route, &tickets).await?;
    assert_resource_authority(&client, &route, &tickets).await?;
    assert_health_authority(&client, &route, &tickets, root.path()).await?;
    cancellation.cancel();
    tokio::time::timeout(Duration::from_secs(8), server).await???;
    Ok(())
}

async fn assert_provider_setup_authority(
    client: &Client,
    route: &str,
    tickets: &TicketStore,
    catalog: &ProviderCatalogService,
) -> Result<(), Box<dyn std::error::Error>> {
    let read_route = route.replace("provider-catalog/refresh", "provider-catalog");
    assert_eq!(
        client.get(&read_route).send().await?.status(),
        StatusCode::UNAUTHORIZED
    );
    let read = client
        .get(&read_route)
        .bearer_auth(
            tickets
                .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
                .await?
                .ticket,
        )
        .send()
        .await?;
    assert_eq!(read.status(), StatusCode::OK);
    assert_eq!(read.json::<ProviderCatalog>().await?, catalog.snapshot());
    for suffix in ["providers/update/check", "providers/update/start"] {
        let update_route = route.replace("provider-catalog/refresh", suffix);
        assert_eq!(
            client
                .post(&update_route)
                .json(&serde_json::json!({"provider_id":"grok","expected_version":"1.0.24"}))
                .send()
                .await?
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let rejected = client
            .post(&update_route)
            .bearer_auth(
                tickets
                    .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
                    .await?
                    .ticket,
            )
            .json(&serde_json::json!({"provider_id":"grok", "command":"update"}))
            .send()
            .await?;
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    }
    Ok(())
}

async fn assert_login_authority(
    client: &Client,
    route: &str,
    tickets: &TicketStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let login_route = route.replace("provider-catalog/refresh", "providers/login");
    assert_eq!(
        client
            .post(&login_route)
            .body("invalid")
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let unsupported = client
        .post(&login_route)
        .bearer_auth(
            tickets
                .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
                .await?
                .ticket,
        )
        .json(&serde_json::json!({"provider_id":"custom_api"}))
        .send()
        .await?;
    assert_eq!(unsupported.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        unsupported.json::<serde_json::Value>().await?["code"],
        "provider_login_unsupported"
    );
    let cancelled = client
        .post(format!("{login_route}/cancel"))
        .bearer_auth(
            tickets
                .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
                .await?
                .ticket,
        )
        .json(&serde_json::json!({"provider_id":"codex"}))
        .send()
        .await?;
    assert_eq!(
        cancelled.json::<serde_json::Value>().await?["status"],
        "not_running"
    );
    Ok(())
}

async fn assert_usage_authority(
    client: &Client,
    route: &str,
    tickets: &TicketStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let route = route.replace("provider-catalog/refresh", "providers/usage");
    assert_eq!(
        client.post(&route).body("invalid").send().await?.status(),
        StatusCode::UNAUTHORIZED
    );
    let response = client
        .post(&route)
        .bearer_auth(
            tickets
                .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
                .await?
                .ticket,
        )
        .json(&serde_json::json!({"provider_id":"custom_api"}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(
        response.json::<serde_json::Value>().await?["code"],
        "provider_usage_unsupported"
    );
    Ok(())
}

async fn assert_resource_authority(
    client: &Client,
    route: &str,
    tickets: &TicketStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let route = route.replace("provider-catalog/refresh", "local-resources");
    assert_eq!(
        client.get(&route).body("invalid").send().await?.status(),
        StatusCode::UNAUTHORIZED
    );
    let token = tickets
        .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
        .await?
        .ticket;
    let response = client
        .get(&route)
        .bearer_auth(&token)
        .header("origin", "tauri://localhost")
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(
        response.headers()["access-control-allow-origin"],
        "tauri://localhost"
    );
    let resources: agentsassemble_domain::LocalResourceStatus = response.json().await?;
    assert!(resources.cpu_sample_seconds.is_none());
    assert_eq!(
        client
            .get(&route)
            .bearer_auth(&token)
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    Ok(())
}

async fn assert_health_authority(
    client: &Client,
    route: &str,
    tickets: &TicketStore,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for suffix in ["release-health", "release-health/queue"] {
        let route = route.replace("provider-catalog/refresh", suffix);
        assert_eq!(
            client.get(&route).body("invalid").send().await?.status(),
            StatusCode::UNAUTHORIZED
        );
        let token = tickets
            .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
            .await?
            .ticket;
        let response = client.get(&route).bearer_auth(token).send().await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        let value: serde_json::Value = response.json().await?;
        if suffix.ends_with("queue") {
            assert!(value.is_null());
        } else {
            assert_eq!(value.as_array().map(Vec::len), Some(6));
        }
    }
    std::fs::create_dir(root.join("release_health"))?;
    std::fs::write(root.join("release_health/latest.json"), b"corrupt")?;
    let response = client
        .get(route.replace("provider-catalog/refresh", "release-health/queue"))
        .bearer_auth(
            tickets
                .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
                .await?
                .ticket,
        )
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    Ok(())
}

async fn assert_restart_authority(
    client: &Client,
    route: &str,
    tickets: &TicketStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let route = route.replace("provider-catalog/refresh", "runtime/rolling-restart");
    assert_eq!(
        client.post(&route).body("invalid").send().await?.status(),
        StatusCode::UNAUTHORIZED
    );
    let token = tickets
        .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
        .await?
        .ticket;
    let response = client.get(&route).bearer_auth(&token).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let status: agentsassemble_domain::RuntimeRestartStatus = response.json().await?;
    assert!(!status.supported);
    assert!(status.operation.is_none());
    assert_eq!(
        client.get(&route).bearer_auth(token).send().await?.status(),
        StatusCode::UNAUTHORIZED
    );
    let token = tickets
        .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
        .await?
        .ticket;
    assert_eq!(
        client
            .post(&route)
            .bearer_auth(token)
            .json(&serde_json::json!({"operation_id": uuid::Uuid::new_v4()}))
            .send()
            .await?
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    Ok(())
}
