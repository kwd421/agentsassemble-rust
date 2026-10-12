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
        .layer(Extension(ConnectorMcp::hosted(state.clone())))
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
    let host = host.to_owned();
    let origins = single_header(request.headers(), header::ORIGIN)
        .map(str::to_owned)
        .into_iter();
    serve_request(
        connector,
        request,
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_sse_keep_alive(None)
            .with_allowed_hosts([host])
            .with_allowed_origins(origins)
            .with_cancellation_token(state.shutdown.child_token()),
    )
    .await
}

/// Two bounded envelopes, independent of the host's authenticated upload pool.
/// The permit moves out of every service clone together at tool dispatch/admission.
pub(super) async fn serve_request(
    mut connector: ConnectorMcp,
    request: Request,
    config: StreamableHttpServerConfig,
) -> Response {
    let Ok(permit) = connector.envelopes.clone().try_acquire_owned() else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(serde_json::json!({"error":{"code":"mcp_envelope_budget_exhausted"}})),
        )
            .into_response();
    };
    let cancellation = config.cancellation_token.child_token();
    let _disconnect = cancellation.clone().drop_guard();
    connector.request_admission = Some(Arc::new(RequestAdmission {
        envelope: parking_lot::Mutex::new(Some(permit)),
        cancellation,
    }));
    let service = StreamableHttpService::new(
        move || Ok(connector.clone()),
        Arc::<LocalSessionManager>::default(),
        config.with_max_request_body_bytes(crate::http_api::MAX_BASE64_UPLOAD_BODY_BYTES),
    );
    service.handle(request).await.map(Body::new)
}

pub(super) struct RequestAdmission {
    envelope: parking_lot::Mutex<Option<tokio::sync::OwnedSemaphorePermit>>,
    pub(super) cancellation: tokio_util::sync::CancellationToken,
}
impl RequestAdmission {
    pub(super) fn release_envelope(&self) {
        self.envelope.lock().take();
    }
}

#[cfg(test)]
mod tests;
