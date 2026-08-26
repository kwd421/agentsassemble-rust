use std::{future::Future, net::SocketAddr, sync::Arc, time::Duration};

use agentsassemble_persistence::PersistenceError;
use axum::{
    Json, Router,
    extract::{Query, Request, State, WebSocketUpgrade},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
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
    AppState, RoomShutdownError, TicketIssueError,
    authenticated_channel::MAX_WS_WIRE_MESSAGE_BYTES,
    host_ticket::{AuthenticatedTicketResponse, HostChallengeResponse},
    http_api::{BodyDecodeError, ensure_empty_body},
    http_transport::{MAX_HTTP_CONNECTIONS, RejectionCounter, serve_connection},
    issue_local_ticket,
    provider_turn_reconciliation_runtime::reconcile_provider_turn_ownership,
    reconcile_runtime_ownership,
    runtime_reconciliation::watch_runtime_reconciliation,
    ticket::{ConsumedSocketTicket, SocketTicketHint},
};

const HTTP_BODY_DEADLINE: Duration = Duration::from_secs(10);
const MAX_TICKET_BODY_BYTES: usize = 4 * 1024;
const TRACKED_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(6);
pub(crate) const JOIN_PATH: &str = "/join";
pub(crate) const JOIN_SLASH_PATH: &str = "/join/";
pub(crate) const PAIR_PATH: &str = "/pair";
pub(crate) const PAIR_SLASH_PATH: &str = "/pair/";
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
        let assets = frontend_root.join("assets");
        app = app
            .route("/", get(|| async { Redirect::temporary("/app/") }))
            .route_service(JOIN_PATH, ServeFile::new(index.clone()))
            .route_service(JOIN_SLASH_PATH, ServeFile::new(index.clone()))
            .route_service(PAIR_PATH, ServeFile::new(index.clone()))
            .route_service(PAIR_SLASH_PATH, ServeFile::new(index.clone()))
            .nest_service("/assets", ServeDir::new(assets))
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
    let hint = state
        .tickets
        .socket_ticket_hint(&query.ticket)
        .await
        .map_err(|error| ApiError::unauthorized(error.to_string()))?;
    let revocations = match &hint {
        SocketTicketHint::Local => None,
        SocketTicketHint::HumanSession { room_id } => {
            Some(state.rooms.session_revocations(room_id).await)
        }
    };
    let grant = state
        .tickets
        .consume_socket(&query.ticket)
        .await
        .map_err(|error| ApiError::unauthorized(error.to_string()))?;
    if !socket_hint_matches_grant(&hint, &grant) {
        return Err(ApiError::unauthorized("Socket ticket authority changed."));
    }
    let lease = state
        .connection_admission
        .acquire(grant.principal())
        .map_err(|error| ApiError::unavailable(error.to_string()))?;
    let connections = state.connections.clone();
    Ok(upgrade
        .max_message_size(MAX_WS_WIRE_MESSAGE_BYTES)
        .max_frame_size(MAX_WS_WIRE_MESSAGE_BYTES)
        .write_buffer_size(64 * 1024)
        .max_write_buffer_size(512 * 1024)
        .on_upgrade(move |socket| {
            connections.track_future(crate::room_socket_session::run(
                socket,
                state,
                grant,
                revocations,
                lease,
            ))
        })
        .into_response())
}

fn socket_hint_matches_grant(hint: &SocketTicketHint, grant: &ConsumedSocketTicket) -> bool {
    match (hint, grant) {
        (SocketTicketHint::Local, ConsumedSocketTicket::Local(_)) => true,
        (SocketTicketHint::HumanSession { room_id }, ConsumedSocketTicket::HumanSession(_)) => {
            room_id == &grant.principal().room_id
        }
        _ => false,
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
