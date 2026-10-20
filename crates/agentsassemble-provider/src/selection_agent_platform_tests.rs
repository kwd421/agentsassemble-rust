use agentsassemble_domain::ProviderCatalog;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::ProviderSelection;
use crate::{
    ProviderCredentialId, ProviderCredentialStore, agent_platform,
    registration::{AGENT_PLATFORM_PROVIDER, loading_provider},
};

async fn agent_platform_catalog() -> ProviderCatalog {
    let credentials = ProviderCredentialStore::isolated_test_store();
    credentials
        .set(
            ProviderCredentialId::AgentPlatform,
            "agent-platform-secret-sentinel",
        )
        .await
        .unwrap_or_else(|error| panic!("store Agent Platform credential: {error}"));
    let provider = agent_platform::discover(
        loading_provider(&AGENT_PLATFORM_PROVIDER),
        &credentials,
        &CancellationToken::new(),
    )
    .await;
    ProviderCatalog {
        status: "ready".to_owned(),
        catalog_revision: "catalog-agent-platform".to_owned(),
        discovered_at: String::new(),
        providers: vec![provider],
    }
}

fn request(settings: &str) -> serde_json::Value {
    json!({
        "provider_id": "agent_platform",
        "catalog_revision": "catalog-agent-platform",
        "display_name": "Agent Platform",
        "workspace": "",
        "provider_endpoint": settings,
        "model": "google/gemini-2.5-flash-lite",
        "permission_mode": "meeting_read_only",
        "max_output_tokens": 4096
    })
}

#[tokio::test]
async fn selection_keeps_validated_project_and_location_settings() {
    let catalog = agent_platform_catalog().await;
    let selected = ProviderSelection::from_catalog(
        "general",
        "operator-local-user",
        "create-agent-platform",
        &request("projects/my-project-123/locations/global"),
        &catalog,
    )
    .await
    .unwrap_or_else(|error| panic!("select Agent Platform: {error}"));

    assert_eq!(
        selected.provider_endpoint,
        "projects/my-project-123/locations/global"
    );
    assert_eq!(selected.model, "google/gemini-2.5-flash-lite");
    assert!(selected.workspace.is_empty());
}

#[tokio::test]
async fn selection_refuses_invalid_project_and_location_before_runtime() {
    let catalog = agent_platform_catalog().await;
    for settings in [
        "https://aiplatform.googleapis.com/v1/projects/my-project/locations/global",
        "projects/Bad_Project/locations/global",
        "projects/my-project/locations/us",
        "",
    ] {
        let error = ProviderSelection::from_catalog(
            "general",
            "operator-local-user",
            "invalid-agent-platform",
            &request(settings),
            &catalog,
        )
        .await
        .err()
        .unwrap_or_else(|| panic!("accepted invalid Agent Platform settings {settings}"));
        assert_eq!(error.code, "invalid_provider_endpoint");
        assert!(!error.message.contains("agent-platform-secret-sentinel"));
    }
}
