use std::time::Duration;

use agentsassemble_domain::DurableAgentSession;
use hyper::StatusCode;
#[cfg(not(unix))]
use process_wrap::tokio::ChildWrapper;
use serde_json::{Value, json};
use tokio::{sync::oneshot, task::JoinHandle};

mod isolation;
mod requests;
#[path = "opencode/session_creation.rs"]
mod session_creation;
mod turn_transport;

use isolation::isolated_environment;

#[cfg(not(any(target_os = "linux", target_os = "android")))]
use crate::filesystem::BoundExecutable;
use crate::opencode_key_broker::KeyBroker;
#[cfg(not(unix))]
use crate::opencode_protocol::health_error;
use crate::{
    driver::ROOM_PORTAL_UNAVAILABLE,
    filesystem::bind_executable_with_children,
    launch_error::DriverLaunchError,
    loopback_http::{JsonResponse, LoopbackHttp, VerifiedLoopbackConnection},
    opencode_protocol::{
        OpenCodeApi, assistant_message, clean_session_id, config_error, executable_error,
        http_driver_error, is_harness, model_mismatch, model_parts, profile_error, protocol_error,
        provider_request_error, provider_response_error, runtime_exited, session_mismatch,
        session_missing, session_model, session_path, session_unconfirmed, startup_error,
        turn_empty, turn_in_progress, turn_mismatch, turn_transport_error, validate_profile,
    },
    opencode_sse::OpenCodeTurnEvents,
    opencode_startup::{reserve_loopback_port, server_password},
    room_portal::{ProviderTurnOutcome, RoomPortal},
    runtime::{
        DriverError, DriverFuture, ProviderDriver, ProviderSessionAttachment,
        ProviderTurnCompleted, ProviderTurnRequest,
    },
};
#[cfg(unix)]
use crate::{
    guardian::GuardianLaunch,
    launch_cleanup,
    opencode_startup::{drain_output, observe_startup},
    runtime_lease::HeldRuntimeLease,
    unix_custody::UnixProcessCustody,
};
use session_creation::{SessionCreationAuthority, guarded_session_creation};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const TURN_TIMEOUT: Duration = Duration::from_mins(3);
const MCP_NAME: &str = "agentsassemble_room";
// OpenCode 2 accepts Basic auth only under this fixed username; the password
// still comes from the per-launch OPENCODE_SERVER_PASSWORD capability.
const SERVER_USERNAME: &str = "opencode";

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
    config_root: tempfile::TempDir,
    /// Holds an API agent's key outside the OpenCode process for the driver's life.
    _key_broker: Option<KeyBroker>,
    session_instructions: Option<String>,
    attached_session_id: Option<String>,
    attached_reused: bool,
    session_creation: SessionCreationAuthority,
    mcp_registered: bool,
    api: OpenCodeApi,
    active_turn: Option<ProviderTurnRequest>,
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
        Self::spawn_inner(session, runtime_lease, guardian, None, None).await
    }

    /// Starts OpenCode for an API provider. The key stays in the broker; OpenCode
    /// only learns the broker's loopback address and per-launch token.
    #[cfg(unix)]
    pub(crate) async fn spawn_harness(
        session: &DurableAgentSession,
        runtime_lease: &HeldRuntimeLease,
        guardian: &GuardianLaunch,
        broker: KeyBroker,
        conversation_workspace: Option<std::path::PathBuf>,
    ) -> Result<Self, DriverLaunchError> {
        Self::spawn_inner(
            session,
            runtime_lease,
            guardian,
            Some(broker),
            conversation_workspace,
        )
        .await
    }

    #[cfg(not(unix))]
    pub(crate) async fn spawn(session: &DurableAgentSession) -> Result<Self, DriverLaunchError> {
        Self::spawn_inner(session, None, None).await
    }

    #[cfg(not(unix))]
    pub(crate) async fn spawn_harness(
        session: &DurableAgentSession,
        broker: KeyBroker,
        conversation_workspace: Option<std::path::PathBuf>,
    ) -> Result<Self, DriverLaunchError> {
        Self::spawn_inner(session, Some(broker), conversation_workspace).await
    }

    async fn spawn_inner(
        session: &DurableAgentSession,
        #[cfg(unix)] runtime_lease: &HeldRuntimeLease,
        #[cfg(unix)] guardian: &GuardianLaunch,
        key_broker: Option<KeyBroker>,
        conversation_workspace: Option<std::path::PathBuf>,
    ) -> Result<Self, DriverLaunchError> {
        validate_profile(session)?;
        // Only the OpenCode harness may run without an owner-selected workspace.
        let workspace_path = if session.workspace.is_empty() {
            conversation_workspace.ok_or_else(|| DriverLaunchError::safe(profile_error()))?
        } else {
            std::path::PathBuf::from(&session.workspace)
        };
        let workspace = workspace_path.as_path();
        let config_root = isolated_config_root()?;
        let server_password = server_password();
        let mut environment = isolated_environment(config_root.path(), &server_password)?;
        if let Some(broker) = &key_broker {
            let content = broker.config_content();
            match environment
                .iter_mut()
                .find(|(name, _)| name == "OPENCODE_CONFIG_CONTENT")
            {
                Some((_, value)) => *value = content,
                None => environment.push(("OPENCODE_CONFIG_CONTENT".to_owned(), content)),
            }
        }
        let port = reserve_loopback_port().await?;
        let endpoint = format!("http://127.0.0.1:{port}/");
        let ready_line = format!("server listening on http://127.0.0.1:{port}");
        let arguments = server_arguments(port);
        let executable = bind_executable_with_children(
            session.executable.clone(),
            session.executable_identity.clone(),
        )
        .await
        .map_err(|_| executable_error())?;
        let http = LoopbackHttp::new(&endpoint, workspace, SERVER_USERNAME, &server_password)
            .map_err(http_driver_error)?;
        let mut room_portal = RoomPortal::create().await.map_err(DriverError::from)?;
        #[cfg(unix)]
        let started = UnixProcessCustody::start_with_children(
            runtime_lease,
            guardian,
            &executable,
            &arguments,
            &environment,
            workspace,
        )
        .await;
        #[cfg(unix)]
        let (process_group, pipes) = match started {
            Ok(started) => started,
            Err(error) => return Err(launch_cleanup::portal(&mut room_portal, error).await),
        };
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
        let (child, stdout_task, stderr_task, startup) = crate::opencode_process::start(
            &executable,
            &arguments,
            workspace,
            &environment,
            ready_line,
            &mut room_portal,
        )
        .await?;
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
            config_root,
            _key_broker: key_broker,
            session_instructions: None,
            attached_session_id: None,
            attached_reused: false,
            session_creation: SessionCreationAuthority::default(),
            mcp_registered: false,
            api: OpenCodeApi::V1,
            active_turn: None,
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
            // OpenCode 2 serves the JSON API under /api and reports its version from
            // /api/info. OpenCode 1 only has /global/health. Probe both once per
            // launch; afterwards every request uses that single decided shape.
            if let Ok(connection) = self.connect_owned_peer().await
                && let Ok(response) = connection
                    .get_json("/api/info", Duration::from_millis(500))
                    .await
                && response.status.is_success()
                && response
                    .value
                    .get("version")
                    .and_then(Value::as_str)
                    .is_some()
            {
                self.api = OpenCodeApi::V2;
                return Ok(());
            }
            if let Ok(connection) = self.connect_owned_peer().await
                && let Ok(response) = connection
                    .get_json("/global/health", Duration::from_millis(500))
                    .await
                && response.status.is_success()
                && response.value.get("healthy").and_then(Value::as_bool) == Some(true)
            {
                self.api = OpenCodeApi::V1;
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
        let headers = json!({
            "Authorization": format!("Bearer {}", self.room_portal.bearer_token()),
        });
        if self.api == OpenCodeApi::V1 {
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
                            "headers": headers,
                            "timeout": 10_000,
                        },
                    }),
                    STARTUP_TIMEOUT,
                )
                .await
                .map_err(http_driver_error)?;
            return if response.status.is_success()
                && response
                    .value
                    .get(MCP_NAME)
                    .and_then(Value::as_object)
                    .and_then(|status| status.get("status"))
                    .and_then(Value::as_str)
                    == Some("connected")
            {
                Ok(())
            } else {
                Err(ROOM_PORTAL_UNAVAILABLE)
            };
        }
        let response = self
            .connect_owned_peer()
            .await?
            .put_json(
                &self.api.mcp_path(MCP_NAME)?,
                &json!({
                    "config": {
                        "type": "remote",
                        "url": self.room_portal.endpoint(),
                        "disabled": false,
                        "headers": headers,
                        "timeout": {"startup": 10_000},
                    },
                }),
                STARTUP_TIMEOUT,
            )
            .await
            .map_err(http_driver_error)?;
        if !response.status.is_success() {
            return Err(ROOM_PORTAL_UNAVAILABLE);
        }
        self.wait_for_mcp_connected().await
    }

    async fn wait_for_mcp_connected(&mut self) -> Result<(), DriverError> {
        let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
        loop {
            if let Ok(connection) = self.connect_owned_peer().await
                && let Ok(response) = connection
                    .get_json(self.api.mcp_list_path(), Duration::from_millis(1_000))
                    .await
                && response.status.is_success()
            {
                let connected = response
                    .value
                    .get("data")
                    .and_then(Value::as_array)
                    .is_some_and(|servers| {
                        servers.iter().any(|server| {
                            server.get("name").and_then(Value::as_str) == Some(MCP_NAME)
                                && server.pointer("/status/status").and_then(Value::as_str)
                                    == Some("connected")
                        })
                    });
                if connected {
                    return Ok(());
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(ROOM_PORTAL_UNAVAILABLE);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn create_session(
        &mut self,
        session: &DurableAgentSession,
    ) -> Result<String, DriverError> {
        let permission_action = match session.public.permission_mode.as_str() {
            "meeting_read_only" => "deny",
            "full_access" => "allow",
            _ => "ask",
        };
        let model = session_model(session)?;
        let (provider, model_id) = model_parts(&model, is_harness(session))?;
        let mut model = json!({
            "id": model_id,
            "providerID": provider,
        });
        if !session.public.variant.is_empty() {
            model["variant"] = json!(session.public.variant);
        }
        let external = if permission_action == "allow" {
            "allow"
        } else {
            "deny"
        };
        let (path, payload) = match self.api {
            OpenCodeApi::V1 => (
                "/session".to_owned(),
                json!({
                    "title": format!("AgentsAssemble {}", session.public.session_id),
                    "model": model,
                    "permission": [
                        {"permission": "*", "pattern": "*", "action": permission_action},
                        {"permission": "read", "pattern": "*", "action": "allow"},
                        {"permission": "glob", "pattern": "*", "action": "allow"},
                        {"permission": "grep", "pattern": "*", "action": "allow"},
                        {"permission": "list", "pattern": "*", "action": "allow"},
                        {"permission": "external_directory", "pattern": "*", "action": external},
                        {"permission": "agentsassemble_room_*", "pattern": "*", "action": "allow"},
                    ],
                }),
            ),
            OpenCodeApi::V2 => (
                self.api.sessions_path().to_owned(),
                json!({
                    "title": format!("AgentsAssemble {}", session.public.session_id),
                    "agent": "build",
                    "model": model,
                    "permissions": [
                        {"action": "*", "resource": "*", "effect": permission_action},
                        {"action": "read", "resource": "*", "effect": "allow"},
                        {"action": "glob", "resource": "*", "effect": "allow"},
                        {"action": "grep", "resource": "*", "effect": "allow"},
                        {"action": "list", "resource": "*", "effect": "allow"},
                        // OpenCode 2 exposes every tool through the Code Mode `execute`
                        // wrapper. Denying it also hides the room MCP namespace, while
                        // nested shell/file tools still follow their own rules.
                        {"action": "execute", "resource": "*", "effect": "allow"},
                        {"action": "external_directory", "resource": "*", "effect": external},
                        {"action": "agentsassemble_room_*", "resource": "*", "effect": "allow"},
                    ],
                }),
            ),
        };
        let connection = self.connect_owned_peer().await?;
        let api = self.api;
        guarded_session_creation(&mut self.session_creation, async move {
            let response = connection
                .post_json(&path, &payload, REQUEST_TIMEOUT)
                .await
                .map_err(http_driver_error)?;
            if !response.status.is_success() {
                return Err(provider_request_error());
            }
            let id = match api {
                OpenCodeApi::V1 => response.value.get("id"),
                OpenCodeApi::V2 => response.value.pointer("/data/id"),
            };
            id.and_then(Value::as_str)
                .and_then(clean_session_id)
                .ok_or_else(session_unconfirmed)
        })
        .await
    }

    async fn require_session(&mut self, provider_session_id: &str) -> Result<(), DriverError> {
        let response = self
            .connect_owned_peer()
            .await?
            .get_json(
                &session_path(self.api, provider_session_id)?,
                REQUEST_TIMEOUT,
            )
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
        if let Some(active) = &self.active_turn {
            if active != request {
                return Err(turn_in_progress());
            }
        } else {
            self.active_turn = Some(request.clone());
        }
        if self.session_instructions != request.session_instructions {
            // Native instructions are reloaded into the system prompt after compaction.
            // A custom agent prompt makes OpenCode reject free-tier model requests.
            let instruction_path = self.config_root.path().join("room-instructions.md");
            std::fs::write(
                &instruction_path,
                request.session_instructions.as_deref().unwrap_or_default(),
            )
            .map_err(|_| config_error())?;
            let config = json!({"instructions": [instruction_path]});
            std::fs::write(
                self.config_root.path().join("opencode.json"),
                config.to_string(),
            )
            .map_err(|_| config_error())?;
            let response = self
                .connect_owned_peer()
                .await?
                .post_json(self.api.reload_path(), &json!({}), STARTUP_TIMEOUT)
                .await
                .map_err(http_driver_error)?;
            let reloaded = match self.api {
                OpenCodeApi::V1 => response.value == json!(true),
                OpenCodeApi::V2 => response.status == StatusCode::NO_CONTENT,
            };
            if !response.status.is_success() || !reloaded {
                return Err(config_error());
            }
            self.register_room_portal().await?;
            self.session_instructions
                .clone_from(&request.session_instructions);
        }
        let event_response = self
            .connect_owned_peer()
            .await?
            .get_stream(self.api.event_path(), TURN_TIMEOUT)
            .await
            .map_err(http_driver_error)?;
        let (path, payload) = match self.api {
            OpenCodeApi::V1 => {
                let model = session_model(session)?;
                let (provider, model_id) = model_parts(&model, is_harness(session))?;
                let mut payload = json!({
                    "model": {
                        "providerID": provider,
                        "modelID": model_id,
                    },
                    "agent": "build",
                    "parts": [{"type": "text", "text": request.input}],
                });
                if !session.public.variant.is_empty() {
                    payload["variant"] = json!(session.public.variant);
                }
                (
                    format!("{}/message", session_path(self.api, &attached)?),
                    payload,
                )
            }
            OpenCodeApi::V2 => (
                format!("{}/prompt", session_path(self.api, &attached)?),
                json!({"text": request.input}),
            ),
        };
        let prompt_connection = self.connect_owned_peer().await?;
        let (prompt, events) = match self
            .run_turn_transport(
                session,
                request,
                prompt_connection,
                event_response,
                &payload,
                &path,
            )
            .await
        {
            Ok(joined) => joined,
            Err(error) => return self.poison(error),
        };
        let completed =
            Self::completed_from_response(session, request, &attached, &prompt, &events, self.api);
        match completed {
            Ok(completed) => {
                self.active_turn = None;
                self.completed_turn = Some((request.clone(), completed.clone()));
                Ok(completed)
            }
            Err(error) => self.poison(error),
        }
    }

    fn completed_from_response(
        session: &DurableAgentSession,
        request: &ProviderTurnRequest,
        attached: &str,
        prompt: &JsonResponse,
        events: &OpenCodeTurnEvents,
        api: OpenCodeApi,
    ) -> Result<ProviderTurnCompleted, DriverError> {
        if !prompt.status.is_success()
            || prompt
                .value
                .pointer("/info/error")
                .is_some_and(Value::is_object)
        {
            return Err(provider_response_error(&prompt.value));
        }
        let (provider_turn_id, content) = match api {
            OpenCodeApi::V1 => {
                let message = assistant_message(&prompt.value)?;
                if message.parent_id != events.request_message {
                    return Err(turn_mismatch());
                }
                if !events.assistant_message.is_empty() && message.id != events.assistant_message {
                    return Err(turn_mismatch());
                }
                if message.observed_model != session_model(session)?
                    || events.observed_model != session_model(session)?
                    || message.observed_model != events.observed_model
                {
                    return Err(model_mismatch());
                }
                (message.id, message.content)
            }
            OpenCodeApi::V2 => {
                let request_message = prompt
                    .value
                    .pointer("/data/id")
                    .and_then(Value::as_str)
                    .and_then(clean_session_id);
                if request_message.as_deref() != Some(events.request_message.as_str()) {
                    return Err(turn_mismatch());
                }
                if events.assistant_message.is_empty() {
                    return Err(turn_empty());
                }
                if events.observed_model != session_model(session)? {
                    return Err(model_mismatch());
                }
                (events.assistant_message.clone(), events.content.clone())
            }
        };
        if content.is_empty() && request.room_observation.is_none() {
            return Err(turn_empty());
        }
        Ok(ProviderTurnCompleted {
            memory_summary: None,
            usage: None,
            turn_id: request.turn_id.clone(),
            provider_turn_id,
            provider_session_id: Some(attached.to_owned()),
            outcome: ProviderTurnOutcome::Message {
                attachment_ids: Vec::new(),
                content,
                target_agent_id: String::new(),
                reply_to_event_id: None,
            },
        })
    }

    async fn abort_session(&mut self, session_id: &str) -> Result<(), DriverError> {
        let Ok(path) = session_path(self.api, session_id) else {
            return Err(protocol_error());
        };
        let (path, payload) = match self.api {
            OpenCodeApi::V1 => (format!("{path}/abort"), json!({})),
            OpenCodeApi::V2 => (format!("{path}/interrupt"), json!({})),
        };
        let response = self
            .connect_owned_peer()
            .await?
            .post_json(&path, &payload, Duration::from_secs(5))
            .await
            .map_err(http_driver_error)?;
        let interrupted = match self.api {
            OpenCodeApi::V1 => response.value.as_bool() == Some(true),
            OpenCodeApi::V2 => response
                .value
                .get("interrupted")
                .and_then(Value::as_bool)
                .is_some(),
        };
        if !response.status.is_success() || !interrupted {
            return Err(provider_request_error());
        }
        Ok(())
    }

    async fn interrupt(
        &mut self,
        session: &DurableAgentSession,
        request: &ProviderTurnRequest,
    ) -> Result<(), DriverError> {
        let attached = self
            .attached_session_id
            .clone()
            .ok_or_else(session_unconfirmed)?;
        if session.provider_session_id != attached {
            return Err(session_mismatch());
        }
        let Some(active) = &self.active_turn else {
            return Ok(());
        };
        if active != request {
            return Err(turn_in_progress());
        }
        let events = self
            .connect_owned_peer()
            .await?
            .get_stream(self.api.event_path(), Duration::from_secs(10))
            .await
            .map_err(http_driver_error)?;
        self.abort_session(&attached).await?;
        crate::opencode_sse::wait_session_idle(events, &attached, Duration::from_secs(10))
            .await
            .map_err(turn_transport_error)?;
        self.active_turn = None;
        self.completed_turn = None;
        Ok(())
    }

    async fn stop_process(&mut self) -> Result<(), DriverError> {
        #[cfg(unix)]
        let process = self.process_group.stop().await;
        #[cfg(not(unix))]
        let process = crate::opencode_process::stop(self.child.as_mut()).await;
        self.stdout_task.abort();
        self.stderr_task.abort();
        let _ = (&mut self.stdout_task).await;
        let _ = (&mut self.stderr_task).await;
        let portal = self.room_portal.shutdown().await.map_err(DriverError::from);
        process.and(portal)
    }

    fn poison<T>(&mut self, error: DriverError) -> Result<T, DriverError> {
        self.poisoned = true;
        Err(error)
    }
}

// OpenCode 2 refuses the old `--pure` flag and upper-case log levels. Purity comes
// from the private HOME/config roots and disabled project, plugin and skill config;
// logs stay in the OpenCode data root's log file.
pub(crate) fn server_arguments(port: u16) -> Vec<String> {
    vec![
        "serve".to_owned(),
        "--hostname".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
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

impl ProviderDriver for OpenCodeDriver {
    fn retains_runtime_after_turn_interrupt(&self) -> bool {
        crate::registration::OPENCODE_PROVIDER
            .turn_interrupt
            .retains_runtime()
    }

    fn attach_session<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>> {
        Box::pin(self.attach(session))
    }

    fn attachment_replay_is_safe(&self) -> bool {
        self.session_creation.replay_is_safe()
    }

    fn send_turn<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnCompleted, DriverError>> {
        Box::pin(self.send(session, request))
    }

    fn interrupt_turn<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<(), DriverError>> {
        Box::pin(self.interrupt(session, request))
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

    fn begin_room_observation<'a>(
        &'a mut self,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<(), DriverError>> {
        Box::pin(async move {
            self.room_portal
                .begin_turn(request)
                .map_err(DriverError::from)
        })
    }

    fn finish_room_observation<'a>(
        &'a mut self,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnOutcome, DriverError>> {
        Box::pin(async move {
            self.room_portal
                .finish_turn(request)
                .map_err(DriverError::from)
        })
    }

    fn abort_room_observation(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(async move {
            self.room_portal
                .end_observation()
                .map_err(DriverError::from)
        })
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

#[cfg(test)]
mod opencode_tests;
