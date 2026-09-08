use agentsassemble_domain::validate_room_id;
use agentsassemble_persistence::{
    LocalRoomManagerAuthority, PersistenceError, RoomMessageContext, RoomMessageSearchPage,
};
use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::{Method, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, bearer_credential, ensure_empty_body, exact_tauri_cors,
    },
    room_session_http_authority::{
        RoomSessionBearerError, RoomSessionBearerResolution, resolve_room_session_bearer,
    },
    ticket::RoomSessionHttpAuthority,
};

const MAX_SEARCH_BODY_BYTES: usize = 4 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchQuery {
    room_id: String,
    #[serde(default)]
    channel_id: String,
    q: String,
    #[serde(default)]
    cursor: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextQuery {
    #[serde(rename = "room_id")]
    room: String,
    #[serde(default)]
    #[serde(rename = "channel_id")]
    channel: String,
    #[serde(rename = "event_id")]
    event: String,
}

pub(crate) fn routes() -> Router<AppState> {
    search_routes()
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::GET]))
}

registered_routes! {
    fn search_routes<AppState>() {
        same_origin_public "/api/room-search" => get(search_messages),
        same_origin_public "/api/room-search/context" => get(message_context),
    }
}

async fn search_messages(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<RoomMessageSearchPage>, MessageSearchHttpError> {
    let grant =
        resolve_read_authority(&state, request.headers(), request.extensions().get()).await?;
    let query = parse_query::<SearchQuery>(&request)?;
    require_room(&grant, &query.room_id)?;
    ensure_empty_body(request, MAX_SEARCH_BODY_BYTES)
        .await
        .map_err(|error| MessageSearchHttpError::from_body(error, "search"))?;
    let page = match &grant {
        RoomSessionHttpAuthority::LocalTicket(local) => {
            state
                .store
                .search_local_messages(local, &query.channel_id, &query.q, &query.cursor)
                .await?
        }
        RoomSessionHttpAuthority::Session(authorization) => {
            state
                .store
                .search_room_session_messages(
                    authorization,
                    &query.channel_id,
                    &query.q,
                    &query.cursor,
                )
                .await?
        }
    };
    Ok(Json(page))
}

async fn message_context(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<RoomMessageContext>, MessageSearchHttpError> {
    let grant =
        resolve_read_authority(&state, request.headers(), request.extensions().get()).await?;
    let query = parse_query::<ContextQuery>(&request)?;
    require_room(&grant, &query.room)?;
    ensure_empty_body(request, MAX_SEARCH_BODY_BYTES)
        .await
        .map_err(|error| MessageSearchHttpError::from_body(error, "context"))?;
    let context = match &grant {
        RoomSessionHttpAuthority::LocalTicket(local) => {
            state
                .store
                .local_message_context(local, &query.channel, &query.event)
                .await?
        }
        RoomSessionHttpAuthority::Session(authorization) => {
            state
                .store
                .room_session_message_context(authorization, &query.channel, &query.event)
                .await?
        }
    };
    Ok(Json(context))
}

async fn resolve_read_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
) -> Result<RoomSessionHttpAuthority<LocalRoomManagerAuthority>, MessageSearchHttpError> {
    let credential = bearer_credential(headers).ok_or_else(MessageSearchHttpError::unauthorized)?;
    match resolve_room_session_bearer(state, headers, origin, credential).await {
        Ok(RoomSessionBearerResolution::Authorized(authorization)) => {
            Ok(RoomSessionHttpAuthority::Session(authorization))
        }
        Ok(RoomSessionBearerResolution::Other) => state
            .tickets
            .consume_message_search_read(credential)
            .await
            .map(RoomSessionHttpAuthority::LocalTicket)
            .map_err(|_| MessageSearchHttpError::unauthorized()),
        Err(RoomSessionBearerError::Invalid) => Err(MessageSearchHttpError::unauthorized()),
        Err(RoomSessionBearerError::Persistence(error)) => Err(error.into()),
    }
}

fn parse_query<T: for<'de> Deserialize<'de>>(
    request: &Request,
) -> Result<T, MessageSearchHttpError> {
    Query::<T>::try_from_uri(request.uri())
        .map(|Query(value)| value)
        .map_err(|_| MessageSearchHttpError::bad_request("Search query parameters are invalid."))
}

fn require_room(
    grant: &RoomSessionHttpAuthority<LocalRoomManagerAuthority>,
    requested_room_id: &str,
) -> Result<(), MessageSearchHttpError> {
    let room_id = validate_room_id(requested_room_id)
        .map_err(|error| MessageSearchHttpError::bad_request(error.message))?;
    if room_id != grant_room_id(grant) {
        return Err(MessageSearchHttpError::unauthorized());
    }
    Ok(())
}

fn grant_room_id(grant: &RoomSessionHttpAuthority<LocalRoomManagerAuthority>) -> &str {
    match grant {
        RoomSessionHttpAuthority::LocalTicket(local) => &local.manager.room_id,
        RoomSessionHttpAuthority::Session(authorization) => &authorization.principal().room_id,
    }
}

#[derive(Debug)]
struct MessageSearchHttpError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl MessageSearchHttpError {
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
            message: "Valid message-search authority is required.".to_owned(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "permission_denied",
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "message_not_found",
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

    fn from_body(error: BodyDecodeError, route: &str) -> Self {
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
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => {
                Self::bad_request(format!("GET {route} body must be empty."))
            }
        }
    }
}

impl From<PersistenceError> for MessageSearchHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::RoomMissing => Self::not_found("Room does not exist."),
            PersistenceError::ParticipantMissing
            | PersistenceError::CommandRejected {
                code:
                    "session_revoked"
                    | "room_authority_changed"
                    | "room_inactive"
                    | "user_profile_missing"
                    | "profile_authority_mismatch",
                ..
            } => Self::unauthorized(),
            PersistenceError::CommandRejected {
                code: "permission_denied",
                message,
            } => Self::forbidden(message),
            PersistenceError::CommandRejected {
                code: "message_missing" | "channel_not_found" | "channel_unavailable",
                message,
            } => Self::not_found(message),
            PersistenceError::CommandRejected {
                code: "bad_request",
                message,
            } => Self::bad_request(message),
            error => {
                tracing::error!(error = ?error, "message-search HTTP persistence failed");
                Self::internal()
            }
        }
    }
}

impl IntoResponse for MessageSearchHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": {"code": self.code, "message": self.message}})),
        )
            .into_response()
    }
}
