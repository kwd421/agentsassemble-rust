use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::{
    AppState,
    http_api::{BodyDecodeError, consume_local_operator, ensure_empty_body},
    public_ingress::PublicIngressStatus,
};

const MAX_STATUS_BODY_BYTES: usize = 4 * 1024;

registered_routes! {
    pub(crate) fn routes<AppState>() {
        private "/api/public-invite/status" => get(status),
    }
}

async fn status(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<PublicIngressStatus>, StatusApiError> {
    if !consume_local_operator(&state, request.headers()).await {
        return Err(StatusApiError::unauthorized());
    }
    ensure_empty_body(request, MAX_STATUS_BODY_BYTES)
        .await
        .map_err(StatusApiError::from)?;
    Ok(Json(state.public_ingress.status()))
}

struct StatusApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl StatusApiError {
    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "A valid one-use server-operator ticket is required.",
        }
    }
}

impl From<BodyDecodeError> for StatusApiError {
    fn from(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self {
                status: StatusCode::REQUEST_TIMEOUT,
                code: "request_timeout",
                message: "Ingress status body timed out.",
            },
            BodyDecodeError::PayloadTooLarge => Self {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: "payload_too_large",
                message: "Ingress status body exceeds the route limit.",
            },
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => Self {
                status: StatusCode::BAD_REQUEST,
                code: "bad_request",
                message: "Ingress status requests must not contain a body.",
            },
        }
    }
}

impl IntoResponse for StatusApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": {"code": self.code, "message": self.message}})),
        )
            .into_response()
    }
}
