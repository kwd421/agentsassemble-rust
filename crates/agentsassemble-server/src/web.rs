use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, ProviderCatalog, public_settings, validate_room_id,
};
use agentsassemble_persistence::{PersistenceError, SqliteStore};
use agentsassemble_protocol::{
    ClientFrame, CommandAck, CommandNack, ProtocolError, RoomSnapshot, ServerFrame, TicketResponse,
};
use axum::{
    Json, Router,
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore},
    time::Instant,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tower_http::services::{ServeDir, ServeFile};

use crate::{RoomRuntime, TicketStore};

const MAX_WS_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_WS_CONNECTIONS: usize = 128;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const SOCKET_IDLE_TIMEOUT: Duration = Duration::from_mins(5);
const INGRESS_WINDOW: Duration = Duration::from_secs(10);
const INGRESS_MESSAGES_PER_WINDOW: usize = 256;
const INGRESS_BYTES_PER_WINDOW: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub store: SqliteStore,
    pub rooms: RoomRuntime,
    pub tickets: TicketStore,
    pub host_token: HostSecret,
    pub shutdown: CancellationToken,
    pub connections: TaskTracker,
    pub connection_admission: Arc<Semaphore>,
    pub frontend_root: Option<PathBuf>,
}

impl AppState {
    #[must_use]
    pub fn local(store: SqliteStore, tickets: TicketStore, host_token: HostSecret) -> Self {
        Self {
            rooms: RoomRuntime::new(store.clone()),
            store,
            tickets,
            host_token,
            shutdown: CancellationToken::new(),
            connections: TaskTracker::new(),
            connection_admission: Arc::new(Semaphore::new(MAX_WS_CONNECTIONS)),
            frontend_root: None,
        }
    }

    #[must_use]
    pub fn with_frontend(mut self, frontend_root: PathBuf) -> Self {
        self.frontend_root = Some(frontend_root);
        self
    }
}

#[derive(Debug, Clone)]
pub struct HostSecret(Arc<str>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("host secret must contain at least 32 non-whitespace bytes")]
pub struct InvalidHostSecret;

impl HostSecret {
    /// Validates a desktop runtime host credential.
    ///
    /// # Errors
    ///
    /// Returns `InvalidHostSecret` for short or whitespace-padded credentials.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidHostSecret> {
        let value = value.into();
        if value.len() < 32 || value.trim() != value {
            return Err(InvalidHostSecret);
        }
        Ok(Self(Arc::from(value)))
    }

    fn matches(&self, provided: &str) -> bool {
        self.0.len() == provided.len() && bool::from(self.0.as_bytes().ct_eq(provided.as_bytes()))
    }
}

#[derive(Debug, Error)]
pub enum ServeError {
    #[error("server I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Deserialize)]
struct TicketRequest {
    meeting_id: String,
}

#[derive(Debug, Deserialize)]
struct TicketQuery {
    ticket: String,
}

pub fn router(state: AppState) -> Router {
    let frontend_root = state.frontend_root.clone();
    let mut app = Router::new()
        .route("/healthz", get(health))
        .route("/api/ws-ticket", post(issue_ticket))
        .route("/ws", get(upgrade_socket));
    if let Some(frontend_root) = frontend_root {
        let index = frontend_root.join("index.html");
        app = app
            .route("/", get(|| async { Redirect::temporary("/app/") }))
            .nest_service(
                "/app",
                ServeDir::new(frontend_root).not_found_service(ServeFile::new(index)),
            );
    }
    app.with_state(state)
}

/// Serves the loopback runtime until its explicit cancellation token fires.
///
/// # Errors
///
/// Returns the listener's serving error.
pub async fn serve(
    listener: TcpListener,
    state: AppState,
    cancellation: CancellationToken,
) -> Result<(), ServeError> {
    let rooms = state.rooms.clone();
    let connections = state.connections.clone();
    let connection_shutdown = state.shutdown.clone();
    let result = axum::serve(listener, router(state))
        .with_graceful_shutdown(cancellation.cancelled_owned())
        .await;
    connection_shutdown.cancel();
    connections.close();
    connections.wait().await;
    rooms.shutdown().await;
    result.map_err(ServeError::Io)
}

async fn health() -> Json<Value> {
    Json(json!({"status": "ready", "runtime": "rust"}))
}

async fn issue_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<TicketRequest>,
) -> Result<Json<TicketResponse>, ApiError> {
    let provided_token = headers
        .get("x-host-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !state.host_token.matches(provided_token) {
        return Err(ApiError::unauthorized("A valid host token is required."));
    }
    let room_id = validate_room_id(&request.meeting_id)
        .map_err(|error| ApiError::bad_request(error.message))?;
    if !state.store.room_exists(&room_id).await? {
        return Err(ApiError::not_found("Room does not exist."));
    }
    let participant = state
        .store
        .participant(&room_id, LOCAL_OPERATOR_PARTICIPANT_ID)
        .await?;
    let client_kind = ClientKind::Browser;
    let invite_scope = InviteScope::ReadWrite;
    let ticket = state
        .tickets
        .issue(AuthenticatedPrincipal {
            principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
            participant_id: participant.participant_id,
            display_name: participant.display_name,
            room_id,
            client_kind,
            invite_scope,
            capabilities: CapabilitySet::local_operator(client_kind, invite_scope),
        })
        .await
        .map_err(|error| ApiError::unavailable(error.to_string()))?;
    Ok(Json(TicketResponse {
        ticket,
        ttl_seconds: state.tickets.ttl_seconds(),
    }))
}

async fn upgrade_socket(
    State(state): State<AppState>,
    Query(query): Query<TicketQuery>,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let permit = state
        .connection_admission
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::unavailable("WebSocket connection limit reached."))?;
    let principal = state
        .tickets
        .consume(&query.ticket)
        .await
        .map_err(|error| ApiError::unauthorized(error.to_string()))?;
    let connections = state.connections.clone();
    Ok(upgrade
        .max_message_size(MAX_WS_MESSAGE_BYTES)
        .max_frame_size(MAX_WS_MESSAGE_BYTES)
        .write_buffer_size(64 * 1024)
        .max_write_buffer_size(384 * 1024)
        .on_upgrade(move |socket| {
            connections.track_future(socket_session(socket, state, principal, permit))
        })
        .into_response())
}

#[allow(clippy::too_many_lines)] // One select loop owns the socket's ordering and lifecycle.
async fn socket_session(
    socket: WebSocket,
    state: AppState,
    principal: AuthenticatedPrincipal,
    _permit: OwnedSemaphorePermit,
) {
    let (mut sender, mut receiver) = socket.split();
    let incoming = tokio::select! {
        () = state.shutdown.cancelled() => return,
        incoming = tokio::time::timeout(HANDSHAKE_TIMEOUT, receiver.next()) => {
            let Ok(incoming) = incoming else {
                let _ = send_nack(&mut sender, "", "subscribe", "subscribe_timeout", "Subscription was not received within 10 seconds.").await;
                return;
            };
            incoming
        },
    };
    let Some(Ok(Message::Text(raw))) = incoming else {
        return;
    };
    let Ok(ClientFrame::Subscribe {
        streams,
        resume_from_seq,
    }) = serde_json::from_str(raw.as_str())
    else {
        let _ = send_nack(
            &mut sender,
            "",
            "subscribe",
            "subscribe_required",
            "The first frame must be a valid subscription.",
        )
        .await;
        return;
    };
    if !streams.iter().any(|stream| stream == "room_events") || resume_from_seq < 0 {
        let _ = send_nack(
            &mut sender,
            "",
            "subscribe",
            "invalid_subscription",
            "room_events and a non-negative cursor are required.",
        )
        .await;
        return;
    }
    let mut events = state.rooms.subscribe(&principal.room_id).await;
    let snapshot_data = match state
        .store
        .snapshot(&principal.room_id, resume_from_seq, 200)
        .await
    {
        Ok(snapshot) => snapshot,
        Err(PersistenceError::InvalidCursor { durable_last_seq }) => {
            let frame = ServerFrame::ResyncRequired {
                stream: "room_events",
                reason: "resume cursor is ahead of durable room state".to_owned(),
                latest_seq: durable_last_seq,
            };
            let _ = send_frame(&mut sender, &frame).await;
            return;
        }
        Err(error) => {
            tracing::error!(error = ?error, room_id = %principal.room_id, "room snapshot failed");
            let _ = send_nack(
                &mut sender,
                "",
                "subscribe",
                "snapshot_failed",
                "Room snapshot failed.",
            )
            .await;
            return;
        }
    };
    let settings = match public_settings(&snapshot_data.settings) {
        Ok(settings) => settings,
        Err(error) => {
            let _ = send_nack(
                &mut sender,
                "",
                "subscribe",
                "snapshot_failed",
                &error.to_string(),
            )
            .await;
            return;
        }
    };
    let snapshot = ServerFrame::Snapshot(Box::new(RoomSnapshot {
        stream: "room_events",
        room: snapshot_data.room,
        room_settings: settings,
        participants: snapshot_data.participants,
        agent_sessions: Vec::new(),
        provider_requests: Vec::new(),
        active_turns: Vec::new(),
        events: snapshot_data.events,
        oldest_seq: snapshot_data.oldest_seq,
        last_seq: snapshot_data.last_seq,
        has_more_before: snapshot_data.has_more_before,
        resume_gap: snapshot_data.resume_gap,
        snapshot_mode: snapshot_data.snapshot_mode,
        provider_catalog: ProviderCatalog::default(),
        available_providers: Vec::new(),
        capabilities: principal.capabilities.clone(),
    }));
    if send_frame(&mut sender, &snapshot).await.is_err() {
        return;
    }
    let mut ingress = IngressBudget::new();
    loop {
        tokio::select! {
            () = state.shutdown.cancelled() => return,
            incoming = tokio::time::timeout(SOCKET_IDLE_TIMEOUT, receiver.next()) => {
                let Ok(Some(Ok(message))) = incoming else { return; };
                let Message::Text(raw) = message else { continue; };
                if !ingress.admit(raw.len()) {
                    let _ = send_nack(&mut sender, "", "frame", "ingress_limited", "WebSocket ingress budget exceeded.").await;
                    return;
                }
                match serde_json::from_str::<ClientFrame>(raw.as_str()) {
                    Ok(ClientFrame::Command { request_id, action, payload }) => {
                        if request_id.is_empty()
                            || request_id.chars().count() > 128
                            || action.is_empty()
                            || action.chars().count() > 64
                        {
                            if send_nack(&mut sender, &request_id, &action, "command_envelope_invalid", "request_id or action is invalid.").await.is_err() { return; }
                            continue;
                        }
                        let outcome = state.rooms.execute(
                            principal.clone(), request_id.clone(), action.clone(), payload,
                        ).await;
                        match outcome {
                            Ok(outcome) => {
                                let frame = ServerFrame::Ack(CommandAck {
                                    request_id,
                                    accepted: true,
                                    action,
                                    result: outcome.result,
                                    deduplicated: outcome.deduplicated,
                                });
                                if send_frame(&mut sender, &frame).await.is_err() { return; }
                            }
                            Err(error) => {
                                if persistence_error_is_internal(&error) {
                                    tracing::error!(error = ?error, room_id = %principal.room_id, action = %action, "room command persistence failed");
                                }
                                let (code, message) = persistence_error(&error);
                                if send_nack(&mut sender, &request_id, &action, code, &message).await.is_err() { return; }
                            }
                        }
                    }
                    Ok(ClientFrame::Ping { nonce }) => {
                        if send_frame(&mut sender, &ServerFrame::Pong { nonce }).await.is_err() { return; }
                    }
                    Ok(ClientFrame::Subscribe { .. }) => {
                        if send_nack(&mut sender, "", "subscribe", "already_subscribed", "This socket is already subscribed.").await.is_err() { return; }
                    }
                    Err(error) => {
                        if send_nack(&mut sender, "", "frame", "frame_invalid", &error.to_string()).await.is_err() { return; }
                    }
                }
            }
            published = events.recv() => {
                match published {
                    Ok(event) => {
                        let latest_seq = event.seq;
                        let frame = ServerFrame::Event {
                            stream: "room_events",
                            events: vec![event],
                            latest_seq,
                        };
                        if send_frame(&mut sender, &frame).await.is_err() { return; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let frame = ServerFrame::ResyncRequired {
                            stream: "room_events",
                            reason: "subscriber fell behind the room event stream".to_owned(),
                            latest_seq: state.store.snapshot(&principal.room_id, 0, 1).await.map_or(0, |snapshot| snapshot.last_seq),
                        };
                        let _ = send_frame(&mut sender, &frame).await;
                        return;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    }
}

async fn send_nack<S>(
    sender: &mut S,
    request_id: &str,
    action: &str,
    code: &str,
    message: &str,
) -> Result<(), axum::Error>
where
    S: futures_util::Sink<Message, Error = axum::Error> + Unpin,
{
    send_frame(
        sender,
        &ServerFrame::Nack(CommandNack {
            request_id: request_id.to_owned(),
            accepted: false,
            action: action.to_owned(),
            error: ProtocolError {
                code: code.to_owned(),
                message: message.to_owned(),
            },
        }),
    )
    .await
}

async fn send_frame<S>(sender: &mut S, frame: &ServerFrame) -> Result<(), axum::Error>
where
    S: futures_util::Sink<Message, Error = axum::Error> + Unpin,
{
    let encoded = serde_json::to_string(frame).map_err(axum::Error::new)?;
    sender.send(Message::Text(encoded.into())).await
}

fn persistence_error(error: &PersistenceError) -> (&'static str, String) {
    match error {
        PersistenceError::CommandConflict => ("command_conflict", error.to_string()),
        PersistenceError::CommandRejected { code, message } => (code, message.clone()),
        PersistenceError::ParticipantMissing => ("session_revoked", error.to_string()),
        PersistenceError::RoomMissing => ("room_not_found", error.to_string()),
        PersistenceError::Database(_)
        | PersistenceError::Json(_)
        | PersistenceError::AuthorityConflict(_)
        | PersistenceError::UnownedDatabase
        | PersistenceError::WriterAlreadyActive(_)
        | PersistenceError::WriterLease(_) => (
            "persistence_failed",
            "Persistence operation failed.".to_owned(),
        ),
        PersistenceError::InvalidCursor { .. } => {
            ("invalid_cursor", "Room cursor is invalid.".to_owned())
        }
    }
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: message.into(),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: message.into(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "unavailable",
            message: message.into(),
        }
    }
}

impl From<PersistenceError> for ApiError {
    fn from(error: PersistenceError) -> Self {
        tracing::error!(error = ?error, "HTTP persistence operation failed");
        Self::unavailable("Persistence operation failed.")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": {"code": self.code, "message": self.message}})),
        )
            .into_response()
    }
}

#[allow(dead_code)]
fn _socket_address_is_send(_: SocketAddr) {}

fn persistence_error_is_internal(error: &PersistenceError) -> bool {
    matches!(
        error,
        PersistenceError::Database(_)
            | PersistenceError::Json(_)
            | PersistenceError::AuthorityConflict(_)
            | PersistenceError::UnownedDatabase
            | PersistenceError::WriterAlreadyActive(_)
            | PersistenceError::WriterLease(_)
    )
}

struct IngressBudget {
    window_started: Instant,
    messages: usize,
    bytes: usize,
}

impl IngressBudget {
    fn new() -> Self {
        Self {
            window_started: Instant::now(),
            messages: 0,
            bytes: 0,
        }
    }

    fn admit(&mut self, bytes: usize) -> bool {
        let now = Instant::now();
        if now.duration_since(self.window_started) >= INGRESS_WINDOW {
            self.window_started = now;
            self.messages = 0;
            self.bytes = 0;
        }
        self.messages = self.messages.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
        self.messages <= INGRESS_MESSAGES_PER_WINDOW && self.bytes <= INGRESS_BYTES_PER_WINDOW
    }
}

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use agentsassemble_persistence::PersistenceError;

    use super::{HostSecret, persistence_error};

    #[test]
    fn host_secret_invariant_cannot_be_bypassed_by_an_adapter() {
        assert!(HostSecret::new("short").is_err());
        assert!(HostSecret::new(" padded-host-secret-00000000000000 ").is_err());
        assert!(HostSecret::new("valid-host-secret-0000000000000001").is_ok());
    }

    #[test]
    fn internal_persistence_errors_have_a_stable_wire_message() {
        let errors = [
            PersistenceError::WriterAlreadyActive(PathBuf::from("/private/data.sqlite3")),
            PersistenceError::WriterLease(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "/private/data.sqlite3 denied",
            )),
        ];
        for error in errors {
            let (_, message) = persistence_error(&error);
            assert_eq!(message, "Persistence operation failed.");
            assert!(!message.contains("/private"));
        }
    }
}
