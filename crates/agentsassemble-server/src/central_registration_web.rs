use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    AppState,
    central_host_identity::{HostIdentityError, HostRegistrationEnvelope},
    http_api::{BodyDecodeError, consume_central_registration, decode_json_body, exact_tauri_cors},
};

const MAX_REGISTRATION_BODY_BYTES: usize = 4 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistrationRequest {
    owner_person_id: String,
}

pub(crate) fn routes() -> Router<AppState> {
    registration_routes().layer(exact_tauri_cors([Method::POST]))
}

registered_routes! {
    fn registration_routes<AppState>() {
        private "/api/central-directory/registration-proof" => post(issue_registration_proof),
    }
}

async fn issue_registration_proof(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<HostRegistrationEnvelope>, RegistrationHttpError> {
    if !consume_central_registration(&state, request.headers()).await {
        return Err(RegistrationHttpError::local_operator_required());
    }
    let payload: RegistrationRequest = decode_json_body(request, MAX_REGISTRATION_BODY_BYTES)
        .await
        .map_err(RegistrationHttpError::from_body)?;
    let owner_person_id = payload.owner_person_id.trim();
    if !valid_owner_person_id(owner_person_id) {
        return Err(RegistrationHttpError::bad_request(
            "owner_person_id is invalid",
        ));
    }
    state
        .central_host_identity
        .registration_envelope(owner_person_id)
        .map(Json)
        .map_err(|error| RegistrationHttpError::from_identity(&error))
}

fn valid_owner_person_id(value: &str) -> bool {
    (8..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

#[derive(Debug)]
struct RegistrationHttpError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl RegistrationHttpError {
    const fn local_operator_required() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "local_operator_required",
            message: "server registration proof is available only to the local operator",
        }
    }

    const fn bad_request(message: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message,
        }
    }

    const fn from_body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self {
                status: StatusCode::REQUEST_TIMEOUT,
                code: "request_timeout",
                message: "Request body timed out.",
            },
            BodyDecodeError::PayloadTooLarge => Self {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: "payload_too_large",
                message: "Request body exceeds the route limit.",
            },
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => {
                Self::bad_request("Request JSON is invalid.")
            }
        }
    }

    fn from_identity(error: &HostIdentityError) -> Self {
        tracing::error!(error = ?error, "central host registration proof failed");
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "host_identity_unavailable",
            message: "Host identity is unavailable.",
        }
    }
}

impl IntoResponse for RegistrationHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": self.message, "code": self.code})),
        )
            .into_response()
    }
}
