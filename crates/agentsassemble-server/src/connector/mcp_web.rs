use std::sync::Arc;

use axum::{
    Extension, Router,
    body::Body,
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState, connector::mcp::ConnectorMcp, http_api::PRIVATE_NO_STORE,
    ingress_trust::single_header,
};

registered_routes! {
    fn mcp_routes<AppState>() {
        same_origin_public "/mcp" => post(handle).get(handle).delete(handle),
    }
}

pub(crate) fn routes(state: &AppState) -> Router<AppState> {
    mcp_routes()
        .layer(Extension(ConnectorMcp::hosted(
            state.public_ingress(),
            state.store.clone(),
        )))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
}

async fn handle(
    State(state): State<AppState>,
    Extension(connector): Extension<ConnectorMcp>,
    request: Request,
) -> Response {
    // This registered route is behind require_trusted_ingress. That owner has
    // already proved the exact Host, Origin, peer and current proxy generation.
    // Derive RMCP's per-request transport checks from that proof; keep the shared
    // private-handle registry across stateless requests and ingress generations.
    let Some(host) = single_header(request.headers(), header::HOST) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let origins = single_header(request.headers(), header::ORIGIN)
        .map(str::to_owned)
        .into_iter();
    let service = StreamableHttpService::new(
        move || Ok(connector.clone()),
        Arc::<LocalSessionManager>::default(),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_sse_keep_alive(None)
            .with_allowed_hosts([host.to_owned()])
            .with_allowed_origins(origins)
            .with_max_request_body_bytes(crate::http_api::MAX_BASE64_UPLOAD_BODY_BYTES)
            .with_cancellation_token(state.shutdown.child_token()),
    );
    service.handle(request).await.map(Body::new)
}
