use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    sync::Arc,
};

use agentsassemble_domain::{
    ProviderAvailability, ProviderCatalog, ProviderControl, ProviderControlOption,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{
    ProviderSelection, ProviderSelectionError,
    filesystem::{FilesystemFailure, resolve_codex_executable, resolve_executable},
    process::{ProbeFailure, probe},
};

const MAX_PUBLIC_CATALOG_BYTES: usize = 48 * 1024;
const MAX_PROVIDER_BYTES: usize = 16 * 1024;
const MAX_PROVIDER_OPTIONS: usize = 256;
const MAX_OPTION_VALUE_BYTES: usize = 128;
const MAX_OPTION_LABEL_BYTES: usize = 256;

#[derive(Clone)]
pub struct ProviderCatalogService {
    _sender: watch::Sender<ProviderCatalog>,
    receiver: watch::Receiver<ProviderCatalog>,
    owner: Arc<CatalogOwner>,
}

struct CatalogOwner {
    cancellation: CancellationToken,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for CatalogOwner {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl ProviderCatalogService {
    #[must_use]
    pub fn discovering() -> Self {
        let initial = loading_catalog();
        let (sender, receiver) = watch::channel(initial);
        let refresh_sender = sender.clone();
        let cancellation = CancellationToken::new();
        let discovery_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let catalog = Box::pin(discover_catalog(&discovery_cancellation)).await;
            if !discovery_cancellation.is_cancelled() {
                let _ = refresh_sender.send(catalog);
            }
        });
        Self {
            _sender: sender,
            receiver,
            owner: Arc::new(CatalogOwner {
                cancellation,
                task: Mutex::new(Some(task)),
            }),
        }
    }

    #[must_use]
    pub fn fixed(catalog: ProviderCatalog) -> Self {
        let (sender, receiver) = watch::channel(bound_catalog(catalog));
        Self {
            _sender: sender,
            receiver,
            owner: Arc::new(CatalogOwner {
                cancellation: CancellationToken::new(),
                task: Mutex::new(None),
            }),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> ProviderCatalog {
        self.receiver.borrow().clone()
    }

    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<ProviderCatalog> {
        self.receiver.clone()
    }

    /// Cancels provider discovery and waits for its task to exit.
    ///
    /// # Errors
    ///
    /// Returns the discovery task's join error instead of hiding a panic or cancellation.
    pub async fn shutdown(&self) -> Result<(), tokio::task::JoinError> {
        self.owner.cancellation.cancel();
        if let Some(task) = self.owner.task.lock().await.take() {
            task.await?;
        }
        Ok(())
    }

    /// Validates a raw `agent.create` request against one exact catalog revision.
    ///
    /// # Errors
    ///
    /// Returns a stable fail-closed selection error.
    pub async fn validate_creation(
        &self,
        room_id: &str,
        principal_id: &str,
        request_id: &str,
        payload: &Value,
    ) -> Result<ProviderSelection, ProviderSelectionError> {
        ProviderSelection::from_catalog(
            room_id,
            principal_id,
            request_id,
            payload,
            &self.snapshot(),
        )
        .await
    }
}

async fn discover_catalog(cancellation: &CancellationToken) -> ProviderCatalog {
    let (codex, antigravity, opencode) = tokio::join!(
        Box::pin(discover_codex(cancellation)),
        Box::pin(discover_antigravity(cancellation)),
        Box::pin(discover_opencode(cancellation))
    );
    let providers = vec![codex, antigravity, opencode];
    let (status, catalog_revision) = match catalog_revision(&providers) {
        Ok(revision) => ("ready".to_owned(), revision),
        Err(_) => ("failed".to_owned(), String::new()),
    };
    let catalog = ProviderCatalog {
        status,
        catalog_revision,
        discovered_at: Utc::now().to_rfc3339(),
        providers,
    };
    bound_catalog(catalog)
}

fn bound_catalog(catalog: ProviderCatalog) -> ProviderCatalog {
    if serde_json::to_vec(&catalog).map_or(true, |encoded| encoded.len() > MAX_PUBLIC_CATALOG_BYTES)
    {
        return ProviderCatalog {
            status: "failed".to_owned(),
            catalog_revision: String::new(),
            discovered_at: catalog.discovered_at,
            providers: Vec::new(),
        };
    }
    catalog
}

fn loading_catalog() -> ProviderCatalog {
    ProviderCatalog {
        status: "loading".to_owned(),
        catalog_revision: String::new(),
        discovered_at: String::new(),
        providers: vec![
            loading_provider("codex", "Codex", "codex_live_session", "live_cli", "codex"),
            loading_provider(
                "antigravity",
                "Antigravity",
                "antigravity_live_session",
                "live_cli",
                "agy",
            ),
            loading_provider(
                "opencode",
                "OpenCode",
                "opencode_server",
                "opencode",
                "opencode",
            ),
        ],
    }
}

fn loading_provider(
    id: &str,
    display_name: &str,
    provider_kind: &str,
    runtime_kind: &str,
    executable: &str,
) -> ProviderAvailability {
    ProviderAvailability {
        id: id.to_owned(),
        display_name: display_name.to_owned(),
        provider_kind: provider_kind.to_owned(),
        runtime_kind: runtime_kind.to_owned(),
        catalog_group: "harness".to_owned(),
        workspace_required: true,
        connection_kind: "native_cli_bridge".to_owned(),
        executable: executable.to_owned(),
        executable_identity: String::new(),
        default_model: String::new(),
        interactive: true,
        startable: false,
        available: false,
        discovery_status: "loading".to_owned(),
        catalog_source: "discovered".to_owned(),
        discovery_error_code: String::new(),
        discovery_error: String::new(),
        login_available: true,
        login_label: "로그인".to_owned(),
        login_flow: if id == "codex" {
            "browser_oauth".to_owned()
        } else {
            "interactive_terminal".to_owned()
        },
        controls: Vec::new(),
    }
}

async fn provider_executable(
    program: &str,
    cancellation: &CancellationToken,
) -> Result<(String, String), ProbeFailure> {
    let resolved = await_filesystem(cancellation, resolve_executable(program)).await?;
    match resolved {
        Some(authority) => Ok(authority),
        None => Err(ProbeFailure::Missing),
    }
}

async fn await_filesystem<T>(
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
        Err(FilesystemFailure::Busy | FilesystemFailure::Failed) => Err(ProbeFailure::Failed),
    }
}

async fn discover_codex(cancellation: &CancellationToken) -> ProviderAvailability {
    let mut provider = loading_provider("codex", "Codex", "codex_live_session", "live_cli", "");
    let (executable, executable_identity) =
        match await_filesystem(cancellation, resolve_codex_executable()).await {
            Ok(Some(authority)) => authority,
            Ok(None) => return failed_provider(provider, ProbeFailure::Missing),
            Err(failure) => return failed_provider(provider, failure),
        };
    provider.executable.clone_from(&executable);
    provider.executable_identity = executable_identity;
    let output = match Box::pin(probe(&executable, &["debug", "models"], cancellation)).await {
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
            permission_control(),
        ],
    )
}

async fn discover_antigravity(cancellation: &CancellationToken) -> ProviderAvailability {
    let mut provider = loading_provider(
        "antigravity",
        "Antigravity",
        "antigravity_live_session",
        "live_cli",
        "",
    );
    let (executable, executable_identity) = match provider_executable("agy", cancellation).await {
        Ok(authority) => authority,
        Err(failure) => return failed_provider(provider, failure),
    };
    provider.executable.clone_from(&executable);
    provider.executable_identity = executable_identity;
    let output = match Box::pin(probe(&executable, &["models"], cancellation)).await {
        Ok(output) => output,
        Err(error) => return failed_provider(provider, error),
    };
    let mut grouped = BTreeMap::<String, Vec<String>>::new();
    for line in output.lines() {
        let id = line.split('\t').next().unwrap_or_default().trim();
        if id.is_empty() || id.starts_with("Fetching ") {
            continue;
        }
        let (model, effort) = split_effort(id);
        if !model.is_empty() {
            let values = grouped.entry(model).or_default();
            if !effort.is_empty() && !values.contains(&effort) {
                values.push(effort);
            }
        }
    }
    let mut models = Vec::new();
    let mut efforts = vec![option("", "기본")];
    for (model, model_efforts) in &grouped {
        for effort in model_efforts {
            push_unique(&mut efforts, option(effort, &title_case(effort)));
        }
        let mut metadata = BTreeMap::new();
        metadata.insert("relation_scope".to_owned(), json!("per_model"));
        metadata.insert("reasoning_efforts".to_owned(), json!(model_efforts));
        models.push(ProviderControlOption {
            value: model.clone(),
            label: model.clone(),
            metadata,
        });
    }
    let default_model = preferred_model(&models, "gemini-3.6-flash");
    ready_provider(
        provider,
        default_model.clone(),
        vec![
            control("model", "모델", "combobox", models, &default_model),
            control("reasoning_effort", "추론 강도", "select", efforts, "medium"),
            permission_control(),
        ],
    )
}

async fn discover_opencode(cancellation: &CancellationToken) -> ProviderAvailability {
    let mut provider = loading_provider("opencode", "OpenCode", "opencode_server", "opencode", "");
    let (executable, executable_identity) =
        match provider_executable("opencode", cancellation).await {
            Ok(authority) => authority,
            Err(failure) => return failed_provider(provider, failure),
        };
    provider.executable.clone_from(&executable);
    provider.executable_identity = executable_identity;
    let output = match Box::pin(probe(&executable, &["models"], cancellation)).await {
        Ok(output) => output,
        Err(error) => return failed_provider(provider, error),
    };
    let models = opencode_models(&output);
    let default_model = preferred_model(&models, "opencode/hy3-free");
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
            permission_control(),
        ],
    )
}

fn ready_provider(
    mut provider: ProviderAvailability,
    default_model: String,
    controls: Vec<ProviderControl>,
) -> ProviderAvailability {
    if default_model.is_empty() {
        return malformed_provider(provider);
    }
    if !controls_are_bounded(&controls) {
        return failed_provider(provider, ProbeFailure::CatalogTooLarge);
    }
    if !controls_are_consistent(&default_model, &controls) {
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

fn failed_provider(
    mut provider: ProviderAvailability,
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
    };
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

fn control(
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

fn permission_control() -> ProviderControl {
    control(
        "permission_mode",
        "권한",
        "select",
        vec![
            option("meeting_read_only", "방 읽기 전용"),
            option("workspace_write", "작업 폴더 쓰기"),
        ],
        "meeting_read_only",
    )
}

fn option(value: &str, label: &str) -> ProviderControlOption {
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

fn preferred_model(options: &[ProviderControlOption], preferred: &str) -> String {
    options
        .iter()
        .find(|option| option.value == preferred)
        .or_else(|| options.first())
        .map_or_else(String::new, |option| option.value.clone())
}

fn split_effort(value: &str) -> (String, String) {
    for effort in ["low", "medium", "high"] {
        if let Some(model) = value.strip_suffix(&format!("-{effort}")) {
            return (model.to_owned(), effort.to_owned());
        }
    }
    (value.to_owned(), String::new())
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

fn controls_are_consistent(default_model: &str, controls: &[ProviderControl]) -> bool {
    let mut control_keys = BTreeSet::new();
    if controls.iter().any(|control| {
        control.options.is_empty()
            || !control_keys.insert(control.key.as_str())
            || control
                .options
                .iter()
                .map(|option| option.value.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                != control.options.len()
            || !control
                .options
                .iter()
                .any(|option| option.value == control.default_value)
    }) {
        return false;
    }
    let Some(model) = controls
        .iter()
        .find(|control| control.key == "model" && control.default_value == default_model)
        .and_then(|control| {
            control
                .options
                .iter()
                .find(|option| option.value == default_model)
        })
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
            return allowed.is_empty();
        }
        allowed
            .iter()
            .any(|value| value.as_str() == Some(&control.default_value))
    })
}

fn catalog_revision(providers: &[ProviderAvailability]) -> Result<String, serde_json::Error> {
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
