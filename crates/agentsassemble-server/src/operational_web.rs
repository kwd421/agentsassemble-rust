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
    }
}

async fn resources(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<LocalResourceStatus>, StatusCode> {
    consume_local_operator(&state, request.headers())
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    ensure_empty_body(request, 4096)
        .await
        .map_err(|error| match error {
            BodyDecodeError::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            BodyDecodeError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => StatusCode::BAD_REQUEST,
        })?;
    state
        .local_resources
        .read()
        .await
        .map(Json)
        .map_err(|()| StatusCode::SERVICE_UNAVAILABLE)
}
