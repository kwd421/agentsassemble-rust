use std::{collections::HashSet, time::Duration};

use agentsassemble_domain::{DurableAgentSession, MAX_MESSAGE_CHARACTERS};
use reqwest::{Client, Url};
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, CallToolResult, Tool},
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};

use crate::{
    credentials::{ProviderCredential, ProviderCredentialStore},
    driver::ROOM_PORTAL_UNAVAILABLE,
    driver::{
        DriverError, DriverFuture, ProviderDriver, ProviderSessionAttachment,
        ProviderTurnCompleted, ProviderTurnRequest,
    },
    launch_cleanup,
    launch_error::DriverLaunchError,
    local_openai::fixed_loopback_completion,
    openai_stream::{AssistantMessage, OpenAiStreamCompletion, ToolCall, send_chat_completion},
    remote_https::fixed_endpoint_client,
    remote_openai_spec::{
        RemoteOpenAiAuthentication, RemoteOpenAiEndpoint, RemoteOpenAiSpec, ResponseModelIdentity,
        provider_error,
    },
    room_portal::{ProviderTurnOutcome, RoomPortal},
};

const MAX_TOOL_RESULT_BYTES: usize = 128 * 1024;
const MAX_TOOL_ROUNDS: usize = 16;
const PORTAL_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

type PortalClient = RunningService<RoleClient, ()>;

enum PortalClientState {
    Active(PortalClient),
    Closed,
    CleanupUnconfirmed,
}

impl PortalClientState {
    const fn active(&self) -> Option<&PortalClient> {
        match self {
            Self::Active(client) => Some(client),
            Self::Closed | Self::CleanupUnconfirmed => None,
        }
    }
}

pub(crate) struct RemoteOpenAiDriver {
    spec: &'static RemoteOpenAiSpec,
    api: RemoteOpenAiApi,
    portal: Option<RoomPortal>,
    portal_client: PortalClientState,
    tools: Vec<Tool>,
    credentials: ProviderCredentialStore,
    attached_session_id: Option<String>,
    turn_effect_uncertain: bool,
    stopped: bool,
    portal_failed: bool,
}

struct RemoteOpenAiApi {
    spec: &'static RemoteOpenAiSpec,
    client: Client,
    endpoint: Url,
    session_endpoint_authority: Option<String>,
}

struct ExecutedTool {
    call: ToolCall,
    result: String,
    terminal: bool,
}

impl RemoteOpenAiDriver {
    pub(crate) async fn launch(
        spec: &'static RemoteOpenAiSpec,
        credentials: ProviderCredentialStore,
    ) -> Result<Self, DriverLaunchError> {
        let (client, endpoint) = match (spec.endpoint, spec.authentication) {
            (RemoteOpenAiEndpoint::Fixed(endpoint), RemoteOpenAiAuthentication::Bearer { .. }) => {
                let client = fixed_endpoint_client().map_err(|_| {
                    provider_error("provider_api_unavailable", spec.errors.api_unavailable)
                })?;
                let endpoint = Url::parse(endpoint).map_err(|_| {
                    provider_error("provider_api_unavailable", spec.errors.api_unavailable)
                })?;
                (client, endpoint)
            }
            (
                RemoteOpenAiEndpoint::FixedLoopback(endpoint),
                RemoteOpenAiAuthentication::Unauthenticated,
            ) => fixed_loopback_completion(endpoint).map_err(|_| {
                provider_error("provider_api_unavailable", spec.errors.api_unavailable)
            })?,
            _ => {
                return Err(provider_error(
                    "provider_api_unavailable",
                    spec.errors.api_unavailable,
                )
                .into());
            }
        };
        Self::launch_with_api(
            spec,
            credentials,
            RemoteOpenAiApi {
                spec,
                client,
                endpoint,
                session_endpoint_authority: None,
            },
        )
        .await
    }

    pub(crate) async fn launch_for_session_endpoint(
        spec: &'static RemoteOpenAiSpec,
        credentials: ProviderCredentialStore,
        session_endpoint_authority: String,
        client: Client,
        endpoint: Url,
    ) -> Result<Self, DriverLaunchError> {
        if spec.endpoint != RemoteOpenAiEndpoint::AgentSession
            || !matches!(
                spec.authentication,
                RemoteOpenAiAuthentication::Bearer { .. }
            )
        {
            return Err(
                provider_error("provider_api_unavailable", spec.errors.api_unavailable).into(),
            );
        }
        Self::launch_with_api(
            spec,
            credentials,
            RemoteOpenAiApi {
                spec,
                client,
                endpoint,
                session_endpoint_authority: Some(session_endpoint_authority),
            },
        )
        .await
    }

    async fn launch_with_api(
        spec: &'static RemoteOpenAiSpec,
        credentials: ProviderCredentialStore,
        api: RemoteOpenAiApi,
    ) -> Result<Self, DriverLaunchError> {
        let mut portal = RoomPortal::create().await.map_err(DriverError::from)?;
        let portal_client = ().serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(portal.endpoint())
                .auth_header(portal.bearer_token()),
        ));
        let Ok(mut portal_client) = portal_client.await else {
            let error = failed_remote_launch(&mut portal, None, ROOM_PORTAL_UNAVAILABLE).await;
            return Err(error);
        };
        let Ok(tools) = portal_client.list_all_tools().await else {
            let error = failed_remote_launch(
                &mut portal,
                Some(&mut portal_client),
                ROOM_PORTAL_UNAVAILABLE,
            )
            .await;
            return Err(error);
        };
        if let Err(error) = validate_tool_catalog(&tools) {
            return Err(failed_remote_launch(&mut portal, Some(&mut portal_client), error).await);
        }
        Ok(Self {
            spec,
            api,
            portal: Some(portal),
            portal_client: PortalClientState::Active(portal_client),
            tools,
            credentials,
            attached_session_id: None,
            turn_effect_uncertain: false,
            stopped: false,
            portal_failed: false,
        })
    }

    async fn run_turn(
        &mut self,
        session: &DurableAgentSession,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnCompleted, DriverError> {
        self.turn_effect_uncertain = false;
        self.validate_session(session)?;
        let credential = self.load_credential().await?;
        let observation = request.room_observation.as_ref();
        let tools =
            observation.map(|observation| api_tools(&self.tools, observation.tabletop_tools));
        let mut messages = vec![json!({"role": "user", "content": request.input})];
        for round in 0..=MAX_TOOL_ROUNDS {
            let response = self
                .api
                .complete(session, credential.as_ref(), &messages, tools.as_deref())
                .await?;
            validate_completion(&response, &session.public.model, self.spec)?;
            let provider_turn_id = response.id.clone();
            let mut message = response.message;
            if message.tool_calls.is_empty() {
                if observation.is_some() {
                    return Err(provider_error(
                        "provider_room_action_missing",
                        self.spec.errors.room_action_missing,
                    ));
                }
                let content = message
                    .content
                    .as_deref()
                    .and_then(canonical_content)
                    .ok_or_else(|| {
                        provider_error(
                            "provider_protocol_invalid",
                            self.spec.errors.invalid_response,
                        )
                    })?;
                return Ok(completed(request, provider_turn_id, content));
            }
            if round == MAX_TOOL_ROUNDS {
                return Err(provider_error(
                    "provider_tool_round_limit",
                    self.spec.errors.tool_round_limit,
                ));
            }
            let forced_read = round == 0 && observation.is_some();
            if forced_read {
                let read = message
                    .tool_calls
                    .iter()
                    .find(|call| call.function.name == "read_discussion")
                    .cloned()
                    .ok_or_else(|| {
                        provider_error(
                            "provider_room_read_missing",
                            self.spec.errors.room_read_missing,
                        )
                    })?;
                message.tool_calls = vec![read.clone()];
                let executed = self.execute_tool(read, false).await?;
                if executed.terminal {
                    return Err(provider_error(
                        "provider_protocol_invalid",
                        self.spec.errors.invalid_response,
                    ));
                }
                messages.push(assistant_value(&message, self.spec.retain_reasoning));
                messages.push(tool_value(&executed));
                continue;
            }
            let mut executed = Vec::new();
            for call in message.tool_calls.iter().cloned() {
                let result = self
                    .execute_tool(call, observation.is_some_and(|value| value.tabletop_tools))
                    .await?;
                let terminal = result.terminal;
                executed.push(result);
                if terminal {
                    break;
                }
            }
            message.tool_calls = executed.iter().map(|item| item.call.clone()).collect();
            messages.push(assistant_value(&message, self.spec.retain_reasoning));
            messages.extend(executed.iter().map(tool_value));
            if executed.last().is_some_and(|item| item.terminal) {
                return Ok(completed(
                    request,
                    provider_turn_id,
                    "Room Portal action completed.".to_owned(),
                ));
            }
        }
        Err(provider_error(
            "provider_protocol_invalid",
            self.spec.errors.invalid_response,
        ))
    }

    async fn load_credential(&self) -> Result<Option<ProviderCredential>, DriverError> {
        let Some(credential) = self.spec.credential_id() else {
            return Ok(None);
        };
        self.credentials
            .secret(credential)
            .await
            .map(Some)
            .map_err(|error| self.spec.credential_error(error))
    }
    async fn execute_tool(
        &mut self,
        call: ToolCall,
        random_tools: bool,
    ) -> Result<ExecutedTool, DriverError> {
        if !crate::room_portal::is_available_provider_tool(&call.function.name, random_tools) {
            return Err(provider_error(
                "provider_tool_call_invalid",
                self.spec.errors.invalid_tool_call,
            ));
        }
        let arguments = serde_json::from_str::<Map<String, Value>>(&call.function.arguments)
            .map_err(|_| {
                provider_error(
                    "provider_tool_call_invalid",
                    self.spec.errors.invalid_tool_call,
                )
            })?;
        let terminal_action = crate::room_portal::is_terminal_provider_tool(&call.function.name);
        let replay_unsafe = crate::room_portal::is_replay_unsafe_provider_tool(&call.function.name);
        let previous_effect_uncertain = self.turn_effect_uncertain;
        if replay_unsafe {
            self.turn_effect_uncertain = true;
        }
        let client = self.portal_client.active().ok_or(ROOM_PORTAL_UNAVAILABLE)?;
        let result = client
            .call_tool(
                CallToolRequestParams::new(call.function.name.clone()).with_arguments(arguments),
            )
            .await
            .map_err(|_| {
                self.portal_failed = true;
                ROOM_PORTAL_UNAVAILABLE
            })?;
        if replay_unsafe && result.is_error == Some(true) {
            self.turn_effect_uncertain = previous_effect_uncertain;
        }
        let terminal = terminal_action && result.is_error != Some(true);
        Ok(ExecutedTool {
            call,
            result: tool_result_text(&result, self.spec)?,
            terminal,
        })
    }

    fn validate_session(&self, session: &DurableAgentSession) -> Result<(), DriverError> {
        let endpoint_matches = match self.spec.endpoint {
            RemoteOpenAiEndpoint::Fixed(_) | RemoteOpenAiEndpoint::FixedLoopback(_) => {
                session.provider_endpoint.is_empty()
                    && self.api.session_endpoint_authority.is_none()
            }
            RemoteOpenAiEndpoint::AgentSession => self
                .api
                .session_endpoint_authority
                .as_deref()
                .is_some_and(|authority| authority == session.provider_endpoint),
        };
        if self.stopped
            || self.attached_session_id.as_deref() != Some(&session.public.session_id)
            || session.public.provider_kind != self.spec.provider_kind
            || session.public.runtime_kind != "api"
            || session.public.transport != self.spec.endpoint.transport()
            || session.public.permission_mode != "meeting_read_only"
            || !endpoint_matches
        {
            return Err(provider_error(
                "provider_session_mismatch",
                self.spec.errors.session_mismatch,
            ));
        }
        Ok(())
    }
}

impl ProviderDriver for RemoteOpenAiDriver {
    fn attach_session<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>> {
        Box::pin(async move {
            if self.stopped || self.portal_failed {
                return Err(ROOM_PORTAL_UNAVAILABLE);
            }
            let namespace = self
                .spec
                .credential_id()
                .map_or(self.spec.provider_kind, |credential| credential.as_str());
            let provider_session_id = format!("{namespace}-{}", session.public.session_id);
            if !session.provider_session_id.is_empty()
                && session.provider_session_id != provider_session_id
            {
                return Err(provider_error(
                    "provider_session_mismatch",
                    self.spec.errors.session_changed,
                ));
            }
            match self.attached_session_id.as_deref() {
                Some(attached) if attached == session.public.session_id => {}
                Some(_) => {
                    return Err(provider_error(
                        "provider_session_mismatch",
                        self.spec.errors.already_bound,
                    ));
                }
                None => {
                    self.attached_session_id = Some(session.public.session_id.clone());
                }
            }
            Ok(ProviderSessionAttachment {
                provider_session_id,
                reused: false,
                observed_model_id: None,
            })
        })
    }

    fn send_turn<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnCompleted, DriverError>> {
        Box::pin(self.run_turn(session, request))
    }

    fn interrupt_turn<'a>(
        &'a mut self,
        _session: &'a DurableAgentSession,
        _request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<(), DriverError>> {
        Box::pin(async move {
            if self.turn_effect_uncertain {
                Err(provider_error(
                    "provider_turn_interrupt_uncertain",
                    self.spec.errors.interrupt_uncertain,
                ))
            } else {
                Ok(())
            }
        })
    }

    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>> {
        Box::pin(async move {
            Ok(!self.stopped
                && !self.portal_failed
                && self.portal.as_ref().is_some_and(RoomPortal::is_running)
                && self.portal_client.active().is_some())
        })
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(async move {
            self.stopped = true;
            self.attached_session_id = None;
            let client = match std::mem::replace(&mut self.portal_client, PortalClientState::Closed)
            {
                PortalClientState::Active(mut client) => {
                    match client.close_with_timeout(PORTAL_CLOSE_TIMEOUT).await {
                        Ok(Some(_)) => Ok(()),
                        Ok(None) => {
                            self.portal_client = PortalClientState::CleanupUnconfirmed;
                            Err(ROOM_PORTAL_UNAVAILABLE)
                        }
                        Err(_) => Err(ROOM_PORTAL_UNAVAILABLE),
                    }
                }
                PortalClientState::CleanupUnconfirmed => {
                    self.portal_client = PortalClientState::CleanupUnconfirmed;
                    Err(ROOM_PORTAL_UNAVAILABLE)
                }
                PortalClientState::Closed => Ok(()),
            };
            let portal = if let Some(portal) = self.portal.as_mut() {
                portal.shutdown().await.map_err(DriverError::from)
            } else {
                Ok(())
            };
            if portal.is_ok() {
                self.portal = None;
            }
            client.and(portal)
        })
    }

    fn begin_room_observation(&mut self, request: &ProviderTurnRequest) -> Result<(), DriverError> {
        self.portal
            .as_ref()
            .ok_or(ROOM_PORTAL_UNAVAILABLE)?
            .begin_turn(request)
            .map_err(DriverError::from)
    }

    fn finish_room_observation(
        &mut self,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnOutcome, DriverError> {
        self.portal
            .as_ref()
            .ok_or(ROOM_PORTAL_UNAVAILABLE)?
            .finish_turn(request)
            .map_err(DriverError::from)
    }

    fn abort_room_observation(&mut self) -> Result<(), DriverError> {
        if let Some(portal) = self.portal.as_ref() {
            portal.end_observation().map_err(DriverError::from)?;
        }
        Ok(())
    }

    fn requires_restart(&self) -> bool {
        self.stopped || self.portal_failed
    }

    fn turn_failure_effect_uncertain(&self) -> bool {
        self.turn_effect_uncertain
    }
}

async fn failed_remote_launch(
    portal: &mut RoomPortal,
    client: Option<&mut PortalClient>,
    failure: DriverError,
) -> DriverLaunchError {
    let client = if let Some(client) = client {
        match client.close_with_timeout(PORTAL_CLOSE_TIMEOUT).await {
            Ok(Some(_)) => Ok(()),
            Ok(None) | Err(_) => Err(launch_cleanup::unconfirmed()),
        }
    } else {
        Ok(())
    };
    launch_cleanup::owned_and_portal(portal, client, DriverLaunchError::safe(failure)).await
}

impl RemoteOpenAiApi {
    async fn complete(
        &self,
        session: &DurableAgentSession,
        credential: Option<&ProviderCredential>,
        messages: &[Value],
        tools: Option<&[Value]>,
    ) -> Result<OpenAiStreamCompletion, DriverError> {
        let payload = (self.spec.request_payload)(session, messages, tools);
        let body = serde_json::to_vec(&payload).map_err(|_| {
            provider_error(
                "provider_protocol_invalid",
                self.spec.errors.invalid_response,
            )
        })?;
        let mut request = self.client.post(self.endpoint.clone());
        match (self.spec.authentication, credential) {
            (RemoteOpenAiAuthentication::Bearer { .. }, Some(credential)) => {
                request = request.bearer_auth(credential.expose());
            }
            (RemoteOpenAiAuthentication::Unauthenticated, None) => {}
            _ => {
                return Err(provider_error(
                    "provider_protocol_invalid",
                    self.spec.errors.invalid_response,
                ));
            }
        }
        for (name, value) in self.spec.headers {
            request = request.header(*name, *value);
        }
        send_chat_completion(request, body)
            .await
            .map_err(|error| self.spec.stream_error(error))
    }
}

fn api_tools(tools: &[Tool], random_tools: bool) -> Vec<Value> {
    tools
        .iter()
        .filter(|tool| {
            crate::room_portal::is_available_provider_tool(tool.name.as_ref(), random_tools)
        })
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                }
            })
        })
        .collect()
}

fn validate_tool_catalog(tools: &[Tool]) -> Result<(), DriverError> {
    let names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<HashSet<_>>();
    crate::room_portal::PROVIDER_ROOM_TOOL_NAMES
        .into_iter()
        .all(|name| names.contains(name))
        .then_some(())
        .ok_or(ROOM_PORTAL_UNAVAILABLE)
}

fn validate_completion(
    response: &OpenAiStreamCompletion,
    expected_model: &str,
    spec: &RemoteOpenAiSpec,
) -> Result<(), DriverError> {
    if spec.response_model == ResponseModelIdentity::Requested && response.model != expected_model {
        return Err(provider_error(
            "provider_protocol_invalid",
            spec.errors.invalid_response,
        ));
    }
    let has_tools = !response.message.tool_calls.is_empty();
    if (has_tools && response.finish_reason != "tool_calls")
        || (!has_tools && response.finish_reason != "stop")
    {
        return Err(provider_error(
            "provider_protocol_invalid",
            spec.errors.invalid_response,
        ));
    }
    Ok(())
}

fn tool_result_text(
    result: &CallToolResult,
    spec: &RemoteOpenAiSpec,
) -> Result<String, DriverError> {
    let invalid = || provider_error("provider_tool_call_invalid", spec.errors.invalid_tool_call);
    let text = if result.is_error == Some(true) {
        "{\"ok\":false,\"error\":{\"code\":\"room_tool_rejected\"}}".to_owned()
    } else {
        result
            .content
            .iter()
            .map(|content| content.as_text().map(|text| text.text.as_str()))
            .collect::<Option<Vec<_>>>()
            .map(|parts| parts.join("\n"))
            .filter(|text| !text.is_empty())
            .ok_or_else(invalid)?
    };
    (text.len() <= MAX_TOOL_RESULT_BYTES)
        .then_some(text)
        .ok_or_else(invalid)
}

fn assistant_value(message: &AssistantMessage, retain_reasoning: bool) -> Value {
    let mut value = json!({
        "role": "assistant",
        "content": message.content.as_deref().unwrap_or_default(),
        "tool_calls": message.tool_calls,
    });
    if retain_reasoning {
        value["reasoning_content"] = json!(message.reasoning_content);
    }
    value
}

fn tool_value(executed: &ExecutedTool) -> Value {
    json!({
        "role": "tool",
        "tool_call_id": executed.call.id,
        "name": executed.call.function.name,
        "content": executed.result,
    })
}

fn completed(
    request: &ProviderTurnRequest,
    provider_turn_id: String,
    content: String,
) -> ProviderTurnCompleted {
    ProviderTurnCompleted {
        turn_id: request.turn_id.clone(),
        provider_turn_id,
        provider_session_id: None,
        outcome: ProviderTurnOutcome::Message {
            content,
            target_agent_id: String::new(),
        },
    }
}

fn canonical_content(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.chars().count() <= MAX_MESSAGE_CHARACTERS).then(|| value.to_owned())
}

#[cfg(test)]
#[path = "deepseek_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "remote_openai_tests.rs"]
mod contract_tests;
