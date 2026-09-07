use std::collections::{BTreeMap, BTreeSet};

#[cfg(any(unix, windows))]
use agentsassemble_domain::DurableAgentSession;
use agentsassemble_domain::{ProviderAvailability, ProviderControlOption};
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::{
    catalog::{
        control, failed_provider, option, permission_control, provider_executable, ready_provider,
    },
    claude_sdk_assets::PrivateClaudeSdkBundle,
    process::{ProbeFailure, probe},
};
#[cfg(any(unix, windows))]
use crate::{
    claude_sdk_client::ClaudeSdkAttachment,
    claude_sdk_runtime::ClaudeSdkRuntime,
    driver::{
        DriverError, DriverFuture, ProviderDriver, ProviderSessionAttachment,
        ProviderTurnCompleted, ProviderTurnRequest,
    },
    launch_error::DriverLaunchError,
    room_portal::ProviderTurnOutcome,
};
#[cfg(unix)]
use crate::{guardian::GuardianLaunch, runtime_lease::HeldRuntimeLease};

const PREFERRED_MODEL: &str = "claude-haiku-4-5";
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

pub(crate) async fn discover(
    mut provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    let (claude, claude_identity) = match provider_executable("claude", cancellation).await {
        Ok(authority) => authority,
        Err(failure) => return failed_provider(provider, failure),
    };
    provider.executable.clone_from(&claude);
    provider.executable_identity = claude_identity;
    let (node, _) = match provider_executable("node", cancellation).await {
        Ok(authority) => authority,
        Err(failure) => return failed_provider(provider, failure),
    };
    let Ok(bundle) = PrivateClaudeSdkBundle::stage().await else {
        return failed_provider(provider, ProbeFailure::Failed);
    };
    let Some(bridge) = bundle.bridge.to_str() else {
        return failed_provider(provider, ProbeFailure::Malformed);
    };
    let Some(sdk) = bundle.sdk.to_str() else {
        return failed_provider(provider, ProbeFailure::Malformed);
    };
    let output = match probe(&node, &[bridge, sdk, &claude, "catalog"], cancellation, &[]).await {
        Ok(output) => output,
        Err(failure) => return failed_provider(provider, failure),
    };
    let Some(catalog) = parse_catalog(&output) else {
        return failed_provider(provider, ProbeFailure::Malformed);
    };
    ready_provider(provider, catalog.default_model.clone(), catalog.controls())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogEnvelope {
    r#type: String,
    models: Vec<CatalogModel>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogModel {
    id: String,
    label: String,
    efforts: Vec<String>,
    fast: bool,
}

struct ClaudeCatalog {
    models: Vec<ProviderControlOption>,
    efforts: Vec<String>,
    has_fast: bool,
    default_model: String,
}

impl ClaudeCatalog {
    fn controls(self) -> Vec<agentsassemble_domain::ProviderControl> {
        let default_effort = self
            .models
            .iter()
            .find(|model| model.value == self.default_model)
            .and_then(|model| model.metadata.get("reasoning_efforts"))
            .and_then(serde_json::Value::as_array)
            .and_then(|efforts| {
                efforts
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .find(|effort| *effort == "medium")
                    .or_else(|| efforts.iter().find_map(serde_json::Value::as_str))
            })
            .or_else(|| self.efforts.first().map(String::as_str))
            .unwrap_or("")
            .to_owned();
        let mut controls = vec![
            control(
                "model",
                "모델",
                "combobox",
                self.models,
                &self.default_model,
            ),
            control(
                "reasoning_effort",
                "추론 강도",
                "select",
                self.efforts
                    .iter()
                    .map(|effort| option(effort, effort_label(effort)))
                    .collect(),
                &default_effort,
            ),
        ];
        if self.has_fast {
            controls.push(control(
                "service_tier",
                "응답 속도",
                "select",
                vec![option("default", "기본"), option("fast", "Fast")],
                "default",
            ));
        }
        controls.push(permission_control(true));
        controls
    }
}

fn parse_catalog(output: &str) -> Option<ClaudeCatalog> {
    let envelope = serde_json::from_str::<CatalogEnvelope>(output.trim()).ok()?;
    if envelope.r#type != "catalog" {
        return None;
    }
    let mut seen_models = BTreeSet::new();
    let mut models = Vec::new();
    let mut observed_efforts = BTreeSet::new();
    let mut has_fast = false;
    for model in envelope.models {
        if !valid_model_id(&model.id)
            || model.label.trim().is_empty()
            || model.label.chars().any(char::is_control)
            || !seen_models.insert(model.id.clone())
        {
            return None;
        }
        let mut efforts = Vec::new();
        for effort in model.efforts {
            if !EFFORTS.contains(&effort.as_str()) || efforts.contains(&effort) {
                return None;
            }
            observed_efforts.insert(effort.clone());
            efforts.push(effort);
        }
        if efforts.is_empty() {
            return None;
        }
        has_fast |= model.fast;
        let service_tiers = if model.fast {
            vec!["default", "fast"]
        } else {
            vec!["default"]
        };
        models.push(ProviderControlOption {
            value: model.id,
            label: model.label,
            metadata: BTreeMap::from([
                ("relation_scope".to_owned(), json!("per_model")),
                ("reasoning_efforts".to_owned(), json!(efforts)),
                ("service_tiers".to_owned(), json!(service_tiers)),
            ]),
        });
    }
    if models.is_empty() {
        return None;
    }
    let efforts = EFFORTS
        .into_iter()
        .filter(|effort| observed_efforts.contains(*effort))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let default_model = models
        .iter()
        .find(|model| model.value == PREFERRED_MODEL)
        .map_or_else(String::new, |model| model.value.clone());
    Some(ClaudeCatalog {
        models,
        efforts,
        has_fast,
        default_model,
    })
}

#[cfg(any(unix, windows))]
pub(crate) struct ClaudeAgentSdkDriver {
    runtime: ClaudeSdkRuntime,
    attachment: Option<ClaudeSdkAttachment>,
}

#[cfg(any(unix, windows))]
impl ClaudeAgentSdkDriver {
    #[cfg(unix)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        runtime_lease: &HeldRuntimeLease,
        guardian: &GuardianLaunch,
    ) -> Result<Self, DriverLaunchError> {
        validate_profile(session).map_err(DriverLaunchError::safe)?;
        let (runtime, attachment) =
            ClaudeSdkRuntime::spawn(session, runtime_lease, guardian).await?;
        Ok(Self {
            runtime,
            attachment: Some(attachment),
        })
    }

    #[cfg(windows)]
    pub(crate) async fn spawn(session: &DurableAgentSession) -> Result<Self, DriverLaunchError> {
        validate_profile(session).map_err(DriverLaunchError::safe)?;
        let (runtime, attachment) = ClaudeSdkRuntime::spawn(session).await?;
        Ok(Self {
            runtime,
            attachment: Some(attachment),
        })
    }
}

#[cfg(any(unix, windows))]
impl ProviderDriver for ClaudeAgentSdkDriver {
    fn attach_session<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>> {
        Box::pin(async move {
            validate_profile(session)?;
            let attachment = self.attachment.take().ok_or_else(protocol_error)?;
            Ok(ProviderSessionAttachment {
                provider_session_id: attachment.session_id,
                reused: attachment.reused,
                observed_model_id: None,
            })
        })
    }

    fn send_turn<'a>(
        &'a mut self,
        _session: &'a DurableAgentSession,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnCompleted, DriverError>> {
        Box::pin(async move {
            let turn = self.runtime.turn(&request.turn_id, &request.input).await?;
            if request.room_observation.is_none() && turn.content.trim().is_empty() {
                return Err(protocol_error());
            }
            Ok(ProviderTurnCompleted {
                turn_id: request.turn_id.clone(),
                provider_turn_id: turn.provider_turn_id,
                provider_session_id: Some(turn.session_id),
                outcome: ProviderTurnOutcome::Message {
                    content: turn.content,
                    target_agent_id: String::new(),
                },
            })
        })
    }

    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>> {
        Box::pin(self.runtime.is_alive())
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(self.runtime.stop())
    }

    fn begin_room_observation(&mut self, request: &ProviderTurnRequest) -> Result<(), DriverError> {
        self.runtime
            .begin_observation(request)
            .map_err(DriverError::from)
    }

    fn finish_room_observation(
        &mut self,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnOutcome, DriverError> {
        self.runtime
            .finish_observation(request)
            .map_err(DriverError::from)
    }

    fn abort_room_observation(&mut self) -> Result<(), DriverError> {
        self.runtime.abort_observation().map_err(DriverError::from)
    }

    fn requires_restart(&self) -> bool {
        self.runtime.requires_restart()
    }
}

#[cfg(any(unix, windows))]
fn validate_profile(session: &DurableAgentSession) -> Result<(), DriverError> {
    if !valid_model_id(&session.public.model)
        || !EFFORTS.contains(&session.public.reasoning_effort.as_str())
        || !matches!(session.public.service_tier.as_str(), "default" | "fast")
        || !matches!(
            session.public.permission_mode.as_str(),
            "meeting_read_only" | "workspace_write"
        )
    {
        return Err(profile_error());
    }
    Ok(())
}

fn valid_model_id(value: &str) -> bool {
    let Some(rest) = [
        "claude-fable-",
        "claude-haiku-",
        "claude-opus-",
        "claude-sonnet-",
    ]
    .into_iter()
    .find_map(|prefix| value.strip_prefix(prefix)) else {
        return false;
    };
    let mut parts = rest.split('-');
    let valid_part =
        |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    parts.next().is_some_and(valid_part)
        && parts.next().is_none_or(valid_part)
        && parts.next().is_none()
}

fn effort_label(effort: &str) -> &str {
    match effort {
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "XHigh",
        "max" => "Max",
        _ => effort,
    }
}

#[cfg(any(unix, windows))]
const fn profile_error() -> DriverError {
    DriverError::new(
        "invalid_runtime_profile",
        "The Claude runtime profile is invalid.",
    )
}

#[cfg(any(unix, windows))]
const fn protocol_error() -> DriverError {
    DriverError::new(
        "provider_protocol_invalid",
        "Claude Agent SDK protocol failed.",
    )
}

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;
