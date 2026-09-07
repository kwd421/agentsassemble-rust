use serde_json::Value;

#[test]
fn catalog_preserves_exact_per_model_relations() {
    let catalog = super::parse_catalog(
        r#"{"type":"catalog","models":[{"id":"claude-haiku-4-5","label":"Haiku","efforts":["low","medium"],"fast":false},{"id":"claude-opus-5","label":"Opus","efforts":["high","xhigh"],"fast":true}]}"#,
    )
    .unwrap_or_else(|| panic!("parse Claude catalog"));
    assert_eq!(catalog.default_model, "claude-haiku-4-5");
    assert_eq!(catalog.efforts, ["low", "medium", "high", "xhigh"]);
    assert_eq!(
        catalog.models[1].metadata.get("service_tiers"),
        Some(&Value::from(vec!["default", "fast"]))
    );
    assert!(!super::valid_model_id("alias-claude-opus-5"));
    assert!(!super::valid_model_id("claude-opus-5-1-2"));
}

#[tokio::test]
async fn catalog_without_fast_models_remains_selectable() {
    let catalog = super::parse_catalog(
        r#"{"type":"catalog","models":[{"id":"claude-haiku-4-5","label":"Haiku","efforts":["low","medium"],"fast":false}]}"#,
    )
    .unwrap_or_else(|| panic!("parse default-only Claude catalog"));
    crate::test_support::assert_default_tier_selection(
        "claude",
        catalog.default_model.clone(),
        catalog.controls(),
    )
    .await;
}
