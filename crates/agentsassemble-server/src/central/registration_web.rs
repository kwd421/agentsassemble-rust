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
    central::host_identity::HostIdentityError,
    http_api::{BodyDecodeError, consume_central_registration, decode_json_body, exact_tauri_cors},
};

#[path = "registration_after_deletion.rs"]
mod after_deletion;

const MAX_REGISTRATION_BODY_BYTES: usize = 4 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProofRequest {
    owner_person_id: String,
    #[serde(default)]
    claim_ownership: bool,
    #[serde(default)]
    new_account_registration: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EpochRequest {
    server_id: String,
    registration_epoch: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostingRequest {
    server_id: String,
    hosting_state: String,
    registration_epoch: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RegistrationRequest {
    AccountStop(after_deletion::AccountStopRequest),
    FreshEpoch(after_deletion::FreshEpochRequest),
    Hosting(HostingRequest),
    Proof(ProofRequest),
    Epoch(EpochRequest),
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
) -> Result<Json<serde_json::Value>, RegistrationHttpError> {
    if !consume_central_registration(&state, request.headers()).await {
        return Err(RegistrationHttpError::local_operator_required());
    }
    let payload: RegistrationRequest = decode_json_body(request, MAX_REGISTRATION_BODY_BYTES)
        .await
        .map_err(RegistrationHttpError::from_body)?;
    let payload = match payload {
        RegistrationRequest::AccountStop(request) => {
            return after_deletion::stop_account_host(&state, request).await;
        }
        RegistrationRequest::FreshEpoch(epoch) => {
            return after_deletion::commit_epoch(&state, epoch).await;
        }
        RegistrationRequest::Hosting(HostingRequest {
            server_id,
            hosting_state,
            registration_epoch,
        }) => {
            if server_id != state.central_host_identity.server_id() {
                return Err(RegistrationHttpError::bad_request(
                    "host binding is invalid",
                ));
            }
            if hosting_state != "status" {
                if hosting_state != "retired"
                    || registration_epoch.as_ref().is_some_and(String::is_empty)
                {
                    return Err(RegistrationHttpError::bad_request(
                        "hosting state is invalid",
                    ));
                }
                if !state
                    .store
                    .retire_hosting_incarnation(registration_epoch.as_deref())
                    .await
                    .map_err(|_| RegistrationHttpError::persistence())?
                {
                    return Err(RegistrationHttpError {
                        status: StatusCode::CONFLICT,
                        code: "incarnation_conflict",
                        message: "Stored incarnation has changed.",
                    });
                }
                state
                    .public_ingress()
                    .demote()
                    .await
                    .map_err(|_| RegistrationHttpError::persistence())?;
            }
            let restriction = state
                .store
                .hosting_restriction()
                .await
                .map_err(|_| RegistrationHttpError::persistence())?;
            return Ok(Json(if restriction.as_deref() == Some("account_deleted") {
                json!({"hosting_state":restriction,"registration_epoch":state.store.registration_epoch().await.map_err(|_|RegistrationHttpError::persistence())?})
            } else {
                json!({"hosting_state":restriction})
            }));
        }
        RegistrationRequest::Epoch(epoch) => {
            if epoch.server_id != state.central_host_identity.server_id()
                || epoch
                    .registration_epoch
                    .as_ref()
                    .is_some_and(String::is_empty)
            {
                return Err(RegistrationHttpError::bad_request(
                    "registration epoch binding is invalid",
                ));
            }
            state
                .store
                .set_registration_epoch(epoch.registration_epoch.as_deref())
                .await
                .map_err(|_| RegistrationHttpError::persistence())?;
            return Ok(Json(json!({"status": "ok"})));
        }
        RegistrationRequest::Proof(proof) => proof,
    };
    registration_envelope(&state, payload).await
}

async fn registration_envelope(
    state: &AppState,
    payload: ProofRequest,
) -> Result<Json<serde_json::Value>, RegistrationHttpError> {
    let owner_person_id = payload.owner_person_id.trim();
    if !valid_owner_person_id(owner_person_id) {
        return Err(RegistrationHttpError::bad_request(
            "owner_person_id is invalid",
        ));
    }
    if !payload.new_account_registration
        && let Some(reason) = state
            .store
            .hosting_restriction()
            .await
            .map_err(|_| RegistrationHttpError::persistence())?
    {
        return Err(RegistrationHttpError {
            status: StatusCode::CONFLICT,
            code: if reason == "retired" {
                "server_retired"
            } else {
                "server_exists"
            },
            message: "This computer can only connect as a device.",
        });
    }
    let epoch = if payload.new_account_registration {
        if payload.claim_ownership {
            return Err(RegistrationHttpError::bad_request(
                "new registration is not an ownership claim",
            ));
        }
        state
            .store
            .require_fresh_host_registration(
                &state
                    .central_directory
                    .issuer()
                    .map_err(|_| RegistrationHttpError::persistence())?,
                owner_person_id,
            )
            .await
            .map_err(|_| RegistrationHttpError::persistence())?;
        None
    } else {
        state
            .store
            .registration_epoch()
            .await
            .map_err(|_| RegistrationHttpError::persistence())?
    };
    let profile = state
        .store
        .local_operator_profile()
        .await
        .map_err(|_| RegistrationHttpError::persistence())?;
    state
        .central_host_identity
        .registration_envelope(
            owner_person_id,
            payload.claim_ownership,
            epoch.as_deref(),
            &profile,
        )
        .await
        .and_then(|envelope| serde_json::to_value(envelope).map_err(HostIdentityError::Json))
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

    const fn persistence() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "registration_epoch_unavailable",
            message: "Stored registration epoch is unavailable.",
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
