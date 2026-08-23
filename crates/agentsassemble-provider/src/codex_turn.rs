use std::{collections::HashSet, time::Duration};

use agentsassemble_domain::{DurableAgentSession, clean_message, has_visible_text};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::time::Instant;

use super::{CodexDriver, protocol_closed, protocol_error};
use crate::runtime::{DriverError, ProviderTurnCompleted, ProviderTurnRequest};

const TURN_INACTIVITY_TIMEOUT: Duration = Duration::from_mins(3);
const INFERRED_COMPLETION_GRACE: Duration = Duration::from_secs(1);
const MAX_PROVIDER_TURN_ID_BYTES: usize = 128;
const MAX_PROVIDER_TURN_IDS: usize = 4_096;
const MAX_FINAL_MESSAGE_CHARS: usize = 12_000;

pub(super) struct QueuedNotification {
    message: Value,
    encoded_bytes: usize,
}

#[derive(Default)]
pub(super) struct CodexTurnState {
    active: Option<ActiveTurn>,
    completed: Option<CompletedTurn>,
    provider_turn_ids: HashSet<String>,
    error: Option<DriverError>,
}

struct ActiveTurn {
    request: ProviderTurnRequest,
    provider_turn_id: String,
    delta_content: String,
    delta_chars: usize,
    final_content: Option<String>,
    last_progress: Instant,
    inferred_completion_at: Option<Instant>,
}

struct CompletedTurn {
    request: ProviderTurnRequest,
    outcome: ProviderTurnCompleted,
}

pub(super) async fn send_turn(
    driver: &mut CodexDriver,
    session: &DurableAgentSession,
    request: &ProviderTurnRequest,
) -> Result<ProviderTurnCompleted, DriverError> {
    if let Some(error) = driver.turn_state.error {
        return Err(error);
    }
    let thread_id = validate_attached_thread(driver, session)?.to_owned();
    if let Some(completed) = &driver.turn_state.completed {
        if completed.request == *request {
            return Ok(completed.outcome.clone());
        }
        if completed.request.turn_id == request.turn_id {
            return Err(turn_conflict());
        }
    }
    if let Some(active) = &driver.turn_state.active {
        if active.request != *request {
            return Err(if active.request.turn_id == request.turn_id {
                turn_conflict()
            } else {
                turn_in_progress()
            });
        }
    } else {
        start_turn(driver, session, request, &thread_id).await?;
    }
    read_turn(driver, &thread_id, &session.public.model).await
}

async fn start_turn(
    driver: &mut CodexDriver,
    session: &DurableAgentSession,
    request: &ProviderTurnRequest,
    thread_id: &str,
) -> Result<(), DriverError> {
    let params = turn_start_params(session, request, thread_id)?;
    let response = match driver.request("turn/start", params).await {
        Ok(response) => response,
        Err(error) => {
            if driver.pending_request.is_none() {
                driver.turn_state.error = Some(error);
            }
            return Err(error);
        }
    };
    let observed_model = match super::observed_model_id_from_response(&response) {
        Ok(value) => value,
        Err(error) => return poison(driver, error),
    };
    if observed_model.is_some_and(|model| model != session.public.model) {
        return poison(driver, crate::codex_identity::provider_model_mismatch());
    }
    let provider_turn_id = match provider_turn_id_from_response(&response) {
        Ok(Some(value)) => value,
        Ok(None) => return poison(driver, turn_unconfirmed()),
        Err(error) => return poison(driver, error),
    };
    if driver
        .turn_state
        .provider_turn_ids
        .contains(&provider_turn_id)
    {
        return poison(driver, provider_turn_reused());
    }
    if driver.turn_state.provider_turn_ids.len() >= MAX_PROVIDER_TURN_IDS {
        return poison(driver, provider_turn_history_exhausted());
    }
    driver
        .turn_state
        .provider_turn_ids
        .insert(provider_turn_id.clone());
    driver.turn_state.completed = None;
    driver.turn_state.active = Some(ActiveTurn {
        request: request.clone(),
        provider_turn_id,
        delta_content: String::new(),
        delta_chars: 0,
        final_content: None,
        last_progress: Instant::now(),
        inferred_completion_at: None,
    });
    Ok(())
}

async fn read_turn(
    driver: &mut CodexDriver,
    thread_id: &str,
    configured_model: &str,
) -> Result<ProviderTurnCompleted, DriverError> {
    loop {
        let (turn_id, deadline, inference_deadline) = {
            let active = driver
                .turn_state
                .active
                .as_ref()
                .ok_or_else(turn_unconfirmed)?;
            let inactivity = active.last_progress + TURN_INACTIVITY_TIMEOUT;
            let deadline = active
                .inferred_completion_at
                .map_or(inactivity, |inferred| inferred.min(inactivity));
            (
                active.provider_turn_id.clone(),
                deadline,
                active.inferred_completion_at,
            )
        };
        let message = tokio::time::timeout_at(
            deadline,
            next_matching_notification(driver, thread_id, &turn_id),
        )
        .await;
        let message = match message {
            Ok(Ok(message)) => message,
            Ok(Err(error)) => return poison(driver, error),
            Err(_) if inference_deadline.is_some_and(|value| value <= Instant::now()) => {
                return finish_turn(driver);
            }
            Err(_) => return poison(driver, turn_timeout()),
        };
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return poison(driver, protocol_error());
        };
        let observed_model = match super::observed_model_id_from_response(&message) {
            Ok(value) => value,
            Err(error) => return poison(driver, error),
        };
        if observed_model.is_some_and(|model| model != configured_model) {
            return poison(driver, crate::codex_identity::provider_model_mismatch());
        }
        let active = driver
            .turn_state
            .active
            .as_mut()
            .ok_or_else(turn_unconfirmed)?;
        active.last_progress = Instant::now();
        match method {
            "agent_message/delta"
            | "agent-message/delta"
            | "item/agent_message/delta"
            | "item/agentMessage/delta" => {
                if let Some(delta) = message_text_delta(&message) {
                    append_bounded_delta(active, delta);
                }
            }
            "agent_message/completed"
            | "agent-message/completed"
            | "item/agent_message/completed" => record_final(active, &message),
            "item/completed" if completed_agent_message(&message) => {
                record_final(active, &message);
            }
            "thread/status/changed" if active.final_content.is_some() && thread_idle(&message) => {
                active.inferred_completion_at = Some(Instant::now() + INFERRED_COMPLETION_GRACE);
            }
            "turn/completed" => return finish_turn(driver),
            "turn/error" | "error" => return poison(driver, turn_failed()),
            "command_execution/request_approval"
            | "file_change/request_approval"
            | "permissions/request_approval" => return poison(driver, approval_required()),
            _ => {}
        }
    }
}

async fn next_matching_notification(
    driver: &mut CodexDriver,
    thread_id: &str,
    turn_id: &str,
) -> Result<Value, DriverError> {
    loop {
        if let Some(index) = matching_pending_index(driver, thread_id, turn_id)? {
            let Some(queued) = driver.pending_notifications.remove(index) else {
                return Err(protocol_error());
            };
            driver.pending_notification_bytes = driver
                .pending_notification_bytes
                .checked_sub(queued.encoded_bytes)
                .ok_or_else(protocol_error)?;
            return Ok(queued.message);
        }
        let line = driver
            .stdout
            .next()
            .await
            .ok_or_else(protocol_closed)?
            .map_err(|_| protocol_error())?;
        let message = serde_json::from_str::<Value>(&line).map_err(|_| protocol_error())?;
        let object = message.as_object().ok_or_else(protocol_error)?;
        if object.get("method").is_none() {
            return Err(DriverError::new(
                "provider_protocol_mismatch",
                "The Codex app-server returned an unexpected response during a turn.",
            ));
        }
        if object.get("id").is_some() {
            driver.reject_server_request(&message).await?;
            continue;
        }
        if message_matches(&message, thread_id, turn_id)? {
            return Ok(message);
        }
        driver.queue_notification(message, line.len())?;
    }
}

fn matching_pending_index(
    driver: &CodexDriver,
    thread_id: &str,
    turn_id: &str,
) -> Result<Option<usize>, DriverError> {
    for (index, queued) in driver.pending_notifications.iter().enumerate() {
        if message_matches(&queued.message, thread_id, turn_id)? {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

fn message_matches(message: &Value, thread_id: &str, turn_id: &str) -> Result<bool, DriverError> {
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(protocol_error)?;
    let message_thread = exact_optional_id(
        [
            nested(message, &["params", "threadId"]),
            nested(message, &["params", "thread", "id"]),
            nested(message, &["params", "thread_id"]),
            nested(message, &["params", "item", "threadId"]),
        ],
        protocol_error,
    )?;
    let message_turn = exact_optional_id(
        [
            nested(message, &["params", "turnId"]),
            nested(message, &["params", "turn", "id"]),
            nested(message, &["params", "turn_id"]),
            nested(message, &["params", "item", "turnId"]),
        ],
        protocol_error,
    )?;
    if turn_scoped_method(method) && (message_thread.is_none() || message_turn.is_none()) {
        return Err(protocol_error());
    }
    if thread_scoped_method(method) && message_thread.is_none() {
        return Err(protocol_error());
    }
    Ok(message_thread
        .as_deref()
        .is_none_or(|value| value == thread_id)
        && message_turn.as_deref().is_none_or(|value| value == turn_id))
}

fn turn_scoped_method(method: &str) -> bool {
    method.starts_with("turn/")
        || method.starts_with("item/")
        || method.starts_with("agent_message/")
        || method.starts_with("agent-message/")
        || matches!(
            method,
            "model/rerouted"
                | "command_execution/request_approval"
                | "file_change/request_approval"
                | "permissions/request_approval"
        )
}

fn thread_scoped_method(method: &str) -> bool {
    method == "thread/status/changed" || method.starts_with("hook/")
}

impl CodexDriver {
    pub(super) fn queue_notification(
        &mut self,
        message: Value,
        encoded_bytes: usize,
    ) -> Result<(), DriverError> {
        self.pending_notification_bytes = super::next_notification_budget(
            self.pending_notifications.len(),
            self.pending_notification_bytes,
            encoded_bytes,
        )?;
        self.pending_notifications.push_back(QueuedNotification {
            message,
            encoded_bytes,
        });
        Ok(())
    }
}

fn turn_start_params(
    session: &DurableAgentSession,
    request: &ProviderTurnRequest,
    thread_id: &str,
) -> Result<Value, DriverError> {
    let (approval_policy, sandbox) = super::profile_permissions(session)?;
    let sandbox_policy = match sandbox {
        "read-only" => json!({"type": "readOnly", "networkAccess": false}),
        "workspace-write" => {
            json!({"type": "workspaceWrite", "networkAccess": false, "writableRoots": []})
        }
        _ => return Err(protocol_error()),
    };
    let mut params = json!({
        "threadId": thread_id,
        "input": [{"type": "text", "text": request.input}],
        "metadata": {"source": "agentsassemble_agent_session"},
        "cwd": session.workspace,
        "model": session.public.model,
        "approvalPolicy": approval_policy,
        "sandboxPolicy": sandbox_policy,
    });
    if !session.public.reasoning_effort.is_empty() {
        params["effort"] = Value::String(session.public.reasoning_effort.clone());
    }
    Ok(params)
}

fn provider_turn_id_from_response(response: &Value) -> Result<Option<String>, DriverError> {
    exact_optional_id(
        [
            nested(response, &["result", "turn", "id"]),
            nested(response, &["result", "turnId"]),
            nested(response, &["params", "turn", "id"]),
            nested(response, &["params", "turnId"]),
        ],
        turn_mismatch,
    )
}

fn exact_optional_id(
    candidates: [Option<&Value>; 4],
    invalid: fn() -> DriverError,
) -> Result<Option<String>, DriverError> {
    let mut observed: Option<&str> = None;
    for candidate in candidates.into_iter().flatten() {
        if candidate.is_null() {
            continue;
        }
        let candidate = candidate.as_str().ok_or_else(invalid)?;
        if candidate.is_empty()
            || candidate.len() > MAX_PROVIDER_TURN_ID_BYTES
            || candidate.trim() != candidate
            || candidate.chars().any(char::is_control)
        {
            return Err(invalid());
        }
        if observed.is_some_and(|value| value != candidate) {
            return Err(invalid());
        }
        observed = Some(candidate);
    }
    Ok(observed.map(str::to_owned))
}

fn nested<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |current, key| current.get(key))
}

fn message_text_delta(message: &Value) -> Option<&str> {
    nested(message, &["params", "delta"])
        .or_else(|| nested(message, &["params", "text"]))
        .and_then(Value::as_str)
}

fn record_final(active: &mut ActiveTurn, message: &Value) {
    let content = nested(message, &["params", "text"])
        .or_else(|| nested(message, &["params", "content"]))
        .or_else(|| nested(message, &["params", "item", "text"]))
        .and_then(Value::as_str)
        .map(|value| clean_message(value, MAX_FINAL_MESSAGE_CHARS))
        .filter(|value| has_visible_text(value))
        .unwrap_or_else(|| clean_message(&active.delta_content, MAX_FINAL_MESSAGE_CHARS));
    active.final_content = Some(content);
}

fn append_bounded_delta(active: &mut ActiveTurn, delta: &str) {
    let remaining = MAX_FINAL_MESSAGE_CHARS.saturating_sub(active.delta_chars);
    let normalized = delta
        .replace('\0', "")
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let bounded = normalized.chars().take(remaining).collect::<String>();
    active.delta_chars += bounded.chars().count();
    active.delta_content.push_str(&bounded);
}

fn completed_agent_message(message: &Value) -> bool {
    nested(message, &["params", "item", "type"]).and_then(Value::as_str) == Some("agentMessage")
}

fn thread_idle(message: &Value) -> bool {
    let status = nested(message, &["params", "thread", "status"])
        .or_else(|| nested(message, &["params", "status"]));
    status.is_some_and(|value| {
        value.as_str() == Some("idle") || value.get("type").and_then(Value::as_str) == Some("idle")
    })
}

fn finish_turn(driver: &mut CodexDriver) -> Result<ProviderTurnCompleted, DriverError> {
    let Some(active) = driver.turn_state.active.take() else {
        return poison(driver, turn_unconfirmed());
    };
    let content = active
        .final_content
        .unwrap_or_else(|| clean_message(&active.delta_content, MAX_FINAL_MESSAGE_CHARS));
    if !has_visible_text(&content) {
        return poison(driver, output_missing());
    }
    let outcome = ProviderTurnCompleted {
        turn_id: active.request.turn_id.clone(),
        provider_turn_id: active.provider_turn_id,
        content,
    };
    driver.turn_state.completed = Some(CompletedTurn {
        request: active.request,
        outcome: outcome.clone(),
    });
    Ok(outcome)
}

fn validate_attached_thread<'a>(
    driver: &'a CodexDriver,
    session: &DurableAgentSession,
) -> Result<&'a str, DriverError> {
    let Some(attached) = driver.attached_thread_id.as_deref() else {
        return Err(super::provider_session_unconfirmed());
    };
    if attached != session.provider_session_id {
        return Err(super::provider_session_mismatch());
    }
    Ok(attached)
}

fn poison<T>(driver: &mut CodexDriver, error: DriverError) -> Result<T, DriverError> {
    driver.turn_state.error = Some(error);
    Err(error)
}

const fn turn_unconfirmed() -> DriverError {
    DriverError::new(
        "provider_turn_unconfirmed",
        "The Codex app-server did not return a provider turn identity.",
    )
}

const fn turn_mismatch() -> DriverError {
    DriverError::new(
        "provider_turn_mismatch",
        "The Codex app-server returned conflicting provider turn identities.",
    )
}

const fn provider_turn_reused() -> DriverError {
    DriverError::new(
        "provider_turn_reused",
        "The Codex app-server reused a provider turn identity in one process.",
    )
}

const fn provider_turn_history_exhausted() -> DriverError {
    DriverError::new(
        "provider_turn_history_exhausted",
        "The bounded Codex provider turn identity history is exhausted.",
    )
}

const fn turn_conflict() -> DriverError {
    DriverError::new(
        "provider_turn_conflict",
        "The durable provider turn identity was reused with different input.",
    )
}

const fn turn_in_progress() -> DriverError {
    DriverError::new(
        "provider_turn_in_progress",
        "A different Codex provider turn may still be in progress.",
    )
}

const fn turn_timeout() -> DriverError {
    DriverError::new(
        "provider_turn_timeout",
        "The Codex provider turn exceeded its inactivity deadline.",
    )
}

const fn turn_failed() -> DriverError {
    DriverError::new(
        "provider_turn_failed",
        "The Codex app-server reported a failed provider turn.",
    )
}

const fn approval_required() -> DriverError {
    DriverError::new(
        "provider_approval_required",
        "The Codex provider turn requested an unsupported approval.",
    )
}

const fn output_missing() -> DriverError {
    DriverError::new(
        "provider_turn_output_missing",
        "The Codex provider turn completed without a final message.",
    )
}
