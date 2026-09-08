use std::collections::BTreeMap;

use agentsassemble_domain::ProviderControlOption;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::remote_https::fixed_catalog_client;

const MAX_CATALOG_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_CATALOG_MODELS: usize = 256;

pub(crate) use crate::remote_https::RemoteReadError as RemoteCatalogError;

pub(crate) async fn fetch_public_catalog(
    endpoint: &'static str,
    cancellation: &CancellationToken,
) -> Result<Value, RemoteCatalogError> {
    let client = fixed_catalog_client().map_err(|_| RemoteCatalogError::Failed)?;
    crate::remote_https::fetch_bounded_json(
        client.get(endpoint),
        MAX_CATALOG_RESPONSE_BYTES,
        cancellation,
    )
    .await
}

pub(crate) fn gateway_model_options(
    payload: &Value,
) -> Result<Vec<ProviderControlOption>, RemoteCatalogError> {
    let entries = payload
        .get("data")
        .and_then(Value::as_array)
        .ok_or(RemoteCatalogError::Malformed)?;
    let options = entries
        .iter()
        .filter_map(gateway_model_option)
        .collect::<Vec<_>>();
    bound_catalog_options(options)
}

pub(crate) fn bound_catalog_options(
    options: Vec<ProviderControlOption>,
) -> Result<Vec<ProviderControlOption>, RemoteCatalogError> {
    if options.len() > MAX_CATALOG_MODELS {
        return Err(RemoteCatalogError::TooLarge);
    }
    Ok(options)
}

pub(crate) async fn fetch_gateway_model_options(
    endpoint: &'static str,
    cancellation: &CancellationToken,
) -> Result<Vec<ProviderControlOption>, RemoteCatalogError> {
    let payload = fetch_public_catalog(endpoint, cancellation).await?;
    gateway_model_options(&payload)
}

fn gateway_model_option(entry: &Value) -> Option<ProviderControlOption> {
    let entry = entry.as_object()?;
    let model_id = bounded_catalog_text(entry.get("id")?, 128)?;
    let supported = text_set(entry.get("supported_parameters"))
        .into_iter()
        .chain(text_set(entry.get("supported_features")))
        .collect::<Vec<_>>();
    let providers = entry.get("providers").and_then(Value::as_array);
    let tool_providers = providers
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter(|provider| provider.get("tools").and_then(Value::as_bool) == Some(true))
        .collect::<Vec<_>>();
    let input_modalities = input_modalities(entry);
    if !supported.iter().any(|value| value == "tools") && tool_providers.is_empty() {
        return None;
    }
    if !input_modalities.iter().any(|value| value == "text") {
        return None;
    }

    let mut metadata = BTreeMap::from([(
        "vision".to_owned(),
        json!(
            input_modalities.iter().any(|value| value == "image")
                || tool_providers.iter().any(|provider| {
                    provider.get("vision").and_then(Value::as_bool) == Some(true)
                })
        ),
    )]);
    metadata.insert("derive_description".to_owned(), json!(true));
    if let Some(family) = catalog_model_family(&model_id) {
        metadata.insert("family".to_owned(), json!(family));
    }
    let context = positive_u64(entry.get("context_length"))
        .or_else(|| positive_u64(entry.get("context_window")));
    if let Some(context) = context {
        metadata.insert("context_length".to_owned(), json!(context));
    }
    if let Some(max_output) = positive_u64(entry.get("max_tokens"))
        .or_else(|| positive_u64(entry.get("max_output_tokens")))
    {
        metadata.insert("max_output_tokens".to_owned(), json!(max_output));
    }
    let reasoning = truthy(entry.get("reasoning"))
        || truthy(entry.get("reasoning_options"))
        || supported.iter().any(|value| {
            matches!(
                value.as_str(),
                "reasoning" | "reasoning_effort" | "include_reasoning"
            )
        })
        || tool_providers
            .iter()
            .any(|provider| provider.get("reasoning").and_then(Value::as_bool) == Some(true));
    metadata.insert("reasoning".to_owned(), json!(reasoning));
    let advertised_reasoning_efforts = tool_providers
        .iter()
        .flat_map(|provider| text_set(provider.get("reasoning_efforts")))
        .collect::<std::collections::BTreeSet<_>>();
    if !advertised_reasoning_efforts.is_empty() {
        let reasoning_efforts = std::iter::once(String::new())
            .chain(advertised_reasoning_efforts)
            .collect::<Vec<_>>();
        metadata.insert("relation_scope".to_owned(), json!("per_model"));
        metadata.insert("reasoning_efforts".to_owned(), json!(reasoning_efforts));
    }
    project_display_metadata(&mut metadata, entry);

    Some(ProviderControlOption {
        value: model_id.clone(),
        label: entry
            .get("name")
            .and_then(|value| bounded_catalog_text(value, 256))
            .unwrap_or(model_id),
        metadata,
    })
}

fn project_display_metadata(
    metadata: &mut BTreeMap<String, Value>,
    entry: &serde_json::Map<String, Value>,
) {
    if let Some(pricing) = entry.get("pricing").and_then(Value::as_object) {
        let input_price = pricing
            .get("prompt")
            .or_else(|| pricing.get("input"))
            .and_then(price_per_million);
        let output_price = pricing
            .get("completion")
            .or_else(|| pricing.get("output"))
            .and_then(price_per_million);
        if let Some(price) = input_price.as_ref() {
            metadata.insert("input_price_per_million".to_owned(), json!(price));
        }
        if let Some(price) = output_price.as_ref() {
            metadata.insert("output_price_per_million".to_owned(), json!(price));
        }
        metadata.insert(
            "pricing".to_owned(),
            json!(if pricing_is_free(pricing) {
                "free"
            } else {
                "paid"
            }),
        );
    } else if entry.get("free").and_then(Value::as_bool) == Some(true) {
        metadata.insert("pricing".to_owned(), json!("free"));
    }
}

fn input_modalities(entry: &serde_json::Map<String, Value>) -> Vec<String> {
    let declared = entry
        .get("input_modalities")
        .or_else(|| {
            entry
                .get("architecture")
                .and_then(|value| value.get("input_modalities"))
        })
        .or_else(|| entry.get("modalities").and_then(|value| value.get("input")));
    let values = text_set(declared);
    if values.is_empty() && declared.is_none() {
        vec!["text".to_owned()]
    } else {
        values
    }
}

fn text_set(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| bounded_catalog_text(value, 64))
        .collect()
}

pub(crate) fn bounded_catalog_text(value: &Value, max_bytes: usize) -> Option<String> {
    let value = value.as_str()?.trim();
    (!value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn positive_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64).filter(|value| *value > 0)
}

fn truthy(value: Option<&Value>) -> bool {
    value.is_some_and(|value| match value {
        Value::Null | Value::Bool(false) => false,
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
        Value::String(value) => !value.is_empty(),
        Value::Number(_) | Value::Bool(true) => true,
    })
}

pub(crate) fn catalog_model_family(model_id: &str) -> Option<String> {
    let (owner, _) = model_id.split_once('/')?;
    Some(
        owner
            .split('-')
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(chars).collect()
                })
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn pricing_is_free(pricing: &serde_json::Map<String, Value>) -> bool {
    let prompt = pricing.get("prompt").or_else(|| pricing.get("input"));
    let completion = pricing.get("completion").or_else(|| pricing.get("output"));
    [prompt, completion]
        .into_iter()
        .all(|value| value.is_some_and(numeric_zero))
}

fn numeric_zero(value: &Value) -> bool {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse::<f64>().ok())
        .is_some_and(|value| value == 0.0)
}

fn price_per_million(value: &Value) -> Option<String> {
    let value = value
        .as_f64()
        .or_else(|| value.as_str()?.parse::<f64>().ok())?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let rendered = format!("{:.12}", value * 1_000_000.0);
    Some(
        rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{RemoteCatalogError, gateway_model_options};

    #[test]
    fn gateway_projection_keeps_only_bounded_text_tool_models() {
        let options = gateway_model_options(&json!({
            "data": [
                {
                    "id": "zai-glm-4.7",
                    "name": "Z.ai GLM 4.7",
                    "input_modalities": ["text"],
                    "supported_features": ["tools", "reasoning"],
                    "context_length": 131_072,
                    "pricing": {"prompt": "0.00000099", "completion": "0.00000149"}
                },
                {
                    "id": "vision-model",
                    "input_modalities": ["text", "image"],
                    "providers": [{"tools": true, "reasoning_efforts": ["high", "low"]}]
                },
                {"id": "no-tools", "input_modalities": ["text"]},
                {"id": "no-text", "input_modalities": ["image"], "supported_features": ["tools"]}
            ]
        }))
        .unwrap_or_else(|error| panic!("project gateway catalog: {error:?}"));

        assert_eq!(
            options
                .iter()
                .map(|option| option.value.as_str())
                .collect::<Vec<_>>(),
            ["zai-glm-4.7", "vision-model"]
        );
        assert_eq!(options[0].metadata["context_length"], json!(131_072));
        assert_eq!(
            options[0].metadata["input_price_per_million"],
            json!("0.99")
        );
        assert_eq!(
            options[0].metadata["output_price_per_million"],
            json!("1.49")
        );
        assert!(!options[0].metadata.contains_key("description"));
        assert_eq!(options[0].metadata["derive_description"], json!(true));
        assert_eq!(options[1].metadata["vision"], json!(true));
        assert_eq!(
            options[1].metadata["reasoning_efforts"],
            json!(["", "high", "low"])
        );
        for key in ["selection_kind", "compatibility_evidence", "tools"] {
            assert!(!options[0].metadata.contains_key(key));
        }
        assert!(!options[0].metadata.contains_key("relation_scope"));
        assert_eq!(options[1].metadata["relation_scope"], json!("per_model"));
    }

    #[test]
    fn gateway_projection_rejects_an_unbounded_remote_inventory() {
        let payload = json!({"data": (0..257).map(|index| json!({
            "id": format!("model-{index}"),
            "supported_parameters": ["tools"]
        })).collect::<Vec<_>>()});
        assert_eq!(
            gateway_model_options(&payload),
            Err(RemoteCatalogError::TooLarge)
        );
    }

    #[test]
    fn gateway_projection_bounds_compatible_models_not_remote_noise() {
        let mut entries = (0..300)
            .map(|index| json!({"id": format!("incompatible-{index}")}))
            .collect::<Vec<_>>();
        entries.push(json!({
            "id": "compatible",
            "supported_parameters": ["tools"]
        }));

        let options = gateway_model_options(&json!({"data": entries}))
            .unwrap_or_else(|error| panic!("project gateway catalog: {error:?}"));
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].value, "compatible");
    }
}
