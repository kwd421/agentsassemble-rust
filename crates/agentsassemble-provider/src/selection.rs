use std::path::Path;

use agentsassemble_domain::{
    AgentSessionDraft, ProviderAvailability, ProviderCatalog, clean_identifier, clean_single_line,
    has_visible_text, stable_identity_hash,
};
use same_file::Handle;
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSelection {
    pub agent_id: String,
    pub display_name: String,
    pub provider_id: String,
    pub provider_kind: String,
    pub runtime_kind: String,
    pub executable: String,
    pub executable_identity: String,
    pub workspace: String,
    pub workspace_identity: String,
    pub model: String,
    pub reasoning_effort: String,
    pub service_tier: String,
    pub variant: String,
    pub execution_harness: String,
    pub permission_mode: String,
    pub max_output_tokens: u32,
    pub catalog_revision: String,
    pub runtime_profile_key: String,
    pub transport: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ProviderSelectionError {
    pub code: &'static str,
    pub message: String,
}

impl ProviderSelectionError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl ProviderSelection {
    #[allow(clippy::too_many_lines)] // One pass owns fail-closed catalog/control/path validation.
    pub(crate) async fn from_catalog(
        room_id: &str,
        principal_id: &str,
        request_id: &str,
        payload: &Value,
        catalog: &ProviderCatalog,
    ) -> Result<Self, ProviderSelectionError> {
        let values = payload.as_object().ok_or_else(|| {
            ProviderSelectionError::new("bad_request", "payload must be an object.")
        })?;
        if truthy(values.get("start")) || truthy(values.get("start_now")) {
            return Err(ProviderSelectionError::new(
                "agent_start_unavailable",
                "Starting an Agent Session is not available in this runtime slice.",
            ));
        }
        if catalog.status != "ready" || catalog.catalog_revision.is_empty() {
            return Err(ProviderSelectionError::new(
                "catalog_not_ready",
                "Provider catalog is not ready.",
            ));
        }
        let revision = string(values.get("catalog_revision"), 128);
        if revision != catalog.catalog_revision {
            return Err(ProviderSelectionError::new(
                "catalog_changed",
                "Provider catalog changed; refresh the selection before creating the session.",
            ));
        }
        let provider_id = string(
            values
                .get("provider_id")
                .or_else(|| values.get("provider_kind"))
                .or_else(|| values.get("provider")),
            64,
        );
        let provider = catalog
            .providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .ok_or_else(|| unsupported(&provider_id))?;
        if !provider.startable || provider.discovery_status != "ready" {
            return Err(unsupported(&provider_id));
        }
        if !Path::new(&provider.executable).is_absolute()
            || !Path::new(&provider.executable).is_file()
        {
            return Err(ProviderSelectionError::new(
                "catalog_inconsistent",
                "Provider executable authority is not an absolute file.",
            ));
        }
        let executable_identity =
            stable_identity_hash(&Handle::from_path(&provider.executable).map_err(|_| {
                ProviderSelectionError::new(
                    "catalog_inconsistent",
                    "Provider executable authority could not be reopened.",
                )
            })?);
        if executable_identity != provider.executable_identity {
            return Err(ProviderSelectionError::new(
                "catalog_changed",
                "Provider executable identity changed; refresh discovery.",
            ));
        }
        reject_server_owned_fields(values)?;
        let model = selected_value(
            provider,
            "model",
            string(values.get("model").or_else(|| values.get("model_id")), 128),
        )?;
        let reasoning_effort = selected_value(
            provider,
            "reasoning_effort",
            string(
                values
                    .get("reasoning_effort")
                    .or_else(|| values.get("effort")),
                32,
            ),
        )?;
        validate_model_relation(provider, &model, "reasoning_efforts", &reasoning_effort)?;
        let service_tier = selected_value(
            provider,
            "service_tier",
            string(values.get("service_tier"), 32),
        )?;
        validate_model_relation(provider, &model, "service_tiers", &service_tier)?;
        let variant = selected_value(provider, "variant", string(values.get("variant"), 64))?;
        let permission_mode = selected_value(
            provider,
            "permission_mode",
            string(
                values
                    .get("permission_mode")
                    .or_else(|| values.get("permission_option")),
                64,
            ),
        )?;
        let execution_harness = string(values.get("execution_harness"), 32);
        if !execution_harness.is_empty() && execution_harness != "builtin" {
            return Err(ProviderSelectionError::new(
                "unsupported_control",
                "Alternate execution harnesses are not available in this runtime slice.",
            ));
        }
        if values
            .get("max_output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            != 0
            || !string(values.get("persona_card_id"), 80).is_empty()
            || !string(values.get("session_id"), 128).is_empty()
        {
            return Err(ProviderSelectionError::new(
                "unsupported_control",
                "This Agent Session option is not available in the current runtime slice.",
            ));
        }
        let display_name = clean_single_line(
            values
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            64,
        );
        if !has_visible_text(&display_name) {
            return Err(ProviderSelectionError::new(
                "bad_request",
                "display_name is required.",
            ));
        }
        let raw_workspace = exact_workspace(values.get("workspace"))?;
        if raw_workspace.is_empty() {
            return Err(ProviderSelectionError::new(
                "invalid_workspace",
                "An existing workspace directory is required.",
            ));
        }
        let workspace = tokio::fs::canonicalize(&raw_workspace)
            .await
            .map_err(|_| invalid_workspace())?;
        let metadata = tokio::fs::metadata(&workspace)
            .await
            .map_err(|_| invalid_workspace())?;
        if !metadata.is_dir() {
            return Err(invalid_workspace());
        }
        let workspace = workspace
            .into_os_string()
            .into_string()
            .map_err(|_| invalid_workspace())?;
        let workspace_identity =
            stable_identity_hash(&Handle::from_path(&workspace).map_err(|_| invalid_workspace())?);
        let transport = match provider.id.as_str() {
            "codex" => "stdio_jsonl",
            "antigravity" if cfg!(windows) => "conpty",
            "antigravity" => "pty",
            "opencode" => "http",
            _ => return Err(unsupported(&provider.id)),
        }
        .to_owned();
        let operation = format!("{room_id}\0{principal_id}\0{request_id}\0agent.create");
        let operation_hash = format!("{:x}", Sha256::digest(operation.as_bytes()));
        let identity = Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("agentsassemble:{operation_hash}").as_bytes(),
        );
        let agent_id = format!("{}-{identity}", provider.id);
        let mut selected = Self {
            agent_id,
            display_name,
            provider_id: provider.id.clone(),
            provider_kind: provider.provider_kind.clone(),
            runtime_kind: provider.runtime_kind.clone(),
            executable: provider.executable.clone(),
            executable_identity,
            workspace,
            workspace_identity,
            model,
            reasoning_effort,
            service_tier,
            variant,
            execution_harness: "builtin".to_owned(),
            permission_mode,
            max_output_tokens: 0,
            catalog_revision: revision,
            runtime_profile_key: String::new(),
            transport,
        };
        selected.runtime_profile_key = selected.profile_key();
        Ok(selected)
    }

    fn profile_key(&self) -> String {
        let fields = [
            self.provider_kind.as_str(),
            self.runtime_kind.as_str(),
            self.executable.as_str(),
            self.executable_identity.as_str(),
            self.workspace.as_str(),
            self.workspace_identity.as_str(),
            self.model.as_str(),
            self.reasoning_effort.as_str(),
            self.service_tier.as_str(),
            self.variant.as_str(),
            self.execution_harness.as_str(),
            self.permission_mode.as_str(),
            self.transport.as_str(),
        ];
        format!(
            "provider-profile-v1-{:x}",
            Sha256::digest(fields.join("\0").as_bytes())
        )
    }
}

impl From<ProviderSelection> for AgentSessionDraft {
    fn from(selection: ProviderSelection) -> Self {
        Self {
            agent_id: selection.agent_id,
            display_name: selection.display_name,
            provider_kind: selection.provider_kind,
            runtime_kind: selection.runtime_kind,
            executable: selection.executable,
            executable_identity: selection.executable_identity,
            workspace: selection.workspace,
            workspace_identity: selection.workspace_identity,
            model: selection.model,
            reasoning_effort: selection.reasoning_effort,
            service_tier: selection.service_tier,
            variant: selection.variant,
            execution_harness: selection.execution_harness,
            permission_mode: selection.permission_mode,
            max_output_tokens: selection.max_output_tokens,
            catalog_revision: selection.catalog_revision,
            runtime_profile_key: selection.runtime_profile_key,
            transport: selection.transport,
        }
    }
}

fn selected_value(
    provider: &ProviderAvailability,
    key: &str,
    requested: String,
) -> Result<String, ProviderSelectionError> {
    let Some(control) = provider.controls.iter().find(|control| control.key == key) else {
        return if requested.is_empty() {
            Ok(String::new())
        } else {
            Err(ProviderSelectionError::new(
                "unsupported_control",
                format!("Provider {} does not support {key}.", provider.id),
            ))
        };
    };
    let selected = if requested.is_empty() {
        control.default_value.clone()
    } else {
        requested
    };
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

fn validate_model_relation(
    provider: &ProviderAvailability,
    model: &str,
    relation: &str,
    selected: &str,
) -> Result<(), ProviderSelectionError> {
    if selected.is_empty() || selected == "default" {
        return Ok(());
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

fn reject_server_owned_fields(
    values: &serde_json::Map<String, Value>,
) -> Result<(), ProviderSelectionError> {
    const OWNED: [&str; 10] = [
        "agent_id",
        "participant_id",
        "owner_id",
        "created_by",
        "command",
        "executable",
        "runtime_kind",
        "transport",
        "process_ownership",
        "runtime_profile_key",
    ];
    if OWNED.iter().any(|key| values.contains_key(*key)) {
        return Err(ProviderSelectionError::new(
            "bad_request",
            "Agent Session identity and runtime ownership are server-controlled.",
        ));
    }
    if string(values.get("provider_endpoint"), 1000).is_empty() {
        Ok(())
    } else {
        Err(ProviderSelectionError::new(
            "unsupported_control",
            "Custom provider endpoints are not available in this runtime slice.",
        ))
    }
}

fn string(value: Option<&Value>, limit: usize) -> String {
    clean_identifier(value.and_then(Value::as_str).unwrap_or_default(), limit)
}

fn exact_workspace(value: Option<&Value>) -> Result<String, ProviderSelectionError> {
    let workspace = value.and_then(Value::as_str).unwrap_or_default();
    if workspace.is_empty() || workspace.len() > 4096 || workspace.chars().any(char::is_control) {
        return Err(invalid_workspace());
    }
    Ok(workspace.to_owned())
}

fn truthy(value: Option<&Value>) -> bool {
    value.and_then(Value::as_bool).unwrap_or(false)
}

fn unsupported(provider_id: &str) -> ProviderSelectionError {
    ProviderSelectionError::new(
        "unsupported_provider",
        format!(
            "Provider {} is not available in the current catalog.",
            if provider_id.is_empty() {
                "unknown"
            } else {
                provider_id
            }
        ),
    )
}

fn invalid_workspace() -> ProviderSelectionError {
    ProviderSelectionError::new(
        "invalid_workspace",
        "An existing workspace directory is required.",
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use agentsassemble_domain::{
        ProviderAvailability, ProviderCatalog, ProviderControl, ProviderControlOption,
        stable_identity_hash,
    };
    use same_file::Handle;
    use serde_json::{Value, json};

    use super::ProviderSelection;

    fn option(value: &str, relations: Value) -> ProviderControlOption {
        let mut metadata = BTreeMap::new();
        if let Value::Object(entries) = relations {
            metadata.extend(entries);
        }
        ProviderControlOption {
            value: value.to_owned(),
            label: value.to_owned(),
            metadata,
        }
    }

    fn control(
        key: &str,
        values: Vec<ProviderControlOption>,
        default_value: &str,
    ) -> ProviderControl {
        ProviderControl {
            key: key.to_owned(),
            label: key.to_owned(),
            kind: "select".to_owned(),
            options: values,
            default_value: default_value.to_owned(),
        }
    }

    fn catalog() -> ProviderCatalog {
        ProviderCatalog {
            status: "ready".to_owned(),
            catalog_revision: "catalog-1".to_owned(),
            discovered_at: String::new(),
            providers: vec![ProviderAvailability {
                id: "codex".to_owned(),
                display_name: "Codex".to_owned(),
                provider_kind: "codex_live_session".to_owned(),
                runtime_kind: "live_cli".to_owned(),
                catalog_group: "subscription".to_owned(),
                workspace_required: true,
                connection_kind: "native_cli_bridge".to_owned(),
                executable: std::env::current_exe()
                    .unwrap_or_else(|error| panic!("resolve test executable: {error}"))
                    .to_string_lossy()
                    .into_owned(),
                executable_identity: stable_identity_hash(
                    &Handle::from_path(
                        std::env::current_exe()
                            .unwrap_or_else(|error| panic!("resolve test executable: {error}")),
                    )
                    .unwrap_or_else(|error| panic!("open test executable: {error}")),
                ),
                default_model: "gpt-5.6-terra".to_owned(),
                interactive: true,
                startable: true,
                available: true,
                discovery_status: "ready".to_owned(),
                catalog_source: "discovered".to_owned(),
                discovery_error_code: String::new(),
                discovery_error: String::new(),
                login_available: true,
                login_label: "Login".to_owned(),
                login_flow: "browser_oauth".to_owned(),
                controls: vec![
                    control(
                        "model",
                        vec![option(
                            "gpt-5.6-terra",
                            json!({
                                "relation_scope": "per_model",
                                "reasoning_efforts": ["medium", "high"],
                                "service_tiers": ["priority"]
                            }),
                        )],
                        "gpt-5.6-terra",
                    ),
                    control(
                        "reasoning_effort",
                        vec![option("medium", json!({})), option("high", json!({}))],
                        "medium",
                    ),
                    control(
                        "service_tier",
                        vec![option("default", json!({})), option("priority", json!({}))],
                        "default",
                    ),
                    control(
                        "permission_mode",
                        vec![option("meeting_read_only", json!({}))],
                        "meeting_read_only",
                    ),
                ],
            }],
        }
    }

    #[tokio::test]
    async fn exact_selection_is_normalized_and_identity_is_stable() {
        let workspace =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create workspace: {error}"));
        let payload = json!({
            "provider_id": "codex",
            "catalog_revision": "catalog-1",
            "display_name": " Terra ",
            "workspace": workspace.path(),
            "model": "gpt-5.6-terra",
            "reasoning_effort": "high",
            "service_tier": "priority",
            "permission_mode": "meeting_read_only"
        });
        let first = ProviderSelection::from_catalog(
            "general",
            "operator-local-user",
            "create-1",
            &payload,
            &catalog(),
        )
        .await
        .unwrap_or_else(|error| panic!("select provider: {error}"));
        let second = ProviderSelection::from_catalog(
            "general",
            "operator-local-user",
            "create-1",
            &payload,
            &catalog(),
        )
        .await
        .unwrap_or_else(|error| panic!("repeat selection: {error}"));
        assert_eq!(first.agent_id, second.agent_id);
        assert_eq!(first.display_name, "Terra");
        assert_eq!(
            first.workspace,
            workspace
                .path()
                .canonicalize()
                .unwrap_or_else(|error| panic!("canonical workspace: {error}"))
        );
        assert!(!first.runtime_profile_key.is_empty());
        assert!(!first.workspace_identity.is_empty());
    }

    #[tokio::test]
    async fn stale_relation_and_start_requests_fail_closed() {
        let workspace =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create workspace: {error}"));
        let base = json!({
            "provider_id": "codex",
            "catalog_revision": "catalog-1",
            "display_name": "Terra",
            "workspace": workspace.path(),
            "model": "gpt-5.6-terra",
            "reasoning_effort": "low"
        });
        let error = ProviderSelection::from_catalog(
            "general",
            "operator-local-user",
            "create-1",
            &base,
            &catalog(),
        )
        .await
        .err()
        .unwrap_or_else(|| panic!("unsupported per-model effort must fail"));
        assert_eq!(error.code, "unsupported_control");
        let mut stale = base.clone();
        stale["catalog_revision"] = json!("catalog-stale");
        stale["reasoning_effort"] = json!("medium");
        let error = ProviderSelection::from_catalog(
            "general",
            "operator-local-user",
            "create-stale",
            &stale,
            &catalog(),
        )
        .await
        .err()
        .unwrap_or_else(|| panic!("stale catalog revision must fail"));
        assert_eq!(error.code, "catalog_changed");
        let mut invalid_workspace = stale;
        invalid_workspace["catalog_revision"] = json!("catalog-1");
        invalid_workspace["workspace"] = json!(workspace.path().join("missing"));
        let error = ProviderSelection::from_catalog(
            "general",
            "operator-local-user",
            "create-missing-workspace",
            &invalid_workspace,
            &catalog(),
        )
        .await
        .err()
        .unwrap_or_else(|| panic!("missing workspace must fail"));
        assert_eq!(error.code, "invalid_workspace");
        let mut starting = base;
        starting["reasoning_effort"] = json!("medium");
        starting["start_now"] = json!(true);
        let error = ProviderSelection::from_catalog(
            "general",
            "operator-local-user",
            "create-2",
            &starting,
            &catalog(),
        )
        .await
        .err()
        .unwrap_or_else(|| panic!("start_now must not produce a fake stopped success"));
        assert_eq!(error.code, "agent_start_unavailable");
    }

    #[tokio::test]
    async fn workspace_path_is_exact_and_per_model_relations_are_mandatory() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("create workspace: {error}"));
        let spaced = root.path().join(" workspace ");
        std::fs::create_dir(&spaced)
            .unwrap_or_else(|error| panic!("create spaced workspace: {error}"));
        let payload = json!({
            "provider_id": "codex",
            "catalog_revision": "catalog-1",
            "display_name": "Terra",
            "workspace": spaced,
            "model": "gpt-5.6-terra",
            "reasoning_effort": "medium"
        });
        let selected = ProviderSelection::from_catalog(
            "general",
            "operator-local-user",
            "create-spaced",
            &payload,
            &catalog(),
        )
        .await
        .unwrap_or_else(|error| panic!("select exact workspace: {error}"));
        assert_eq!(
            std::path::Path::new(&selected.workspace)
                .file_name()
                .and_then(std::ffi::OsStr::to_str),
            Some(" workspace ")
        );

        let mut control_character = payload.clone();
        control_character["workspace"] = json!(format!("{}\n", root.path().display()));
        let error = ProviderSelection::from_catalog(
            "general",
            "operator-local-user",
            "create-control",
            &control_character,
            &catalog(),
        )
        .await
        .err()
        .unwrap_or_else(|| panic!("control character must fail"));
        assert_eq!(error.code, "invalid_workspace");

        let mut incomplete = catalog();
        incomplete.providers[0].controls[0].options[0]
            .metadata
            .remove("reasoning_efforts");
        let error = ProviderSelection::from_catalog(
            "general",
            "operator-local-user",
            "create-incomplete",
            &payload,
            &incomplete,
        )
        .await
        .err()
        .unwrap_or_else(|| panic!("missing per-model relation must fail"));
        assert_eq!(error.code, "catalog_inconsistent");
    }
}
