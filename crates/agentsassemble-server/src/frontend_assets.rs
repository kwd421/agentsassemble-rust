use axum::{
    Router,
    extract::{Path, Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

use crate::AppState;

pub(crate) fn routes() -> Router<AppState> {
    asset_routes().layer(SetResponseHeaderLayer::overriding(
        header::CACHE_CONTROL,
        crate::web::STATIC_FRONTEND_CACHE_CONTROL,
    ))
}

registered_routes! {
    fn asset_routes<AppState>() {
        same_origin_public "/frontend-builds/{build_id}/assets/{*path}" => get(asset),
    }
}

async fn asset(
    State(state): State<AppState>,
    Path((build_id, _)): Path<(String, String)>,
    request: Request,
) -> Result<Response, StatusCode> {
    if !crate::frontend_release::is_build_id(&build_id) {
        return Err(StatusCode::NOT_FOUND);
    }
    let frontend = state.frontend.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    let releases = frontend.root().parent().ok_or(StatusCode::NOT_FOUND)?;
    let request =
        crate::web::strip_static_prefix(request, &format!("/frontend-builds/{build_id}/assets"))?;
    ServeDir::new(releases.join(build_id).join("assets"))
        .try_call(request)
        .await
        .map(IntoResponse::into_response)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}
