use agentsassemble_domain::{RuntimeRestartReceipt, RuntimeRestartRequest, RuntimeRestartStatus};
use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::{Method, StatusCode, header},
};
use serde::Deserialize;
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

use crate::{
    AppState,
    http_api::{self, BodyDecodeError},
    runtime_restart::RuntimeRestartError,
};

pub(crate) fn routes() -> Router<AppState> {
    restart_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            http_api::PRIVATE_NO_STORE.clone(),
        ))
        .layer(http_api::exact_tauri_cors([Method::GET, Method::POST]))
}

registered_routes! {
    fn restart_routes<AppState>() {
        private "/api/runtime/rolling-restart" => get(status).post(restart),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusQuery {
    operation_id: Option<Uuid>,
}

async fn status(
    State(state): State<AppState>,
    Query(query): Query<StatusQuery>,
    request: Request,
) -> Result<Json<RuntimeRestartStatus>, StatusCode> {
    authorize(&state, request.headers()).await?;
    http_api::ensure_empty_body(request, 4096)
        .await
        .map_err(body_error)?;
    state
        .runtime_restart
        .status(&state.store, query.operation_id)
        .await
        .map(Json)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}

async fn restart(
    State(state): State<AppState>,
    request: Request,
) -> Result<(StatusCode, Json<RuntimeRestartReceipt>), StatusCode> {
    authorize(&state, request.headers()).await?;
    let request: RuntimeRestartRequest = http_api::decode_json_body(request, 4096)
        .await
        .map_err(body_error)?;
    state
        .runtime_restart
        .request(&state, request.operation_id)
        .await
        .map(|receipt| (StatusCode::ACCEPTED, Json(receipt)))
        .map_err(|error| match error {
            RuntimeRestartError::Busy
            | RuntimeRestartError::Persistence(
                agentsassemble_persistence::PersistenceError::CommandRejected { .. },
            ) => StatusCode::CONFLICT,
            RuntimeRestartError::Unavailable
            | RuntimeRestartError::Candidate
            | RuntimeRestartError::Persistence(_) => StatusCode::SERVICE_UNAVAILABLE,
        })
}

async fn authorize(state: &AppState, headers: &axum::http::HeaderMap) -> Result<(), StatusCode> {
    http_api::consume_local_operator(state, headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    Ok(())
}

fn body_error(error: BodyDecodeError) -> StatusCode {
    match error {
        BodyDecodeError::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
        BodyDecodeError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => StatusCode::BAD_REQUEST,
    }
}
