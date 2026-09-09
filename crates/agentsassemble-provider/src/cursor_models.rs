use std::collections::{BTreeMap, BTreeSet};

use agent_client_protocol::schema::v1::{
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption,
    SessionConfigSelectOptions,
};
use agentsassemble_domain::{ProviderControl, ProviderControlOption};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    acp_client::AcpClient,
    catalog::{MAX_OPTION_VALUE_BYTES, MAX_PROVIDER_OPTIONS, control, option, permission_control},
    driver::DriverError,
};

pub(crate) struct CursorCatalog {
    choices: Vec<ModelChoice>,
    pub(crate) default_model: String,
}

struct ModelChoice {
    value: String,
    label: String,
    base: String,
    parameters: BTreeMap<String, String>,
    effort: Option<(String, Vec<String>)>,
    tiers: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeModel {
    value: String,
    name: String,
    config_options: Vec<SessionConfigOption>,
}

impl CursorCatalog {
    pub(crate) async fn read(client: &mut AcpClient) -> Result<Self, DriverError> {
        Self::parse(
            &client
                .request_extension("cursor/list_available_models", json!({}))
                .await?,
        )
        .ok_or_else(invalid)
    }

    fn parse(payload: &Value) -> Option<Self> {
        let native: Vec<NativeModel> =
            serde_json::from_value(payload.get("models")?.clone()).ok()?;
        if native.is_empty() || native.len() > MAX_PROVIDER_OPTIONS {
            return None;
        }
        let mut choices = Vec::new();
        let mut bases = BTreeSet::new();
        for model in native {
            if !identifier(&model.value) || !bases.insert(model.value.clone()) {
                return None;
            }
            let mut fixed = vec![(BTreeMap::new(), Vec::new())];
            let mut parameters = BTreeMap::new();
            let mut effort = None;
            let mut tiers = vec!["default".to_owned()];
            for setting in &model.config_options {
                let key = setting.id.0.to_string();
                let (current, values) = advertised_values(&setting.kind)?;
                if !identifier(&key)
                    || key == "model"
                    || parameters.insert(key.clone(), current.to_owned()).is_some()
                {
                    return None;
                }
                if key != "thinking"
                    && setting.category == Some(SessionConfigOptionCategory::ThoughtLevel)
                {
                    if effort.is_some() {
                        return None;
                    }
                    effort = Some((
                        key,
                        values
                            .iter()
                            .map(|value| value.value.0.to_string())
                            .collect(),
                    ));
                } else if key == "fast" {
                    if values
                        .iter()
                        .any(|value| !matches!(value.value.0.as_ref(), "true" | "false"))
                        || !values.iter().any(|value| value.value.0.as_ref() == "false")
                    {
                        return None;
                    }
                    if values.iter().any(|value| value.value.0.as_ref() == "true") {
                        tiers.push("fast".to_owned());
                    }
                } else {
                    fixed = expand_fixed(fixed, setting, values, current)?;
                }
            }
            for (fixed, labels) in fixed {
                let value = if fixed.is_empty() {
                    model.value.clone()
                } else {
                    format!(
                        "{}[{}]",
                        model.value,
                        fixed
                            .iter()
                            .map(|(key, value)| format!("{key}={value}"))
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                };
                if value.len() > MAX_OPTION_VALUE_BYTES || choices.len() >= MAX_PROVIDER_OPTIONS {
                    return None;
                }
                let mut parameters = parameters.clone();
                parameters.extend(fixed);
                choices.push(ModelChoice {
                    value,
                    label: if labels.is_empty() {
                        model.name.clone()
                    } else {
                        format!("{} ({})", model.name, labels.join(", "))
                    },
                    base: model.value.clone(),
                    parameters,
                    effort: effort.clone(),
                    tiers: tiers.clone(),
                });
            }
        }
        let default_model = choices
            .iter()
            .find(|choice| choice.base == "default")?
            .value
            .clone();
        Some(Self {
            choices,
            default_model,
        })
    }

    pub(crate) fn selection(
        &self,
        model: &str,
        effort: &str,
        tier: &str,
    ) -> Option<Vec<(String, String)>> {
        let choice = self.choices.iter().find(|choice| choice.value == model)?;
        if !choice.tiers.iter().any(|allowed| allowed == tier) {
            return None;
        }
        let mut parameters = choice.parameters.clone();
        if !effort.is_empty() {
            let (key, allowed) = choice.effort.as_ref()?;
            if !allowed.iter().any(|allowed| allowed == effort) {
                return None;
            }
            parameters.insert(key.clone(), effort.to_owned());
        }
        if parameters.contains_key("fast") {
            parameters.insert("fast".to_owned(), (tier == "fast").to_string());
        }
        let mut selection = vec![("model".to_owned(), choice.base.clone())];
        selection.extend(parameters);
        Some(selection)
    }

    pub(crate) fn controls(self) -> Vec<ProviderControl> {
        let mut efforts = BTreeSet::from([String::new()]);
        let mut tiers = BTreeSet::from(["default".to_owned()]);
        let models = self
            .choices
            .into_iter()
            .map(|choice| {
                let mut allowed = vec![String::new()];
                if let Some((_, values)) = choice.effort {
                    allowed.extend(values);
                }
                efforts.extend(allowed.iter().cloned());
                tiers.extend(choice.tiers.iter().cloned());
                ProviderControlOption {
                    value: choice.value,
                    label: choice.label,
                    metadata: BTreeMap::from([
                        ("relation_scope".to_owned(), json!("per_model")),
                        ("reasoning_efforts".to_owned(), json!(allowed)),
                        ("service_tiers".to_owned(), json!(choice.tiers)),
                    ]),
                }
            })
            .collect();
        vec![
            control("model", "모델", "combobox", models, &self.default_model),
            control(
                "reasoning_effort",
                "추론 강도",
                "select",
                efforts
                    .iter()
                    .map(|effort| option(effort, if effort.is_empty() { "기본" } else { effort }))
                    .collect(),
                "",
            ),
            control(
                "service_tier",
                "응답 속도",
                "select",
                tiers
                    .iter()
                    .map(|tier| option(tier, if tier == "default" { "기본" } else { "Fast" }))
                    .collect(),
                "default",
            ),
            permission_control(false),
        ]
    }
}

type FixedChoice = (BTreeMap<String, String>, Vec<String>);

fn advertised_values(kind: &SessionConfigKind) -> Option<(&str, Vec<&SessionConfigSelectOption>)> {
    let SessionConfigKind::Select(select) = kind else {
        return None;
    };
    let values = match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options.iter().collect::<Vec<_>>(),
        SessionConfigSelectOptions::Grouped(groups) => {
            groups.iter().flat_map(|group| &group.options).collect()
        }
        _ => return None,
    };
    let current = select.current_value.0.as_ref();
    if values.is_empty()
        || values.len() > MAX_PROVIDER_OPTIONS
        || !values.iter().any(|value| value.value.0.as_ref() == current)
        || values.iter().any(|value| !identifier(&value.value.0))
        || values
            .iter()
            .map(|value| value.value.0.as_ref())
            .collect::<BTreeSet<_>>()
            .len()
            != values.len()
    {
        return None;
    }
    Some((current, values))
}

fn expand_fixed(
    fixed: Vec<FixedChoice>,
    setting: &SessionConfigOption,
    mut values: Vec<&SessionConfigSelectOption>,
    current: &str,
) -> Option<Vec<FixedChoice>> {
    if fixed.len().checked_mul(values.len())? > MAX_PROVIDER_OPTIONS {
        return None;
    }
    let mut expanded = Vec::new();
    values.sort_by_key(|value| value.value.0.as_ref() != current);
    for (params, labels) in fixed {
        for value in &values {
            let mut params = params.clone();
            params.insert(setting.id.0.to_string(), value.value.0.to_string());
            let mut labels = labels.clone();
            labels.push(format!("{}: {}", setting.name, value.name));
            expanded.push((params, labels));
        }
    }
    Some(expanded)
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_OPTION_VALUE_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
}

const fn invalid() -> DriverError {
    DriverError::new(
        "provider_model_catalog_invalid",
        "Cursor did not expose a valid parameterized model catalog.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Value {
        json!({"models": [
            {"value": "default", "name": "Auto", "configOptions": []},
            {"value": "gpt-5.6-sol", "name": "Sol", "configOptions": [
                {"id": "context", "name": "Context", "category": "model_config", "type": "select", "currentValue": "272k", "options": [{"value":"272k","name":"272k"},{"value":"1m","name":"1M"}]},
                {"id": "reasoning", "name": "Reasoning", "category": "thought_level", "type": "select", "currentValue": "medium", "options": [{"value":"medium","name":"Medium"},{"value":"high","name":"High"}]},
                {"id": "fast", "name": "Fast", "category": "model_config", "type": "select", "currentValue": "true", "options": [{"value":"false","name":"Off"},{"value":"true","name":"On"}]}
            ]},
            {"value": "claude-opus-5", "name": "Opus", "configOptions": [
                {"id":"thinking","name":"Thinking","category":"thought_level","type":"select","currentValue":"true","options":[{"value":"false","name":"Off"},{"value":"true","name":"On"}]},
                {"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":"high","options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]}
            ]}
        ]})
    }

    #[tokio::test]
    async fn native_catalog_drives_context_reasoning_speed_and_auto_selection() {
        let catalog = CursorCatalog::parse(&catalog()).unwrap_or_else(|| panic!("native catalog"));
        assert_eq!(catalog.default_model, "default");
        assert_eq!(
            catalog.selection("claude-opus-5[thinking=false]", "low", "default"),
            Some(vec![
                ("model".into(), "claude-opus-5".into()),
                ("effort".into(), "low".into()),
                ("thinking".into(), "false".into())
            ])
        );
        assert_eq!(
            catalog.selection("default", "", "default"),
            Some(vec![("model".into(), "default".into())])
        );
        assert!(catalog.selection("default", "high", "default").is_none());
        assert!(catalog.selection("default", "", "fast").is_none());
        assert!(
            catalog
                .selection("gpt-5.6-sol[context=1m]", "max", "fast")
                .is_none()
        );
        assert_eq!(
            catalog.selection("gpt-5.6-sol[context=1m]", "high", "fast"),
            Some(vec![
                ("model".into(), "gpt-5.6-sol".into()),
                ("context".into(), "1m".into()),
                ("fast".into(), "true".into()),
                ("reasoning".into(), "high".into())
            ])
        );
        assert_eq!(
            catalog
                .selection("gpt-5.6-sol[context=272k]", "", "default")
                .and_then(|pairs| pairs
                    .into_iter()
                    .find(|(key, _)| key == "fast")
                    .map(|(_, value)| value)),
            Some("false".into())
        );
        crate::test_support::assert_default_tier_selection(
            "cursor",
            catalog.default_model.clone(),
            catalog.controls(),
        )
        .await;
    }

    #[test]
    fn malformed_parameter_authority_does_not_become_a_default_catalog() {
        for change in [
            "missing_current",
            "duplicate",
            "unsupported_kind",
            "missing_auto",
        ] {
            let mut payload = catalog();
            match change {
                "missing_current" => {
                    payload["models"][1]["configOptions"][0]["currentValue"] = json!("missing");
                }
                "duplicate" => payload["models"][1]["configOptions"][1]["id"] = json!("context"),
                "unsupported_kind" => {
                    payload["models"][1]["configOptions"][0]["type"] = json!("future");
                }
                _ => payload["models"][0]["value"] = json!("other"),
            }
            assert!(
                CursorCatalog::parse(&payload).is_none(),
                "accepted {change}"
            );
        }
    }
}
