//! The agent card's usage line: an API AI's model-use totals. Only the server
//! owner reads them, on the same owner routes as the memory tab.
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
    usage_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::GET]))
}

registered_routes! {
    fn usage_routes<AppState>() {
        private "/api/agent-usage/{room_id}/{session_id}" => get(read_usage),
        secure_remote "/api/central-owner/agent-usage/{room_id}/{session_id}" => get(read_usage),
    }
}

async fn read_usage(
    State(state): State<AppState>,
    Path((room_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Result<Json<Value>, Response> {
    authorize_server_owner(
        &state,
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
        })?;
    let usage = state
        .store
        .agent_usage(&room_id, &session_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "persistence_failed",
                "Persistence operation failed.",
            )
        })?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                "agent_not_found",
                "The agent is unavailable.",
            )
        })?;
    Ok(Json(json!({"usage": usage})))
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"error": message, "code": code}))).into_response()
}
