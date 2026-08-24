use std::{net::SocketAddr, sync::Arc, time::Duration};

use agentsassemble_domain::public_event_for_principal;
use agentsassemble_persistence::PersistenceError;
use agentsassemble_protocol::{ClientFrame, CommandAck, ServerFrame};
use axum::{
    Json, Router, body,
    extract::{
        Query, Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{StatusCode, header},
    middleware,
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore},
};
use tokio_util::sync::CancellationToken;
use tower_http::{
    services::{ServeDir, ServeFile},
    timeout::RequestBodyDeadlineLayer,
};

use crate::{
    AppState, ConsumedTicket, RoomShutdownError, TicketIssueError,
    authenticated_channel::MAX_WS_WIRE_MESSAGE_BYTES,
    host_ticket::{AuthenticatedTicketResponse, HostChallengeResponse},
    http_transport::{MAX_HTTP_CONNECTIONS, RejectionCounter, serve_connection},
    ingress_budget::IngressBudget,
    issue_local_ticket, reconcile_runtime_ownership,
    room_socket::{
        EstablishedSubscription, establish, persistence_error, persistence_error_is_internal,
    },
};

const HTTP_BODY_DEADLINE: Duration = Duration::from_secs(10);
const MAX_TICKET_BODY_BYTES: usize = 4 * 1024;
const TRACKED_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(6);
const SOCKET_IDLE_TIMEOUT: Duration = Duration::from_mins(5);
#[derive(Debug, Error)]
pub enum ServeError {
    #[error("server I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("provider discovery task failed: {0}")]
    ProviderDiscovery(#[from] tokio::task::JoinError),
    #[error("room runtime shutdown failed: {0}")]
    RoomShutdown(#[from] RoomShutdownError),
    #[error("runtime reconciliation failed: {0}")]
    Reconciliation(#[from] PersistenceError),
}

#[derive(Debug, Deserialize)]
struct TicketQuery {
    ticket: String,
}

pub fn router(state: AppState) -> Router {
    let frontend_root = state.frontend_root.clone();
    let mut app = core_routes()
        .merge(crate::room_directory_web::routes())
        .merge(crate::profile_web::routes());
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
        .layer(middleware::map_response(crate::security_headers::apply))
}

registered_routes! {
    fn core_routes<AppState>() {
        "/healthz" => get(health),
        "/api/host-challenge" => get(issue_host_challenge),
        "/api/ws-ticket" => post(issue_ticket),
        "/ws" => get(upgrade_socket),
    }
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
    reconcile_runtime_ownership(&state.store, &state.provider_adapter).await?;
    let rooms = state.rooms.clone();
    let provider_catalog = state.provider_catalog.clone();
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
    let room_shutdown = rooms.shutdown().await;
    let provider_shutdown = provider_catalog.shutdown().await;
    room_shutdown?;
    provider_shutdown?;
    result.map_err(ServeError::Io)
}

async fn health() -> Json<Value> {
    Json(json!({"status": "ready", "runtime": "rust"}))
}

async fn issue_host_challenge(
    State(state): State<AppState>,
) -> Result<Json<HostChallengeResponse>, ApiError> {
    state
        .host_token
        .challenge()
        .map(Json)
        .ok_or_else(|| ApiError::unavailable("Host challenge capacity is unavailable."))
}

async fn issue_ticket(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<AuthenticatedTicketResponse>, ApiError> {
    let Some(authenticated) = state
        .host_token
        .authenticate_ticket_request(request.headers())
    else {
        return Err(ApiError::unauthorized("A valid host proof is required."));
    };
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
    if !encoded.is_empty() {
        return Err(ApiError::bad_request(
            "Ticket requests must not contain a body.",
        ));
    }
    let grant = issue_local_ticket(&state, &authenticated.meeting_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(state.host_token.authenticated_ticket_response(
        &authenticated.challenge,
        grant,
    )))
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
        .max_message_size(MAX_WS_WIRE_MESSAGE_BYTES)
        .max_frame_size(MAX_WS_WIRE_MESSAGE_BYTES)
        .write_buffer_size(64 * 1024)
        .max_write_buffer_size(512 * 1024)
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
    let (mut sender, mut receiver) = socket.split();
    let Some(EstablishedSubscription {
        principal,
        mut events,
        mut catalog_updates,
        mut delivered_seq,
        mut channel,
    }) = establish(&mut sender, &mut receiver, &state, grant).await
    else {
        return;
    };
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
                    let _ = channel.send_nack(&mut sender, &state.shutdown, "", "frame", "ingress_limited", "WebSocket ingress budget exceeded.").await;
                    return;
                }
                let Message::Text(raw) = message else {
                    if matches!(message, Message::Binary(_)) {
                        let _ = channel.send_nack(&mut sender, &state.shutdown, "", "frame", "binary_frame_unsupported", "Binary WebSocket frames are not supported.").await;
                        return;
                    }
                    continue;
                };
                let Ok((client_frame, authenticated_bytes)) =
                    channel.decode_client(raw.as_str())
                else {
                    let _ = channel.send_nack(&mut sender, &state.shutdown, "", "frame", "frame_authentication_invalid", "WebSocket frame authentication failed.").await;
                    return;
                };
                match client_frame {
                    ClientFrame::Command { request_id, action, payload } => {
                        let action = action.as_str().to_owned();
                        let outcome = state.rooms.execute(
                            principal.clone(), request_id.clone(), action.clone(), payload, authenticated_bytes,
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
                                if channel.send(&mut sender, &state.shutdown, &frame).await.is_err() { return; }
                            }
                            Err(error) => {
                                if persistence_error_is_internal(&error) {
                                    tracing::error!(error = ?error, room_id = %principal.room_id, action = %action, "room command persistence failed");
                                }
                                let (code, message) = persistence_error(&error);
                                if channel.send_nack(&mut sender, &state.shutdown, &request_id, &action, code, &message).await.is_err() { return; }
                            }
                        }
                    }
                    ClientFrame::Ping { nonce } => {
                        if channel.send(&mut sender, &state.shutdown, &ServerFrame::Pong { nonce }).await.is_err() { return; }
                    }
                    ClientFrame::Subscribe { .. } => {
                        if channel.send_nack(&mut sender, &state.shutdown, "", "subscribe", "already_subscribed", "This socket is already subscribed.").await.is_err() { return; }
                    }
                }
            }
            published = events.recv() => {
                match published {
                    Ok(event) => {
                        if event.seq <= delivered_seq {
                            continue;
                        }
                        if event.seq != delivered_seq.saturating_add(1) {
                            let frame = ServerFrame::ResyncRequired {
                                stream: "room_events",
                                reason: "live room event sequence is not contiguous".to_owned(),
                                latest_seq: state.store.snapshot(&principal.room_id, 0, 1).await.map_or(delivered_seq, |snapshot| snapshot.last_seq),
                            };
                            let _ = channel.send(&mut sender, &state.shutdown, &frame).await;
                            return;
                        }
                        let current_principal = match state.store.resolve_principal(&principal).await {
                            Ok(principal) => principal,
                            Err(error) => {
                                if persistence_error_is_internal(&error) {
                                    tracing::error!(error = ?error, room_id = %principal.room_id, "live principal resolution failed");
                                }
                                let (code, message) = persistence_error(&error);
                                let _ = channel.send_nack(
                                    &mut sender,
                                    &state.shutdown,
                                    "",
                                    "session",
                                    code,
                                    &message,
                                ).await;
                                return;
                            }
                        };
                        let latest_seq = event.seq;
                        let frame = ServerFrame::Event {
                            stream: "room_events",
                            events: vec![public_event_for_principal(&event, &current_principal)],
                            latest_seq,
                        };
                        if channel.send(&mut sender, &state.shutdown, &frame).await.is_err() { return; }
                        delivered_seq = latest_seq;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let frame = ServerFrame::ResyncRequired {
                            stream: "room_events",
                            reason: "subscriber fell behind the room event stream".to_owned(),
                            latest_seq: state.store.snapshot(&principal.room_id, 0, 1).await.map_or(0, |snapshot| snapshot.last_seq),
                        };
                        let _ = channel.send(&mut sender, &state.shutdown, &frame).await;
                        return;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
            changed = catalog_updates.changed() => {
                if changed.is_err() {
                    continue;
                }
                let frame = ServerFrame::ProviderCatalogUpdated {
                    catalog: catalog_updates.borrow_and_update().clone(),
                };
                if channel.send(&mut sender, &state.shutdown, &frame).await.is_err() { return; }
            }
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
            TicketIssueError::BootstrapIncomplete => {
                Self::unauthorized("Local identity bootstrap is not complete.")
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

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use agentsassemble_persistence::PersistenceError;

    use crate::ingress_budget::{CONTROL_FRAMES_PER_WINDOW, IngressBudget};

    use crate::HostSecret;
    use crate::room_socket::persistence_error;

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
