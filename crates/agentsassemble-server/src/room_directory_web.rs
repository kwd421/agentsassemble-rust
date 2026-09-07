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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleRequest {
    server_id: String,
    authority_lineage_id: String,
    request_id: String,
    room_id: String,
    action: agentsassemble_protocol::RoomAction,
    payload: Value,
}

pub(crate) fn routes() -> Router<AppState> {
    directory_routes().layer(exact_tauri_cors([Method::GET, Method::POST]))
}

registered_routes! {
    fn directory_routes<AppState>() {
        private "/api/rooms" => get(list_rooms).post(create_room),
        private "/api/rooms/lifecycle" => post(change_lifecycle),
        same_origin_public "/api/room-session/lifecycle" => post(change_session_lifecycle),
    }
}

async fn change_lifecycle(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, DirectoryHttpError> {
    consume_operator(&state, request.headers()).await?;
    execute_lifecycle(&state, request, None).await
}

async fn change_session_lifecycle(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, DirectoryHttpError> {
    use crate::room_session_http_authority::{
        RoomSessionBearerError, RoomSessionBearerResolution, resolve_room_session_bearer,
    };
    let credential = crate::http_api::bearer_credential(request.headers())
        .ok_or_else(DirectoryHttpError::unauthorized)?;
    let authorization = match resolve_room_session_bearer(
        &state,
        request.headers(),
        request.extensions().get(),
        credential,
    )
    .await
    {
        Ok(RoomSessionBearerResolution::Authorized(authorization))
            if matches!(
                *authorization,
                agentsassemble_persistence::RoomSessionAuthorization::Operator(_)
            ) =>
        {
            authorization
        }
        Err(RoomSessionBearerError::Persistence(error)) => return Err(error.into()),
        _ => return Err(DirectoryHttpError::unauthorized()),
    };
    execute_lifecycle(&state, request, Some(&authorization)).await
}

async fn execute_lifecycle(
    state: &AppState,
    request: Request,
    session: Option<&agentsassemble_persistence::RoomSessionAuthorization>,
) -> Result<Response, DirectoryHttpError> {
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
        LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID,
    };
    use agentsassemble_protocol::RoomAction;
    let body: LifecycleRequest = decode_json_body(request, MAX_DIRECTORY_BODY_BYTES)
        .await
        .map_err(DirectoryHttpError::from_body)?;
    if !matches!(
        body.action,
        RoomAction::RoomClose | RoomAction::RoomArchive | RoomAction::RoomDelete
    ) {
        return Err(DirectoryHttpError::bad_request(
            "This route accepts room lifecycle commands only.",
        ));
    }
    let authority = state.store.local_bootstrap_status().await?;
    if body.server_id != authority.server_id
        || body.authority_lineage_id != authority.authority_lineage_id
    {
        return Err(DirectoryHttpError::bad_request(
            "The requested server authority no longer matches this runtime.",
        ));
    }
    let room_id = validate_room_id(&body.room_id)
        .map_err(|error| DirectoryHttpError::bad_request(error.message))?;
    let execution = if let Some(session) = session {
        if room_id != session.principal().room_id {
            return Err(DirectoryHttpError::bad_request(
                "The paired session belongs to another room.",
            ));
        }
        state
            .rooms
            .execute_room_session(session, body.request_id.clone(), body.action, body.payload)
            .await
    } else {
        let principal = AuthenticatedPrincipal {
            principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            display_name: String::new(),
            room_id,
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: true,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        };
        state
            .rooms
            .execute(
                principal,
                None,
                body.request_id.clone(),
                body.action,
                body.payload,
            )
            .await
    };
    let outcome = match execution {
        Ok(outcome) => outcome,
        Err(failure) => {
            let mut error = DirectoryHttpError::from(failure.error);
            if session.is_some() && error.code == "room_deletion_pending" {
                "Deletion was accepted and this paired session has ended. Check the native host for completion.".clone_into(&mut error.message);
            }
            return Ok((
                error.status,
                Json(json!({
                    "error": error.message, "code": error.code, "resolution": failure.resolution,
                    "request_id": body.request_id, "action": body.action,
                })),
            )
                .into_response());
        }
    };
    Ok(Json(json!({
        "server_id": authority.server_id, "authority_lineage_id": authority.authority_lineage_id,
        "request_id": body.request_id, "action": body.action, "resolution": "committed",
        "result": outcome.result, "deduplicated": outcome.deduplicated,
    }))
    .into_response())
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
    if consume_local_operator(state, headers).await.is_none() {
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
    payload["cleanup_pending"] = json!(room.cleanup_pending);
    payload["deletion_pending"] = json!(room.deletion_pending);
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
            message: "Valid request authority is required.".to_owned(),
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
            PersistenceError::CommandConflict => Self {
                status: StatusCode::CONFLICT,
                code: "command_conflict",
                message: "The request id was already used with a different command.".to_owned(),
            },
            PersistenceError::CommandUnresolved { code, message } => Self {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code,
                message,
            },
            PersistenceError::CommandRejected { code, message } => {
                let status = match code {
                    "invalid_state"
                    | "room_already_exists"
                    | "room_create_request_conflict"
                    | "room_incarnation_changed"
                    | "room_closed"
                    | "runtime_cleanup_pending" => StatusCode::CONFLICT,
                    "permission_denied" | "session_revoked" => StatusCode::FORBIDDEN,
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
