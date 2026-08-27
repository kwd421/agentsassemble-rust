use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};

use crate::{
    AppState,
    central_host_identity::{ServerChallengeEnvelope, ServerChallengeError, ServerInfoEnvelope},
    http_api::{BodyDecodeError, decode_json_body},
    ingress_trust::TrustedIdentityOrigin,
};

const MAX_CHALLENGE_BODY_BYTES: usize = 4 * 1024;

#[derive(Deserialize)]
struct ChallengeRequest {
    challenge: String,
}

pub(crate) fn routes() -> Router<AppState> {
    identity_routes().layer(identity_cors())
}

registered_routes! {
    fn identity_routes<AppState>() {
        identity_probe_public "/api/server-info" => get(server_info),
        identity_probe_public "/api/server-info/challenge" => post(issue_challenge),
    }
}

fn identity_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::CONTENT_TYPE])
        .max_age(std::time::Duration::from_mins(10))
}

async fn server_info(State(state): State<AppState>) -> Json<ServerInfoEnvelope> {
    Json(state.central_host_identity.server_info())
}

async fn issue_challenge(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ServerChallengeEnvelope>, IdentityHttpError> {
    let origin = request_origin(&request)?;
    let payload: ChallengeRequest = decode_json_body(request, MAX_CHALLENGE_BODY_BYTES)
        .await
        .map_err(IdentityHttpError::from_body)?;
    state
        .central_host_identity
        .challenge_envelope(&payload.challenge, &origin)
        .map(Json)
        .map_err(|error| IdentityHttpError::from_challenge(&error))
}

fn request_origin(request: &Request) -> Result<String, IdentityHttpError> {
    let origin = request
        .extensions()
        .get::<TrustedIdentityOrigin>()
        .ok_or_else(IdentityHttpError::invalid_origin)?;
    Ok(origin.as_str().to_owned())
}

#[derive(Debug)]
struct IdentityHttpError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl IdentityHttpError {
    const fn bad_request(message: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message,
        }
    }

    const fn invalid_origin() -> Self {
        Self::bad_request("server origin is invalid")
    }

    const fn from_challenge(error: &ServerChallengeError) -> Self {
        match error {
            ServerChallengeError::InvalidChallenge => {
                Self::bad_request("challenge must be 22-128 base64url characters")
            }
            ServerChallengeError::InvalidOrigin => Self::invalid_origin(),
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
}

impl IntoResponse for IdentityHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": self.message, "code": self.code})),
        )
            .into_response()
    }
}
