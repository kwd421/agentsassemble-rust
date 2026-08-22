use serde_json::{Map, Value};

use crate::ProviderSelectionError;

pub(crate) struct SelectionInput {
    pub catalog_revision: String,
    pub provider_id: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub variant: Option<String>,
    pub permission_mode: Option<String>,
    pub execution_harness: Option<String>,
    pub display_name: String,
    pub workspace: String,
    pub provider_endpoint: Option<String>,
    pub persona_card_id: Option<String>,
    pub provider_session_id: Option<String>,
    pub max_output_tokens: Option<u64>,
    pub start_requested: bool,
}

impl SelectionInput {
    pub fn parse(payload: &Value) -> Result<Self, ProviderSelectionError> {
        let values = payload
            .as_object()
            .ok_or_else(|| bad_request("payload must be an object."))?;
        reject_server_owned_fields(values)?;
        reject_unknown_fields(values)?;
        let start = alias_bool(values, &["start", "start_now"])?;
        Ok(Self {
            catalog_revision: required_identifier(values, &["catalog_revision"], 128)?,
            provider_id: required_identifier(
                values,
                &["provider_id", "provider_kind", "provider"],
                64,
            )?,
            model: optional_identifier(values, &["model", "model_id"], 128)?,
            reasoning_effort: optional_identifier(values, &["reasoning_effort", "effort"], 32)?,
            service_tier: optional_identifier(values, &["service_tier"], 32)?,
            variant: optional_identifier(values, &["variant"], 64)?,
            permission_mode: optional_identifier(
                values,
                &["permission_mode", "permission_option"],
                64,
            )?,
            execution_harness: optional_identifier(values, &["execution_harness"], 32)?,
            display_name: required_display_name(values)?,
            workspace: required_workspace(values)?,
            provider_endpoint: optional_identifier(values, &["provider_endpoint"], 1000)?,
            persona_card_id: optional_identifier(values, &["persona_card_id"], 80)?,
            provider_session_id: optional_identifier(values, &["session_id"], 128)?,
            max_output_tokens: optional_u64(values, "max_output_tokens")?,
            start_requested: start.unwrap_or(false),
        })
    }
}

fn reject_unknown_fields(values: &Map<String, Value>) -> Result<(), ProviderSelectionError> {
    const ALLOWED: [&str; 21] = [
        "catalog_revision",
        "provider_id",
        "provider_kind",
        "provider",
        "model",
        "model_id",
        "reasoning_effort",
        "effort",
        "service_tier",
        "variant",
        "permission_mode",
        "permission_option",
        "execution_harness",
        "display_name",
        "workspace",
        "provider_endpoint",
        "persona_card_id",
        "session_id",
        "max_output_tokens",
        "start",
        "start_now",
    ];
    if let Some(key) = values.keys().find(|key| !ALLOWED.contains(&key.as_str())) {
        return Err(bad_request(format!("Unknown agent.create field: {key}.")));
    }
    Ok(())
}

fn reject_server_owned_fields(values: &Map<String, Value>) -> Result<(), ProviderSelectionError> {
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
        return Err(bad_request(
            "Agent Session identity and runtime ownership are server-controlled.",
        ));
    }
    Ok(())
}

fn required_identifier(
    values: &Map<String, Value>,
    names: &[&str],
    limit: usize,
) -> Result<String, ProviderSelectionError> {
    optional_identifier(values, names, limit)?.ok_or_else(|| {
        bad_request(format!(
            "{} is required.",
            names.first().copied().unwrap_or("field")
        ))
    })
}

fn optional_identifier(
    values: &Map<String, Value>,
    names: &[&str],
    limit: usize,
) -> Result<Option<String>, ProviderSelectionError> {
    alias_value(values, names, |name, value| {
        let raw = value
            .as_str()
            .ok_or_else(|| bad_request(format!("{name} must be a string.")))?;
        if raw.len() > limit || raw.chars().any(char::is_control) || raw.trim() != raw {
            return Err(bad_request(format!("{name} is invalid.")));
        }
        Ok(raw.to_owned())
    })
}

fn alias_bool(
    values: &Map<String, Value>,
    names: &[&str],
) -> Result<Option<bool>, ProviderSelectionError> {
    alias_value(values, names, |name, value| {
        value
            .as_bool()
            .ok_or_else(|| bad_request(format!("{name} must be a boolean.")))
    })
}

fn alias_value<T: PartialEq>(
    values: &Map<String, Value>,
    names: &[&str],
    parse: impl Fn(&str, &Value) -> Result<T, ProviderSelectionError>,
) -> Result<Option<T>, ProviderSelectionError> {
    let mut selected = None;
    for name in names {
        let Some(value) = values.get(*name) else {
            continue;
        };
        let parsed = parse(name, value)?;
        if selected.as_ref().is_some_and(|current| current != &parsed) {
            return Err(bad_request(format!(
                "{} aliases must have the same value.",
                names.first().copied().unwrap_or("field")
            )));
        }
        selected = Some(parsed);
    }
    Ok(selected)
}

fn optional_u64(
    values: &Map<String, Value>,
    name: &str,
) -> Result<Option<u64>, ProviderSelectionError> {
    values
        .get(name)
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| bad_request(format!("{name} must be a nonnegative integer.")))
        })
        .transpose()
}

fn required_display_name(values: &Map<String, Value>) -> Result<String, ProviderSelectionError> {
    let raw = values
        .get("display_name")
        .and_then(Value::as_str)
        .ok_or_else(|| bad_request("display_name must be a string."))?;
    let display_name = agentsassemble_domain::clean_single_line(raw, 64);
    if !agentsassemble_domain::has_visible_text(&display_name) {
        return Err(bad_request("display_name is required."));
    }
    Ok(display_name)
}

fn required_workspace(values: &Map<String, Value>) -> Result<String, ProviderSelectionError> {
    let workspace = values
        .get("workspace")
        .and_then(Value::as_str)
        .ok_or_else(invalid_workspace)?;
    if workspace.is_empty() || workspace.len() > 4096 || workspace.chars().any(char::is_control) {
        return Err(invalid_workspace());
    }
    Ok(workspace.to_owned())
}

fn bad_request(message: impl Into<String>) -> ProviderSelectionError {
    ProviderSelectionError::new("bad_request", message)
}

fn invalid_workspace() -> ProviderSelectionError {
    ProviderSelectionError::new(
        "invalid_workspace",
        "An existing workspace directory is required.",
    )
}
