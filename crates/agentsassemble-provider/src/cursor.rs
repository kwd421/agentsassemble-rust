use std::{collections::BTreeMap, time::Duration};

use agentsassemble_domain::{ProviderAvailability, ProviderControlOption};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::{
    catalog::{
        control, failed_provider, option, permission_control, provider_executable, ready_provider,
    },
    process::{ProbeFailure, probe_with_timeout},
};

const MODEL_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
const EFFORT_PARTS: [&str; 9] = [
    "thinking", "minimal", "none", "low", "medium", "high", "xhigh", "max", "ultra",
];

pub(crate) async fn discover(
    mut provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    let (executable, executable_identity) =
        match provider_executable("cursor-agent", cancellation).await {
            Ok(authority) => authority,
            Err(failure) => return failed_provider(provider, failure),
        };
    provider.executable.clone_from(&executable);
    provider.executable_identity = executable_identity;
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
    let Some(catalog) = parse_models(&output) else {
        return failed_provider(provider, ProbeFailure::Malformed);
    };
    ready_provider(provider, catalog.default_model.clone(), catalog.controls())
}

pub(crate) fn effective_model(
    model: &str,
    reasoning_effort: &str,
    service_tier: &str,
) -> Option<String> {
    if model.is_empty()
        || model.chars().any(char::is_control)
        || (!reasoning_effort.is_empty()
            && reasoning_effort
                .split('-')
                .any(|part| !EFFORT_PARTS.contains(&part)))
        || !matches!(service_tier, "default" | "fast")
    {
        return None;
    }
    let mut result = model.to_owned();
    if !reasoning_effort.is_empty() {
        result.push('-');
        result.push_str(reasoning_effort);
    }
    if service_tier == "fast" {
        result.push_str("-fast");
    }
    Some(result)
}

struct CursorCatalog {
    models: Vec<ProviderControlOption>,
    efforts: Vec<String>,
    has_fast: bool,
    default_model: String,
}

impl CursorCatalog {
    fn controls(self) -> Vec<agentsassemble_domain::ProviderControl> {
        let mut controls = vec![control(
            "model",
            "모델",
            "combobox",
            self.models,
            &self.default_model,
        )];
        if self.efforts.iter().any(|effort| !effort.is_empty()) {
            controls.push(control(
                "reasoning_effort",
                "추론 강도",
                "select",
                self.efforts
                    .iter()
                    .map(|effort| option(effort, effort_label(effort)))
                    .collect(),
                "",
            ));
        }
        if self.has_fast {
            controls.push(control(
                "service_tier",
                "응답 속도",
                "select",
                vec![option("default", "기본"), option("fast", "Fast")],
                "default",
            ));
        }
        controls.push(permission_control(false));
        controls
    }
}

struct ModelGroup {
    base: String,
    label: String,
    variants: Vec<(String, String)>,
}

fn parse_models(output: &str) -> Option<CursorCatalog> {
    let mut groups: Vec<ModelGroup> = Vec::new();
    let mut efforts = Vec::new();
    let mut has_fast = false;
    for line in output.lines().map(str::trim) {
        let Some((slug, label)) = line.split_once(" - ") else {
            continue;
        };
        if !valid_slug(slug) || label.is_empty() || label.chars().any(char::is_control) {
            continue;
        }
        let (base, effort, tier) = split_variant(slug);
        has_fast |= tier == "fast";
        push_unique(&mut efforts, effort.clone());
        let group = if let Some(group) = groups.iter_mut().find(|group| group.base == base) {
            group
        } else {
            groups.push(ModelGroup {
                base: base.clone(),
                label: label.trim_end_matches(" Fast").to_owned(),
                variants: Vec::new(),
            });
            groups.last_mut()?
        };
        if label.len() < group.label.len() {
            label.clone_into(&mut group.label);
        }
        if !group.variants.contains(&(effort.clone(), tier.clone())) {
            group.variants.push((effort, tier));
        }
    }
    if groups.is_empty() {
        return None;
    }
    efforts.sort_by_key(|effort| effort_rank(effort));
    let default_model = groups
        .iter()
        .find(|group| group.base == "auto")
        .map_or_else(|| groups[0].base.clone(), |group| group.base.clone());
    let models = groups
        .into_iter()
        .map(|group| {
            let mut reasoning_efforts = Vec::new();
            let mut service_tiers = Vec::new();
            for (effort, tier) in &group.variants {
                push_unique(&mut reasoning_efforts, effort.clone());
                push_unique(&mut service_tiers, tier.clone());
            }
            let runtime_variants = group
                .variants
                .into_iter()
                .map(|(reasoning_effort, service_tier)| {
                    json!({
                        "reasoning_effort": reasoning_effort,
                        "service_tier": service_tier,
                    })
                })
                .collect::<Vec<_>>();
            ProviderControlOption {
                value: group.base,
                label: group.label,
                metadata: BTreeMap::from([
                    ("relation_scope".to_owned(), json!("per_model")),
                    ("reasoning_efforts".to_owned(), json!(reasoning_efforts)),
                    ("service_tiers".to_owned(), json!(service_tiers)),
                    ("runtime_variants".to_owned(), json!(runtime_variants)),
                ]),
            }
        })
        .collect();
    Some(CursorCatalog {
        models,
        efforts,
        has_fast,
        default_model,
    })
}

fn split_variant(slug: &str) -> (String, String, String) {
    let (slug, tier) = slug
        .strip_suffix("-fast")
        .map_or((slug, "default"), |slug| (slug, "fast"));
    let mut parts = slug.split('-').collect::<Vec<_>>();
    let mut effort = Vec::new();
    while parts.last().is_some_and(|part| EFFORT_PARTS.contains(part)) {
        if let Some(part) = parts.pop() {
            effort.push(part);
        }
    }
    effort.reverse();
    (parts.join("-"), effort.join("-"), tier.to_owned())
}

fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= crate::catalog::MAX_OPTION_VALUE_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn effort_rank(value: &str) -> usize {
    [
        "", "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
    ]
    .iter()
    .position(|candidate| *candidate == value)
    .unwrap_or(usize::MAX)
}

fn effort_label(value: &str) -> &str {
    match value {
        "" => "기본",
        "none" => "None",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra High",
        "max" => "Max",
        "ultra" => "Ultra",
        _ => value,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{effective_model, parse_models};

    #[test]
    fn cursor_models_preserve_only_advertised_runtime_pairs() {
        let catalog = parse_models(
            "Available models\nauto - Auto\ngpt-5.6-sol-high - Sol High\n\
             gpt-5.6-sol-high-fast - Sol High Fast\ngpt-5.6-sol-xhigh-fast - Sol Extra High Fast\n\
             claude-opus-5-thinking-high - Claude Opus Thinking\n",
        )
        .unwrap_or_else(|| panic!("valid cursor catalog"));
        assert_eq!(catalog.default_model, "auto");
        let sol = catalog
            .models
            .iter()
            .find(|model| model.value == "gpt-5.6-sol")
            .unwrap_or_else(|| panic!("grouped sol model"));
        assert_eq!(sol.metadata["reasoning_efforts"], json!(["high", "xhigh"]));
        assert_eq!(sol.metadata["service_tiers"], json!(["default", "fast"]));
        assert_eq!(
            sol.metadata["runtime_variants"].as_array().map(Vec::len),
            Some(3)
        );
        assert_eq!(
            effective_model("gpt-5.6-sol", "xhigh", "fast").as_deref(),
            Some("gpt-5.6-sol-xhigh-fast")
        );
        assert_eq!(
            effective_model("claude-opus-5", "thinking-high", "default").as_deref(),
            Some("claude-opus-5-thinking-high")
        );
        let controls = catalog.controls();
        let permission = controls
            .iter()
            .find(|control| control.key == "permission_mode")
            .unwrap_or_else(|| panic!("Cursor permission control"));
        assert_eq!(permission.options.len(), 1);
        assert_eq!(permission.options[0].value, "meeting_read_only");
    }
}
