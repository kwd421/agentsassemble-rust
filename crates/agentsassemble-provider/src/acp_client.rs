use std::{
    collections::HashMap,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use agent_client_protocol::schema::{
    ProtocolVersion,
    v1::{
        AgentCapabilities, CancelNotification, ContentBlock, InitializeRequest, LoadSessionRequest,
        McpServer, NewSessionRequest, PermissionOptionKind, PromptRequest,
        RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
        SelectedPermissionOutcome, SessionConfigKind, SessionConfigOption,
        SessionConfigOptionCategory, SessionConfigSelectOptions, SessionId, SessionNotification,
        SessionUpdate, SetSessionConfigOptionRequest, StopReason, TextContent,
    },
};
use agent_client_protocol::{Agent, Client, ConnectionTo, Lines};
use futures_util::{Sink, SinkExt, StreamExt};
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};
use tokio_util::{
    codec::{FramedRead, FramedWrite, LinesCodec},
    sync::CancellationToken,
};

use crate::room_portal_tool_contract::PROVIDER_ROOM_TOOL_NAMES;
use crate::{
    driver::{DriverError, ProviderTurnCompleted},
    launch_error::DriverLaunchError,
    room_portal::ProviderTurnOutcome,
};

const PROTOCOL_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PROTOCOL_LINE_BYTES: usize = 256 * 1024;
const MAX_RESPONSE_BYTES: usize = 128 * 1024;
type ProtocolReady = (ConnectionTo<Agent>, AgentCapabilities, Option<String>);

#[derive(Default)]
struct ProtocolState {
    permission_policy: AcpPermissionPolicy,
    room_observation_active: bool,
    session_id: Option<SessionId>,
    active_turn_id: Option<String>,
    active_tools: HashMap<String, String>,
    output: String,
    output_overflow: bool,
}

#[derive(Clone, Copy, Default)]
pub(crate) enum AcpPermissionPolicy {
    #[default]
    Reject,
    RoomTools,
}

struct ActiveTurn {
    turn_id: String,
    completion: watch::Receiver<Option<Result<StopReason, DriverError>>>,
    task: Option<JoinHandle<()>>,
}

struct OpenedSession {
    attachment: AcpAttachment,
    id: SessionId,
    options: Option<Vec<SessionConfigOption>>,
}

pub(super) struct AcpAttachment {
    pub(super) session_id: String,
    pub(super) reused: bool,
}

pub(super) struct AcpClient {
    shutdown: CancellationToken,
    closed: Arc<AtomicBool>,
    task: JoinHandle<()>,
    connection: ConnectionTo<Agent>,
    capabilities: AgentCapabilities,
    initialized_model_id: Option<String>,
    state: Arc<Mutex<ProtocolState>>,
    attached_session_id: Option<SessionId>,
    active_turn: Option<ActiveTurn>,
    poisoned: bool,
}

impl AcpClient {
    pub(super) async fn connect<I, O>(
        stdin: I,
        stdout: O,
        permission_policy: AcpPermissionPolicy,
    ) -> Result<Self, DriverLaunchError>
    where
        I: tokio::io::AsyncWrite + Unpin + Send + 'static,
        O: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let state = Arc::new(Mutex::new(ProtocolState {
            permission_policy,
            ..ProtocolState::default()
        }));
        let shutdown = CancellationToken::new();
        let closed = Arc::new(AtomicBool::new(false));
        let (ready_sender, ready_receiver) = oneshot::channel();
        let task = spawn_protocol(
            stdin,
            stdout,
            Arc::clone(&state),
            shutdown.clone(),
            Arc::clone(&closed),
            ready_sender,
        );
        let ready = match tokio::time::timeout(PROTOCOL_TIMEOUT, ready_receiver).await {
            Ok(Ok(ready)) => ready,
            Ok(Err(_)) | Err(_) => Err(DriverLaunchError::uncertain(protocol_error())),
        };
        let (connection, capabilities, initialized_model_id) = match ready {
            Ok(ready) => ready,
            Err(error) => {
                shutdown.cancel();
                task.abort();
                let _ = task.await;
                return Err(error);
            }
        };
        Ok(Self {
            shutdown,
            closed,
            task,
            connection,
            capabilities,
            initialized_model_id,
            state,
            attached_session_id: None,
            active_turn: None,
            poisoned: false,
        })
    }

    pub(super) async fn attach(
        &mut self,
        workspace: &str,
        existing_session_id: &str,
        server: McpServer,
        model: &str,
    ) -> Result<AcpAttachment, DriverError> {
        let opened = self
            .open_session(workspace, existing_session_id, server)
            .await?;
        self.select_model(&opened.id, opened.options, model).await?;
        self.bind_session(opened.id)?;
        Ok(opened.attachment)
    }

    pub(super) async fn attach_process_model(
        &mut self,
        workspace: &str,
        existing_session_id: &str,
        server: McpServer,
        model: &str,
    ) -> Result<AcpAttachment, DriverError> {
        if self.initialized_model_id.as_deref() != Some(model) {
            return self.poison(DriverError::new(
                "provider_model_unconfirmed",
                "The ACP provider did not confirm the process-selected model.",
            ));
        }
        let opened = self
            .open_session(workspace, existing_session_id, server)
            .await?;
        self.bind_session(opened.id)?;
        Ok(opened.attachment)
    }

    pub(super) async fn prompt(
        &mut self,
        turn_id: &str,
        input: &str,
        room_observation: bool,
    ) -> Result<ProviderTurnCompleted, DriverError> {
        self.start_turn(turn_id, input)?;
        let stop_reason = self.await_turn(turn_id).await?;
        let output = self.take_output(turn_id)?;
        let Some(session_id) = self.attached_session_id.as_ref().map(ToString::to_string) else {
            return self.poison(protocol_error());
        };
        let outcome = match stop_reason {
            StopReason::EndTurn | StopReason::MaxTokens | StopReason::MaxTurnRequests
                if !output.trim().is_empty() || room_observation =>
            {
                ProviderTurnOutcome::Message {
                    content: output,
                    target_agent_id: String::new(),
                }
            }
            StopReason::Refusal => ProviderTurnOutcome::Declined {
                reason_code: "provider_refusal".to_owned(),
            },
            StopReason::Cancelled => {
                return Err(DriverError::new(
                    "provider_turn_cancelled",
                    "The ACP provider cancelled the turn without an authorized interrupt.",
                ));
            }
            _ => return Err(protocol_error()),
        };
        Ok(ProviderTurnCompleted {
            turn_id: turn_id.to_owned(),
            provider_turn_id: turn_id.to_owned(),
            provider_session_id: Some(session_id),
            outcome,
        })
    }

    pub(super) async fn cancel(&mut self, turn_id: &str) -> Result<(), DriverError> {
        let Some(session_id) = self.attached_session_id.clone() else {
            return self.poison(protocol_error());
        };
        if self.active_turn.as_ref().map(|turn| turn.turn_id.as_str()) != Some(turn_id) {
            return self.poison(protocol_error());
        }
        if self
            .connection
            .send_notification(CancelNotification::new(session_id))
            .is_err()
        {
            return self.poison(protocol_error());
        }
        let reason = match tokio::time::timeout(PROTOCOL_TIMEOUT, self.await_turn(turn_id)).await {
            Ok(Ok(reason)) => reason,
            Ok(Err(error)) => return Err(error),
            Err(_) => return self.poison(protocol_error()),
        };
        self.take_output(turn_id)?;
        if reason == StopReason::Cancelled {
            Ok(())
        } else {
            self.poison(DriverError::new(
                "provider_turn_interrupt_unconfirmed",
                "The ACP provider did not confirm cancellation of the exact turn.",
            ))
        }
    }

    pub(super) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub(super) fn requires_restart(&self) -> bool {
        self.poisoned || self.is_closed()
    }

    pub(super) fn set_room_observation_active(&self, active: bool) -> Result<(), DriverError> {
        self.state
            .lock()
            .map(|mut state| state.room_observation_active = active)
            .map_err(|_| protocol_error())
    }

    pub(super) async fn shutdown(&mut self) {
        self.shutdown.cancel();
        self.task.abort();
        let _ = (&mut self.task).await;
    }

    fn start_turn(&mut self, turn_id: &str, input: &str) -> Result<(), DriverError> {
        let Some(session_id) = self.attached_session_id.clone() else {
            return self.poison(protocol_error());
        };
        if self.active_turn.is_some() {
            return Err(DriverError::new(
                "provider_request_in_progress",
                "An ACP provider turn is already active.",
            ));
        }
        {
            let Ok(mut state) = self.state.lock() else {
                return self.poison(protocol_error());
            };
            state.active_turn_id = Some(turn_id.to_owned());
            state.active_tools.clear();
            state.output.clear();
            state.output_overflow = false;
        }
        let connection = self.connection.clone();
        let prompt = PromptRequest::new(
            session_id,
            vec![ContentBlock::Text(TextContent::new(input.to_owned()))],
        );
        let (sender, completion) = watch::channel(None);
        let task = tokio::spawn(async move {
            let result = connection
                .send_request(prompt)
                .block_task()
                .await
                .map(|response| response.stop_reason)
                .map_err(|_| protocol_error());
            let _ = sender.send(Some(result));
        });
        self.active_turn = Some(ActiveTurn {
            turn_id: turn_id.to_owned(),
            completion,
            task: Some(task),
        });
        Ok(())
    }

    async fn await_turn(&mut self, expected_turn_id: &str) -> Result<StopReason, DriverError> {
        let Some(active) = self.active_turn.as_mut() else {
            return self.poison(protocol_error());
        };
        if active.turn_id != expected_turn_id {
            return self.poison(protocol_error());
        }
        loop {
            let completed = active.completion.borrow_and_update().clone();
            if let Some(result) = completed {
                let Some(task) = active.task.take() else {
                    self.poisoned = true;
                    return Err(protocol_error());
                };
                if task.await.is_err() {
                    self.poisoned = true;
                    return Err(protocol_error());
                }
                return match result {
                    Ok(reason) => Ok(reason),
                    Err(error) => self.poison(error),
                };
            }
            if active.completion.changed().await.is_err() {
                self.poisoned = true;
                return Err(protocol_error());
            }
        }
    }

    fn take_output(&mut self, turn_id: &str) -> Result<String, DriverError> {
        let Some(active) = self.active_turn.take() else {
            return self.poison(protocol_error());
        };
        if active.turn_id != turn_id || active.task.is_some() {
            return self.poison(protocol_error());
        }
        let Ok(mut state) = self.state.lock() else {
            return self.poison(protocol_error());
        };
        if state.active_turn_id.as_deref() != Some(turn_id) || state.output_overflow {
            state.active_turn_id = None;
            state.active_tools.clear();
            state.output.clear();
            self.poisoned = true;
            return Err(DriverError::new(
                "provider_protocol_overflow",
                "The ACP provider response exceeded its bounded output authority.",
            ));
        }
        state.active_turn_id = None;
        state.active_tools.clear();
        Ok(std::mem::take(&mut state.output))
    }

    async fn open_session(
        &mut self,
        workspace: &str,
        existing_session_id: &str,
        server: McpServer,
    ) -> Result<OpenedSession, DriverError> {
        if self.attached_session_id.is_some() || self.active_turn.is_some() {
            return Err(protocol_error());
        }
        if !self.capabilities.mcp_capabilities.http {
            return self.poison(DriverError::new(
                "provider_capability_missing",
                "The ACP provider does not support the required HTTP MCP transport.",
            ));
        }
        let reused = !existing_session_id.is_empty();
        let (id, options) = if reused {
            if !self.capabilities.load_session {
                return self.poison(DriverError::new(
                    "provider_session_restore_unsupported",
                    "The ACP provider cannot restore the durable provider session.",
                ));
            }
            let id = SessionId::new(existing_session_id.to_owned());
            let Ok(response) = self
                .connection
                .send_request(
                    LoadSessionRequest::new(id.clone(), workspace).mcp_servers(vec![server]),
                )
                .block_task()
                .await
            else {
                return self.poison(protocol_error());
            };
            (id, response.config_options)
        } else {
            let Ok(response) = self
                .connection
                .send_request(NewSessionRequest::new(workspace).mcp_servers(vec![server]))
                .block_task()
                .await
            else {
                return self.poison(protocol_error());
            };
            (response.session_id, response.config_options)
        };
        let encoded = id.to_string();
        if encoded.is_empty()
            || encoded.len() > 200
            || encoded.trim() != encoded
            || encoded.chars().any(char::is_control)
        {
            return self.poison(protocol_error());
        }
        Ok(OpenedSession {
            attachment: AcpAttachment {
                session_id: encoded,
                reused,
            },
            id,
            options,
        })
    }

    fn bind_session(&mut self, session_id: SessionId) -> Result<(), DriverError> {
        {
            let Ok(mut state) = self.state.lock() else {
                return self.poison(protocol_error());
            };
            state.session_id = Some(session_id.clone());
        }
        self.attached_session_id = Some(session_id);
        Ok(())
    }

    async fn select_model(
        &mut self,
        session_id: &SessionId,
        options: Option<Vec<SessionConfigOption>>,
        model: &str,
    ) -> Result<(), DriverError> {
        let mut models = options
            .unwrap_or_default()
            .into_iter()
            .filter(is_model_option);
        let Some(option) = models.next() else {
            return self.poison(DriverError::new(
                "provider_model_unconfirmed",
                "The ACP provider did not expose its selected model authority.",
            ));
        };
        if models.next().is_some() || !select_contains(&option.kind, model) {
            return self.poison(DriverError::new(
                "provider_model_unconfirmed",
                "The ACP provider did not advertise the selected model.",
            ));
        }
        if selected_value(&option.kind) == Some(model) {
            return Ok(());
        }
        let Ok(response) = self
            .connection
            .send_request(SetSessionConfigOptionRequest::new(
                session_id.clone(),
                option.id,
                model,
            ))
            .block_task()
            .await
        else {
            return self.poison(protocol_error());
        };
        if response
            .config_options
            .iter()
            .any(|option| is_model_option(option) && selected_value(&option.kind) == Some(model))
        {
            Ok(())
        } else {
            self.poison(DriverError::new(
                "provider_model_unconfirmed",
                "The ACP provider did not confirm the selected model.",
            ))
        }
    }

    fn poison<T>(&mut self, error: DriverError) -> Result<T, DriverError> {
        self.poisoned = true;
        Err(error)
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.task.abort();
    }
}

fn spawn_protocol<I, O>(
    stdin: I,
    stdout: O,
    state: Arc<Mutex<ProtocolState>>,
    shutdown: CancellationToken,
    closed: Arc<AtomicBool>,
    ready: oneshot::Sender<Result<ProtocolReady, DriverLaunchError>>,
) -> JoinHandle<()>
where
    I: tokio::io::AsyncWrite + Unpin + Send + 'static,
    O: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let incoming = FramedRead::new(
            stdout,
            LinesCodec::new_with_max_length(MAX_PROTOCOL_LINE_BYTES),
        )
        .map(|line| line.map_err(io::Error::other));
        let outgoing = outgoing_lines(stdin);
        let _ = Client
            .builder()
            .on_receive_notification(
                {
                    let state = Arc::clone(&state);
                    async move |notification: SessionNotification, _connection| {
                        record_notification(&state, notification);
                        Ok(())
                    }
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                {
                    let state = Arc::clone(&state);
                    async move |request: RequestPermissionRequest, responder, _connection| {
                        responder.respond(permission_response(&state, &request))
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(Lines::new(outgoing, incoming), async move |connection| {
                let initialized = connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await;
                match initialized {
                    Ok(initialized) if initialized.protocol_version == ProtocolVersion::V1 => {
                        let model = initialized_model_id(initialized.meta.as_ref());
                        let _ = ready.send(Ok((connection, initialized.agent_capabilities, model)));
                    }
                    _ => {
                        let _ = ready.send(Err(DriverLaunchError::safe(protocol_error())));
                        return Ok(());
                    }
                }
                shutdown.cancelled().await;
                Ok(())
            })
            .await;
        closed.store(true, Ordering::Release);
    })
}

fn record_notification(state: &Mutex<ProtocolState>, notification: SessionNotification) {
    let Ok(mut state) = state.lock() else {
        return;
    };
    if state.session_id.as_ref() != Some(&notification.session_id) || state.active_turn_id.is_none()
    {
        return;
    }
    match notification.update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            if let ContentBlock::Text(text) = chunk.content {
                if state.output.len().saturating_add(text.text.len()) > MAX_RESPONSE_BYTES {
                    state.output_overflow = true;
                    state.output.clear();
                } else if !state.output_overflow {
                    state.output.push_str(&text.text);
                }
            }
        }
        SessionUpdate::ToolCall(tool) => record_tool_identity(
            &mut state.active_tools,
            tool.tool_call_id.to_string(),
            tool.raw_input.as_ref(),
        ),
        SessionUpdate::ToolCallUpdate(tool) => record_tool_identity(
            &mut state.active_tools,
            tool.tool_call_id.to_string(),
            tool.fields.raw_input.as_ref(),
        ),
        _ => {}
    }
}

fn permission_response(
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
        .and_then(room_tool_identity);
    let cached_tool = state
        .active_tools
        .get(&request.tool_call.tool_call_id.to_string())
        .map(String::as_str);
    let allow = matches!(state.permission_policy, AcpPermissionPolicy::RoomTools)
        && state.room_observation_active
        && requested_tool.is_some()
        && cached_tool != Some("")
        && cached_tool.is_none_or(|cached| Some(cached) == requested_tool);
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

fn record_tool_identity(
    active_tools: &mut HashMap<String, String>,
    tool_call_id: String,
    raw_input: Option<&serde_json::Value>,
) {
    let Some(tool) = raw_input.and_then(room_tool_identity) else {
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

fn room_tool_identity(raw_input: &serde_json::Value) -> Option<&str> {
    let name = raw_input.get("tool_name")?.as_str()?;
    let bare = name
        .strip_prefix("agentsassemble_room__")
        .or_else(|| name.strip_prefix("agentsassemble_room_"))
        .unwrap_or(name);
    PROVIDER_ROOM_TOOL_NAMES.contains(&bare).then_some(bare)
}

fn initialized_model_id(
    meta: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<String> {
    let model = meta?.get("modelState")?.get("currentModelId")?.as_str()?;
    (!model.is_empty()
        && model.len() <= crate::catalog::MAX_OPTION_VALUE_BYTES
        && !model.chars().any(char::is_control))
    .then(|| model.to_owned())
}

fn select_contains(kind: &SessionConfigKind, model: &str) -> bool {
    let SessionConfigKind::Select(select) = kind else {
        return false;
    };
    match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .any(|option| option.value.to_string() == model),
        SessionConfigSelectOptions::Grouped(groups) => groups.iter().any(|group| {
            group
                .options
                .iter()
                .any(|option| option.value.to_string() == model)
        }),
        _ => false,
    }
}

fn selected_value(kind: &SessionConfigKind) -> Option<&str> {
    let SessionConfigKind::Select(select) = kind else {
        return None;
    };
    Some(&select.current_value.0)
}

fn is_model_option(option: &SessionConfigOption) -> bool {
    option.category == Some(SessionConfigOptionCategory::Model) || option.id.to_string() == "model"
}

fn outgoing_lines<W>(writer: W) -> impl Sink<String, Error = io::Error> + Send + 'static
where
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    FramedWrite::new(writer, LinesCodec::new())
        .with(|line: String| async move { Ok::<String, tokio_util::codec::LinesCodecError>(line) })
        .sink_map_err(io::Error::other)
}

const fn protocol_error() -> DriverError {
    DriverError::new(
        "provider_protocol_invalid",
        "The ACP provider returned an invalid protocol message.",
    )
}

#[cfg(test)]
#[path = "acp_client_tests.rs"]
mod tests;
