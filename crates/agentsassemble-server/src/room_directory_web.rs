use agentsassemble_domain::{RoomStatus, clean_single_line, public_settings, validate_room_id};
use agentsassemble_persistence::{LocalBootstrapPhase, PersistenceError, StoredRoomSummary};
use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, consume_local_operator, decode_json_body, ensure_empty_body,
        exact_tauri_cors,
    },
};

const MAX_DIRECTORY_BODY_BYTES: usize = 8 * 1024;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryQuery {
    #[serde(default)]
    include_archived: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRoomRequest {
    request_id: String,
    room_id: String,
    #[serde(default)]
    label: String,
}

pub(crate) fn routes() -> Router<AppState> {
    directory_routes().layer(exact_tauri_cors([Method::GET, Method::POST]))
}

registered_routes! {
    fn directory_routes<AppState>() {
        "/api/rooms" => get(list_rooms).post(create_room),
    }
}

async fn list_rooms(
    State(state): State<AppState>,
    Query(query): Query<DirectoryQuery>,
    request: Request,
) -> Result<Json<Value>, DirectoryHttpError> {
    consume_operator(&state, request.headers()).await?;
    ensure_empty_body(request, MAX_DIRECTORY_BODY_BYTES)
        .await
        .map_err(DirectoryHttpError::from_body)?;
    let include_archived = matches!(
        query.include_archived.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    );
    let bootstrap = state.store.local_bootstrap_status().await?;
    if bootstrap.phase != LocalBootstrapPhase::Complete {
        return Err(DirectoryHttpError::authority_unavailable());
    }
    let rooms = state.store.list_room_directory(include_archived).await?;
    let rooms = rooms
        .iter()
        .map(|room| room_payload(room, "agent_session"))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({
        "server_id": bootstrap.server_id,
        "authority_lineage_id": bootstrap.authority_lineage_id,
        "server_product_surface": state.server_product_surface,
        "rooms": rooms,
    })))
}

async fn create_room(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, DirectoryHttpError> {
    consume_operator(&state, request.headers()).await?;
    let payload: CreateRoomRequest = decode_json_body(request, MAX_DIRECTORY_BODY_BYTES)
        .await
        .map_err(DirectoryHttpError::from_body)?;
    let room_id = validate_room_id(&payload.room_id)
        .map_err(|error| DirectoryHttpError::bad_request(error.message))?;
    let label = clean_single_line(&payload.label, 128);
    let label = if label.is_empty() {
        room_id.as_str()
    } else {
        label.as_str()
    };
    let commit = state
        .store
        .create_room_for_local_operator(&payload.request_id, &room_id, label)
        .await?;
    state.rooms.notify_committed_events(&commit.events).await;
    let room = room_identity_payload(&commit.room, &commit.settings, "frontend_room");
    Ok(Json(json!({
        "status": "ready",
        "server_id": commit.server_id,
        "authority_lineage_id": commit.authority_lineage_id,
        "room": room,
        "deduplicated": commit.deduplicated,
    })))
}

async fn consume_operator(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(), DirectoryHttpError> {
    if !consume_local_operator(state, headers).await {
        return Err(DirectoryHttpError::unauthorized());
    }
    Ok(())
}

fn room_payload(room: &StoredRoomSummary, origin: &str) -> Result<Value, DirectoryHttpError> {
    let mut settings = serde_json::to_value(public_settings(&room.settings)?)?;
    settings
        .as_object_mut()
        .ok_or_else(DirectoryHttpError::internal)?
        .insert(
            "room_id".to_owned(),
            Value::String(room.room.room_id.clone()),
        );
    let mut payload = room_identity_payload(&room.room, &room.settings, origin);
    payload
        .as_object_mut()
        .ok_or_else(DirectoryHttpError::internal)?
        .insert("room_settings".to_owned(), settings);
    Ok(payload)
}

fn room_identity_payload(
    room: &agentsassemble_domain::Room,
    settings: &agentsassemble_domain::RoomSettings,
    origin: &str,
) -> Value {
    json!({
        "room_id": room.room_id,
        "room_uid": room.room_uid,
        "label": settings.label,
        "last_active_at": room.updated_at,
        "archived": room.status == RoomStatus::Archived,
        "status": room_status(room.status),
        "origin": origin,
    })
}

const fn room_status(status: RoomStatus) -> &'static str {
    match status {
        RoomStatus::Active => "active",
        RoomStatus::Closed => "closed",
        RoomStatus::Archived => "archived",
    }
}

#[derive(Debug)]
struct DirectoryHttpError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl DirectoryHttpError {
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
            message: "A valid one-use server-operator ticket is required.".to_owned(),
        }
    }

    fn from_body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::PayloadTooLarge => Self {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: "payload_too_large",
                message: "Request body exceeds the route limit.".to_owned(),
            },
            BodyDecodeError::InvalidJson => Self::bad_request("Request JSON is invalid."),
            BodyDecodeError::NonEmpty => {
                Self::bad_request("GET room-directory requests must not contain a body.")
            }
        }
    }

    fn internal() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "persistence_failed",
            message: "Persistence operation failed.".to_owned(),
        }
    }

    fn authority_unavailable() -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "bootstrap_required",
            message: "Local authority is not complete.".to_owned(),
        }
    }
}

impl From<PersistenceError> for DirectoryHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::CommandRejected { code, message } => {
                let status = match code {
                    "invalid_state" | "room_already_exists" | "room_create_request_conflict" => {
                        StatusCode::CONFLICT
                    }
                    _ => StatusCode::BAD_REQUEST,
                };
                Self {
                    status,
                    code,
                    message,
                }
            }
            internal => {
                tracing::error!(error = ?internal, "room directory persistence operation failed");
                Self::internal()
            }
        }
    }
}

impl From<serde_json::Error> for DirectoryHttpError {
    fn from(_: serde_json::Error) -> Self {
        Self::internal()
    }
}

impl IntoResponse for DirectoryHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": self.message, "code": self.code})),
        )
            .into_response()
    }
}
