//! Native ACP permissions retain exact room execution and flushed-response custody.
use super::{AcpPermissionPolicy, ProtocolState, delivery::Deliveries};
use crate::room_portal_tool_contract::PROVIDER_ROOM_TOOL_NAMES;
use crate::{
    ProviderRequestIngress,
    driver::{DriverError, ProviderTurnRequest},
};
use agent_client_protocol::{
    Responder,
    schema::v1::{
        PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
        RequestPermissionResponse, SelectedPermissionOutcome,
    },
};
use agentsassemble_domain::{
    ProviderRequest, ProviderRequestKind, ProviderRequestOption, ProviderRequestPrompt,
    ProviderRequestResolution, redact_persisted_diagnostic_text,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

/// Each provider selects its observed native identity contract; no shape guessing.
#[derive(Clone, Copy, Default)]
pub(crate) enum AcpToolIdentityContract {
    #[default]
    RequestToolName,
    QualifiedMcpCall,
}

pub(super) struct RequestTurn(Scope);
#[derive(Clone)]
struct Scope {
    session_id: String,
    generation: u64,
    execution_id: String,
    ingress: Option<ProviderRequestIngress>,
    cancelled: CancellationToken,
    handlers: TaskTracker,
}
impl RequestTurn {
    pub(super) fn new(session_id: &str, request: &ProviderTurnRequest) -> Self {
        Self(Scope {
            session_id: session_id.to_owned(),
            generation: request.turn_generation,
            execution_id: request.execution_id.clone(),
            ingress: request.request_ingress.clone(),
            cancelled: CancellationToken::new(),
            handlers: TaskTracker::new(),
        })
    }
    pub(super) fn cancel(&self) {
        self.0.cancelled.cancel();
    }
    pub(super) fn close(&self) -> TaskTracker {
        self.cancel();
        self.0.handlers.close();
        self.0.handlers.clone()
    }
}
impl Drop for RequestTurn {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub(super) async fn handle(
    state: &Arc<Mutex<ProtocolState>>,
    deliveries: &Deliveries,
    request: RequestPermissionRequest,
    responder: Responder<RequestPermissionResponse>,
) -> Result<(), agent_client_protocol::Error> {
    let automatic = permission_response(state, &request);
    let allowed = matches!(&automatic.outcome, RequestPermissionOutcome::Selected(selected)
        if request.options.iter().any(|option| option.option_id == selected.option_id
            && option.kind == PermissionOptionKind::AllowOnce));
    let scope = {
        let state = state
            .lock()
            .map_err(|_| agent_client_protocol::Error::internal_error())?;
        if matches!(
            state.permission_policy,
            super::AcpPermissionPolicy::RoomTools
        ) && state.session_id.as_ref() == Some(&request.session_id)
            && state.active_turn_id.is_some()
        {
            state
                .request_turn
                .as_ref()
                .map(|turn| (turn.0.clone(), turn.0.handlers.token()))
        } else {
            None
        }
    };
    let Some((scope, _handler)) = scope else {
        return responder.respond(automatic);
    };
    if allowed || scope.ingress.is_none() {
        return responder.respond(automatic);
    }
    let result = exchange(&scope, deliveries, &request, responder).await;
    if result.is_err()
        && let Ok(mut state) = state.lock()
        && state
            .request_turn
            .as_ref()
            .is_some_and(|turn| turn.0.execution_id == scope.execution_id)
    {
        state.request_failed = true;
    }
    result.map_err(|_| agent_client_protocol::Error::internal_error())
}

async fn exchange(
    scope: &Scope,
    deliveries: &Deliveries,
    request: &RequestPermissionRequest,
    responder: Responder<RequestPermissionResponse>,
) -> Result<(), DriverError> {
    let cancelled = responder.cancellation();
    let mapped = map_request(request)?;
    let ingress = scope.ingress.as_ref().ok_or_else(request_error)?;
    let opened = async {
        let mut exchange = ingress
            .open(
                &scope.session_id,
                scope.generation,
                &scope.execution_id,
                mapped,
            )
            .await
            .map_err(|_| request_error())?;
        let resolution = exchange.receive().await.map_err(|_| request_error())?;
        Ok::<_, DriverError>((exchange, resolution))
    };
    let (mut exchange, resolution) = tokio::select! {
        biased;
        () = scope.cancelled.cancelled() => return responder.respond(cancelled_response()).map_err(|_| request_error()),
        () = cancelled.cancelled() => return responder.respond(cancelled_response()).map_err(|_| request_error()),
        result = opened => result?,
    };
    let ProviderRequestResolution::Option { option_id } = resolution else {
        return Err(request_error());
    };
    let index = option_id
        .strip_prefix("option-")
        .and_then(|id| id.parse::<usize>().ok())
        .ok_or_else(request_error)?;
    let selected = request.options.get(index).ok_or_else(request_error)?;
    let id = serde_json::to_string(responder.id()).map_err(|_| request_error())?;
    let mut delivery = deliveries.register(id).map_err(|_| request_error())?;
    let queued = responder.respond(RequestPermissionResponse::new(
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            selected.option_id.clone(),
        )),
    ));
    let delivered = queued.is_ok() && delivery.receive().await;
    // Once emitted, await the actual write and durable receipt even if the native turn ends.
    exchange
        .complete(delivered)
        .await
        .map_err(|_| request_error())?;
    if delivered {
        Ok(())
    } else {
        Err(request_error())
    }
}

fn map_request(request: &RequestPermissionRequest) -> Result<ProviderRequest, DriverError> {
    let options = request
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            let (label, kind) = match option.kind {
                PermissionOptionKind::AllowOnce => ("Allow once", "allow_once"),
                PermissionOptionKind::AllowAlways => ("Always allow", "allow_always"),
                PermissionOptionKind::RejectOnce => ("Reject once", "reject_once"),
                PermissionOptionKind::RejectAlways => ("Always reject", "reject_always"),
                _ => return Err(request_error()),
            };
            Ok(ProviderRequestOption {
                id: format!("option-{index}"),
                label: label.to_owned(),
                kind: kind.to_owned(),
                description: String::new(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let request = ProviderRequest {
        provider_request_id: uuid::Uuid::new_v4(),
        request_kind: ProviderRequestKind::Permission,
        title: "Provider requests permission".to_owned(),
        description: redact_persisted_diagnostic_text(
            request
                .tool_call
                .fields
                .title
                .as_deref()
                .unwrap_or_default(),
            1200,
        ),
        timeout_seconds: 600,
        prompt: ProviderRequestPrompt::Option { options },
    };
    if request.is_valid() {
        Ok(request)
    } else {
        Err(request_error())
    }
}
fn cancelled_response() -> RequestPermissionResponse {
    RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
}
pub(super) const fn request_error() -> DriverError {
    DriverError::new(
        "provider_request_unavailable",
        "The ACP provider request could not complete.",
    )
}

pub(super) fn permission_response(
    state: &Mutex<ProtocolState>,
    request: &RequestPermissionRequest,
) -> RequestPermissionResponse {
    let Ok(state) = state.lock() else {
        return RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled);
    };
    if state.session_id.as_ref() != Some(&request.session_id) || state.active_turn_id.is_none() {
        return RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled);
    }
    let requested_tool = request
        .tool_call
        .fields
        .raw_input
        .as_ref()
        .and_then(|raw| room_tool_identity(state.tool_identity, raw));
    let cached_tool = state
        .active_tools
        .get(&request.tool_call.tool_call_id.to_string())
        .map(String::as_str);
    let identity_confirmed = match state.tool_identity {
        AcpToolIdentityContract::RequestToolName => {
            requested_tool.is_some()
                && cached_tool != Some("")
                && cached_tool.is_none_or(|cached| Some(cached) == requested_tool)
        }
        AcpToolIdentityContract::QualifiedMcpCall => {
            // ACP permission toolCall is an update referring to the same call ID.
            // Cursor announces qualified MCP identity before that request, but does
            // not repeat it there. Only this configured native contract uses it.
            cached_tool.is_some_and(|cached| !cached.is_empty())
                && request
                    .tool_call
                    .fields
                    .raw_input
                    .as_ref()
                    .is_none_or(|raw| {
                        raw.as_object().is_some_and(serde_json::Map::is_empty)
                            || requested_tool == cached_tool
                    })
        }
    };
    let allow = matches!(state.permission_policy, AcpPermissionPolicy::RoomTools)
        && state.room_observation_active
        && identity_confirmed;
    let selected = if allow {
        request
            .options
            .iter()
            .find(|option| option.kind == PermissionOptionKind::AllowOnce)
    } else {
        request
            .options
            .iter()
            .find(|option| option.kind == PermissionOptionKind::RejectOnce)
            .or_else(|| {
                request
                    .options
                    .iter()
                    .find(|option| option.kind == PermissionOptionKind::RejectAlways)
            })
    };
    selected.map_or_else(
        || RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled),
        |option| {
            RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new(option.option_id.clone()),
            ))
        },
    )
}

pub(super) fn record_tool_identity(
    contract: AcpToolIdentityContract,
    active_tools: &mut HashMap<String, String>,
    tool_call_id: String,
    raw_input: Option<&serde_json::Value>,
) {
    let Some(tool) = raw_input.and_then(|raw| room_tool_identity(contract, raw)) else {
        if matches!(contract, AcpToolIdentityContract::QualifiedMcpCall)
            && raw_input.is_some_and(|raw| {
                !raw.is_null() && !raw.as_object().is_some_and(serde_json::Map::is_empty)
            })
        {
            // A nonempty replacement with unknown/foreign identity invalidates
            // this call permanently; later updates cannot revive it.
            active_tools.insert(tool_call_id, String::new());
        }
        return;
    };
    match active_tools.entry(tool_call_id) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(tool.to_owned());
        }
        std::collections::hash_map::Entry::Occupied(mut entry) if entry.get() != tool => {
            entry.insert(String::new());
        }
        std::collections::hash_map::Entry::Occupied(_) => {}
    }
}

fn room_tool_identity(
    contract: AcpToolIdentityContract,
    raw_input: &serde_json::Value,
) -> Option<&str> {
    if matches!(contract, AcpToolIdentityContract::QualifiedMcpCall) {
        if raw_input.get("providerIdentifier")?.as_str()? != "agentsassemble_room" {
            return None;
        }
        let name = raw_input.get("toolName")?.as_str()?;
        return PROVIDER_ROOM_TOOL_NAMES.contains(&name).then_some(name);
    }
    let name = raw_input.get("tool_name")?.as_str()?;
    let bare = name
        .strip_prefix("agentsassemble_room__")
        .or_else(|| name.strip_prefix("agentsassemble_room_"))
        .unwrap_or(name);
    PROVIDER_ROOM_TOOL_NAMES.contains(&bare).then_some(bare)
}
