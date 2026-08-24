use std::{path::Path, time::Duration};

#[cfg(not(unix))]
use std::process::Stdio;

use agentsassemble_domain::DurableAgentSession;
use hyper::StatusCode;
#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(not(unix))]
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::oneshot,
    task::JoinHandle,
};
use uuid::Uuid;

#[cfg(not(any(target_os = "linux", target_os = "android")))]
use crate::filesystem::BoundExecutable;
#[cfg(not(unix))]
use crate::opencode_protocol::{health_error, stop_error};
#[cfg(not(unix))]
use crate::process::sanitize_environment;
use crate::{
    filesystem::bind_executable_with_children,
    launch_error::DriverLaunchError,
    loopback_http::{JsonResponse, LoopbackHttp, VerifiedLoopbackConnection},
    opencode_protocol::{
        TurnTransportError, assistant_message, clean_session_id, config_error, executable_error,
        http_driver_error, model_id, model_mismatch, portal_driver_error, portal_unavailable,
        profile_error, protocol_error, provider_id, provider_request_error, runtime_exited,
        session_mismatch, session_missing, session_path, session_unconfirmed, spawn_error,
        startup_error, turn_empty, turn_mismatch, turn_timeout, turn_transport_error,
        validate_profile,
    },
    opencode_sse::{OpenCodeTurnEvents, collect_turn_events},
    room_portal::{ProviderTurnOutcome, RoomPortal},
    runtime::{
        DriverError, DriverFuture, ProviderDriver, ProviderSessionAttachment,
        ProviderTurnCompleted, ProviderTurnRequest,
    },
};
#[cfg(unix)]
use crate::{
    guardian::GuardianLaunch, runtime_lease::HeldRuntimeLease, unix_custody::UnixProcessCustody,
};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const TURN_TIMEOUT: Duration = Duration::from_mins(3);
#[cfg(not(unix))]
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const MCP_NAME: &str = "agentsassemble_room";
const SERVER_USERNAME: &str = "agentsassemble";
const MAX_STARTUP_LINE_BYTES: usize = 1024;

pub(crate) struct OpenCodeDriver {
    #[cfg(unix)]
    process_group: UnixProcessCustody,
    #[cfg(not(unix))]
    child: Box<dyn ChildWrapper>,
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    _executable_guard: BoundExecutable,
    stdout_task: JoinHandle<()>,
    stderr_task: JoinHandle<()>,
    http: LoopbackHttp,
    room_portal: RoomPortal,
    _config_root: tempfile::TempDir,
    attached_session_id: Option<String>,
    attached_reused: bool,
    mcp_registered: bool,
    completed_turn: Option<(ProviderTurnRequest, ProviderTurnCompleted)>,
    poisoned: bool,
}

impl OpenCodeDriver {
    #[cfg(unix)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        runtime_lease: &HeldRuntimeLease,
        guardian: &GuardianLaunch,
    ) -> Result<Self, DriverLaunchError> {
        Self::spawn_inner(session, runtime_lease, guardian).await
    }

    #[cfg(not(unix))]
    pub(crate) async fn spawn(session: &DurableAgentSession) -> Result<Self, DriverLaunchError> {
        Self::spawn_inner(session).await
    }

    async fn spawn_inner(
        session: &DurableAgentSession,
        #[cfg(unix)] runtime_lease: &HeldRuntimeLease,
        #[cfg(unix)] guardian: &GuardianLaunch,
    ) -> Result<Self, DriverLaunchError> {
        validate_profile(session)?;
        let workspace = Path::new(&session.workspace);
        let config_root = isolated_config_root()?;
        let server_password = server_password();
        let environment = isolated_environment(config_root.path(), &server_password)?;
        let port = reserve_loopback_port().await?;
        let endpoint = format!("http://127.0.0.1:{port}/");
        let ready_line = format!("opencode server listening on http://127.0.0.1:{port}");
        let arguments = server_arguments(port);
        let executable = bind_executable_with_children(
            session.executable.clone(),
            session.executable_identity.clone(),
        )
        .await
        .map_err(|_| executable_error())?;
        let room_portal = RoomPortal::create().await.map_err(portal_driver_error)?;
        #[cfg(unix)]
        let (process_group, pipes) = UnixProcessCustody::start_with_children(
            runtime_lease,
            guardian,
            &executable,
            &arguments,
            &environment,
            workspace,
        )
        .await?;
        #[cfg(unix)]
        let (stdout_task, stderr_task, startup) = {
            drop(pipes.stdin);
            let (stdout_task, startup) = observe_startup(pipes.stdout, ready_line.clone());
            (
                stdout_task,
                tokio::spawn(drain_output(pipes.stderr)),
                startup,
            )
        };
        #[cfg(not(unix))]
        let (child, stdout_task, stderr_task, startup) = {
            let mut command = CommandWrap::with_new(executable.launch_path(), |command| {
                command
                    .args(&arguments)
                    .current_dir(workspace)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
            });
            sanitize_environment(command.command_mut());
            command.command_mut().envs(environment.iter().cloned());
            command.wrap(KillOnDrop);
            #[cfg(windows)]
            command.wrap(JobObject);
            let mut child = command
                .spawn()
                .map_err(|_| DriverLaunchError::safe(spawn_error()))?;
            drop(command);
            let stdout = child
                .stdout()
                .take()
                .ok_or_else(|| DriverLaunchError::uncertain(spawn_error()))?;
            let stderr = child
                .stderr()
                .take()
                .ok_or_else(|| DriverLaunchError::uncertain(spawn_error()))?;
            let (stdout_task, startup) = observe_startup(stdout, ready_line);
            (
                child,
                stdout_task,
                tokio::spawn(drain_output(stderr)),
                startup,
            )
        };
        let http = LoopbackHttp::new(&endpoint, workspace, SERVER_USERNAME, &server_password)
            .map_err(http_driver_error)?;
        let driver = Self {
            #[cfg(unix)]
            process_group,
            #[cfg(not(unix))]
            child,
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            _executable_guard: executable,
            stdout_task,
            stderr_task,
            http,
            room_portal,
            _config_root: config_root,
            attached_session_id: None,
            attached_reused: false,
            mcp_registered: false,
            completed_turn: None,
            poisoned: false,
        };
        Self::confirm_startup(driver, startup).await
    }

    async fn confirm_startup(
        mut driver: Self,
        startup: oneshot::Receiver<bool>,
    ) -> Result<Self, DriverLaunchError> {
        if let Err(error) = driver.wait_until_ready(startup).await {
            return Err(if driver.stop_process().await.is_ok() {
                DriverLaunchError::safe(error)
            } else {
                DriverLaunchError::uncertain(error)
            });
        }
        Ok(driver)
    }

    async fn wait_until_ready(
        &mut self,
        startup: oneshot::Receiver<bool>,
    ) -> Result<(), DriverError> {
        let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
        if !matches!(
            tokio::time::timeout_at(deadline, startup).await,
            Ok(Ok(true))
        ) {
            return Err(startup_error());
        }
        loop {
            if let Ok(connection) = self.connect_owned_peer().await
                && let Ok(response) = connection
                    .get_json("/global/health", Duration::from_millis(500))
                    .await
                && response.status.is_success()
                && response.value.get("healthy").and_then(Value::as_bool) == Some(true)
            {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(startup_error());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn connect_owned_peer(&mut self) -> Result<VerifiedLoopbackConnection, DriverError> {
        let connection = self.http.connect().await.map_err(http_driver_error)?;
        let exact_child_is_alive = self.is_alive().await?;
        if !exact_child_is_alive {
            return Err(runtime_exited());
        }
        self.http
            .verify_peer(connection, true)
            .map_err(http_driver_error)
    }

    async fn attach(
        &mut self,
        session: &DurableAgentSession,
    ) -> Result<ProviderSessionAttachment, DriverError> {
        if self.poisoned {
            return Err(protocol_error());
        }
        if !self.mcp_registered {
            self.register_room_portal().await?;
            self.mcp_registered = true;
        }
        if let Some(attached) = &self.attached_session_id {
            if !session.provider_session_id.is_empty() && session.provider_session_id != *attached {
                return self.poison(session_mismatch());
            }
            return Ok(ProviderSessionAttachment {
                provider_session_id: attached.clone(),
                reused: self.attached_reused,
                observed_model_id: Some(session.public.model.clone()),
            });
        }
        let (provider_session_id, reused) = if session.provider_session_id.is_empty() {
            (self.create_session(session).await?, false)
        } else {
            let provider_session_id =
                clean_session_id(&session.provider_session_id).ok_or_else(profile_error)?;
            self.require_session(&provider_session_id).await?;
            (provider_session_id, true)
        };
        self.attached_session_id = Some(provider_session_id.clone());
        self.attached_reused = reused;
        Ok(ProviderSessionAttachment {
            provider_session_id,
            reused,
            observed_model_id: Some(session.public.model.clone()),
        })
    }

    async fn register_room_portal(&mut self) -> Result<(), DriverError> {
        let response = self
            .connect_owned_peer()
            .await?
            .post_json(
                "/mcp",
                &json!({
                    "name": MCP_NAME,
                    "config": {
                        "type": "remote",
                        "url": self.room_portal.endpoint(),
                        "enabled": true,
                        "headers": {
                            "Authorization": format!("Bearer {}", self.room_portal.bearer_token()),
                        },
                        "timeout": 10_000,
                    },
                }),
                STARTUP_TIMEOUT,
            )
            .await
            .map_err(http_driver_error)?;
        let connected = response.status.is_success()
            && response
                .value
                .get(MCP_NAME)
                .and_then(Value::as_object)
                .and_then(|status| status.get("status"))
                .and_then(Value::as_str)
                == Some("connected");
        if connected {
            Ok(())
        } else {
            Err(portal_unavailable())
        }
    }

    async fn create_session(
        &mut self,
        session: &DurableAgentSession,
    ) -> Result<String, DriverError> {
        let permission_action = if session.public.permission_mode == "meeting_read_only" {
            "deny"
        } else {
            "ask"
        };
        let mut model = json!({
            "id": model_id(&session.public.model)?,
            "providerID": provider_id(&session.public.model)?,
        });
        if !session.public.variant.is_empty() {
            model["variant"] = json!(session.public.variant);
        }
        let response = self
            .connect_owned_peer()
            .await?
            .post_json(
                "/session",
                &json!({
                    "title": format!("AgentsAssemble {}", session.public.session_id),
                    "model": model,
                    "permission": [
                        {"permission": "*", "pattern": "*", "action": permission_action},
                        {"permission": "read", "pattern": "*", "action": "allow"},
                        {"permission": "glob", "pattern": "*", "action": "allow"},
                        {"permission": "grep", "pattern": "*", "action": "allow"},
                        {"permission": "list", "pattern": "*", "action": "allow"},
                        {"permission": "external_directory", "pattern": "*", "action": "deny"},
                        {"permission": "agentsassemble_room_*", "pattern": "*", "action": "allow"},
                    ],
                }),
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(http_driver_error)?;
        if !response.status.is_success() {
            return Err(provider_request_error());
        }
        response
            .value
            .get("id")
            .and_then(Value::as_str)
            .and_then(clean_session_id)
            .ok_or_else(session_unconfirmed)
    }

    async fn require_session(&mut self, provider_session_id: &str) -> Result<(), DriverError> {
        let response = self
            .connect_owned_peer()
            .await?
            .get_json(&session_path(provider_session_id)?, REQUEST_TIMEOUT)
            .await
            .map_err(http_driver_error)?;
        match response.status {
            status if status.is_success() => Ok(()),
            StatusCode::NOT_FOUND => Err(session_missing()),
            _ => Err(provider_request_error()),
        }
    }

    async fn send(
        &mut self,
        session: &DurableAgentSession,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnCompleted, DriverError> {
        if self.poisoned {
            return Err(protocol_error());
        }
        if let Some((completed_request, completed)) = &self.completed_turn
            && completed_request == request
        {
            return Ok(completed.clone());
        }
        let attached = self
            .attached_session_id
            .clone()
            .ok_or_else(session_unconfirmed)?;
        if session.provider_session_id != attached {
            return self.poison(session_mismatch());
        }
        let event_response = self
            .connect_owned_peer()
            .await?
            .get_stream("/event", TURN_TIMEOUT)
            .await
            .map_err(http_driver_error)?;
        let path = format!("{}/message", session_path(&attached)?);
        let mut payload = json!({
            "model": {
                "providerID": provider_id(&session.public.model)?,
                "modelID": model_id(&session.public.model)?,
            },
            "parts": [{"type": "text", "text": request.input}],
        });
        if !session.public.variant.is_empty() {
            payload["variant"] = json!(session.public.variant);
        }
        let prompt_connection = self.connect_owned_peer().await?;
        let prompt = async move {
            prompt_connection
                .post_json(&path, &payload, TURN_TIMEOUT)
                .await
                .map_err(TurnTransportError::from)
        };
        let events = async {
            collect_turn_events(event_response, &attached, TURN_TIMEOUT)
                .await
                .map_err(TurnTransportError::from)
        };
        let joined = tokio::time::timeout(TURN_TIMEOUT, async { tokio::try_join!(prompt, events) })
            .await
            .map_err(|_| turn_timeout())?;
        let (prompt, events) = match joined {
            Ok(joined) => joined,
            Err(error) => {
                self.abort_session(&attached).await;
                return Err(turn_transport_error(error));
            }
        };
        let completed = self
            .completed_from_response(session, request, &attached, prompt, events)
            .await;
        match completed {
            Ok(completed) => {
                self.completed_turn = Some((request.clone(), completed.clone()));
                Ok(completed)
            }
            Err(error) => self.poison(error),
        }
    }

    async fn completed_from_response(
        &mut self,
        session: &DurableAgentSession,
        request: &ProviderTurnRequest,
        attached: &str,
        prompt: JsonResponse,
        events: OpenCodeTurnEvents,
    ) -> Result<ProviderTurnCompleted, DriverError> {
        if !prompt.status.is_success() {
            return Err(provider_request_error());
        }
        let mut message = assistant_message(&prompt.value)?;
        if message.parent_id != events.request_message {
            return Err(turn_mismatch());
        }
        if !events.assistant_message.is_empty() && message.id != events.assistant_message {
            return Err(turn_mismatch());
        }
        if message.observed_model != session.public.model
            || events.observed_model != session.public.model
            || message.observed_model != events.observed_model
        {
            return Err(model_mismatch());
        }
        if message.content.is_empty() {
            message.content = self
                .assistant_text_for_parent(attached, &events.request_message, &session.public.model)
                .await?;
        }
        if message.content.is_empty() {
            return Err(turn_empty());
        }
        Ok(ProviderTurnCompleted {
            turn_id: request.turn_id.clone(),
            provider_turn_id: message.id,
            provider_session_id: Some(attached.to_owned()),
            outcome: ProviderTurnOutcome::Message {
                content: message.content,
                target_agent_id: String::new(),
            },
        })
    }

    async fn assistant_text_for_parent(
        &mut self,
        attached: &str,
        parent_id: &str,
        configured_model: &str,
    ) -> Result<String, DriverError> {
        let path = format!("{}/message", session_path(attached)?);
        let response = self
            .connect_owned_peer()
            .await?
            .get_json(&path, REQUEST_TIMEOUT)
            .await
            .map_err(http_driver_error)?;
        if !response.status.is_success() {
            return Err(provider_request_error());
        }
        let messages = response.value.as_array().ok_or_else(protocol_error)?;
        for value in messages.iter().rev() {
            if value.pointer("/info/role").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            let message = assistant_message(value)?;
            if message.parent_id != parent_id {
                continue;
            }
            if message.observed_model != configured_model {
                return Err(model_mismatch());
            }
            return Ok(message.content);
        }
        Ok(String::new())
    }

    async fn abort_session(&mut self, session_id: &str) {
        let Ok(path) = session_path(session_id).map(|path| format!("{path}/abort")) else {
            return;
        };
        if let Ok(connection) = self.connect_owned_peer().await {
            let _ = connection
                .post_json(&path, &json!({}), Duration::from_secs(5))
                .await;
        }
    }

    async fn stop_process(&mut self) -> Result<(), DriverError> {
        if let Some(session_id) = self.attached_session_id.clone() {
            self.abort_session(&session_id).await;
        }
        if self.mcp_registered
            && let Ok(connection) = self.connect_owned_peer().await
        {
            let _ = connection
                .post_json(
                    &format!("/mcp/{MCP_NAME}/disconnect"),
                    &json!({}),
                    Duration::from_secs(2),
                )
                .await;
        }
        #[cfg(unix)]
        self.process_group.stop().await?;
        #[cfg(not(unix))]
        {
            tokio::time::timeout(STOP_TIMEOUT, Box::into_pin(self.child.kill()))
                .await
                .map_err(|_| stop_error())?
                .map_err(|_| stop_error())?;
        }
        self.stdout_task.abort();
        self.stderr_task.abort();
        let _ = (&mut self.stdout_task).await;
        let _ = (&mut self.stderr_task).await;
        Ok(())
    }

    fn poison<T>(&mut self, error: DriverError) -> Result<T, DriverError> {
        self.poisoned = true;
        Err(error)
    }
}

fn server_arguments(port: u16) -> Vec<String> {
    vec![
        "serve".to_owned(),
        "--pure".to_owned(),
        "--hostname".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--log-level".to_owned(),
        "ERROR".to_owned(),
    ]
}

fn isolated_config_root() -> Result<tempfile::TempDir, DriverLaunchError> {
    let root = tempfile::Builder::new()
        .prefix("agentsassemble-opencode-config-")
        .tempdir()
        .map_err(|_| DriverLaunchError::safe(config_error()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .map_err(|_| DriverLaunchError::safe(config_error()))?;
        let metadata = std::fs::symlink_metadata(root.path())
            .map_err(|_| DriverLaunchError::safe(config_error()))?;
        if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
            return Err(DriverLaunchError::safe(config_error()));
        }
    }
    Ok(root)
}

fn isolated_environment(
    root: &Path,
    server_password: &str,
) -> Result<Vec<(String, String)>, DriverLaunchError> {
    let root = root
        .to_str()
        .filter(|value| !value.is_empty() && value.len() <= 4096)
        .ok_or_else(|| DriverLaunchError::safe(config_error()))?
        .to_owned();
    Ok(vec![
        ("XDG_CONFIG_HOME".to_owned(), root.clone()),
        ("OPENCODE_CONFIG_DIR".to_owned(), root),
        (
            "OPENCODE_CONFIG_CONTENT".to_owned(),
            "{\"plugin\":[],\"mcp\":{}}".to_owned(),
        ),
        ("OPENCODE_DISABLE_PROJECT_CONFIG".to_owned(), "1".to_owned()),
        (
            "OPENCODE_DISABLE_DEFAULT_PLUGINS".to_owned(),
            "1".to_owned(),
        ),
        (
            "OPENCODE_DISABLE_EXTERNAL_SKILLS".to_owned(),
            "1".to_owned(),
        ),
        (
            "OPENCODE_DISABLE_CLAUDE_CODE_SKILLS".to_owned(),
            "1".to_owned(),
        ),
        ("OPENCODE_PURE".to_owned(), "1".to_owned()),
        (
            "OPENCODE_SERVER_USERNAME".to_owned(),
            SERVER_USERNAME.to_owned(),
        ),
        (
            "OPENCODE_SERVER_PASSWORD".to_owned(),
            server_password.to_owned(),
        ),
    ])
}

impl ProviderDriver for OpenCodeDriver {
    fn attach_session<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>> {
        Box::pin(self.attach(session))
    }

    fn send_turn<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnCompleted, DriverError>> {
        Box::pin(self.send(session, request))
    }

    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>> {
        Box::pin(async move {
            #[cfg(unix)]
            return self.process_group.leader_is_running().await;
            #[cfg(not(unix))]
            self.child
                .try_wait()
                .map(|status| status.is_none())
                .map_err(|_| health_error())
        })
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(self.stop_process())
    }

    fn begin_room_observation(&mut self, request: &ProviderTurnRequest) -> Result<(), DriverError> {
        let observation = request
            .room_observation
            .as_ref()
            .ok_or_else(portal_unavailable)?;
        self.room_portal
            .begin_observation(
                &observation.session_id,
                &request.turn_id,
                observation.input_up_to_seq,
                &observation.view,
                &observation.allowed_agent_ids,
                observation.room_tool_ingress.clone(),
            )
            .map_err(portal_driver_error)
    }

    fn finish_room_observation(
        &mut self,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnOutcome, DriverError> {
        let observation = request
            .room_observation
            .as_ref()
            .ok_or_else(portal_unavailable)?;
        self.room_portal
            .finish_observation(&request.turn_id, observation.input_up_to_seq)
            .map_err(portal_driver_error)
    }

    fn abort_room_observation(&mut self) {
        let _ = self.room_portal.end_observation();
    }

    fn requires_restart(&self) -> bool {
        self.poisoned
    }
}

impl Drop for OpenCodeDriver {
    fn drop(&mut self) {
        #[cfg(unix)]
        self.process_group.request_stop();
    }
}

async fn reserve_loopback_port() -> Result<u16, DriverError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| spawn_error())?;
    let port = listener.local_addr().map_err(|_| spawn_error())?.port();
    drop(listener);
    Ok(port)
}

fn server_password() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn observe_startup<R>(
    mut output: R,
    expected_line: String,
) -> (JoinHandle<()>, oneshot::Receiver<bool>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let (ready_sender, ready_receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let ready = read_startup_line(&mut output)
            .await
            .is_some_and(|line| line == expected_line.as_bytes());
        let _ = ready_sender.send(ready);
        drain_output(output).await;
    });
    (task, ready_receiver)
}

async fn read_startup_line<R: AsyncRead + Unpin>(output: &mut R) -> Option<Vec<u8>> {
    let mut line = Vec::with_capacity(128);
    let mut byte = [0_u8; 1];
    loop {
        if line.len() >= MAX_STARTUP_LINE_BYTES {
            return None;
        }
        match output.read(&mut byte).await {
            Ok(0) | Err(_) => return None,
            Ok(_) if byte[0] == b'\n' => {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Some(line);
            }
            Ok(_) => line.push(byte[0]),
        }
    }
}

async fn drain_output<R: AsyncRead + Unpin>(mut output: R) {
    let mut buffer = [0_u8; 8 * 1024];
    while output.read(&mut buffer).await.is_ok_and(|count| count != 0) {}
}

#[cfg(test)]
mod opencode_tests;
