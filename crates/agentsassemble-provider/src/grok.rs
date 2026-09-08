pub(crate) const HOME_ENV: &str = "GROK_HOME";

use std::{collections::BTreeSet, env, io, path::PathBuf, time::Duration};

use agentsassemble_domain::{ProviderAvailability, ProviderControlOption};
use tokio_util::sync::CancellationToken;

use crate::{
    catalog::{
        control, failed_provider, option, permission_control, provider_executable, ready_provider,
    },
    process::{ProbeFailure, probe_with_timeout},
};

const MODEL_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
const CONFIG_READ_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_CUSTOM_MODELS: usize = 256;

pub(crate) async fn discover(
    mut provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    let (executable, executable_identity) = match provider_executable("grok", cancellation).await {
        Ok(authority) => authority,
        Err(failure) => return failed_provider(provider, failure),
    };
    provider.executable.clone_from(&executable);
    provider.executable_identity = executable_identity;
    let custom_models = match custom_model_ids().await {
        Ok(models) => models,
        Err(failure) => return failed_provider(provider, failure),
    };
    let output = match probe_with_timeout(
        &executable,
        &["models"],
        MODEL_DISCOVERY_TIMEOUT,
        cancellation,
        &[],
    )
    .await
    {
        Ok(output) => output,
        Err(failure) => return failed_provider(provider, failure),
    };
    let Some(catalog) = parse_models(&output, &custom_models) else {
        return failed_provider(provider, ProbeFailure::Malformed);
    };
    ready_provider(provider, catalog.default_model.clone(), catalog.controls())
}

struct GrokCatalog {
    models: Vec<String>,
    default_model: String,
}

impl GrokCatalog {
    fn controls(self) -> Vec<agentsassemble_domain::ProviderControl> {
        vec![
            control(
                "model",
                "모델",
                "combobox",
                self.models
                    .iter()
                    .map(|model| option(model, model))
                    .collect::<Vec<ProviderControlOption>>(),
                &self.default_model,
            ),
            control(
                "reasoning_effort",
                "추론 강도",
                "select",
                vec![
                    option("low", "Low"),
                    option("medium", "Medium"),
                    option("high", "High"),
                ],
                "medium",
            ),
            permission_control(true),
        ]
    }
}

fn parse_models(output: &str, custom_models: &BTreeSet<String>) -> Option<GrokCatalog> {
    let mut models = Vec::new();
    let mut advertised_default = None;
    for line in output.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("Default model:") {
            let model = value.trim();
            if valid_model_id(model) {
                advertised_default = Some(model.to_owned());
            }
            continue;
        }
        let model = line
            .strip_prefix("* ")
            .map(|model| model.strip_suffix(" (default)").unwrap_or(model))
            .or_else(|| line.strip_prefix("- "));
        if let Some(model) = model
            && valid_model_id(model)
            && !custom_models.contains(model)
            && !models.iter().any(|known| known == model)
        {
            models.push(model.to_owned());
        }
    }
    let first = models.first()?.clone();
    let default_model = advertised_default
        .filter(|candidate| models.iter().any(|model| model == candidate))
        .unwrap_or(first);
    Some(GrokCatalog {
        models,
        default_model,
    })
}

async fn custom_model_ids() -> Result<BTreeSet<String>, ProbeFailure> {
    let Some(path) = config_path() else {
        return Ok(BTreeSet::new());
    };
    tokio::time::timeout(CONFIG_READ_TIMEOUT, read_custom_model_ids(path))
        .await
        .map_err(|_| ProbeFailure::Timeout)?
}

async fn read_custom_model_ids(path: PathBuf) -> Result<BTreeSet<String>, ProbeFailure> {
    let metadata = match tokio::fs::metadata(&path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(_) => return Err(ProbeFailure::Failed),
    };
    if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
        return Err(ProbeFailure::Malformed);
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| ProbeFailure::Failed)?;
    parse_custom_model_ids(&bytes)
}

fn parse_custom_model_ids(bytes: &[u8]) -> Result<BTreeSet<String>, ProbeFailure> {
    let text = std::str::from_utf8(bytes).map_err(|_| ProbeFailure::Malformed)?;
    let document = toml::from_str::<toml::Value>(text).map_err(|_| ProbeFailure::Malformed)?;
    let Some(models) = document.get("model") else {
        return Ok(BTreeSet::new());
    };
    let models = models.as_table().ok_or(ProbeFailure::Malformed)?;
    if models.len() > MAX_CUSTOM_MODELS || models.keys().any(|model| !valid_model_id(model)) {
        return Err(ProbeFailure::Malformed);
    }
    Ok(models.keys().cloned().collect())
}

fn config_path() -> Option<PathBuf> {
    env::var_os(HOME_ENV)
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".grok")))
        .map(|home| home.join("config.toml"))
}

pub(crate) fn valid_model_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= crate::catalog::MAX_OPTION_VALUE_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

#[cfg(test)]
#[path = "grok_tests.rs"]
mod tests;
