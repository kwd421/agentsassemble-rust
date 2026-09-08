use agentsassemble_domain::{SideChatSnapshot, validate_room_id};
use agentsassemble_persistence::PersistenceError;
use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::{Method, StatusCode, header::CACHE_CONTROL},
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

// Keep errors as their status/body until Axum builds the final response.
type Failure = (StatusCode, Json<serde_json::Value>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SideChatQuery {
    room_id: String,
}

pub(crate) fn routes() -> Router<AppState> {
    side_chat_routes()
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::GET]))
}

registered_routes! {
    fn side_chat_routes<AppState>() {
        same_origin_public "/api/side-chat" => get(read_side_chat),
    }
}

async fn read_side_chat(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<SideChatSnapshot>, Failure> {
    let credential = bearer_credential(request.headers()).ok_or_else(unauthorized)?;
    let grant = match resolve_room_session_bearer(
        &state,
        request.headers(),
        request.extensions().get(),
        credential,
    )
    .await
    {
        Ok(RoomSessionBearerResolution::Authorized(authorization)) => {
            RoomSessionHttpAuthority::Session(authorization)
        }
        Ok(RoomSessionBearerResolution::Other) => RoomSessionHttpAuthority::LocalTicket(
            state
                .tickets
                .consume_side_chat_read(credential)
                .await
                .map_err(|_| unauthorized())?,
        ),
        Err(RoomSessionBearerError::Invalid) => return Err(unauthorized()),
        Err(RoomSessionBearerError::Persistence(error)) => return Err(persistence_error(error)),
    };
    let Query(query) = Query::<SideChatQuery>::try_from_uri(request.uri()).map_err(|_| {
        error_response(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "Side chat requires one room_id.",
        )
    })?;
    let room_id = validate_room_id(&query.room_id)
        .map_err(|error| error_response(StatusCode::BAD_REQUEST, error.code, &error.message))?;
    let granted_room = match &grant {
        RoomSessionHttpAuthority::LocalTicket(local) => &local.manager.room_id,
        RoomSessionHttpAuthority::Session(session) => &session.principal().room_id,
    };
    if room_id != *granted_room {
        return Err(unauthorized());
    }
    ensure_empty_body(request, 1024).await.map_err(|error| {
        if matches!(error, BodyDecodeError::PayloadTooLarge) {
            return error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "Side-chat read body exceeds the route limit.",
            );
        }
        if matches!(error, BodyDecodeError::RequestTimeout) {
            return error_response(
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout",
                "Side-chat read body timed out.",
            );
        }
        error_response(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "Side-chat reads require an empty body.",
        )
    })?;
    let now = chrono::Utc::now();
    let snapshot = match &grant {
        RoomSessionHttpAuthority::LocalTicket(local) => {
            state.store.local_side_chat_snapshot(local, now).await
        }
        RoomSessionHttpAuthority::Session(session) => {
            state
                .store
                .side_chat_snapshot(session.mutation_authority(), now)
                .await
        }
    }
    .map_err(persistence_error)?;
    Ok(Json(snapshot))
}

fn unauthorized() -> Failure {
    error_response(
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "Current side-chat read authority is required.",
    )
}

fn persistence_error(error: PersistenceError) -> Failure {
    match error {
        PersistenceError::CommandRejected { code, message }
            if matches!(
                code.as_bytes(),
                b"permission_denied" | b"human_side_chat_required"
            ) =>
        {
            error_response(StatusCode::FORBIDDEN, &code, &message)
        }
        PersistenceError::RoomMissing | PersistenceError::ParticipantMissing => unauthorized(),
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
            unauthorized()
        }
        error => {
            tracing::error!(error = ?error, "side-chat bootstrap failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Side chat could not be read.",
            )
        }
    }
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Failure {
    (
        status,
        Json(json!({"error":{"code":code,"message":message}})),
    )
}
