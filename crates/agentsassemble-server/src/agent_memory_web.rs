//! The memory tab: the server owner reads and deletes an API agent's memory
//! pieces, on the same owner routes as the persona library.
use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{PRIVATE_NO_STORE, ensure_empty_body, exact_tauri_cors},
    persona_web::authorize_server_owner,
};

const MAX_EMPTY_BODY_BYTES: usize = 4 * 1024;

pub(crate) fn routes() -> Router<AppState> {
    memory_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::GET, Method::DELETE]))
}

registered_routes! {
    fn memory_routes<AppState>() {
        private "/api/agent-memory/{room_id}/{session_id}" => get(list_memory),
        private "/api/agent-memory/{room_id}/{session_id}/{from_seq}" => delete(delete_memory),
        secure_remote "/api/central-owner/agent-memory/{room_id}/{session_id}" => get(list_memory),
        secure_remote "/api/central-owner/agent-memory/{room_id}/{session_id}/{from_seq}" => delete(delete_memory),
    }
}

async fn admit(state: &AppState, request: Request) -> Result<(), Response> {
    authorize_server_owner(
        state,
        request.headers(),
        request.extensions().get(),
        request.uri().path().starts_with("/api/central-owner/"),
    )
    .await
    .map_err(|(status, code, message)| error(status, code, &message))?;
    ensure_empty_body(request, MAX_EMPTY_BODY_BYTES)
        .await
        .map_err(|_| {
            error(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "This request takes no body.",
            )
        })
}

async fn list_memory(
    State(state): State<AppState>,
    Path((room_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Result<Json<Value>, Response> {
    admit(&state, request).await?;
    let pieces = state
        .store
        .agent_memory(&room_id, &session_id)
        .await
        .map_err(|_| unavailable())?;
    Ok(Json(json!({"pieces": pieces})))
}

async fn delete_memory(
    State(state): State<AppState>,
    Path((room_id, session_id, from_seq)): Path<(String, String, i64)>,
    request: Request,
) -> Result<Json<Value>, Response> {
    admit(&state, request).await?;
    let deleted = state
        .store
        .delete_agent_memory(&room_id, &session_id, from_seq)
        .await
        .map_err(|_| unavailable())?;
    if !deleted {
        return Err(error(
            StatusCode::NOT_FOUND,
            "memory_not_found",
            "The memory piece is unavailable.",
        ));
    }
    Ok(Json(json!({"deleted": true})))
}

fn unavailable() -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "persistence_failed",
        "Persistence operation failed.",
    )
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"error": message, "code": code}))).into_response()
}
