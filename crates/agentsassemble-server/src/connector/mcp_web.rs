use futures_util::StreamExt;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::OwnedSemaphorePermit,
    time::{Instant, timeout_at},
};

const ENVELOPE_BYTES: usize = 256 * 1024 * 1024;
pub(super) const SMALL_ENVELOPE_BYTES: usize = 128 * 64 * 1024 * 4;
const LARGE_ENVELOPE_BYTES: usize = ENVELOPE_BYTES - SMALL_ENVELOPE_BYTES;
const ENVELOPE_CHUNK: usize = 64 * 1024;
const ENVELOPE_COPIES: usize = 4;
const PREPARSE_IDLE: Duration = Duration::from_secs(2);
const PREPARSE_TOTAL: Duration = Duration::from_secs(10);

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

#[derive(Clone)]
pub(super) struct EnvelopeBudget {
    small: Arc<tokio::sync::Semaphore>,
    large: Arc<tokio::sync::Semaphore>,
}
impl Default for EnvelopeBudget {
    fn default() -> Self {
        Self {
            small: Arc::new(tokio::sync::Semaphore::new(SMALL_ENVELOPE_BYTES)),
            large: Arc::new(tokio::sync::Semaphore::new(LARGE_ENVELOPE_BYTES)),
        }
    }
}
struct EnvelopeLease {
    _small: OwnedSemaphorePermit,
    large: Option<OwnedSemaphorePermit>,
}

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

/// Byte-weighted envelopes, independent of the authenticated upload pool.
/// The permit moves out of every service clone together at tool dispatch/admission.
pub(super) async fn serve_request(
    mut connector: ConnectorMcp,
    request: Request,
    config: StreamableHttpServerConfig,
) -> Response {
    let (request, permit) = match collect_envelope(&connector.envelopes, request).await {
        Ok(admitted) => admitted,
        Err(response) => return response,
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

// The endpoint is stateless: no authenticated MCP session/bearer is bound to the
// incoming request. A connector's private handle arrives inside tool arguments.
// Host/Origin/proxy identity proves ingress, not connector identity. Charge actual
// collected bytes (rounded to 64 KiB). Protect the first 64 KiB for 128 requests,
// comparable to the 127 public connections. Large envelopes share only the remaining
// 224 MiB, so filling that pool cannot deny an unrelated small tool request.
async fn collect_envelope(
    budget: &EnvelopeBudget,
    request: Request,
) -> Result<(Request, EnvelopeLease), Response> {
    let small = budget
        .small
        .clone()
        .try_acquire_many_owned((ENVELOPE_CHUNK * ENVELOPE_COPIES) as u32)
        .map_err(|_| {
            envelope_error(
                StatusCode::TOO_MANY_REQUESTS,
                "mcp_envelope_budget_exhausted",
            )
        })?;
    let mut permit = EnvelopeLease {
        _small: small,
        large: None,
    };
    let (parts, body) = request.into_parts();
    let mut stream = body.into_data_stream();
    let mut content = Vec::new();
    let started = Instant::now();
    let mut last_data = started;
    loop {
        let deadline = (started + PREPARSE_TOTAL).min(last_data + PREPARSE_IDLE);
        if Instant::now() >= deadline {
            return Err(envelope_error(
                StatusCode::REQUEST_TIMEOUT,
                "mcp_envelope_timeout",
            ));
        }
        let next = timeout_at(deadline, stream.next())
            .await
            .map_err(|_| envelope_error(StatusCode::REQUEST_TIMEOUT, "mcp_envelope_timeout"))?;
        let Some(bytes) = next else {
            break;
        };
        let bytes =
            bytes.map_err(|_| envelope_error(StatusCode::BAD_REQUEST, "invalid_mcp_envelope"))?;
        if bytes.is_empty() {
            continue;
        }
        let length = content.len().saturating_add(bytes.len());
        if length > crate::http_api::MAX_BASE64_UPLOAD_BODY_BYTES {
            return Err(envelope_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "mcp_envelope_too_large",
            ));
        }
        let weight = (length.div_ceil(ENVELOPE_CHUNK) - 1) * ENVELOPE_CHUNK * ENVELOPE_COPIES;
        let held = permit
            .large
            .as_ref()
            .map_or(0, OwnedSemaphorePermit::num_permits);
        if weight > held {
            let extra = budget
                .large
                .clone()
                .try_acquire_many_owned((weight - held) as u32)
                .map_err(|_| {
                    envelope_error(
                        StatusCode::TOO_MANY_REQUESTS,
                        "mcp_envelope_budget_exhausted",
                    )
                })?;
            if let Some(large) = &mut permit.large {
                large.merge(extra);
            } else {
                permit.large = Some(extra);
            }
        }
        content.extend_from_slice(&bytes);
        last_data = Instant::now();
    }
    Ok((Request::from_parts(parts, Body::from(content)), permit))
}

fn envelope_error(status: StatusCode, code: &'static str) -> Response {
    (
        status,
        axum::Json(serde_json::json!({"error":{"code":code}})),
    )
        .into_response()
}

pub(super) struct RequestAdmission {
    envelope: parking_lot::Mutex<Option<EnvelopeLease>>,
    pub(super) cancellation: tokio_util::sync::CancellationToken,
}
impl RequestAdmission {
    pub(super) fn release_envelope(&self) {
        self.envelope.lock().take();
    }
}

#[cfg(test)]
mod tests;
