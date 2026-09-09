use agentsassemble_domain::LocalResourceStatus;
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header},
};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, consume_local_operator, ensure_empty_body,
        exact_tauri_cors,
    },
};

pub(crate) fn routes() -> Router<AppState> {
    operation_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::GET]))
}

registered_routes! {
    fn operation_routes<AppState>() {
        private "/api/local-resources" => get(resources),
        private "/api/release-health" => get(health_catalog),
        private "/api/release-health/queue" => get(health_report),
    }
}

async fn resources(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<LocalResourceStatus>, StatusCode> {
    authorize_empty(&state, request).await?;
    state
        .local_resources
        .read()
        .await
        .map(Json)
        .map_err(|()| StatusCode::SERVICE_UNAVAILABLE)
}

async fn health_catalog(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Vec<agentsassemble_domain::ReleaseHealthCheck>>, StatusCode> {
    authorize_empty(&state, request).await?;
    Ok(Json(crate::release_health::catalog()))
}

async fn health_report(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Option<agentsassemble_domain::ReleaseHealthReport>>, StatusCode> {
    authorize_empty(&state, request).await?;
    let root = state
        .runtime_state_root
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    tokio::task::spawn_blocking(move || crate::release_health::read_report(&root))
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .map(Json)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}

async fn authorize_empty(state: &AppState, request: Request) -> Result<(), StatusCode> {
    consume_local_operator(state, request.headers())
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    ensure_empty_body(request, 4096)
        .await
        .map_err(|error| match error {
            BodyDecodeError::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            BodyDecodeError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => StatusCode::BAD_REQUEST,
        })?;
    Ok(())
}
