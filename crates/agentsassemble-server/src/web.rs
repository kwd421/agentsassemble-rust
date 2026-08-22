use std::{net::SocketAddr, sync::Arc};

use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, ProviderCatalog, SnapshotMode,
    public_settings, validate_room_id,
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
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::{RoomRuntime, TicketStore};

#[derive(Clone)]
pub struct AppState {
    pub store: SqliteStore,
    pub rooms: RoomRuntime,
    pub tickets: TicketStore,
    pub host_token: Arc<str>,
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
    Router::new()
        .route("/healthz", get(health))
        .route("/api/ws-ticket", post(issue_ticket))
        .route("/ws", get(upgrade_socket))
        .with_state(state)
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
    axum::serve(listener, router(state))
        .with_graceful_shutdown(cancellation.cancelled_owned())
        .await
        .map_err(ServeError::Io)
}

async fn health() -> Json<Value> {
    Json(json!({"status": "ready", "runtime": "rust"}))
}

async fn issue_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<TicketRequest>,
) -> Result<Json<TicketResponse>, ApiError> {
    if !state.host_token.is_empty()
        && headers
            .get("x-host-token")
            .and_then(|value| value.to_str().ok())
            != Some(state.host_token.as_ref())
    {
        return Err(ApiError::unauthorized("A valid host token is required."));
    }
    let room_id = validate_room_id(&request.meeting_id)
        .map_err(|error| ApiError::bad_request(error.message))?;
    if !state.store.room_exists(&room_id).await? {
        return Err(ApiError::not_found("Room does not exist."));
    }
    let participant = state.store.participant(&room_id, "host").await?;
    let client_kind = ClientKind::Browser;
    let invite_scope = InviteScope::ReadWrite;
    let ticket = state
        .tickets
        .issue(AuthenticatedPrincipal {
            principal_id: "local-operator".to_owned(),
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
    let principal = state
        .tickets
        .consume(&query.ticket)
        .await
        .map_err(|error| ApiError::unauthorized(error.to_string()))?;
    Ok(upgrade
        .on_upgrade(move |socket| socket_session(socket, state, principal))
        .into_response())
}

#[allow(clippy::too_many_lines)] // One select loop owns the socket's ordering and lifecycle.
async fn socket_session(socket: WebSocket, state: AppState, principal: AuthenticatedPrincipal) {
    let (mut sender, mut receiver) = socket.split();
    let Some(Ok(Message::Text(raw))) = receiver.next().await else {
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
    if resume_from_seq > snapshot_data.last_seq {
        let frame = ServerFrame::ResyncRequired {
            stream: "room_events",
            reason: "resume cursor is ahead of durable room state".to_owned(),
            latest_seq: snapshot_data.last_seq,
        };
        let _ = send_frame(&mut sender, &frame).await;
        return;
    }
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
        resume_gap: false,
        snapshot_mode: if resume_from_seq == 0 {
            SnapshotMode::Initial
        } else {
            SnapshotMode::Resume
        },
        provider_catalog: ProviderCatalog::default(),
        available_providers: Vec::new(),
        capabilities: principal.capabilities.clone(),
    }));
    if send_frame(&mut sender, &snapshot).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            incoming = receiver.next() => {
                let Some(Ok(message)) = incoming else { return; };
                let Message::Text(raw) = message else { continue; };
                match serde_json::from_str::<ClientFrame>(raw.as_str()) {
                    Ok(ClientFrame::Command { request_id, action, payload }) => {
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
        | PersistenceError::AuthorityConflict(_) => ("persistence_failed", error.to_string()),
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
        Self::unavailable(error.to_string())
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
