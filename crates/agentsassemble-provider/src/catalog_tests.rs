use agentsassemble_domain::{ProviderAvailability, ProviderCatalog, ProviderControlOption};
use tokio_util::sync::CancellationToken;

use super::{
    FilesystemFailure, MAX_PROVIDER_OPTIONS, ProbeFailure, await_filesystem, failed_provider,
    opencode_models, ready_provider,
};

#[tokio::test]
async fn cancelled_catalog_does_not_wait_for_stalled_filesystem_work() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let stalled = std::future::pending::<Result<(), FilesystemFailure>>();
    assert_eq!(
        await_filesystem(&cancellation, stalled).await,
        Err(ProbeFailure::Cancelled)
    );
}

#[test]
fn opencode_catalog_accepts_only_managed_valid_namespaces() {
    let models = opencode_models(
        "openai/gpt-5\nopencode/hy3-free\nopencode-go/free/model\nopencode/HY3-free\n\
         opencode/bad model\nopencode/bad?model\nopencode/hy3-free\n",
    );
    let values = models
        .iter()
        .map(|model| model.value.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        [
            "opencode-go/free/model",
            "opencode/HY3-free",
            "opencode/hy3-free"
        ]
    );
}

#[test]
fn failed_and_oversized_catalogs_cannot_remain_startable() {
    let provider = fixture_provider();
    let failed = failed_provider(provider.clone(), ProbeFailure::CatalogTooLarge);
    assert!(!failed.startable);
    assert!(failed.default_model.is_empty());
    assert!(failed.controls.is_empty());

    let mut controls = Vec::new();
    controls.push(super::control(
        "model",
        "Model",
        "select",
        (0..=MAX_PROVIDER_OPTIONS)
            .map(|index| super::option(&format!("opencode/model-{index}"), "Model"))
            .collect(),
        "opencode/model-0",
    ));
    let bounded = ready_provider(provider, "opencode/model-0".to_owned(), controls);
    assert!(!bounded.startable);
    assert_eq!(bounded.discovery_error_code, "model_catalog_too_large");
}

#[test]
fn inconsistent_default_relation_cannot_be_startable() {
    let provider = fixture_provider();
    let controls = vec![
        super::control(
            "model",
            "Model",
            "select",
            vec![ProviderControlOption {
                value: "model-high".to_owned(),
                label: "Model high".to_owned(),
                metadata: std::collections::BTreeMap::from([(
                    "reasoning_efforts".to_owned(),
                    serde_json::json!(["high"]),
                )]),
            }],
            "model-high",
        ),
        super::control(
            "reasoning_effort",
            "Effort",
            "select",
            vec![super::option("low", "Low"), super::option("high", "High")],
            "low",
        ),
    ];
    let inconsistent = ready_provider(provider, "model-high".to_owned(), controls);
    assert!(!inconsistent.startable);
    assert_eq!(
        inconsistent.discovery_error_code,
        "model_discovery_malformed"
    );
}

#[test]
fn duplicate_option_authority_cannot_be_startable() {
    let provider = fixture_provider();
    let controls = vec![super::control(
        "model",
        "Model",
        "select",
        vec![
            super::option("duplicate", "First"),
            super::option("duplicate", "Second"),
        ],
        "duplicate",
    )];
    let inconsistent = ready_provider(provider, "duplicate".to_owned(), controls);
    assert!(!inconsistent.startable);
    assert_eq!(
        inconsistent.discovery_error_code,
        "model_discovery_malformed"
    );
}

#[test]
fn fixed_catalogs_are_bounded_before_publication() {
    let mut provider = fixture_provider();
    provider.startable = true;
    provider.controls.push(super::control(
        "model",
        "Model",
        "select",
        (0..MAX_PROVIDER_OPTIONS)
            .map(|index| {
                super::option(
                    &format!("opencode/model-{index}"),
                    &"x".repeat(super::MAX_OPTION_LABEL_BYTES),
                )
            })
            .collect(),
        "opencode/model-0",
    ));
    let snapshot = super::ProviderCatalogService::fixed(ProviderCatalog {
        status: "ready".to_owned(),
        catalog_revision: "oversized".to_owned(),
        discovered_at: String::new(),
        providers: vec![provider],
    })
    .snapshot();
    assert_eq!(snapshot.status, "failed");
    assert!(snapshot.catalog_revision.is_empty());
    assert!(snapshot.providers.is_empty());
}

fn fixture_provider() -> ProviderAvailability {
    super::loading_provider(
        "opencode",
        "OpenCode",
        "opencode_server",
        "opencode",
        "/bin/true",
    )
}
