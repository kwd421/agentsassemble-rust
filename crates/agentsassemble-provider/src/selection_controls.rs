use agentsassemble_domain::ProviderAvailability;
use serde_json::Value;

use super::ProviderSelectionError;

pub(super) fn selected_value(
    provider: &ProviderAvailability,
    key: &str,
    requested: Option<String>,
) -> Result<String, ProviderSelectionError> {
    let Some(control) = provider.controls.iter().find(|control| control.key == key) else {
        return if requested.as_deref().is_none_or(str::is_empty) {
            Ok(String::new())
        } else {
            Err(ProviderSelectionError::new(
                "unsupported_control",
                format!("Provider {} does not support {key}.", provider.id),
            ))
        };
    };
    let selected = requested.unwrap_or_else(|| control.default_value.clone());
    control
        .options
        .iter()
        .any(|option| option.value == selected)
        .then_some(selected)
        .ok_or_else(|| {
            ProviderSelectionError::new(
                "unsupported_control",
                format!("Provider {} rejected the selected {key}.", provider.id),
            )
        })
}

pub(super) fn selected_u32(
    provider: &ProviderAvailability,
    key: &str,
    requested: Option<u64>,
) -> Result<u32, ProviderSelectionError> {
    let requested = requested
        .filter(|value| *value != 0)
        .map(|value| value.to_string());
    let selected = selected_value(provider, key, requested)?;
    if selected.is_empty() {
        return Ok(0);
    }
    selected.parse::<u32>().map_err(|_| {
        ProviderSelectionError::new(
            "catalog_inconsistent",
            format!("Provider {} has an invalid {key} authority.", provider.id),
        )
    })
}

pub(super) fn validate_model_relation(
    provider: &ProviderAvailability,
    model: &str,
    relation: &str,
    selected: &str,
) -> Result<(), ProviderSelectionError> {
    if selected == "default" {
        return Ok(());
    }

    if selected.is_empty() {
        let allowed = provider
            .controls
            .iter()
            .find(|control| control.key == "model")
            .and_then(|control| control.options.iter().find(|option| option.value == model))
            .and_then(|option| option.metadata.get(relation))
            .and_then(Value::as_array);
        return match allowed {
            Some(allowed) if allowed.iter().any(|value| value.as_str() == Some("")) => Ok(()),
            Some(allowed)
                if allowed
                    .iter()
                    .any(|value| value.as_str().is_some_and(|value| !value.is_empty())) =>
            {
                Err(ProviderSelectionError::new(
                    "unsupported_control",
                    format!(
                        "Provider {} model {model} does not support an empty {relation} value.",
                        provider.id
                    ),
                ))
            }
            _ => Ok(()),
        };
    }

    let model_option = provider
        .controls
        .iter()
        .find(|control| control.key == "model")
        .and_then(|control| control.options.iter().find(|option| option.value == model));
    let Some(model_option) = model_option else {
        return Err(ProviderSelectionError::new(
            "catalog_inconsistent",
            format!("Provider {} has no selected model authority.", provider.id),
        ));
    };
    let relation_scope = model_option
        .metadata
        .get("relation_scope")
        .and_then(Value::as_str);
    let Some(Value::Array(allowed)) = model_option.metadata.get(relation) else {
        if relation_scope == Some("per_model") {
            return Err(ProviderSelectionError::new(
                "catalog_inconsistent",
                format!(
                    "Provider {} has incomplete per-model controls.",
                    provider.id
                ),
            ));
        }
        return Ok(());
    };
    if allowed.iter().any(|value| value.as_str() == Some(selected)) {
        return Ok(());
    }
    Err(ProviderSelectionError::new(
        "unsupported_control",
        format!(
            "Provider {} model {model} does not support {selected}.",
            provider.id
        ),
    ))
}

pub(super) fn validate_runtime_variant(
    provider: &ProviderAvailability,
    model: &str,
    reasoning_effort: &str,
    service_tier: &str,
) -> Result<(), ProviderSelectionError> {
    let Some(model_control) = provider
        .controls
        .iter()
        .find(|control| control.key == "model")
    else {
        return Ok(());
    };
    let Some(model_option) = model_control
        .options
        .iter()
        .find(|option| option.value == model)
    else {
        return Err(ProviderSelectionError::new(
            "catalog_inconsistent",
            format!("Provider {} has no selected model authority.", provider.id),
        ));
    };
    let Some(variants) = model_option.metadata.get("runtime_variants") else {
        return Ok(());
    };
    let Some(variants) = variants.as_array() else {
        return Err(ProviderSelectionError::new(
            "catalog_inconsistent",
            format!("Provider {} has malformed runtime variants.", provider.id),
        ));
    };
    let matches = variants.iter().any(|variant| {
        variant.as_object().is_some_and(|variant| {
            variant.get("reasoning_effort").and_then(Value::as_str) == Some(reasoning_effort)
                && variant.get("service_tier").and_then(Value::as_str) == Some(service_tier)
                && variant.len() == 2
        })
    });
    if matches {
        return Ok(());
    }
    Err(ProviderSelectionError::new(
        "unsupported_control",
        format!(
            "Provider {} model {model} does not support the selected runtime variant.",
            provider.id
        ),
    ))
}

/// The same catalog selection owner validates both create and stopped configuration.
pub(super) fn validate_model_permission(
    provider: &ProviderAvailability,
    model: &str,
    permission_mode: &str,
) -> Result<(), ProviderSelectionError> {
    let free = provider
        .controls
        .iter()
        .find(|control| control.key == "model")
        .and_then(|control| control.options.iter().find(|option| option.value == model))
        .and_then(|option| option.metadata.get("pricing"))
        .and_then(Value::as_str)
        .is_some_and(|pricing| matches!(pricing, "free" | "free_tier"));
    if provider.id == "opencode" && free && permission_mode == "meeting_read_only" {
        return Err(ProviderSelectionError::new(
            "opencode_free_requires_workspace_write",
            "OpenCode 무료 모델은 작업 폴더 쓰기나 전체 액세스 권한을 선택해야 사용할 수 있어요.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_access_requires_catalog_support() {
        let mut provider =
            crate::registration::loading_provider(&crate::registration::OPENCODE_PROVIDER);
        for supported in [false, true] {
            provider.controls = vec![crate::catalog::permission_control(true, supported)];
            assert_eq!(
                selected_value(&provider, "permission_mode", Some("full_access".into())).is_ok(),
                supported
            );
        }
    }

    #[test]
    fn free_opencode_permission_is_rejected_without_changing_selection() {
        let mut provider =
            crate::registration::loading_provider(&crate::registration::OPENCODE_PROVIDER);
        let mut model = crate::catalog::option("opencode/no-suffix", "Free");
        model
            .metadata
            .insert("pricing".to_owned(), serde_json::json!("free"));
        provider.controls = vec![crate::catalog::control(
            "model",
            "Model",
            "select",
            vec![model],
            "",
        )];
        let error = validate_model_permission(&provider, "opencode/no-suffix", "meeting_read_only")
            .err()
            .unwrap_or_else(|| panic!("free model cannot use conversation-only"));
        assert_eq!(error.code, "opencode_free_requires_workspace_write");
        assert!(error.message.contains("작업 폴더 쓰기"));
        assert!(
            validate_model_permission(&provider, "opencode/no-suffix", "workspace_write").is_ok()
        );
        assert!(validate_model_permission(&provider, "opencode/no-suffix", "full_access").is_ok());
        provider.controls[0].options[0]
            .metadata
            .insert("pricing".to_owned(), serde_json::json!("paid"));
        assert!(
            validate_model_permission(&provider, "opencode/no-suffix", "meeting_read_only").is_ok()
        );
        provider.id = "other".to_owned();
        provider.controls[0].options[0]
            .metadata
            .insert("pricing".to_owned(), serde_json::json!("free"));
        assert!(
            validate_model_permission(&provider, "opencode/no-suffix", "meeting_read_only").is_ok()
        );
    }
}
