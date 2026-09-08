use agentsassemble_domain::validate_room_id;
use agentsassemble_persistence::{LocalRoomManagerAuthority, PersistenceError, PinnedMessage};
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
        BodyDecodeError, PRIVATE_NO_STORE, bearer_credential, decode_json_body, ensure_empty_body,
        exact_tauri_cors,
    },
    room_session_http_authority::{
        RoomSessionBearerError, RoomSessionBearerResolution, resolve_room_session_bearer,
    },
    ticket::RoomSessionHttpAuthority,
};

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
    pins: Vec<PinnedMessage>,
}

#[derive(Serialize)]
struct PinMutationResponse {
    pinned: bool,
    pins: Vec<PinnedMessage>,
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
    let grant =
        resolve_read_authority(&state, request.headers(), request.extensions().get()).await?;
    let room_id = require_room_request(&grant, &query.room_id)?;
    ensure_empty_body(request, MAX_MESSAGE_PIN_BODY_BYTES)
        .await
        .map_err(MessagePinsHttpError::from_body)?;
    let pins = match &grant {
        RoomSessionHttpAuthority::LocalTicket(grant) => {
            state
                .store
                .local_message_pins(grant, &query.channel_id)
                .await?
        }
        RoomSessionHttpAuthority::Session(authorization) => {
            state
                .store
                .room_session_message_pins(authorization, &query.channel_id)
                .await?
        }
    };
    debug_assert_eq!(room_id, grant_room_id(&grant));
    Ok(Json(PinListResponse { pins }))
}

async fn set_pin(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<PinMutationResponse>, MessagePinsHttpError> {
    let grant =
        resolve_write_authority(&state, request.headers(), request.extensions().get()).await?;
    reauthorize_write(&state, &grant).await?;
    let payload: PinMutation = decode_json_body(request, MAX_MESSAGE_PIN_BODY_BYTES)
        .await
        .map_err(MessagePinsHttpError::from_body)?;
    require_room_request(&grant, &payload.room_id)?;
    let pins = match &grant {
        RoomSessionHttpAuthority::LocalTicket(grant) => {
            state
                .store
                .set_local_message_pin(
                    grant,
                    &payload.channel_id,
                    &payload.event_id,
                    payload.pinned,
                )
                .await?
        }
        RoomSessionHttpAuthority::Session(authorization) => {
            state
                .store
                .set_room_session_message_pin(
                    authorization,
                    &payload.channel_id,
                    &payload.event_id,
                    payload.pinned,
                )
                .await?
        }
    };
    Ok(Json(PinMutationResponse {
        pinned: payload.pinned,
        pins,
    }))
}

async fn resolve_read_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
) -> Result<RoomSessionHttpAuthority<LocalRoomManagerAuthority>, MessagePinsHttpError> {
    let credential = bearer_credential(headers).ok_or_else(MessagePinsHttpError::unauthorized)?;
    match resolve_room_session_bearer(state, headers, origin, credential).await {
        Ok(RoomSessionBearerResolution::Authorized(authorization)) => {
            Ok(RoomSessionHttpAuthority::Session(authorization))
        }
        Ok(RoomSessionBearerResolution::Other) => state
            .tickets
            .consume_message_pins_read(credential)
            .await
            .map(RoomSessionHttpAuthority::LocalTicket)
            .map_err(|_| MessagePinsHttpError::unauthorized()),
        Err(RoomSessionBearerError::Invalid) => Err(MessagePinsHttpError::unauthorized()),
        Err(RoomSessionBearerError::Persistence(error)) => Err(error.into()),
    }
}

async fn resolve_write_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
) -> Result<RoomSessionHttpAuthority<LocalRoomManagerAuthority>, MessagePinsHttpError> {
    let credential = bearer_credential(headers).ok_or_else(MessagePinsHttpError::unauthorized)?;
    match resolve_room_session_bearer(state, headers, origin, credential).await {
        Ok(RoomSessionBearerResolution::Authorized(authorization)) => {
            Ok(RoomSessionHttpAuthority::Session(authorization))
        }
        Ok(RoomSessionBearerResolution::Other) => state
            .tickets
            .consume_message_pins_write(credential)
            .await
            .map(RoomSessionHttpAuthority::LocalTicket)
            .map_err(|_| MessagePinsHttpError::unauthorized()),
        Err(RoomSessionBearerError::Invalid) => Err(MessagePinsHttpError::unauthorized()),
        Err(RoomSessionBearerError::Persistence(error)) => Err(error.into()),
    }
}

async fn reauthorize_write(
    state: &AppState,
    grant: &RoomSessionHttpAuthority<LocalRoomManagerAuthority>,
) -> Result<(), MessagePinsHttpError> {
    match grant {
        RoomSessionHttpAuthority::LocalTicket(grant) => {
            state
                .store
                .authorize_local_room_manager(
                    &grant.manager.room_id,
                    &grant.manager.user_id,
                    &grant.manager.participant_id,
                )
                .await?;
        }
        RoomSessionHttpAuthority::Session(authorization) => {
            let current = state
                .store
                .revalidate_room_session_authorization(authorization)
                .await?;
            if !current.principal().capabilities.message_modify {
                return Err(MessagePinsHttpError::forbidden());
            }
        }
    }
    Ok(())
}

fn require_room_request<'a>(
    grant: &'a RoomSessionHttpAuthority<LocalRoomManagerAuthority>,
    requested_room_id: &str,
) -> Result<&'a str, MessagePinsHttpError> {
    let room_id = validate_room_id(requested_room_id)
        .map_err(|error| MessagePinsHttpError::bad_request(error.message))?;
    if room_id != grant_room_id(grant) {
        return Err(MessagePinsHttpError::unauthorized());
    }
    Ok(grant_room_id(grant))
}

fn grant_room_id(grant: &RoomSessionHttpAuthority<LocalRoomManagerAuthority>) -> &str {
    match grant {
        RoomSessionHttpAuthority::LocalTicket(grant) => &grant.manager.room_id,
        RoomSessionHttpAuthority::Session(authorization) => &authorization.principal().room_id,
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
            PersistenceError::ParticipantMissing => Self::unauthorized(),
            PersistenceError::CommandRejected { code, .. }
                if matches!(
                    code.as_bytes(),
                    b"session_revoked"
                        | b"room_authority_changed"
                        | b"room_inactive"
                        | b"user_profile_missing"
                        | b"profile_authority_mismatch"
                ) =>
            {
                Self::unauthorized()
            }
            PersistenceError::CommandRejected { code, .. }
                if matches!(code.as_bytes(), b"permission_denied") =>
            {
                Self::forbidden()
            }
            PersistenceError::CommandRejected { code, message }
                if matches!(
                    code.as_bytes(),
                    b"message_missing" | b"channel_not_found" | b"channel_unavailable"
                ) =>
            {
                Self::not_found(message)
            }
            PersistenceError::CommandRejected { code, message }
                if matches!(code.as_bytes(), b"bad_request") =>
            {
                Self::bad_request(message)
            }
            PersistenceError::CommandRejected { code, message }
                if matches!(code.as_bytes(), b"pin_limit_reached") =>
            {
                Self::conflict("pin_limit_reached", message)
            }
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
