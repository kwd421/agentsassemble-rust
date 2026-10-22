//! The agent card's "지침" and "불렀을 때만 반응": the owner's own settings for
//! one AI in a room, app AI or external AI. Only the server owner reads or changes them, on the
//! same owner routes as the memory tab.
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

// 4,000 characters of up to 4 UTF-8 bytes each, plus the JSON envelope.
const MAX_BODY_BYTES: usize = 20 * 1024;

pub(crate) fn routes() -> Router<AppState> {
    instruction_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::GET, Method::POST]))
}

registered_routes! {
    fn instruction_routes<AppState>() {
        private "/api/agent-instructions/{room_id}/{participant_id}" => get(read_instructions).post(write_instructions),
        secure_remote "/api/central-owner/agent-instructions/{room_id}/{participant_id}" => get(read_instructions).post(write_instructions),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstructionsBody {
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default)]
    mention_only: Option<bool>,
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

async fn read_instructions(
    State(state): State<AppState>,
    Path((room_id, participant_id)): Path<(String, String)>,
    request: Request,
) -> Result<Json<Value>, Response> {
    let request = authorize(&state, request).await?;
    ensure_empty_body(request, MAX_BODY_BYTES)
        .await
        .map_err(|_| bad_request("The request body is invalid."))?;
    current(&state, &room_id, &participant_id).await
}

async fn write_instructions(
    State(state): State<AppState>,
    Path((room_id, participant_id)): Path<(String, String)>,
    request: Request,
) -> Result<Json<Value>, Response> {
    let request = authorize(&state, request).await?;
    let body: InstructionsBody = decode_json_body(request, MAX_BODY_BYTES)
        .await
        .map_err(|_| bad_request("The request body is invalid."))?;
    if body.instructions.is_none() && body.mention_only.is_none() {
        return Err(bad_request("Nothing to change."));
    }
    match state
        .store
        .set_agent_owner_settings(
            &room_id,
            &participant_id,
            body.instructions.as_deref(),
            body.mention_only,
            chrono::Utc::now(),
        )
        .await
    {
        Ok(true) => {}
        Ok(false) => return Err(missing()),
        Err(agentsassemble_persistence::PersistenceError::CommandRejected { message, .. }) => {
            return Err(bad_request(&message));
        }
        Err(_) => return Err(unavailable()),
    }
    current(&state, &room_id, &participant_id).await
}

async fn current(state: &AppState, room_id: &str, participant_id: &str) -> Result<Json<Value>, Response> {
    let current = state
        .store
        .agent_instructions(room_id, participant_id)
        .await
        .map_err(|_| unavailable())?
        .ok_or_else(missing)?;
    Ok(Json(json!({"instructions": current.instructions, "mention_only": current.mention_only})))
}

fn bad_request(message: &str) -> Response {
    error(StatusCode::BAD_REQUEST, "bad_request", message)
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
