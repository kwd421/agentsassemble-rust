use agentsassemble_domain::DurableAgentSession;
use serde_json::{Map, Value, json};

use crate::{ProviderCatalogService, ProviderSelection, ProviderSelectionError};

const CONFIGURE_FIELDS: &[&str] = &[
    "agent_id",
    "catalog_revision",
    "provider_id",
    "provider_kind",
    "workspace",
    "model",
    "reasoning_effort",
    "service_tier",
    "variant",
    "execution_harness",
    "permission_mode",
    "max_output_tokens",
    "persona_card_id",
];

impl ProviderCatalogService {
    /// Revalidates a stopped session's complete runtime profile against one catalog snapshot.
    ///
    /// # Errors
    ///
    /// Returns a stable selection failure without allowing the client to replace provider or
    /// runtime identity.
    pub async fn validate_configuration(
        &self,
        room_id: &str,
        principal_id: &str,
        request_id: &str,
        current: &DurableAgentSession,
        payload: &Value,
    ) -> Result<ProviderSelection, ProviderSelectionError> {
        let values = payload
            .as_object()
            .ok_or_else(|| bad_request("payload must be an object."))?;
        if let Some(key) = values
            .keys()
            .find(|key| !CONFIGURE_FIELDS.contains(&key.as_str()))
        {
            return Err(bad_request(format!(
                "Unknown agent.configure runtime field: {key}."
            )));
        }
        let agent_id = bounded_string(values, "agent_id", 128)?
            .ok_or_else(|| bad_request("agent_id is required."))?;
        if agent_id != current.public.session_id {
            return Err(bad_request("agent_id does not match the selected session."));
        }

        let catalog = self.snapshot();
        let provider = provider_for_current(&catalog.providers, current)?;
        for key in ["provider_id", "provider_kind"] {
            if let Some(requested) = bounded_string(values, key, 64)?
                && requested != provider.id
                && requested != provider.provider_kind
            {
                return Err(ProviderSelectionError::new(
                    "provider_mismatch",
                    "An existing Agent Session cannot change provider kind; remove it and create a new session.",
                ));
            }
        }

        let merged = merged_configuration(values, current, &provider.id)?;

        let mut selected = ProviderSelection::from_catalog(
            room_id,
            principal_id,
            request_id,
            &Value::Object(merged),
            &catalog,
        )
        .await?;
        if selected.provider_kind != current.public.provider_kind
            || selected.runtime_kind != current.public.runtime_kind
        {
            return Err(ProviderSelectionError::new(
                "provider_mismatch",
                "The selected provider no longer matches this Agent Session.",
            ));
        }
        selected.agent_id.clone_from(&current.public.session_id);
        Ok(selected)
    }
}

fn provider_for_current<'a>(
    providers: &'a [agentsassemble_domain::ProviderAvailability],
    current: &DurableAgentSession,
) -> Result<&'a agentsassemble_domain::ProviderAvailability, ProviderSelectionError> {
    let mut matches = providers
        .iter()
        .filter(|provider| provider.provider_kind == current.public.provider_kind);
    let provider = matches.next().ok_or_else(|| {
        ProviderSelectionError::new(
            "unsupported_provider",
            "The stored provider is not available in the current catalog.",
        )
    })?;
    if matches.next().is_some() {
        return Err(ProviderSelectionError::new(
            "catalog_inconsistent",
            "The catalog has ambiguous provider authority for this Agent Session.",
        ));
    }
    Ok(provider)
}

fn merged_configuration(
    values: &Map<String, Value>,
    current: &DurableAgentSession,
    provider_id: &str,
) -> Result<Map<String, Value>, ProviderSelectionError> {
    let mut merged = Map::new();
    merged.insert(
        "catalog_revision".to_owned(),
        values
            .get("catalog_revision")
            .cloned()
            .ok_or_else(|| bad_request("catalog_revision is required."))?,
    );
    merged.insert("provider_id".to_owned(), json!(provider_id));
    merged.insert(
        "display_name".to_owned(),
        json!(current.public.display_name),
    );
    merged.insert(
        "workspace".to_owned(),
        values
            .get("workspace")
            .cloned()
            .unwrap_or_else(|| json!(current.workspace)),
    );
    for (key, value) in [
        ("model", current.public.model.as_str()),
        ("reasoning_effort", current.public.reasoning_effort.as_str()),
        ("service_tier", current.public.service_tier.as_str()),
        ("variant", current.public.variant.as_str()),
        (
            "execution_harness",
            current.public.execution_harness.as_str(),
        ),
        ("permission_mode", current.public.permission_mode.as_str()),
        ("persona_card_id", current.public.persona_card_id.as_ref()),
    ] {
        copy_or_current(&mut merged, values, key, value);
    }
    if let Some(value) = normalized_max_output_tokens(values.get("max_output_tokens"))? {
        merged.insert("max_output_tokens".to_owned(), value);
    }
    Ok(merged)
}

fn copy_or_current(
    merged: &mut Map<String, Value>,
    values: &Map<String, Value>,
    key: &str,
    current: &str,
) {
    merged.insert(
        key.to_owned(),
        values.get(key).cloned().unwrap_or_else(|| json!(current)),
    );
}

fn normalized_max_output_tokens(
    value: Option<&Value>,
) -> Result<Option<Value>, ProviderSelectionError> {
    match value {
        None => Ok(None),
        Some(Value::String(value)) if value.is_empty() => Ok(None),
        Some(Value::String(value)) => value
            .parse::<u64>()
            .map(|value| Some(json!(value)))
            .map_err(|_| bad_request("max_output_tokens must be a nonnegative integer.")),
        Some(Value::Number(value)) if value.as_u64().is_some() => {
            Ok(Some(Value::Number(value.clone())))
        }
        Some(_) => Err(bad_request(
            "max_output_tokens must be a nonnegative integer.",
        )),
    }
}

fn bounded_string(
    values: &Map<String, Value>,
    key: &str,
    limit: usize,
) -> Result<Option<String>, ProviderSelectionError> {
    values
        .get(key)
        .map(|value| {
            let value = value
                .as_str()
                .ok_or_else(|| bad_request(format!("{key} must be a string.")))?;
            if value.len() > limit || value.trim() != value || value.chars().any(char::is_control) {
                return Err(bad_request(format!("{key} is invalid.")));
            }
            Ok(value.to_owned())
        })
        .transpose()
}

fn bad_request(message: impl Into<String>) -> ProviderSelectionError {
    ProviderSelectionError::new("bad_request", message)
}
