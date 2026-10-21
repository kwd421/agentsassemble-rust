//! The agent card's "이 사람의 AI가 넘긴 일은 묻지 않기": the people whose AIs
//! this agent trusts. Only the server owner reads or changes it, on the same
//! owner routes as the memory tab.
use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{PRIVATE_NO_STORE, decode_json_body, ensure_empty_body, exact_tauri_cors},
    persona_web::authorize_server_owner,
};

const MAX_BODY_BYTES: usize = 4 * 1024;

pub(crate) fn routes() -> Router<AppState> {
    trust_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::GET, Method::POST]))
}

registered_routes! {
    fn trust_routes<AppState>() {
        private "/api/agent-handoff-trust/{room_id}/{session_id}" => get(read_trust).post(write_trust),
        secure_remote "/api/central-owner/agent-handoff-trust/{room_id}/{session_id}" => get(read_trust).post(write_trust),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustBody {
    trusted_owners: Vec<String>,
}

/// Checks the owner on the request head, then hands the request back whole.
async fn authorize(state: &AppState, request: Request) -> Result<Request, Response> {
    let (parts, body) = request.into_parts();
    authorize_server_owner(
        state,
        &parts.headers,
        parts.extensions.get(),
        parts.uri.path().starts_with("/api/central-owner/"),
    )
    .await
    .map_err(|(status, code, message)| error(status, code, &message))?;
    Ok(Request::from_parts(parts, body))
}

async fn read_trust(
    State(state): State<AppState>,
    Path((room_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Result<Json<Value>, Response> {
    let request = authorize(&state, request).await?;
    ensure_empty_body(request, MAX_BODY_BYTES)
        .await
        .map_err(|_| bad_request())?;
    let trusted = state
        .store
        .agent_trusted_ai_owners(&room_id, &session_id)
        .await
        .map_err(|_| unavailable())?
        .ok_or_else(missing)?;
    Ok(Json(json!({"trusted_owners": trusted})))
}

async fn write_trust(
    State(state): State<AppState>,
    Path((room_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Result<Json<Value>, Response> {
    let request = authorize(&state, request).await?;
    let body: TrustBody = decode_json_body(request, MAX_BODY_BYTES)
        .await
        .map_err(|_| bad_request())?;
    match state
        .store
        .set_agent_trusted_ai_owners(&room_id, &session_id, &body.trusted_owners)
        .await
    {
        Ok(true) => {}
        Ok(false) => return Err(missing()),
        Err(agentsassemble_persistence::PersistenceError::CommandRejected { message, .. }) => {
            return Err(error(StatusCode::BAD_REQUEST, "bad_request", &message));
        }
        Err(_) => return Err(unavailable()),
    }
    let trusted = state
        .store
        .agent_trusted_ai_owners(&room_id, &session_id)
        .await
        .map_err(|_| unavailable())?
        .ok_or_else(missing)?;
    Ok(Json(json!({"trusted_owners": trusted})))
}

fn bad_request() -> Response {
    error(
        StatusCode::BAD_REQUEST,
        "bad_request",
        "The request body is invalid.",
    )
}

fn missing() -> Response {
    error(
        StatusCode::NOT_FOUND,
        "agent_not_found",
        "The agent is unavailable.",
    )
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
