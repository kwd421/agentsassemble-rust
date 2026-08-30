use std::{collections::HashSet, time::Duration};

use agentsassemble_domain::DurableAgentSession;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode, Url};
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, CallToolResult, Tool},
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    credentials::{DeepSeekCredential, ProviderCredentialStore, deepseek_credential_error},
    driver::{
        DriverError, DriverFuture, ProviderDriver, ProviderSessionAttachment,
        ProviderTurnCompleted, ProviderTurnRequest,
    },
    remote_https::direct_client,
    room_portal::{ProviderTurnOutcome, RoomObservationStart, RoomPortal},
};

const DEEPSEEK_API_HOST: &str = "api.deepseek.com";
const CHAT_COMPLETIONS_URL: &str = "https://api.deepseek.com/chat/completions";
const MAX_REQUEST_BYTES: usize = 256_000 - 16_384 - 32_768;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_TOOL_ARGUMENT_BYTES: usize = 64 * 1024;
const MAX_TOOL_RESULT_BYTES: usize = 128 * 1024;
const MAX_TOOL_ROUNDS: usize = 16;
const PORTAL_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const INVALID_RESPONSE: DriverError = DriverError::new(
    "provider_protocol_invalid",
    "DeepSeek returned an invalid bounded response.",
);
const INVALID_TOOL_CALL: DriverError = DriverError::new(
    "provider_tool_call_invalid",
    "DeepSeek returned an invalid room-tool call.",
);
const API_UNAVAILABLE: DriverError = DriverError::new(
    "provider_api_unavailable",
    "The DeepSeek API request did not complete.",
);
const PORTAL_UNAVAILABLE: DriverError = DriverError::new(
    "room_portal_unavailable",
    "The server-owned room portal is unavailable.",
);

type PortalClient = RunningService<RoleClient, ()>;

pub(crate) struct DeepSeekDriver {
    api: DeepSeekApi,
    portal: Option<RoomPortal>,
    portal_client: Option<PortalClient>,
    tools: Vec<Tool>,
    credentials: ProviderCredentialStore,
    attached_session_id: Option<String>,
    turn_effect_uncertain: bool,
    stopped: bool,
    portal_failed: bool,
}

struct DeepSeekApi {
    client: Client,
    endpoint: Url,
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    id: String,
    model: String,
    choices: Vec<CompletionChoice>,
}

#[derive(Debug, Deserialize)]
struct CompletionChoice {
    index: u32,
    finish_reason: String,
    message: AssistantMessage,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AssistantMessage {
    role: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: ToolFunction,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ToolFunction {
    name: String,
    arguments: String,
}

struct ExecutedTool {
    call: ToolCall,
    result: String,
    terminal: bool,
}

impl DeepSeekDriver {
    pub(crate) async fn launch(credentials: ProviderCredentialStore) -> Result<Self, DriverError> {
        let portal = RoomPortal::create().await.map_err(|_| PORTAL_UNAVAILABLE)?;
        let portal_client = ().serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(portal.endpoint())
                .auth_header(portal.bearer_token()),
        ));
        let portal_client = portal_client.await.map_err(|_| PORTAL_UNAVAILABLE)?;
        let tools = portal_client
            .list_all_tools()
            .await
            .map_err(|_| PORTAL_UNAVAILABLE)?;
        validate_tool_catalog(&tools)?;
        Ok(Self {
            api: DeepSeekApi::new()?,
            portal: Some(portal),
            portal_client: Some(portal_client),
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
        let credential = self
            .credentials
            .deepseek_secret()
            .await
            .map_err(deepseek_credential_error)?;
        let observation = request.room_observation.as_ref();
        let tools =
            observation.map(|observation| api_tools(&self.tools, observation.tabletop_tools));
        let mut messages = vec![json!({"role": "user", "content": request.input})];
        for round in 0..=MAX_TOOL_ROUNDS {
            let response = self
                .api
                .complete(session, &credential, &messages, tools.as_deref())
                .await?;
            validate_completion(&response, &session.public.model)?;
            let provider_turn_id = response.id.clone();
            let mut message = response
                .choices
                .into_iter()
                .next()
                .ok_or(INVALID_RESPONSE)?
                .message;
            validate_tool_calls(&message.tool_calls)?;
            if message.tool_calls.is_empty() {
                if observation.is_some() {
                    return Err(DriverError::new(
                        "provider_room_action_missing",
                        "DeepSeek ended without the required room action.",
                    ));
                }
                let content = message
                    .content
                    .as_deref()
                    .and_then(canonical_content)
                    .ok_or(INVALID_RESPONSE)?;
                return Ok(completed(request, provider_turn_id, content));
            }
            if round == MAX_TOOL_ROUNDS {
                return Err(DriverError::new(
                    "provider_tool_round_limit",
                    "DeepSeek exceeded the bounded room-tool rounds.",
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
                        DriverError::new(
                            "provider_room_read_missing",
                            "DeepSeek did not perform the required room read.",
                        )
                    })?;
                message.tool_calls = vec![read.clone()];
                let executed = self.execute_tool(read, false).await?;
                if executed.terminal {
                    return Err(INVALID_RESPONSE);
                }
                messages.push(assistant_value(&message));
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
            messages.push(assistant_value(&message));
            messages.extend(executed.iter().map(tool_value));
            if executed.last().is_some_and(|item| item.terminal) {
                return Ok(completed(
                    request,
                    provider_turn_id,
                    "Room Portal action completed.".to_owned(),
                ));
            }
        }
        Err(INVALID_RESPONSE)
    }
    async fn execute_tool(
        &mut self,
        call: ToolCall,
        random_tools: bool,
    ) -> Result<ExecutedTool, DriverError> {
        if !allowed_tool(&call.function.name, random_tools) {
            return Err(INVALID_TOOL_CALL);
        }
        let arguments = serde_json::from_str::<Map<String, Value>>(&call.function.arguments)
            .map_err(|_| INVALID_TOOL_CALL)?;
        let terminal_action = matches!(
            call.function.name.as_str(),
            "publish_message" | "decline_to_speak"
        );
        let replay_unsafe =
            terminal_action || matches!(call.function.name.as_str(), "roll_dice" | "choose_random");
        let previous_effect_uncertain = self.turn_effect_uncertain;
        if replay_unsafe {
            self.turn_effect_uncertain = true;
        }
        let client = self.portal_client.as_ref().ok_or(PORTAL_UNAVAILABLE)?;
        let result = client
            .call_tool(
                CallToolRequestParams::new(call.function.name.clone()).with_arguments(arguments),
            )
            .await
            .map_err(|_| {
                self.portal_failed = true;
                PORTAL_UNAVAILABLE
            })?;
        if replay_unsafe && result.is_error == Some(true) {
            self.turn_effect_uncertain = previous_effect_uncertain;
        }
        let terminal = terminal_action && result.is_error != Some(true);
        Ok(ExecutedTool {
            call,
            result: tool_result_text(&result)?,
            terminal,
        })
    }

    fn validate_session(&self, session: &DurableAgentSession) -> Result<(), DriverError> {
        if self.stopped
            || self.attached_session_id.as_deref() != Some(&session.public.session_id)
            || session.public.provider_kind != "deepseek_api"
            || session.public.runtime_kind != "api"
            || session.public.transport != "https"
            || session.public.permission_mode != "meeting_read_only"
        {
            return Err(DriverError::new(
                "provider_session_mismatch",
                "DeepSeek runtime authority does not match the Agent Session.",
            ));
        }
        Ok(())
    }
}

impl ProviderDriver for DeepSeekDriver {
    fn attach_session<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>> {
        Box::pin(async move {
            if self.stopped || self.portal_failed {
                return Err(PORTAL_UNAVAILABLE);
            }
            let provider_session_id = format!("deepseek-{}", session.public.session_id);
            if !session.provider_session_id.is_empty()
                && session.provider_session_id != provider_session_id
            {
                return Err(DriverError::new(
                    "provider_session_mismatch",
                    "DeepSeek session authority changed after attachment.",
                ));
            }
            match self.attached_session_id.as_deref() {
                Some(attached) if attached == session.public.session_id => {}
                Some(_) => {
                    return Err(DriverError::new(
                        "provider_session_mismatch",
                        "DeepSeek driver is already bound to another Agent Session.",
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
                Err(DriverError::new(
                    "provider_turn_interrupt_uncertain",
                    "The DeepSeek room action may have completed before interruption.",
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
                && self.portal_client.is_some())
        })
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(async move {
            self.stopped = true;
            self.attached_session_id = None;
            let closed = if let Some(client) = self.portal_client.as_mut() {
                client
                    .close_with_timeout(PORTAL_CLOSE_TIMEOUT)
                    .await
                    .map_err(|_| PORTAL_UNAVAILABLE)?
                    .is_some()
            } else {
                true
            };
            self.portal_client = None;
            self.portal = None;
            closed.then_some(()).ok_or(PORTAL_UNAVAILABLE)
        })
    }

    fn begin_room_observation(&mut self, request: &ProviderTurnRequest) -> Result<(), DriverError> {
        let observation = request.room_observation.as_ref().ok_or(INVALID_RESPONSE)?;
        self.portal
            .as_ref()
            .ok_or(PORTAL_UNAVAILABLE)?
            .begin_observation(RoomObservationStart {
                session_id: &observation.session_id,
                turn_id: &request.turn_id,
                input_up_to_seq: observation.input_up_to_seq,
                durable_turn_generation: request.turn_generation,
                execution_id: &request.execution_id,
                room_view: &observation.view,
                attachment_ids: &observation.attachment_ids,
                attachment_ingress: observation.attachment_ingress.clone(),
                allowed_agent_ids: &observation.allowed_agent_ids,
                tabletop_tools: observation.tabletop_tools,
                tool_ingress: observation.room_tool_ingress.clone(),
            })
            .map_err(|_| PORTAL_UNAVAILABLE)
    }

    fn finish_room_observation(
        &mut self,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnOutcome, DriverError> {
        let observation = request.room_observation.as_ref().ok_or(INVALID_RESPONSE)?;
        self.portal
            .as_ref()
            .ok_or(PORTAL_UNAVAILABLE)?
            .finish_observation(&request.turn_id, observation.input_up_to_seq)
            .map_err(|_| PORTAL_UNAVAILABLE)
    }

    fn abort_room_observation(&mut self) {
        if let Some(portal) = self.portal.as_ref() {
            let _ = portal.end_observation();
        }
    }

    fn requires_restart(&self) -> bool {
        self.stopped || self.portal_failed
    }

    fn turn_failure_effect_uncertain(&self) -> bool {
        self.turn_effect_uncertain
    }
}

impl DeepSeekApi {
    fn new() -> Result<Self, DriverError> {
        let client = direct_client(DEEPSEEK_API_HOST).map_err(|_| API_UNAVAILABLE)?;
        let endpoint = Url::parse(CHAT_COMPLETIONS_URL).map_err(|_| API_UNAVAILABLE)?;
        Ok(Self { client, endpoint })
    }

    async fn complete(
        &self,
        session: &DurableAgentSession,
        credential: &DeepSeekCredential,
        messages: &[Value],
        tools: Option<&[Value]>,
    ) -> Result<CompletionResponse, DriverError> {
        let mut payload = json!({
            "model": session.public.model,
            "messages": messages,
            "thinking": {"type": if session.public.variant == "non_thinking" { "disabled" } else { "enabled" }},
            "reasoning_effort": session.public.reasoning_effort,
            "max_tokens": session.public.max_output_tokens,
            "stream": false,
        });
        if let Some(tools) = tools {
            payload["tools"] = Value::Array(tools.to_vec());
        }
        let encoded = serde_json::to_vec(&payload).map_err(|_| INVALID_RESPONSE)?;
        if encoded.len() > MAX_REQUEST_BYTES {
            return Err(DriverError::new(
                "provider_context_limit",
                "The bounded DeepSeek request context is too large.",
            ));
        }
        let response = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(credential.expose())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(encoded)
            .send()
            .await
            .map_err(|_| API_UNAVAILABLE)?;
        let status = response.status();
        let body = bounded_body(response).await?;
        if !status.is_success() {
            return Err(http_error(status));
        }
        serde_json::from_slice(&body).map_err(|_| INVALID_RESPONSE)
    }
}

async fn bounded_body(response: reqwest::Response) -> Result<Vec<u8>, DriverError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(INVALID_RESPONSE);
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| API_UNAVAILABLE)?;
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(INVALID_RESPONSE);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn api_tools(tools: &[Tool], random_tools: bool) -> Vec<Value> {
    tools
        .iter()
        .filter(|tool| allowed_tool(tool.name.as_ref(), random_tools))
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

fn allowed_tool(name: &str, random_tools: bool) -> bool {
    matches!(
        name,
        "read_discussion"
            | "search_messages"
            | "read_message_context"
            | "publish_message"
            | "decline_to_speak"
    ) || (random_tools && matches!(name, "roll_dice" | "choose_random"))
}

fn validate_tool_catalog(tools: &[Tool]) -> Result<(), DriverError> {
    let names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<HashSet<_>>();
    [
        "read_discussion",
        "search_messages",
        "read_message_context",
        "publish_message",
        "decline_to_speak",
        "roll_dice",
        "choose_random",
    ]
    .into_iter()
    .all(|name| names.contains(name))
    .then_some(())
    .ok_or(PORTAL_UNAVAILABLE)
}

fn validate_completion(
    response: &CompletionResponse,
    expected_model: &str,
) -> Result<(), DriverError> {
    if response.id.is_empty()
        || response.id.len() > 128
        || response.id.trim() != response.id
        || response.id.chars().any(char::is_control)
        || response.model != expected_model
        || response.choices.len() != 1
    {
        return Err(INVALID_RESPONSE);
    }
    let choice = &response.choices[0];
    let has_tools = !choice.message.tool_calls.is_empty();
    if choice.index != 0
        || choice.message.role != "assistant"
        || (has_tools && choice.finish_reason != "tool_calls")
        || (!has_tools && choice.finish_reason != "stop")
    {
        return Err(INVALID_RESPONSE);
    }
    Ok(())
}

fn validate_tool_calls(calls: &[ToolCall]) -> Result<(), DriverError> {
    let mut ids = HashSet::new();
    if calls.len() > 16
        || calls.iter().any(|call| {
            call.kind != "function"
                || call.id.is_empty()
                || call.id.len() > 128
                || call.id.trim() != call.id
                || call.id.chars().any(char::is_control)
                || !ids.insert(call.id.as_str())
                || call.function.name.is_empty()
                || call.function.name.len() > 128
                || call.function.arguments.len() > MAX_TOOL_ARGUMENT_BYTES
        })
    {
        return Err(INVALID_TOOL_CALL);
    }
    Ok(())
}

fn tool_result_text(result: &CallToolResult) -> Result<String, DriverError> {
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
            .ok_or(INVALID_TOOL_CALL)?
    };
    (text.len() <= MAX_TOOL_RESULT_BYTES)
        .then_some(text)
        .ok_or(INVALID_TOOL_CALL)
}

fn assistant_value(message: &AssistantMessage) -> Value {
    json!({
        "role": "assistant",
        "content": message.content.as_deref().unwrap_or_default(),
        "reasoning_content": message.reasoning_content,
        "tool_calls": message.tool_calls,
    })
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
    (!value.is_empty() && value.chars().count() <= 12_000).then(|| value.to_owned())
}

fn http_error(status: StatusCode) -> DriverError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => DriverError::new(
            "provider_credential_rejected",
            "DeepSeek rejected the configured credential.",
        ),
        StatusCode::TOO_MANY_REQUESTS => DriverError::new(
            "provider_rate_limited",
            "DeepSeek rate-limited the request.",
        ),
        _ => API_UNAVAILABLE,
    }
}

#[cfg(test)]
#[path = "deepseek_tests.rs"]
mod tests;
