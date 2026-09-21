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
        AgentCapabilities, CancelNotification, ClientCapabilities, ContentBlock, InitializeRequest,
        LoadSessionRequest, McpServer, NewSessionRequest, PromptRequest, RequestPermissionRequest,
        SessionConfigOption, SessionId, SessionNotification, SessionUpdate, StopReason,
        TextContent,
    },
};
use agent_client_protocol::{Agent, Client, ConnectionTo, Lines};
use futures_util::StreamExt;
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};
use tokio_util::{
    codec::{FramedRead, LinesCodec},
    sync::CancellationToken,
};

use crate::{
    driver::{DriverError, ProviderTurnCompleted, ProviderTurnRequest},
    launch_error::DriverLaunchError,
    room_portal::{ProviderTurnOutcome, TerminalWatch},
};

const PROTOCOL_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PROTOCOL_LINE_BYTES: usize = 256 * 1024;
const MAX_RESPONSE_BYTES: usize = 128 * 1024;
type ProtocolReady = (ConnectionTo<Agent>, AgentCapabilities, Option<String>);

#[path = "acp_delivery.rs"]
mod delivery;
#[path = "acp_permissions.rs"]
mod permissions;
pub(crate) use permissions::AcpToolIdentityContract;
use permissions::record_tool_identity;

#[path = "acp_configuration.rs"]
mod configuration;

#[derive(Default)]
pub(crate) struct AcpClientConfiguration {
    pub(crate) permission_policy: AcpPermissionPolicy,
    pub(crate) tool_identity: AcpToolIdentityContract,
    pub(crate) capabilities: ClientCapabilities,
}

#[derive(Default)]
struct ProtocolState {
    permission_policy: AcpPermissionPolicy,
    tool_identity: AcpToolIdentityContract,
    room_observation_active: bool,
    session_id: Option<SessionId>,
    active_turn_id: Option<String>,
    request_turn: Option<permissions::RequestTurn>,
    request_failed: bool,
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
        configuration: AcpClientConfiguration,
    ) -> Result<Self, DriverLaunchError>
    where
        I: tokio::io::AsyncWrite + Unpin + Send + 'static,
        O: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let state = Arc::new(Mutex::new(ProtocolState {
            permission_policy: configuration.permission_policy,
            tool_identity: configuration.tool_identity,
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
            configuration.capabilities,
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
        selection: &[(String, String)],
    ) -> Result<AcpAttachment, DriverError> {
        let opened = self
            .open_session(workspace, existing_session_id, server)
            .await?;
        self.select_configuration(&opened.id, opened.options, selection)
            .await?;
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
        session_id: &str,
        request: &ProviderTurnRequest,
        terminal: Option<TerminalWatch>,
    ) -> Result<ProviderTurnCompleted, DriverError> {
        let turn_id = &request.turn_id;
        let room_observation = request.room_observation.is_some();
        self.start_turn(session_id, request)?;
        let stop_reason = match terminal.filter(|_| room_observation) {
            Some(terminal) => {
                let finished = tokio::select! {
                    biased;
                    reason = self.await_turn(turn_id) => Some(reason?),
                    () = terminal.staged() => None,
                };
                match finished {
                    Some(reason) => reason,
                    None => self.end_after_terminal_action(turn_id).await?,
                }
            }
            None => self.await_turn(turn_id).await?,
        };
        self.finish_requests().await?;
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

    /// Ends a turn whose room action is already staged, without the closing model call.
    ///
    /// The room outcome comes from the portal, not from this turn's text, so the
    /// provider has nothing left to contribute. Whether it confirms the cancellation or
    /// had just finished on its own, the turn ended normally.
    async fn end_after_terminal_action(
        &mut self,
        turn_id: &str,
    ) -> Result<StopReason, DriverError> {
        let Some(session_id) = self.attached_session_id.clone() else {
            return self.poison(protocol_error());
        };
        {
            let state = self.state.lock().map_err(|_| protocol_error())?;
            if let Some(turn) = &state.request_turn {
                turn.cancel();
            }
        }
        if self
            .connection
            .send_notification(CancelNotification::new(session_id))
            .is_err()
        {
            return self.poison(protocol_error());
        }
        match tokio::time::timeout(PROTOCOL_TIMEOUT, self.await_turn(turn_id)).await {
            Ok(Ok(StopReason::Cancelled | StopReason::EndTurn)) => Ok(StopReason::EndTurn),
            Ok(Err(error)) => Err(error),
            Ok(Ok(_)) | Err(_) => self.poison(protocol_error()),
        }
    }

    pub(super) async fn cancel(&mut self, turn_id: &str) -> Result<(), DriverError> {
        let Some(session_id) = self.attached_session_id.clone() else {
            return self.poison(protocol_error());
        };
        if self.active_turn.as_ref().map(|turn| turn.turn_id.as_str()) != Some(turn_id) {
            return self.poison(protocol_error());
        }
        {
            let state = self.state.lock().map_err(|_| protocol_error())?;
            if let Some(turn) = &state.request_turn {
                turn.cancel();
            }
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
        self.finish_requests().await?;
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

    async fn finish_requests(&self) -> Result<(), DriverError> {
        let handlers = self
            .state
            .lock()
            .map_err(|_| protocol_error())?
            .request_turn
            .as_ref()
            .map(permissions::RequestTurn::close);
        if let Some(handlers) = handlers {
            handlers.wait().await;
        }
        Ok(())
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

    pub(super) async fn request_extension(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, DriverError> {
        use agent_client_protocol::schema::v1::{ClientRequest, ExtRequest};
        let params = serde_json::value::to_raw_value(&params).map_err(|_| protocol_error())?;
        self.connection
            .send_request(ClientRequest::ExtMethodRequest(ExtRequest::new(
                method.to_owned(),
                params.into(),
            )))
            .block_task()
            .await
            .map_err(|_| protocol_error())
    }

    pub(super) async fn shutdown(&mut self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .request_turn
            .take();
        self.shutdown.cancel();
        self.task.abort();
        let _ = (&mut self.task).await;
    }

    fn start_turn(
        &mut self,
        room_session_id: &str,
        request: &ProviderTurnRequest,
    ) -> Result<(), DriverError> {
        let turn_id = &request.turn_id;
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
            state.request_turn = Some(permissions::RequestTurn::new(room_session_id, request));
            state.request_failed = false;
            state.active_tools.clear();
            state.output.clear();
            state.output_overflow = false;
        }
        let connection = self.connection.clone();
        let prompt = PromptRequest::new(
            session_id,
            vec![ContentBlock::Text(TextContent::new(request.input.clone()))],
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
        state.request_turn.take();
        if state.request_failed {
            state.active_turn_id = None;
            state.active_tools.clear();
            state.output.clear();
            self.poisoned = true;
            return Err(permissions::request_error());
        }
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

    fn poison<T>(&mut self, error: DriverError) -> Result<T, DriverError> {
        self.poisoned = true;
        Err(error)
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .request_turn
            .take();
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
    capabilities: ClientCapabilities,
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
        let deliveries = delivery::Deliveries::default();
        let outgoing = delivery::outgoing_lines(stdin, deliveries.clone());
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
                        permissions::handle(&state, &deliveries, request, responder).await
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(Lines::new(outgoing, incoming), async move |connection| {
                let initialized = connection
                    .send_request(
                        InitializeRequest::new(ProtocolVersion::V1)
                            .client_capabilities(capabilities),
                    )
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
    let contract = state.tool_identity;
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
            contract,
            &mut state.active_tools,
            tool.tool_call_id.to_string(),
            tool.raw_input.as_ref(),
        ),
        SessionUpdate::ToolCallUpdate(tool) => record_tool_identity(
            contract,
            &mut state.active_tools,
            tool.tool_call_id.to_string(),
            tool.fields.raw_input.as_ref(),
        ),
        _ => {}
    }
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

const fn protocol_error() -> DriverError {
    DriverError::new(
        "provider_protocol_invalid",
        "The ACP provider returned an invalid protocol message.",
    )
}

#[cfg(test)]
#[path = "acp_client_tests.rs"]
mod tests;
