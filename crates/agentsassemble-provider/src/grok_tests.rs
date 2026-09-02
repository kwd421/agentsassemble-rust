use std::collections::BTreeSet;

use super::{parse_custom_model_ids, parse_models};

#[test]
fn native_catalog_excludes_custom_models_and_preserves_advertised_default() {
    let custom = parse_custom_model_ids(
        br#"
            [model.custom-private]
            name = "private"
        "#,
    )
    .unwrap_or_else(|_| panic!("parse custom model keys"));
    let catalog = parse_models(
        "Default model: grok-4.6\n* grok-4.6\n- custom-private\n- grok-4.5\n",
        &custom,
    )
    .unwrap_or_else(|| panic!("parse native model catalog"));
    assert_eq!(catalog.models, ["grok-4.6", "grok-4.5"]);
    assert_eq!(catalog.default_model, "grok-4.6");
}

#[test]
fn catalog_and_custom_model_inputs_fail_closed() {
    assert!(
        parse_models(
            "Default model: custom\n- custom\n",
            &BTreeSet::from(["custom".to_owned()])
        )
        .is_none()
    );
    assert!(parse_custom_model_ids(b"model = [\"not-a-table\"]").is_err());
    assert!(parse_custom_model_ids(b"[model.'bad/model']\nname = 'bad'").is_err());
}
