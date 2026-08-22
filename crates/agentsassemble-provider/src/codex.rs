use std::{collections::VecDeque, io, process::Stdio, time::Duration};

use agentsassemble_domain::DurableAgentSession;
use futures_util::StreamExt;
#[cfg(windows)]
use process_wrap::tokio::JobObject;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::ChildStdin,
    task::JoinHandle,
};
use tokio_util::codec::{FramedRead, LinesCodec};

#[cfg(not(any(target_os = "linux", target_os = "android")))]
use crate::filesystem::BoundExecutable;
use crate::{
    filesystem::bind_executable,
    process::sanitize_environment,
    runtime::{DriverError, DriverFuture, ProviderDriver},
};
#[cfg(unix)]
use crate::{guardian::GuardianLaunch, unix_custody::UnixProcessCustody};

const PROTOCOL_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(windows)]
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PROTOCOL_LINE_BYTES: usize = 256 * 1024;
const MAX_PENDING_NOTIFICATIONS: usize = 256;
const MAX_PENDING_NOTIFICATION_BYTES: usize = 2 * 1024 * 1024;

pub(crate) struct CodexDriver {
    child: Box<dyn ChildWrapper>,
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    _executable_guard: BoundExecutable,
    #[cfg(unix)]
    process_group: UnixProcessCustody,
    stdin: ChildStdin,
    stdout: FramedRead<tokio::process::ChildStdout, LinesCodec>,
    stderr_task: JoinHandle<()>,
    next_request_id: u64,
    pending_notifications: VecDeque<Value>,
    pending_notification_bytes: usize,
    initialized: bool,
}

impl CodexDriver {
    #[cfg(unix)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        guardian_launch: &GuardianLaunch,
    ) -> Result<Self, DriverError> {
        Self::spawn_inner(session, guardian_launch).await
    }

    #[cfg(not(unix))]
    pub(crate) async fn spawn(session: &DurableAgentSession) -> Result<Self, DriverError> {
        Self::spawn_inner(session).await
    }

    async fn spawn_inner(
        session: &DurableAgentSession,
        #[cfg(unix)] guardian_launch: &GuardianLaunch,
    ) -> Result<Self, DriverError> {
        #[cfg(not(any(unix, windows)))]
        return Err(DriverError::new(
            "provider_runtime_unsupported",
            "Provider processes are unsupported on this platform.",
        ));

        let arguments = command_arguments(session)?;
        let executable = bind_executable(
            session.executable.clone(),
            session.executable_identity.clone(),
        )
        .await
        .map_err(|_| executable_authority_error())?;
        #[cfg(unix)]
        let mut process_group = UnixProcessCustody::start(
            &session.public.room_id,
            &session.public.session_id,
            guardian_launch,
        )
        .await?;
        let mut command = CommandWrap::with_new(executable.launch_path(), |command| {
            command
                .args(&arguments)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        });
        #[cfg(any(target_os = "linux", target_os = "android"))]
        executable
            .configure_command(command.command_mut())
            .map_err(|_| executable_authority_error())?;
        #[cfg(unix)]
        process_group.attach(command.command_mut());
        sanitize_environment(command.command_mut());
        command.wrap(KillOnDrop);
        #[cfg(windows)]
        command.wrap(JobObject);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                #[cfg(unix)]
                process_group.request_stop();
                return Err(spawn_error(&error));
            }
        };
        #[cfg(unix)]
        match process_group.bind_provider(child.id()) {
            Ok(()) => {}
            Err(error) => {
                let _ = process_group.stop(child.as_mut()).await;
                return Err(error);
            }
        }
        let Some(stdin) = child.stdin().take() else {
            terminate_failed_spawn(
                child.as_mut(),
                #[cfg(unix)]
                &mut process_group,
            )
            .await;
            return Err(protocol_error());
        };
        let Some(stdout) = child.stdout().take() else {
            terminate_failed_spawn(
                child.as_mut(),
                #[cfg(unix)]
                &mut process_group,
            )
            .await;
            return Err(protocol_error());
        };
        let Some(stderr) = child.stderr().take() else {
            terminate_failed_spawn(
                child.as_mut(),
                #[cfg(unix)]
                &mut process_group,
            )
            .await;
            return Err(protocol_error());
        };
        let stderr_task = tokio::spawn(drain_stderr(stderr));
        Ok(Self {
            child,
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            _executable_guard: executable,
            #[cfg(unix)]
            process_group,
            stdin,
            stdout: FramedRead::new(
                stdout,
                LinesCodec::new_with_max_length(MAX_PROTOCOL_LINE_BYTES),
            ),
            stderr_task,
            next_request_id: 1,
            pending_notifications: VecDeque::new(),
            pending_notification_bytes: 0,
            initialized: false,
        })
    }

    async fn initialize(&mut self) -> Result<(), DriverError> {
        if self.initialized {
            return Ok(());
        }
        self.request(
            "initialize",
            json!({"clientInfo": {"name": "AgentsAssemble", "version": "0"}}),
        )
        .await?;
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {},
        }))
        .await?;
        self.initialized = true;
        Ok(())
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, DriverError> {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        }))
        .await?;
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
            if object.get("error").is_some_and(|value| !value.is_null()) {
                return Err(DriverError::new(
                    "provider_request_rejected",
                    "The Codex app-server rejected its initialization request.",
                ));
            }
            return object.get("result").cloned().ok_or_else(protocol_error);
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
        let stopped = self.process_group.stop(self.child.as_mut()).await;
        #[cfg(windows)]
        let stopped = tokio::time::timeout(STOP_TIMEOUT, Box::into_pin(self.child.kill())).await;
        #[cfg(windows)]
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
    fn ensure_ready(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(self.initialize())
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
    let (approval, sandbox) = match session.public.permission_mode.as_str() {
        "workspace_write" => ("on-request", "workspace-write"),
        "meeting_read_only" => ("never", "read-only"),
        _ => {
            return Err(DriverError::new(
                "invalid_runtime_profile",
                "The Codex permission mode is unsupported.",
            ));
        }
    };
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

fn push_config(arguments: &mut Vec<String>, key: &str, value: &str) -> Result<(), DriverError> {
    arguments.push("-c".to_owned());
    arguments.push(format!("{key}={}", json_string(value)?));
    Ok(())
}

fn json_string(value: &str) -> Result<String, DriverError> {
    serde_json::to_string(value).map_err(|_| protocol_error())
}

async fn drain_stderr(mut stderr: tokio::process::ChildStderr) {
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

async fn terminate_failed_spawn(
    child: &mut dyn ChildWrapper,
    #[cfg(unix)] process_group: &mut UnixProcessCustody,
) {
    #[cfg(unix)]
    {
        let _ = process_group.stop(child).await;
    }
    #[cfg(not(unix))]
    let _ = tokio::time::timeout(STOP_TIMEOUT, Box::into_pin(child.kill())).await;
}

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
