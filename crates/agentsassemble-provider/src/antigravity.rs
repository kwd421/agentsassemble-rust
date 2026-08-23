use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use agentsassemble_domain::DurableAgentSession;
use tokio::time::Instant;
use uuid::Uuid;

use crate::{
    antigravity_hook::AntigravityHookRegistration,
    antigravity_terminal::AntigravityRoomPermissionPolicy,
    antigravity_transcript::AntigravityTranscript,
    antigravity_transport::AntigravityTerminal,
    filesystem::{BoundExecutable, bind_executable},
    launch_error::DriverLaunchError,
    room_portal::{ProviderTurnOutcome, RoomPortal, RoomPortalError},
    room_portal_terminal::RoomPortalTerminalHelper,
    runtime::{
        DriverError, DriverFuture, ProviderDriver, ProviderSessionAttachment,
        ProviderTurnCompleted, ProviderTurnRequest,
    },
};
#[cfg(unix)]
use crate::{guardian::GuardianLaunch, runtime_lease::HeldRuntimeLease};

const MAX_PROVIDER_SESSION_ID_BYTES: usize = 200;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
const STARTUP_QUIET: Duration = Duration::from_secs(5);
const TURN_INACTIVITY_TIMEOUT: Duration = Duration::from_mins(3);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const SUBMIT_DELAY: Duration = Duration::from_millis(100);
const MAX_TERMINAL_TAIL_BYTES: usize = 64 * 1024;
const PENDING_SESSION_PREFIX: &str = "pending-antigravity-";

pub(super) fn command_arguments(session: &DurableAgentSession) -> Result<Vec<String>, DriverError> {
    let model = session.public.model.trim();
    let effort = session.public.reasoning_effort.trim().to_ascii_lowercase();
    if model.is_empty()
        || model != session.public.model
        || model.chars().any(char::is_control)
        || !matches!(effort.as_str(), "" | "low" | "medium" | "high")
    {
        return Err(profile_error());
    }
    let effective_model =
        if effort.is_empty() || model.to_ascii_lowercase().ends_with(&format!("-{effort}")) {
            model.to_owned()
        } else {
            format!("{model}-{effort}")
        };
    let mut arguments = vec!["--model".to_owned(), effective_model];
    match session.public.permission_mode.as_str() {
        "workspace_write" => arguments.extend(["--mode".to_owned(), "accept-edits".to_owned()]),
        "meeting_read_only" => arguments.push("--sandbox".to_owned()),
        _ => return Err(profile_error()),
    }
    let provider_session_id = clean_identifier(&session.provider_session_id);
    if !session.provider_session_id.is_empty() && provider_session_id.is_empty() {
        return Err(profile_error());
    }
    if !provider_session_id.is_empty() && !provider_session_id.starts_with(PENDING_SESSION_PREFIX) {
        arguments.extend(["--conversation".to_owned(), provider_session_id]);
    }
    Ok(arguments)
}

struct ActiveTurn {
    request: ProviderTurnRequest,
    provider_turn_id: String,
    last_progress: Instant,
}

struct PreparedAntigravity {
    arguments: Vec<String>,
    workspace: PathBuf,
    transcript: AntigravityTranscript,
    executable: BoundExecutable,
    room_portal: RoomPortal,
}

async fn prepare(session: &DurableAgentSession) -> Result<PreparedAntigravity, DriverLaunchError> {
    let arguments = command_arguments(session)?;
    let workspace = PathBuf::from(&session.workspace);
    let home = env_home()?;
    let mut transcript = AntigravityTranscript::new(home, workspace.clone());
    let resume_id = clean_identifier(&session.provider_session_id);
    let resume_id = (!resume_id.is_empty() && !resume_id.starts_with(PENDING_SESSION_PREFIX))
        .then_some(resume_id.as_str());
    transcript.prepare_start(resume_id)?;
    let executable = bind_executable(
        session.executable.clone(),
        session.executable_identity.clone(),
    )
    .await
    .map_err(|_| executable_error())?;
    let room_portal = RoomPortal::create().await.map_err(portal_driver_error)?;
    Ok(PreparedAntigravity {
        arguments,
        workspace,
        transcript,
        executable,
        room_portal,
    })
}

pub(crate) struct AntigravityDriver {
    terminal: Box<dyn AntigravityTerminal>,
    transcript: AntigravityTranscript,
    room_portal: RoomPortal,
    terminal_helper: Option<RoomPortalTerminalHelper>,
    hook: Option<AntigravityHookRegistration>,
    attached_session_id: Option<String>,
    attached_reused: bool,
    startup_drained: bool,
    terminal_query_tail: Vec<u8>,
    permission_policy: AntigravityRoomPermissionPolicy,
    transcript_nonce: Uuid,
    active_turn: Option<ActiveTurn>,
    completed_turn: Option<(ProviderTurnRequest, ProviderTurnCompleted)>,
    terminal_tail: Vec<u8>,
    poisoned: bool,
}

impl AntigravityDriver {
    #[cfg(unix)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        runtime_lease: &HeldRuntimeLease,
        guardian: &GuardianLaunch,
    ) -> Result<Self, DriverLaunchError> {
        let PreparedAntigravity {
            arguments,
            workspace,
            transcript,
            executable,
            room_portal,
        } = prepare(session).await?;
        let terminal_helper = room_portal
            .create_terminal_helper(guardian)
            .map_err(portal_driver_error)?;
        let hook = AntigravityHookRegistration::register(&workspace)?;
        let mut environment = terminal_helper.provider_environment();
        environment.extend([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ("COLUMNS".to_owned(), "120".to_owned()),
            ("LINES".to_owned(), "40".to_owned()),
        ]);
        let terminal = crate::antigravity_unix::spawn_terminal(
            runtime_lease,
            guardian,
            executable,
            &arguments,
            &environment,
            &workspace,
        )
        .await?;
        Ok(Self::from_parts(
            terminal,
            transcript,
            room_portal,
            terminal_helper,
            hook,
        ))
    }

    #[cfg(windows)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        companion: &BoundExecutable,
    ) -> Result<Self, DriverLaunchError> {
        let PreparedAntigravity {
            arguments,
            workspace,
            transcript,
            executable,
            room_portal,
        } = prepare(session).await?;
        let terminal_helper = room_portal
            .create_terminal_helper(companion)
            .map_err(portal_driver_error)?;
        let hook = AntigravityHookRegistration::register(&workspace)?;
        let mut environment = terminal_helper.provider_environment();
        environment.extend([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ("COLUMNS".to_owned(), "120".to_owned()),
            ("LINES".to_owned(), "40".to_owned()),
        ]);
        let terminal = crate::antigravity_windows::spawn_terminal(
            executable,
            &arguments,
            &environment,
            &workspace,
        )?;
        Ok(Self::from_parts(
            terminal,
            transcript,
            room_portal,
            terminal_helper,
            hook,
        ))
    }

    fn from_parts(
        terminal: Box<dyn AntigravityTerminal>,
        transcript: AntigravityTranscript,
        room_portal: RoomPortal,
        terminal_helper: RoomPortalTerminalHelper,
        hook: AntigravityHookRegistration,
    ) -> Self {
        Self {
            terminal,
            transcript,
            room_portal,
            terminal_helper: Some(terminal_helper),
            hook: Some(hook),
            attached_session_id: None,
            attached_reused: false,
            startup_drained: false,
            terminal_query_tail: Vec::new(),
            permission_policy: AntigravityRoomPermissionPolicy::default(),
            transcript_nonce: Uuid::new_v4(),
            active_turn: None,
            completed_turn: None,
            terminal_tail: Vec::new(),
            poisoned: false,
        }
    }

    async fn attach(
        &mut self,
        session: &DurableAgentSession,
    ) -> Result<ProviderSessionAttachment, DriverError> {
        self.drain_startup().await?;
        let durable = clean_identifier(&session.provider_session_id);
        if !session.provider_session_id.is_empty() && durable.is_empty() {
            return self.poison(profile_error());
        }
        if let Some(attached) = &self.attached_session_id {
            if !durable.is_empty() && durable != *attached {
                return self.poison(session_mismatch());
            }
            return Ok(ProviderSessionAttachment {
                provider_session_id: attached.clone(),
                reused: self.attached_reused,
                observed_model_id: Some(session.public.model.clone()),
            });
        }
        let attached = if durable.is_empty() {
            format!("{PENDING_SESSION_PREFIX}{}", session.public.session_id)
        } else {
            durable.clone()
        };
        self.attached_reused = !durable.is_empty() && !durable.starts_with(PENDING_SESSION_PREFIX);
        self.attached_session_id = Some(attached.clone());
        Ok(ProviderSessionAttachment {
            provider_session_id: attached,
            reused: self.attached_reused,
            observed_model_id: Some(session.public.model.clone()),
        })
    }

    async fn drain_startup(&mut self) -> Result<(), DriverError> {
        if self.startup_drained {
            return Ok(());
        }
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let mut last_read = None;
        let mut trust_accepted = false;
        loop {
            if Instant::now() >= deadline {
                return self.poison(startup_error());
            }
            if !self.terminal.is_alive()? {
                return self.poison(runtime_exited());
            }
            if let Ok(chunk) = tokio::time::timeout(POLL_INTERVAL, self.read_terminal()).await {
                let chunk = chunk?;
                if !chunk.is_empty() {
                    self.answer_terminal_queries(&chunk).await?;
                    self.record_terminal(&chunk);
                    if !trust_accepted && contains_bytes(&self.terminal_tail, b"Do you trust") {
                        self.write_terminal(b"\r").await?;
                        trust_accepted = true;
                    }
                    last_read = Some(Instant::now());
                }
            }
            if last_read.is_some_and(|last| Instant::now().duration_since(last) >= STARTUP_QUIET) {
                self.startup_drained = true;
                self.terminal_tail.clear();
                return Ok(());
            }
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
        let attached = self
            .attached_session_id
            .as_deref()
            .ok_or_else(session_missing)?;
        if let Some((completed_request, completed)) = &self.completed_turn
            && completed_request == request
        {
            let native = completed.provider_session_id.as_deref().unwrap_or(attached);
            if session.provider_session_id != attached
                && !(session
                    .provider_session_id
                    .starts_with(PENDING_SESSION_PREFIX)
                    && native == attached)
            {
                return self.poison(session_mismatch());
            }
            return Ok(completed.clone());
        }
        if session.provider_session_id != attached {
            return self.poison(session_mismatch());
        }
        if let Some(active) = &self.active_turn {
            if active.request != *request {
                return Err(turn_conflict());
            }
        } else {
            self.drain_terminal_available().await?;
            self.permission_policy.begin_turn();
            let prompt = terminal_prompt(request, self.transcript_nonce);
            self.transcript.begin_turn(&prompt)?;
            self.write_terminal(format!("\x1b[200~{prompt}\x1b[201~").as_bytes())
                .await?;
            tokio::time::sleep(SUBMIT_DELAY).await;
            self.write_terminal(b"\r").await?;
            self.active_turn = Some(ActiveTurn {
                request: request.clone(),
                provider_turn_id: Uuid::new_v4().to_string(),
                last_progress: Instant::now(),
            });
        }
        self.read_turn(session).await
    }

    async fn read_turn(
        &mut self,
        session: &DurableAgentSession,
    ) -> Result<ProviderTurnCompleted, DriverError> {
        loop {
            let deadline = self
                .active_turn
                .as_ref()
                .ok_or_else(turn_missing)?
                .last_progress
                + TURN_INACTIVITY_TIMEOUT;
            if Instant::now() >= deadline {
                let _ = self.write_terminal(b"\x03").await;
                return self.poison(turn_timeout());
            }
            if let Ok(chunk) = tokio::time::timeout(POLL_INTERVAL, self.read_terminal()).await {
                let chunk = chunk?;
                if !chunk.is_empty() {
                    self.record_terminal(&chunk);
                    self.answer_terminal_queries(&chunk).await?;
                    let Ok(permission_response) =
                        self.permission_policy.response_for(&self.terminal_tail)
                    else {
                        return self.poison(unexpected_permission());
                    };
                    if let Some(response) = permission_response {
                        self.write_terminal(response).await?;
                    }
                    if let Some(active) = self.active_turn.as_mut() {
                        active.last_progress = Instant::now();
                    }
                }
            }
            let Some(snapshot) = self.transcript.poll()? else {
                if !self.terminal.is_alive()? {
                    return self.poison(runtime_exited());
                }
                continue;
            };
            if !snapshot.observed_model_id.is_empty()
                && !model_matches(&session.public.model, &snapshot.observed_model_id)
            {
                return self.poison(model_mismatch());
            }
            let active = self.active_turn.take().ok_or_else(turn_missing)?;
            let current = self
                .attached_session_id
                .as_deref()
                .ok_or_else(session_missing)?;
            if !current.starts_with(PENDING_SESSION_PREFIX)
                && current != snapshot.provider_session_id
            {
                return self.poison(session_mismatch());
            }
            self.attached_session_id = Some(snapshot.provider_session_id.clone());
            let completed = ProviderTurnCompleted {
                turn_id: active.request.turn_id.clone(),
                provider_turn_id: active.provider_turn_id,
                provider_session_id: Some(snapshot.provider_session_id),
                outcome: ProviderTurnOutcome::Message {
                    content: snapshot.content,
                    target_agent_id: String::new(),
                },
            };
            self.completed_turn = Some((active.request, completed.clone()));
            self.terminal_tail.clear();
            return Ok(completed);
        }
    }

    async fn drain_terminal_available(&mut self) -> Result<(), DriverError> {
        loop {
            match tokio::time::timeout(Duration::from_millis(5), self.read_terminal()).await {
                Ok(Ok(chunk)) if !chunk.is_empty() => self.record_terminal(&chunk),
                Ok(Ok(_)) | Err(_) => return Ok(()),
                Ok(Err(error)) => return Err(error),
            }
        }
    }

    async fn read_terminal(&mut self) -> Result<Vec<u8>, DriverError> {
        self.terminal.read().await
    }

    async fn write_terminal(&mut self, data: &[u8]) -> Result<(), DriverError> {
        self.terminal.write(data).await
    }

    async fn answer_terminal_queries(&mut self, chunk: &[u8]) -> Result<(), DriverError> {
        const QUERIES: [(&str, &[u8], &[u8]); 7] = [
            ("cursor", b"\x1b[6n", b"\x1b[1;1R"),
            ("device", b"\x1b[c", b"\x1b[?1;2c"),
            ("keyboard", b"\x1b[?u", b"\x1b[?0u"),
            ("synchronized-output", b"\x1b[?2026$p", b"\x1b[?2026;2$y"),
            ("unicode-core", b"\x1b[?2027$p", b"\x1b[?2027;2$y"),
            (
                "foreground",
                b"\x1b]10;?\x1b\\",
                b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\",
            ),
            (
                "background",
                b"\x1b]11;?\x1b\\",
                b"\x1b]11;rgb:0000/0000/0000\x1b\\",
            ),
        ];
        let prefix_len = self.terminal_query_tail.len();
        self.terminal_query_tail.extend_from_slice(chunk);
        let mut responses = Vec::new();
        for (_name, query, response) in QUERIES {
            for (offset, window) in self.terminal_query_tail.windows(query.len()).enumerate() {
                if window == query && offset + query.len() > prefix_len {
                    responses.push(response);
                }
            }
        }
        let retained = QUERIES
            .iter()
            .map(|(_, query, _)| query.len())
            .max()
            .unwrap_or(1)
            .saturating_sub(1);
        if self.terminal_query_tail.len() > retained {
            let excess = self.terminal_query_tail.len() - retained;
            self.terminal_query_tail.drain(..excess);
        }
        for response in responses {
            self.write_terminal(response).await?;
        }
        Ok(())
    }

    fn record_terminal(&mut self, chunk: &[u8]) {
        self.terminal_tail.extend_from_slice(chunk);
        if self.terminal_tail.len() > MAX_TERMINAL_TAIL_BYTES {
            let excess = self.terminal_tail.len() - MAX_TERMINAL_TAIL_BYTES;
            self.terminal_tail.drain(..excess);
        }
    }

    fn poison<T>(&mut self, error: DriverError) -> Result<T, DriverError> {
        self.poisoned = true;
        Err(error)
    }

    async fn stop_process(&mut self) -> Result<(), DriverError> {
        self.terminal.stop().await?;
        self.hook.take();
        self.terminal_helper.take();
        Ok(())
    }
}

impl ProviderDriver for AntigravityDriver {
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

    fn is_alive(&mut self) -> Result<bool, DriverError> {
        self.terminal.is_alive()
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(self.stop_process())
    }

    fn begin_room_observation(&mut self, request: &ProviderTurnRequest) -> Result<(), DriverError> {
        let observation = request
            .room_observation
            .as_ref()
            .ok_or_else(portal_missing)?;
        self.room_portal
            .begin_observation(
                &request.turn_id,
                observation.input_up_to_seq,
                &observation.view,
                &observation.allowed_agent_ids,
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
            .ok_or_else(portal_missing)?;
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

impl Drop for AntigravityDriver {
    fn drop(&mut self) {
        self.terminal.request_stop();
    }
}

fn terminal_prompt(request: &ProviderTurnRequest, transcript_nonce: Uuid) -> String {
    format!(
        "{}\n\n<agentsassemble-transport turn=\"{}\" launch=\"{transcript_nonce}\">Antigravity room transport: first run `agentsassemble-room help`, then run `agentsassemble-room read`. Finish with exactly one `agentsassemble-room speak 'message'`, `agentsassemble-room speak-to agent-id 'message'`, or `agentsassemble-room decline reason`. Run one helper command per terminal tool call. Ordinary assistant final text is not a room publication.</agentsassemble-transport>",
        request.input, request.turn_id
    )
}

fn model_matches(configured: &str, observed: &str) -> bool {
    let normalized = |value: &str| {
        value
            .to_ascii_lowercase()
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .split('-')
            .filter(|part| !part.is_empty() && !matches!(*part, "low" | "medium" | "high"))
            .collect::<Vec<_>>()
            .join("-")
    };
    normalized(configured) == normalized(observed)
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn env_home() -> Result<PathBuf, DriverError> {
    #[cfg(unix)]
    const HOME_ENVIRONMENT: &str = "HOME";
    #[cfg(windows)]
    const HOME_ENVIRONMENT: &str = "USERPROFILE";

    std::env::var_os(HOME_ENVIRONMENT)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(profile_error)
}

fn portal_driver_error(_error: RoomPortalError) -> DriverError {
    DriverError::new(
        "room_portal_unavailable",
        "The Antigravity room portal is unavailable.",
    )
}

const fn executable_error() -> DriverError {
    DriverError::new(
        "provider_executable_changed",
        "The selected Antigravity executable authority changed.",
    )
}

const fn startup_error() -> DriverError {
    DriverError::new(
        "provider_startup_timeout",
        "The Antigravity interactive session did not become ready.",
    )
}

const fn runtime_exited() -> DriverError {
    DriverError::new(
        "provider_runtime_exited",
        "The Antigravity interactive session exited unexpectedly.",
    )
}

const fn unexpected_permission() -> DriverError {
    DriverError::new(
        "unexpected_provider_permission_request",
        "Antigravity requested an unapproved terminal command during room observation.",
    )
}

const fn session_missing() -> DriverError {
    DriverError::new(
        "provider_session_unconfirmed",
        "The Antigravity conversation is not attached.",
    )
}

const fn session_mismatch() -> DriverError {
    DriverError::new(
        "provider_session_mismatch",
        "The Antigravity conversation identity changed.",
    )
}

const fn turn_conflict() -> DriverError {
    DriverError::new(
        "provider_turn_conflict",
        "The Antigravity session already owns another turn.",
    )
}

const fn turn_missing() -> DriverError {
    DriverError::new(
        "provider_turn_unconfirmed",
        "The Antigravity turn identity is unavailable.",
    )
}

const fn turn_timeout() -> DriverError {
    DriverError::new(
        "provider_turn_timeout",
        "The Antigravity turn made no progress before its deadline.",
    )
}

const fn protocol_error() -> DriverError {
    DriverError::new(
        "provider_protocol_invalid",
        "The Antigravity transcript authority is invalid.",
    )
}

const fn model_mismatch() -> DriverError {
    DriverError::new(
        "provider_model_mismatch",
        "The Antigravity transcript reported a different model.",
    )
}

const fn portal_missing() -> DriverError {
    DriverError::new(
        "room_portal_unavailable",
        "The Antigravity room observation is unavailable.",
    )
}

pub(super) fn clean_identifier(value: &str) -> String {
    let value = value.trim();
    let mut components = Path::new(value).components();
    if value.is_empty()
        || value.len() > MAX_PROVIDER_SESSION_ID_BYTES
        || value.chars().any(char::is_control)
        || value == "--last"
        || !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        String::new()
    } else {
        value.to_owned()
    }
}

const fn profile_error() -> DriverError {
    DriverError::new(
        "invalid_runtime_profile",
        "The stored Antigravity runtime profile is invalid.",
    )
}

#[cfg(test)]
#[path = "antigravity_tests.rs"]
mod tests;
