use std::collections::BTreeSet;

use agentsassemble_domain::{ProviderAvailability, ProviderControlOption};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStore,
    catalog::{
        control, failed_provider, option, permission_control, preferred_model, ready_provider,
        remote_output_token_control, unavailable_provider,
    },
    process::ProbeFailure,
    remote_catalog::{RemoteCatalogError, bound_catalog_options, bounded_catalog_text},
    remote_https::{fetch_bounded_json, fixed_catalog_client},
};

// The endpoint publishes available IDs. It does not publish prices or model limits.
// Thinking controls are the provider's documented Chat Completions dialect.
const ENDPOINT: &str = "https://api.deepseek.com/models";
const EFFORTS: [&str; 3] = ["low", "high", "max"];

pub(crate) async fn discover(
    provider: ProviderAvailability,
    credentials: &ProviderCredentialStore,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    if cancellation.is_cancelled() {
        return failed_provider(provider, ProbeFailure::Cancelled);
    }
    let secret = match credentials.secret(ProviderCredentialId::DeepSeek).await {
        Ok(secret) => secret,
        Err(ProviderCredentialError::MissingSecret | ProviderCredentialError::InvalidSecret) => {
            return failed_provider(provider, ProbeFailure::Authentication);
        }
        Err(ProviderCredentialError::SecureStoreUnavailable) => {
            return unavailable_provider(
                provider,
                true,
                "secure_store_unavailable",
                "DeepSeek model discovery cannot access the OS credential store.",
            );
        }
    };
    let Ok(client) = fixed_catalog_client() else {
        return failed_provider(provider, ProbeFailure::Failed);
    };
    let result = fetch_bounded_json(
        client.get(ENDPOINT).bearer_auth(secret.expose()),
        128 * 1024,
        cancellation,
    )
    .await
    .and_then(model_options);
    match result {
        Ok(models) if !models.is_empty() => ready(provider, models),
        Ok(_) => unavailable_provider(
            provider,
            true,
            "no_supported_models",
            "DeepSeek returned no available models.",
        ),
        Err(error) => failed_provider(provider, super::catalog::remote_catalog_failure(error)),
    }
}

fn ready(
    provider: ProviderAvailability,
    models: Vec<ProviderControlOption>,
) -> ProviderAvailability {
    let default_model = preferred_model(&models, "deepseek-v4-flash");
    ready_provider(
        provider,
        default_model.clone(),
        vec![
            control("model", "모델", "combobox", models, &default_model),
            control(
                "reasoning_effort",
                "추론 강도",
                "select",
                EFFORTS
                    .into_iter()
                    .map(|effort| option(effort, effort))
                    .collect(),
                "high",
            ),
            control(
                "variant",
                "Thinking",
                "select",
                vec![
                    option("thinking", "사용"),
                    option("non_thinking", "사용 안 함"),
                ],
                "thinking",
            ),
            remote_output_token_control(),
            permission_control(false),
        ],
    )
}

#[derive(Deserialize)]
struct ModelList {
    object: String,
    data: Vec<Model>,
}

#[derive(Deserialize)]
struct Model {
    id: String,
    object: String,
    owned_by: String,
}

fn model_options(payload: Value) -> Result<Vec<ProviderControlOption>, RemoteCatalogError> {
    let list: ModelList =
        serde_json::from_value(payload).map_err(|_| RemoteCatalogError::Malformed)?;
    if list.object != "list" {
        return Err(RemoteCatalogError::Malformed);
    }
    let mut seen = BTreeSet::new();
    let mut options = Vec::new();
    for model in list.data {
        let id = bounded_catalog_text(&json!(model.id), 128)
            .filter(|id| id == &model.id)
            .ok_or(RemoteCatalogError::Malformed)?;
        if model.object != "model" || model.owned_by != "deepseek" || !seen.insert(id.clone()) {
            return Err(RemoteCatalogError::Malformed);
        }
        let mut model = option(&id, &id);
        model
            .metadata
            .insert("relation_scope".to_owned(), json!("global"));
        model
            .metadata
            .insert("reasoning_efforts".to_owned(), json!(EFFORTS));
        options.push(model);
    }
    bound_catalog_options(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovered_new_ids_replace_old_ids_without_invented_metadata() {
        let payload = |ids: &[&str]| {
            json!({"object":"list", "data":ids.iter().map(|id|
            json!({"id":id,"object":"model","owned_by":"deepseek"})
        ).collect::<Vec<_>>()})
        };
        let current = model_options(payload(&["deepseek-v4-flash"]))
            .unwrap_or_else(|error| panic!("{error:?}"));
        let updated = model_options(payload(&["new-official-model-id"]))
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_ne!(current, updated);
        assert_eq!(updated[0].value, "new-official-model-id");
        assert_eq!(
            updated[0].metadata["reasoning_efforts"],
            json!(["low", "high", "max"])
        );
        for key in [
            "context_length",
            "max_output_tokens",
            "input_price_per_million",
            "output_price_per_million",
        ] {
            assert!(!updated[0].metadata.contains_key(key));
        }
        assert!(model_options(payload(&["duplicate", "duplicate"])).is_err());
        assert!(model_options(payload(&["bad\nidentifier"])).is_err());
        assert!(model_options(json!({"data":[]})).is_err());
    }

    #[tokio::test]
    async fn new_model_selection_uses_the_current_catalog_without_a_name_allowlist() {
        let models = model_options(json!({"object":"list", "data":[{
            "id":"new-official-model-id", "object":"model", "owned_by":"deepseek"
        }]}))
        .unwrap_or_else(|error| panic!("{error:?}"));
        let provider = ready(
            crate::registration::loading_provider(&crate::registration::DEEPSEEK_PROVIDER),
            models,
        );
        let catalog =
            crate::ProviderCatalogService::fixed(agentsassemble_domain::ProviderCatalog {
                status: "ready".to_owned(),
                catalog_revision: "fresh".to_owned(),
                discovered_at: chrono::Utc::now().to_rfc3339(),
                providers: vec![provider],
            });
        let mut input = json!({
            "provider_id":"deepseek", "catalog_revision":"fresh", "display_name":"Agent", "workspace":"",
            "model":"new-official-model-id", "reasoning_effort":"low", "variant":"thinking",
            "permission_mode":"meeting_read_only", "max_output_tokens":4096
        });
        let selection = catalog
            .validate_creation("room", "operator", "create", &input)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(selection.model, "new-official-model-id");
        assert_eq!(selection.reasoning_effort, "low");
        input["model"] = json!("deepseek-v4-flash");
        assert!(
            catalog
                .validate_creation("room", "operator", "create", &input)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn missing_key_and_cancellation_do_not_publish_static_models() {
        let credentials = ProviderCredentialStore::isolated_test_store();
        let provider =
            || crate::registration::loading_provider(&crate::registration::DEEPSEEK_PROVIDER);
        let cancellation = CancellationToken::new();
        let missing = discover(provider(), &credentials, &cancellation).await;
        assert!(!missing.startable);
        assert!(missing.controls.is_empty());
        assert_eq!(missing.discovery_error_code, "authentication_required");
        cancellation.cancel();
        let cancelled = discover(provider(), &credentials, &cancellation).await;
        assert_eq!(cancelled.discovery_error_code, "model_discovery_cancelled");
    }
}
