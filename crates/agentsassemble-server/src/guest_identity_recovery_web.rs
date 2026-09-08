use agentsassemble_persistence::{
    AccountAuthority, GUEST_RECOVERY_CODE_PREFIX, GuestRecoveryRequest, GuestRecoveryResult,
    PersistenceError,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, DEVICE_CREDENTIAL_HEADER, PRIVATE_NO_STORE, bearer_credential,
        decode_json_body, exact_tauri_cors,
    },
    human_browser_credential::fingerprint_browser_credential,
    human_session_http_authority::{
        HumanSessionBearerError, HumanSessionBearerResolution, resolve_human_session_bearer,
    },
    ingress_trust::{PeerAddr, TrustedIngressOrigin, single_header},
    operator_pairing_web::{device_fingerprint, fingerprint_token},
};

const MAX_BODY: usize = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueRequest {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RedeemRequest {
    recovery_code: String,
    room_id: String,
    device_token: String,
    client_id: String,
}

#[derive(Serialize)]
struct RecoveryResponse<'a> {
    status: &'static str,
    #[serde(flatten)]
    result: GuestRecoveryResult,
    session_token: String,
    recovery_code: String,
    server_id: String,
    authority_lineage_id: String,
    server_product_surface: &'a agentsassemble_protocol::ServerProductSurface,
}

registered_routes! {
    fn recovery_routes<AppState>() {
        same_origin_public "/api/identity/recovery-code" => post(issue),
        same_origin_public "/api/identity/recovery-code/redeem" => post(redeem),
    }
}

pub(crate) fn routes() -> Router<AppState> {
    recovery_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::POST]).allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            DEVICE_CREDENTIAL_HEADER,
        ]))
}

async fn issue(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, RecoveryHttpError> {
    let origin = recovery_origin(&state, &request)?;
    let device =
        device_fingerprint(request.headers()).ok_or_else(RecoveryHttpError::unauthorized)?;
    single_header(request.headers(), header::AUTHORIZATION)
        .ok_or_else(RecoveryHttpError::unauthorized)?;
    let bearer =
        bearer_credential(request.headers()).ok_or_else(RecoveryHttpError::unauthorized)?;
    let session = match resolve_human_session_bearer(&state, bearer).await {
        Ok(HumanSessionBearerResolution::Authorized(session)) => session,
        Err(HumanSessionBearerError::Persistence(error)) => return Err(error.into()),
        _ => return Err(RecoveryHttpError::unauthorized()),
    };
    let room_id = session.principal().room_id.clone();
    let _: IssueRequest = decode_json_body(request, MAX_BODY).await?;
    let identity = state
        .store
        .account_identity(AccountAuthority::HumanSession {
            authorization: session,
            browser_fingerprint: device,
        })
        .await?;
    let bootstrap = state.store.local_bootstrap_status().await?;
    let mut url = url::Url::parse(&origin).map_err(|_| RecoveryHttpError::unavailable())?;
    url.set_path("/recover");
    url.query_pairs_mut()
        .append_pair("recover", "1")
        .append_pair("room", &room_id);
    let code = state.store.issue_guest_recovery_code(&identity).await?;
    url.set_fragment(Some(&format!("recovery={code}")));
    Ok(Json(json!({
        "status": "issued", "server_id": bootstrap.server_id, "room_id": room_id,
        "recovery_code": code, "recovery_url": url.as_str(),
    })))
}

async fn redeem(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, RecoveryHttpError> {
    recovery_origin(&state, &request)?;
    let network = request
        .extensions()
        .get::<PeerAddr>()
        .ok_or_else(RecoveryHttpError::unauthorized)?
        .0
        .ip();
    let body: RedeemRequest = decode_json_body(request, MAX_BODY).await?;
    let fingerprint = fingerprint_token(body.recovery_code.trim(), GUEST_RECOVERY_CODE_PREFIX);
    let attempt_key =
        fingerprint.unwrap_or_else(|| Sha256::digest(body.recovery_code.trim().as_bytes()).into());
    if !state.recovery_attempts.allows(network, attempt_key) {
        return Err(RecoveryHttpError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "recovery_rate_limited",
            "Too many recovery attempts. Try again later.",
        ));
    }
    let device = fingerprint_browser_credential(&body.device_token)
        .ok_or_else(RecoveryHttpError::unauthorized)?;
    let fingerprint = fingerprint.ok_or_else(RecoveryHttpError::unauthorized)?;
    let bootstrap = state.store.local_bootstrap_status().await?;
    let commit = state
        .rooms
        .recover_guest_identity(&GuestRecoveryRequest {
            fingerprint: &fingerprint,
            device: &device,
            room_id: &body.room_id,
            client_id: &body.client_id,
        })
        .await?;
    serde_json::to_value(RecoveryResponse {
        status: "recovered",
        result: commit.result,
        session_token: commit.session_bearer,
        recovery_code: commit.recovery_code,
        server_id: bootstrap.server_id,
        authority_lineage_id: bootstrap.authority_lineage_id,
        server_product_surface: state.server_product_surface.as_ref(),
    })
    .map(Json)
    .map_err(|_| RecoveryHttpError::unavailable())
}

fn recovery_origin(state: &AppState, request: &Request) -> Result<String, RecoveryHttpError> {
    let ready = state.public_ingress.ready_snapshot().ok_or_else(|| {
        RecoveryHttpError::new(
            StatusCode::CONFLICT,
            "public_ingress_not_ready",
            "Public HTTPS ingress is not ready.",
        )
    })?;
    // The common ingress middleware has already verified transport, Origin and the
    // public route policy. Never infer trusted HTTPS from a caller-supplied header.
    if request
        .extensions()
        .get::<TrustedIngressOrigin>()
        .is_none_or(|origin| origin.as_str() != ready.public_url)
    {
        return Err(RecoveryHttpError::unauthorized());
    }
    Ok(ready.public_url)
}

struct RecoveryHttpError {
    status: StatusCode,
    code: std::borrow::Cow<'static, str>,
    message: String,
}

impl RecoveryHttpError {
    fn new(
        status: StatusCode,
        code: impl Into<std::borrow::Cow<'static, str>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }
    fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "recovery_authority_required",
            "Valid recovery authority and browser device are required.",
        )
    }
    fn unavailable() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "recovery_unavailable",
            "Identity recovery is unavailable.",
        )
    }
}
impl From<BodyDecodeError> for RecoveryHttpError {
    fn from(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self::new(
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout",
                "Request body timed out.",
            ),
            BodyDecodeError::PayloadTooLarge => Self::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "Recovery request exceeds its limit.",
            ),
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => Self::new(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "Recovery request body is invalid.",
            ),
        }
    }
}
impl From<PersistenceError> for RecoveryHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::CommandRejected { code, message } => {
                let status = match code.as_bytes() {
                    b"account_device_mismatch" => StatusCode::CONFLICT,
                    b"recovery_capacity_reached" => StatusCode::TOO_MANY_REQUESTS,
                    b"invalid_state" | b"recovery_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
                    b"recovery_request_invalid" => StatusCode::BAD_REQUEST,
                    _ => StatusCode::FORBIDDEN,
                };
                Self::new(status, code, message)
            }
            _ => Self::unavailable(),
        }
    }
}
impl IntoResponse for RecoveryHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": self.message, "code": self.code})),
        )
            .into_response()
    }
}
