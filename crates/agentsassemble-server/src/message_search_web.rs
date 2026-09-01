use agentsassemble_domain::validate_room_id;
use agentsassemble_persistence::{LobbyMessageContext, LobbyMessageSearchPage, PersistenceError};
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
        BodyDecodeError, PRIVATE_NO_STORE, bearer_credential, ensure_empty_body, exact_tauri_cors,
    },
    ticket::RoomHumanHttpAuthority,
};

const LOBBY_CHANNEL_ID: &str = "lobby";
const ALL_CHANNEL_ID: &str = "all";
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

#[derive(Serialize)]
struct SearchResponse {
    results: Vec<SearchResult>,
    next_cursor: String,
}

#[derive(Serialize)]
struct SearchResult {
    event_id: String,
    participant_id: String,
    channel_id: &'static str,
    seq: i64,
    created_at: String,
    author: String,
    content: String,
    attachment_filenames: Vec<String>,
}

#[derive(Serialize)]
struct ContextResponse {
    channel_id: &'static str,
    event_id: String,
    events: Vec<agentsassemble_domain::RoomEvent>,
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
) -> Result<Json<SearchResponse>, MessageSearchHttpError> {
    let grant = consume_ticket(&state, request.headers()).await?;
    let query = parse_query::<SearchQuery>(&request)?;
    require_search_scope(&grant, &query.room_id, &query.channel_id)?;
    ensure_empty_body(request, MAX_SEARCH_BODY_BYTES)
        .await
        .map_err(|error| MessageSearchHttpError::from_body(error, "search"))?;
    let page = match &grant {
        RoomHumanHttpAuthority::LocalTicket(local) => {
            state
                .store
                .search_local_lobby_messages(
                    &local.room_id,
                    &local.principal_id,
                    &local.participant_id,
                    &query.q,
                    &query.cursor,
                )
                .await?
        }
        RoomHumanHttpAuthority::HumanSession(authorization) => {
            state
                .store
                .search_human_session_lobby_messages(authorization, &query.q, &query.cursor)
                .await?
        }
    };
    Ok(Json(project_page(page)))
}

async fn message_context(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ContextResponse>, MessageSearchHttpError> {
    let grant = consume_ticket(&state, request.headers()).await?;
    let query = parse_query::<ContextQuery>(&request)?;
    require_context_scope(&grant, &query.room, &query.channel)?;
    ensure_empty_body(request, MAX_SEARCH_BODY_BYTES)
        .await
        .map_err(|error| MessageSearchHttpError::from_body(error, "context"))?;
    let context = match &grant {
        RoomHumanHttpAuthority::LocalTicket(local) => {
            state
                .store
                .local_lobby_message_context(
                    &local.room_id,
                    &local.principal_id,
                    &local.participant_id,
                    &query.event,
                )
                .await?
        }
        RoomHumanHttpAuthority::HumanSession(authorization) => {
            state
                .store
                .human_session_lobby_message_context(authorization, &query.event)
                .await?
        }
    };
    Ok(Json(project_context(context)))
}

async fn consume_ticket(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<RoomHumanHttpAuthority, MessageSearchHttpError> {
    let ticket = bearer_credential(headers).ok_or_else(MessageSearchHttpError::unauthorized)?;
    state
        .tickets
        .consume_message_search_read(ticket)
        .await
        .map_err(|_| MessageSearchHttpError::unauthorized())
}

fn parse_query<T: for<'de> Deserialize<'de>>(
    request: &Request,
) -> Result<T, MessageSearchHttpError> {
    Query::<T>::try_from_uri(request.uri())
        .map(|Query(value)| value)
        .map_err(|_| MessageSearchHttpError::bad_request("Search query parameters are invalid."))
}

fn require_search_scope(
    grant: &RoomHumanHttpAuthority,
    requested_room_id: &str,
    channel_id: &str,
) -> Result<(), MessageSearchHttpError> {
    require_room(grant, requested_room_id)?;
    if !matches!(channel_id, "" | LOBBY_CHANNEL_ID | ALL_CHANNEL_ID) {
        return Err(MessageSearchHttpError::not_found(
            "Text channel search is not available.",
        ));
    }
    Ok(())
}

fn require_context_scope(
    grant: &RoomHumanHttpAuthority,
    requested_room_id: &str,
    channel_id: &str,
) -> Result<(), MessageSearchHttpError> {
    require_room(grant, requested_room_id)?;
    if channel_id == ALL_CHANNEL_ID {
        return Err(MessageSearchHttpError::bad_request(
            "A concrete channel_id is required.",
        ));
    }
    if !matches!(channel_id, "" | LOBBY_CHANNEL_ID) {
        return Err(MessageSearchHttpError::not_found(
            "Text channel search is not available.",
        ));
    }
    Ok(())
}

fn require_room(
    grant: &RoomHumanHttpAuthority,
    requested_room_id: &str,
) -> Result<(), MessageSearchHttpError> {
    let room_id = validate_room_id(requested_room_id)
        .map_err(|error| MessageSearchHttpError::bad_request(error.message))?;
    if room_id != grant_room_id(grant) {
        return Err(MessageSearchHttpError::unauthorized());
    }
    Ok(())
}

fn grant_room_id(grant: &RoomHumanHttpAuthority) -> &str {
    match grant {
        RoomHumanHttpAuthority::LocalTicket(local) => &local.room_id,
        RoomHumanHttpAuthority::HumanSession(authorization) => &authorization.principal().room_id,
    }
}

fn project_page(page: LobbyMessageSearchPage) -> SearchResponse {
    SearchResponse {
        results: page
            .results
            .into_iter()
            .map(|result| SearchResult {
                event_id: result.event_id,
                participant_id: result.participant_id,
                channel_id: LOBBY_CHANNEL_ID,
                seq: result.seq,
                created_at: result.created_at,
                author: result.author,
                content: result.content,
                attachment_filenames: result.attachment_filenames,
            })
            .collect(),
        next_cursor: page.next_cursor,
    }
}

fn project_context(context: LobbyMessageContext) -> ContextResponse {
    ContextResponse {
        channel_id: LOBBY_CHANNEL_ID,
        event_id: context.event_id,
        events: context.events,
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
            message: "A valid one-use message-search ticket is required.".to_owned(),
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
                code: "message_missing",
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
