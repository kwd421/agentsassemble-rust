use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::num::NonZeroU64;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, consume_local_operator, ensure_empty_body,
        exact_tauri_cors,
    },
    public_ingress::{PublicIngressControlError, PublicIngressStatus},
};

const MAX_CONTROL_BODY_BYTES: usize = 4 * 1024;

registered_routes! {
    fn control_routes<AppState>() {
        private "/api/public-invite/status" => get(status),
        private "/api/public-invite/tunnel/start" => post(start),
        private "/api/public-invite/tunnel/stop" => post(stop),
    }
}

pub(crate) fn routes() -> Router<AppState> {
    control_routes()
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::GET, Method::POST]))
}

async fn status(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<PublicIngressStatus>, ControlApiError> {
    authorize_empty(&state, request).await?;
    Ok(Json(state.public_ingress.status()))
}

async fn start(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<PublicIngressStatus>, ControlApiError> {
    let issue_sequence = authorize_empty(&state, request).await?;
    state
        .public_ingress
        .start(issue_sequence)
        .await
        .map(Json)
        .map_err(ControlApiError::from)
}

async fn stop(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<PublicIngressStatus>, ControlApiError> {
    let issue_sequence = authorize_empty(&state, request).await?;
    state
        .public_ingress
        .stop(issue_sequence)
        .await
        .map(Json)
        .map_err(ControlApiError::from)
}

async fn authorize_empty(
    state: &AppState,
    request: Request,
) -> Result<NonZeroU64, ControlApiError> {
    let issue_sequence = consume_local_operator(state, request.headers())
        .await
        .ok_or_else(ControlApiError::unauthorized)?;
    ensure_empty_body(request, MAX_CONTROL_BODY_BYTES)
        .await
        .map_err(ControlApiError::from)?;
    Ok(issue_sequence)
}

struct ControlApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl ControlApiError {
    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "A valid one-use server-operator ticket is required.",
        }
    }
}

impl From<BodyDecodeError> for ControlApiError {
    fn from(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self {
                status: StatusCode::REQUEST_TIMEOUT,
                code: "request_timeout",
                message: "Ingress control body timed out.",
            },
            BodyDecodeError::PayloadTooLarge => Self {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: "payload_too_large",
                message: "Ingress control body exceeds the route limit.",
            },
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => Self {
                status: StatusCode::BAD_REQUEST,
                code: "bad_request",
                message: "Ingress control requests must not contain a body.",
            },
        }
    }
}

impl From<PublicIngressControlError> for ControlApiError {
    fn from(error: PublicIngressControlError) -> Self {
        let (code, message) = match error {
            PublicIngressControlError::Unconfigured => (
                "ingress_unconfigured",
                "Managed public ingress is not configured.",
            ),
            PublicIngressControlError::CleanupFailed => (
                "ingress_cleanup_failed",
                "Managed public ingress cleanup failed.",
            ),
            PublicIngressControlError::Closed => {
                ("ingress_closed", "Managed public ingress is shutting down.")
            }
        };
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code,
            message,
        }
    }
}

impl IntoResponse for ControlApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": {"code": self.code, "message": self.message}})),
        )
            .into_response()
    }
}
