use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use agentsassemble_domain::{ProviderCatalog, SnapshotMode, public_settings};
use agentsassemble_persistence::{PersistenceError, SqliteStore};
use agentsassemble_protocol::{
    ClientFrame, CommandAck, CommandNack, ProtocolError, RoomSnapshot, ServerFrame, TicketResponse,
};
use axum::{
    Json, Router, body,
    extract::{
        Query, Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderValue, StatusCode, header},
    middleware,
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
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tower_http::{
    services::{ServeDir, ServeFile},
    timeout::RequestBodyDeadlineLayer,
};

use crate::{
    ConsumedTicket, RoomRuntime, TicketIssueError, TicketStore,
    http_transport::{MAX_HTTP_CONNECTIONS, RejectionCounter, serve_connection},
    ingress_budget::IngressBudget,
    issue_local_ticket,
    server_proof::{challenge_is_valid, sign_challenge},
};

const MAX_WS_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_WS_CONNECTIONS: usize = 128;
const HTTP_BODY_DEADLINE: Duration = Duration::from_secs(10);
const MAX_TICKET_BODY_BYTES: usize = 4 * 1024;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const WS_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const TRACKED_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(6);
const SOCKET_IDLE_TIMEOUT: Duration = Duration::from_mins(5);
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self' ws://127.0.0.1:*; object-src 'none'; base-uri 'none'; frame-ancestors 'none'";

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
        .layer(RequestBodyDeadlineLayer::new(HTTP_BODY_DEADLINE))
        .layer(middleware::map_response(security_headers))
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
    let http_admission = Arc::new(Semaphore::new(MAX_HTTP_CONNECTIONS));
    let rejected_connections = RejectionCounter::default();
    let app = router(state);
    let result = loop {
        let accepted = tokio::select! {
            () = cancellation.cancelled() => break Ok(()),
            accepted = listener.accept() => accepted,
        };
        let (stream, _) = match accepted {
            Ok(accepted) => accepted,
            Err(error) => break Err(error),
        };
        let Ok(permit) = http_admission.clone().try_acquire_owned() else {
            rejected_connections.record();
            drop(stream);
            continue;
        };
        let connection_app = app.clone();
        let shutdown = connection_shutdown.clone();
        connections.spawn(async move {
            tokio::select! {
                () = shutdown.cancelled() => {}
                () = serve_connection(stream, connection_app, permit) => {}
            }
        });
    };
    connection_shutdown.cancel();
    connections.close();
    if tokio::time::timeout(TRACKED_SHUTDOWN_TIMEOUT, connections.wait())
        .await
        .is_err()
    {
        tracing::warn!("tracked connections exceeded the shutdown deadline");
    }
    let rejected = rejected_connections.total();
    if rejected > 0 {
        tracing::warn!(rejected, "HTTP overload connections were rejected");
    }
    rooms.shutdown().await;
    result.map_err(ServeError::Io)
}

async fn security_headers(mut response: Response) -> Response {
    let is_upgrade = response.status() == StatusCode::SWITCHING_PROTOCOLS;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if !is_upgrade {
        headers.insert(header::CONNECTION, HeaderValue::from_static("close"));
    }
    response
}

async fn health() -> Json<Value> {
    Json(json!({"status": "ready", "runtime": "rust"}))
}

async fn issue_ticket(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<TicketResponse>, ApiError> {
    let provided_token = request
        .headers()
        .get("x-host-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !state.host_token.matches(provided_token) {
        return Err(ApiError::unauthorized("A valid host token is required."));
    }
    if request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > MAX_TICKET_BODY_BYTES)
    {
        return Err(ApiError::payload_too_large(
            "Ticket request body exceeds the route limit.",
        ));
    }
    let encoded = body::to_bytes(request.into_body(), MAX_TICKET_BODY_BYTES)
        .await
        .map_err(|_| ApiError::payload_too_large("Ticket request body exceeds the route limit."))?;
    let request: TicketRequest = serde_json::from_slice(&encoded)
        .map_err(|_| ApiError::bad_request("Ticket request JSON is invalid."))?;
    issue_local_ticket(&state, &request.meeting_id)
        .await
        .map(Json)
        .map_err(ApiError::from)
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
    let grant = state
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
            connections.track_future(socket_session(socket, state, grant, permit))
        })
        .into_response())
}

#[allow(clippy::too_many_lines)] // One select loop owns the socket's ordering and lifecycle.
async fn socket_session(
    socket: WebSocket,
    state: AppState,
    grant: ConsumedTicket,
    _permit: OwnedSemaphorePermit,
) {
    let ConsumedTicket {
        principal,
        proof_key,
    } = grant;
    let (mut sender, mut receiver) = socket.split();
    let incoming = tokio::select! {
        () = state.shutdown.cancelled() => return,
        incoming = tokio::time::timeout(HANDSHAKE_TIMEOUT, receiver.next()) => {
            let Ok(incoming) = incoming else {
                let _ = send_nack(&mut sender, &state.shutdown, "", "subscribe", "subscribe_timeout", "Subscription was not received within 10 seconds.").await;
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
        server_challenge,
    }) = serde_json::from_str(raw.as_str())
    else {
        let _ = send_nack(
            &mut sender,
            &state.shutdown,
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
            &state.shutdown,
            "",
            "subscribe",
            "invalid_subscription",
            "room_events and a non-negative cursor are required.",
        )
        .await;
        return;
    }
    let server_proof = match server_challenge {
        Some(challenge) if challenge_is_valid(&challenge) => sign_challenge(&proof_key, &challenge),
        Some(_) => {
            let _ = send_nack(
                &mut sender,
                &state.shutdown,
                "",
                "subscribe",
                "server_challenge_invalid",
                "The server challenge must be 32 random bytes encoded as hexadecimal.",
            )
            .await;
            return;
        }
        None => String::new(),
    };
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
            let _ = send_frame(&mut sender, &state.shutdown, &frame).await;
            return;
        }
        Err(error) => {
            tracing::error!(error = ?error, room_id = %principal.room_id, "room snapshot failed");
            let _ = send_nack(
                &mut sender,
                &state.shutdown,
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
                &state.shutdown,
                "",
                "subscribe",
                "snapshot_failed",
                &error.to_string(),
            )
            .await;
            return;
        }
    };
    let snapshot = RoomSnapshot {
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
        server_proof,
    };
    let Some(snapshot) = fit_snapshot_frame(snapshot) else {
        let _ = send_nack(
            &mut sender,
            &state.shutdown,
            "",
            "subscribe",
            "snapshot_too_large",
            "Room metadata exceeds the WebSocket snapshot limit.",
        )
        .await;
        return;
    };
    if send_frame(&mut sender, &state.shutdown, &snapshot)
        .await
        .is_err()
    {
        return;
    }
    let mut ingress = IngressBudget::new();
    loop {
        tokio::select! {
            () = state.shutdown.cancelled() => return,
            incoming = tokio::time::timeout(SOCKET_IDLE_TIMEOUT, receiver.next()) => {
                let Ok(Some(Ok(message))) = incoming else { return; };
                let (frame_bytes, control_frame) = match &message {
                    Message::Text(raw) => (raw.len(), false),
                    Message::Binary(raw) => (raw.len(), false),
                    Message::Ping(raw) | Message::Pong(raw) => (raw.len(), true),
                    Message::Close(_) => return,
                };
                if !ingress.admit(frame_bytes, control_frame) {
                    let _ = send_nack(&mut sender, &state.shutdown, "", "frame", "ingress_limited", "WebSocket ingress budget exceeded.").await;
                    return;
                }
                let Message::Text(raw) = message else {
                    if matches!(message, Message::Binary(_)) {
                        let _ = send_nack(&mut sender, &state.shutdown, "", "frame", "binary_frame_unsupported", "Binary WebSocket frames are not supported.").await;
                        return;
                    }
                    continue;
                };
                match serde_json::from_str::<ClientFrame>(raw.as_str()) {
                    Ok(ClientFrame::Command { request_id, action, payload }) => {
                        if request_id.is_empty()
                            || request_id.chars().count() > 128
                            || action.is_empty()
                            || action.chars().count() > 64
                        {
                            if send_nack(&mut sender, &state.shutdown, &request_id, &action, "command_envelope_invalid", "request_id or action is invalid.").await.is_err() { return; }
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
                                if send_frame(&mut sender, &state.shutdown, &frame).await.is_err() { return; }
                            }
                            Err(error) => {
                                if persistence_error_is_internal(&error) {
                                    tracing::error!(error = ?error, room_id = %principal.room_id, action = %action, "room command persistence failed");
                                }
                                let (code, message) = persistence_error(&error);
                                if send_nack(&mut sender, &state.shutdown, &request_id, &action, code, &message).await.is_err() { return; }
                            }
                        }
                    }
                    Ok(ClientFrame::Ping { nonce }) => {
                        if send_frame(&mut sender, &state.shutdown, &ServerFrame::Pong { nonce }).await.is_err() { return; }
                    }
                    Ok(ClientFrame::Subscribe { .. }) => {
                        if send_nack(&mut sender, &state.shutdown, "", "subscribe", "already_subscribed", "This socket is already subscribed.").await.is_err() { return; }
                    }
                    Err(error) => {
                        if send_nack(&mut sender, &state.shutdown, "", "frame", "frame_invalid", &error.to_string()).await.is_err() { return; }
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
                        if send_frame(&mut sender, &state.shutdown, &frame).await.is_err() { return; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let frame = ServerFrame::ResyncRequired {
                            stream: "room_events",
                            reason: "subscriber fell behind the room event stream".to_owned(),
                            latest_seq: state.store.snapshot(&principal.room_id, 0, 1).await.map_or(0, |snapshot| snapshot.last_seq),
                        };
                        let _ = send_frame(&mut sender, &state.shutdown, &frame).await;
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
    cancellation: &CancellationToken,
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
        cancellation,
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

async fn send_frame<S>(
    sender: &mut S,
    cancellation: &CancellationToken,
    frame: &ServerFrame,
) -> Result<(), axum::Error>
where
    S: futures_util::Sink<Message, Error = axum::Error> + Unpin,
{
    let encoded = serde_json::to_string(frame).map_err(axum::Error::new)?;
    tokio::select! {
        () = cancellation.cancelled() => Err(axum::Error::new(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "runtime shutdown interrupted WebSocket send",
        ))),
        result = tokio::time::timeout(WS_WRITE_TIMEOUT, sender.send(Message::Text(encoded.into()))) => {
            result.map_err(axum::Error::new)?
        }
    }
}

fn fit_snapshot_frame(mut snapshot: RoomSnapshot) -> Option<ServerFrame> {
    loop {
        let frame = ServerFrame::Snapshot(Box::new(snapshot.clone()));
        if serde_json::to_vec(&frame).ok()?.len() <= MAX_WS_MESSAGE_BYTES {
            return Some(frame);
        }
        if snapshot.events.is_empty() {
            return None;
        }
        let remove = (snapshot.events.len() / 2).max(1);
        snapshot.events.drain(..remove);
        snapshot.oldest_seq = snapshot
            .events
            .first()
            .map_or(snapshot.last_seq, |event| event.seq);
        snapshot.has_more_before = true;
        if snapshot.snapshot_mode != SnapshotMode::Initial {
            snapshot.resume_gap = true;
            snapshot.snapshot_mode = SnapshotMode::Gap;
        }
    }
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

    fn payload_too_large(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            code: "payload_too_large",
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

impl From<TicketIssueError> for ApiError {
    fn from(error: TicketIssueError) -> Self {
        match error {
            TicketIssueError::InvalidRoom(message) => Self::bad_request(message),
            TicketIssueError::RoomMissing => Self::not_found("Room does not exist."),
            TicketIssueError::ParticipantInactive => {
                Self::unauthorized("The local operator is not an active room participant.")
            }
            TicketIssueError::Persistence(error) => Self::from(error),
            TicketIssueError::Unavailable => Self::unavailable("Ticket capacity is unavailable."),
        }
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

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use agentsassemble_persistence::PersistenceError;

    use crate::ingress_budget::{CONTROL_FRAMES_PER_WINDOW, IngressBudget};

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

    #[test]
    fn control_frames_have_an_independent_ingress_budget() {
        let mut budget = IngressBudget::new();
        for _ in 0..CONTROL_FRAMES_PER_WINDOW {
            assert!(budget.admit(0, true));
        }
        assert!(!budget.admit(0, true));
    }
}
