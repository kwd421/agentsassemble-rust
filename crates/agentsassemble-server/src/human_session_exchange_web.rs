use agentsassemble_domain::InviteScope;
use agentsassemble_persistence::PersistenceError;
use agentsassemble_protocol::{CommandResolution, RoomAction};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, bearer_ticket, decode_json_body, ensure_empty_body,
        exact_tauri_cors,
    },
    human_session_bearer::fingerprint_presented_bearer,
    room_command_result::CommandFailure,
    room_socket::{persistence_error, persistence_error_is_internal},
};

const MAX_EXCHANGE_BODY_BYTES: usize = 4 * 1024;

#[derive(Serialize)]
struct SessionTicketResponse {
    ticket: String,
    ttl_seconds: u64,
}

#[derive(Serialize)]
struct SessionSocketTicketResponse {
    ticket: String,
    ttl_seconds: u64,
    server_proof_key: String,
}

#[derive(Serialize)]
struct LeaveResponse {
    status: &'static str,
    agent_id: String,
}

pub(crate) fn routes() -> Router<AppState> {
    session_exchange_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::POST]))
}

registered_routes! {
    fn session_exchange_routes<AppState>() {
        same_origin_public "/api/session-tickets/profile" => post(issue_profile_ticket),
        same_origin_public "/api/session-tickets/socket" => post(issue_socket_ticket),
        same_origin_public "/api/session-tickets/preferences-read" => post(issue_preferences_read_ticket),
        same_origin_public "/api/session-tickets/preferences-write" => post(issue_preferences_write_ticket),
        same_origin_public "/api/room-invite/leave" => post(leave_room),
    }
}

async fn leave_room(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<LeaveResponse>, SessionExchangeError> {
    let authorization = authorize_presented_session(&state, request.headers()).await?;
    let payload: Value = decode_json_body(request, MAX_EXCHANGE_BODY_BYTES)
        .await
        .map_err(SessionExchangeError::from_leave_body)?;
    let participant_id = authorization.principal().participant_id.clone();
    state
        .rooms
        .execute_human_session(
            &authorization,
            uuid::Uuid::new_v4().to_string(),
            RoomAction::ParticipantLeave,
            payload,
        )
        .await
        .map_err(|failure| SessionExchangeError::from_command(&failure))?;
    Ok(Json(LeaveResponse {
        status: "left",
        agent_id: participant_id,
    }))
}

async fn issue_profile_ticket(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, SessionExchangeError> {
    let authorization = authorize_exchange(&state, request).await?;
    let ttl_seconds = session_ticket_ttl(&state, &authorization);
    let issued = state
        .tickets
        .issue_human_session_profile(authorization)
        .await
        .map_err(|_| SessionExchangeError::capacity())?;
    Ok(Json(SessionTicketResponse {
        ticket: issued.ticket,
        ttl_seconds,
    })
    .into_response())
}

async fn issue_socket_ticket(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, SessionExchangeError> {
    let authorization = authorize_exchange(&state, request).await?;
    let ttl_seconds = session_ticket_ttl(&state, &authorization);
    let issued = state
        .tickets
        .issue_human_session_socket(authorization)
        .await
        .map_err(|_| SessionExchangeError::capacity())?;
    Ok(Json(SessionSocketTicketResponse {
        ticket: issued.ticket,
        ttl_seconds,
        server_proof_key: issued.proof_key,
    })
    .into_response())
}

async fn issue_preferences_read_ticket(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, SessionExchangeError> {
    let authorization = authorize_exchange(&state, request).await?;
    let ttl_seconds = session_ticket_ttl(&state, &authorization);
    let issued = state
        .tickets
        .issue_human_session_preferences_read(authorization)
        .await
        .map_err(|_| SessionExchangeError::capacity())?;
    Ok(Json(SessionTicketResponse {
        ticket: issued.ticket,
        ttl_seconds,
    })
    .into_response())
}

async fn issue_preferences_write_ticket(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, SessionExchangeError> {
    let authorization = authorize_exchange(&state, request).await?;
    if authorization.principal().invite_scope != InviteScope::ReadWrite {
        return Err(SessionExchangeError::read_only());
    }
    let ttl_seconds = session_ticket_ttl(&state, &authorization);
    let issued = state
        .tickets
        .issue_human_session_preferences_write(authorization)
        .await
        .map_err(|_| SessionExchangeError::capacity())?;
    Ok(Json(SessionTicketResponse {
        ticket: issued.ticket,
        ttl_seconds,
    })
    .into_response())
}

async fn authorize_exchange(
    state: &AppState,
    request: Request,
) -> Result<agentsassemble_persistence::HumanSessionAuthorization, SessionExchangeError> {
    let authorization = authorize_presented_session(state, request.headers()).await?;
    ensure_empty_body(request, MAX_EXCHANGE_BODY_BYTES)
        .await
        .map_err(SessionExchangeError::from_body)?;
    Ok(authorization)
}

async fn authorize_presented_session(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<agentsassemble_persistence::HumanSessionAuthorization, SessionExchangeError> {
    let fingerprint = bearer_ticket(headers)
        .and_then(fingerprint_presented_bearer)
        .ok_or_else(SessionExchangeError::unauthorized)?;
    state
        .store
        .authorize_human_session(&fingerprint)
        .await
        .map_err(Into::into)
}

fn session_ticket_ttl(
    state: &AppState,
    authorization: &agentsassemble_persistence::HumanSessionAuthorization,
) -> u64 {
    state.tickets.ttl_seconds().min(
        authorization
            .expires_at()
            .signed_duration_since(Utc::now())
            .num_seconds()
            .max(0)
            .try_into()
            .unwrap_or(0),
    )
}

#[derive(Debug)]
struct SessionExchangeError {
    status: StatusCode,
    code: String,
    message: String,
}

impl SessionExchangeError {
    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "session_invalid".to_owned(),
            message: "A live room session is required.".to_owned(),
        }
    }

    fn capacity() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "ticket_capacity_reached".to_owned(),
            message: "Session ticket capacity is unavailable.".to_owned(),
        }
    }

    fn read_only() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "session_read_only".to_owned(),
            message: "Read-only room sessions cannot change preferences.".to_owned(),
        }
    }

    fn from_body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self {
                status: StatusCode::REQUEST_TIMEOUT,
                code: "request_timeout".to_owned(),
                message: "Request body timed out.".to_owned(),
            },
            BodyDecodeError::PayloadTooLarge => Self {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: "payload_too_large".to_owned(),
                message: "Request body exceeds the route limit.".to_owned(),
            },
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => Self {
                status: StatusCode::BAD_REQUEST,
                code: "body_not_empty".to_owned(),
                message: "Session ticket requests must not contain a body.".to_owned(),
            },
        }
    }

    fn from_leave_body(error: BodyDecodeError) -> Self {
        let (status, code, message) = match error {
            BodyDecodeError::RequestTimeout => (
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout",
                "Request body timed out.",
            ),
            BodyDecodeError::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "Request body exceeds the route limit.",
            ),
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => (
                StatusCode::BAD_REQUEST,
                "invalid_participant_leave",
                "Room leave requires an empty JSON object.",
            ),
        };
        Self {
            status,
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }

    fn from_command(failure: &CommandFailure) -> Self {
        let status = if failure.resolution == CommandResolution::Unresolved {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            match &failure.error {
                PersistenceError::CommandRejected {
                    code: "permission_denied",
                    ..
                } => StatusCode::FORBIDDEN,
                PersistenceError::CommandRejected {
                    code: "session_revoked",
                    ..
                }
                | PersistenceError::ParticipantMissing
                | PersistenceError::RoomMissing => StatusCode::UNAUTHORIZED,
                PersistenceError::CommandRejected {
                    code: "invalid_participant_leave",
                    ..
                } => StatusCode::BAD_REQUEST,
                PersistenceError::CommandRejected { .. }
                | PersistenceError::CommandConflict
                | PersistenceError::StoredCommandRejected { .. } => StatusCode::CONFLICT,
                _ => StatusCode::SERVICE_UNAVAILABLE,
            }
        };
        if persistence_error_is_internal(&failure.error) {
            tracing::error!(error = ?failure.error, "human session leave failed");
        }
        let (code, message) = persistence_error(&failure.error);
        Self {
            status,
            code,
            message,
        }
    }
}

impl From<PersistenceError> for SessionExchangeError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::CommandRejected {
                code: "session_revoked",
                ..
            }
            | PersistenceError::ParticipantMissing
            | PersistenceError::RoomMissing => Self::unauthorized(),
            internal => {
                tracing::error!(error = ?internal, "human session exchange failed");
                Self {
                    status: StatusCode::SERVICE_UNAVAILABLE,
                    code: "session_authority_unavailable".to_owned(),
                    message: "Session authority is unavailable.".to_owned(),
                }
            }
        }
    }
}

impl IntoResponse for SessionExchangeError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": self.message, "code": self.code})),
        )
            .into_response()
    }
}
