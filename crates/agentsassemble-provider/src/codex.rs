use std::{collections::VecDeque, time::Duration};

#[cfg(not(unix))]
use std::{io, process::Stdio};

use agentsassemble_domain::DurableAgentSession;
use futures_util::StreamExt;
#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(not(unix))]
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    task::JoinHandle,
};
use tokio_util::codec::{FramedRead, LinesCodec};

#[cfg(not(any(target_os = "linux", target_os = "android")))]
use crate::filesystem::BoundExecutable;
#[cfg(not(unix))]
use crate::process::sanitize_environment;
use crate::{
    codex_identity::{
        checked_provider_session_id, observed_model_id_from_response,
        provider_session_id_from_response, provider_session_mismatch, provider_session_unconfirmed,
    },
    filesystem::bind_executable,
    launch_error::DriverLaunchError,
    runtime::{DriverError, DriverFuture, ProviderDriver, ProviderSessionAttachment},
};
#[cfg(unix)]
use crate::{
    guardian::GuardianLaunch, runtime_lease::HeldRuntimeLease, unix_custody::UnixProcessCustody,
};

const PROTOCOL_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(not(unix))]
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PROTOCOL_LINE_BYTES: usize = 256 * 1024;
const MAX_PENDING_NOTIFICATIONS: usize = 256;
const MAX_PENDING_NOTIFICATION_BYTES: usize = 2 * 1024 * 1024;
struct PendingRequest {
    id: u64,
    method: String,
    params: Value,
}

pub(crate) struct CodexDriver {
    #[cfg(not(unix))]
    child: Box<dyn ChildWrapper>,
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    _executable_guard: BoundExecutable,
    #[cfg(unix)]
    process_group: UnixProcessCustody,
    stdin: ProviderStdin,
    stdout: FramedRead<ProviderStdout, LinesCodec>,
    stderr_task: JoinHandle<()>,
    next_request_id: u64,
    pending_notifications: VecDeque<Value>,
    pending_notification_bytes: usize,
    pending_request: Option<PendingRequest>,
    initialization_error: Option<DriverError>,
    initialize_acked: bool,
    initialized_notification_started: bool,
    initialized: bool,
    attached_thread_id: Option<String>,
    attached_observed_model_id: Option<String>,
    attachment_error: Option<DriverError>,
}

#[cfg(unix)]
type ProviderStdin = tokio::fs::File;
#[cfg(not(unix))]
type ProviderStdin = tokio::process::ChildStdin;
#[cfg(unix)]
type ProviderStdout = tokio::fs::File;
#[cfg(not(unix))]
type ProviderStdout = tokio::process::ChildStdout;

impl CodexDriver {
    #[cfg(unix)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        runtime_lease: &HeldRuntimeLease,
        guardian_launch: &GuardianLaunch,
    ) -> Result<Self, DriverLaunchError> {
        Self::spawn_inner(session, runtime_lease, guardian_launch).await
    }

    #[cfg(not(unix))]
    pub(crate) async fn spawn(session: &DurableAgentSession) -> Result<Self, DriverLaunchError> {
        Self::spawn_inner(session).await
    }

    async fn spawn_inner(
        session: &DurableAgentSession,
        #[cfg(unix)] runtime_lease: &HeldRuntimeLease,
        #[cfg(unix)] guardian_launch: &GuardianLaunch,
    ) -> Result<Self, DriverLaunchError> {
        #[cfg(not(any(unix, windows)))]
        return Err(DriverError::new(
            "provider_runtime_unsupported",
            "Provider processes are unsupported on this platform.",
        )
        .into());

        let arguments = command_arguments(session)?;
        let executable = bind_executable(
            session.executable.clone(),
            session.executable_identity.clone(),
        )
        .await
        .map_err(|_| executable_authority_error())?;
        #[cfg(unix)]
        {
            let (process_group, pipes) =
                UnixProcessCustody::start(runtime_lease, guardian_launch, &executable, &arguments)
                    .await?;
            let stderr_task = tokio::spawn(drain_stderr(pipes.stderr));
            Ok(Self {
                process_group,
                #[cfg(not(any(target_os = "linux", target_os = "android")))]
                _executable_guard: executable,
                stdin: pipes.stdin,
                stdout: FramedRead::new(
                    pipes.stdout,
                    LinesCodec::new_with_max_length(MAX_PROTOCOL_LINE_BYTES),
                ),
                stderr_task,
                next_request_id: 1,
                pending_notifications: VecDeque::new(),
                pending_notification_bytes: 0,
                pending_request: None,
                initialization_error: None,
                initialize_acked: false,
                initialized_notification_started: false,
                initialized: false,
                attached_thread_id: None,
                attached_observed_model_id: None,
                attachment_error: None,
            })
        }
        #[cfg(not(unix))]
        {
            let mut command = CommandWrap::with_new(executable.launch_path(), |command| {
                command
                    .args(&arguments)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
            });
            sanitize_environment(command.command_mut());
            command.wrap(KillOnDrop);
            #[cfg(windows)]
            command.wrap(JobObject);
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(error) => return Err(spawn_error(&error).into()),
            };
            drop(command);
            let Some(stdin) = child.stdin().take() else {
                return Err(failed_spawn_error(child.as_mut()).await);
            };
            let Some(stdout) = child.stdout().take() else {
                return Err(failed_spawn_error(child.as_mut()).await);
            };
            let Some(stderr) = child.stderr().take() else {
                return Err(failed_spawn_error(child.as_mut()).await);
            };
            let stderr_task = tokio::spawn(drain_stderr(stderr));
            Ok(Self {
                child,
                _executable_guard: executable,
                stdin,
                stdout: FramedRead::new(
                    stdout,
                    LinesCodec::new_with_max_length(MAX_PROTOCOL_LINE_BYTES),
                ),
                stderr_task,
                next_request_id: 1,
                pending_notifications: VecDeque::new(),
                pending_notification_bytes: 0,
                pending_request: None,
                initialization_error: None,
                initialize_acked: false,
                initialized_notification_started: false,
                initialized: false,
                attached_thread_id: None,
                attached_observed_model_id: None,
                attachment_error: None,
            })
        }
    }

    async fn initialize(&mut self) -> Result<(), DriverError> {
        if self.initialized {
            return Ok(());
        }
        if let Some(error) = self.initialization_error {
            return Err(error);
        }
        if !self.initialize_acked {
            let initialized = self
                .request(
                    "initialize",
                    json!({"clientInfo": {"name": "AgentsAssemble", "version": "0"}}),
                )
                .await;
            if let Err(error) = initialized {
                if self.pending_request.is_none() {
                    self.initialization_error = Some(error);
                }
                return Err(error);
            }
            self.initialize_acked = true;
        }
        if self.initialized_notification_started {
            return Err(initialization_uncertain());
        }
        self.initialized_notification_started = true;
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {},
        }))
        .await?;
        self.initialized = true;
        Ok(())
    }

    async fn attach(
        &mut self,
        session: &DurableAgentSession,
    ) -> Result<ProviderSessionAttachment, DriverError> {
        self.initialize().await?;
        if let Some(error) = self.attachment_error {
            return Err(error);
        }
        let durable_id = checked_provider_session_id(&session.provider_session_id)?;
        if let Some(thread_id) = &self.attached_thread_id {
            if durable_id.is_some_and(|durable| durable != thread_id) {
                return Err(provider_session_mismatch());
            }
            return Ok(ProviderSessionAttachment {
                provider_session_id: thread_id.clone(),
                reused: durable_id.is_some(),
                observed_model_id: self.attached_observed_model_id.clone(),
            });
        }
        let (method, params) = match durable_id {
            Some(thread_id) => ("thread/resume", json!({"threadId": thread_id})),
            None => ("thread/start", thread_start_params(session)?),
        };
        let response = match self.request(method, params).await {
            Ok(response) => response,
            Err(error) => {
                if self.pending_request.is_none() {
                    self.attachment_error = Some(error);
                }
                return Err(error);
            }
        };
        let observed_id = match provider_session_id_from_response(&response) {
            Ok(observed_id) => observed_id,
            Err(error) => return self.poison_attachment(error),
        };
        let observed_model_id = match observed_model_id_from_response(&response) {
            Ok(observed_model_id) => observed_model_id.map(str::to_owned),
            Err(error) => return self.poison_attachment(error),
        };
        let thread_id = match (durable_id, observed_id) {
            (Some(expected), Some(observed)) if expected != observed => {
                return self.poison_attachment(provider_session_mismatch());
            }
            (Some(expected), Some(_)) => expected.to_owned(),
            (None, Some(observed)) => observed.to_owned(),
            (Some(_) | None, None) => {
                return self.poison_attachment(provider_session_unconfirmed());
            }
        };
        self.attached_thread_id = Some(thread_id.clone());
        self.attached_observed_model_id
            .clone_from(&observed_model_id);
        Ok(ProviderSessionAttachment {
            provider_session_id: thread_id,
            reused: durable_id.is_some(),
            observed_model_id,
        })
    }

    fn poison_attachment<T>(&mut self, error: DriverError) -> Result<T, DriverError> {
        self.attachment_error = Some(error);
        Err(error)
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, DriverError> {
        let request_id = if let Some(pending) = &self.pending_request {
            if pending.method != method || pending.params != params {
                return Err(request_in_progress());
            }
            pending.id
        } else {
            let request_id = self.next_request_id;
            self.next_request_id = self
                .next_request_id
                .checked_add(1)
                .ok_or_else(protocol_error)?;
            self.pending_request = Some(PendingRequest {
                id: request_id,
                method: method.to_owned(),
                params: params.clone(),
            });
            self.write_message(&json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params,
            }))
            .await?;
            request_id
        };
        tokio::time::timeout(PROTOCOL_TIMEOUT, self.read_response(request_id))
            .await
            .map_err(|_| {
                DriverError::new(
                    "provider_protocol_timeout",
                    "The Codex app-server did not answer within its protocol deadline.",
                )
            })?
    }

    async fn read_response(&mut self, request_id: u64) -> Result<Value, DriverError> {
        loop {
            let line = self
                .stdout
                .next()
                .await
                .ok_or_else(protocol_closed)?
                .map_err(|_| protocol_error())?;
            let message = serde_json::from_str::<Value>(&line).map_err(|_| protocol_error())?;
            let object = message.as_object().ok_or_else(protocol_error)?;
            if object.get("method").is_some() {
                if object.get("id").is_some() {
                    self.reject_server_request(&message).await?;
                } else {
                    self.pending_notification_bytes = next_notification_budget(
                        self.pending_notifications.len(),
                        self.pending_notification_bytes,
                        line.len(),
                    )?;
                    self.pending_notifications.push_back(message);
                }
                continue;
            }
            if object.get("id").and_then(Value::as_u64) != Some(request_id) {
                return Err(DriverError::new(
                    "provider_protocol_mismatch",
                    "The Codex app-server returned an unmatched response.",
                ));
            }
            self.pending_request = None;
            if object.get("error").is_some_and(|value| !value.is_null()) {
                return Err(DriverError::new(
                    "provider_request_rejected",
                    "The Codex app-server rejected a provider request.",
                ));
            }
            if object.get("result").is_none() {
                return Err(protocol_error());
            }
            return Ok(message);
        }
    }

    async fn reject_server_request(&mut self, message: &Value) -> Result<(), DriverError> {
        let id = message.get("id").cloned().ok_or_else(protocol_error)?;
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": "Unsupported provider request.",
            },
        }))
        .await
    }

    async fn write_message(&mut self, message: &Value) -> Result<(), DriverError> {
        let mut encoded = serde_json::to_vec(message).map_err(|_| protocol_error())?;
        if encoded.len() > MAX_PROTOCOL_LINE_BYTES {
            return Err(DriverError::new(
                "provider_protocol_overflow",
                "The Codex app-server request exceeded its protocol bound.",
            ));
        }
        encoded.push(b'\n');
        self.stdin
            .write_all(&encoded)
            .await
            .map_err(|_| protocol_closed())?;
        self.stdin.flush().await.map_err(|_| protocol_closed())
    }

    async fn stop_process(&mut self) -> Result<(), DriverError> {
        #[cfg(unix)]
        let stopped = self.process_group.stop().await;
        #[cfg(not(unix))]
        let stopped = tokio::time::timeout(STOP_TIMEOUT, Box::into_pin(self.child.kill())).await;
        #[cfg(not(unix))]
        let stopped = stopped.map_err(|_| {
            DriverError::new(
                "provider_stop_unconfirmed",
                "The Codex app-server exceeded its shutdown deadline.",
            )
        })?;
        stopped.map_err(|_| {
            DriverError::new(
                "provider_stop_unconfirmed",
                "The Codex app-server shutdown could not be confirmed.",
            )
        })?;
        self.stderr_task.abort();
        let _ = (&mut self.stderr_task).await;
        Ok(())
    }
}

impl ProviderDriver for CodexDriver {
    fn attach_session<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>> {
        Box::pin(self.attach(session))
    }

    fn is_alive(&mut self) -> Result<bool, DriverError> {
        #[cfg(unix)]
        return self.process_group.leader_is_running();
        #[cfg(not(unix))]
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|_| {
                DriverError::new(
                    "provider_health_unknown",
                    "The Codex app-server health could not be observed.",
                )
            })
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(self.stop_process())
    }
}

impl Drop for CodexDriver {
    fn drop(&mut self) {
        #[cfg(unix)]
        self.process_group.request_stop();
        self.stderr_task.abort();
    }
}

fn command_arguments(session: &DurableAgentSession) -> Result<Vec<String>, DriverError> {
    let (approval, sandbox) = profile_permissions(session)?;
    if session.public.model.is_empty() {
        return Err(DriverError::new(
            "invalid_runtime_profile",
            "The Codex runtime profile has no model.",
        ));
    }
    let mut arguments = vec!["app-server".to_owned()];
    push_config(&mut arguments, "model", &session.public.model)?;
    if !session.public.reasoning_effort.is_empty() {
        push_config(
            &mut arguments,
            "model_reasoning_effort",
            &session.public.reasoning_effort,
        )?;
    }
    if !session.public.service_tier.is_empty() && session.public.service_tier != "default" {
        push_config(&mut arguments, "service_tier", &session.public.service_tier)?;
    }
    push_config(&mut arguments, "sandbox_mode", sandbox)?;
    push_config(&mut arguments, "approval_policy", approval)?;
    let project_key = format!("projects.{}.trust_level", json_string(&session.workspace)?);
    push_config(&mut arguments, &project_key, "trusted")?;
    arguments.push("--stdio".to_owned());
    Ok(arguments)
}

fn thread_start_params(session: &DurableAgentSession) -> Result<Value, DriverError> {
    let (approval, sandbox) = profile_permissions(session)?;
    if session.workspace.is_empty() || session.public.model.is_empty() {
        return Err(invalid_runtime_profile());
    }
    Ok(json!({
        "cwd": session.workspace,
        "model": session.public.model,
        "approvalPolicy": approval,
        "sandbox": sandbox,
    }))
}

fn profile_permissions(
    session: &DurableAgentSession,
) -> Result<(&'static str, &'static str), DriverError> {
    match session.public.permission_mode.as_str() {
        "workspace_write" => Ok(("on-request", "workspace-write")),
        "meeting_read_only" => Ok(("never", "read-only")),
        _ => Err(invalid_runtime_profile()),
    }
}

fn push_config(arguments: &mut Vec<String>, key: &str, value: &str) -> Result<(), DriverError> {
    arguments.push("-c".to_owned());
    arguments.push(format!("{key}={}", json_string(value)?));
    Ok(())
}

fn json_string(value: &str) -> Result<String, DriverError> {
    serde_json::to_string(value).map_err(|_| protocol_error())
}

async fn drain_stderr(mut stderr: impl AsyncRead + Unpin) {
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

#[cfg(not(unix))]
async fn failed_spawn_error(child: &mut dyn ChildWrapper) -> DriverLaunchError {
    match tokio::time::timeout(STOP_TIMEOUT, Box::into_pin(child.kill())).await {
        Ok(Ok(())) => DriverLaunchError::safe(protocol_error()),
        Ok(Err(_)) | Err(_) => DriverLaunchError::uncertain(protocol_error()),
    }
}

#[cfg(not(unix))]
fn spawn_error(error: &io::Error) -> DriverError {
    if error.kind() == io::ErrorKind::NotFound {
        DriverError::new(
            "provider_executable_missing",
            "The Codex executable is no longer available.",
        )
    } else {
        DriverError::new(
            "provider_spawn_failed",
            "The Codex app-server process could not be started.",
        )
    }
}

const fn protocol_error() -> DriverError {
    DriverError::new(
        "provider_protocol_invalid",
        "The Codex app-server returned an invalid protocol message.",
    )
}

const fn protocol_closed() -> DriverError {
    DriverError::new(
        "provider_protocol_closed",
        "The Codex app-server protocol stream closed unexpectedly.",
    )
}

const fn initialization_uncertain() -> DriverError {
    DriverError::new(
        "provider_initialization_uncertain",
        "The Codex initialized notification may already have been sent.",
    )
}

const fn request_in_progress() -> DriverError {
    DriverError::new(
        "provider_request_in_progress",
        "A different Codex provider request may still be in progress.",
    )
}

const fn invalid_runtime_profile() -> DriverError {
    DriverError::new(
        "invalid_runtime_profile",
        "The Codex runtime profile is invalid.",
    )
}

const fn executable_authority_error() -> DriverError {
    DriverError::new(
        "executable_authority_changed",
        "The provider executable authority changed before process creation.",
    )
}

fn next_notification_budget(
    retained_count: usize,
    retained_bytes: usize,
    next_bytes: usize,
) -> Result<usize, DriverError> {
    if retained_count >= MAX_PENDING_NOTIFICATIONS {
        return Err(notification_overflow());
    }
    retained_bytes
        .checked_add(next_bytes)
        .filter(|total| *total <= MAX_PENDING_NOTIFICATION_BYTES)
        .ok_or_else(notification_overflow)
}

const fn notification_overflow() -> DriverError {
    DriverError::new(
        "provider_protocol_overflow",
        "The Codex app-server notification queue exceeded its bound.",
    )
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::DurableAgentSession;

    use super::{
        MAX_PENDING_NOTIFICATION_BYTES, MAX_PENDING_NOTIFICATIONS, command_arguments,
        next_notification_budget,
    };

    #[test]
    fn command_uses_app_server_and_process_local_profile_settings() {
        let mut session = serde_json::from_value::<DurableAgentSession>(serde_json::json!({
            "room_id": "room",
            "session_id": "agent",
            "participant_id": "agent",
            "display_name": "Codex",
            "status": "available",
            "runtime_status": "starting",
            "enabled": true,
            "provider_kind": "codex_live_session",
            "runtime_kind": "live_cli",
            "connection_kind": "native_cli_bridge",
            "external_owned": false,
            "process_ownership": "server",
            "model": "gpt-5.6-terra",
            "reasoning_effort": "high",
            "service_tier": "priority",
            "variant": "",
            "execution_harness": "builtin",
            "permission_mode": "workspace_write",
            "max_output_tokens": 0,
            "catalog_revision": "revision",
            "transport": "stdio_jsonl",
            "last_seen_event_id": "",
            "last_seen_seq": 0,
            "last_provider_sync_event_id": "",
            "last_provider_sync_seq": 0,
            "bootstrap_cutoff_seq": 0,
            "turn_count": 0,
            "created_at": "2026-08-23T00:00:00Z",
            "updated_at": "2026-08-23T00:00:00Z",
            "workspace": "/tmp/work space",
            "runtime_profile_key": "profile"
        }))
        .unwrap_or_else(|error| panic!("decode session fixture: {error}"));
        session.executable = "/bin/codex".to_owned();
        let arguments = command_arguments(&session)
            .unwrap_or_else(|error| panic!("build app-server command: {error}"));
        assert_eq!(arguments.first().map(String::as_str), Some("app-server"));
        assert_eq!(arguments.last().map(String::as_str), Some("--stdio"));
        assert!(
            arguments
                .iter()
                .any(|value| value == "model=\"gpt-5.6-terra\"")
        );
        assert!(
            arguments
                .iter()
                .any(|value| value == "approval_policy=\"on-request\"")
        );
        assert!(
            arguments
                .iter()
                .any(|value| value == "sandbox_mode=\"workspace-write\"")
        );
        assert!(
            arguments
                .iter()
                .any(|value| { value == "projects.\"/tmp/work space\".trust_level=\"trusted\"" })
        );
        assert!(!arguments.iter().any(|value| value == "print"));
    }

    #[test]
    fn pending_notifications_have_an_encoded_byte_budget() {
        assert_eq!(
            next_notification_budget(0, 0, MAX_PENDING_NOTIFICATION_BYTES),
            Ok(MAX_PENDING_NOTIFICATION_BYTES)
        );
        assert!(next_notification_budget(0, MAX_PENDING_NOTIFICATION_BYTES, 1).is_err());
        assert!(next_notification_budget(MAX_PENDING_NOTIFICATIONS, 0, 1).is_err());
    }
}
