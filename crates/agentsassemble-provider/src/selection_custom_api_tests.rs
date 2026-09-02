use agentsassemble_domain::ProviderCatalog;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::ProviderSelection;
use crate::{
    catalog::discover_custom_api,
    registration::{CUSTOM_API_PROVIDER, loading_provider},
};

async fn custom_catalog() -> ProviderCatalog {
    let provider = discover_custom_api(
        loading_provider(&CUSTOM_API_PROVIDER),
        &CancellationToken::new(),
    )
    .await;
    ProviderCatalog {
        status: "ready".to_owned(),
        catalog_revision: "catalog-custom".to_owned(),
        discovered_at: String::new(),
        providers: vec![provider],
    }
}

fn request(endpoint: &str, model: &str) -> serde_json::Value {
    json!({
        "provider_id": "custom_api",
        "catalog_revision": "catalog-custom",
        "display_name": "Custom vendor",
        "workspace": "",
        "provider_endpoint": endpoint,
        "model": model,
        "permission_mode": "meeting_read_only",
        "max_output_tokens": 4096
    })
}

#[tokio::test]
async fn selection_normalizes_private_custom_authority_into_session_identity() {
    let catalog = custom_catalog().await;
    let selected = ProviderSelection::from_catalog(
        "general",
        "operator-local-user",
        "create-custom",
        &request(
            "https://api.example.com/v1/chat/completions",
            "vendor-model",
        ),
        &catalog,
    )
    .await
    .unwrap_or_else(|error| panic!("select Custom API: {error}"));

    assert_eq!(selected.provider_endpoint, "https://api.example.com/v1");
    assert_eq!(selected.model, "vendor-model");
    assert_eq!(selected.permission_mode, "meeting_read_only");
    assert!(selected.workspace.is_empty());
    assert!(!selected.runtime_profile_key.is_empty());

    let changed = ProviderSelection::from_catalog(
        "general",
        "operator-local-user",
        "create-custom",
        &request("https://other.example.com/v1", "vendor-model"),
        &catalog,
    )
    .await
    .unwrap_or_else(|error| panic!("select changed Custom API: {error}"));
    assert_ne!(selected.runtime_profile_key, changed.runtime_profile_key);
}

#[tokio::test]
async fn selection_rejects_missing_model_and_private_network_authority() {
    let catalog = custom_catalog().await;
    let Err(missing_model) = ProviderSelection::from_catalog(
        "general",
        "operator-local-user",
        "missing-model",
        &request("https://api.example.com/v1", ""),
        &catalog,
    )
    .await
    else {
        panic!("missing model must fail");
    };
    assert_eq!(missing_model.code, "unsupported_model");

    let Err(private_endpoint) = ProviderSelection::from_catalog(
        "general",
        "operator-local-user",
        "private-endpoint",
        &request("https://127.0.0.1/v1", "vendor-model"),
        &catalog,
    )
    .await
    else {
        panic!("private endpoint must fail");
    };
    assert_eq!(private_endpoint.code, "invalid_provider_endpoint");
}
