use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
};

use agentsassemble_domain::{ProviderAvailability, ProviderControl, ProviderControlOption};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::{
    cerebras, custom_api,
    filesystem::{FilesystemFailure, resolve_codex_executable, resolve_executable},
    llm_gateway, openrouter,
    process::{ProbeFailure, probe},
    remote_catalog::{RemoteCatalogError, fetch_gateway_model_options, fetch_public_catalog},
    tokenrouter, vercel,
};

// The current Vercel and LLM Gateway tool-capable projections are about 75 KiB
// each. They remain beneath this authority and the outer WebSocket frame bound.
const MAX_PROVIDER_BYTES: usize = 128 * 1024;
pub(crate) const MAX_PROVIDER_OPTIONS: usize = 256;
pub(crate) const MAX_OPTION_VALUE_BYTES: usize = 128;
const MAX_OPTION_LABEL_BYTES: usize = 256;

pub(crate) async fn provider_executable(
    program: &str,
    cancellation: &CancellationToken,
) -> Result<(String, String), ProbeFailure> {
    let resolved = await_filesystem(cancellation, resolve_executable(program)).await?;
    match resolved {
        Some(authority) => Ok(authority),
        None => Err(ProbeFailure::Missing),
    }
}

pub(crate) async fn await_filesystem<T>(
    cancellation: &CancellationToken,
    operation: impl Future<Output = Result<T, FilesystemFailure>>,
) -> Result<T, ProbeFailure> {
    let resolved = tokio::select! {
        biased;
        () = cancellation.cancelled() => return Err(ProbeFailure::Cancelled),
        resolved = operation => resolved,
    };
    if cancellation.is_cancelled() {
        return Err(ProbeFailure::Cancelled);
    }
    match resolved {
        Ok(value) => Ok(value),
        Err(FilesystemFailure::Timeout) => Err(ProbeFailure::Timeout),
        Err(FilesystemFailure::Failed) => Err(ProbeFailure::Failed),
    }
}

pub(crate) async fn resolved_codex(
    cancellation: &CancellationToken,
) -> Result<(String, String), ProbeFailure> {
    await_filesystem(cancellation, resolve_codex_executable())
        .await?
        .ok_or(ProbeFailure::Missing)
}

pub(crate) async fn discover_codex(
    mut provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    let (executable, executable_identity) = match resolved_codex(cancellation).await {
        Ok(authority) => authority,
        Err(failure) => return failed_provider(provider, failure),
    };
    provider.executable.clone_from(&executable);
    provider.executable_identity = executable_identity;
    let Ok(home) = crate::codex::config::home() else {
        return failed_provider(provider, ProbeFailure::Failed);
    };
    let output = match Box::pin(probe(
        &executable,
        &["debug", "models"],
        cancellation,
        &[("CODEX_HOME".to_owned(), home)],
    ))
    .await
    {
        Ok(output) => output,
        Err(error) => return failed_provider(provider, error),
    };
    let Ok(payload) = serde_json::from_str::<CodexModels>(&output) else {
        return malformed_provider(provider);
    };
    let mut models = Vec::new();
    let mut efforts = Vec::new();
    let mut tiers = vec![option("default", "기본")];
    for model in payload.models {
        if model.slug.is_empty() {
            continue;
        }
        let model_efforts = model
            .supported_reasoning_levels
            .into_iter()
            .filter_map(|level| nonempty(level.effort))
            .collect::<Vec<_>>();
        for effort in &model_efforts {
            push_unique(&mut efforts, option(effort, &title_case(effort)));
        }
        let model_tiers = model
            .service_tiers
            .into_iter()
            .filter_map(|tier| nonempty(tier.id))
            .collect::<Vec<_>>();
        for tier in &model_tiers {
            push_unique(
                &mut tiers,
                option(tier, if tier == "priority" { "Fast" } else { tier }),
            );
        }
        let mut metadata = BTreeMap::new();
        metadata.insert("relation_scope".to_owned(), json!("per_model"));
        metadata.insert("reasoning_efforts".to_owned(), json!(model_efforts));
        metadata.insert("service_tiers".to_owned(), json!(model_tiers));
        models.push(ProviderControlOption {
            value: model.slug.clone(),
            label: nonempty(model.display_name).unwrap_or(model.slug),
            metadata,
        });
    }
    let default_model = preferred_model(&models, "gpt-5.6-luna");
    ready_provider(
        provider,
        default_model.clone(),
        vec![
            control("model", "모델", "combobox", models, &default_model),
            control("reasoning_effort", "추론 강도", "select", efforts, "low"),
            control("service_tier", "응답 속도", "select", tiers, "default"),
            permission_control(true),
        ],
    )
}

pub(crate) async fn discover_opencode(
    mut provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    let (executable, executable_identity) =
        match provider_executable("opencode", cancellation).await {
            Ok(authority) => authority,
            Err(failure) => return failed_provider(provider, failure),
        };
    provider.executable.clone_from(&executable);
    provider.executable_identity = executable_identity;
    // Native --refresh updates OpenCode's own model cache when discovery is requested.
    let output = match Box::pin(probe(
        &executable,
        &["models", "--refresh"],
        cancellation,
        &[],
    ))
    .await
    {
        Ok(output) => output,
        Err(error) => return failed_provider(provider, error),
    };
    let models = opencode_models(&output);
    let default_model = preferred_model(&models, "opencode/muse-spark-1.2-contributor-free");
    ready_provider(
        provider,
        default_model.clone(),
        vec![
            control("model", "모델", "combobox", models, &default_model),
            control(
                "variant",
                "모델 변형",
                "select",
                vec![
                    option("", "기본"),
                    option("high", "High"),
                    option("max", "Max"),
                ],
                "",
            ),
            permission_control(true),
        ],
    )
}

pub(crate) use crate::deepseek_catalog::discover as discover_deepseek;

pub(crate) async fn discover_cerebras(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    discover_gateway(
        provider,
        cancellation,
        cerebras::CATALOG_ENDPOINT,
        "Cerebras",
        "gpt-oss-120b",
        Some(control(
            "reasoning_effort",
            "추론 강도",
            "select",
            vec![
                option("low", "Low"),
                option("medium", "Medium"),
                option("high", "High"),
            ],
            "low",
        )),
    )
    .await
}

pub(crate) async fn discover_openrouter(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    let models = match openrouter::model_options(cancellation).await {
        Ok(models) => models,
        Err(error) => return failed_provider(provider, remote_catalog_failure(error)),
    };
    ready_gateway(provider, models, "OpenRouter", "openai/gpt-4.1-mini", None)
}

pub(crate) async fn discover_vercel(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    discover_gateway(
        provider,
        cancellation,
        vercel::CATALOG_ENDPOINT,
        "Vercel AI Gateway",
        "openai/gpt-5.4-mini",
        None,
    )
    .await
}

pub(crate) async fn discover_llm_gateway(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    discover_gateway(
        provider,
        cancellation,
        llm_gateway::CATALOG_ENDPOINT,
        llm_gateway::DISPLAY_NAME,
        llm_gateway::PREFERRED_MODEL,
        Some(control(
            "reasoning_effort",
            "추론 강도",
            "select",
            llm_gateway::REASONING_EFFORTS
                .into_iter()
                .map(|(value, label)| option(value, label))
                .collect(),
            "",
        )),
    )
    .await
}

pub(crate) async fn discover_tokenrouter(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    let payload = match fetch_public_catalog(tokenrouter::CATALOG_ENDPOINT, cancellation).await {
        Ok(payload) => payload,
        Err(error) => return failed_provider(provider, remote_catalog_failure(error)),
    };
    let models = match tokenrouter::model_options(&payload) {
        Ok(models) => models,
        Err(error) => return failed_provider(provider, remote_catalog_failure(error)),
    };
    ready_gateway(
        provider,
        models,
        tokenrouter::DISPLAY_NAME,
        tokenrouter::PREFERRED_MODEL,
        None,
    )
}

pub(crate) async fn discover_custom_api(
    mut provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    if cancellation.is_cancelled() {
        return failed_provider(provider, ProbeFailure::Cancelled);
    }
    custom_api::DISPLAY_NAME.clone_into(&mut provider.display_name);
    "static_manifest".clone_into(&mut provider.catalog_source);
    ready_provider(
        provider,
        String::new(),
        vec![remote_output_token_control(), permission_control(false)],
    )
}

async fn discover_gateway(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
    endpoint: &'static str,
    display_name: &str,
    preferred: &str,
    provider_control: Option<ProviderControl>,
) -> ProviderAvailability {
    let models = match fetch_gateway_model_options(endpoint, cancellation).await {
        Ok(models) => models,
        Err(error) => return failed_provider(provider, remote_catalog_failure(error)),
    };
    ready_gateway(provider, models, display_name, preferred, provider_control)
}

fn ready_gateway(
    provider: ProviderAvailability,
    models: Vec<ProviderControlOption>,
    display_name: &str,
    preferred: &str,
    provider_control: Option<ProviderControl>,
) -> ProviderAvailability {
    if models.is_empty() {
        return unavailable_provider(
            provider,
            true,
            "no_supported_models",
            &format!("{display_name} returned no text models with room-tool support"),
        );
    }
    let default_model = preferred_model(&models, preferred);
    let mut controls = vec![control("model", "모델", "combobox", models, &default_model)];
    controls.extend(provider_control);
    controls.extend([remote_output_token_control(), permission_control(false)]);
    ready_provider(provider, default_model.clone(), controls)
}

pub(crate) const fn remote_catalog_failure(error: RemoteCatalogError) -> ProbeFailure {
    match error {
        RemoteCatalogError::Cancelled => ProbeFailure::Cancelled,
        RemoteCatalogError::Timeout => ProbeFailure::Timeout,
        RemoteCatalogError::Authentication => ProbeFailure::Authentication,
        RemoteCatalogError::Malformed => ProbeFailure::Malformed,
        RemoteCatalogError::Failed => ProbeFailure::Failed,
        RemoteCatalogError::TooLarge => ProbeFailure::CatalogTooLarge,
    }
}

pub(crate) fn ready_provider(
    mut provider: ProviderAvailability,
    default_model: String,
    controls: Vec<ProviderControl>,
) -> ProviderAvailability {
    if !controls_are_bounded(&controls) {
        return failed_provider(provider, ProbeFailure::CatalogTooLarge);
    }
    if !controls_are_consistent(&default_model, &controls, provider.custom_model) {
        return malformed_provider(provider);
    }
    provider.default_model = default_model;
    provider.available = true;
    provider.startable = true;
    "ready".clone_into(&mut provider.discovery_status);
    provider.controls = controls;
    if serde_json::to_vec(&provider).map_or(true, |encoded| encoded.len() > MAX_PROVIDER_BYTES) {
        return failed_provider(provider, ProbeFailure::CatalogTooLarge);
    }
    provider
}

pub(crate) fn failed_provider(
    provider: ProviderAvailability,
    failure: ProbeFailure,
) -> ProviderAvailability {
    let (code, message, available) = match failure {
        ProbeFailure::Missing => ("command_missing", "configured command missing", false),
        ProbeFailure::Timeout => ("model_discovery_timeout", "model discovery timed out", true),
        ProbeFailure::Authentication => (
            "authentication_required",
            "provider login is required",
            true,
        ),
        ProbeFailure::Malformed => (
            "model_discovery_malformed",
            "provider returned malformed model data",
            true,
        ),
        ProbeFailure::Failed => (
            "model_discovery_failed",
            "provider model discovery failed",
            true,
        ),
        ProbeFailure::Cancelled => (
            "model_discovery_cancelled",
            "provider model discovery was cancelled",
            false,
        ),
        ProbeFailure::CatalogTooLarge => (
            "model_catalog_too_large",
            "provider model catalog exceeded its bounded authority",
            true,
        ),
        ProbeFailure::CleanupUnconfirmed => (
            "model_discovery_cleanup_failed",
            "provider model discovery process cleanup could not be confirmed",
            false,
        ),
    };
    unavailable_provider(provider, available, code, message)
}

pub(crate) fn unavailable_provider(
    mut provider: ProviderAvailability,
    available: bool,
    code: &str,
    message: &str,
) -> ProviderAvailability {
    provider.available = available;
    provider.startable = false;
    provider.default_model.clear();
    provider.controls.clear();
    "failed".clone_into(&mut provider.discovery_status);
    code.clone_into(&mut provider.discovery_error_code);
    message.clone_into(&mut provider.discovery_error);
    provider
}

fn malformed_provider(provider: ProviderAvailability) -> ProviderAvailability {
    failed_provider(provider, ProbeFailure::Malformed)
}

pub(crate) fn control(
    key: &str,
    label: &str,
    kind: &str,
    options: Vec<ProviderControlOption>,
    default_value: &str,
) -> ProviderControl {
    ProviderControl {
        key: key.to_owned(),
        label: label.to_owned(),
        kind: kind.to_owned(),
        options,
        default_value: default_value.to_owned(),
    }
}

pub(crate) fn permission_control(workspace_write: bool) -> ProviderControl {
    let mut options = vec![option("meeting_read_only", "방 읽기 전용")];
    if workspace_write {
        options.push(option("workspace_write", "작업 폴더 쓰기"));
    }
    control(
        "permission_mode",
        "권한",
        "select",
        options,
        "meeting_read_only",
    )
}

pub(crate) fn remote_output_token_control() -> ProviderControl {
    control(
        "max_output_tokens",
        "최대 응답 길이",
        "select",
        [1_024_u32, 2_048, 4_096, 8_192, 16_384]
            .into_iter()
            .map(|value| option(&value.to_string(), &format!("{value} 토큰")))
            .collect(),
        "4096",
    )
}

pub(crate) fn option(value: &str, label: &str) -> ProviderControlOption {
    ProviderControlOption {
        value: value.to_owned(),
        label: label.to_owned(),
        metadata: BTreeMap::new(),
    }
}

fn push_unique(values: &mut Vec<ProviderControlOption>, value: ProviderControlOption) {
    if !values.iter().any(|item| item.value == value.value) {
        values.push(value);
    }
}

pub(crate) fn preferred_model(options: &[ProviderControlOption], preferred: &str) -> String {
    options
        .iter()
        .find(|option| option.value == preferred)
        .map_or_else(String::new, |option| option.value.clone())
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn opencode_models(output: &str) -> Vec<ProviderControlOption> {
    output
        .lines()
        .map(str::trim)
        .filter(|model| valid_opencode_model(model))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|model| option(model, model))
        .collect()
}

fn valid_opencode_model(model: &str) -> bool {
    if model.is_empty() || model.len() > MAX_OPTION_VALUE_BYTES {
        return false;
    }
    let Some((namespace, name)) = model.split_once('/') else {
        return false;
    };
    if !matches!(namespace, "opencode" | "opencode-go") {
        return false;
    }
    !namespace.is_empty()
        && !name.is_empty()
        && namespace
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
}

fn controls_are_bounded(controls: &[ProviderControl]) -> bool {
    controls.iter().all(|control| {
        control.options.len() <= MAX_PROVIDER_OPTIONS
            && control.options.iter().all(|option| {
                option.value.len() <= MAX_OPTION_VALUE_BYTES
                    && option.label.len() <= MAX_OPTION_LABEL_BYTES
                    && !option.value.chars().any(char::is_control)
                    && !option.label.chars().any(char::is_control)
            })
    })
}

fn controls_are_consistent(
    default_model: &str,
    controls: &[ProviderControl],
    custom_model: bool,
) -> bool {
    let mut control_keys = BTreeSet::new();
    if controls.iter().any(|control| {
        let unselected_model = control.key == "model" && control.default_value.is_empty();
        control.options.is_empty()
            || !control_keys.insert(control.key.as_str())
            || control
                .options
                .iter()
                .map(|option| option.value.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                != control.options.len()
            || (!unselected_model
                && !control
                    .options
                    .iter()
                    .any(|option| option.value == control.default_value))
    }) {
        return false;
    }
    let model_control = controls.iter().find(|control| control.key == "model");
    if custom_model {
        return default_model.is_empty() && model_control.is_none();
    }
    let Some(model_control) = model_control else {
        return false;
    };
    if model_control.default_value != default_model {
        return false;
    }
    if default_model.is_empty() {
        return true;
    }
    let Some(model) = model_control
        .options
        .iter()
        .find(|option| option.value == default_model)
    else {
        return false;
    };
    [
        ("reasoning_effort", "reasoning_efforts"),
        ("service_tier", "service_tiers"),
    ]
    .into_iter()
    .all(|(control_key, relation_key)| {
        let Some(control) = controls.iter().find(|control| control.key == control_key) else {
            return true;
        };
        if control.default_value == "default" {
            return true;
        }
        let relation_scope = model.metadata.get("relation_scope").and_then(Value::as_str);
        let Some(allowed) = model.metadata.get(relation_key).and_then(Value::as_array) else {
            return relation_scope != Some("per_model");
        };
        if control.default_value.is_empty() {
            return allowed.is_empty() || allowed.iter().any(|value| value.as_str() == Some(""));
        }
        allowed
            .iter()
            .any(|value| value.as_str() == Some(&control.default_value))
    })
}

pub(crate) fn catalog_revision(
    providers: &[ProviderAvailability],
) -> Result<String, serde_json::Error> {
    let mut authority = serde_json::to_value(providers)?;
    if let Value::Array(entries) = &mut authority {
        for (entry, provider) in entries.iter_mut().zip(providers) {
            if let Value::Object(values) = entry {
                values.insert("executable".to_owned(), json!(provider.executable));
                values.insert(
                    "executable_identity".to_owned(),
                    json!(provider.executable_identity),
                );
            }
        }
    }
    let encoded = serde_json::to_vec(&authority)?;
    Ok(format!("provider-catalog-v1-{:x}", Sha256::digest(encoded)))
}

#[derive(Debug, Deserialize)]
struct CodexModels {
    #[serde(default)]
    models: Vec<CodexModel>,
}

#[derive(Debug, Deserialize)]
struct CodexModel {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    supported_reasoning_levels: Vec<CodexReasoning>,
    #[serde(default)]
    service_tiers: Vec<CodexTier>,
}

#[derive(Debug, Deserialize)]
struct CodexReasoning {
    #[serde(default)]
    effort: String,
}

#[derive(Debug, Deserialize)]
struct CodexTier {
    #[serde(default)]
    id: String,
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
