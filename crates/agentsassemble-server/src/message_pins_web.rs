use agentsassemble_domain::validate_room_id;
use agentsassemble_persistence::{PersistenceError, PinnedLobbyMessage};
use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::{Method, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, bearer_ticket, decode_json_body, ensure_empty_body,
        exact_tauri_cors,
    },
    human_session_http_authority::{
        HumanSessionBearerError, HumanSessionBearerResolution, resolve_human_session_bearer,
    },
    ticket::RoomHumanHttpAuthority,
};

const LOBBY_CHANNEL_ID: &str = "lobby";
const MAX_MESSAGE_PIN_BODY_BYTES: usize = 4 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PinListQuery {
    room_id: String,
    channel_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PinMutation {
    room_id: String,
    channel_id: String,
    event_id: String,
    pinned: bool,
}

#[derive(Serialize)]
struct PinListResponse {
    pins: Vec<PinProjection>,
}

#[derive(Serialize)]
struct PinMutationResponse {
    pinned: bool,
    pins: Vec<PinProjection>,
}

#[derive(Serialize)]
struct PinProjection {
    event_id: String,
    channel_id: &'static str,
    pinned_at: String,
    seq: i64,
    author: String,
    content: String,
    created_at: String,
    attachment_filenames: Vec<String>,
}

impl From<PinnedLobbyMessage> for PinProjection {
    fn from(pin: PinnedLobbyMessage) -> Self {
        Self {
            event_id: pin.event_id,
            channel_id: LOBBY_CHANNEL_ID,
            pinned_at: pin.pinned_at,
            seq: pin.seq,
            author: pin.author,
            content: pin.content,
            created_at: pin.created_at,
            attachment_filenames: pin.attachment_filenames,
        }
    }
}

pub(crate) fn routes() -> Router<AppState> {
    pin_routes()
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::GET, Method::POST]))
}

registered_routes! {
    fn pin_routes<AppState>() {
        same_origin_public "/api/room-pins" => get(list_pins).post(set_pin),
    }
}

async fn list_pins(
    State(state): State<AppState>,
    Query(query): Query<PinListQuery>,
    request: Request,
) -> Result<Json<PinListResponse>, MessagePinsHttpError> {
    let grant = resolve_read_authority(&state, request.headers()).await?;
    let room_id = require_lobby_request(&grant, &query.room_id, &query.channel_id)?;
    ensure_empty_body(request, MAX_MESSAGE_PIN_BODY_BYTES)
        .await
        .map_err(MessagePinsHttpError::from_body)?;
    let pins = match &grant {
        RoomHumanHttpAuthority::LocalTicket(grant) => {
            state
                .store
                .local_lobby_message_pins(
                    &grant.room_id,
                    &grant.principal_id,
                    &grant.participant_id,
                )
                .await?
        }
        RoomHumanHttpAuthority::HumanSession(authorization) => {
            state
                .store
                .human_session_lobby_message_pins(authorization)
                .await?
        }
    };
    debug_assert_eq!(room_id, grant_room_id(&grant));
    Ok(Json(PinListResponse {
        pins: pins.into_iter().map(Into::into).collect(),
    }))
}

async fn set_pin(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<PinMutationResponse>, MessagePinsHttpError> {
    let grant = resolve_write_authority(&state, request.headers()).await?;
    reauthorize_write(&state, &grant).await?;
    let payload: PinMutation = decode_json_body(request, MAX_MESSAGE_PIN_BODY_BYTES)
        .await
        .map_err(MessagePinsHttpError::from_body)?;
    require_lobby_request(&grant, &payload.room_id, &payload.channel_id)?;
    let pins = match &grant {
        RoomHumanHttpAuthority::LocalTicket(grant) => {
            state
                .store
                .set_local_lobby_message_pin(
                    &grant.room_id,
                    &grant.principal_id,
                    &grant.participant_id,
                    &payload.event_id,
                    payload.pinned,
                )
                .await?
        }
        RoomHumanHttpAuthority::HumanSession(authorization) => {
            state
                .store
                .set_human_session_lobby_message_pin(
                    authorization,
                    &payload.event_id,
                    payload.pinned,
                )
                .await?
        }
    };
    Ok(Json(PinMutationResponse {
        pinned: payload.pinned,
        pins: pins.into_iter().map(Into::into).collect(),
    }))
}

async fn resolve_read_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<RoomHumanHttpAuthority, MessagePinsHttpError> {
    let ticket = bearer_ticket(headers).ok_or_else(MessagePinsHttpError::unauthorized)?;
    match resolve_human_session_bearer(state, ticket).await {
        Ok(HumanSessionBearerResolution::Authorized(authorization)) => {
            Ok(RoomHumanHttpAuthority::HumanSession(authorization))
        }
        Ok(HumanSessionBearerResolution::Other) => state
            .tickets
            .consume_message_pins_read(ticket)
            .await
            .map(RoomHumanHttpAuthority::LocalTicket)
            .map_err(|_| MessagePinsHttpError::unauthorized()),
        Err(HumanSessionBearerError::Invalid) => Err(MessagePinsHttpError::unauthorized()),
        Err(HumanSessionBearerError::Persistence(error)) => Err(error.into()),
    }
}

async fn resolve_write_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<RoomHumanHttpAuthority, MessagePinsHttpError> {
    let ticket = bearer_ticket(headers).ok_or_else(MessagePinsHttpError::unauthorized)?;
    match resolve_human_session_bearer(state, ticket).await {
        Ok(HumanSessionBearerResolution::Authorized(authorization)) => {
            Ok(RoomHumanHttpAuthority::HumanSession(authorization))
        }
        Ok(HumanSessionBearerResolution::Other) => state
            .tickets
            .consume_message_pins_write(ticket)
            .await
            .map(RoomHumanHttpAuthority::LocalTicket)
            .map_err(|_| MessagePinsHttpError::unauthorized()),
        Err(HumanSessionBearerError::Invalid) => Err(MessagePinsHttpError::unauthorized()),
        Err(HumanSessionBearerError::Persistence(error)) => Err(error.into()),
    }
}

async fn reauthorize_write(
    state: &AppState,
    grant: &RoomHumanHttpAuthority,
) -> Result<(), MessagePinsHttpError> {
    match grant {
        RoomHumanHttpAuthority::LocalTicket(grant) => {
            state
                .store
                .authorize_local_room_manager(
                    &grant.room_id,
                    &grant.principal_id,
                    &grant.participant_id,
                )
                .await?;
        }
        RoomHumanHttpAuthority::HumanSession(authorization) => {
            let current = state
                .store
                .revalidate_human_session_authorization(authorization)
                .await?;
            if !current.principal().capabilities.message_modify {
                return Err(MessagePinsHttpError::forbidden());
            }
        }
    }
    Ok(())
}

fn require_lobby_request<'a>(
    grant: &'a RoomHumanHttpAuthority,
    requested_room_id: &str,
    channel_id: &str,
) -> Result<&'a str, MessagePinsHttpError> {
    let room_id = validate_room_id(requested_room_id)
        .map_err(|error| MessagePinsHttpError::bad_request(error.message))?;
    if room_id != grant_room_id(grant) {
        return Err(MessagePinsHttpError::unauthorized());
    }
    if channel_id != LOBBY_CHANNEL_ID {
        return Err(MessagePinsHttpError::not_found(
            "Only the lobby message stream is available.",
        ));
    }
    Ok(grant_room_id(grant))
}

fn grant_room_id(grant: &RoomHumanHttpAuthority) -> &str {
    match grant {
        RoomHumanHttpAuthority::LocalTicket(grant) => &grant.room_id,
        RoomHumanHttpAuthority::HumanSession(authorization) => &authorization.principal().room_id,
    }
}

#[derive(Debug)]
struct MessagePinsHttpError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl MessagePinsHttpError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: message.into(),
        }
    }

    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "Valid message-pin authority is required.".to_owned(),
        }
    }

    fn forbidden() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "permission_denied",
            message: "This room session cannot modify messages.".to_owned(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "message_not_found",
            message: message.into(),
        }
    }

    fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code,
            message: message.into(),
        }
    }

    fn internal() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "persistence_failed",
            message: "Persistence operation failed.".to_owned(),
        }
    }

    fn from_body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self {
                status: StatusCode::REQUEST_TIMEOUT,
                code: "request_timeout",
                message: "Request body timed out.".to_owned(),
            },
            BodyDecodeError::PayloadTooLarge => Self {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: "payload_too_large",
                message: "Request body exceeds the route limit.".to_owned(),
            },
            BodyDecodeError::InvalidJson => Self::bad_request("Request JSON is invalid."),
            BodyDecodeError::NonEmpty => {
                Self::bad_request("GET room-pins requests must not contain a body.")
            }
        }
    }
}

impl From<PersistenceError> for MessagePinsHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::RoomMissing => Self::not_found("Room does not exist."),
            PersistenceError::ParticipantMissing
            | PersistenceError::CommandRejected {
                code:
                    "session_revoked"
                    | "room_inactive"
                    | "user_profile_missing"
                    | "profile_authority_mismatch",
                ..
            } => Self::unauthorized(),
            PersistenceError::CommandRejected {
                code: "permission_denied",
                ..
            } => Self::forbidden(),
            PersistenceError::CommandRejected {
                code: "message_missing",
                message,
            } => Self::not_found(message),
            PersistenceError::CommandRejected {
                code: "bad_request",
                message,
            } => Self::bad_request(message),
            PersistenceError::CommandRejected {
                code: "pin_limit_reached",
                message,
            } => Self::conflict("pin_limit_reached", message),
            error => {
                tracing::error!(error = ?error, "message-pin HTTP persistence failed");
                Self::internal()
            }
        }
    }
}

impl IntoResponse for MessagePinsHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": {"code": self.code, "message": self.message}})),
        )
            .into_response()
    }
}
