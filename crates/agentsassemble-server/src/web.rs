use std::{future::Future, net::SocketAddr, sync::Arc, time::Duration};

use agentsassemble_domain::public_event_for_principal;
use agentsassemble_persistence::PersistenceError;
use agentsassemble_protocol::{
    ClientFrame, CommandAck, CommandResolution, ProtocolError, ServerFrame,
};
use axum::{
    Json, Router,
    extract::{
        Query, Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    middleware,
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::{net::TcpListener, sync::Semaphore, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tower_http::{
    services::{ServeDir, ServeFile},
    timeout::RequestBodyDeadlineLayer,
};

use crate::{
    AppState, ConsumedTicket, RoomShutdownError, TicketIssueError,
    authenticated_channel::MAX_WS_WIRE_MESSAGE_BYTES,
    connection_admission::ConnectionLease,
    host_ticket::{AuthenticatedTicketResponse, HostChallengeResponse},
    http_api::{BodyDecodeError, ensure_empty_body},
    http_transport::{MAX_HTTP_CONNECTIONS, RejectionCounter, serve_connection},
    issue_local_ticket,
    provider_turn_reconciliation_runtime::reconcile_provider_turn_ownership,
    reconcile_runtime_ownership,
    room_socket::{
        EstablishedSubscription, establish, persistence_error, persistence_error_is_internal,
    },
    runtime_reconciliation::watch_runtime_reconciliation,
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
    #[error("runtime reconciliation task failed: {0}")]
    RuntimeReconciliationTask(tokio::task::JoinError),
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
        .merge(crate::room_preferences_web::routes())
        .merge(crate::profile_web::routes())
        .merge(crate::human_session_exchange_web::routes())
        .merge(crate::human_invite_web::routes());
    if state.central_registration_enabled {
        app = app.merge(crate::central_registration_web::routes());
    }
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
    let reconciled_turns = Box::pin(reconcile_provider_turn_ownership(
        &state.store,
        &state.provider_adapter,
        &state.rooms,
    ))
    .await?;
    if reconciled_turns > 0 {
        tracing::warn!(
            reconciled_turns,
            "reconciled provider turn authority before network admission"
        );
    }
    let reconciliation = reconcile_runtime_ownership(&state.store, &state.provider_adapter).await?;
    if reconciliation.reconciled_sessions > 0 {
        tracing::warn!(
            reconciled_sessions = reconciliation.reconciled_sessions,
            "reconciled provider runtime authority before network admission"
        );
    }
    for assignment in reconciliation.assignments {
        let room_id = assignment.session.public.room_id.clone();
        state
            .rooms
            .publish_then_resume_assigned_turns(&room_id, vec![assignment])
            .await?;
    }
    let rooms = state.rooms.clone();
    let provider_catalog = state.provider_catalog.clone();
    let connections = state.connections.clone();
    let connection_shutdown = state.shutdown.clone();
    let http_admission = Arc::new(Semaphore::new(MAX_HTTP_CONNECTIONS));
    let rejected_connections = RejectionCounter::default();
    let reconciliation_owner = tokio::spawn(watch_runtime_reconciliation(
        state.store.clone(),
        state.provider_adapter.clone(),
        rooms.clone(),
        connection_shutdown.clone(),
    ));
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
    let (reconciliation_shutdown, (room_shutdown, provider_shutdown)) =
        drain_reconciliation_then(reconciliation_owner, async {
            let room_shutdown = rooms.shutdown().await;
            let provider_shutdown = provider_catalog.shutdown().await;
            (room_shutdown, provider_shutdown)
        })
        .await;
    room_shutdown?;
    provider_shutdown?;
    reconciliation_shutdown.map_err(ServeError::RuntimeReconciliationTask)?;
    result.map_err(ServeError::Io)
}

async fn drain_reconciliation_then<T>(
    owner: JoinHandle<()>,
    shutdown: impl Future<Output = T>,
) -> (Result<(), tokio::task::JoinError>, T) {
    let reconciliation = drain_reconciliation_owner(owner).await;
    (reconciliation, shutdown.await)
}

async fn drain_reconciliation_owner(owner: JoinHandle<()>) -> Result<(), tokio::task::JoinError> {
    drain_reconciliation_owner_after(owner, TRACKED_SHUTDOWN_TIMEOUT).await
}

async fn drain_reconciliation_owner_after(
    mut owner: JoinHandle<()>,
    warning_after: Duration,
) -> Result<(), tokio::task::JoinError> {
    if let Ok(result) = tokio::time::timeout(warning_after, &mut owner).await {
        result
    } else {
        tracing::warn!(
            "runtime reconciliation exceeded the shutdown deadline; waiting for exact custody to drain"
        );
        owner.await
    }
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
    ensure_empty_body(request, MAX_TICKET_BODY_BYTES)
        .await
        .map_err(ApiError::from_body)?;
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
    let grant = state
        .tickets
        .consume(&query.ticket)
        .await
        .map_err(|error| ApiError::unauthorized(error.to_string()))?;
    let lease = state
        .connection_admission
        .acquire(&grant.principal)
        .map_err(|error| ApiError::unavailable(error.to_string()))?;
    let connections = state.connections.clone();
    Ok(upgrade
        .max_message_size(MAX_WS_WIRE_MESSAGE_BYTES)
        .max_frame_size(MAX_WS_WIRE_MESSAGE_BYTES)
        .write_buffer_size(64 * 1024)
        .max_write_buffer_size(512 * 1024)
        .on_upgrade(move |socket| {
            connections.track_future(socket_session(socket, state, grant, lease))
        })
        .into_response())
}

#[allow(clippy::too_many_lines)] // One select loop owns the socket's ordering and lifecycle.
async fn socket_session(
    socket: WebSocket,
    state: AppState,
    grant: ConsumedTicket,
    _lease: ConnectionLease,
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
                if !state.raw_ingress.admit(&principal, frame_bytes, control_frame) {
                    let _ = channel.send_nack(&mut sender, &state.shutdown, "", "frame", CommandResolution::Unresolved, ProtocolError::new("ingress_limited", "WebSocket ingress budget exceeded.")).await;
                    return;
                }
                let Message::Text(raw) = message else {
                    if matches!(message, Message::Binary(_)) {
                        let _ = channel.send_nack(&mut sender, &state.shutdown, "", "frame", CommandResolution::Unresolved, ProtocolError::new("binary_frame_unsupported", "Binary WebSocket frames are not supported.")).await;
                        return;
                    }
                    continue;
                };
                let Ok((client_frame, _authenticated_bytes)) =
                    channel.decode_client(raw.as_str())
                else {
                    let _ = channel.send_nack(&mut sender, &state.shutdown, "", "frame", CommandResolution::Unresolved, ProtocolError::new("frame_authentication_invalid", "WebSocket frame authentication failed.")).await;
                    return;
                };
                match client_frame {
                    ClientFrame::Command { request_id, action, payload } => {
                        let action_name = action.as_str().to_owned();
                        let outcome = state.rooms.execute(
                            principal.clone(), request_id.clone(), action, payload,
                        ).await;
                        match outcome {
                            Ok(outcome) => {
                                let frame = ServerFrame::Ack(CommandAck {
                                    request_id,
                                    accepted: true,
                                    resolution: CommandResolution::Committed,
                                    action: action_name,
                                    result: outcome.result,
                                    deduplicated: outcome.deduplicated,
                                });
                                if channel.send(&mut sender, &state.shutdown, &frame).await.is_err() { return; }
                            }
                            Err(failure) => {
                                if persistence_error_is_internal(&failure.error) {
                                    tracing::error!(error = ?failure.error, room_id = %principal.room_id, action = %action_name, "room command persistence failed");
                                }
                                let (code, message) = persistence_error(&failure.error);
                                if channel.send_nack(&mut sender, &state.shutdown, &request_id, &action_name, failure.resolution, ProtocolError::new(code, message)).await.is_err() { return; }
                            }
                        }
                    }
                    ClientFrame::Ping { nonce } => {
                        if channel.send(&mut sender, &state.shutdown, &ServerFrame::Pong { nonce }).await.is_err() { return; }
                    }
                    ClientFrame::Subscribe { .. } => {
                        if channel.send_nack(&mut sender, &state.shutdown, "", "subscribe", CommandResolution::Unresolved, ProtocolError::new("already_subscribed", "This socket is already subscribed.")).await.is_err() { return; }
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
                                    CommandResolution::Unresolved,
                                    ProtocolError::new(code, message),
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
    fn from_body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self {
                status: StatusCode::REQUEST_TIMEOUT,
                code: "request_timeout",
                message: "Request body timed out.".to_owned(),
            },
            BodyDecodeError::PayloadTooLarge => {
                Self::payload_too_large("Ticket request body exceeds the route limit.")
            }
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => {
                Self::bad_request("Ticket requests must not contain a body.")
            }
        }
    }

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
    use std::{
        io,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use agentsassemble_persistence::PersistenceError;

    use crate::HostSecret;
    use crate::room_socket::persistence_error;

    use super::{drain_reconciliation_owner_after, drain_reconciliation_then};

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

    #[tokio::test]
    async fn reconciliation_owner_is_joined_after_its_warning_deadline() {
        let (release, blocked) = tokio::sync::oneshot::channel();
        let owner = tokio::spawn(async move {
            let _ = blocked.await;
        });
        let drain = tokio::spawn(drain_reconciliation_owner_after(
            owner,
            Duration::from_millis(1),
        ));

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            !drain.is_finished(),
            "warning timeout must not detach custody"
        );
        release
            .send(())
            .unwrap_or_else(|()| panic!("reconciliation owner was dropped before release"));
        drain
            .await
            .unwrap_or_else(|error| panic!("join drain task: {error}"))
            .unwrap_or_else(|error| panic!("join reconciliation owner: {error}"));
    }

    #[tokio::test]
    async fn reconciliation_panic_does_not_skip_shutdown_cleanup() {
        let owner = tokio::spawn(async {
            panic!("simulated reconciliation owner failure");
        });
        let cleaned = Arc::new(AtomicBool::new(false));
        let cleanup_observation = Arc::clone(&cleaned);

        let (reconciliation, ()) = drain_reconciliation_then(owner, async move {
            cleanup_observation.store(true, Ordering::SeqCst);
        })
        .await;

        let Err(error) = reconciliation else {
            panic!("panicked reconciliation owner must remain an observable failure");
        };
        assert!(error.is_panic());
        assert!(cleaned.load(Ordering::SeqCst));
    }
}
