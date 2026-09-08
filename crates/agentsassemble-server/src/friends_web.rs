use agentsassemble_domain::SaveFriend;
use agentsassemble_persistence::PersistenceError;
use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, consume_local_operator, decode_json_body,
        ensure_empty_body, exact_tauri_cors,
    },
};

const MAX_FRIEND_BODY_BYTES: usize = 16 * 1024;

pub(crate) fn routes() -> Router<AppState> {
    friend_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([
            Method::GET,
            Method::POST,
            Method::DELETE,
        ]))
}

registered_routes! {
    fn friend_routes<AppState>() {
        private "/api/room-friends" => get(list).post(save).delete(delete),
    }
}

async fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    if consume_local_operator(state, headers).await.is_none() {
        return Err(failure(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "A local operator ticket is required.",
        ));
    }
    Ok(())
}

async fn list(State(state): State<AppState>, request: Request) -> Result<Response, Response> {
    authorize(&state, request.headers()).await?;
    ensure_empty_body(request, MAX_FRIEND_BODY_BYTES)
        .await
        .map_err(body_error)?;
    let friends = state.store.saved_friends().await.map_err(storage_error)?;
    Ok(Json(json!({"friends": friends})).into_response())
}

async fn save(State(state): State<AppState>, request: Request) -> Result<Response, Response> {
    authorize(&state, request.headers()).await?;
    let body: SaveFriend = decode_json_body(request, MAX_FRIEND_BODY_BYTES)
        .await
        .map_err(body_error)?;
    let friend = state
        .store
        .save_friend(&body)
        .await
        .map_err(storage_error)?;
    Ok(Json(friend).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteQuery {
    friend_id: Uuid,
}

async fn delete(
    State(state): State<AppState>,
    Query(query): Query<DeleteQuery>,
    request: Request,
) -> Result<Response, Response> {
    authorize(&state, request.headers()).await?;
    ensure_empty_body(request, MAX_FRIEND_BODY_BYTES)
        .await
        .map_err(body_error)?;
    let deleted = state
        .store
        .delete_friend(query.friend_id)
        .await
        .map_err(storage_error)?;
    Ok(Json(json!({"deleted": deleted})).into_response())
}

fn storage_error(error: PersistenceError) -> Response {
    match error {
        PersistenceError::CommandRejected { code, message } => {
            let status = match code.as_bytes() {
                b"friend_invalid" => StatusCode::BAD_REQUEST,
                b"friend_conflict" => StatusCode::CONFLICT,
                _ => StatusCode::SERVICE_UNAVAILABLE,
            };
            failure(status, &code, &message)
        }
        _ => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "persistence_failed",
            "Contact storage is unavailable.",
        ),
    }
}

fn body_error(error: BodyDecodeError) -> Response {
    let (status, code, message) = match error {
        BodyDecodeError::RequestTimeout => (
            StatusCode::REQUEST_TIMEOUT,
            "request_timeout",
            "Request body timed out.",
        ),
        BodyDecodeError::PayloadTooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "Contact request exceeds the body limit.",
        ),
        BodyDecodeError::InvalidJson => (
            StatusCode::BAD_REQUEST,
            "bad_request",
            "Contact JSON is invalid.",
        ),
        BodyDecodeError::NonEmpty => (
            StatusCode::BAD_REQUEST,
            "bad_request",
            "This request must not contain a body.",
        ),
    };
    failure(status, code, message)
}

fn failure(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code": code, "error": message}))).into_response()
}
