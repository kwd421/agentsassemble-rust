use agentsassemble_persistence::PersistenceError;
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use serde::Serialize;
use serde_json::json;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, bearer_ticket, ensure_empty_body, exact_tauri_cors,
    },
    human_session_bearer::fingerprint_presented_bearer,
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
        "/api/session-tickets/profile" => post(issue_profile_ticket),
        "/api/session-tickets/socket" => post(issue_socket_ticket),
    }
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

async fn authorize_exchange(
    state: &AppState,
    request: Request,
) -> Result<agentsassemble_persistence::HumanSessionAuthorization, SessionExchangeError> {
    let fingerprint = bearer_ticket(request.headers())
        .and_then(fingerprint_presented_bearer)
        .ok_or_else(SessionExchangeError::unauthorized)?;
    ensure_empty_body(request, MAX_EXCHANGE_BODY_BYTES)
        .await
        .map_err(SessionExchangeError::from_body)?;
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
    code: &'static str,
    message: &'static str,
}

impl SessionExchangeError {
    const fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "session_invalid",
            message: "A live room session is required.",
        }
    }

    const fn capacity() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "ticket_capacity_reached",
            message: "Session ticket capacity is unavailable.",
        }
    }

    const fn from_body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self {
                status: StatusCode::REQUEST_TIMEOUT,
                code: "request_timeout",
                message: "Request body timed out.",
            },
            BodyDecodeError::PayloadTooLarge => Self {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: "payload_too_large",
                message: "Request body exceeds the route limit.",
            },
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => Self {
                status: StatusCode::BAD_REQUEST,
                code: "body_not_empty",
                message: "Session ticket requests must not contain a body.",
            },
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
                    code: "session_authority_unavailable",
                    message: "Session authority is unavailable.",
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
