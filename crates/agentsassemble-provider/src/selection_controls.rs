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
        let advertises_nonempty_values = provider
            .controls
            .iter()
            .find(|control| control.key == "model")
            .and_then(|control| control.options.iter().find(|option| option.value == model))
            .and_then(|option| option.metadata.get(relation))
            .and_then(Value::as_array)
            .is_some_and(|allowed| {
                allowed
                    .iter()
                    .any(|value| value.as_str().is_some_and(|value| !value.is_empty()))
            });
        return if advertises_nonempty_values {
            Err(ProviderSelectionError::new(
                "unsupported_control",
                format!(
                    "Provider {} model {model} does not support an empty {relation} value.",
                    provider.id
                ),
            ))
        } else {
            Ok(())
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
    if selected.is_empty() && allowed.is_empty() {
        return Ok(());
    }
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
